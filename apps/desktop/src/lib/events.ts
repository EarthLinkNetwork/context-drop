/**
 * Subscribe to a backend event (Tauri `emit`). Outside the Tauri runtime
 * (tests, plain browser) this is a no-op. Returns an unsubscribe function that
 * is safe to call before the async subscription has finished.
 */
export function onBackendEvent<T>(name: string, handler: (payload: T) => void): () => void {
  const isTauri =
    typeof window !== "undefined" &&
    "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);
  if (!isTauri) return () => {};
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  void import("@tauri-apps/api/event").then(({ listen }) =>
    listen<T>(name, (e) => handler(e.payload)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    }),
  );
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
