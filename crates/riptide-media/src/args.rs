//! The yt-dlp command lines, built from validated values only.
//!
//! Every run starts with [`COMMON`]: `--ignore-config` (a user's own yt-dlp config must not
//! change what Riptide does), `--no-playlist`, UTF-8 output. Nothing here ever adds a flag that
//! plays or keeps DRM-protected formats (`--allow-unplayable-formats`); the test
//! `no_drm_bypass_flag_is_ever_passed` pins that.

use std::ffi::OsString;
use std::path::Path;

use riptide_core::MediaSelection;
use url::Url;

use crate::{Error, Result};

/// Flags every run gets.
pub const COMMON: &[&str] = &[
    "--ignore-config",
    "--no-playlist",
    "--encoding",
    "utf-8",
    "--no-color",
];

/// Flags that would bypass DRM or keep unplayable formats. Never passed.
pub const FORBIDDEN: &[&str] = &["--allow-unplayable-formats", "--no-check-formats"];

/// `--progress-template` for the download phase; parsed by
/// [`crate::parse::parse_progress_line`].
pub const PROGRESS_TEMPLATE: &str = "download:RIPTIDE-P %(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.format_id)s";

/// `--print` template naming the finished file; parsed by [`crate::parse::parse_file_line`].
pub const FILE_TEMPLATE: &str = "after_move:RIPTIDE-F %(filepath)s";

/// Accepts only absolute `http(s)` URLs (no credentials in them, nothing that could pass for a
/// flag).
pub fn check_url(raw: &str) -> Result<Url> {
    let raw = raw.trim();
    if raw.starts_with('-') {
        return Err(Error::InvalidUrl);
    }
    let url = Url::parse(raw).map_err(|_| Error::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::InvalidUrl);
    }
    Ok(url)
}

/// The YouTube player clients Riptide may ask yt-dlp to use (`--extractor-args
/// youtube:player_client=...`). A fixed set: the value on the command line comes from this enum
/// and from nowhere else, never from page, browser or user text.
///
/// The ladder order was measured against yt-dlp 2026.08.19 without a JavaScript runtime: the
/// default clients work for most videos. When they answer "The page needs to be reloaded.",
/// `android_vr` still lists formats (its https formats need a PO token, so a download may end
/// in HTTP 403 and move on); `tv,mweb` and `web_safari,tv` come last because on this build
/// they fail the same way without a JavaScript runtime, but they may work from other networks
/// or once a runtime is available. All four names are accepted by yt-dlp 2026.08.19.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerClients {
    /// yt-dlp's own choice (no extractor argument).
    Default,
    /// `android_vr`.
    AndroidVr,
    /// `tv,mweb`.
    TvMweb,
    /// `web_safari,tv`.
    WebSafariTv,
}

impl PlayerClients {
    /// Every step, in the order the fallback ladder tries them.
    pub const LADDER: [PlayerClients; 4] = [
        PlayerClients::Default,
        PlayerClients::AndroidVr,
        PlayerClients::TvMweb,
        PlayerClients::WebSafariTv,
    ];

    /// Stable key (stored in the per-download flags, shown in logs).
    pub fn key(self) -> &'static str {
        match self {
            PlayerClients::Default => "default",
            PlayerClients::AndroidVr => "android_vr",
            PlayerClients::TvMweb => "tv_mweb",
            PlayerClients::WebSafariTv => "web_safari_tv",
        }
    }

    /// Inverse of [`PlayerClients::key`].
    pub fn parse(key: &str) -> Option<PlayerClients> {
        Self::LADDER.into_iter().find(|c| c.key() == key)
    }

    /// The `--extractor-args` value, or `None` for the default clients.
    pub fn extractor_args(self) -> Option<&'static str> {
        match self {
            PlayerClients::Default => None,
            PlayerClients::AndroidVr => Some("youtube:player_client=android_vr"),
            PlayerClients::TvMweb => Some("youtube:player_client=tv,mweb"),
            PlayerClients::WebSafariTv => Some("youtube:player_client=web_safari,tv"),
        }
    }
}

/// `true` for the YouTube family of hosts, the only ones the client ladder applies to.
pub fn is_youtube_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    ["youtube.com", "youtu.be", "youtube-nocookie.com"]
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

fn push_clients(a: &mut Vec<OsString>, clients: PlayerClients) {
    if let Some(v) = clients.extractor_args() {
        a.push("--extractor-args".into());
        a.push(v.into());
    }
}

/// `yt-dlp -J` for one page.
pub fn probe_args(url: &Url, cookies: Option<&Path>, clients: PlayerClients) -> Vec<OsString> {
    let mut a: Vec<OsString> = COMMON.iter().map(OsString::from).collect();
    a.push("-J".into());
    if let Some(c) = cookies {
        a.push("--cookies".into());
        a.push(c.into());
    }
    push_clients(&mut a, clients);
    a.push("--".into());
    a.push(url.as_str().into());
    a
}

/// Everything a download run needs.
#[derive(Debug, Clone)]
pub struct RunArgs<'a> {
    /// Page URL (already checked).
    pub url: &'a Url,
    /// Quality.
    pub selection: MediaSelection,
    /// Destination folder.
    pub dir: &'a Path,
    /// File name without extension (already sanitised; `%` is escaped here).
    pub stem: &'a str,
    /// Bundled ffmpeg, if any.
    pub ffmpeg: Option<&'a Path>,
    /// Temporary cookie file, if any.
    pub cookies: Option<&'a Path>,
    /// `--limit-rate` in bytes per second.
    pub rate_limit: Option<u64>,
    /// `--proxy` URL.
    pub proxy: Option<&'a str>,
    /// YouTube player clients (see [`PlayerClients`]).
    pub clients: PlayerClients,
}

/// The format selector and sort for a quality. Video prefers H.264 in MP4 with AAC (M4A) at the
/// same resolution and frame rate, so the merged file plays everywhere (AV1/VP9 only win where
/// no H.264 exists, as for 1440p and up on YouTube); `-S res:N` takes the largest resolution not
/// above N.
pub fn selection_args(selection: MediaSelection) -> Vec<OsString> {
    let v: Vec<String> = match selection {
        MediaSelection::Best => vec![
            "-f".into(),
            "bv*+ba/b".into(),
            "-S".into(),
            "res,fps,vcodec:h264,ext:mp4:m4a".into(),
            "--merge-output-format".into(),
            "mp4".into(),
        ],
        MediaSelection::MaxHeight(h) => vec![
            "-f".into(),
            "bv*+ba/b".into(),
            "-S".into(),
            format!("res:{h},fps,vcodec:h264,ext:mp4:m4a"),
            "--merge-output-format".into(),
            "mp4".into(),
        ],
        MediaSelection::AudioM4a => vec![
            "-f".into(),
            "ba[ext=m4a]/ba/b".into(),
            "-x".into(),
            "--audio-format".into(),
            "m4a".into(),
        ],
        MediaSelection::AudioMp3 => vec![
            "-f".into(),
            "ba/b".into(),
            "-x".into(),
            "--audio-format".into(),
            "mp3".into(),
            "--audio-quality".into(),
            "0".into(),
        ],
    };
    v.into_iter().map(OsString::from).collect()
}

/// The download command line.
pub fn download_args(r: &RunArgs<'_>) -> Vec<OsString> {
    let mut a: Vec<OsString> = COMMON.iter().map(OsString::from).collect();
    for f in [
        "--newline",
        "--progress",
        "--no-simulate",
        "--continue",
        "--no-mtime",
        "--no-write-info-json",
        "--no-write-thumbnail",
    ] {
        a.push(f.into());
    }
    a.push("--progress-template".into());
    a.push(PROGRESS_TEMPLATE.into());
    a.push("--print".into());
    a.push(FILE_TEMPLATE.into());
    if let Some(f) = r.ffmpeg {
        a.push("--ffmpeg-location".into());
        a.push(f.into());
    }
    if let Some(c) = r.cookies {
        a.push("--cookies".into());
        a.push(c.into());
    }
    if let Some(limit) = r.rate_limit.filter(|l| *l > 0) {
        a.push("--limit-rate".into());
        a.push(limit.to_string().into());
    }
    if let Some(p) = r.proxy.filter(|p| !p.is_empty()) {
        a.push("--proxy".into());
        a.push(p.into());
    }
    push_clients(&mut a, r.clients);
    a.extend(selection_args(r.selection));
    a.push("-P".into());
    a.push(r.dir.into());
    a.push("-o".into());
    a.push(format!("{}.%(ext)s", r.stem.replace('%', "%%")).into());
    a.push("--".into());
    a.push(r.url.as_str().into());
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn urls_must_be_http_and_cannot_pose_as_flags() {
        assert!(check_url("https://www.youtube.com/watch?v=x").is_ok());
        assert!(check_url(" http://vimeo.com/1 ").is_ok());
        for bad in [
            "--exec calc",
            "-o C:/x",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "ftp://x/y",
            "notaurl",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn download_args_end_with_the_url_after_a_separator() {
        let url = check_url("https://www.youtube.com/watch?v=aqz-KE-bpKQ").unwrap();
        let r = RunArgs {
            url: &url,
            selection: MediaSelection::MaxHeight(720),
            dir: Path::new("C:/dl"),
            stem: "100% Bunny",
            ffmpeg: Some(Path::new("C:/app/ffmpeg.exe")),
            cookies: Some(Path::new("C:/tmp/c.txt")),
            rate_limit: Some(1000),
            proxy: Some("socks5://127.0.0.1:9050"),
            clients: PlayerClients::Default,
        };
        let a = strings(&download_args(&r));
        let n = a.len();
        assert_eq!(a[n - 2], "--");
        assert_eq!(a[n - 1], "https://www.youtube.com/watch?v=aqz-KE-bpKQ");
        assert!(a.contains(&"100%% Bunny.%(ext)s".to_string()));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "-S" && w[1] == "res:720,fps,vcodec:h264,ext:mp4:m4a"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--ffmpeg-location" && w[1] == "C:/app/ffmpeg.exe"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--limit-rate" && w[1] == "1000"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--merge-output-format" && w[1] == "mp4"));
        assert!(a.contains(&"--ignore-config".to_string()));
        assert!(a.contains(&"--continue".to_string()));
    }

    #[test]
    fn no_drm_bypass_flag_is_ever_passed() {
        let url = check_url("https://x.test/v").unwrap();
        for selection in [
            MediaSelection::Best,
            MediaSelection::MaxHeight(1080),
            MediaSelection::AudioM4a,
            MediaSelection::AudioMp3,
        ] {
            let r = RunArgs {
                url: &url,
                selection,
                dir: Path::new("/tmp"),
                stem: "v",
                ffmpeg: None,
                cookies: None,
                rate_limit: None,
                proxy: None,
                clients: PlayerClients::TvMweb,
            };
            for list in [
                download_args(&r),
                probe_args(&url, None, PlayerClients::AndroidVr),
            ] {
                let s = strings(&list);
                for f in FORBIDDEN {
                    assert!(!s.iter().any(|a| a == f), "{f} in {s:?}");
                }
            }
        }
        let a = strings(&selection_args(MediaSelection::AudioMp3));
        assert!(a.contains(&"-x".to_string()) && a.contains(&"mp3".to_string()));
    }

    #[test]
    fn player_clients_become_fixed_extractor_args() {
        let url = check_url("https://www.youtube.com/watch?v=x").unwrap();
        let find = |a: Vec<OsString>| -> Option<String> {
            let a = strings(&a);
            a.windows(2)
                .find(|w| w[0] == "--extractor-args")
                .map(|w| w[1].clone())
        };
        assert_eq!(find(probe_args(&url, None, PlayerClients::Default)), None);
        assert_eq!(
            find(probe_args(&url, None, PlayerClients::AndroidVr)).as_deref(),
            Some("youtube:player_client=android_vr")
        );
        assert_eq!(
            find(probe_args(&url, None, PlayerClients::TvMweb)).as_deref(),
            Some("youtube:player_client=tv,mweb")
        );
        assert_eq!(
            find(probe_args(&url, None, PlayerClients::WebSafariTv)).as_deref(),
            Some("youtube:player_client=web_safari,tv")
        );
        // the argument comes before the `--` separator, and the ladder keys round-trip
        let a = strings(&probe_args(&url, None, PlayerClients::TvMweb));
        let sep = a.iter().position(|x| x == "--").unwrap();
        assert!(a.iter().position(|x| x == "--extractor-args").unwrap() < sep);
        for c in PlayerClients::LADDER {
            assert_eq!(PlayerClients::parse(c.key()), Some(c));
        }
        assert_eq!(PlayerClients::parse("web --exec calc"), None);
        assert_eq!(PlayerClients::LADDER[0], PlayerClients::Default);
    }

    #[test]
    fn only_the_youtube_family_gets_the_ladder() {
        assert!(is_youtube_host("www.youtube.com"));
        assert!(is_youtube_host("youtu.be"));
        assert!(is_youtube_host("m.YouTube.com"));
        assert!(!is_youtube_host("notyoutube.com"));
        assert!(!is_youtube_host("youtube.com.evil.test"));
        assert!(!is_youtube_host("vimeo.com"));
    }
}
