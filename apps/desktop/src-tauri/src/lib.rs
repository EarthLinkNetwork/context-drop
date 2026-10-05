//! Context Drop desktop app (Tauri 2).
//!
//! A menu-bar / system-tray utility. Capture is OFF by default and only watches
//! the clipboard while ON (never an always-on daemon). The app and the CLI
//! coordinate purely through the shared SQLite DB + filesystem. The desktop
//! never sends anything over the network.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::menu::MenuBuilder;
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_opener::OpenerExt;

use context_drop_core::clock::now_ms;
use context_drop_core::{
    append_snapshot, cleanup, clear_packet as core_clear_packet, create_draft, current_draft_id,
    integration, packet_summary, recent_items, snapshot_content_hash, status,
    undo_last_dispatch, AppendOutcome, CapturedItem, CapturedSnapshot, Db, ItemKind, Limits,
    PacketItem, PacketState, Settings, Storage, DEFAULT_UNDO_WINDOW_MS,
};
use context_drop_clipboard::{ClipboardProvider, Payload, Watcher};

/// How often the capture loop polls the clipboard while ON. Cheap change tokens
/// mean an unchanged clipboard costs almost nothing; this never busy-loops.
const POLL_INTERVAL: Duration = Duration::from_millis(350);
const TRAY_ID: &str = "main";
const RECENT_ITEMS: usize = 8;
/// How many past dispatches the desktop lists (newest first).
const RECENT_DISPATCHES: usize = 10;

/// Shared application state. Fields the capture thread needs are `Arc`s so they
/// can be cloned into the thread; the rest are accessed via the Tauri `State`.
struct AppState {
    storage: Storage,
    capturing: Arc<AtomicBool>,
    /// Generation of the current capture session. Incremented on every start /
    /// stop; a capture worker only mutates shared state while its generation is
    /// still current, so a retired worker can never stop a newer capture.
    capture_generation: Arc<AtomicU64>,
    current_draft: Arc<Mutex<Option<String>>>,
    notice: Arc<Mutex<Option<String>>>,
    /// Size limits shared with the capture worker so a settings change takes
    /// effect on the very next clipboard event (no restart needed).
    limits: Arc<Mutex<Limits>>,
    shortcut: Mutex<String>,
    shortcut_registered: Arc<AtomicBool>,
    /// Serializes capture lifecycle actions (start/stop/clear/save) AND the
    /// capture worker's own state mutations, so their check-then-act sequences
    /// cannot interleave. Shared with the worker via `Arc`.
    lifecycle: Arc<Mutex<()>>,
    /// Cache of image thumbnails (data URLs) keyed by item content hash, so the
    /// same image is not re-decoded on every snapshot refresh/poll.
    thumb_cache: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl AppState {
    fn new(storage: Storage) -> AppState {
        AppState {
            storage,
            capturing: Arc::new(AtomicBool::new(false)),
            capture_generation: Arc::new(AtomicU64::new(0)),
            current_draft: Arc::new(Mutex::new(None)),
            notice: Arc::new(Mutex::new(None)),
            limits: Arc::new(Mutex::new(Limits::default())),
            shortcut: Mutex::new(String::new()),
            shortcut_registered: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(Mutex::new(())),
            thumb_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// ---- Serializable snapshot for the frontend (camelCase) -----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecentItem {
    /// Item id — lets the UI open the full-content viewer or delete this item.
    id: String,
    kind: String,
    mime_type: String,
    byte_size: i64,
    /// First ~100 chars of a text-like item (so the user can tell WHAT they
    /// copied), else `None`.
    preview: Option<String>,
    /// A small PNG thumbnail as a `data:` URL for image items (and dropped image
    /// files), else `None`.
    image_thumb: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentDraft {
    packet_id: String,
    item_count: i64,
    recent_items: Vec<RecentItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LastDispatchInfo {
    packet_id: String,
    project_name: String,
    item_count: i64,
    claimed_at: String,
    state: String,
    session_id: String,
    cwd: String,
    config_dir: Option<String>,
    /// The user's pull instruction (recorded by the plugin), if any.
    note: Option<String>,
    /// Terminal tab/pane label (e.g. "iTerm2 w0t2p0"), if known.
    terminal: Option<String>,
}

impl From<&context_drop_core::LastDispatch> for LastDispatchInfo {
    fn from(l: &context_drop_core::LastDispatch) -> Self {
        LastDispatchInfo {
            packet_id: l.packet_id.clone(),
            project_name: l.project_name.clone(),
            item_count: l.item_count,
            claimed_at: context_drop_core::clock::ms_to_rfc3339(l.claimed_at_ms),
            // A claim undone (and maybe re-claimed by another session) shows as
            // RELEASED rather than the packet's current state.
            state: if l.claim_status == "released" {
                "RELEASED".to_string()
            } else {
                l.packet_state.as_str().to_string()
            },
            session_id: l.session_id.clone(),
            cwd: l.cwd.clone(),
            config_dir: l.config_dir.clone(),
            note: l.note.clone(),
            terminal: l.terminal.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationStatus {
    config_dir: String,
    /// The app staged the local marketplace files under this config dir.
    installed: bool,
    /// The plugin is registered in Claude Code here (user ran `/plugin install`).
    enabled: bool,
    /// Local marketplace dir (for the offline `/plugin marketplace add <path>`).
    marketplace_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSnapshot {
    capturing: bool,
    shortcut: String,
    shortcut_registered: bool,
    current_draft: Option<CurrentDraft>,
    last_dispatch: Option<LastDispatchInfo>,
    /// The most recent dispatches, newest first (capped at RECENT_DISPATCHES).
    recent_dispatches: Vec<LastDispatchInfo>,
    ready_count: i64,
    settings: Settings,
    integrations: Vec<IntegrationStatus>,
    /// The public GitHub marketplace slug for `/plugin marketplace add`.
    marketplace_github: String,
    notice: Option<String>,
    /// The running app's version (from tauri.conf.json), shown in the UI.
    app_version: String,
}

// ---- Helpers ------------------------------------------------------------

fn open_db(storage: &Storage) -> Result<Db, String> {
    storage.ensure_layout().map_err(|e| e.to_string())?;
    Db::open(storage.db_path()).map_err(|e| e.to_string())
}

fn set_notice(app: &AppHandle, msg: impl Into<String>) {
    let state = app.state::<AppState>();
    *state.notice.lock().unwrap() = Some(msg.into());
    let _ = app.emit("cd:refresh", ());
}

/// Clear any lingering notice (called after a successful action so a stale
/// rejection message doesn't persist forever).
fn clear_notice(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state.notice.lock().unwrap() = None;
}

/// Tray icon for the given capture state, and whether it is a macOS template
/// image. The menu bar shows the icon ONLY (no title text, to save space):
/// an outlined drop while idle, a filled green drop while capturing — so an
/// active capture is always obvious. On macOS the idle drop is a template image
/// (follows the light/dark menu bar); elsewhere templates are not supported, so
/// the idle drop is colored to stay visible on dark taskbars.
fn tray_icon_bytes(capturing: bool) -> (&'static [u8], bool) {
    if capturing {
        (include_bytes!("../icons/tray-active.png"), false)
    } else if cfg!(target_os = "macos") {
        (include_bytes!("../icons/tray.png"), true)
    } else {
        (include_bytes!("../icons/tray-idle-color.png"), false)
    }
}

fn update_tray(app: &AppHandle, capturing: bool, count: i64) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        // The state and live item count go to the tooltip, not the menu bar.
        let tooltip = if capturing {
            format!("Context Drop — capturing ({count} item(s))")
        } else {
            "Context Drop — capture off".to_string()
        };
        let _ = tray.set_tooltip(Some(&tooltip));
        let (icon_bytes, template) = tray_icon_bytes(capturing);
        if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_icon_as_template(template);
        }
    }
}

/// Repaint the tray from the CURRENT app state (not a caller-supplied value), so
/// concurrent transitions can never leave a stale "capturing" indicator: the
/// last repaint to run always reflects the true state.
fn refresh_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let capturing = state.capturing.load(Ordering::SeqCst);
    let count = if capturing {
        let draft = state.current_draft.lock().unwrap().clone();
        draft
            .and_then(|id| {
                open_db(&state.storage)
                    .ok()
                    .and_then(|db| packet_summary(&db, &id).ok().flatten())
            })
            .map(|s| s.item_count)
            .unwrap_or(0)
    } else {
        0
    };
    update_tray(app, capturing, count);
}

fn build_snapshot(app: &AppHandle) -> Result<AppSnapshot, String> {
    let state = app.state::<AppState>();
    let db = open_db(&state.storage)?;
    let report = status(&db).map_err(|e| e.to_string())?;

    let current_draft = match &report.current_draft {
        Some(summary) => {
            let items = recent_items(&db, &summary.id, RECENT_ITEMS)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|it| RecentItem {
                    id: it.id.clone(),
                    kind: it.kind.as_str().to_string(),
                    mime_type: it.mime_type.clone(),
                    byte_size: it.byte_size,
                    preview: item_preview(&state.storage, &summary.id, &it),
                    image_thumb: item_thumb(&state.storage, &summary.id, &it, &state.thumb_cache),
                })
                .collect();
            Some(CurrentDraft {
                packet_id: summary.id.clone(),
                item_count: summary.item_count,
                recent_items: items,
            })
        }
        None => None,
    };

    let last_dispatch = report.last_dispatch.as_ref().map(LastDispatchInfo::from);
    let recent_dispatches = context_drop_core::recent_dispatches(&db, RECENT_DISPATCHES)
        .map_err(|e| e.to_string())?
        .iter()
        .map(LastDispatchInfo::from)
        .collect();

    let settings = db.load_settings().map_err(|e| e.to_string())?;
    let integrations = integration::detect_config_dirs()
        .into_iter()
        .map(|dir| IntegrationStatus {
            installed: integration::is_plugin_installed(&dir),
            enabled: integration::is_plugin_enabled(&dir),
            marketplace_path: integration::local_marketplace_dir(&dir)
                .to_string_lossy()
                .into_owned(),
            config_dir: dir.to_string_lossy().into_owned(),
        })
        .collect();

    // Extract lock-guarded values into locals so no MutexGuard temporary
    // outlives the `state` borrow at the end of the function.
    let capturing = state.capturing.load(Ordering::SeqCst);
    let shortcut = state.shortcut.lock().unwrap().clone();
    let shortcut_registered = state.shortcut_registered.load(Ordering::SeqCst);
    let notice = state.notice.lock().unwrap().clone();

    Ok(AppSnapshot {
        capturing,
        shortcut,
        shortcut_registered,
        current_draft,
        last_dispatch,
        recent_dispatches,
        ready_count: report.ready_count,
        settings,
        integrations,
        marketplace_github: integration::MARKETPLACE_GITHUB.to_string(),
        notice,
        app_version: app.package_info().version.to_string(),
    })
}

/// Read up to `max` bytes of a file. Returns `Ok(None)` if the file is larger
/// than `max` (so we never load an oversize/growing file into memory).
fn read_bounded(path: &std::path::Path, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    // Read one extra byte to detect "exceeds max".
    f.take(max.saturating_add(1)).read_to_end(&mut buf)?;
    if buf.len() as u64 > max {
        Ok(None)
    } else {
        Ok(Some(buf))
    }
}

/// Read the first `max` bytes of a file (the actual prefix), regardless of the
/// file's total size. Unlike `read_bounded`, a large file yields its prefix
/// rather than `None` — right for previews (a 5 KB log should still preview).
fn read_prefix(path: &std::path::Path, max: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Absolute path of a stored item file.
fn item_path(storage: &Storage, packet_id: &str, item: &PacketItem) -> PathBuf {
    storage.packet_dir(packet_id).join(&item.relative_path)
}

/// A short text preview (first ~100 chars, whitespace collapsed) of a text-like
/// item, so the user can tell WHAT they captured. `None` for non-text items or
/// unreadable/empty files. Reads at most 4 KB.
fn item_preview(storage: &Storage, packet_id: &str, item: &PacketItem) -> Option<String> {
    if !matches!(
        item.kind,
        ItemKind::Text | ItemKind::Json | ItemKind::Html | ItemKind::Url
    ) {
        return None;
    }
    let bytes = read_prefix(&item_path(storage, packet_id, item), 4096)?;
    let text = String::from_utf8_lossy(&bytes);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let head: String = collapsed.chars().take(100).collect();
    if collapsed.chars().count() > 100 {
        Some(format!("{head}…"))
    } else {
        Some(head)
    }
}

/// True if a stored item's file extension denotes an image we can thumbnail.
fn ext_is_image(relative_path: &str) -> bool {
    let ext = std::path::Path::new(relative_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Keep in sync with the image codecs enabled in context-drop-clipboard's
    // Cargo.toml (png/jpeg/gif/webp/bmp/tiff) — an ext we can't decode would only
    // fall back to a badge anyway.
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif"
    )
}

/// A small image thumbnail (data URL) for image items and dropped image files,
/// cached by content hash so the image is decoded at most once. `None` for
/// non-images or on decode failure.
fn item_thumb(
    storage: &Storage,
    packet_id: &str,
    item: &PacketItem,
    cache: &Mutex<HashMap<String, Option<String>>>,
) -> Option<String> {
    let is_image =
        item.kind == ItemKind::Image || (item.kind == ItemKind::File && ext_is_image(&item.relative_path));
    if !is_image {
        return None;
    }
    if let Some(hit) = cache.lock().unwrap().get(&item.sha256) {
        return hit.clone();
    }
    // Not cached: decode + downscale once. Item files are already bounded by the
    // per-item size limit at capture time, so reading the whole file is safe.
    let thumb = std::fs::read(item_path(storage, packet_id, item))
        .ok()
        .and_then(|bytes| context_drop_clipboard::thumbnail_data_url(&bytes, 56));
    let mut c = cache.lock().unwrap();
    // Crude bound: thumbnails are tiny and cheap to regenerate, so a hard reset
    // beats tracking LRU state here.
    if c.len() > 256 {
        c.clear();
    }
    c.insert(item.sha256.clone(), thumb.clone());
    thumb
}

/// Map a clipboard capture into a core snapshot. Enforces BOTH the per-item and
/// the cumulative packet-size budgets *while reading*, so copying many files
/// cannot exhaust memory, and a file that grew after inspection is rejected.
fn map_capture(
    app: &AppHandle,
    cap: &context_drop_clipboard::RawCapture,
    limits: &Limits,
) -> Option<CapturedSnapshot> {
    let mut items = Vec::new();
    let mut total: i64 = 0;
    for it in &cap.items {
        match &it.payload {
            Payload::Inline(bytes) => {
                let size = bytes.len() as i64;
                if size > limits.max_item_bytes {
                    set_notice(app, format!("Skipped an item: {size} bytes exceeds the item limit"));
                    continue;
                }
                if total.saturating_add(size) > limits.max_packet_bytes {
                    set_notice(app, "Packet size budget reached; item skipped".to_string());
                    continue;
                }
                total += size;
                items.push(CapturedItem {
                    kind: ItemKind::parse(it.kind.as_str()),
                    mime_type: it.mime_type.clone(),
                    ext: it.ext.clone(),
                    bytes: bytes.clone(),
                });
            }
            Payload::FileRef { path, byte_size, .. } => {
                if *byte_size as i64 > limits.max_item_bytes {
                    set_notice(
                        app,
                        format!(
                            "Skipped large file {} ({} bytes exceeds the item limit)",
                            path.display(),
                            byte_size
                        ),
                    );
                    continue;
                }
                let remaining = (limits.max_packet_bytes - total).max(0) as u64;
                if (*byte_size) > remaining {
                    set_notice(app, "Packet size budget reached; file skipped".to_string());
                    continue;
                }
                // Bound the read by BOTH the item limit and the remaining packet
                // budget, so a file that grew after inspection cannot overshoot.
                let bound = (limits.max_item_bytes as u64).min(remaining);
                match read_bounded(path, bound) {
                    Ok(Some(bytes)) => {
                        total += bytes.len() as i64;
                        items.push(CapturedItem {
                            kind: ItemKind::File,
                            mime_type: it.mime_type.clone(),
                            ext: it.ext.clone(),
                            bytes,
                        });
                    }
                    Ok(None) => set_notice(
                        app,
                        format!("Skipped {}: it grew beyond the item limit", path.display()),
                    ),
                    Err(e) => set_notice(app, format!("Could not read {}: {e}", path.display())),
                }
            }
        }
    }
    if items.is_empty() {
        return None;
    }
    // Dedupe on actual CONTENT (bytes we just read), not on the clipboard-side
    // path+size+mtime hash — so re-copying an edited file with an unchanged size
    // (or coarse mtime resolution) is still recognized as new content.
    Some(CapturedSnapshot {
        snapshot_sha256: snapshot_content_hash(&items),
        items,
    })
}

struct CaptureCtx {
    app: AppHandle,
    storage: Storage,
    current_draft: Arc<Mutex<Option<String>>>,
    capturing: Arc<AtomicBool>,
    capture_generation: Arc<AtomicU64>,
    limits: Arc<Mutex<Limits>>,
    lifecycle: Arc<Mutex<()>>,
    generation: u64,
}

impl CaptureCtx {
    /// This worker is still the active capture session (call under the
    /// lifecycle lock for a decision that must not race a start/stop).
    fn is_current(&self) -> bool {
        self.capture_generation.load(Ordering::SeqCst) == self.generation
            && self.capturing.load(Ordering::SeqCst)
    }

    /// End capture — but only if THIS worker is still current. The state mutation
    /// happens under the lifecycle lock (atomic with start/stop); the tray/UI is
    /// refreshed AFTER releasing the lock, from the true current state.
    fn end_capture(&self) {
        {
            let _lifecycle = self.lifecycle.lock().unwrap();
            if self.capture_generation.load(Ordering::SeqCst) == self.generation {
                self.capturing.store(false, Ordering::SeqCst);
            }
        }
        refresh_tray(&self.app);
        let _ = self.app.emit("cd:refresh", ());
    }
}

fn capture_loop(ctx: CaptureCtx) {
    let provider = match context_drop_clipboard::system_provider() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("context-drop: clipboard unavailable: {e}");
            ctx.end_capture();
            return;
        }
    };
    let mut watcher = Watcher::new(provider);
    // Prime (not reset): record the CURRENT clipboard state without capturing it,
    // so only what the user copies AFTER pressing Start Capture is collected — a
    // stale pre-existing clipboard item must not be swept into the packet.
    watcher.prime();

    while ctx.is_current() {
        // Read the (possibly updated) size limits for this event.
        let limits = *ctx.limits.lock().unwrap();

        // Detect an external claim even without a clipboard change: if our
        // current DRAFT is no longer DRAFT (claimed) or gone, capture ends.
        let draft_id = match ctx.current_draft.lock().unwrap().clone() {
            Some(id) => id,
            None => {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
        };
        let still_draft = Db::open(ctx.storage.db_path())
            .ok()
            .and_then(|db| packet_summary(&db, &draft_id).ok().flatten())
            .map(|s| s.state == PacketState::Draft)
            .unwrap_or(false);
        if !still_draft {
            // Clear may have swapped in a fresh draft (keep going); otherwise it
            // was claimed or removed (stop).
            let current_now = ctx.current_draft.lock().unwrap().clone();
            if current_now.as_deref() == Some(draft_id.as_str()) || current_now.is_none() {
                ctx.end_capture();
                break;
            }
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        match watcher.poll() {
            Ok(Some(cap)) => {
                // Clear any stale notice from a PREVIOUS event before mapping, so
                // a rejection for THIS event survives a same-event success.
                clear_notice(&ctx.app);
                if let Some(snapshot) = map_capture(&ctx.app, &cap, &limits) {
                    // Append to whatever the current draft is NOW, retrying once
                    // against a replacement if Clear swapped it mid-read.
                    if !append_with_retry(&ctx, &snapshot, &limits) {
                        break; // packet was claimed -> capture ended
                    }
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("context-drop: clipboard poll error: {e}"),
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Append the snapshot to the current draft. Returns false if the packet was
/// claimed (capture should end). Retries once against a replacement draft if
/// Clear swapped the target out from under us (PacketNotFound).
fn append_with_retry(ctx: &CaptureCtx, snapshot: &CapturedSnapshot, limits: &Limits) -> bool {
    enum Outcome {
        Continue,
        Added,
        Ended,
        Notice(String),
    }

    // The generation check, target selection, and append run under the lifecycle
    // lock so a start/stop cannot slip in between (a retired worker can never
    // append into a newer session's draft). All tray/UI work happens AFTER the
    // lock is released.
    let outcome = {
        let _lifecycle = ctx.lifecycle.lock().unwrap();
        let mut result = Outcome::Continue;
        for _ in 0..2 {
            if !ctx.is_current() {
                result = Outcome::Ended;
                break;
            }
            let Some(target) = ctx.current_draft.lock().unwrap().clone() else {
                break;
            };
            let Ok(mut db) = Db::open(ctx.storage.db_path()) else {
                break;
            };
            match append_snapshot(&mut db, &ctx.storage, &target, snapshot, limits) {
                Ok(AppendOutcome::Added { .. }) => {
                    result = Outcome::Added;
                    break;
                }
                Ok(AppendOutcome::StateChanged { .. }) => {
                    // Packet claimed: end capture (still under the lock).
                    if ctx.capture_generation.load(Ordering::SeqCst) == ctx.generation {
                        ctx.capturing.store(false, Ordering::SeqCst);
                    }
                    result = Outcome::Ended;
                    break;
                }
                Ok(AppendOutcome::Duplicate) => break,
                Ok(AppendOutcome::RejectedItemTooLarge { size, limit, .. }) => {
                    result = Outcome::Notice(format!(
                        "Rejected item: {size} bytes exceeds the {limit}-byte item limit"
                    ));
                    break;
                }
                Ok(AppendOutcome::RejectedPacketFull { limit, .. }) => {
                    result = Outcome::Notice(format!(
                        "Packet is full (limit {limit} bytes); item rejected"
                    ));
                    break;
                }
                Err(context_drop_core::CoreError::PacketNotFound(_)) => continue, // Clear swapped it; retry
                Err(e) => {
                    eprintln!("context-drop: append error: {e}");
                    break;
                }
            }
        }
        result
    };

    match outcome {
        Outcome::Added => {
            refresh_tray(&ctx.app);
            let _ = ctx.app.emit("cd:refresh", ());
            true
        }
        Outcome::Ended => {
            refresh_tray(&ctx.app);
            let _ = ctx.app.emit("cd:refresh", ());
            false
        }
        Outcome::Notice(msg) => {
            set_notice(&ctx.app, msg);
            true
        }
        Outcome::Continue => true,
    }
}

fn do_start_capture(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    // Do the DB + state work under the lifecycle lock (serializes start/stop/
    // clear/save), then release it BEFORE any main-thread UI call so we never
    // hold the lock across a tray round-trip (which could deadlock the hotkey
    // thread against the main thread).
    {
        let _lifecycle = state.lifecycle.lock().unwrap();
        if state.capturing.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut db = open_db(&state.storage)?;
        let settings = db.load_settings().map_err(|e| e.to_string())?;
        if settings.auto_cleanup {
            let _ = cleanup(&mut db, &state.storage, settings.packet_ttl_hours, now_ms());
        }
        // Reuse a recoverable DRAFT if present, else create a fresh one.
        let draft_id = match current_draft_id(&db).map_err(|e| e.to_string())? {
            Some(id) => id,
            None => create_draft(&mut db, &state.storage).map_err(|e| e.to_string())?,
        };

        *state.current_draft.lock().unwrap() = Some(draft_id);
        *state.limits.lock().unwrap() = settings.limits();
        let generation = state.capture_generation.fetch_add(1, Ordering::SeqCst) + 1;
        state.capturing.store(true, Ordering::SeqCst);

        let ctx = CaptureCtx {
            app: app.clone(),
            storage: state.storage.clone(),
            current_draft: state.current_draft.clone(),
            capturing: state.capturing.clone(),
            capture_generation: state.capture_generation.clone(),
            limits: state.limits.clone(),
            lifecycle: state.lifecycle.clone(),
            generation,
        };
        thread::spawn(move || capture_loop(ctx));
    }

    clear_notice(app);
    refresh_tray(app);
    let _ = app.emit("cd:refresh", ());
    Ok(())
}

fn do_stop_capture(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let _lifecycle = state.lifecycle.lock().unwrap();
        if !state.capturing.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Bump the generation first so the running worker sees it is retired.
        state.capture_generation.fetch_add(1, Ordering::SeqCst);
        state.capturing.store(false, Ordering::SeqCst);
        // Stopping only PAUSES capture — it must NOT clear or hide the collected
        // items (that is what the Clear button is for). We keep the packet as a
        // DRAFT: it stays visible in the UI, a later Start Capture reuses the same
        // DRAFT, Clear empties it, and `/context-drop:pull` still claims it (a
        // DRAFT with items is the highest claim priority). We only discard an
        // EMPTY draft so stopping right after starting doesn't litter packets.
        let draft = state.current_draft.lock().unwrap().take();
        if let Some(draft_id) = draft {
            let mut db = open_db(&state.storage)?;
            let count = packet_summary(&db, &draft_id)
                .map_err(|e| e.to_string())?
                .map(|s| s.item_count)
                .unwrap_or(0);
            if count == 0 {
                // Nothing captured: discard the empty draft.
                let _ = core_clear_packet(&mut db, &state.storage, &draft_id);
            }
            // count > 0: keep the DRAFT intact (do NOT finalize or clear).
        }
    }
    refresh_tray(app);
    let _ = app.emit("cd:refresh", ());
    Ok(())
}

fn do_toggle_capture(app: &AppHandle) {
    let capturing = app.state::<AppState>().capturing.load(Ordering::SeqCst);
    let result = if capturing {
        do_stop_capture(app)
    } else {
        do_start_capture(app)
    };
    if let Err(e) = result {
        set_notice(app, format!("Capture toggle failed: {e}"));
    }
}

fn do_clear_packet(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let capturing = {
        let _lifecycle = state.lifecycle.lock().unwrap();
        let mut db = open_db(&state.storage)?;

        // Determine capture state first: it decides both which packet to clear
        // and whether to swap in a fresh draft.
        let capturing = state.capturing.load(Ordering::SeqCst);
        // The packet to clear. While capturing, it is the live in-memory draft
        // (fall back to the DB right after a restart, where the UI shows a
        // recovered draft but the in-memory pointer is still None). While idle,
        // IGNORE the in-memory pointer: a claim auto-ends capture without clearing
        // it, so it can name an already-claimed packet — clear the newest real
        // DRAFT (e.g. one built up purely from drops) instead.
        let old = if capturing {
            match state.current_draft.lock().unwrap().clone() {
                Some(id) => Some(id),
                None => current_draft_id(&db).map_err(|e| e.to_string())?,
            }
        } else {
            current_draft_id(&db).map_err(|e| e.to_string())?
        };

        // Swap `current_draft` to the new target BEFORE deleting the old packet,
        // so the capture loop never observes a deleted-but-still-current id.
        if capturing {
            let new_id = create_draft(&mut db, &state.storage).map_err(|e| e.to_string())?;
            *state.current_draft.lock().unwrap() = Some(new_id);
        } else {
            *state.current_draft.lock().unwrap() = None;
        }
        if let Some(old_id) = old {
            // A StateChanged (claimed between read and delete) is a benign
            // "nothing to clear"; any other error is real.
            match core_clear_packet(&mut db, &state.storage, &old_id) {
                Ok(()) | Err(context_drop_core::CoreError::StateChanged { .. }) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        capturing
    };
    let _ = capturing;
    refresh_tray(app);
    clear_notice(app);
    let _ = app.emit("cd:refresh", ());
    Ok(())
}

fn do_undo_last(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut db = open_db(&state.storage)?;
    // Selection + eligibility + release happen in one transaction, guarded by the
    // exact claim id, so we never release a claim that re-used the packet since.
    match undo_last_dispatch(&mut db, &state.storage, DEFAULT_UNDO_WINDOW_MS) {
        Ok(res) => {
            let msg = format!(
                "Undid dispatch to {} (routing only; code changes are not reverted).",
                res.project_name
            );
            set_notice(app, msg.clone());
            Ok(msg)
        }
        Err(context_drop_core::CoreError::NothingToUndo) => {
            Err("No recent dispatch is eligible to undo.".into())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Handle files dropped onto the window. Works whether or not Capture is ON:
/// a drop is an explicit "capture this", so it appends to the current DRAFT
/// (reusing the newest one or creating a fresh one) exactly like a clipboard
/// event would — reusing the same size-budget enforcement and atomic append.
fn do_drop(app: &AppHandle, paths: Vec<PathBuf>) -> Result<(), String> {
    // Only real files are captured (folders are not walked yet).
    let files: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_file()).collect();
    if files.is_empty() {
        set_notice(app, "Nothing captured: drop files (folders aren't captured yet).");
        return Ok(());
    }
    // Clear any stale notice so THIS drop's outcome is what the user sees.
    clear_notice(app);
    let items = files
        .into_iter()
        .map(context_drop_clipboard::file_item)
        .collect();
    let raw = context_drop_clipboard::RawCapture::from_items(items);
    append_once(app, &raw, "Drop target vanished; try again.")
}

/// Capture what is on the clipboard RIGHT NOW, once, without starting Capture —
/// for when the user already copied something before pressing Start Capture.
/// Like a drop it is an explicit "capture this", so it works whether or not
/// Capture is ON and appends to the current DRAFT (reusing or creating one).
fn do_capture_clipboard_now(app: &AppHandle) -> Result<(), String> {
    let mut provider = context_drop_clipboard::system_provider()
        .map_err(|e| format!("clipboard unavailable: {e}"))?;
    let raw = match provider.read() {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            set_notice(app, "Nothing captured: the clipboard is empty or unsupported.");
            let _ = app.emit("cd:refresh", ());
            return Ok(());
        }
        Err(e) => return Err(format!("could not read the clipboard: {e}")),
    };
    // Clear any stale notice so THIS capture's outcome is what the user sees.
    clear_notice(app);
    append_once(app, &raw, "Capture target vanished; try again.")
}

/// Append a one-shot capture (drop / clipboard-now) to the current DRAFT,
/// reusing the same size-budget enforcement and atomic append as the capture
/// loop. Outcomes other than success are surfaced as a notice.
fn append_once(
    app: &AppHandle,
    raw: &context_drop_clipboard::RawCapture,
    vanished_notice: &str,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    // Use the SAVED limits: `state.limits` may still be the default before the
    // first capture start / settings save, so read the persisted settings.
    let limits = {
        let db = open_db(&state.storage)?;
        db.load_settings().map_err(|e| e.to_string())?.limits()
    };
    let Some(snapshot) = map_capture(app, raw, &limits) else {
        // Every item was skipped; map_capture already set an explanatory notice.
        let _ = app.emit("cd:refresh", ());
        return Ok(());
    };

    // Append under the lifecycle lock so a start/stop/clear cannot interleave
    // with the target selection + append (same lock the capture worker uses).
    {
        let _lifecycle = state.lifecycle.lock().unwrap();
        let mut db = open_db(&state.storage)?;
        // Target selection. While capturing, the in-memory pointer is the live
        // session draft. While idle it must be IGNORED: a claim auto-ends capture
        // WITHOUT clearing `current_draft`, so an idle pointer can name an
        // already-claimed packet — trusting it would make every idle drop bounce
        // off that dead packet. When idle we resolve the newest real DRAFT from
        // the DB (or create one), and never write `current_draft` (the capture
        // loop is not running; a later Start Capture re-resolves from the DB).
        let resolve_from_db = |db: &mut Db| -> Result<String, String> {
            match current_draft_id(db).map_err(|e| e.to_string())? {
                Some(id) => Ok(id),
                None => create_draft(db, &state.storage).map_err(|e| e.to_string()),
            }
        };
        let target = if state.capturing.load(Ordering::SeqCst) {
            // Bind (and drop) the guard before any DB work.
            let live = state.current_draft.lock().unwrap().clone();
            match live {
                Some(id) => id,
                None => resolve_from_db(&mut db)?,
            }
        } else {
            resolve_from_db(&mut db)?
        };
        match append_snapshot(&mut db, &state.storage, &target, &snapshot, &limits) {
            Ok(AppendOutcome::Added { .. }) => {}
            Ok(AppendOutcome::Duplicate) => set_notice(app, "Already captured (duplicate)."),
            Ok(AppendOutcome::StateChanged { .. }) => set_notice(
                app,
                "That packet was just claimed; start capture to begin a new one.",
            ),
            Ok(AppendOutcome::RejectedItemTooLarge { size, limit, .. }) => set_notice(
                app,
                format!("Rejected item: {size} bytes exceeds the {limit}-byte item limit"),
            ),
            Ok(AppendOutcome::RejectedPacketFull { limit, .. }) => set_notice(
                app,
                format!("Packet is full (limit {limit} bytes); item rejected"),
            ),
            Err(context_drop_core::CoreError::PacketNotFound(_)) => set_notice(app, vanished_notice),
            Err(e) => return Err(e.to_string()),
        }
    }
    refresh_tray(app);
    let _ = app.emit("cd:refresh", ());
    Ok(())
}

fn resolve_plugin_src(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(env) = std::env::var("CONTEXT_DROP_PLUGIN_DIR") {
        let p = PathBuf::from(env);
        if p.join(".claude-plugin/plugin.json").is_file() {
            return Ok(p);
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("claude-code");
        if p.join(".claude-plugin/plugin.json").is_file() {
            return Ok(p);
        }
    }
    // Dev fallback: the repo's integrations directory.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../integrations/claude-code");
    if dev.join(".claude-plugin/plugin.json").is_file() {
        return Ok(dev);
    }
    Err("could not locate the Context Drop plugin source".into())
}

// ---- Commands -----------------------------------------------------------

#[tauri::command]
fn get_snapshot(app: AppHandle) -> Result<AppSnapshot, String> {
    build_snapshot(&app)
}

#[tauri::command]
fn start_capture(app: AppHandle) -> Result<(), String> {
    do_start_capture(&app)
}

#[tauri::command]
fn capture_clipboard_now(app: AppHandle) -> Result<(), String> {
    do_capture_clipboard_now(&app)
}

#[tauri::command]
fn stop_capture(app: AppHandle) -> Result<(), String> {
    do_stop_capture(&app)
}

#[tauri::command]
fn clear_packet(app: AppHandle) -> Result<(), String> {
    do_clear_packet(&app)
}

#[tauri::command]
fn undo_last(app: AppHandle) -> Result<String, String> {
    do_undo_last(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let sanitized = settings.sanitized();
    {
        // Persist AND publish the limits under the lifecycle lock so a concurrent
        // capture start (which reads settings and publishes limits under the same
        // lock) can never overwrite the freshly saved limits with stale ones.
        let _lifecycle = state.lifecycle.lock().unwrap();
        let db = open_db(&state.storage)?;
        db.save_settings(&sanitized).map_err(|e| e.to_string())?;
        *state.limits.lock().unwrap() = sanitized.limits();
    }
    // Re-register the global shortcut if it changed (outside the lifecycle lock;
    // this touches the main-thread shortcut registry).
    let old = state.shortcut.lock().unwrap().clone();
    if old != sanitized.global_shortcut {
        let gs = app.global_shortcut();
        if let Ok(prev) = old.parse::<Shortcut>() {
            let _ = gs.unregister(prev);
        }
        match sanitized.global_shortcut.parse::<Shortcut>() {
            Ok(sc) => {
                let registered = gs.register(sc).is_ok();
                state.shortcut_registered.store(registered, Ordering::SeqCst);
            }
            Err(_) => state.shortcut_registered.store(false, Ordering::SeqCst),
        }
        *state.shortcut.lock().unwrap() = sanitized.global_shortcut.clone();
    }
    let _ = app.emit("cd:refresh", ());
    Ok(())
}

#[tauri::command]
fn install_integration(app: AppHandle, config_dir: Option<String>) -> Result<String, String> {
    let src = resolve_plugin_src(&app)?;
    let targets: Vec<PathBuf> = match config_dir {
        Some(d) => vec![PathBuf::from(d)],
        None => integration::detect_config_dirs(),
    };
    if targets.is_empty() {
        return Err("No Claude config directory found.".into());
    }

    // Install the bundled CLI onto the machine and stage the plugin under the
    // data dir, so the skill can run `context-drop` and a relocated CLI can
    // install into other config roots. The CLI ships as a Tauri sidecar next to
    // the app executable (in dev, the workspace `context-drop` binary is there).
    let state = app.state::<AppState>();
    let data_dir = state.storage.root().to_path_buf();
    let sidecar = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join(integration::cli_file_name())));
    let cli_line = match sidecar {
        Some(sidecar) if sidecar.is_file() => {
            // Essential: the canonical CLI copy must succeed (PATH symlink is
            // best-effort inside install_cli).
            let report = integration::install_cli(&data_dir, &sidecar)?;
            let mut line = format!(" CLI at {}.", report.installed_path.display());
            if let Some(hint) = report.path_hint {
                line.push_str(&format!(" {hint}."));
            }
            line
        }
        // No bundled sidecar (e.g. a dev build without staging). Only proceed if
        // a usable CLI copy already exists; otherwise report the real problem
        // rather than "Installed" with no working CLI.
        _ => {
            let existing = integration::cli_install_path(&data_dir);
            if integration::is_usable_cli(&existing) {
                format!(" CLI already installed at {}.", existing.display())
            } else {
                return Err(
                    "No bundled CLI sidecar was found and no installed `context-drop` copy \
                     exists, so the Claude Code skills would have nothing to run. \
                     Reinstall the Context Drop app (its bundle ships the CLI)."
                        .into(),
                );
            }
        }
    };
    // Essential: stage the plugin so a relocated CLI can install elsewhere.
    integration::stage_plugin(&data_dir, &src)?;

    let mut lines = Vec::new();
    let mut marketplace_dir = None;
    for dir in targets {
        let report = integration::install_plugin(&dir, &src)?;
        lines.push(format!(
            "{} {}",
            if report.already_present { "Refreshed" } else { "Installed" },
            report.config_dir.display()
        ));
        marketplace_dir.get_or_insert(report.marketplace_dir);
    }
    // Give the full, correct enable sequence: the marketplace must be added
    // (with its path) BEFORE the plugin can be installed.
    let enable = match marketplace_dir {
        Some(mp) => format!(
            " Enable it in Claude Code:  /plugin marketplace add {}   then  /plugin install context-drop@context-drop",
            mp.display()
        ),
        None => " Enable it in Claude Code with /plugin marketplace add <path> then /plugin install context-drop@context-drop".to_string(),
    };
    Ok(format!("{}.{cli_line}{enable}", lines.join("; ")))
}

#[tauri::command]
fn install_short_alias(app: AppHandle, config_dir: Option<String>) -> Result<String, String> {
    let src = resolve_plugin_src(&app)?.join("alias").join("cd");
    let state = app.state::<AppState>();
    let dir = match config_dir {
        Some(d) => PathBuf::from(d),
        None => integration::detect_config_dirs()
            .into_iter()
            .next()
            .ok_or("No Claude config directory found.")?,
    };
    let report = integration::install_short_alias(&dir, &src, false)?;
    let msg = if report.skipped_existing {
        format!(
            "A /cd skill already exists at {}; not overwritten.",
            report.skill_path.display()
        )
    } else {
        // Record the installed flag in settings.
        let db = open_db(&state.storage)?;
        let mut s = db.load_settings().map_err(|e| e.to_string())?;
        s.short_alias_installed = true;
        db.save_settings(&s).map_err(|e| e.to_string())?;
        format!("Installed /cd at {}", report.skill_path.display())
    };
    let _ = app.emit("cd:refresh", ());
    Ok(msg)
}

#[tauri::command]
fn open_data_folder(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.storage.ensure_layout().map_err(|e| e.to_string())?;
    let path = state.storage.root().to_string_lossy().into_owned();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn privacy_info() -> String {
    "Context Drop is local-only: no telemetry, analytics, or cloud backend, and it transmits \
     nothing over the network. The clipboard is watched only while Capture is ON (or read once \
     when you press Capture Clipboard Now). The only \
     time packet content leaves your machine is when Claude Code itself sends it to its configured \
     model provider as the isolated subagent reads it."
        .to_string()
}

/// Full text of a captured item (for the click-to-view modal). Bounded to 1 MB
/// so the modal never loads a pathological file; the UI scrolls within that.
#[tauri::command]
fn item_full_text(app: AppHandle, packet_id: String, item_id: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let db = open_db(&state.storage)?;
    let item = context_drop_core::get_item(&db, &packet_id, &item_id)
        .map_err(|e| e.to_string())?
        .ok_or("Item not found")?;
    let bytes = read_prefix(&item_path(&state.storage, &packet_id, &item), 1024 * 1024)
        .ok_or("Could not read the item")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// A larger image (data URL, fit within ~1400px) for the click-to-view modal —
/// crisp enough to inspect, without shipping a multi-MB original over IPC.
#[tauri::command]
fn item_full_image(app: AppHandle, packet_id: String, item_id: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let db = open_db(&state.storage)?;
    let item = context_drop_core::get_item(&db, &packet_id, &item_id)
        .map_err(|e| e.to_string())?
        .ok_or("Item not found")?;
    let bytes =
        std::fs::read(item_path(&state.storage, &packet_id, &item)).map_err(|e| e.to_string())?;
    context_drop_clipboard::thumbnail_data_url(&bytes, 2000)
        .ok_or_else(|| "Could not render the image".to_string())
}

/// Remove ONE item from the current DRAFT packet (the trash action). Refused if
/// the packet has already been claimed/sent.
#[tauri::command]
fn delete_item(app: AppHandle, packet_id: String, item_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        // Serialize with the capture worker + other lifecycle actions.
        let _lifecycle = state.lifecycle.lock().unwrap();
        let mut db = open_db(&state.storage)?;
        context_drop_core::delete_item(&mut db, &state.storage, &packet_id, &item_id)
            .map_err(|e| e.to_string())?;
    }
    refresh_tray(&app);
    let _ = app.emit("cd:refresh", ());
    Ok(())
}

// ---- App entry ----------------------------------------------------------

/// Run the Context Drop desktop app.
pub fn run() {
    let storage = Storage::resolve().unwrap_or_else(|_| Storage::at(std::env::temp_dir().join("ContextDrop")));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        // Return from the hotkey callback immediately: doing the
                        // DB/tray work here can deadlock the hotkey thread against
                        // the main thread (notably on X11). Run it off-thread.
                        let app = app.clone();
                        thread::spawn(move || do_toggle_capture(&app));
                    }
                })
                .build(),
        )
        .manage(AppState::new(storage))
        // Closing the window must NOT quit this tray utility — hide it instead,
        // so the tray icon, global shortcut, and capture keep running. The same
        // handler also receives OS drag-and-drop: dropped files are captured
        // into the current packet (no need to copy them to the clipboard first).
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::DragDrop(drag) => {
                let app = window.app_handle();
                match drag {
                    // Highlight the drop target while a drag hovers the window.
                    tauri::DragDropEvent::Enter { .. } | tauri::DragDropEvent::Over { .. } => {
                        let _ = app.emit("cd:dragover", true);
                    }
                    tauri::DragDropEvent::Leave => {
                        let _ = app.emit("cd:dragover", false);
                    }
                    tauri::DragDropEvent::Drop { paths, .. } => {
                        let _ = app.emit("cd:dragover", false);
                        // Do the DB/file work off the UI thread.
                        let app = app.clone();
                        let paths = paths.clone();
                        thread::spawn(move || {
                            if let Err(e) = do_drop(&app, paths) {
                                set_notice(&app, format!("Drop failed: {e}"));
                            }
                        });
                    }
                    _ => {}
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            start_capture,
            capture_clipboard_now,
            stop_capture,
            clear_packet,
            undo_last,
            save_settings,
            install_integration,
            install_short_alias,
            open_data_folder,
            privacy_info,
            item_full_text,
            item_full_image,
            delete_item,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // macOS: run as a menu-bar accessory (no dock icon).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Create the private (0700) data layout BEFORE anything writes into
            // it (e.g. refresh_cli's bin dir), so the data root is never left
            // world-traversable.
            {
                let state = handle.state::<AppState>();
                if let Err(e) = state.storage.ensure_layout() {
                    eprintln!("context-drop: WARNING could not secure the data directory: {e}");
                }
            }

            // Refresh the managed CLI copy from the bundled sidecar so it always
            // matches this app version (design D5). Best-effort, copy-only (no
            // PATH/symlink changes at startup).
            {
                let state = handle.state::<AppState>();
                let data_dir = state.storage.root().to_path_buf();
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(sidecar) =
                        exe.parent().map(|p| p.join(integration::cli_file_name()))
                    {
                        if sidecar.is_file() {
                            let _ = integration::refresh_cli(&data_dir, &sidecar);
                        }
                    }
                }
            }

            // Also refresh the staged plugin copy under the data dir: the
            // managed CLI (in <data-dir>/bin, outside the app bundle) falls back
            // to it, so without this `context-drop install-claude` would keep
            // handing out the plugin from the version that was first installed.
            // Only when it differs from the bundle (i.e. once after an update), so
            // a concurrent `install-claude` reading the copy is almost never raced.
            if let Ok(src) = resolve_plugin_src(&handle) {
                let state = handle.state::<AppState>();
                let staged = state.storage.root().join("claude-code");
                let manifest = ".claude-plugin/plugin.json";
                let stale = std::fs::read(src.join(manifest)).ok()
                    != std::fs::read(staged.join(manifest)).ok();
                if stale {
                    let _ = integration::stage_plugin(state.storage.root(), &src);
                }
            }

            // Refresh an already-installed Context Drop `/cd` alias in every
            // Claude config dir, so fixes reach existing users (the settings
            // button is disabled once /cd is installed). Best-effort; never
            // installs /cd where absent, never touches a user's own /cd.
            if let Ok(src) = resolve_plugin_src(&handle) {
                for dir in integration::detect_config_dirs() {
                    let _ = integration::refresh_short_alias(&dir, &src.join("alias").join("cd"));
                }
            }

            // TTL cleanup on startup.
            {
                let state = handle.state::<AppState>();
                if let Ok(mut db) = open_db(&state.storage) {
                    if let Ok(settings) = db.load_settings() {
                        if settings.auto_cleanup {
                            let _ = cleanup(&mut db, &state.storage, settings.packet_ttl_hours, now_ms());
                        }
                        // Register the configured global shortcut.
                        *state.shortcut.lock().unwrap() = settings.global_shortcut.clone();
                        if let Ok(sc) = settings.global_shortcut.parse::<Shortcut>() {
                            let ok = handle.global_shortcut().register(sc).is_ok();
                            state.shortcut_registered.store(ok, Ordering::SeqCst);
                        }
                    }
                }
            }

            // Build the tray icon + menu.
            let menu = MenuBuilder::new(&handle)
                .text("toggle", "Start / Stop Capture")
                .text("clear", "Clear Packet")
                .text("undo", "Undo Last Dispatch")
                .separator()
                .text("open", "Open Data Folder")
                .text("show", "Show Window")
                .separator()
                .text("quit", "Quit Context Drop")
                .build()?;

            let (icon_bytes, template) = tray_icon_bytes(false);
            let icon = tauri::image::Image::from_bytes(icon_bytes).ok();

            let mut tray = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .icon_as_template(template)
                .tooltip("Context Drop — capture off")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "toggle" => do_toggle_capture(app),
                    "clear" => {
                        if let Err(e) = do_clear_packet(app) {
                            set_notice(app, format!("Clear failed: {e}"));
                        }
                    }
                    "undo" => {
                        if let Err(e) = do_undo_last(app) {
                            set_notice(app, e);
                        }
                    }
                    "open" => {
                        let _ = open_data_folder(app.clone());
                    }
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = icon {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Context Drop");
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
