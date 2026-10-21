//! The magnet / `.torrent` handlers against the real per-user registry, moved under a throw-away
//! key with the `RIPTIDE_REGISTRY_ROOT` knob (the same one the installer tests use), so the
//! machine's real `magnet:` handler is never touched. Windows only.
#![cfg(windows)]
#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;

use riptide_desktop_lib::integration::{platform_registry, REGISTRY_ROOT_ENV};
use riptide_desktop_lib::protocol::{register, status, unregister};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

fn read(root: &str, key: &str, name: &str) -> Option<String> {
    hkcu()
        .open_subkey_with_flags(format!(r"{root}\{key}"), KEY_READ)
        .ok()?
        .get_value::<String, _>(name)
        .ok()
}

#[test]
fn registering_and_unregistering_through_the_registry_root_knob() {
    // one key of its own, so deleting it at the end leaves nothing behind
    let root = format!(r"Software\RiptideProtocolTest_{}", std::process::id());
    std::env::set_var(REGISTRY_ROOT_ENV, &root);
    let exe = Path::new(r"C:\Riptide Test\riptide-desktop.exe");

    let mut registry = platform_registry().expect("a registry on Windows");
    assert!(!status(exe, registry.as_ref()).magnet);

    let s = register(exe, registry.as_mut()).unwrap();
    assert!(s.supported && s.magnet && s.torrent);

    // what the shell reads, under the test root and nowhere else
    assert_eq!(
        read(&root, r"Software\Classes\magnet", "URL Protocol").as_deref(),
        Some("")
    );
    assert_eq!(
        read(&root, r"Software\Classes\magnet\shell\open\command", "").as_deref(),
        Some(r#""C:\Riptide Test\riptide-desktop.exe" --open-link -- "%1""#)
    );
    assert_eq!(
        read(
            &root,
            r"Software\Classes\Riptide.Torrent\shell\open\command",
            ""
        )
        .as_deref(),
        Some(r#""C:\Riptide Test\riptide-desktop.exe" --open-link -- "%1""#)
    );
    assert_eq!(
        read(
            &root,
            r"Software\Classes\.torrent\OpenWithProgids",
            "Riptide.Torrent"
        )
        .as_deref(),
        Some("")
    );
    assert_eq!(
        read(&root, r"Software\Classes\.torrent", "").as_deref(),
        Some("Riptide.Torrent")
    );

    // a second install is not touched by the first one's unregister
    let other = Path::new(r"D:\Other\riptide-desktop.exe");
    assert!(!status(other, registry.as_ref()).magnet);
    let s = unregister(other, registry.as_mut()).unwrap();
    assert!(!s.magnet && !s.torrent);
    assert!(read(&root, r"Software\Classes\magnet\shell\open\command", "").is_some());

    let s = unregister(exe, registry.as_mut()).unwrap();
    assert!(!s.magnet && !s.torrent);
    assert!(read(&root, r"Software\Classes\magnet", "").is_none());
    assert!(read(&root, r"Software\Classes\Riptide.Torrent", "").is_none());
    assert!(read(
        &root,
        r"Software\Classes\.torrent\OpenWithProgids",
        "Riptide.Torrent"
    )
    .is_none());
    assert!(read(&root, r"Software\Classes\.torrent", "").is_none());

    // clean up the throw-away root
    let _ = hkcu().delete_subkey_all(&root);
    std::env::remove_var(REGISTRY_ROOT_ENV);
}
