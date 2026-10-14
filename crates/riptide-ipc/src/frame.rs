//! Length-prefixed JSON framing: `u32 little-endian length` + JSON bytes.

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Error, Result};

/// Largest frame either side will send or accept (1 MiB). Matches the Native Messaging limit so
/// the host never has to split or buffer.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Serialises `value` into a complete frame (header + body).
pub fn encode_frame<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge {
            len: body.len(),
            max: MAX_FRAME_BYTES,
        });
    }
    let len = u32::try_from(body.len()).map_err(|_| Error::FrameTooLarge {
        len: body.len(),
        max: MAX_FRAME_BYTES,
    })?;
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Parses a frame body (without the header).
pub fn decode_frame<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge {
            len: body.len(),
            max: MAX_FRAME_BYTES,
        });
    }
    Ok(serde_json::from_slice(body)?)
}

/// Writes one frame and flushes.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize + ?Sized,
{
    let frame = encode_frame(value)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one frame. `Ok(None)` on a clean end of stream (no bytes of a header read);
/// [`Error::Io`] with `UnexpectedEof` if the stream ends mid-frame; [`Error::FrameTooLarge`] if
/// the header announces more than [`MAX_FRAME_BYTES`] (nothing further is read in that case).
pub async fn read_frame<R, T>(reader: &mut R) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut header = [0u8; 4];
    let mut filled = 0;
    while filled < 4 {
        let n = reader.read(&mut header[filled..]).await?;
        if n == 0 {
            if filled == 0 {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "stream ended inside a frame header",
            )
            .into());
        }
        filled += n;
    }
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge {
            len,
            max: MAX_FRAME_BYTES,
        });
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    decode_frame(&body).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Msg {
        a: u32,
        b: String,
    }

    #[test]
    fn header_is_little_endian_length() {
        let frame = encode_frame(&Msg {
            a: 1,
            b: "x".into(),
        })
        .unwrap();
        let body = br#"{"a":1,"b":"x"}"#;
        assert_eq!(&frame[..4], &(body.len() as u32).to_le_bytes());
        assert_eq!(&frame[4..], body);
        assert_eq!(
            decode_frame::<Msg>(body).unwrap(),
            Msg {
                a: 1,
                b: "x".into()
            }
        );
    }

    #[test]
    fn oversized_value_is_refused_before_writing() {
        let big = Msg {
            a: 0,
            b: "x".repeat(MAX_FRAME_BYTES + 1),
        };
        assert!(matches!(
            encode_frame(&big),
            Err(Error::FrameTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn round_trip_over_a_duplex_stream() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let writer = tokio::spawn(async move {
            for i in 0..3u32 {
                write_frame(
                    &mut a,
                    &Msg {
                        a: i,
                        b: "é".repeat(i as usize),
                    },
                )
                .await
                .unwrap();
            }
            // a is dropped here: clean EOF
        });
        let mut got = Vec::new();
        while let Some(m) = read_frame::<_, Msg>(&mut b).await.unwrap() {
            got.push(m);
        }
        writer.await.unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[2],
            Msg {
                a: 2,
                b: "éé".into()
            }
        );
    }

    #[tokio::test]
    async fn announced_oversize_is_refused_without_reading_body() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let _ = a
                .write_all(&(MAX_FRAME_BYTES as u32 + 1).to_le_bytes())
                .await;
        });
        let err = read_frame::<_, Msg>(&mut b).await.unwrap_err();
        assert!(matches!(err, Error::FrameTooLarge { .. }), "{err}");
    }

    #[tokio::test]
    async fn truncated_frame_is_unexpected_eof() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let _ = a.write_all(&[10, 0, 0, 0, b'{', b'"']).await;
        });
        let err = read_frame::<_, Msg>(&mut b).await.unwrap_err();
        match err {
            Error::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof),
            other => panic!("unexpected {other}"),
        }
    }

    #[tokio::test]
    async fn invalid_json_is_a_json_error() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let _ = a.write_all(&[3, 0, 0, 0, b'n', b'o', b'!']).await;
        });
        let err = read_frame::<_, Msg>(&mut b).await.unwrap_err();
        assert!(matches!(err, Error::Json(_)), "{err}");
    }
}
