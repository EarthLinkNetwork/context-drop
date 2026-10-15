//! In-app updates (tauri-plugin-updater): check GitHub's latest.json for a
//! newer, signed release and, when the user asks, download it, replace the
//! installed .app and restart.
//!
//! Everything runs in the backend; the WebView only calls `check_for_updates`
//! / `install_update` and reads the status from the snapshot. One check or
//! install runs at a time (`busy`), released on every exit path by `Busy`.
//! When an automatic check is due is decided by `context_drop_core::updates`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};

use context_drop_core::updates::{install_location_problem, CheckClock};
use serde::Serialize;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Fetching latest.json gives up after this long.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Downloading the update gives up after this long in total.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The first automatic check runs this long after launch.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
/// How often the automatic-check loop wakes to see whether a check is due.
/// Short, because the wall-clock day it waits for may pass while asleep.
const TICK: Duration = Duration::from_secs(10 * 60);

pub const TRAY_ITEM_ID: &str = "update";
const TRAY_CHECK_TEXT: &str = "Check for Updates…";

/// What the UI shows about updates (part of the app snapshot).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// "idle" | "checking" | "upToDate" | "available" | "downloading" | "installing"
    pub state: &'static str,
    /// The newer version, while one is known ("available" and later).
    pub version: Option<String>,
    /// Its release notes.
    pub notes: Option<String>,
    /// Download progress in bytes ("downloading").
    pub downloaded: u64,
    pub total: Option<u64>,
    /// The last check's or install's failure, cleared when one succeeds.
    pub error: Option<String>,
    /// "check" | "install": which of the two failed.
    pub error_during: Option<&'static str>,
    /// When the last successful check finished (ms since the epoch).
    pub checked_at: Option<i64>,
    /// Why this copy of the app cannot be replaced (dev build, run from the
    /// disk image…), if so. Install is refused while set.
    pub blocked_reason: Option<String>,
    /// Updates exist for this platform at all (macOS only).
    pub supported: bool,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        UpdateStatus {
            state: "idle",
            version: None,
            notes: None,
            downloaded: 0,
            total: None,
            error: None,
            error_during: None,
            checked_at: None,
            blocked_reason: location_problem(),
            supported: cfg!(target_os = "macos"),
        }
    }
}

/// Update state shared by the commands, the tray and the automatic checks.
#[derive(Default)]
pub struct Updates {
    status: Mutex<UpdateStatus>,
    /// The update found by the last check, ready to install.
    pending: Mutex<Option<Update>>,
    busy: AtomicBool,
    clock: Mutex<CheckClock>,
    tray_item: Mutex<Option<MenuItem<tauri::Wry>>>,
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn set_tray_item(&self, item: MenuItem<tauri::Wry>) {
        *self.tray_item.lock().unwrap() = Some(item);
    }
}

/// Marks a check / install as running; clears the mark when dropped, so a
/// failure, timeout or panic can never leave updates stuck as "busy".
struct Busy(AppHandle);

impl Busy {
    fn take(app: &AppHandle) -> Option<Busy> {
        let updates = app.state::<Updates>();
        if updates.busy.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(Busy(app.clone()))
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0
            .state::<Updates>()
            .busy
            .store(false, Ordering::SeqCst);
    }
}

fn location_problem() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    install_location_problem(&exe).map(str::to_string)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Change the status, then repaint the tray item and the UI.
fn update_status(app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) {
    let status = {
        let updates = app.state::<Updates>();
        let mut s = updates.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let item = app.state::<Updates>().tray_item.lock().unwrap().clone();
    if let Some(item) = item {
        let text = match (status.state, &status.version) {
            ("available" | "downloading" | "installing", Some(v)) => format!("Update to v{v}…"),
            _ => TRAY_CHECK_TEXT.to_string(),
        };
        let _ = item.set_text(text);
    }
    let _ = app.emit("cd:refresh", ());
}

/// Check for a newer release. `manual` = the user asked (Check for Updates);
/// an automatic check also needs a replaceable install. Returns the status
/// afterwards. Does nothing while another check or an install is running.
pub async fn check(app: &AppHandle, manual: bool) -> UpdateStatus {
    let updates = app.state::<Updates>();
    if !cfg!(target_os = "macos") {
        return updates.status();
    }
    if !manual && location_problem().is_some() {
        return updates.status();
    }
    let Some(_busy) = Busy::take(app) else {
        return updates.status();
    };
    update_status(app, |s| {
        s.state = "checking";
        s.error = None;
        s.error_during = None;
    });

    let result = match app.updater_builder().timeout(CHECK_TIMEOUT).build() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let now = SystemTime::now();
    match result {
        Ok(found) => {
            updates.clock.lock().unwrap().last_success = Some(now);
            let (version, notes) = match &found {
                Some(u) => (Some(u.version.clone()), u.body.clone()),
                None => (None, None),
            };
            *updates.pending.lock().unwrap() = found;
            update_status(app, |s| {
                s.state = if version.is_some() {
                    "available"
                } else {
                    "upToDate"
                };
                s.version = version;
                s.notes = notes;
                s.checked_at = Some(now_ms());
                s.blocked_reason = location_problem();
            });
        }
        Err(e) => {
            updates.clock.lock().unwrap().last_failure = Some(now);
            // A failed re-check keeps an update that was already found.
            let has_pending = updates.pending.lock().unwrap().is_some();
            update_status(app, |s| {
                s.state = if has_pending { "available" } else { "idle" };
                s.error = Some(format!("Could not check for updates: {e}"));
                s.error_during = Some("check");
            });
        }
    }
    updates.status()
}

/// Download, verify and install the update found by the last check, then
/// restart. On failure the current app keeps running and the update stays
/// ready for another try.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let updates = app.state::<Updates>();
    let Some(_busy) = Busy::take(app) else {
        return Err("An update check or install is already running.".into());
    };
    if let Some(problem) = location_problem() {
        return Err(problem);
    }
    let Some(mut update) = updates.pending.lock().unwrap().clone() else {
        return Err("No update is ready. Check for updates first.".into());
    };
    update.timeout = Some(DOWNLOAD_TIMEOUT);

    update_status(app, |s| {
        s.state = "downloading";
        s.downloaded = 0;
        s.total = None;
        s.error = None;
        s.error_during = None;
    });
    let progress = app.clone();
    let downloaded = update
        .download(
            move |chunk, total| {
                let updates = progress.state::<Updates>();
                let mut s = updates.status.lock().unwrap();
                s.downloaded += chunk as u64;
                s.total = total;
            },
            || {},
        )
        .await;
    let result = match downloaded {
        // `install` must not run on the main thread: when /Applications needs
        // an administrator password, the plugin runs that prompt on the main
        // thread and waits for it.
        Ok(bytes) => {
            update_status(app, |s| s.state = "installing");
            let to_install = update.clone();
            tauri::async_runtime::spawn_blocking(move || to_install.install(bytes))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()))
        }
        Err(e) => Err(e.to_string()),
    };
    match result {
        Ok(()) => {
            // The new .app is in place: start it (this process exits).
            app.restart();
        }
        Err(e) => {
            let message = format!("Could not install the update: {e}");
            update_status(app, |s| {
                s.state = "available";
                s.error = Some(message.clone());
                s.error_during = Some("install");
            });
            Err(message)
        }
    }
}

/// The automatic checks: once shortly after launch, then whenever a day of
/// wall-clock time has passed since the last successful check (failures are
/// retried an hour later). Runs while the app runs.
pub fn spawn_automatic_checks(
    app: AppHandle,
    enabled: impl Fn(&AppHandle) -> bool + Send + 'static,
) {
    thread::spawn(move || {
        thread::sleep(FIRST_CHECK_DELAY);
        loop {
            let due = {
                let clock = *app.state::<Updates>().clock.lock().unwrap();
                clock.due(enabled(&app), SystemTime::now())
            };
            if due {
                tauri::async_runtime::block_on(check(&app, false));
            }
            thread::sleep(TICK);
        }
    });
}

/// The tray's update item: open the window, and check first unless an update
/// is already known (the window then shows it).
pub fn on_tray_item(app: &AppHandle, show_window: impl Fn(&AppHandle)) {
    show_window(app);
    let known = app.state::<Updates>().pending.lock().unwrap().is_some();
    if !known {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            check(&app, true).await;
        });
    }
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(check(&app, true).await)
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    install(&app).await
}
