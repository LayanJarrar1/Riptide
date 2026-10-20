//! Server-sent-events plumbing for [`crate::Provider::complete_stream`]: an incremental SSE
//! parser that copes with any chunk boundary, and the decoders that turn each provider's
//! events into text deltas.
//!
//! Dropping the future of a streaming call closes the HTTP connection, which is how callers
//! cancel. Retries happen only before the first byte of the body: once text has been handed
//! to the caller the call is never silently restarted (that would duplicate output).

use serde_json::Value;

use crate::error::{AiError, Result};
use crate::types::Usage;

/// Most bytes one SSE line or one event's data may take.
const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// One server-sent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, if any.
    pub event: Option<String>,
    /// The `data:` lines joined with `\n`.
    pub data: String,
}

/// Incremental SSE parser. Feed it raw body chunks as they arrive; chunk boundaries may fall
/// anywhere, including inside a line terminator or a multi-byte character.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    data_bytes: usize,
}

impl SseParser {
    /// A parser with no pending input.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a chunk and returns the events it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < self.buf.len() {
            let b = self.buf[i];
            let (line_end, next) = match b {
                b'\n' => (i, i + 1),
                b'\r' => {
                    // \r\n is one terminator; a lone \r at the end of the buffer might be the
                    // first half of one, so wait for the next byte
                    match self.buf.get(i + 1) {
                        Some(b'\n') => (i, i + 2),
                        Some(_) => (i, i + 1),
                        None => break,
                    }
                }
                _ => {
                    i += 1;
                    continue;
                }
            };
            let line = String::from_utf8_lossy(&self.buf[start..line_end]).into_owned();
            self.line(&line, &mut out)?;
            start = next;
            i = next;
        }
        self.buf.drain(..start);
        if self.buf.len() > MAX_EVENT_BYTES {
            return Err(AiError::InvalidResponse("stream line too long".into()));
        }
        Ok(out)
    }

    /// Ends the stream: a final event that was not closed by a blank line is still delivered.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let line = String::from_utf8_lossy(&std::mem::take(&mut self.buf)).into_owned();
            let _ = self.line(line.trim_end_matches(['\r', '\n']), &mut out);
        }
        self.dispatch(&mut out);
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<SseEvent>) -> Result<()> {
        if line.is_empty() {
            self.dispatch(out);
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(()); // comment / keep-alive
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => {
                self.data_bytes += value.len() + 1;
                if self.data_bytes > MAX_EVENT_BYTES {
                    return Err(AiError::InvalidResponse("stream event too large".into()));
                }
                self.data.push(value.to_string());
            }
            _ => {} // id, retry and unknown fields do not matter here
        }
        Ok(())
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        let event = self.event.take();
        let data = std::mem::take(&mut self.data);
        self.data_bytes = 0;
        if data.is_empty() && event.is_none() {
            return;
        }
        out.push(SseEvent {
            event,
            data: data.join("\n"),
        });
    }
}

/// What the stream decoder wants the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Keep reading.
    Continue,
    /// The model finished; stop reading.
    Done,
}

/// What a stream produced so far.
#[derive(Debug, Default)]
pub(crate) struct Acc {
    pub text: String,
    pub model: Option<String>,
    pub finish_reason: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// An explicit end marker was seen (`[DONE]`, `message_stop`, a finish reason).
    pub ended: bool,
}

impl Acc {
    pub(crate) fn usage(&self, prompt_chars: usize) -> Usage {
        match (self.prompt_tokens, self.completion_tokens) {
            (Some(p), Some(c)) => Usage {
                prompt_tokens: p,
                completion_tokens: c,
                total_tokens: self.total_tokens.unwrap_or(p + c),
                estimated: false,
            },
            _ => Usage::estimate(prompt_chars, self.text.len()),
        }
    }
}

fn u64_at(v: &Value, ptr: &str) -> Option<u64> {
    v.pointer(ptr).and_then(Value::as_u64)
}

/// Maps an error object found inside a 200 stream to a typed error.
fn in_stream_error(v: &Value, scrub: &dyn Fn(&str) -> String) -> AiError {
    let kind = v
        .pointer("/error/type")
        .or_else(|| v.pointer("/error/code"))
        .and_then(|t| {
            t.as_str()
                .map(str::to_string)
                .or_else(|| Some(t.to_string()))
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let msg = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("the provider reported an error");
    let msg: String = scrub(msg).chars().take(200).collect();
    if kind.contains("rate") || kind.contains("429") {
        AiError::RateLimited { retry_after: None }
    } else if kind.contains("overloaded") || kind.contains("unavailable") || kind.contains("server")
    {
        AiError::ProviderDown { status: 503 }
    } else if kind.contains("auth") || kind.contains("permission") || kind.contains("401") {
        AiError::Unauthorized
    } else {
        AiError::BadRequest { message: msg }
    }
}

/// Decodes one OpenAI-compatible chunk (`data:` payload).
pub(crate) fn openai_event(
    ev: &SseEvent,
    acc: &mut Acc,
    on_delta: &mut (dyn FnMut(String) + Send),
    scrub: &dyn Fn(&str) -> String,
) -> Result<Flow> {
    let data = ev.data.trim();
    if data == "[DONE]" {
        acc.ended = true;
        return Ok(Flow::Done);
    }
    if data.is_empty() {
        return Ok(Flow::Continue);
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|_| AiError::InvalidResponse("a stream event is not valid JSON".into()))?;
    if v.get("error").is_some_and(|e| !e.is_null()) {
        return Err(in_stream_error(&v, scrub));
    }
    if let Some(m) = v.get("model").and_then(Value::as_str) {
        acc.model.get_or_insert_with(|| m.to_string());
    }
    if let Some(choice) = v.pointer("/choices/0") {
        let piece = match choice.pointer("/delta/content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect(),
            _ => String::new(),
        };
        if !piece.is_empty() {
            acc.text.push_str(&piece);
            on_delta(piece.clone());
        }
        if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
            acc.finish_reason = Some(r.to_string());
            acc.ended = true;
        }
    }
    if let (Some(p), Some(c)) = (
        u64_at(&v, "/usage/prompt_tokens"),
        u64_at(&v, "/usage/completion_tokens"),
    ) {
        acc.prompt_tokens = Some(p);
        acc.completion_tokens = Some(c);
        acc.total_tokens = u64_at(&v, "/usage/total_tokens");
    }
    Ok(Flow::Continue)
}

/// Decodes one Anthropic Messages stream event.
pub(crate) fn anthropic_event(
    ev: &SseEvent,
    acc: &mut Acc,
    on_delta: &mut (dyn FnMut(String) + Send),
    scrub: &dyn Fn(&str) -> String,
) -> Result<Flow> {
    let data = ev.data.trim();
    if data.is_empty() {
        return Ok(Flow::Continue);
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|_| AiError::InvalidResponse("a stream event is not valid JSON".into()))?;
    let kind = v
        .get("type")
        .and_then(Value::as_str)
        .or(ev.event.as_deref())
        .unwrap_or("");
    match kind {
        "message_start" => {
            if let Some(m) = v.pointer("/message/model").and_then(Value::as_str) {
                acc.model = Some(m.to_string());
            }
            acc.prompt_tokens = u64_at(&v, "/message/usage/input_tokens");
        }
        "content_block_delta" => {
            if v.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") {
                if let Some(t) = v.pointer("/delta/text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        acc.text.push_str(t);
                        on_delta(t.to_string());
                    }
                }
            }
        }
        "message_delta" => {
            if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                acc.finish_reason = Some(r.to_string());
            }
            if let Some(c) = u64_at(&v, "/usage/output_tokens") {
                acc.completion_tokens = Some(c);
            }
        }
        "message_stop" => {
            acc.ended = true;
            return Ok(Flow::Done);
        }
        "error" => return Err(in_stream_error(&v, scrub)),
        _ => {} // ping, content_block_start/stop, unknown
    }
    Ok(Flow::Continue)
}

/// Decodes one Gemini `streamGenerateContent?alt=sse` chunk.
pub(crate) fn gemini_event(
    ev: &SseEvent,
    acc: &mut Acc,
    on_delta: &mut (dyn FnMut(String) + Send),
    scrub: &dyn Fn(&str) -> String,
) -> Result<Flow> {
    let data = ev.data.trim();
    if data.is_empty() {
        return Ok(Flow::Continue);
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|_| AiError::InvalidResponse("a stream event is not valid JSON".into()))?;
    if v.get("error").is_some_and(|e| !e.is_null()) {
        return Err(in_stream_error(&v, scrub));
    }
    if let Some(m) = v.get("modelVersion").and_then(Value::as_str) {
        acc.model = Some(m.to_string());
    }
    if let Some(parts) = v
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
    {
        for p in parts {
            if p.get("thought").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                if !t.is_empty() {
                    acc.text.push_str(t);
                    on_delta(t.to_string());
                }
            }
        }
    }
    if let Some(r) = v
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str)
    {
        acc.finish_reason = Some(r.to_string());
        acc.ended = true;
    }
    if let (Some(p), Some(t)) = (
        u64_at(&v, "/usageMetadata/promptTokenCount"),
        u64_at(&v, "/usageMetadata/totalTokenCount"),
    ) {
        acc.prompt_tokens = Some(p);
        acc.completion_tokens =
            Some(u64_at(&v, "/usageMetadata/candidatesTokenCount").unwrap_or(t.saturating_sub(p)));
        acc.total_tokens = Some(t);
    }
    Ok(Flow::Continue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut all = Vec::new();
        for c in chunks {
            all.extend(p.push(c).unwrap());
        }
        all.extend(p.finish());
        all
    }

    #[test]
    fn events_survive_any_chunk_boundary() {
        let whole =
            "event: a\ndata: {\"x\":1}\n\n: keepalive\ndata: l1\ndata: l2\n\ndata: [DONE]\n\n";
        let expect = collect(&[whole.as_bytes()]);
        assert_eq!(expect.len(), 3);
        assert_eq!(expect[0].event.as_deref(), Some("a"));
        assert_eq!(expect[1].data, "l1\nl2");
        // split at every possible byte position
        for cut in 0..whole.len() {
            let (a, b) = whole.as_bytes().split_at(cut);
            assert_eq!(collect(&[a, b]), expect, "cut at {cut}");
        }
        // one byte at a time
        let bytes: Vec<&[u8]> = whole.as_bytes().chunks(1).collect();
        assert_eq!(collect(&bytes), expect);
    }

    #[test]
    fn crlf_and_split_multibyte_characters() {
        let s = "data: \u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}\r\n\r\n";
        let bytes = s.as_bytes();
        for cut in 0..bytes.len() {
            let (a, b) = bytes.split_at(cut);
            let ev = collect(&[a, b]);
            assert_eq!(ev.len(), 1, "cut {cut}");
            assert_eq!(
                ev[0].data,
                "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629}"
            );
        }
    }

    #[test]
    fn unterminated_last_event_is_delivered_on_finish() {
        let ev = collect(&[b"data: tail"]);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].data, "tail");
    }

    #[test]
    fn oversized_lines_are_refused() {
        let mut p = SseParser::new();
        let big = vec![b'a'; MAX_EVENT_BYTES + 10];
        assert!(p.push(&big).is_err());
    }

    #[test]
    fn openai_chunks_accumulate_and_error_objects_fail() {
        let scrub = |s: &str| s.to_string();
        let mut acc = Acc::default();
        let mut got = String::new();
        let mut sink = |d: String| got.push_str(&d);
        for d in [
            r#"{"model":"m","choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"{"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#,
        ] {
            let f = openai_event(
                &SseEvent {
                    event: None,
                    data: d.into(),
                },
                &mut acc,
                &mut sink,
                &scrub,
            )
            .unwrap();
            assert_eq!(f, Flow::Continue);
        }
        assert_eq!(acc.text, "Hello");
        assert!(acc.ended);
        assert_eq!(acc.usage(0).total_tokens, 7);
        let err = openai_event(
            &SseEvent {
                event: None,
                data: r#"{"error":{"message":"boom","type":"server_error"}}"#.into(),
            },
            &mut acc,
            &mut sink,
            &scrub,
        );
        assert!(matches!(err, Err(AiError::ProviderDown { .. })));
    }
}
