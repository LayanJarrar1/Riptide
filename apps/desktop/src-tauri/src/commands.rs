//! `#[tauri::command]`s the webview may invoke.
//!
//! Commands live in their own module (not at the crate root) because Tauri's command macro
//! both `#[macro_export]`s a helper macro and re-exports it with the function's visibility;
//! a `pub` command at the crate root therefore defines the same name twice (E0255).

use serde::Serialize;

/// Milestone label embedded in the binary.
pub const BUILD_MILESTONE: &str = "M0";

/// Product name shown in the window title and the About screen.
pub const PRODUCT_NAME: &str = "Riptide";

/// Static facts about the running application, reported to the UI once at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    /// Product name.
    pub name: &'static str,
    /// Crate version (`CARGO_PKG_VERSION`), inherited from the workspace.
    pub version: &'static str,
    /// Milestone the binary was built from.
    pub build: &'static str,
}

/// Returns the application's name, version and milestone.
///
/// Exposed to the webview as the `app_info` command.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        build: BUILD_MILESTONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_name_and_version() {
        let info = app_info();
        assert_eq!(info.name, "Riptide");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.version.is_empty());
        assert_eq!(info.build, "M0");
    }

    #[test]
    fn app_info_serializes_with_the_field_names_the_ui_expects() {
        let json = serde_json::to_value(app_info()).unwrap();
        assert_eq!(json["name"], "Riptide");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["build"], "M0");
        assert_eq!(json.as_object().unwrap().len(), 3);
    }
}
