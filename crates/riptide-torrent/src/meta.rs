//! Reading and vetting `.torrent` data before the engine ever sees it.
//!
//! [`inspect_torrent`] parses the bytes, enforces every limit in [`crate::limits`], and returns a
//! [`TorrentPreview`] whose file paths are already sanitised ([`crate::paths`]). The user picks
//! files from that preview, so what they see is exactly what would be written.

use std::path::{Path, PathBuf};

use librqbit::{torrent_from_bytes, ValidatedTorrentMetaV1Info};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::limits::{
    MAX_FILES, MAX_PIECES, MAX_PIECE_LENGTH, MAX_TORRENT_FILE_BYTES, MAX_TOTAL_BYTES,
    MIN_PIECE_LENGTH,
};
use crate::paths::{plan_layout, RawFile};

/// One file in a [`TorrentPreview`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Index to use in [`crate::AddOptions::only_files`].
    pub index: usize,
    /// Sanitised path relative to the download folder.
    pub path: PathBuf,
    /// Size in bytes.
    pub len: u64,
    /// BEP 47 padding file (never written, never shown to users).
    pub padding: bool,
}

/// What a torrent contains, as it would be written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentPreview {
    /// Info-hash, 40 lowercase hex digits (also the id of the torrent in the engine).
    pub info_hash: String,
    /// Sanitised torrent name.
    pub name: String,
    /// Files, in torrent order.
    pub files: Vec<FileEntry>,
    /// Sum of all file sizes.
    pub total_bytes: u64,
    /// Piece length in bytes.
    pub piece_length: u32,
    /// Number of pieces.
    pub piece_count: u64,
    /// `private` flag (no DHT / PEX for this torrent).
    pub private: bool,
    /// Web seeds (BEP 19 `url-list`): `http(s)` URLs only, at most [`MAX_WEB_SEEDS`]. Empty for
    /// a torrent resolved from a magnet link (the peers send the info dictionary only).
    pub web_seeds: Vec<String>,
    /// The complete `.torrent` file; feed it back to [`crate::TorrentSource::Bytes`] to add the
    /// torrent without resolving the magnet link a second time.
    #[serde(skip)]
    pub torrent_bytes: Vec<u8>,
}

/// Checks that `data` is a well-formed bencode value no deeper than 32 levels, without
/// allocating. Guards the (recursive) parser against stack exhaustion.
pub fn check_bencode_shape(data: &[u8]) -> Result<()> {
    const MAX_DEPTH: usize = 32;
    let bad = |m: &str| Error::InvalidTorrent(format!("not valid bencode: {m}"));
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut top_values = 0usize;
    while i < data.len() {
        match data[i] {
            b'i' => {
                let end = data[i..]
                    .iter()
                    .position(|&b| b == b'e')
                    .ok_or_else(|| bad("unterminated integer"))?;
                i += end + 1;
                if depth == 0 {
                    top_values += 1;
                }
            }
            b'l' | b'd' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(bad("nested too deeply"));
                }
                i += 1;
            }
            b'e' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| bad("stray end marker"))?;
                i += 1;
                if depth == 0 {
                    top_values += 1;
                }
            }
            b'0'..=b'9' => {
                let colon = data[i..]
                    .iter()
                    .position(|&b| b == b':')
                    .ok_or_else(|| bad("string without length"))?;
                if colon > 12 {
                    return Err(bad("string length too long"));
                }
                let len: usize = std::str::from_utf8(&data[i..i + colon])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| bad("bad string length"))?;
                let start = i + colon + 1;
                let end = start.checked_add(len).ok_or_else(|| bad("overflow"))?;
                if end > data.len() {
                    return Err(bad("string runs past the end"));
                }
                i = end;
                if depth == 0 {
                    top_values += 1;
                }
            }
            _ => return Err(bad("unexpected byte")),
        }
    }
    if depth != 0 || top_values != 1 {
        return Err(bad("truncated or trailing data"));
    }
    Ok(())
}

/// Numeric sanity limits shared by `.torrent` files and metadata fetched through a magnet link.
fn check_numbers(
    piece_length: u32,
    pieces_len: usize,
    file_lens: impl Iterator<Item = u64>,
) -> Result<(u64, u64)> {
    if !(MIN_PIECE_LENGTH..=MAX_PIECE_LENGTH).contains(&piece_length) {
        return Err(Error::InvalidTorrent(format!(
            "piece length {piece_length} is outside {MIN_PIECE_LENGTH}..={MAX_PIECE_LENGTH}"
        )));
    }
    let mut total: u64 = 0;
    let mut count = 0usize;
    for len in file_lens {
        count += 1;
        if count > MAX_FILES {
            return Err(Error::InvalidTorrent(format!(
                "more than {MAX_FILES} files"
            )));
        }
        total = total
            .checked_add(len)
            .filter(|t| *t <= MAX_TOTAL_BYTES)
            .ok_or_else(|| Error::InvalidTorrent("total size is absurdly large".into()))?;
    }
    if count == 0 || total == 0 {
        return Err(Error::InvalidTorrent("the torrent has no data".into()));
    }
    let pieces = total.div_ceil(u64::from(piece_length));
    if pieces > MAX_PIECES {
        return Err(Error::InvalidTorrent(format!(
            "{pieces} pieces is more than the limit of {MAX_PIECES}"
        )));
    }
    if pieces_len as u64 != pieces * 20 {
        return Err(Error::InvalidTorrent(
            "piece hash list does not match the size and piece length".into(),
        ));
    }
    Ok((total, pieces))
}

/// Builds the preview of a validated info dictionary (the one already parsed by the engine
/// library, either from a `.torrent` or fetched through a magnet link).
pub fn preview_from_info<B: AsRef<[u8]>>(
    info_hash: String,
    info: &ValidatedTorrentMetaV1Info<B>,
    torrent_bytes: Vec<u8>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<TorrentPreview> {
    let raw = info.info();
    let (total_bytes, piece_count) = check_numbers(
        raw.piece_length,
        raw.pieces.as_ref().len(),
        info.iter_file_lengths(),
    )?;

    let name = info
        .name()
        .map(|n| n.into_owned())
        .unwrap_or_else(|| "torrent".to_string());
    let raw_files: Vec<RawFile> = info
        .iter_file_details()
        .map(|fd| RawFile {
            components: fd
                .filename
                .iter_components()
                .map(|c| c.into_owned())
                .collect(),
            len: fd.len,
            padding: fd.attrs().padding,
        })
        .collect();
    let layout = plan_layout(&name, &raw_files, exists)?;
    let files = raw_files
        .iter()
        .zip(layout)
        .enumerate()
        .map(|(index, (f, path))| FileEntry {
            index,
            path,
            len: f.len,
            padding: f.padding,
        })
        .collect();
    Ok(TorrentPreview {
        info_hash,
        name: crate::paths::sanitize_component(&name),
        files,
        total_bytes,
        piece_length: raw.piece_length,
        piece_count,
        private: raw.private,
        web_seeds: web_seeds(&torrent_bytes),
        torrent_bytes,
    })
}

/// Most web seeds kept from one `.torrent` file.
pub const MAX_WEB_SEEDS: usize = 8;

/// The BEP 19 web seeds (`url-list`) in the top level of a `.torrent` file: a string or a list of
/// strings, kept only when they are `http` or `https` URLs, at most [`MAX_WEB_SEEDS`], no repeats.
/// Anything that does not parse gives an empty list (the data was vetted by
/// [`check_bencode_shape`] first). Riptide never contacts a web seed by itself: the user chooses
/// to download from one, and then it is an ordinary download.
pub fn web_seeds(data: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &[u8]| {
        let Ok(text) = std::str::from_utf8(raw) else {
            return;
        };
        let Ok(url) = url::Url::parse(text.trim()) else {
            return;
        };
        if matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && out.len() < MAX_WEB_SEEDS
            && !out.iter().any(|u| u == url.as_str())
        {
            out.push(url.to_string());
        }
    };
    let mut i = 0;
    if data.first() != Some(&b'd') {
        return out;
    }
    i += 1;
    while data.get(i).is_some_and(|b| *b != b'e') {
        let Some(key) = bencode_str(data, &mut i) else {
            return Vec::new();
        };
        if key == b"url-list" {
            match data.get(i) {
                Some(b'l') => {
                    i += 1;
                    while data.get(i).is_some_and(|b| *b != b'e') {
                        match bencode_str(data, &mut i) {
                            Some(s) => push(s),
                            None => {
                                if bencode_skip(data, &mut i, 0).is_none() {
                                    return out;
                                }
                            }
                        }
                    }
                    i += 1;
                }
                _ => {
                    if let Some(s) = bencode_str(data, &mut i) {
                        push(s);
                    } else if bencode_skip(data, &mut i, 0).is_none() {
                        return out;
                    }
                }
            }
        } else if bencode_skip(data, &mut i, 0).is_none() {
            return out;
        }
    }
    out
}

/// A bencoded byte string at `*i` (`<len>:<bytes>`); moves `*i` past it.
fn bencode_str<'a>(data: &'a [u8], i: &mut usize) -> Option<&'a [u8]> {
    let start = *i;
    let mut j = start;
    while data.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    if j == start || data.get(j) != Some(&b':') {
        return None;
    }
    let len: usize = std::str::from_utf8(&data[start..j]).ok()?.parse().ok()?;
    let from = j + 1;
    let to = from.checked_add(len)?;
    let bytes = data.get(from..to)?;
    *i = to;
    Some(bytes)
}

/// Skips one bencoded value at `*i` (depth-limited).
fn bencode_skip(data: &[u8], i: &mut usize, depth: usize) -> Option<()> {
    if depth > 32 {
        return None;
    }
    match *data.get(*i)? {
        b'i' => {
            let end = data[*i..].iter().position(|b| *b == b'e')?;
            *i += end + 1;
            Some(())
        }
        b'l' | b'd' => {
            *i += 1;
            while *data.get(*i)? != b'e' {
                bencode_skip(data, i, depth + 1)?;
            }
            *i += 1;
            Some(())
        }
        b'0'..=b'9' => bencode_str(data, i).map(|_| ()),
        _ => None,
    }
}

/// Parses and vets `.torrent` data.
pub fn inspect_torrent(bytes: &[u8], exists: &dyn Fn(&Path) -> bool) -> Result<TorrentPreview> {
    if bytes.len() > MAX_TORRENT_FILE_BYTES {
        return Err(Error::InvalidTorrent(format!(
            "file is larger than {MAX_TORRENT_FILE_BYTES} bytes"
        )));
    }
    check_bencode_shape(bytes)?;
    let parsed = torrent_from_bytes(bytes)
        .map_err(|e| Error::InvalidTorrent(format!("cannot parse: {e}")))?;
    let info = &parsed.info.data;

    // numeric limits on the raw fields first: validate() sums lengths and divides by the piece
    // length, so it must only ever see numbers we already accepted
    let single = info.length;
    let lens: Vec<u64> = match (&info.files, single) {
        (Some(files), _) => files.iter().map(|f| f.length).collect(),
        (None, Some(l)) => vec![l],
        (None, None) => {
            return Err(Error::InvalidTorrent(
                "neither a length nor a file list".into(),
            ))
        }
    };
    check_numbers(
        info.piece_length,
        info.pieces.as_ref().len(),
        lens.into_iter(),
    )?;

    let validated = info
        .clone()
        .validate()
        .map_err(|e| Error::InvalidTorrent(e.to_string()))?;
    preview_from_info(
        parsed.info_hash.as_string(),
        &validated,
        bytes.to_vec(),
        exists,
    )
}

/// The name and the file paths exactly as the `.torrent` spells them (one list of components per
/// file, empty for a BEP 47 padding file). The planned layout of [`TorrentPreview`] is for the
/// disk; a web seed serves the files under the torrent's own names.
pub fn original_layout(bytes: &[u8]) -> Result<(String, Vec<Vec<String>>)> {
    inspect_torrent(bytes, &|_| false)?; // vets the data again
    let parsed = torrent_from_bytes(bytes)
        .map_err(|e| Error::InvalidTorrent(format!("cannot parse: {e}")))?;
    let info = parsed
        .info
        .data
        .clone()
        .validate()
        .map_err(|e| Error::InvalidTorrent(e.to_string()))?;
    let name = info
        .name()
        .map(|n| n.into_owned())
        .unwrap_or_else(|| "torrent".to_string());
    let files = info
        .iter_file_details()
        .map(|fd| {
            if fd.attrs().padding {
                Vec::new()
            } else {
                fd.filename
                    .iter_components()
                    .map(|c| c.into_owned())
                    .collect()
            }
        })
        .collect();
    Ok((name, files))
}

/// The address of one file on a web seed (BEP 19). A multi-file torrent is served under
/// `<seed>/<name>/<path...>`; a single-file torrent at the seed itself, or at `<seed>/<name>`
/// when the seed is a folder (ends with `/`). Every part is percent-encoded; empty, `.` and `..`
/// parts are dropped, so the address never leaves the seed's folder. `None` for a seed that is
/// not an `http(s)` address or carries credentials.
pub fn web_seed_url(
    seed: &str,
    name: &str,
    components: &[String],
    multi_file: bool,
) -> Option<String> {
    let mut url = url::Url::parse(seed.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    let folder = url.path().ends_with('/');
    if !multi_file && !folder {
        return Some(url.to_string());
    }
    let parts: Vec<&str> = if multi_file {
        std::iter::once(name)
            .chain(components.iter().map(String::as_str))
            .collect()
    } else {
        vec![name]
    };
    {
        let mut segs = url.path_segments_mut().ok()?;
        segs.pop_if_empty();
        for p in parts {
            if p.is_empty() || p == "." || p == ".." {
                continue;
            }
            segs.push(p);
        }
    }
    Some(url.to_string())
}

/// Reads a `.torrent` file from disk, refusing anything over the size cap.
pub fn read_torrent_file(path: &Path) -> Result<Vec<u8>> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_TORRENT_FILE_BYTES as u64 {
        return Err(Error::InvalidTorrent(format!(
            "file is larger than {MAX_TORRENT_FILE_BYTES} bytes"
        )));
    }
    Ok(std::fs::read(path)?)
}

/// Reads a `.torrent` file from disk (size-capped) and vets it.
pub fn inspect_torrent_file(path: &Path, exists: &dyn Fn(&Path) -> bool) -> Result<TorrentPreview> {
    inspect_torrent(&read_torrent_file(path)?, exists)
}

/// The info-hash of `.torrent` data, `None` when it does not parse. Used to tell a torrent's own
/// files from other torrents' before the full vetting.
pub fn info_hash_of_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_TORRENT_FILE_BYTES || check_bencode_shape(bytes).is_err() {
        return None;
    }
    torrent_from_bytes(bytes)
        .ok()
        .map(|t| t.info_hash.as_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_seed_urls_use_the_names_of_the_torrent_and_stay_in_the_seed_folder() {
        let c = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            web_seed_url(
                "https://m.example/dl/",
                "Item",
                &c(&["dir one", "f#1.mp4"]),
                true
            )
            .as_deref(),
            Some("https://m.example/dl/Item/dir%20one/f%231.mp4")
        );
        // the original names, not the sanitised ones: CON, a trailing dot, a duplicate
        assert_eq!(
            web_seed_url(
                "https://m.example/dl/",
                "set.",
                &c(&["CON", "a (1).txt"]),
                true
            )
            .as_deref(),
            Some("https://m.example/dl/set./CON/a%20(1).txt")
        );
        assert_eq!(
            web_seed_url("https://m.example/f/a.iso", "a.iso", &[], false).as_deref(),
            Some("https://m.example/f/a.iso")
        );
        assert_eq!(
            web_seed_url("https://m.example/f/", "a.iso", &[], false).as_deref(),
            Some("https://m.example/f/a.iso")
        );
        // never leaves the folder, never takes credentials or other schemes
        assert_eq!(
            web_seed_url(
                "https://m.example/f/",
                "x",
                &c(&["..", "..", "etc", "passwd"]),
                true
            )
            .as_deref(),
            Some("https://m.example/f/x/etc/passwd")
        );
        assert_eq!(
            web_seed_url("ftp://m.example/f/", "a", &c(&["b"]), true),
            None
        );
        assert_eq!(web_seed_url("javascript:alert(1)", "a", &[], true), None);
        assert_eq!(
            web_seed_url("https://u:p@m.example/f/", "a", &[], true),
            None
        );
        assert_eq!(web_seed_url("not a url", "a", &[], true), None);
    }

    #[test]
    fn bencode_shape_accepts_and_rejects() {
        assert!(check_bencode_shape(b"d3:foo3:bare").is_ok());
        assert!(check_bencode_shape(b"l4:spami42ee").is_ok());
        for bad in [
            &b""[..],
            b"d3:foo",
            b"i12",
            b"5:ab",
            b"x",
            b"de3:foo",
            b"e",
            b"99999999999999:a",
        ] {
            assert!(check_bencode_shape(bad).is_err(), "{:?}", bad);
        }
        let deep = [vec![b'l'; 10_000], vec![b'e'; 10_000]].concat();
        assert!(check_bencode_shape(&deep).is_err());
    }
}
