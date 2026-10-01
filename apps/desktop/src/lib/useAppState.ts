import { useCallback, useEffect, useState } from "react";
import { api, type AppSnapshot, type Settings } from "./api";

export interface AppStateHook {
  snapshot: AppSnapshot | null;
  loading: boolean;
  error: string | null;
  busy: boolean;
  /** True while an OS drag hovers the window (for the drop-target highlight). */
  dragOver: boolean;
  refresh: () => Promise<void>;
  startCapture: () => Promise<void>;
  stopCapture: () => Promise<void>;
  clearPacket: () => Promise<void>;
  deleteItem: (packetId: string, itemId: string) => Promise<void>;
  undoLast: () => Promise<void>;
  saveSettings: (settings: Settings) => Promise<void>;
  /** Resolves to the success message, or `undefined` if the command failed. */
  installIntegration: (configDir?: string) => Promise<string | undefined>;
  installShortAlias: (configDir?: string) => Promise<string | undefined>;
  openDataFolder: () => Promise<void>;
}

/**
 * Loads the app snapshot and exposes actions. Every action refreshes the
 * snapshot afterward and surfaces a human-readable error string on failure —
 * the UI never throws.
 */
export function useAppState(pollMs = 1500): AppStateHook {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragOver, setDragOver] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const snap = await api.getSnapshot();
      setSnapshot(snap);
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const act = useCallback(
    async <T,>(fn: () => Promise<T>): Promise<T | undefined> => {
      setBusy(true);
      try {
        const result = await fn();
        setError(null);
        await refresh();
        return result;
      } catch (e) {
        setError(errorMessage(e));
        return undefined;
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  useEffect(() => {
    void refresh();
    if (pollMs <= 0) return;
    const id = setInterval(() => void refresh(), pollMs);
    return () => clearInterval(id);
  }, [refresh, pollMs]);

  // Under the Tauri runtime, subscribe to backend events for near-instant
  // updates (so the item list is seen to grow the moment something is captured)
  // and to the drag-over signal that drives the drop-target highlight. Guarded
  // so the non-Tauri test/browser environment simply relies on polling.
  useEffect(() => {
    const isTauri =
      typeof window !== "undefined" &&
      "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);
    if (!isTauri) return;
    let cancelled = false;
    const unlisten: Array<() => void> = [];
    // Register each listener, but if the effect was already cleaned up by the
    // time the async `listen()` resolves, unlisten immediately — otherwise the
    // subscription would leak (cleanup already ran against an empty array).
    const track = (u: () => void) => {
      if (cancelled) u();
      else unlisten.push(u);
    };
    void import("@tauri-apps/api/event").then(({ listen }) => {
      if (cancelled) return;
      void listen("cd:refresh", () => void refresh()).then(track);
      void listen<boolean>("cd:dragover", (e) => setDragOver(Boolean(e.payload))).then(track);
    });
    return () => {
      cancelled = true;
      for (const u of unlisten) u();
    };
  }, [refresh]);

  return {
    snapshot,
    loading,
    error,
    busy,
    dragOver,
    refresh,
    startCapture: () => act(api.startCapture).then(() => undefined),
    stopCapture: () => act(api.stopCapture).then(() => undefined),
    clearPacket: () => act(api.clearPacket).then(() => undefined),
    deleteItem: (packetId, itemId) =>
      act(() => api.deleteItem(packetId, itemId)).then(() => undefined),
    undoLast: () => act(api.undoLast).then(() => undefined),
    saveSettings: (settings) => act(() => api.saveSettings(settings)).then(() => undefined),
    // Returns the success message, or undefined when the command failed (in
    // which case `act` has already set the error banner).
    installIntegration: (configDir) => act(() => api.installIntegration(configDir)),
    installShortAlias: (configDir) => act(() => api.installShortAlias(configDir)),
    openDataFolder: () => act(api.openDataFolder).then(() => undefined),
  };
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return "Unknown error";
  }
}
