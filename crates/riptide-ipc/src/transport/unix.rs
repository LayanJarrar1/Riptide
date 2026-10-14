//! Unix domain sockets with mode 0600 (macOS / Linux builds come later; kept compiling and
//! tested in CI once those targets are added).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use crate::{Error, Result};

/// The app's endpoint for this user: `$XDG_RUNTIME_DIR/riptide.sock`, falling back to
/// `<temp>/riptide-<user>.sock`.
pub fn default_endpoint() -> Result<String> {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) => PathBuf::from(d),
        None => std::env::temp_dir(),
    };
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .map_err(|_| Error::Endpoint("neither USER nor LOGNAME is set".into()))?;
    let name = if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        "riptide.sock".to_string()
    } else {
        format!("riptide-{user}.sock")
    };
    Ok(dir.join(name).to_string_lossy().into_owned())
}

/// A unique endpoint for tests, inside the temp dir.
pub fn test_endpoint(tag: &str) -> String {
    std::env::temp_dir()
        .join(format!("riptide-test-{}-{tag}.sock", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

/// Server side.
#[derive(Debug)]
pub struct Listener {
    endpoint: String,
    inner: UnixListener,
}

impl Listener {
    pub(crate) fn bind(endpoint: &str) -> Result<Listener> {
        let path = Path::new(endpoint);
        if path.exists() {
            // A stale socket from a crashed instance. If another instance is alive it will have
            // the single-instance lock; the pipe-level check is not our job here.
            std::fs::remove_file(path)?;
        }
        let inner = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener {
            endpoint: endpoint.to_string(),
            inner,
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) async fn accept(&mut self) -> Result<Stream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(stream)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.endpoint);
    }
}

/// Client side.
pub(crate) async fn connect(endpoint: &str) -> Result<Stream> {
    match UnixStream::connect(endpoint).await {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::NotRunning(endpoint.to_string()))
        }
        Err(e) => Err(e.into()),
    }
}

/// Either end of a socket.
pub type Stream = UnixStream;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_file_is_private() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let ep = test_endpoint("perm");
        let listener = Listener::bind(&ep).unwrap();
        let mode = std::fs::metadata(&ep).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(listener);
        assert!(!Path::new(&ep).exists(), "socket file removed on drop");
    }
}
