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
}

export interface IntegrationStatus {
  configDir: string;
  installed: boolean;
}

export interface Settings {
  globalShortcut: string;
  packetTtlHours: number;
  maxItemBytes: number;
  maxPacketBytes: number;
  autoCleanup: boolean;
  shortAliasInstalled: boolean;
}

export interface AppSnapshot {
  capturing: boolean;
  shortcut: string;
  /** False when the global shortcut could not be registered (conflict). */
  shortcutRegistered: boolean;
  currentDraft: CurrentDraft | null;
  lastDispatch: LastDispatchInfo | null;
  readyCount: number;
  settings: Settings;
  integrations: IntegrationStatus[];
  /** A transient notice from the backend (e.g. an item rejected for size). */
  notice: string | null;
}

// ---- Command surface ----------------------------------------------------

/**
 * Thin wrapper over the Tauri command layer. Kept as a single object so tests
 * can mock it wholesale and so component code never touches `invoke` directly.
 */
export const api = {
  getSnapshot: () => invoke<AppSnapshot>("get_snapshot"),
  startCapture: () => invoke<void>("start_capture"),
  stopCapture: () => invoke<void>("stop_capture"),
  clearPacket: () => invoke<void>("clear_packet"),
  undoLast: () => invoke<string>("undo_last"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
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
};

export type Api = typeof api;
