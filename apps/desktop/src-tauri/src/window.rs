//! The main window on multi-display desktops: which screen it opens on,
//! always-on-top, and showing it on every desktop (Space).
//!
//! The geometry lives in `context_drop_core::placement` (pure, tested); this
//! module only converts Tauri's monitor / window / cursor values into its one
//! global coordinate space and applies the result.
//!
//! macOS coordinate note (tao 0.37): a monitor's physical position and size
//! are its logical (point) values times THAT monitor's scale factor, while the
//! cursor position is logical times the PRIMARY monitor's scale factor. With
//! mixed-DPI displays those physical values are not one space, so on macOS
//! everything is converted back to points (each value with its own factor) and
//! the window is moved with a logical position. Other platforms already report
//! one physical space.

use context_drop_core::placement::{self, Display, Plan, Rect};
use context_drop_core::Settings;
use serde::Serialize;
use tauri::{AppHandle, Manager, Monitor, WebviewWindow};

pub const MAIN: &str = "main";

/// A connected display, for the Settings picker.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    /// Saved in `Settings::window_display`.
    pub key: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[cfg(target_os = "macos")]
fn monitor_scale(m: &Monitor) -> f64 {
    m.scale_factor()
}

#[cfg(not(target_os = "macos"))]
fn monitor_scale(_m: &Monitor) -> f64 {
    1.0
}

fn to_display(m: &Monitor) -> Display {
    let s = monitor_scale(m);
    let (p, z, wa) = (m.position(), m.size(), m.work_area());
    Display {
        name: m.name().cloned().unwrap_or_else(|| "Display".to_string()),
        frame: Rect {
            x: p.x as f64 / s,
            y: p.y as f64 / s,
            w: z.width as f64 / s,
            h: z.height as f64 / s,
        },
        work: Rect {
            x: wa.position.x as f64 / s,
            y: wa.position.y as f64 / s,
            w: wa.size.width as f64 / s,
            h: wa.size.height as f64 / s,
        },
    }
}

pub fn displays(app: &AppHandle) -> Vec<Display> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(to_display)
        .collect()
}

pub fn display_infos(app: &AppHandle) -> Vec<DisplayInfo> {
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| to_display(&m))
        .map(|d| placement::display_key(&d.name, &d.frame));
    displays(app)
        .into_iter()
        .map(|d| {
            let key = placement::display_key(&d.name, &d.frame);
            DisplayInfo {
                primary: primary.as_deref() == Some(key.as_str()),
                width: d.frame.w.round() as u32,
                height: d.frame.h.round() as u32,
                name: d.name,
                key,
            }
        })
        .collect()
}

fn cursor(app: &AppHandle) -> Option<(f64, f64)> {
    let p = app.cursor_position().ok()?;
    #[cfg(target_os = "macos")]
    let s = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    #[cfg(not(target_os = "macos"))]
    let s = 1.0;
    Some((p.x / s, p.y / s))
}

#[cfg(target_os = "macos")]
fn window_scale(win: &WebviewWindow) -> f64 {
    win.scale_factor().unwrap_or(1.0)
}

#[cfg(not(target_os = "macos"))]
fn window_scale(_win: &WebviewWindow) -> f64 {
    1.0
}

fn window_rect(win: &WebviewWindow) -> Option<Rect> {
    let s = window_scale(win);
    let p = win.outer_position().ok()?;
    let z = win.outer_size().ok()?;
    Some(Rect {
        x: p.x as f64 / s,
        y: p.y as f64 / s,
        w: z.width as f64 / s,
        h: z.height as f64 / s,
    })
}

/// The window's top-left corner in the global space (what `remember` saves).
pub fn current_position(win: &WebviewWindow) -> Option<(f64, f64)> {
    window_rect(win).map(|r| (r.x, r.y))
}

fn move_to(win: &WebviewWindow, x: f64, y: f64) {
    #[cfg(target_os = "macos")]
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    #[cfg(not(target_os = "macos"))]
    let _ = win.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// On macOS `set_position` only queues the move on the main dispatch queue,
/// while `show` orders the window front at once — so without waiting, the
/// window would flash on its old screen before jumping. Poll (briefly) until
/// the move has landed. Call off the main thread only: the move needs the main
/// thread to run, and the position getter round-trips through it.
fn wait_until_at(win: &WebviewWindow, x: f64, y: f64) {
    for _ in 0..25 {
        if let Some(r) = window_rect(win) {
            if (r.x - x).abs() < 2.0 && (r.y - y).abs() < 2.0 {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

/// Show + focus the main window on the screen `settings` asks for. Call off
/// the main thread (see `wait_until_at`).
pub fn show(app: &AppHandle, settings: &Settings) {
    let Some(win) = app.get_webview_window(MAIN) else {
        return;
    };
    if let Some(rect) = window_rect(&win) {
        let plan = placement::plan_show(
            settings.window_placement,
            &displays(app),
            cursor(app),
            &settings.window_display,
            rect,
        );
        if let Plan::MoveTo(x, y) = plan {
            move_to(&win, x, y);
            wait_until_at(&win, x, y);
        }
    }
    // Undo a previous app-level hide (see `hide`).
    #[cfg(target_os = "macos")]
    let _ = app.show();
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

/// Whether the window is currently in front of the user (visible AND focused),
/// i.e. a "show/hide" shortcut press should hide it rather than show it.
pub fn is_frontmost(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN).is_some_and(|w| {
        w.is_visible().unwrap_or(false)
            && !w.is_minimized().unwrap_or(false)
            && w.is_focused().unwrap_or(false)
    })
}

/// Hide the window and, on macOS, the app too, so keyboard focus goes back to
/// the app the user was in (hiding only the window of this Dock-less app
/// would leave it active with no window).
pub fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN) {
        let _ = win.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.hide();
}

/// Apply always-on-top and show-on-all-desktops to the main window.
pub fn apply_flags(app: &AppHandle, settings: &Settings) {
    if let Some(win) = app.get_webview_window(MAIN) {
        let _ = win.set_always_on_top(settings.always_on_top);
        let _ = win.set_visible_on_all_workspaces(settings.show_on_all_desktops);
    }
}

/// Put the window back at a remembered position, unless that spot is no longer
/// on any connected display (e.g. the monitor was unplugged).
pub fn restore(app: &AppHandle, x: f64, y: f64) {
    let Some(win) = app.get_webview_window(MAIN) else {
        return;
    };
    let w = window_rect(&win).map(|r| r.w).unwrap_or(380.0);
    if placement::position_is_visible(&displays(app), x, y, w) {
        move_to(&win, x, y);
    }
}
