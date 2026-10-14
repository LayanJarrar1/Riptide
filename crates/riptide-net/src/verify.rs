//! Post-completion checksum, computed by reading the file back on a blocking thread.

use std::io::Read;
use std::path::{Path, PathBuf};

use md5::Md5;
use riptide_core::{Checksum, ChecksumAlgo};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Hex digest of the file at `path` with `algo`. Runs on the blocking pool; the read is the
/// disk's job, not the runtime's.
pub async fn checksum_of(path: PathBuf, algo: ChecksumAlgo) -> Result<String> {
    tokio::task::spawn_blocking(move || checksum_blocking(&path, algo))
        .await
        .map_err(|e| Error::Protocol(format!("checksum task failed: {e}")))?
}

/// Synchronous variant for callers already on a blocking thread.
pub fn checksum_blocking(path: &Path, algo: ChecksumAlgo) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1024 * 1024];
    match algo {
        ChecksumAlgo::Md5 => hash_with(Md5::new(), &mut file, &mut buf),
        ChecksumAlgo::Sha1 => hash_with(Sha1::new(), &mut file, &mut buf),
        ChecksumAlgo::Sha256 => hash_with(Sha256::new(), &mut file, &mut buf),
    }
}

fn hash_with<D: Digest>(mut hasher: D, file: &mut std::fs::File, buf: &mut [u8]) -> Result<String> {
    loop {
        let n = file.read(buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Compares the file against `expected`; `Ok(actual)` on a match, [`Error::ChecksumMismatch`]
/// otherwise.
pub async fn verify_file(path: PathBuf, expected: &Checksum) -> Result<String> {
    let actual = checksum_of(path, expected.algo).await?;
    if actual.eq_ignore_ascii_case(&expected.hex) {
        Ok(actual)
    } else {
        Err(Error::ChecksumMismatch {
            algo: expected.algo,
            expected: expected.hex.clone(),
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn known_digests() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            checksum_of(path.clone(), ChecksumAlgo::Md5).await.unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            checksum_of(path.clone(), ChecksumAlgo::Sha1).await.unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            checksum_of(path.clone(), ChecksumAlgo::Sha256)
                .await
                .unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let ok = Checksum::new(
            ChecksumAlgo::Sha256,
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
        )
        .unwrap();
        assert!(verify_file(path.clone(), &ok).await.is_ok());
        let bad = Checksum::new(ChecksumAlgo::Md5, &"0".repeat(32)).unwrap();
        assert!(matches!(
            verify_file(path, &bad).await,
            Err(Error::ChecksumMismatch { .. })
        ));
    }
}
