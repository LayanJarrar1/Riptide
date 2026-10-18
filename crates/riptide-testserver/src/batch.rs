//! The `/batch/<name>` item catalogue: which names exist and what bytes each one serves.
//!
//! Two families exist, matching the patterns the batch tests expand:
//! `file001.zip` ... `file120.zip` (exactly three digits) and `img_a.jpg` ... `img_z.jpg`.
//! Anything else is a `404`, so a test can ask for `file121.zip` and see a missing item.
//! A name maps to a fixed seed and a default size (4 KiB to about 260 KiB, so the items differ
//! in length), which [`item`] reports so tests can compute the expected SHA-256 locally.

use crate::content::splitmix64;

/// The bytes one batch item serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchItem {
    /// Content seed (see [`crate::bytes_at`]).
    pub seed: u64,
    /// Default size in bytes (`?size=` overrides it on the wire).
    pub size: u64,
}

/// FNV-1a over the name, finished with SplitMix64.
fn name_hash(name: &str) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    splitmix64(h)
}

/// Whether `name` is one of the served items.
#[must_use]
pub fn exists(name: &str) -> bool {
    if let Some(num) = name
        .strip_prefix("file")
        .and_then(|r| r.strip_suffix(".zip"))
    {
        return num.len() == 3
            && num.bytes().all(|b| b.is_ascii_digit())
            && matches!(num.parse::<u32>(), Ok(1..=120));
    }
    if let Some(letter) = name
        .strip_prefix("img_")
        .and_then(|r| r.strip_suffix(".jpg"))
    {
        return matches!(letter.as_bytes(), [b'a'..=b'z']);
    }
    false
}

/// The seed and default size of a served item, `None` outside the catalogue.
#[must_use]
pub fn item(name: &str) -> Option<BatchItem> {
    exists(name).then(|| {
        let h = name_hash(name);
        BatchItem {
            seed: h,
            size: 4 * 1024 + (h >> 20) % (256 * 1024),
        }
    })
}
