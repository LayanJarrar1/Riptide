//! Platform transport: named pipes on Windows, Unix domain sockets elsewhere. Both are wrapped in
//! the same [`Listener`] / [`Connection`] types so the rest of the crate is platform-free.

use std::pin::Pin;
use std::task::{Context, Poll};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::frame::{read_frame, write_frame};
use crate::Result;

#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

#[cfg(unix)]
#[path = "unix.rs"]
mod platform;

pub use platform::{default_endpoint, test_endpoint};

#[cfg(windows)]
pub use platform::current_user_sid;

/// Accepts connections on one endpoint. Dropping it closes the endpoint.
#[derive(Debug)]
pub struct Listener {
    inner: platform::Listener,
}

impl Listener {
    /// Creates the endpoint. On Windows the pipe's DACL admits only the current user and remote
    /// clients are rejected; on Unix a stale socket file is removed first and the new one gets
    /// mode `0600`.
    pub fn bind(endpoint: &str) -> Result<Listener> {
        Ok(Listener {
            inner: platform::Listener::bind(endpoint)?,
        })
    }

    /// The endpoint this listener serves.
    pub fn endpoint(&self) -> &str {
        self.inner.endpoint()
    }

    /// Waits for the next client.
    pub async fn accept(&mut self) -> Result<Connection> {
        Ok(Connection {
            inner: self.inner.accept().await?,
        })
    }
}

/// Connects to a running server.
pub async fn connect(endpoint: &str) -> Result<Connection> {
    Ok(Connection {
        inner: platform::connect(endpoint).await?,
    })
}

/// One client/server connection. Implements `AsyncRead` + `AsyncWrite`; [`Connection::send`] /
/// [`Connection::recv`] add the framing.
#[derive(Debug)]
pub struct Connection {
    inner: platform::Stream,
}

impl Connection {
    /// Sends one frame.
    pub async fn send<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        write_frame(&mut self.inner, value).await
    }

    /// Receives one frame; `None` when the peer closed the connection cleanly.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        read_frame(&mut self.inner).await
    }

    /// Splits into independent read and write halves (for a reader task and a writer task).
    pub fn split(
        self,
    ) -> (
        tokio::io::ReadHalf<Connection>,
        tokio::io::WriteHalf<Connection>,
    ) {
        tokio::io::split(self)
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
