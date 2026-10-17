//! The Native Messaging host `app.riptide.host`: the only door from the browser extension into
//! the Riptide app (hard rule 5, "the browser can only ask").
//!
//! The browser starts `riptide-host` with the caller's identity on the command line and talks to
//! it over stdio; the host checks the caller, checks every message and forwards the survivors to
//! the app over the local IPC endpoint (`riptide-ipc`, named pipe / Unix socket):
//!
//! ```text
//!  extension ──stdio (u32 LE length + JSON)──► riptide-host ──riptide-ipc pipe──► app (Engine)
//! ```
//!
//! * **Framing.** Both directions are a 4-byte little-endian length followed by that many bytes
//!   of UTF-8 JSON, at most [`MAX_FRAME_BYTES`] (1 MiB, the browser's own limit for messages to
//!   an extension). A larger announced frame gets one `error` frame and the host exits.
//! * **Caller.** Chrome and Edge pass the origin `chrome-extension://<id>/` as the first
//!   argument; Firefox passes the path of the host manifest, then the extension id. The id must
//!   be on the [`AllowList`]: the published ids ([`CHROMIUM_EXTENSION_ID`],
//!   [`FIREFOX_EXTENSION_ID`]) plus `extra_extension_ids` from the app's [`INTEGRATION_FILE`].
//!   Anyone else gets one `error` frame and the host exits.
//! * **Schema.** Only [`BROWSER_REQUEST_TYPES`] are accepted (`ping`, `probe`, `add`, `refresh`
//!   and the read-only `list`), and only if they parse as a [`riptide_ipc::Request`]. Everything
//!   else is answered with an `error` (`bad_request`) and the session continues.
//! * **App not running.** Requests get an `error` (`internal`, "the Riptide app is not
//!   running"); the session continues. A `ping` first tries to start the app (see [`Launcher`])
//!   and waits up to [`LAUNCH_WAIT`] for its endpoint.
//!
//! Nothing the browser sends is executed, shelled, logged or used as a path: the manifest path
//! Firefox passes is ignored, the app executable is looked up only next to the host and in the
//! per-user install folder, and request payloads go to the app as data.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![deny(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use riptide_ipc::{
    connect, read_frame, write_frame, Connection, ErrorCode, Request, RequestBody, Response,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

pub use riptide_ipc::MAX_FRAME_BYTES;

/// Name the host is registered under (the `name` field of both manifests).
pub const HOST_NAME: &str = "app.riptide.host";

/// Id of the published Chrome / Edge extension (derived from the `key` in its manifest).
pub const CHROMIUM_EXTENSION_ID: &str = "dcoeaodjhaibfchhcbfdkekpcacofnhm";

/// Id of the Firefox extension (`browser_specific_settings.gecko.id`).
pub const FIREFOX_EXTENSION_ID: &str = "riptide@riptide.app";

/// The integration settings file inside the data directory; see [`IntegrationSettings`].
pub const INTEGRATION_FILE: &str = "integration.json";

/// File name of the host executable (next to the app executable).
pub const HOST_EXE: &str = if cfg!(windows) {
    "riptide-host.exe"
} else {
    "riptide-host"
};

/// File names the app executable may have, in order of preference: the cargo name (portable
/// zip, `mainBinaryName` of the installer) and the product name.
pub const APP_EXE_NAMES: &[&str] = if cfg!(windows) {
    &["riptide-desktop.exe", "Riptide.exe"]
} else {
    &["riptide-desktop", "riptide"]
};

/// Request types the host forwards from the browser. `list` is read-only and lets the popup
/// show live states; everything else (pause, remove, shutdown, ...) is for the CLI only.
pub const BROWSER_REQUEST_TYPES: &[&str] = &["ping", "probe", "add", "refresh", "list"];

/// How long a `ping` waits for the app's endpoint after starting the app.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(10);

/// Environment override of the IPC endpoint (tests and side-by-side development builds).
pub const ENDPOINT_ENV: &str = "RIPTIDE_IPC_ENDPOINT";

/// Environment switch that stops the host from ever starting the app (tests).
pub const NO_LAUNCH_ENV: &str = "RIPTIDE_HOST_NO_LAUNCH";

/// Environment override of the data directory (integration file and logs; tests).
pub const DATA_DIR_ENV: &str = "RIPTIDE_HOST_DATA_DIR";

/// Message of the `error` answered while the app's endpoint is down. The extension maps an
/// `error` in answer to `ping` to "app not running".
pub const NOT_RUNNING_MESSAGE: &str = "the Riptide app is not running";

/// Message of the `error` sent to a caller that is not on the allow-list.
pub const FORBIDDEN_MESSAGE: &str = "this extension is not allowed to use the Riptide host";

/// Who started the host, as the browser told it on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// Chrome or Edge: the id from `chrome-extension://<id>/`.
    Chromium(String),
    /// Firefox: the add-on id.
    Firefox(String),
}

impl Caller {
    /// The extension id.
    pub fn id(&self) -> &str {
        match self {
            Caller::Chromium(id) | Caller::Firefox(id) => id,
        }
    }
}

/// Reads the caller from the command line (without `argv[0]`).
///
/// * Chrome / Edge: `chrome-extension://<id>/` (followed by `--parent-window=<hwnd>` on
///   Windows).
/// * Firefox: `<path to the host manifest> <extension id>`; the path is ignored.
///
/// Returns `None` when neither form is present or the id is not well formed.
pub fn parse_caller(args: &[String]) -> Option<Caller> {
    if let Some(origin) = args.iter().find(|a| a.starts_with("chrome-extension://")) {
        let id = origin
            .trim_start_matches("chrome-extension://")
            .trim_end_matches('/');
        return is_chromium_id(id).then(|| Caller::Chromium(id.to_string()));
    }
    match args {
        [_manifest, id, ..] if is_firefox_id(id) => Some(Caller::Firefox(id.clone())),
        _ => None,
    }
}

/// A Chromium extension id: 32 characters `a`–`p`.
pub fn is_chromium_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// A Firefox add-on id: `name@domain` or `{guid}`, at most 80 characters of
/// `[A-Za-z0-9@._{}+-]`.
pub fn is_firefox_id(id: &str) -> bool {
    let charset = id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"@._{}+-".contains(&b));
    let shaped = id.contains('@') || (id.starts_with('{') && id.ends_with('}'));
    !id.is_empty() && id.len() <= 80 && charset && shaped
}

/// `<data_dir>/integration.json`, written by the app (*Settings → Integration*) and read by
/// both the app and the host. Missing or unreadable means the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationSettings {
    /// Register the host with the browsers on start (the default). The app sets it to `false`
    /// when the user turns the integration off, so it is not re-registered behind their back.
    pub browser_host: bool,
    /// Ids of extra extension builds to accept (store builds with their own ids). Chromium ids
    /// and Firefox ids may be mixed; malformed entries are ignored.
    pub extra_extension_ids: Vec<String>,
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        IntegrationSettings {
            browser_host: true,
            extra_extension_ids: Vec::new(),
        }
    }
}

impl IntegrationSettings {
    /// Reads the file; the defaults when it is missing or malformed.
    pub fn load(path: &Path) -> IntegrationSettings {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "integration settings unreadable; using defaults");
                IntegrationSettings::default()
            }),
            Err(_) => IntegrationSettings::default(),
        }
    }

    /// Writes the file (pretty JSON), creating its folder.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Chromium ids to accept: the published one plus the well-formed extras.
    pub fn chromium_ids(&self) -> Vec<String> {
        let mut ids = vec![CHROMIUM_EXTENSION_ID.to_string()];
        for id in &self.extra_extension_ids {
            if is_chromium_id(id) && !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// Firefox ids to accept: the published one plus the well-formed extras.
    pub fn firefox_ids(&self) -> Vec<String> {
        let mut ids = vec![FIREFOX_EXTENSION_ID.to_string()];
        for id in &self.extra_extension_ids {
            if !is_chromium_id(id) && is_firefox_id(id) && !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }
}

/// The extensions allowed to talk to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowList {
    chromium: Vec<String>,
    firefox: Vec<String>,
}

impl AllowList {
    /// The published ids plus the extras in `settings`.
    pub fn new(settings: &IntegrationSettings) -> AllowList {
        AllowList {
            chromium: settings.chromium_ids(),
            firefox: settings.firefox_ids(),
        }
    }

    /// Whether `caller` may use the host.
    pub fn allows(&self, caller: &Caller) -> bool {
        match caller {
            Caller::Chromium(id) => self.chromium.iter().any(|a| a == id),
            Caller::Firefox(id) => self.firefox.iter().any(|a| a == id),
        }
    }
}

/// Validates one frame from the browser: a JSON object whose `type` is in
/// [`BROWSER_REQUEST_TYPES`] and that parses as a [`Request`]. The `Err` is the `error` response
/// to send back (with the frame's `id` when it has one).
#[allow(clippy::result_large_err)]
pub fn check_request(frame: &Value) -> Result<Request, Response> {
    let id = frame.get("id").and_then(Value::as_u64).unwrap_or(0);
    let Some(object) = frame.as_object() else {
        return Err(Response::error(
            0,
            ErrorCode::BadRequest,
            "request is not a JSON object",
        ));
    };
    let kind = object.get("type").and_then(Value::as_str).unwrap_or("");
    if !BROWSER_REQUEST_TYPES.contains(&kind) {
        return Err(Response::error(
            id,
            ErrorCode::BadRequest,
            "request type not accepted from the browser",
        ));
    }
    serde_json::from_value::<Request>(frame.clone()).map_err(|e| {
        Response::error(
            id,
            ErrorCode::BadRequest,
            format!("malformed {kind} request: {e}"),
        )
    })
}

/// Starts the app when a `ping` finds it not running.
#[derive(Debug, Clone)]
pub struct Launcher {
    candidates: Vec<PathBuf>,
}

impl Launcher {
    /// Looks for the app next to the host executable, then in the per-user install folder
    /// (`%LOCALAPPDATA%\Riptide`). `None` when [`NO_LAUNCH_ENV`] is set.
    pub fn for_this_host() -> Option<Launcher> {
        if std::env::var_os(NO_LAUNCH_ENV).is_some() {
            return None;
        }
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            dirs.push(dir);
        }
        if let Some(local) = dirs::data_local_dir() {
            dirs.push(local.join("Riptide"));
        }
        Some(Launcher::in_dirs(&dirs))
    }

    /// A launcher that looks for [`APP_EXE_NAMES`] in `dirs`, in order.
    pub fn in_dirs(dirs: &[PathBuf]) -> Launcher {
        let candidates = dirs
            .iter()
            .flat_map(|d| APP_EXE_NAMES.iter().map(move |n| d.join(n)))
            .collect();
        Launcher { candidates }
    }

    /// The executable that would be started, if any exists.
    pub fn app_path(&self) -> Option<&Path> {
        self.candidates
            .iter()
            .find(|p| p.is_file())
            .map(PathBuf::as_path)
    }

    /// Starts the app detached from the browser (no inherited stdio, own process group, out of
    /// the browser's job when allowed). `false` when there is no app or it cannot start.
    pub fn spawn(&self) -> bool {
        let Some(path) = self.app_path() else {
            tracing::warn!("no Riptide app executable found to start");
            return false;
        };
        match spawn_detached(path) {
            Ok(()) => {
                tracing::info!(app = %path.display(), "started the Riptide app");
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, app = %path.display(), "cannot start the Riptide app");
                false
            }
        }
    }
}

#[cfg(windows)]
fn spawn_detached(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let command = |flags: u32| {
        let mut c = std::process::Command::new(path);
        c.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags);
        if let Some(dir) = path.parent() {
            c.current_dir(dir);
        }
        c
    };
    // `CreateProcess` runs with handle inheritance on, and the browser created our stdio pipes
    // inheritable: the app would keep the browser's pipe open and the port would never close.
    no_inherit::std_handles();
    // The browser runs hosts in a job object; breaking away keeps the app alive when the
    // browser closes the port. Jobs that forbid breakaway make that flag fail: retry without.
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    match command(base | CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Ok(_) => Ok(()),
        Err(_) => command(base).spawn().map(|_| ()),
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod no_inherit {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};

    /// Marks this process's stdin / stdout / stderr as not inheritable.
    pub(super) fn std_handles() {
        let handles = [
            std::io::stdin().as_raw_handle(),
            std::io::stdout().as_raw_handle(),
            std::io::stderr().as_raw_handle(),
        ];
        for handle in handles {
            if handle.is_null() {
                continue;
            }
            // SAFETY: a handle owned by this process for its whole lifetime (std's stdio);
            // only its inherit flag changes. A failure (no such handle) is harmless.
            unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

#[cfg(not(windows))]
fn spawn_detached(path: &Path) -> std::io::Result<()> {
    let mut c = std::process::Command::new(path);
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = path.parent() {
        c.current_dir(dir);
    }
    c.spawn().map(|_| ())
}

/// The app side of a session: one lazily opened IPC connection, re-opened when the app
/// restarted, and the optional [`Launcher`].
#[derive(Debug)]
pub struct Bridge {
    endpoint: String,
    conn: Option<Connection>,
    launcher: Option<Launcher>,
    launched_at: Option<Instant>,
    launch_wait: Duration,
}

impl Bridge {
    /// A bridge to `endpoint`; `launcher` is used by `ping` when the endpoint is down.
    pub fn new(endpoint: impl Into<String>, launcher: Option<Launcher>) -> Bridge {
        Bridge {
            endpoint: endpoint.into(),
            conn: None,
            launcher,
            launched_at: None,
            launch_wait: LAUNCH_WAIT,
        }
    }

    /// Overrides [`LAUNCH_WAIT`] (tests).
    pub fn with_launch_wait(mut self, wait: Duration) -> Bridge {
        self.launch_wait = wait;
        self
    }

    /// Answers one frame from the browser: validated, forwarded, or refused.
    pub async fn handle(&mut self, frame: &Value) -> Response {
        let request = match check_request(frame) {
            Ok(r) => r,
            Err(refusal) => return refusal,
        };
        match self.forward(&request).await {
            Ok(response) => return response,
            Err(e) => tracing::debug!(error = %e, "app endpoint unavailable"),
        }
        if matches!(request.body, RequestBody::Ping) && self.start_app_and_wait().await {
            if let Ok(response) = self.forward(&request).await {
                return response;
            }
        }
        Response::error(request.id, ErrorCode::Internal, NOT_RUNNING_MESSAGE)
    }

    /// Sends `request` and waits for its answer. A connection that turns out stale on send
    /// (the app restarted) is replaced once; a failure after the request went out is not
    /// retried, so an `add` is never sent twice.
    async fn forward(&mut self, request: &Request) -> riptide_ipc::Result<Response> {
        for attempt in 0..2 {
            let fresh = self.conn.is_none();
            if fresh {
                self.conn = Some(connect(&self.endpoint).await?);
            }
            let Some(conn) = self.conn.as_mut() else {
                continue;
            };
            if let Err(e) = conn.send(request).await {
                self.conn = None;
                if fresh || attempt == 1 {
                    return Err(e);
                }
                continue;
            }
            return match conn.recv::<Response>().await {
                Ok(Some(response)) => Ok(response),
                Ok(None) => {
                    self.conn = None;
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "the app closed the connection",
                    )
                    .into())
                }
                Err(e) => {
                    self.conn = None;
                    Err(e)
                }
            };
        }
        Err(riptide_ipc::Error::NotRunning(self.endpoint.clone()))
    }

    /// Starts the app (at most once per [`LAUNCH_WAIT`]) and waits for its endpoint. `true`
    /// once a connection is open.
    async fn start_app_and_wait(&mut self) -> bool {
        let Some(launcher) = &self.launcher else {
            return false;
        };
        let recently = self
            .launched_at
            .is_some_and(|t| t.elapsed() < self.launch_wait);
        if !recently {
            if !launcher.spawn() {
                return false;
            }
            self.launched_at = Some(Instant::now());
        }
        let deadline = tokio::time::Instant::now() + self.launch_wait;
        while tokio::time::Instant::now() < deadline {
            if let Ok(conn) = connect(&self.endpoint).await {
                self.conn = Some(conn);
                return true;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        false
    }
}

/// How a stdio session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEnd {
    /// The browser closed the port (stdin reached end of file).
    Closed,
    /// The caller is not on the allow-list (or did not identify itself).
    Forbidden,
    /// A frame larger than [`MAX_FRAME_BYTES`] was announced.
    Oversize,
    /// stdin / stdout failed.
    Io,
}

impl SessionEnd {
    /// Process exit code: 0 closed, 2 forbidden, 3 oversize, 1 IO.
    pub fn exit_code(self) -> u8 {
        match self {
            SessionEnd::Closed => 0,
            SessionEnd::Io => 1,
            SessionEnd::Forbidden => 2,
            SessionEnd::Oversize => 3,
        }
    }
}

/// Runs one Native Messaging session on `input` / `output` (stdin / stdout in the binary):
/// checks the caller, then answers frames until the browser closes the port.
pub async fn run_session<R, W>(
    input: &mut R,
    output: &mut W,
    caller: Option<&Caller>,
    allow: &AllowList,
    bridge: &mut Bridge,
) -> SessionEnd
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    match caller {
        Some(c) if allow.allows(c) => tracing::info!(caller = c.id(), "browser session started"),
        other => {
            tracing::warn!(caller = ?other.map(Caller::id), "refused a caller");
            let refusal = Response::error(0, ErrorCode::BadRequest, FORBIDDEN_MESSAGE);
            let _ = write_frame(output, &refusal).await;
            return SessionEnd::Forbidden;
        }
    }
    loop {
        let response = match read_frame::<_, Value>(input).await {
            Ok(None) => return SessionEnd::Closed,
            Ok(Some(frame)) => bridge.handle(&frame).await,
            Err(riptide_ipc::Error::FrameTooLarge { len, max }) => {
                let refusal = Response::error(
                    0,
                    ErrorCode::BadRequest,
                    format!("frame of {len} bytes exceeds the {max}-byte limit"),
                );
                let _ = write_frame(output, &refusal).await;
                return SessionEnd::Oversize;
            }
            // the body was read in full, so the stream is still in step: answer and go on
            Err(riptide_ipc::Error::Json(_)) => {
                Response::error(0, ErrorCode::BadRequest, "frame is not valid JSON")
            }
            Err(e) => {
                tracing::debug!(error = %e, "stdin closed");
                return SessionEnd::Io;
            }
        };
        let written = match write_frame(output, &response).await {
            // nothing was written; the browser would refuse a frame this large anyway
            Err(riptide_ipc::Error::FrameTooLarge { .. }) => {
                let refusal = Response::error(
                    response.id,
                    ErrorCode::Internal,
                    "the answer is larger than the browser accepts",
                );
                write_frame(output, &refusal).await
            }
            other => other,
        };
        if let Err(e) = written {
            tracing::debug!(error = %e, "stdout closed");
            return SessionEnd::Io;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn chrome_and_firefox_callers_are_recognised() {
        assert_eq!(
            parse_caller(&args(&[
                "chrome-extension://dcoeaodjhaibfchhcbfdkekpcacofnhm/",
                "--parent-window=0"
            ])),
            Some(Caller::Chromium(CHROMIUM_EXTENSION_ID.into()))
        );
        assert_eq!(
            parse_caller(&args(&[
                r"C:\Users\x\AppData\Local\Riptide\native-messaging\app.riptide.host.firefox.json",
                "riptide@riptide.app"
            ])),
            Some(Caller::Firefox(FIREFOX_EXTENSION_ID.into()))
        );
        assert_eq!(parse_caller(&args(&[])), None);
        assert_eq!(parse_caller(&args(&["chrome-extension://short/"])), None);
        assert_eq!(parse_caller(&args(&["only-one-arg"])), None);
        assert_eq!(parse_caller(&args(&["m.json", "rm -rf /"])), None);
    }

    #[test]
    fn allow_list_has_the_published_ids_and_well_formed_extras() {
        let settings = IntegrationSettings {
            browser_host: true,
            extra_extension_ids: vec![
                "abcdefghijklmnopabcdefghijklmnop".into(),
                "{01234567-89ab-cdef-0123-456789abcdef}".into(),
                "not an id".into(),
            ],
        };
        let allow = AllowList::new(&settings);
        assert!(allow.allows(&Caller::Chromium(CHROMIUM_EXTENSION_ID.into())));
        assert!(allow.allows(&Caller::Firefox(FIREFOX_EXTENSION_ID.into())));
        assert!(allow.allows(&Caller::Chromium("abcdefghijklmnopabcdefghijklmnop".into())));
        assert!(allow.allows(&Caller::Firefox(
            "{01234567-89ab-cdef-0123-456789abcdef}".into()
        )));
        assert!(!allow.allows(&Caller::Firefox("not an id".into())));
        assert!(!allow.allows(&Caller::Chromium("pppppppppppppppppppppppppppppppp".into())));
        // a Chromium id is not accepted as a Firefox id and vice versa
        assert!(!allow.allows(&Caller::Firefox(CHROMIUM_EXTENSION_ID.into())));
    }

    #[test]
    fn integration_settings_default_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(INTEGRATION_FILE);
        assert_eq!(
            IntegrationSettings::load(&path),
            IntegrationSettings::default()
        );
        assert!(IntegrationSettings::default().browser_host);
        let s = IntegrationSettings {
            browser_host: false,
            extra_extension_ids: vec!["x@y".into()],
        };
        s.save(&path).unwrap();
        assert_eq!(IntegrationSettings::load(&path), s);
        std::fs::write(&path, b"{ nope").unwrap();
        assert_eq!(
            IntegrationSettings::load(&path),
            IntegrationSettings::default()
        );
    }

    #[test]
    fn only_browser_request_types_pass() {
        let ok = check_request(&serde_json::json!({"id": 3, "type": "ping"})).unwrap();
        assert_eq!(ok.id, 3);
        for kind in ["pause", "remove", "shutdown", "subscribe", "get", "exec"] {
            let refusal =
                check_request(&serde_json::json!({"id": 4, "type": kind, "download_id": 1}))
                    .unwrap_err();
            assert_eq!(refusal.id, 4);
            assert!(matches!(
                refusal.body,
                riptide_ipc::ResponseBody::Error {
                    code: ErrorCode::BadRequest,
                    ..
                }
            ));
        }
        let malformed =
            check_request(&serde_json::json!({"id": 5, "type": "add", "request": 7})).unwrap_err();
        assert_eq!(malformed.id, 5);
        let not_object = check_request(&serde_json::json!([1, 2])).unwrap_err();
        assert_eq!(not_object.id, 0);
    }

    #[test]
    fn launcher_finds_nothing_in_an_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = Launcher::in_dirs(&[dir.path().to_path_buf()]);
        assert!(launcher.app_path().is_none());
        assert!(!launcher.spawn());
    }
}
