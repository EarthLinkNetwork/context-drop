import { invoke } from "@tauri-apps/api/core";

// ---- Types (mirror the Rust command payloads; camelCase) ----------------

export interface RecentItem {
  /** Item id — used to open the full-content viewer or delete this item. */
  id: string;
  kind: string;
  mimeType: string;
  byteSize: number;
  /** First ~100 chars of a text-like item, so you can tell what was copied. */
  preview?: string | null;
  /** A small PNG thumbnail (data: URL) for image items and dropped images. */
  imageThumb?: string | null;
}

export interface CurrentDraft {
  packetId: string;
  itemCount: number;
  recentItems: RecentItem[];
}

export interface LastDispatchInfo {
  packetId: string;
  projectName: string;
  itemCount: number;
  claimedAt: string;
  state: string;
  sessionId: string;
  cwd: string;
  configDir: string | null;
  /** The user's pull instruction, recorded by the plugin. */
  note: string | null;
  /** Terminal tab/pane label, e.g. "iTerm2 w0t2p0". */
  terminal: string | null;
}

export interface IntegrationStatus {
  configDir: string;
  /** The app staged the local marketplace files under this config dir. */
  installed: boolean;
  /** The plugin is registered in Claude Code here (user ran `/plugin install`). */
  enabled: boolean;
  /** Local marketplace dir (for the offline `/plugin marketplace add <path>`). */
  marketplacePath: string;
}

/** Which screen the window opens on (tray click or show-window shortcut). */
export type WindowPlacement = "cursor" | "display" | "last";

export interface Settings {
  /** Toggles Capture ON/OFF. Empty = off. */
  globalShortcut: string;
  /** Shows / hides the window. Empty = off. */
  showWindowShortcut: string;
  /** Toggles Always on Top. Empty = off. */
  onTopShortcut: string;
  windowPlacement: WindowPlacement;
  /** Key of the chosen display (DisplayInfo.key) for "display" placement. */
  windowDisplay: string;
  alwaysOnTop: boolean;
  showOnAllDesktops: boolean;
  packetTtlHours: number;
  maxItemBytes: number;
  maxPacketBytes: number;
  autoCleanup: boolean;
  shortAliasInstalled: boolean;
  /** Check GitHub for a newer release at launch and about once a day. */
  autoCheckUpdates: boolean;
}

/** In-app updates (mirrors updater::UpdateStatus in the backend). */
export interface UpdateStatus {
  /** A re-check while an update is known keeps "available" (see `checking`). */
  state: "idle" | "checking" | "upToDate" | "available" | "downloading" | "installing";
  /** A check is running. */
  checking: boolean;
  /** The newer version, while one is known. */
  version: string | null;
  notes: string | null;
  /** Download progress in bytes. */
  downloaded: number;
  total: number | null;
  /** The last check's or install's failure. */
  error: string | null;
  errorDuring: "check" | "install" | null;
  /** When the last successful check finished (ms since the epoch). */
  checkedAt: number | null;
  /** Why this copy of the app cannot be replaced (dev build, disk image…). */
  blockedReason: string | null;
  /** Updates exist for this platform (macOS only). */
  supported: boolean;
}

export interface AppSnapshot {
  capturing: boolean;
  /** Each global shortcut's status (capture, window, onTop). */
  shortcuts: ShortcutStatus[];
  currentDraft: CurrentDraft | null;
  lastDispatch: LastDispatchInfo | null;
  /** Recent dispatches, newest first (up to 10). */
  recentDispatches: LastDispatchInfo[];
  readyCount: number;
  settings: Settings;
  integrations: IntegrationStatus[];
  /** The public GitHub marketplace slug for `/plugin marketplace add`. */
  marketplaceGithub: string;
  /** A transient notice from the backend (e.g. an item rejected for size). */
  notice: string | null;
  /** The running app's version (e.g. "0.1.6"). */
  appVersion: string;
  update: UpdateStatus;
}

export type ShortcutActionId = "capture" | "window" | "onTop";

export interface ShortcutStatus {
  action: ShortcutActionId;
  accelerator: string;
  /** off | active | failed (the OS refused it) | invalid | duplicate */
  state: "off" | "active" | "failed" | "invalid" | "duplicate";
  /** For "duplicate": the action that already uses the key. */
  duplicateOf: string | null;
}

/** A connected display, for the "Open window on" picker. */
export interface DisplayInfo {
  key: string;
  name: string;
  width: number;
  height: number;
  primary: boolean;
}

// ---- Command surface ----------------------------------------------------

/**
 * Thin wrapper over the Tauri command layer. Kept as a single object so tests
 * can mock it wholesale and so component code never touches `invoke` directly.
 */
export const api = {
  getSnapshot: () => invoke<AppSnapshot>("get_snapshot"),
  startCapture: () => invoke<void>("start_capture"),
  captureClipboardNow: () => invoke<void>("capture_clipboard_now"),
  stopCapture: () => invoke<void>("stop_capture"),
  clearPacket: () => invoke<void>("clear_packet"),
  undoLast: () => invoke<string>("undo_last"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listDisplays: () => invoke<DisplayInfo[]>("list_displays"),
  /** Save only the given fields (auto-save); returns the settings as saved. */
  updateSettings: (patch: Partial<Settings>) => invoke<Settings>("update_settings", { patch }),
  /** While recording, a press of a registered shortcut is sent as an event. */
  beginRecording: () => invoke<void>("begin_recording"),
  endRecording: () => invoke<void>("end_recording"),
  installIntegration: (configDir?: string) =>
    invoke<string>("install_integration", { configDir: configDir ?? null }),
  installShortAlias: (configDir?: string) =>
    invoke<string>("install_short_alias", { configDir: configDir ?? null }),
  openDataFolder: () => invoke<void>("open_data_folder"),
  privacyInfo: () => invoke<string>("privacy_info"),
  itemFullText: (packetId: string, itemId: string) =>
    invoke<string>("item_full_text", { packetId, itemId }),
  itemFullImage: (packetId: string, itemId: string) =>
    invoke<string>("item_full_image", { packetId, itemId }),
  deleteItem: (packetId: string, itemId: string) =>
    invoke<void>("delete_item", { packetId, itemId }),
  /** Check GitHub for a newer release now; resolves to the status after. */
  checkForUpdates: () => invoke<UpdateStatus>("check_for_updates"),
  /** Download, verify and install the update found, then restart the app. */
  installUpdate: () => invoke<void>("install_update"),
};

export type Api = typeof api;
