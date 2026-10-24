//! One request, done properly: manual redirects with the cross-origin rule, one authentication
//! round trip per hop, `Retry-After` parsing. Every request the engine makes goes through
//! [`send`].

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderValue, AUTHORIZATION, LOCATION, RETRY_AFTER, WWW_AUTHENTICATE};
use reqwest::{Client, Method, RequestBuilder, Response};
use url::Url;

use crate::context::RequestContext;
use crate::error::{Error, Result};
use crate::redirect::{is_redirect, next_method, resolve_location, strip_query, MAX_REDIRECTS};

/// A final (non-redirect) response and the URL it came from.
#[derive(Debug)]
pub struct Sent {
    /// The response; any status, the caller interprets it.
    pub response: Response,
    /// URL after redirects.
    pub url: Url,
    /// Method used for the final hop (`303` turns `POST` into `GET`).
    pub method: Method,
}

/// Sends `method url`, following up to [`MAX_REDIRECTS`] redirects and answering one
/// authentication challenge per hop. `configure` is applied to every hop (that is where
/// `Range` / `If-Range` go) after the context's own headers.
pub async fn send<F>(
    client: &Client,
    ctx: &RequestContext,
    method: Method,
    url: Url,
    configure: F,
) -> Result<Sent>
where
    F: Fn(RequestBuilder) -> RequestBuilder,
{
    let first_url = url.clone();
    let mut url = url;
    let mut method = method;
    for hop in 0..=MAX_REDIRECTS {
        let response = send_hop(client, ctx, &method, &url, &configure).await?;
        let status = response.status().as_u16();
        if !is_redirect(status) {
            return Ok(Sent {
                response,
                url,
                method,
            });
        }
        if hop == MAX_REDIRECTS {
            break;
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Protocol(format!("{status} without a Location header")))?;
        let next = resolve_location(&url, location)?;
        ctx.guard().check_redirect(&first_url, &next).await?;
        tracing::debug!(
            status,
            from = %strip_query(url.as_str()),
            to = %strip_query(next.as_str()),
            cross_origin = !ctx.same_origin(&next),
            "redirect"
        );
        method = next_method(status, &method);
        url = next;
    }
    Err(Error::TooManyRedirects { max: MAX_REDIRECTS })
}

/// One hop: the request with context headers, retried once with an `Authorization` answer if
/// the server challenges and we have credentials.
async fn send_hop<F>(
    client: &Client,
    ctx: &RequestContext,
    method: &Method,
    url: &Url,
    configure: &F,
) -> Result<Response>
where
    F: Fn(RequestBuilder) -> RequestBuilder,
{
    let build = || {
        configure(ctx.apply(
            client.request(method.clone(), url.clone()),
            url,
            method.as_str(),
        ))
    };
    let response = build().send().await?;
    if response.status().as_u16() != 401 || !ctx.same_origin(url) {
        return Ok(response);
    }
    let challenge = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    // prefer Digest over Basic when both are offered
    let chosen = challenge
        .iter()
        .find(|c| c.trim_start().len() >= 6 && c.trim_start()[..6].eq_ignore_ascii_case("digest"))
        .or_else(|| challenge.first());
    let Some(chosen) = chosen else {
        return Ok(response);
    };
    // a 401 while we were already sending a remembered scheme means it stopped working
    ctx.forget_auth();
    let Some(answer) = ctx.answer_challenge(chosen, url, method.as_str()) else {
        return Ok(response);
    };
    drop(response);
    let retried = build().header(AUTHORIZATION, answer).send().await?;
    Ok(retried)
}

/// Maps a non-2xx/206 status to [`Error::Status`] with its `Retry-After`.
pub fn ensure_success(response: &Response) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    if status.as_u16() == 401 || status.as_u16() == 407 {
        let scheme = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_whitespace().next())
            .unwrap_or("none")
            .to_string();
        return Err(Error::AuthFailed { scheme });
    }
    Err(Error::status(
        status.as_u16(),
        retry_after_secs(response.headers().get(RETRY_AFTER)),
    ))
}

/// `Retry-After` in seconds: the delta-seconds form as is, the HTTP-date form as the time left
/// until that date (0 if it is already past). Callers cap the wait.
pub fn retry_after_secs(value: Option<&HeaderValue>) -> Option<u64> {
    retry_after_at(value, SystemTime::now())
}

fn retry_after_at(value: Option<&HeaderValue>, now: SystemTime) -> Option<u64> {
    let v = value?.to_str().ok()?.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(secs);
    }
    let date = parse_imf_fixdate(v)?;
    let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(date.saturating_sub(now))
}

/// `Wed, 21 Oct 2026 07:28:00 GMT` (IMF-fixdate) as Unix seconds.
fn parse_imf_fixdate(v: &str) -> Option<u64> {
    let (_, rest) = v.split_once(", ")?;
    let mut parts = rest.split_whitespace();
    let day: u64 = parts.next()?.parse().ok()?;
    let month_name = parts.next()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| m.eq_ignore_ascii_case(month_name))? as u64
        + 1;
    let year: u64 = parts.next()?.parse().ok()?;
    let mut hms = parts.next()?.split(':');
    let h: u64 = hms.next()?.parse().ok()?;
    let m: u64 = hms.next()?.parse().ok()?;
    let s: u64 = hms.next()?.parse().ok()?;
    if parts.next()? != "GMT" || parts.next().is_some() || hms.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&day) || year < 1970 || h > 23 || m > 59 || s > 60 {
        return None;
    }
    // days from civil (Howard Hinnant), for years >= 1970
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + m * 60 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_forms() {
        assert_eq!(
            retry_after_secs(Some(&HeaderValue::from_static("120"))),
            Some(120)
        );
        // an HTTP-date is the time left until it
        let date = HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT");
        let at = |unix: u64| UNIX_EPOCH + std::time::Duration::from_secs(unix);
        assert_eq!(parse_imf_fixdate("Thu, 01 Jan 1970 00:00:10 GMT"), Some(10));
        assert_eq!(
            parse_imf_fixdate("Wed, 21 Oct 2026 07:28:00 GMT"),
            Some(1_792_567_680)
        );
        assert_eq!(
            retry_after_at(Some(&date), at(1_792_567_680 - 90)),
            Some(90)
        );
        assert_eq!(retry_after_at(Some(&date), at(1_792_567_680 + 90)), Some(0));
        assert_eq!(
            parse_imf_fixdate("Mon, 29 Feb 2028 12:00:00 GMT"),
            Some(1_835_438_400)
        );
        assert_eq!(parse_imf_fixdate("Wed, 21 Oct 2026 07:28:00 PST"), None);
        assert_eq!(parse_imf_fixdate("garbage GMT"), None);
        assert_eq!(
            retry_after_secs(Some(&HeaderValue::from_static("soon"))),
            None
        );
        assert_eq!(retry_after_secs(None), None);
    }
}
