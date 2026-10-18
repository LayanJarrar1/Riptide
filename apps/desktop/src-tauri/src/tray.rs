//! The system-tray icon: a tooltip with the total speed and the number of active downloads
//! (refreshed at most once a second from engine events) and a menu with Open, Pause all,
//! Resume all and Quit, labelled in the app language (see [`crate::shell::ShellLabels`]).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use riptide_core::{DownloadId, Event};
use riptide_engine::Engine;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::shell::{self, Shell, ShellLabels};

/// Id of the tray icon.
pub const TRAY_ID: &str = "main";

const MENU_OPEN: &str = "tray.open";
const MENU_PAUSE_ALL: &str = "tray.pause_all";
const MENU_RESUME_ALL: &str = "tray.resume_all";
const MENU_QUIT: &str = "tray.quit";

/// `12.5 MB/s` with the given unit labels; same rounding as the webview's `FormatService`
/// (base 1024; two decimals under 10, one under 100, none above; plain bytes as integers).
pub fn format_speed(labels: &ShellLabels, bps: u64) -> String {
    let mut value = bps as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < labels.units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if unit == 0 || value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    format!("{text} {}{}", labels.units[unit], labels.per_second)
}

/// The tooltip text: the idle label with nothing running, else the active label with
/// `{count}` and `{speed}` filled in.
pub fn tray_tooltip(labels: &ShellLabels, active: usize, speed_bps: u64) -> String {
    if active == 0 {
        return labels.tooltip_idle.clone();
    }
    labels
        .tooltip_active
        .replace("{count}", &active.to_string())
        .replace("{speed}", &format_speed(labels, speed_bps))
}

/// The menu items, kept so a language switch can relabel them; plus the last tooltip set.
struct TrayItems<R: Runtime> {
    open: MenuItem<R>,
    pause_all: MenuItem<R>,
    resume_all: MenuItem<R>,
    quit: MenuItem<R>,
    last_tooltip: Mutex<String>,
}

/// Creates the tray icon and starts the tooltip updater.
///
/// # Errors
///
/// When the menu or the icon cannot be created (no notification area); the app still runs.
pub fn install<R: Runtime>(app: &AppHandle<R>, engine: Engine) -> tauri::Result<()> {
    let labels = shell::prefs(app).labels;
    let open = MenuItem::with_id(app, MENU_OPEN, &labels.open, true, None::<&str>)?;
    let pause_all = MenuItem::with_id(app, MENU_PAUSE_ALL, &labels.pause_all, true, None::<&str>)?;
    let resume_all =
        MenuItem::with_id(app, MENU_RESUME_ALL, &labels.resume_all, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, &labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &separator,
            &pause_all,
            &resume_all,
            &separator2,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(&labels.tooltip_idle)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_icon);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    app.manage(TrayItems {
        open,
        pause_all,
        resume_all,
        quit,
        last_tooltip: Mutex::new(labels.tooltip_idle.clone()),
    });

    let handle = app.clone();
    tauri::async_runtime::spawn(update_tooltip(handle, engine));
    Ok(())
}

/// Relabels the menu (language switch) and forces the next tooltip refresh.
pub fn relabel<R: Runtime>(app: &AppHandle<R>, labels: &ShellLabels) {
    let Some(items) = app.try_state::<TrayItems<R>>() else {
        return;
    };
    for (item, text) in [
        (&items.open, &labels.open),
        (&items.pause_all, &labels.pause_all),
        (&items.resume_all, &labels.resume_all),
        (&items.quit, &labels.quit),
    ] {
        if let Err(e) = item.set_text(text) {
            tracing::debug!(error = %e, "tray item not relabelled");
        }
    }
    items
        .last_tooltip
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

fn on_icon<R: Runtime>(tray: &TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        crate::show_main_window(tray.app_handle());
    }
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_OPEN => crate::show_main_window(app),
        MENU_PAUSE_ALL | MENU_RESUME_ALL => {
            let pause = event.id().as_ref() == MENU_PAUSE_ALL;
            if let Some(engine) = app.try_state::<Engine>() {
                let engine = engine.inner().clone();
                tauri::async_runtime::spawn(async move {
                    let result = if pause {
                        engine.pause_all().await
                    } else {
                        engine.resume_all().await
                    };
                    if let Err(e) = result {
                        tracing::warn!(error = %e, pause, "tray pause/resume all failed");
                    }
                });
            }
        }
        MENU_QUIT => quit(app),
        _ => {}
    }
}

/// Exits for real: stops the transfers at a checkpoint (as closing did before the tray), then
/// ends the process.
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(shell) = app.try_state::<Shell>() {
        shell.begin_quit();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(engine) = app.try_state::<Engine>() {
            let engine = engine.inner().clone();
            engine.shutdown().await;
        }
        app.exit(0);
    });
}

/// Keeps the latest speed of every download and refreshes the tooltip once a second, only
/// when its text changed.
async fn update_tooltip<R: Runtime>(app: AppHandle<R>, engine: Engine) {
    let mut rx = engine.subscribe();
    let mut speeds: HashMap<DownloadId, u64> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(Event::Progress { id, speed_bps, .. }) => {
                    speeds.insert(id, speed_bps);
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            _ = tick.tick() => {
                let running = engine.running_ids();
                speeds.retain(|id, _| running.contains(id));
                let total: u64 = speeds.values().sum();
                let labels = shell::prefs(&app).labels;
                let text = tray_tooltip(&labels, running.len(), total);
                set_tooltip(&app, text);
            }
        }
    }
}

fn set_tooltip<R: Runtime>(app: &AppHandle<R>, text: String) {
    let Some(items) = app.try_state::<TrayItems<R>>() else {
        return;
    };
    let mut last = items
        .last_tooltip
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if *last == text {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_tooltip(Some(&text)) {
            tracing::debug!(error = %e, "tray tooltip not updated");
            return;
        }
    }
    *last = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_matches_the_webview_rounding() {
        let l = ShellLabels::default();
        assert_eq!(format_speed(&l, 0), "0 B/s");
        assert_eq!(format_speed(&l, 900), "900 B/s");
        assert_eq!(format_speed(&l, 1536), "1.50 KB/s");
        assert_eq!(format_speed(&l, 12 * 1024 * 1024 + 512 * 1024), "12.5 MB/s");
        assert_eq!(format_speed(&l, 250 * 1024 * 1024), "250 MB/s");
        assert_eq!(format_speed(&l, 3 * 1024_u64.pow(5)), "3072 TB/s");
    }

    #[test]
    fn tooltip_idle_and_active() {
        let l = ShellLabels::default();
        assert_eq!(tray_tooltip(&l, 0, 5_000), "Riptide");
        assert_eq!(
            tray_tooltip(&l, 3, 2 * 1024 * 1024),
            "Riptide: 3 active, 2.00 MB/s"
        );
    }

    #[test]
    fn tooltip_uses_the_translated_labels() {
        let l = ShellLabels {
            tooltip_active: "ريبتايد: {count} نشطة · {speed}".into(),
            units: [
                "بايت".into(),
                "ك.ب".into(),
                "م.ب".into(),
                "ج.ب".into(),
                "ت.ب".into(),
            ],
            per_second: "/ث".into(),
            ..ShellLabels::default()
        };
        assert_eq!(tray_tooltip(&l, 1, 1024), "ريبتايد: 1 نشطة · 1.00 ك.ب/ث");
    }
}
