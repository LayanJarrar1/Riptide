//! Browser integration without elevation: registers the Native Messaging host
//! `app.riptide.host` for Chrome, Edge and Firefox, per user.
//!
//! * Two host manifests are written to `<data>\native-messaging\` (`%LOCALAPPDATA%\Riptide` when
//!   installed, `data\` next to the executable when portable): the Chromium form with
//!   `allowed_origins`, the Firefox form with `allowed_extensions`. Both point at
//!   `riptide-host.exe` next to the app executable.
//! * The default value of `HKCU\Software\Google\Chrome\NativeMessagingHosts\app.riptide.host`,
//!   `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\app.riptide.host` and
//!   `HKCU\Software\Mozilla\NativeMessagingHosts\app.riptide.host` is set to the manifest path.
//!   Nothing is written outside `HKCU`.
//! * The choice is remembered in `<data>\integration.json` (`browser_host`), which also lists
//!   extra extension ids; the host reads the same file. The app registers on every start while
//!   `browser_host` is on (idempotent: it also repairs a moved app), and never after the user
//!   turned it off.
//!
//! * The NSIS installer calls `riptide-desktop.exe --register-host` after copying files (also on
//!   upgrade, which rewrites the manifests with the new paths and ids) and
//!   `--unregister-host` before removing them (see [`run_cli`]). Both are headless and exit
//!   before any window or the single-instance lock, so they work while the app is running.
//!   Unregistering only touches what points at *this* install (see [`remove_ours`]).
//!
//! The registry sits behind [`Registry`] so the logic is tested against memory; the Windows
//! implementation uses the `winreg` crate (a safe wrapper over the Win32 registry API, already
//! in the dependency graph), which keeps `unsafe` out of the app crate.

use std::path::{Path, PathBuf};

use riptide_host::{IntegrationSettings, HOST_EXE, HOST_NAME, INTEGRATION_FILE};
use serde::Serialize;
use serde_json::{json, Value};

/// Folder (inside the data directory) that holds the host manifests.
pub const MANIFEST_DIR: &str = "native-messaging";

/// Description written into both manifests.
pub const HOST_DESCRIPTION: &str = "Riptide download manager — browser bridge";

/// Per-user registry, `HKCU` relative paths. Only default values are used.
pub trait Registry {
    /// The default value of `key`, if the key exists and has a string default.
    fn default_value(&self, key: &str) -> Option<String>;
    /// Creates `key` if needed and sets its default value.
    fn set_default_value(&mut self, key: &str, value: &str) -> std::io::Result<()>;
    /// Deletes `key` and everything under it; a missing key is not an error.
    fn delete_key(&mut self, key: &str) -> std::io::Result<()>;
    /// The string value `name` of `key` (`""` is the default value).
    fn named_value(&self, _key: &str, _name: &str) -> Option<String> {
        None
    }
    /// Creates `key` if needed and sets its string value `name` (`""` is the default value).
    fn set_named_value(&mut self, _key: &str, _name: &str, _value: &str) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
    /// Deletes the value `name` of `key` and nothing else; a missing key or value is not an error.
    fn delete_named_value(&mut self, _key: &str, _name: &str) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

/// Test knob: moves every registry key under `HKCU\<value>` (an installer test run must not
/// touch the real browser keys). Unset in normal use.
pub const REGISTRY_ROOT_ENV: &str = "RIPTIDE_REGISTRY_ROOT";

/// Test knob: folder for the manifests and `integration.json` instead of the data folder.
pub const INTEGRATION_DIR_ENV: &str = "RIPTIDE_INTEGRATION_DIR";

/// `HKEY_CURRENT_USER` through `winreg`.
#[cfg(windows)]
#[derive(Debug, Default, Clone)]
pub struct CurrentUserRegistry {
    root: Option<String>,
}

#[cfg(windows)]
impl CurrentUserRegistry {
    /// Keys live under `HKCU\<root>\...` (tests); `None` is the real location.
    pub fn with_root(root: Option<String>) -> CurrentUserRegistry {
        CurrentUserRegistry {
            root: root.filter(|r| !r.trim().is_empty()),
        }
    }

    /// Honours [`REGISTRY_ROOT_ENV`].
    pub fn from_env() -> CurrentUserRegistry {
        CurrentUserRegistry::with_root(std::env::var(REGISTRY_ROOT_ENV).ok())
    }

    fn path(&self, key: &str) -> String {
        match &self.root {
            Some(root) => format!(r"{}\{key}", root.trim_end_matches('\\')),
            None => key.to_string(),
        }
    }
}

#[cfg(windows)]
impl Registry for CurrentUserRegistry {
    fn default_value(&self, key: &str) -> Option<String> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        hkcu.open_subkey(self.path(key))
            .ok()?
            .get_value::<String, _>("")
            .ok()
    }

    fn set_default_value(&mut self, key: &str, value: &str) -> std::io::Result<()> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let (k, _) = hkcu.create_subkey(self.path(key))?;
        k.set_value("", &value)
    }

    fn delete_key(&mut self, key: &str) -> std::io::Result<()> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        match hkcu.delete_subkey_all(self.path(key)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn named_value(&self, key: &str, name: &str) -> Option<String> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        hkcu.open_subkey(self.path(key))
            .ok()?
            .get_value::<String, _>(name)
            .ok()
    }

    fn set_named_value(&mut self, key: &str, name: &str, value: &str) -> std::io::Result<()> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let (k, _) = hkcu.create_subkey(self.path(key))?;
        k.set_value(name, &value)
    }

    fn delete_named_value(&mut self, key: &str, name: &str) -> std::io::Result<()> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let Ok(k) = hkcu.open_subkey_with_flags(self.path(key), winreg::enums::KEY_SET_VALUE)
        else {
            return Ok(());
        };
        match k.delete_value(name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// The registry of this platform: `HKCU` on Windows, none elsewhere (the integration is
/// Windows-only in v1).
pub fn platform_registry() -> Option<Box<dyn Registry + Send>> {
    #[cfg(windows)]
    {
        Some(Box::new(CurrentUserRegistry::from_env()))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// A browser the host is registered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Browser {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
    /// Mozilla Firefox.
    Firefox,
}

impl Browser {
    /// Every supported browser.
    pub const ALL: [Browser; 3] = [Browser::Chrome, Browser::Edge, Browser::Firefox];

    /// `HKCU`-relative key whose default value is the manifest path.
    pub fn registry_key(self) -> String {
        let vendor = match self {
            Browser::Chrome => r"Software\Google\Chrome",
            Browser::Edge => r"Software\Microsoft\Edge",
            Browser::Firefox => r"Software\Mozilla",
        };
        format!(r"{vendor}\NativeMessagingHosts\{HOST_NAME}")
    }

    fn is_chromium(self) -> bool {
        !matches!(self, Browser::Firefox)
    }
}

/// Where everything lives for one installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Folder of the two manifests.
    pub manifest_dir: PathBuf,
    /// `integration.json`.
    pub settings_file: PathBuf,
    /// The host executable the manifests point at.
    pub host_exe: PathBuf,
}

impl Layout {
    /// The layout for a data directory and the folder of the app executable.
    pub fn new(data_dir: &Path, exe_dir: &Path) -> Layout {
        Layout {
            manifest_dir: data_dir.join(MANIFEST_DIR),
            settings_file: data_dir.join(INTEGRATION_FILE),
            host_exe: exe_dir.join(HOST_EXE),
        }
    }

    /// The layout of the running app (installed or portable).
    pub fn for_this_app() -> Result<Layout, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| "the app executable has no folder".to_string())?;
        if let Some(dir) = std::env::var_os(INTEGRATION_DIR_ENV).filter(|d| !d.is_empty()) {
            return Ok(Layout::new(Path::new(&dir), exe_dir));
        }
        let paths = riptide_log::Paths::resolve().map_err(|e| e.to_string())?;
        Ok(Layout::new(&paths.data_dir, exe_dir))
    }

    /// The Chromium manifest (Chrome and Edge share it).
    pub fn chromium_manifest(&self) -> PathBuf {
        self.manifest_dir.join(format!("{HOST_NAME}.chromium.json"))
    }

    /// The Firefox manifest.
    pub fn firefox_manifest(&self) -> PathBuf {
        self.manifest_dir.join(format!("{HOST_NAME}.firefox.json"))
    }

    fn manifest_for(&self, browser: Browser) -> PathBuf {
        if browser.is_chromium() {
            self.chromium_manifest()
        } else {
            self.firefox_manifest()
        }
    }
}

/// The Chromium host manifest: `allowed_origins` with the trailing slash.
pub fn chromium_manifest(host_exe: &Path, ids: &[String]) -> Value {
    let origins: Vec<String> = ids
        .iter()
        .map(|id| format!("chrome-extension://{id}/"))
        .collect();
    json!({
        "name": HOST_NAME,
        "description": HOST_DESCRIPTION,
        "path": host_exe.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": origins,
    })
}

/// The Firefox host manifest: `allowed_extensions`.
pub fn firefox_manifest(host_exe: &Path, ids: &[String]) -> Value {
    json!({
        "name": HOST_NAME,
        "description": HOST_DESCRIPTION,
        "path": host_exe.to_string_lossy(),
        "type": "stdio",
        "allowed_extensions": ids,
    })
}

/// Registration state of one browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserStatus {
    /// Which browser.
    pub browser: Browser,
    /// Its key points at our manifest, the manifest exists and names our host.
    pub registered: bool,
}

/// What *Settings → Integration* shows (`integration_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    /// This platform supports the integration.
    pub supported: bool,
    /// Every browser is registered and the host executable exists.
    pub registered: bool,
    /// The user wants it on (`browser_host` in `integration.json`).
    pub enabled: bool,
    /// `riptide-host.exe` exists next to the app.
    pub host_found: bool,
    /// Path of the host executable.
    pub host_path: String,
    /// Folder of the manifests.
    pub manifest_dir: String,
    /// Per-browser detail.
    pub browsers: Vec<BrowserStatus>,
}

impl IntegrationStatus {
    /// The answer on a platform without the integration.
    pub fn unsupported() -> IntegrationStatus {
        IntegrationStatus {
            supported: false,
            registered: false,
            enabled: false,
            host_found: false,
            host_path: String::new(),
            manifest_dir: String::new(),
            browsers: Vec::new(),
        }
    }
}

fn read_manifest(manifest: &Path) -> Option<Value> {
    std::fs::read(manifest)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
}

/// The manifest at `manifest` names `host_exe` (Windows paths compare case-insensitively).
fn manifest_names_host(manifest: &Path, host_exe: &Path) -> bool {
    read_manifest(manifest)
        .and_then(|v| v.get("path").and_then(Value::as_str).map(PathBuf::from))
        .is_some_and(|p| same_path(&p, host_exe))
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// What the manifest for `browser` should contain right now.
fn expected_manifest(layout: &Layout, browser: Browser, settings: &IntegrationSettings) -> Value {
    if browser.is_chromium() {
        chromium_manifest(&layout.host_exe, &settings.chromium_ids())
    } else {
        firefox_manifest(&layout.host_exe, &settings.firefox_ids())
    }
}

/// The current state, read from the registry and the files.
pub fn status(layout: &Layout, registry: &dyn Registry) -> IntegrationStatus {
    let host_found = layout.host_exe.is_file();
    let settings = IntegrationSettings::load(&layout.settings_file);
    let browsers: Vec<BrowserStatus> = Browser::ALL
        .iter()
        .map(|&browser| {
            let manifest = layout.manifest_for(browser);
            let points_here = registry
                .default_value(&browser.registry_key())
                .is_some_and(|v| same_path(Path::new(&v), &manifest));
            // The whole manifest is compared, so a changed extension id (an upgrade) is a repair.
            let current = read_manifest(&manifest)
                .is_some_and(|found| found == expected_manifest(layout, browser, &settings));
            BrowserStatus {
                browser,
                registered: points_here && current,
            }
        })
        .collect();
    IntegrationStatus {
        supported: true,
        registered: host_found && browsers.iter().all(|b| b.registered),
        enabled: settings.browser_host,
        host_found,
        host_path: layout.host_exe.to_string_lossy().into_owned(),
        manifest_dir: layout.manifest_dir.to_string_lossy().into_owned(),
        browsers,
    }
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn io_err(what: &str, path: &Path, e: &std::io::Error) -> String {
    format!("{what} {}: {e}", path.display())
}

/// Writes both manifests and the three `HKCU` keys, and remembers the choice. Idempotent.
///
/// # Errors
///
/// A message for the UI when the host executable is missing or a file / key cannot be written.
pub fn register(layout: &Layout, registry: &mut dyn Registry) -> Result<IntegrationStatus, String> {
    if !layout.host_exe.is_file() {
        return Err(format!(
            "{} was not found next to the app",
            layout.host_exe.display()
        ));
    }
    let mut settings = IntegrationSettings::load(&layout.settings_file);
    settings.browser_host = true;
    settings
        .save(&layout.settings_file)
        .map_err(|e| io_err("cannot write", &layout.settings_file, &e))?;

    std::fs::create_dir_all(&layout.manifest_dir)
        .map_err(|e| io_err("cannot create", &layout.manifest_dir, &e))?;
    let chromium = layout.chromium_manifest();
    write_json(
        &chromium,
        &chromium_manifest(&layout.host_exe, &settings.chromium_ids()),
    )
    .map_err(|e| io_err("cannot write", &chromium, &e))?;
    let firefox = layout.firefox_manifest();
    write_json(
        &firefox,
        &firefox_manifest(&layout.host_exe, &settings.firefox_ids()),
    )
    .map_err(|e| io_err("cannot write", &firefox, &e))?;

    for browser in Browser::ALL {
        let manifest = layout.manifest_for(browser);
        registry
            .set_default_value(&browser.registry_key(), &manifest.to_string_lossy())
            .map_err(|e| format!(r"cannot write HKCU\{}: {e}", browser.registry_key()))?;
    }
    tracing::info!(host = %layout.host_exe.display(), "browser host registered");
    Ok(status(layout, registry))
}

/// Removes what points at this install: a browser key whose manifest names this install's
/// host (or whose manifest file is gone but sits in this install's manifest folder), and the
/// manifest files that name this host. Keys and files of another install (a second copy, a
/// portable folder, a throwaway test install) are left alone. Settings are not touched.
///
/// # Errors
///
/// A message when a key or file that is ours cannot be removed.
pub fn remove_ours(layout: &Layout, registry: &mut dyn Registry) -> Result<(), String> {
    for browser in Browser::ALL {
        let key = browser.registry_key();
        let Some(value) = registry.default_value(&key) else {
            continue;
        };
        let manifest = PathBuf::from(&value);
        let ours = if manifest.is_file() {
            manifest_names_host(&manifest, &layout.host_exe)
        } else {
            // dangling: only ours when it sits in our own manifest folder
            manifest
                .parent()
                .is_some_and(|dir| same_path(dir, &layout.manifest_dir))
        };
        if ours {
            registry
                .delete_key(&key)
                .map_err(|e| format!(r"cannot remove HKCU\{key}: {e}"))?;
        }
    }
    for manifest in [layout.chromium_manifest(), layout.firefox_manifest()] {
        if manifest.is_file() && !manifest_names_host(&manifest, &layout.host_exe) {
            continue; // written by another install
        }
        match std::fs::remove_file(&manifest) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(io_err("cannot remove", &manifest, &e));
            }
            _ => {}
        }
    }
    // the folder goes only when nothing else is in it
    let _ = std::fs::remove_dir(&layout.manifest_dir);
    Ok(())
}

/// Turns the integration off (Settings): removes what is ours and remembers the choice.
///
/// # Errors
///
/// A message for the UI when a key or file cannot be removed.
pub fn unregister(
    layout: &Layout,
    registry: &mut dyn Registry,
) -> Result<IntegrationStatus, String> {
    let mut settings = IntegrationSettings::load(&layout.settings_file);
    settings.browser_host = false;
    settings
        .save(&layout.settings_file)
        .map_err(|e| io_err("cannot write", &layout.settings_file, &e))?;
    remove_ours(layout, registry)?;
    tracing::info!("browser host unregistered");
    Ok(status(layout, registry))
}

/// Installer entry point. Registers like [`register_on_start`] (so an integration the user
/// turned off stays off across an upgrade) and reports what happened. `Ok(true)` = registered
/// now, `Ok(false)` = skipped on purpose.
///
/// # Errors
///
/// A message when the host executable is missing or a file / key cannot be written.
pub fn register_for_installer(
    layout: &Layout,
    registry: &mut dyn Registry,
) -> Result<bool, String> {
    if !IntegrationSettings::load(&layout.settings_file).browser_host {
        return Ok(false);
    }
    if status(layout, registry).registered {
        return Ok(true);
    }
    register(layout, registry).map(|_| true)
}

/// Handles `--register-host` / `--unregister-host` (the installer hooks). `None` when `args`
/// (without the program name) holds neither: the app starts normally. `Some(code)` is the
/// process exit code; nothing is printed (the release build has no console).
pub fn run_cli(args: &[String]) -> Option<i32> {
    let register = args.iter().any(|a| a == "--register-host");
    let unregister = args.iter().any(|a| a == "--unregister-host");
    if register == unregister {
        return None;
    }
    let (Some(mut registry), Ok(layout)) = (platform_registry(), Layout::for_this_app()) else {
        return Some(1);
    };
    let result = if register {
        register_for_installer(&layout, registry.as_mut()).map(|_| ())
    } else {
        remove_ours(&layout, registry.as_mut())
    };
    match result {
        Ok(()) => Some(0),
        Err(e) => {
            tracing::warn!(error = %e, "host registration command failed");
            Some(1)
        }
    }
}

/// Start-up hook: registers while the user has not turned the integration off. Failures are
/// logged, never fatal (a development build may not have built the host).
pub fn register_on_start(layout: &Layout, registry: &mut dyn Registry) {
    if !IntegrationSettings::load(&layout.settings_file).browser_host {
        return;
    }
    if status(layout, registry).registered {
        return;
    }
    if let Err(e) = register(layout, registry) {
        tracing::warn!(error = %e, "browser host not registered");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use riptide_host::{CHROMIUM_EXTENSION_ID, FIREFOX_EXTENSION_ID};

    use super::*;

    #[derive(Default)]
    struct MemoryRegistry(BTreeMap<String, String>);

    impl Registry for MemoryRegistry {
        fn default_value(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_default_value(&mut self, key: &str, value: &str) -> std::io::Result<()> {
            self.0.insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn delete_key(&mut self, key: &str) -> std::io::Result<()> {
            self.0.remove(key);
            Ok(())
        }
    }

    fn setup() -> (tempfile::TempDir, Layout) {
        let dir = tempfile::tempdir().unwrap();
        let exe_dir = dir.path().join("app");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let layout = Layout::new(&dir.path().join("data"), &exe_dir);
        (dir, layout)
    }

    fn read(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn keys_are_per_user_browser_paths() {
        assert_eq!(
            Browser::Chrome.registry_key(),
            r"Software\Google\Chrome\NativeMessagingHosts\app.riptide.host"
        );
        assert_eq!(
            Browser::Edge.registry_key(),
            r"Software\Microsoft\Edge\NativeMessagingHosts\app.riptide.host"
        );
        assert_eq!(
            Browser::Firefox.registry_key(),
            r"Software\Mozilla\NativeMessagingHosts\app.riptide.host"
        );
    }

    #[test]
    fn register_writes_both_manifests_and_three_keys_then_unregister_undoes_it() {
        let (_dir, layout) = setup();
        let mut reg = MemoryRegistry::default();
        assert!(
            register(&layout, &mut reg).is_err(),
            "no host executable yet"
        );

        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let before = status(&layout, &reg);
        assert!(!before.registered && before.host_found && before.enabled);

        let st = register(&layout, &mut reg).unwrap();
        assert!(st.registered, "{st:?}");
        assert!(st.browsers.iter().all(|b| b.registered));

        let chromium = read(&layout.chromium_manifest());
        assert_eq!(chromium["name"], HOST_NAME);
        assert_eq!(chromium["type"], "stdio");
        assert_eq!(chromium["path"], layout.host_exe.to_string_lossy().as_ref());
        assert_eq!(
            chromium["allowed_origins"],
            json!([format!("chrome-extension://{CHROMIUM_EXTENSION_ID}/")])
        );
        assert!(chromium.get("allowed_extensions").is_none());
        let firefox = read(&layout.firefox_manifest());
        assert_eq!(firefox["allowed_extensions"], json!([FIREFOX_EXTENSION_ID]));
        assert!(firefox.get("allowed_origins").is_none());
        assert_eq!(
            reg.default_value(&Browser::Edge.registry_key()).unwrap(),
            layout.chromium_manifest().to_string_lossy()
        );
        assert_eq!(
            reg.default_value(&Browser::Firefox.registry_key()).unwrap(),
            layout.firefox_manifest().to_string_lossy()
        );

        // idempotent
        assert_eq!(register(&layout, &mut reg).unwrap(), st);

        let st = unregister(&layout, &mut reg).unwrap();
        assert!(!st.registered && !st.enabled);
        assert!(reg.0.is_empty());
        assert!(!layout.chromium_manifest().exists());
        assert!(!layout.firefox_manifest().exists());
        // and stays off across starts
        register_on_start(&layout, &mut reg);
        assert!(reg.0.is_empty());
    }

    #[test]
    fn extra_ids_land_in_the_matching_manifest() {
        let (_dir, layout) = setup();
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        IntegrationSettings {
            browser_host: true,
            extra_extension_ids: vec![
                "abcdefghijklmnopabcdefghijklmnop".into(),
                "store@riptide.app".into(),
                "garbage\"],\"x".into(),
            ],
        }
        .save(&layout.settings_file)
        .unwrap();
        let mut reg = MemoryRegistry::default();
        register_on_start(&layout, &mut reg);
        let chromium = read(&layout.chromium_manifest());
        assert_eq!(
            chromium["allowed_origins"].as_array().unwrap().len(),
            2,
            "{chromium}"
        );
        let firefox = read(&layout.firefox_manifest());
        assert_eq!(
            firefox["allowed_extensions"],
            json!([FIREFOX_EXTENSION_ID, "store@riptide.app"])
        );
    }

    #[test]
    fn a_moved_app_is_repaired_on_start() {
        let (dir, layout) = setup();
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let mut reg = MemoryRegistry::default();
        register(&layout, &mut reg).unwrap();

        let moved_dir = dir.path().join("moved");
        std::fs::create_dir_all(&moved_dir).unwrap();
        let moved = Layout::new(&dir.path().join("data"), &moved_dir);
        std::fs::write(&moved.host_exe, b"exe").unwrap();
        assert!(
            !status(&moved, &reg).registered,
            "manifest names the old host"
        );
        register_on_start(&moved, &mut reg);
        assert!(status(&moved, &reg).registered);
    }

    #[test]
    fn paths_with_spaces_and_unicode_survive_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let exe_dir = dir.path().join("Riptide مجلد ü ñ 日本");
        std::fs::create_dir_all(&exe_dir).unwrap();
        let layout = Layout::new(&dir.path().join("data dir"), &exe_dir);
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let mut reg = MemoryRegistry::default();
        assert!(register(&layout, &mut reg).unwrap().registered);

        let raw = std::fs::read_to_string(layout.chromium_manifest()).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["path"], layout.host_exe.to_string_lossy().as_ref());
        assert!(parsed["path"].as_str().unwrap().contains("مجلد ü ñ 日本"));
        // the registry value is the manifest path, spaces included, no quoting
        let value = reg.default_value(&Browser::Chrome.registry_key()).unwrap();
        assert_eq!(Path::new(&value), layout.chromium_manifest());
        assert!(value.contains("data dir"));
    }

    #[test]
    fn a_default_install_trusts_exactly_the_published_ids() {
        let (_dir, layout) = setup();
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let mut reg = MemoryRegistry::default();
        register(&layout, &mut reg).unwrap();
        assert_eq!(
            read(&layout.chromium_manifest())["allowed_origins"],
            json!([format!("chrome-extension://{CHROMIUM_EXTENSION_ID}/")])
        );
        assert_eq!(
            read(&layout.firefox_manifest())["allowed_extensions"],
            json!([FIREFOX_EXTENSION_ID])
        );
    }

    fn second_install(dir: &tempfile::TempDir) -> Layout {
        let other = dir.path().join("other-app");
        std::fs::create_dir_all(&other).unwrap();
        let layout = Layout::new(&dir.path().join("data"), &other);
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        layout
    }

    #[test]
    fn uninstalling_one_install_leaves_the_other_installs_registration() {
        let (dir, a) = setup();
        std::fs::write(&a.host_exe, b"exe").unwrap();
        let b = second_install(&dir);
        let mut reg = MemoryRegistry::default();
        register(&a, &mut reg).unwrap();
        register(&b, &mut reg).unwrap(); // B took over the shared manifests

        remove_ours(&a, &mut reg).unwrap();
        assert!(status(&b, &reg).registered, "B is untouched");
        assert!(b.chromium_manifest().is_file() && b.firefox_manifest().is_file());
        assert_eq!(reg.0.len(), 3);

        remove_ours(&b, &mut reg).unwrap();
        assert!(reg.0.is_empty());
        assert!(!b.manifest_dir.exists(), "empty folder removed");
    }

    #[test]
    fn keys_pointing_elsewhere_are_never_deleted() {
        let (_dir, layout) = setup();
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let mut reg = MemoryRegistry::default();
        let foreign = r"D:\Elsewhere\app.riptide.host.json".to_string();
        for browser in Browser::ALL {
            reg.0.insert(browser.registry_key(), foreign.clone());
        }
        remove_ours(&layout, &mut reg).unwrap();
        assert_eq!(reg.0.len(), 3);
        assert!(reg.0.values().all(|v| *v == foreign));
    }

    #[test]
    fn a_dangling_key_in_our_own_folder_is_cleaned_up() {
        let (_dir, layout) = setup();
        let mut reg = MemoryRegistry::default();
        let ours = layout.chromium_manifest().to_string_lossy().into_owned();
        reg.0.insert(Browser::Chrome.registry_key(), ours);
        remove_ours(&layout, &mut reg).unwrap();
        assert!(reg.0.is_empty());
    }

    #[test]
    fn the_installer_registers_repairs_stale_ids_and_respects_a_user_who_turned_it_off() {
        let (_dir, layout) = setup();
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let mut reg = MemoryRegistry::default();
        // fresh install: no integration.json yet -> on
        assert!(register_for_installer(&layout, &mut reg).unwrap());
        assert!(status(&layout, &reg).registered);

        // an older version wrote another id: the upgrade rewrites the manifest
        let stale = chromium_manifest(&layout.host_exe, &["a".repeat(32)]);
        write_json(&layout.chromium_manifest(), &stale).unwrap();
        assert!(!status(&layout, &reg).registered);
        assert!(register_for_installer(&layout, &mut reg).unwrap());
        assert!(status(&layout, &reg).registered);

        // the user turned it off in Settings: an upgrade does not turn it back on
        unregister(&layout, &mut reg).unwrap();
        assert!(!register_for_installer(&layout, &mut reg).unwrap());
        assert!(reg.0.is_empty());
    }

    #[test]
    fn the_installer_fails_loudly_without_a_host_executable() {
        let (_dir, layout) = setup();
        let mut reg = MemoryRegistry::default();
        assert!(register_for_installer(&layout, &mut reg).is_err());
        assert!(reg.0.is_empty());
    }

    #[test]
    fn run_cli_only_reacts_to_exactly_one_installer_flag() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert_eq!(run_cli(&args(&[])), None);
        assert_eq!(run_cli(&args(&["--minimized"])), None);
        assert_eq!(
            run_cli(&args(&["--register-host", "--unregister-host"])),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_registry_round_trips_under_a_throwaway_root() {
        let root = format!(r"Software\RiptideUnitTest\{}", std::process::id());
        let mut reg = CurrentUserRegistry::with_root(Some(root.clone()));
        let (_dir, layout) = setup();
        std::fs::write(&layout.host_exe, b"exe").unwrap();
        let key = Browser::Chrome.registry_key();
        assert_eq!(reg.default_value(&key), None);
        let st = register(&layout, &mut reg).unwrap();
        assert!(st.registered, "{st:?}");
        // it went under the throwaway root, not the real browser key
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        assert!(hkcu.open_subkey(format!(r"{root}\{key}")).is_ok());
        remove_ours(&layout, &mut reg).unwrap();
        assert_eq!(reg.default_value(&key), None);
        let _ = hkcu.delete_subkey_all(&root);
    }

    #[test]
    fn installer_artifacts_agree_with_the_constants() {
        // install-extension.ps1 cannot call Rust, so its copy of the id is pinned here
        let script = include_str!("../../../../scripts/install-extension.ps1");
        assert!(
            script.contains(&format!("$ExpectedId = '{CHROMIUM_EXTENSION_ID}'")),
            "scripts/install-extension.ps1 has a different extension id"
        );
        // the NSIS hooks must keep their BOM (makensis reads them as UTF-8 only with it) and must
        // not carry ids or paths of their own: they call the app, which owns them
        let hooks = include_bytes!("../windows/hooks.nsh");
        assert!(
            hooks.starts_with(&[0xEF, 0xBB, 0xBF]),
            "hooks.nsh lost its BOM"
        );
        let text = String::from_utf8_lossy(hooks);
        assert!(text.contains("--register-host") && text.contains("--unregister-host"));
        assert!(!text.contains(CHROMIUM_EXTENSION_ID));
        assert!(!text.contains(FIREFOX_EXTENSION_ID));
        assert!(
            !text.contains("DeleteRegKey HKCU"),
            "uninstall must not delete browser keys unconditionally"
        );
    }
}
