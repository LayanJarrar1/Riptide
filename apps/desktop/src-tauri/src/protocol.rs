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
//! What arrives on the command line is treated as untrusted text. The registered command is
//! `"exe" --open-link -- "%1"`: after the marker and the `--` there is exactly **one** argument,
//! and it is data, never a flag. A launcher that does not escape a `"` inside the link could split
//! it into several arguments (`... " --unregister-host`); such a command line has more than one
//! data argument and is refused as a whole, and an open launch never runs the installer hooks
//! (`--register-host`, `--unregister-host`) or honours `--log-level`. A link must be a `magnet:?`
//! URI of at most 8 KiB without quotes or control characters, a file must end in `.torrent` and
//! exist; anything else is ignored. The link or path only ever opens the torrent dialog (the user
//! still chooses the files); nothing is executed, and the engine re-checks the torrent itself.

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
/// Where Riptide keeps the `magnet:` command another app had, to give it back on unregister.
pub const PREVIOUS_COMMAND_VALUE: &str = "Riptide.PreviousCommand";
/// ProgID Riptide registers for `.torrent` files.
pub const TORRENT_PROG_ID: &str = "Riptide.Torrent";
/// Tauri event carrying a [`TorrentSourceRequest`] to open (a second launch handed over a link).
pub const OPEN_EVENT: &str = "riptide://open-torrent";

/// Longest magnet link accepted from the command line.
const MAX_MAGNET_LEN: usize = 8 * 1024;
/// Longest `.torrent` path accepted from the command line (the Windows limit is 32 767).
const MAX_PATH_LEN: usize = 32 * 1024;
/// First argument of a launch that opens a link or file; everything after the following `--` is
/// data.
pub const OPEN_MARKER: &str = "--open-link";

fn prog_id_key() -> String {
    format!(r"Software\Classes\{TORRENT_PROG_ID}")
}

fn command_key(root: &str) -> String {
    format!(r"{root}\shell\open\command")
}

/// The command Windows runs: the executable, the marker, `--` and the link or file as one quoted
/// argument.
fn command_line(exe: &Path) -> String {
    format!("\"{}\" {OPEN_MARKER} -- \"%1\"", exe.display())
}

/// A command that starts one of Riptide's own executables (any install).
fn is_riptide_command(cmd: &str) -> bool {
    cmd.to_ascii_lowercase().contains("riptide")
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

    // another app's `magnet:` handler is remembered (once), so unregistering can give it back
    if let Some(previous) = registry.default_value(&command_key(MAGNET_KEY)) {
        let has_backup = registry
            .named_value(MAGNET_KEY, PREVIOUS_COMMAND_VALUE)
            .is_some();
        if !has_backup && !previous.trim().is_empty() && !is_riptide_command(&previous) {
            registry
                .set_named_value(MAGNET_KEY, PREVIOUS_COMMAND_VALUE, &previous)
                .map_err(|e| io("cannot write the magnet key", &e))?;
        }
    }
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
        let previous = registry
            .named_value(MAGNET_KEY, PREVIOUS_COMMAND_VALUE)
            .filter(|c| !c.trim().is_empty() && !same_command(c, &command_line(exe)));
        registry
            .delete_key(MAGNET_KEY)
            .map_err(|e| io("cannot remove the magnet key", &e))?;
        // the handler that was there before Riptide is put back
        if let Some(previous) = previous {
            registry
                .set_default_value(MAGNET_KEY, "URL:Magnet Protocol")
                .and_then(|()| registry.set_named_value(MAGNET_KEY, "URL Protocol", ""))
                .and_then(|()| registry.set_default_value(&command_key(MAGNET_KEY), &previous))
                .map_err(|e| io("cannot restore the previous magnet handler", &e))?;
        }
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

fn looks_like_target(a: &str) -> bool {
    a.get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("magnet:?"))
        || Path::new(a)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
}

/// Whether this command line (program name first) asks to open a link or file, as opposed to
/// being an ordinary launch or an installer hook. Such a launch carries no flags at all: the
/// caller must not honour `--register-host`, `--unregister-host` or `--log-level` in it.
pub fn is_open_launch(args: &[String]) -> bool {
    match args.get(1) {
        Some(a) => a == OPEN_MARKER || (!a.starts_with("--") && looks_like_target(a)),
        None => false,
    }
}

/// The one argument that carries the link or file of an open launch; `None` when the command
/// line is not a well-formed open launch (extra arguments mean a link that was split).
fn open_data(args: &[String]) -> Option<&str> {
    match args.get(1).map(String::as_str)? {
        OPEN_MARKER => (args.len() == 4 && args[2] == "--").then(|| args[3].as_str()),
        // a handler registered by an earlier preview: `"exe" "%1"`, nothing else
        a if !a.starts_with("--") && looks_like_target(a) => (args.len() == 2).then_some(a),
        _ => None,
    }
}

/// The torrent a command line asks to open: a `magnet:?` link (at most 8 KiB) or an existing
/// file whose name ends in `.torrent`, passed as the single data argument of an open launch (see
/// the module docs). Flags, extra arguments, quotes and control characters make the whole command
/// line invalid.
pub fn open_target(args: &[String]) -> Option<TorrentSourceRequest> {
    let a = open_data(args)?.trim();
    if a.is_empty() || a.starts_with("--") || a.chars().any(|c| c == '"' || c.is_control()) {
        return None;
    }
    if a.get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("magnet:?"))
    {
        return (a.len() <= MAX_MAGNET_LEN)
            .then(|| TorrentSourceRequest::Magnet { uri: a.to_string() });
    }
    if a.len() > MAX_PATH_LEN {
        return None;
    }
    let path = PathBuf::from(a);
    let is_torrent = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("torrent"));
    (is_torrent && path.is_file()).then_some(TorrentSourceRequest::File { path })
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
            Some(r#""C:\Apps\Riptide\riptide-desktop.exe" --open-link -- "%1""#)
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
    fn another_apps_magnet_handler_is_given_back_when_ours_is_removed() {
        let mut r = Memory::default();
        let theirs = r#""C:\Program Files\qBittorrent\qbittorrent.exe" "%1""#;
        r.set_default_value(MAGNET_KEY, "URL:Magnet Protocol")
            .unwrap();
        r.set_default_value(&command_key(MAGNET_KEY), theirs)
            .unwrap();

        register(&exe(), &mut r).unwrap();
        assert!(status(&exe(), &r).magnet);
        // registering again keeps the original backup, not our own command
        register(&exe(), &mut r).unwrap();
        assert_eq!(
            r.named_value(MAGNET_KEY, PREVIOUS_COMMAND_VALUE).as_deref(),
            Some(theirs)
        );

        let s = unregister(&exe(), &mut r).unwrap();
        assert!(!s.magnet);
        assert_eq!(
            r.default_value(&command_key(MAGNET_KEY)).as_deref(),
            Some(theirs),
            "the other app lost its handler"
        );
        assert_eq!(
            r.named_value(MAGNET_KEY, "URL Protocol").as_deref(),
            Some("")
        );
        assert!(r.named_value(MAGNET_KEY, PREVIOUS_COMMAND_VALUE).is_none());
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
        // the registered form
        assert_eq!(
            open_target(&args(&["riptide-desktop.exe", OPEN_MARKER, "--", &link])),
            Some(TorrentSourceRequest::Magnet { uri: link.clone() })
        );
        // a flag next to the link is not accepted any more
        assert_eq!(
            open_target(&args(&["riptide-desktop.exe", "--minimized", &link])),
            None
        );
    }

    /// Splits a command line the way the Microsoft C runtime does (what `CommandLineToArgvW` and
    /// Rust's `std::env::args` implement): double quotes group, `\"` is a literal quote,
    /// backslashes only count before a quote.
    fn split_msvc(cmd: &str) -> Vec<String> {
        let chars: Vec<char> = cmd.chars().collect();
        let (mut out, mut cur, mut in_q, mut has) = (Vec::new(), String::new(), false, false);
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\\' => {
                    let mut n = 0;
                    while chars.get(i) == Some(&'\\') {
                        n += 1;
                        i += 1;
                    }
                    has = true;
                    if chars.get(i) == Some(&'"') {
                        cur.extend(std::iter::repeat_n('\\', n / 2));
                        if n % 2 == 1 {
                            cur.push('"');
                            i += 1;
                        }
                    } else {
                        cur.extend(std::iter::repeat_n('\\', n));
                    }
                    continue;
                }
                '"' => {
                    in_q = !in_q;
                    has = true;
                }
                c if c.is_whitespace() && !in_q => {
                    if has {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                c => {
                    cur.push(c);
                    has = true;
                }
            }
            i += 1;
        }
        if has {
            out.push(cur);
        }
        out
    }

    #[test]
    fn a_link_that_breaks_out_of_its_quotes_cannot_become_flags() {
        let hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
        // what a launcher that does not escape quotes would substitute for %1
        let hostile =
            format!("magnet:?xt=urn:btih:{hash}&dn=x\" --unregister-host --log-level \"trace");
        let template = command_line(&exe());
        let cmd = template.replace("%1", &hostile);
        let argv = split_msvc(&cmd);
        // the control: the quote really does split the link into flags
        assert!(argv.iter().any(|a| a == "--unregister-host"), "{argv:?}");
        assert!(is_open_launch(&argv));
        assert_eq!(open_target(&argv), None, "a split link must be refused");

        // the same holds for an earlier registration (`"exe" "%1"`)
        let legacy = format!("\"{}\" \"{hostile}\"", exe().display());
        let argv = split_msvc(&legacy);
        assert!(argv.iter().any(|a| a == "--unregister-host"), "{argv:?}");
        assert!(is_open_launch(&argv));
        assert_eq!(open_target(&argv), None);

        // and one well-formed link goes through the registered command unchanged
        let good = format!("magnet:?xt=urn:btih:{hash}&dn=a%20b");
        let argv = split_msvc(&template.replace("%1", &good));
        assert_eq!(
            open_target(&argv),
            Some(TorrentSourceRequest::Magnet { uri: good })
        );
    }

    #[test]
    fn quotes_control_characters_and_missing_separators_are_refused() {
        let hash = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";
        for bad in [
            format!("magnet:?xt=urn:btih:{hash}&dn=a\"b"),
            format!("magnet:?xt=urn:btih:{hash}&dn=a\nb"),
            format!("magnet:?xt=urn:btih:{hash}&dn=a\0b"),
            format!("magnet:?xt=urn:btih:{hash}&dn=a\rb"),
        ] {
            assert_eq!(
                open_target(&args(&["x.exe", OPEN_MARKER, "--", &bad])),
                None,
                "{bad:?}"
            );
        }
        let link = format!("magnet:?xt=urn:btih:{hash}");
        // the marker without the `--`, a flag after it, or no data at all
        assert_eq!(open_target(&args(&["x.exe", OPEN_MARKER, &link])), None);
        assert_eq!(
            open_target(&args(&["x.exe", OPEN_MARKER, "--x", &link])),
            None
        );
        assert_eq!(open_target(&args(&["x.exe", OPEN_MARKER, "--"])), None);
        // a link that starts with dashes is data, but not a link
        assert_eq!(
            open_target(&args(&["x.exe", OPEN_MARKER, "--", "--unregister-host"])),
            None
        );
        // an oversized link
        let huge = format!("magnet:?xt=urn:btih:{}", "a".repeat(MAX_MAGNET_LEN));
        assert_eq!(
            open_target(&args(&["x.exe", OPEN_MARKER, "--", &huge])),
            None
        );
    }

    #[test]
    fn a_launch_with_a_torrent_file_opens_it_only_if_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Big Buck Bunny.TORRENT");
        std::fs::write(&file, b"d4:infod4:name1:xee").unwrap();
        let path = file.to_string_lossy().into_owned();
        assert_eq!(
            open_target(&args(&["x.exe", OPEN_MARKER, "--", &path])),
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
