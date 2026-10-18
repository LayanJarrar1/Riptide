//! A stand-in for yt-dlp in tests. It accepts the command lines `riptide-media` builds, never
//! touches the network, and prints the same protocol (`-J` JSON, `RIPTIDE-P` progress lines,
//! `RIPTIDE-F` final path, `ERROR:` lines on stderr). Never shipped.
//!
//! The page URL steers it: `.../drm` and `.../live` probe as DRM-only / live pages,
//! `.../missing` fails like an unavailable video, `.../fail` fails the download with HTTP 503,
//! `.../slow` downloads slowly enough to be stopped half-way. Downloads write deterministic
//! bytes (see `byte`) in two streams (video `f137`, audio `f140`) that are "merged" by
//! concatenation, resume from their `.part` files with `--continue`, and leave `args.json` (the
//! command line) and `cookies-seen.txt` (the cookie file's content) next to the output.
#![allow(clippy::unwrap_used, missing_docs)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const YOUTUBE: &str = include_str!("../../tests/fixtures/youtube.json");
const DRM: &str = include_str!("../../tests/fixtures/drm.json");
const LIVE: &str = include_str!("../../tests/fixtures/live.json");

/// Byte `i` of stream `stream` (the tests compute the same).
fn byte(stream: u64, i: u64) -> u8 {
    ((i * 31 + stream * 7 + i / 251) % 256) as u8
}

const VALUE_FLAGS: &[&str] = &[
    "--encoding",
    "--progress-template",
    "--print",
    "--ffmpeg-location",
    "--cookies",
    "--limit-rate",
    "--proxy",
    "-f",
    "-S",
    "--merge-output-format",
    "--audio-format",
    "--audio-quality",
    "-P",
    "-o",
];

struct Args {
    all: Vec<String>,
    flags: Vec<String>,
    values: Vec<(String, String)>,
    url: String,
}

impl Args {
    fn parse() -> Args {
        let all: Vec<String> = std::env::args().skip(1).collect();
        let mut flags = Vec::new();
        let mut values = Vec::new();
        let mut url = String::new();
        let mut it = all.iter();
        while let Some(a) = it.next() {
            if a == "--" {
                url = it.next().cloned().unwrap_or_default();
                break;
            }
            if VALUE_FLAGS.contains(&a.as_str()) {
                values.push((a.clone(), it.next().cloned().unwrap_or_default()));
            } else {
                flags.push(a.clone());
            }
        }
        Args {
            all,
            flags,
            values,
            url,
        }
    }

    fn has(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }

    fn value(&self, f: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == f)
            .map(|(_, v)| v.as_str())
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    std::process::exit(1);
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn main() {
    let args = Args::parse();
    if args.has("--version") {
        say("2099.01.01-fake");
        return;
    }
    if args.has("-U") {
        say("Updated yt-dlp to 2099.01.02-fake");
        return;
    }
    if args.has("-J") {
        if args.url.contains("missing") {
            fail("[youtube] missing: Video unavailable");
        }
        let json = if args.url.contains("drm") {
            DRM
        } else if args.url.contains("live") {
            LIVE
        } else {
            YOUTUBE
        };
        say(json.trim());
        return;
    }
    download(&args);
}

fn download(args: &Args) {
    let dir = PathBuf::from(args.value("-P").unwrap_or("."));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("args.json"),
        serde_json::to_string(&args.all).unwrap(),
    )
    .unwrap();
    if let Some(c) = args.value("--cookies") {
        let mut text = String::new();
        fs::File::open(c)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        fs::write(dir.join("cookies-seen.txt"), text).unwrap();
    }
    if args.url.contains("fail") {
        fail("unable to download video data: HTTP Error 503: Service Unavailable");
    }
    if args.url.contains("drm") {
        fail("[youtube] drm-movie: This video is DRM protected");
    }
    let template = args.value("-o").unwrap_or("video.%(ext)s");
    let stem = template
        .strip_suffix(".%(ext)s")
        .unwrap_or(template)
        .replace("%%", "%");
    let audio_only = args.has("-x");
    let final_ext = if audio_only {
        args.value("--audio-format").unwrap_or("m4a").to_string()
    } else {
        args.value("--merge-output-format")
            .unwrap_or("mp4")
            .to_string()
    };
    let streams: Vec<(u64, &str, &str, u64)> = if audio_only {
        vec![(1, "140", "m4a", 256 * 1024)]
    } else {
        vec![(0, "137", "mp4", 768 * 1024), (1, "140", "m4a", 256 * 1024)]
    };
    let delay = if args.url.contains("slow") {
        Duration::from_millis(15)
    } else {
        Duration::from_millis(1)
    };
    let chunk = 16 * 1024u64;
    let mut done_files = Vec::new();
    for (stream, id, ext, size) in streams {
        let done = dir.join(format!("{stem}.f{id}.{ext}"));
        done_files.push(done.clone());
        if done.exists() {
            say(&format!(
                "[download] {} has already been downloaded",
                done.display()
            ));
            continue;
        }
        let part = PathBuf::from(format!("{}.part", done.display()));
        let mut start = match fs::metadata(&part) {
            Ok(m) if args.has("--continue") => m.len().min(size),
            _ => 0,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(start == 0)
            .open(&part)
            .unwrap();
        file.set_len(start).unwrap();
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(start)).unwrap();
        while start < size {
            let end = (start + chunk).min(size);
            let bytes: Vec<u8> = (start..end).map(|i| byte(stream, i)).collect();
            file.write_all(&bytes).unwrap();
            file.flush().unwrap();
            start = end;
            say(&format!(
                "RIPTIDE-P downloading|{start}|{size}|NA|1048576.0|{}|{id}",
                (size - start) / 1_048_576
            ));
            std::thread::sleep(delay);
        }
        drop(file);
        say(&format!("RIPTIDE-P finished|{size}|{size}|NA|NA|NA|{id}"));
        fs::rename(&part, &done).unwrap();
    }
    let out = dir.join(format!("{stem}.{final_ext}"));
    merge(&done_files, &out);
    say(&format!(
        "[Merger] Merging formats into \"{}\"",
        out.display()
    ));
    say(&format!("RIPTIDE-F {}", out.display()));
}

fn merge(parts: &[PathBuf], out: &Path) {
    let mut all = Vec::new();
    for p in parts {
        all.extend(fs::read(p).unwrap());
    }
    fs::write(out, all).unwrap();
    for p in parts {
        let _ = fs::remove_file(p);
    }
}
