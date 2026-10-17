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
}

/// `HKEY_CURRENT_USER` through `winreg`.
#[cfg(windows)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CurrentUserRegistry;

#[cfg(windows)]
impl Registry for CurrentUserRegistry {
    fn default_value(&self, key: &str) -> Option<String> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        hkcu.open_subkey(key).ok()?.get_value::<String, _>("").ok()
    }

    fn set_default_value(&mut self, key: &str, value: &str) -> std::io::Result<()> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let (k, _) = hkcu.create_subkey(key)?;
        k.set_value("", &value)
    }

    fn delete_key(&mut self, key: &str) -> std::io::Result<()> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        match hkcu.delete_subkey_all(key) {
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
        Some(Box::new(CurrentUserRegistry))
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
        let paths = riptide_log::Paths::resolve().map_err(|e| e.to_string())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| "the app executable has no folder".to_string())?;
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

fn manifest_names_host(manifest: &Path, host_exe: &Path) -> bool {
    std::fs::read(manifest)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.get("path").and_then(Value::as_str).map(PathBuf::from))
        .is_some_and(|p| p == host_exe)
}

/// The current state, read from the registry and the files.
pub fn status(layout: &Layout, registry: &dyn Registry) -> IntegrationStatus {
    let host_found = layout.host_exe.is_file();
    let browsers: Vec<BrowserStatus> = Browser::ALL
        .iter()
        .map(|&browser| {
            let manifest = layout.manifest_for(browser);
            let points_here = registry
                .default_value(&browser.registry_key())
                .is_some_and(|v| Path::new(&v) == manifest);
            BrowserStatus {
                browser,
                registered: points_here && manifest_names_host(&manifest, &layout.host_exe),
            }
        })
        .collect();
    IntegrationStatus {
        supported: true,
        registered: host_found && browsers.iter().all(|b| b.registered),
        enabled: IntegrationSettings::load(&layout.settings_file).browser_host,
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

/// Removes the three `HKCU` keys and both manifests, and remembers the choice.
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
    for browser in Browser::ALL {
        registry
            .delete_key(&browser.registry_key())
            .map_err(|e| format!(r"cannot remove HKCU\{}: {e}", browser.registry_key()))?;
    }
    for manifest in [layout.chromium_manifest(), layout.firefox_manifest()] {
        match std::fs::remove_file(&manifest) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(io_err("cannot remove", &manifest, &e));
            }
            _ => {}
        }
    }
    tracing::info!("browser host unregistered");
    Ok(status(layout, registry))
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
}
