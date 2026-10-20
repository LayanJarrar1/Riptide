//! A tiny scripted SSE server on loopback: wiremock can only send whole bodies, and these tests
//! need chunk boundaries, delays and abrupt disconnects. One script per accepted connection.
#![allow(dead_code, missing_docs)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One step of the response body.
#[derive(Clone, Debug)]
pub enum Step {
    /// Send these bytes as one HTTP chunk.
    Chunk(Vec<u8>),
    /// Wait.
    Sleep(u64),
    /// Close the socket without the terminating chunk (a broken connection).
    Drop,
}

pub fn chunk(s: &str) -> Step {
    Step::Chunk(s.as_bytes().to_vec())
}

/// What the server does for one connection.
#[derive(Clone, Debug)]
pub struct Script {
    pub status: u16,
    pub steps: Vec<Step>,
    /// Extra raw header lines, e.g. `retry-after: 1`.
    pub headers: Vec<String>,
}

impl Script {
    pub fn ok(steps: Vec<Step>) -> Self {
        Script {
            status: 200,
            steps,
            headers: vec![],
        }
    }
    pub fn status(code: u16, body: &str) -> Self {
        Script {
            status: code,
            steps: vec![chunk(body)],
            headers: vec![],
        }
    }
}

/// What the server saw.
#[derive(Default, Debug)]
pub struct Seen {
    /// Request head + body of each connection, in order.
    pub requests: Vec<String>,
    /// Connections whose peer went away while we were still writing (cancel proof).
    pub write_failures: usize,
    /// Connections accepted.
    pub connections: usize,
}

pub struct SseServer {
    pub url: String,
    pub seen: Arc<Mutex<Seen>>,
}

/// Starts the server; connection n gets `scripts[n]` (the last script repeats).
pub async fn serve(scripts: Vec<Script>) -> SseServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let seen = Arc::new(Mutex::new(Seen::default()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        let mut n = 0usize;
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let script = scripts
                .get(n)
                .or(scripts.last())
                .cloned()
                .expect("a script");
            n += 1;
            seen2.lock().expect("lock").connections += 1;
            let seen3 = seen2.clone();
            tokio::spawn(async move {
                // read the request head and the body
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let head_end = loop {
                    let Ok(k) = sock.read(&mut tmp).await else {
                        return;
                    };
                    if k == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..k]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let want: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                while buf.len() < head_end + want {
                    let Ok(k) = sock.read(&mut tmp).await else {
                        return;
                    };
                    if k == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..k]);
                }
                seen3
                    .lock()
                    .expect("lock")
                    .requests
                    .push(String::from_utf8_lossy(&buf).to_string());
                let mut resp = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n",
                    script.status
                );
                for h in &script.headers {
                    resp.push_str(h);
                    resp.push_str("\r\n");
                }
                resp.push_str("\r\n");
                if sock.write_all(resp.as_bytes()).await.is_err() {
                    return;
                }
                for step in &script.steps {
                    match step {
                        Step::Chunk(b) => {
                            let mut framed = format!("{:x}\r\n", b.len()).into_bytes();
                            framed.extend_from_slice(b);
                            framed.extend_from_slice(b"\r\n");
                            if sock.write_all(&framed).await.is_err()
                                || sock.flush().await.is_err()
                            {
                                seen3.lock().expect("lock").write_failures += 1;
                                return;
                            }
                        }
                        Step::Sleep(ms) => tokio::time::sleep(Duration::from_millis(*ms)).await,
                        Step::Drop => return,
                    }
                }
                let _ = sock.write_all(b"0\r\n\r\n").await;
            });
        }
    });
    SseServer { url, seen }
}
