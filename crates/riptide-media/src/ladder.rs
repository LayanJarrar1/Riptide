//! The retry policy for one media page: cookies only when the site asks for a login, and a
//! fixed ladder of YouTube player clients when the site refuses a request.
//!
//! * The first attempt never carries cookies. YouTube rotates the cookies of a signed-in
//!   account every few minutes and answers a stale set with "The page needs to be reloaded.",
//!   so cookies are a fallback, not a default.
//! * An error that says a login is needed (age gate, bot check, private or members-only video)
//!   retries **once** with the browser's cookies, if there are any.
//! * A cookie attempt that fails with "The page needs to be reloaded." retries without cookies.
//! * On YouTube hosts a refusal that another client may get past ("The page needs to be
//!   reloaded.", "Sign in to confirm", HTTP 403, a failed precondition, a missing PO token)
//!   moves down [`PlayerClients::LADDER`]. "Video unavailable" and "Private video" never do.
//! * At most [`MAX_ATTEMPTS`] attempts in total. The attempt that worked is kept for the record
//!   (the engine stores it) and is tried first on the next run.

use url::Url;

use crate::args::{is_youtube_host, PlayerClients};
use crate::Error;

/// Attempts per run (probe or download), the first one included.
pub const MAX_ATTEMPTS: u8 = 4;

/// One way of running yt-dlp: with or without the browser's cookies, with one set of player
/// clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    /// Pass the cookies.
    pub cookies: bool,
    /// YouTube player clients.
    pub clients: PlayerClients,
}

impl Attempt {
    /// No cookies, yt-dlp's own clients: where every record starts.
    pub const FIRST: Attempt = Attempt {
        cookies: false,
        clients: PlayerClients::Default,
    };
}

/// Decides the next [`Attempt`] after a failure.
#[derive(Debug)]
pub struct Ladder {
    youtube: bool,
    cookies_available: bool,
    tried_cookies: bool,
    attempt: Attempt,
    /// Client order: the remembered client first, then the rest of the ladder.
    order: Vec<PlayerClients>,
    position: usize,
    used: u8,
}

impl Ladder {
    /// A ladder for `url`. `cookies_available` is whether the browser sent cookies; `hint` is
    /// the attempt that worked last time for this record.
    pub fn new(url: &str, cookies_available: bool, hint: Option<Attempt>) -> Ladder {
        let youtube = Url::parse(url.trim())
            .ok()
            .and_then(|u| u.host_str().map(is_youtube_host))
            .unwrap_or(false);
        let hint = hint.unwrap_or(Attempt::FIRST);
        let first_client = if youtube {
            hint.clients
        } else {
            PlayerClients::Default
        };
        let mut order = vec![first_client];
        if youtube {
            order.extend(
                PlayerClients::LADDER
                    .into_iter()
                    .filter(|c| *c != first_client),
            );
        }
        let cookies = hint.cookies && cookies_available;
        Ladder {
            youtube,
            cookies_available,
            tried_cookies: cookies,
            attempt: Attempt {
                cookies,
                clients: first_client,
            },
            order,
            position: 0,
            used: 1,
        }
    }

    /// The attempt to run now.
    pub fn current(&self) -> Attempt {
        self.attempt
    }

    /// How many attempts have been started, the current one included.
    pub fn used(&self) -> u8 {
        self.used
    }

    /// After `err` failed the current attempt: the next attempt, or `None` to give up.
    pub fn next(&mut self, err: &Error) -> Option<Attempt> {
        if self.used >= MAX_ATTEMPTS || !matches!(err, Error::Tool { .. }) {
            return None;
        }
        if self.attempt.cookies && err.is_stale_page() {
            // stale account cookies: drop them and ask again as an anonymous visitor
            self.attempt.cookies = false;
        } else if err.needs_login()
            && self.cookies_available
            && !self.tried_cookies
            && !self.attempt.cookies
        {
            self.attempt.cookies = true;
            self.tried_cookies = true;
        } else if self.youtube && err.client_retryable() && self.position + 1 < self.order.len() {
            self.position += 1;
            self.attempt.clients = self.order[self.position];
        } else {
            return None;
        }
        self.used += 1;
        Some(self.attempt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::classify_error;

    const YT: &str = "https://www.youtube.com/watch?v=x";

    fn err(line: &str) -> Error {
        classify_error(&format!("ERROR: {line}"))
    }

    fn reload() -> Error {
        err("[youtube] x: The page needs to be reloaded.")
    }

    #[test]
    fn the_first_attempt_never_has_cookies() {
        let l = Ladder::new(YT, true, None);
        assert_eq!(l.current(), Attempt::FIRST);
        assert!(!l.current().cookies);
    }

    #[test]
    fn login_errors_retry_once_with_cookies_and_never_twice() {
        let mut l = Ladder::new(YT, true, None);
        let a = l
            .next(&err("[youtube] x: Sign in to confirm your age."))
            .unwrap();
        assert!(a.cookies);
        assert_eq!(a.clients, PlayerClients::Default);
        // the cookie attempt fails with a login error again: no second cookie attempt; the
        // "Sign in to confirm" refusal moves on to the next client instead (without cookies
        // being dropped, they are the user's only chance)
        let b = l
            .next(&err("[youtube] x: Sign in to confirm your age."))
            .unwrap();
        assert_eq!(b.clients, PlayerClients::AndroidVr);
        // a private video has nothing left to try
        let mut p = Ladder::new(YT, true, None);
        assert!(
            p.next(&err(
                "[youtube] x: Private video. Sign in if you've been granted access"
            ))
            .unwrap()
            .cookies
        );
        assert!(p
            .next(&err(
                "[youtube] x: Private video. Sign in if you've been granted access"
            ))
            .is_none());
    }

    #[test]
    fn no_cookies_means_no_cookie_attempt() {
        let mut l = Ladder::new(YT, false, None);
        let a = l
            .next(&err("[youtube] x: Sign in to confirm your age."))
            .unwrap();
        assert!(!a.cookies, "goes down the ladder instead");
        assert_eq!(a.clients, PlayerClients::AndroidVr);
        let mut private = Ladder::new(YT, false, None);
        assert!(private
            .next(&err(
                "[youtube] x: Private video. Sign in if you've been granted access"
            ))
            .is_none());
    }

    #[test]
    fn stale_cookies_are_dropped_and_the_ladder_goes_on() {
        let mut l = Ladder::new(
            YT,
            true,
            Some(Attempt {
                cookies: true,
                clients: PlayerClients::Default,
            }),
        );
        assert!(
            l.current().cookies,
            "a remembered cookie attempt starts with cookies"
        );
        let a = l.next(&reload()).unwrap();
        assert_eq!(
            a,
            Attempt {
                cookies: false,
                clients: PlayerClients::Default
            }
        );
        let b = l.next(&reload()).unwrap();
        assert_eq!(b.clients, PlayerClients::AndroidVr);
        assert!(!b.cookies);
    }

    #[test]
    fn the_ladder_visits_each_client_once_and_stops_at_four_attempts() {
        let mut l = Ladder::new(YT, false, None);
        let mut seen = vec![l.current().clients];
        while let Some(a) = l.next(&reload()) {
            seen.push(a.clients);
        }
        assert_eq!(seen, PlayerClients::LADDER.to_vec());
        assert_eq!(l.used(), MAX_ATTEMPTS);
        // the cap also holds when cookies and clients both move
        let mut m = Ladder::new(
            YT,
            true,
            Some(Attempt {
                cookies: true,
                clients: PlayerClients::Default,
            }),
        );
        let mut n = 1;
        while m.next(&reload()).is_some() {
            n += 1;
        }
        assert!(n <= MAX_ATTEMPTS);
    }

    #[test]
    fn a_remembered_client_goes_first_and_the_rest_follow() {
        let mut l = Ladder::new(
            YT,
            false,
            Some(Attempt {
                cookies: false,
                clients: PlayerClients::TvMweb,
            }),
        );
        assert_eq!(l.current().clients, PlayerClients::TvMweb);
        assert_eq!(l.next(&reload()).unwrap().clients, PlayerClients::Default);
    }

    #[test]
    fn unavailable_and_non_youtube_pages_do_not_climb() {
        let mut l = Ladder::new(YT, true, None);
        assert!(l
            .next(&err(
                "[youtube] x: Video unavailable. This content isn't available"
            ))
            .is_none());
        let mut v = Ladder::new("https://vimeo.com/1", false, None);
        assert!(v.next(&reload()).is_none(), "the ladder is YouTube only");
        let mut t = Ladder::new(YT, false, None);
        assert!(t.next(&Error::Timeout).is_none());
        assert!(t.next(&Error::Drm).is_none());
    }

    #[test]
    fn a_non_youtube_site_still_gets_the_cookie_retry() {
        let mut v = Ladder::new("https://vimeo.com/1", true, None);
        let a = v.next(&err("This video is private")).unwrap();
        assert!(a.cookies);
        assert_eq!(a.clients, PlayerClients::Default);
    }
}
