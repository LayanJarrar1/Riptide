//! Opening `magnet:` links and `.torrent` files with Riptide, per user and without elevation.
//!
//! Everything is written under `HKCU\Software\Classes` (never `HKLM`), only when the user presses
//! the button in *Settings -> Integration*, and removed again by the matching button:
//!
//! * `magnet` is made a URL protocol whose `shell\open\command` runs this executable with the
//!   link as its argument.
//! * `Riptide.Torrent` is a ProgID with the same command; it is listed under
//!   `.torrent\OpenWithProgids` (so Riptide shows up in "Open with") and becomes the `.torrent`
//!   default only when the user has no per-user default yet: another app's choice is never
//!   overwritten.
//! * Unregistering removes only what points at *this* executable, and only the values it added
//!   (the `.torrent` key itself stays: other apps keep their own entries in it).
//!
//! The registry sits behind [`Registry`], the same abstraction (and the same
//! `RIPTIDE_REGISTRY_ROOT` test knob) as the browser integration, so the logic is tested against
//! memory and, on Windows, against a throw-away key.
//!
//! What arrives on the command line is treated as untrusted text: a link must be a `magnet:?`
//! URI of at most 8 KiB, a file must end in `.torrent` and exist; anything else is ignored. The
//! link or path only ever opens the torrent dialog (the user still chooses the files); nothing is
//! executed, and the engine re-checks the torrent itself.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use riptide_core::TorrentSourceRequest;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::integration::{platform_registry, Registry};

/// Registry key of the `magnet:` URL protocol (relative to `HKCU`).
pub const MAGNET_KEY: &str = r"Software\Classes\magnet";
/// Registry key of the `.torrent` extension.
pub const TORRENT_EXT_KEY: &str = r"Software\Classes\.torrent";
/// ProgID Riptide registers for `.torrent` files.
pub const TORRENT_PROG_ID: &str = "Riptide.Torrent";
/// Tauri event carrying a [`TorrentSourceRequest`] to open (a second launch handed over a link).
pub const OPEN_EVENT: &str = "riptide://open-torrent";

/// Longest magnet link accepted from the command line.
const MAX_MAGNET_LEN: usize = 8 * 1024;

fn prog_id_key() -> String {
    format!(r"Software\Classes\{TORRENT_PROG_ID}")
}

fn command_key(root: &str) -> String {
    format!(r"{root}\shell\open\command")
}

/// The command Windows runs: the executable and the link or file as one quoted argument.
fn command_line(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

fn same_command(found: &str, expected: &str) -> bool {
    found.trim().eq_ignore_ascii_case(expected)
}

/// What Settings shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolStatus {
    /// There is a registry on this platform.
    pub supported: bool,
    /// `magnet:` links open this executable.
    pub magnet: bool,
    /// `.torrent` files offer this executable ("Open with") and, with no other per-user default,
    /// open it on double click.
    pub torrent: bool,
}

impl ProtocolStatus {
    /// What a platform without a registry reports.
    pub const fn unsupported() -> ProtocolStatus {
        ProtocolStatus {
            supported: false,
            magnet: false,
            torrent: false,
        }
    }
}

/// Which of the handlers point at `exe`.
pub fn status(exe: &Path, registry: &dyn Registry) -> ProtocolStatus {
    let cmd = command_line(exe);
    let magnet = registry
        .default_value(&command_key(MAGNET_KEY))
        .is_some_and(|c| same_command(&c, &cmd));
    let torrent = registry
        .default_value(&command_key(&prog_id_key()))
        .is_some_and(|c| same_command(&c, &cmd));
    ProtocolStatus {
        supported: true,
        magnet,
        torrent,
    }
}

fn io(what: &str, e: &std::io::Error) -> String {
    format!(r"{what}: {e}")
}

/// Registers `exe` for `magnet:` links and `.torrent` files (see the module docs).
///
/// # Errors
///
/// A message for the UI when a key cannot be written.
pub fn register(exe: &Path, registry: &mut dyn Registry) -> Result<ProtocolStatus, String> {
    let cmd = command_line(exe);
    let icon = format!("\"{}\",0", exe.display());

    registry
        .set_default_value(MAGNET_KEY, "URL:Magnet Protocol")
        .map_err(|e| io("cannot write the magnet key", &e))?;
    registry
        .set_named_value(MAGNET_KEY, "URL Protocol", "")
        .map_err(|e| io("cannot write the magnet key", &e))?;
    registry
        .set_default_value(&format!(r"{MAGNET_KEY}\DefaultIcon"), &icon)
        .map_err(|e| io("cannot write the magnet key", &e))?;
    registry
        .set_default_value(&command_key(MAGNET_KEY), &cmd)
        .map_err(|e| io("cannot write the magnet key", &e))?;

    let prog = prog_id_key();
    registry
        .set_default_value(&prog, "BitTorrent file")
        .map_err(|e| io("cannot write the torrent key", &e))?;
    registry
        .set_default_value(&format!(r"{prog}\DefaultIcon"), &icon)
        .map_err(|e| io("cannot write the torrent key", &e))?;
    registry
        .set_default_value(&command_key(&prog), &cmd)
        .map_err(|e| io("cannot write the torrent key", &e))?;
    registry
        .set_named_value(
            &format!(r"{TORRENT_EXT_KEY}\OpenWithProgids"),
            TORRENT_PROG_ID,
            "",
        )
        .map_err(|e| io("cannot write the torrent key", &e))?;
    // the default only when nobody has one: another app's choice stays
    if registry
        .default_value(TORRENT_EXT_KEY)
        .is_none_or(|v| v.trim().is_empty())
    {
        registry
            .set_default_value(TORRENT_EXT_KEY, TORRENT_PROG_ID)
            .map_err(|e| io("cannot write the torrent key", &e))?;
    }
    tracing::info!(exe = %exe.display(), "magnet and .torrent handlers registered");
    Ok(status(exe, registry))
}

/// Removes what points at `exe` and nothing else.
///
/// # Errors
///
/// A message for the UI when a key cannot be removed.
pub fn unregister(exe: &Path, registry: &mut dyn Registry) -> Result<ProtocolStatus, String> {
    let current = status(exe, registry);

    if current.magnet {
        registry
            .delete_key(MAGNET_KEY)
            .map_err(|e| io("cannot remove the magnet key", &e))?;
    }
    if current.torrent {
        registry
            .delete_key(&prog_id_key())
            .map_err(|e| io("cannot remove the torrent key", &e))?;
        registry
            .delete_named_value(
                &format!(r"{TORRENT_EXT_KEY}\OpenWithProgids"),
                TORRENT_PROG_ID,
            )
            .map_err(|e| io("cannot remove the torrent key", &e))?;
        if registry
            .default_value(TORRENT_EXT_KEY)
            .is_some_and(|v| v == TORRENT_PROG_ID)
        {
            registry
                .delete_named_value(TORRENT_EXT_KEY, "")
                .map_err(|e| io("cannot remove the torrent key", &e))?;
        }
    }
    tracing::info!("magnet and .torrent handlers removed");
    Ok(status(exe, registry))
}

/// The torrent a command line asks to open: the first argument that is a `magnet:?` link (at most
/// 8 KiB) or an existing file whose name ends in `.torrent`. Flags (`--...`) and everything else
/// are ignored.
pub fn open_target(args: &[String]) -> Option<TorrentSourceRequest> {
    args.iter().skip(1).find_map(|a| {
        let a = a.trim();
        if a.starts_with("--") || a.is_empty() {
            return None;
        }
        if a.len() <= MAX_MAGNET_LEN
            && a.get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("magnet:?"))
        {
            return Some(TorrentSourceRequest::Magnet { uri: a.to_string() });
        }
        let path = PathBuf::from(a);
        let is_torrent = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("torrent"));
        (is_torrent && path.is_file()).then_some(TorrentSourceRequest::File { path })
    })
}

/// A torrent that came with the first launch, waiting for the webview to ask for it.
#[derive(Default)]
pub struct PendingOpen(Mutex<Option<TorrentSourceRequest>>);

impl PendingOpen {
    /// Remembers a torrent for [`take_pending_open`].
    pub fn set(&self, target: TorrentSourceRequest) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(target);
        }
    }

    /// Hands the remembered torrent over once.
    pub fn take(&self) -> Option<TorrentSourceRequest> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// A second launch (the user double-clicked a `.torrent` or clicked a magnet link while Riptide
/// runs): the running window opens the torrent dialog.
pub fn handle_second_launch<R: Runtime>(app: &AppHandle<R>, args: &[String]) {
    let Some(target) = open_target(args) else {
        return;
    };
    if let Err(e) = app.emit(OPEN_EVENT, &target) {
        tracing::warn!(error = %e, "torrent hand-over not delivered");
        if let Some(pending) = app.try_state::<PendingOpen>() {
            pending.set(target);
        }
    }
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

async fn with_registry<T, F>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Path, &mut dyn Registry) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let exe = current_exe()?;
        match platform_registry() {
            Some(mut registry) => op(&exe, registry.as_mut()),
            None => Err("not supported on this platform".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether the handlers are registered (Settings -> Integration).
#[tauri::command]
pub async fn protocol_status() -> Result<ProtocolStatus, String> {
    match with_registry(|exe, registry| Ok(status(exe, registry))).await {
        Ok(s) => Ok(s),
        Err(_) => Ok(ProtocolStatus::unsupported()),
    }
}

/// Registers the handlers for the current user.
#[tauri::command]
pub async fn register_protocols() -> Result<ProtocolStatus, String> {
    with_registry(|exe, registry| register(exe, registry)).await
}

/// Removes the handlers this executable registered.
#[tauri::command]
pub async fn unregister_protocols() -> Result<ProtocolStatus, String> {
    with_registry(|exe, registry| unregister(exe, registry)).await
}

/// The torrent the app was launched with (a magnet link or a `.torrent` file), once.
#[tauri::command]
pub fn take_pending_open(pending: tauri::State<'_, PendingOpen>) -> Option<TorrentSourceRequest> {
    pending.take()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A registry in memory: `(key, value name)` to value, keys compared case-insensitively.
    #[derive(Default)]
    struct Memory(BTreeMap<(String, String), String>);

    fn k(key: &str) -> String {
        key.to_ascii_lowercase()
    }

    impl Registry for Memory {
        fn default_value(&self, key: &str) -> Option<String> {
            self.named_value(key, "")
        }
        fn set_default_value(&mut self, key: &str, value: &str) -> std::io::Result<()> {
            self.set_named_value(key, "", value)
        }
        fn delete_key(&mut self, key: &str) -> std::io::Result<()> {
            let prefix = k(key);
            self.0
                .retain(|(kk, _), _| kk != &prefix && !kk.starts_with(&format!("{prefix}\\")));
            Ok(())
        }
        fn named_value(&self, key: &str, name: &str) -> Option<String> {
            self.0.get(&(k(key), name.to_string())).cloned()
        }
        fn set_named_value(&mut self, key: &str, name: &str, value: &str) -> std::io::Result<()> {
            self.0.insert((k(key), name.to_string()), value.to_string());
            Ok(())
        }
        fn delete_named_value(&mut self, key: &str, name: &str) -> std::io::Result<()> {
            self.0.remove(&(k(key), name.to_string()));
            Ok(())
        }
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Apps\Riptide\riptide-desktop.exe")
    }

    #[test]
    fn registering_writes_the_per_user_keys_and_status_follows() {
        let mut r = Memory::default();
        assert_eq!(
            status(&exe(), &r),
            ProtocolStatus {
                supported: true,
                magnet: false,
                torrent: false
            }
        );
        let s = register(&exe(), &mut r).unwrap();
        assert!(s.magnet && s.torrent);
        assert_eq!(
            r.default_value(r"Software\Classes\magnet\shell\open\command")
                .as_deref(),
            Some(r#""C:\Apps\Riptide\riptide-desktop.exe" "%1""#)
        );
        assert_eq!(
            r.named_value(MAGNET_KEY, "URL Protocol").as_deref(),
            Some("")
        );
        assert_eq!(
            r.named_value(
                r"Software\Classes\.torrent\OpenWithProgids",
                TORRENT_PROG_ID
            )
            .as_deref(),
            Some("")
        );
        assert_eq!(
            r.default_value(TORRENT_EXT_KEY).as_deref(),
            Some(TORRENT_PROG_ID)
        );
        // everything is under Software\Classes of the current user
        assert!(r
            .0
            .keys()
            .all(|(key, _)| key.starts_with(r"software\classes")));
    }

    #[test]
    fn another_apps_torrent_default_is_never_overwritten_or_removed() {
        let mut r = Memory::default();
        r.set_default_value(TORRENT_EXT_KEY, "qBittorrent.File.torrent")
            .unwrap();
        r.set_named_value(
            r"Software\Classes\.torrent\OpenWithProgids",
            "qBittorrent.File.torrent",
            "",
        )
        .unwrap();
        register(&exe(), &mut r).unwrap();
        assert_eq!(
            r.default_value(TORRENT_EXT_KEY).as_deref(),
            Some("qBittorrent.File.torrent")
        );
        let s = unregister(&exe(), &mut r).unwrap();
        assert!(!s.magnet && !s.torrent);
        assert_eq!(
            r.default_value(TORRENT_EXT_KEY).as_deref(),
            Some("qBittorrent.File.torrent")
        );
        assert!(r
            .named_value(
                r"Software\Classes\.torrent\OpenWithProgids",
                "qBittorrent.File.torrent"
            )
            .is_some());
        assert!(r
            .named_value(
                r"Software\Classes\.torrent\OpenWithProgids",
                TORRENT_PROG_ID
            )
            .is_none());
    }

    #[test]
    fn unregistering_removes_ours_and_leaves_another_install_alone() {
        let mut r = Memory::default();
        register(&exe(), &mut r).unwrap();
        // a different install (a portable copy) is not touched by this one
        let other = PathBuf::from(r"D:\Portable\riptide-desktop.exe");
        let untouched = unregister(&other, &mut r).unwrap();
        assert!(
            !untouched.magnet && !untouched.torrent,
            "not registered for `other`"
        );
        assert!(r.default_value(MAGNET_KEY).is_some(), "left alone");
        // ours goes, including the .torrent default we set
        let s = unregister(&exe(), &mut r).unwrap();
        assert!(!s.magnet && !s.torrent);
        assert!(r.default_value(MAGNET_KEY).is_none());
        assert!(r.default_value(&prog_id_key()).is_none());
        assert!(r.default_value(TORRENT_EXT_KEY).is_none());
        assert!(r.0.is_empty(), "nothing left behind: {:?}", r.0);
    }

    #[test]
    fn a_registry_without_value_support_reports_an_error_not_a_panic() {
        struct Plain;
        impl Registry for Plain {
            fn default_value(&self, _: &str) -> Option<String> {
                None
            }
            fn set_default_value(&mut self, _: &str, _: &str) -> std::io::Result<()> {
                Ok(())
            }
            fn delete_key(&mut self, _: &str) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(register(&exe(), &mut Plain).is_err());
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_launch_with_a_magnet_link_opens_it() {
        let hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
        let link = format!("magnet:?xt=urn:btih:{hash}&dn=x");
        assert_eq!(
            open_target(&args(&["riptide-desktop.exe", &link])),
            Some(TorrentSourceRequest::Magnet { uri: link.clone() })
        );
        assert_eq!(
            open_target(&args(&["riptide-desktop.exe", "--minimized", &link])),
            Some(TorrentSourceRequest::Magnet { uri: link })
        );
    }

    #[test]
    fn a_launch_with_a_torrent_file_opens_it_only_if_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Big Buck Bunny.TORRENT");
        std::fs::write(&file, b"d4:infod4:name1:xee").unwrap();
        let path = file.to_string_lossy().into_owned();
        assert_eq!(
            open_target(&args(&["x.exe", &path])),
            Some(TorrentSourceRequest::File { path: file })
        );
        let missing = dir.path().join("gone.torrent");
        assert_eq!(
            open_target(&args(&["x.exe", &missing.to_string_lossy()])),
            None
        );
    }

    #[test]
    fn nothing_else_on_the_command_line_is_taken_for_a_torrent() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("notes.txt");
        std::fs::write(&other, b"x").unwrap();
        for bad in [
            "https://example.com/a.torrent",
            "magnet",
            "--magnet:?xt=urn:btih:abc",
            "C:\\Windows\\System32\\cmd.exe",
            &other.to_string_lossy(),
            "",
        ] {
            assert_eq!(open_target(&args(&["x.exe", bad])), None, "{bad:?}");
        }
        // the program name itself is never a candidate
        assert_eq!(open_target(&args(&["magnet:?xt=urn:btih:abc"])), None);
        // an oversized magnet link is refused
        let huge = format!("magnet:?xt=urn:btih:{}", "a".repeat(MAX_MAGNET_LEN));
        assert_eq!(open_target(&args(&["x.exe", &huge])), None);
    }

    #[test]
    fn the_pending_torrent_is_handed_over_once() {
        let p = PendingOpen::default();
        assert!(p.take().is_none());
        p.set(TorrentSourceRequest::Magnet {
            uri: "magnet:?x".into(),
        });
        assert!(p.take().is_some());
        assert!(p.take().is_none());
    }
}
