//! The uninstaller's `--unregister-handlers`: the magnet and .torrent handlers of this executable
//! go, another app's handler is left alone (and given back), all against a throw-away registry root
//! (`RIPTIDE_REGISTRY_ROOT`), never the real keys. Windows only.
#![cfg(windows)]
#![allow(clippy::unwrap_used, missing_docs)]

use riptide_desktop_lib::integration::{platform_registry, run_cli, REGISTRY_ROOT_ENV};
use riptide_desktop_lib::protocol::{
    migrate_legacy, register, status, MAGNET_KEY, TORRENT_EXT_KEY,
};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

const FOREIGN: &str = r#""C:\Program Files\qBittorrent\qbittorrent.exe" "%1""#;

#[test]
fn the_uninstaller_removes_our_handlers_and_gives_back_another_apps() {
    let _env = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let root = format!(r"Software\RiptideUninstallTest_{}", std::process::id());
    std::env::set_var(REGISTRY_ROOT_ENV, &root);
    let exe = std::env::current_exe().unwrap();
    let mut reg = platform_registry().unwrap();

    // another app owns magnet: and .torrent before Riptide is registered
    let cmd_key = format!(r"{MAGNET_KEY}\shell\open\command");
    reg.set_default_value(MAGNET_KEY, "URL:Magnet Protocol")
        .unwrap();
    reg.set_default_value(&cmd_key, FOREIGN).unwrap();
    reg.set_default_value(TORRENT_EXT_KEY, "qBittorrent.File.torrent")
        .unwrap();

    let s = register(&exe, reg.as_mut()).unwrap();
    assert!(s.magnet && s.torrent);

    // the uninstall hook
    assert_eq!(run_cli(&args(&["--unregister-handlers"])), Some(0));
    let reg = platform_registry().unwrap();
    let s = status(&exe, reg.as_ref());
    assert!(!s.magnet && !s.torrent, "{s:?}");
    assert_eq!(reg.default_value(&cmd_key).as_deref(), Some(FOREIGN));
    assert_eq!(
        reg.default_value(TORRENT_EXT_KEY).as_deref(),
        Some("qBittorrent.File.torrent"),
        "another app's .torrent default is untouched"
    );
    assert_eq!(
        reg.default_value(r"Software\Classes\Riptide.Torrent\shell\open\command"),
        None
    );

    // running it again, with nothing of ours left, is harmless
    assert_eq!(run_cli(&args(&["--unregister-handlers"])), Some(0));
    assert_eq!(
        platform_registry()
            .unwrap()
            .default_value(&cmd_key)
            .as_deref(),
        Some(FOREIGN)
    );

    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&root);
    std::env::remove_var(REGISTRY_ROOT_ENV);
}

#[test]
fn with_no_foreign_handler_everything_of_ours_is_gone_and_old_registrations_are_removed_too() {
    let _env = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let root = format!(r"Software\RiptideUninstallTest2_{}", std::process::id());
    std::env::set_var(REGISTRY_ROOT_ENV, &root);
    let exe = std::env::current_exe().unwrap();
    let mut reg = platform_registry().unwrap();
    // an earlier preview registered the bare form
    let legacy = format!("\"{}\" \"%1\"", exe.display());
    reg.set_default_value(MAGNET_KEY, "URL:Magnet Protocol")
        .unwrap();
    reg.set_default_value(&format!(r"{MAGNET_KEY}\shell\open\command"), &legacy)
        .unwrap();
    assert!(
        status(&exe, reg.as_ref()).magnet,
        "the old form still counts"
    );
    // the next start moves it to the current form
    migrate_legacy(&exe, reg.as_mut());
    let now = reg
        .default_value(&format!(r"{MAGNET_KEY}\shell\open\command"))
        .unwrap();
    assert!(now.contains("--open-link"), "{now}");

    assert_eq!(run_cli(&args(&["--unregister-handlers"])), Some(0));
    let reg = platform_registry().unwrap();
    assert_eq!(
        reg.default_value(MAGNET_KEY),
        None,
        "the magnet key is gone"
    );
    // nothing of ours is left under Classes, not even the empty shared `.torrent` key
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let left: Vec<String> = hkcu
        .open_subkey(format!(r"{root}\Software\Classes"))
        .map(|k| k.enum_keys().flatten().collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "left behind: {left:?}");

    // an old registration is removed by the uninstaller as well
    let mut reg = platform_registry().unwrap();
    reg.set_default_value(MAGNET_KEY, "URL:Magnet Protocol")
        .unwrap();
    reg.set_default_value(&format!(r"{MAGNET_KEY}\shell\open\command"), &legacy)
        .unwrap();
    assert_eq!(run_cli(&args(&["--unregister-handlers"])), Some(0));
    assert_eq!(platform_registry().unwrap().default_value(MAGNET_KEY), None);

    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&root);
    std::env::remove_var(REGISTRY_ROOT_ENV);
}

#[test]
fn a_plain_register_and_uninstall_leaves_nothing_under_classes() {
    let _env = ENV.lock().unwrap_or_else(|p| p.into_inner());
    let root = format!(r"Software\RiptideUninstallTest3_{}", std::process::id());
    std::env::set_var(REGISTRY_ROOT_ENV, &root);
    let exe = std::env::current_exe().unwrap();
    let mut reg = platform_registry().unwrap();
    register(&exe, reg.as_mut()).unwrap();
    assert_eq!(run_cli(&args(&["--unregister-handlers"])), Some(0));
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let left: Vec<String> = hkcu
        .open_subkey(format!(r"{root}\Software\Classes"))
        .map(|k| k.enum_keys().flatten().collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "left behind: {left:?}");
    let _ = hkcu.delete_subkey_all(&root);
    std::env::remove_var(REGISTRY_ROOT_ENV);
}
