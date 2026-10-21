//! A launch that opens a link or file never runs the installer hooks, whatever the link
//! contained (review round of alpha.5, P2-3). The registry and the manifests are moved to
//! throw-away places first, so even a failing run touches nothing of the real installation.
#![allow(clippy::unwrap_used, missing_docs)]

use riptide_desktop_lib::integration::{run_cli, INTEGRATION_DIR_ENV, REGISTRY_ROOT_ENV};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn a_split_link_cannot_run_the_unregister_hook() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var(
        REGISTRY_ROOT_ENV,
        format!(r"Software\RiptideOpenLaunchTest_{}", std::process::id()),
    );
    std::env::set_var(INTEGRATION_DIR_ENV, dir.path());
    let link = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=x";
    for hostile in [
        // the registered command with a link that was split by an unescaped quote
        args(&["--open-link", "--", link, "--unregister-host"]),
        args(&["--open-link", "--", link, "--register-host"]),
        // an earlier registration
        args(&[link, "--unregister-host"]),
        args(&["magnet:x", "--unregister-host"]),
    ] {
        assert_eq!(run_cli(&hostile), None, "{hostile:?}");
    }
    // the hooks themselves still work for the installer (against the throw-away root)
    assert!(run_cli(&args(&["--unregister-host"])).is_some());
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        let _ = winreg::RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(format!(
            r"Software\RiptideOpenLaunchTest_{}",
            std::process::id()
        ));
    }
    std::env::remove_var(REGISTRY_ROOT_ENV);
    std::env::remove_var(INTEGRATION_DIR_ENV);
}
