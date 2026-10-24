//! Build script: embeds `tauri.conf.json`, the icons and the generated capability schemas.

fn main() {
    // Unit tests that use `tauri::test` (the updater specs) link the whole runtime, whose
    // Windows imports need Common Controls v6; without a manifest the test exe does not start
    // (STATUS_ENTRYPOINT_NOT_FOUND). The app itself gets its manifest from `tauri_build`.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let manifest =
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
                .join("windows")
                .join("test.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    tauri_build::build();
}
