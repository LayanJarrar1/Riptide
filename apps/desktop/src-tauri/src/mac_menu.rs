//! The macOS menu bar: the app menu (About, Hide, Quit), Edit (so ⌘C / ⌘V / ⌘A work in text
//! fields) and Window. It replaces Tauri's default menu because that one's Quit ends the process
//! on the spot: ours goes through [`exit::request_quit`], which asks first while downloads run
//! and stops them at a checkpoint. Everything else stays in the in-window menu bar.

use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

use crate::exit;

/// Id of the Quit item.
const MENU_QUIT: &str = "mac.quit";

/// Builds the menu bar (Builder::menu).
///
/// # Errors
///
/// A menu item that cannot be created.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = AboutMetadata {
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        ..AboutMetadata::default()
    };
    let app_menu = Submenu::with_items(
        app,
        "Riptide",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Riptide"), Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, MENU_QUIT, "Quit Riptide", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            // a close request: close-to-tray and the exit prompt apply as for the red button
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &window])
}

/// Builder::on_menu_event. Tray items arrive here too; only ours are handled.
pub fn on_menu<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    if event.id().as_ref() == MENU_QUIT {
        exit::request_quit(app);
    }
}
