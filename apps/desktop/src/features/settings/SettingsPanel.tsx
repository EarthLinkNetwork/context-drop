import { useEffect, useState } from "react";
import type { DisplayInfo, Settings, WindowPlacement } from "../../lib/api";

interface Props {
  settings: Settings;
  shortcutRegistered: boolean;
  showShortcutRegistered: boolean;
  busy: boolean;
  onSave: (settings: Settings) => void;
  onOpenDataFolder: () => void;
  /** Connected displays, for the "A specific display" picker. */
  onListDisplays: () => Promise<DisplayInfo[]>;
}

const MB = 1024 * 1024;

/** Settings editor: shortcuts, window, TTL, size limits (Claude setup lives in InstallationPanel). */
export function SettingsPanel({
  settings,
  shortcutRegistered,
  showShortcutRegistered,
  busy,
  onSave,
  onOpenDataFolder,
  onListDisplays,
}: Props) {
  // Each field follows its OWN saved value: when that value really changes
  // (backend clamp, tray "Always on Top"), only that field is reset. The ~1.5s
  // snapshot poll (new object, equal values) and changes to other fields leave
  // the user's unsaved edits alone.
  const [shortcut, setShortcut] = useSavedField(settings.globalShortcut);
  const [showShortcut, setShowShortcut] = useSavedField(settings.showWindowShortcut);
  const [placement, setPlacement] = useSavedField<WindowPlacement>(settings.windowPlacement);
  const [display, setDisplay] = useSavedField(settings.windowDisplay);
  const [alwaysOnTop, setAlwaysOnTop] = useSavedField(settings.alwaysOnTop);
  const [allDesktops, setAllDesktops] = useSavedField(settings.showOnAllDesktops);
  const [ttl, setTtl] = useSavedField(String(settings.packetTtlHours));
  const [itemMb, setItemMb] = useSavedField(String(Math.round(settings.maxItemBytes / MB)));
  const [packetMb, setPacketMb] = useSavedField(String(Math.round(settings.maxPacketBytes / MB)));
  const [autoCleanup, setAutoCleanup] = useSavedField(settings.autoCleanup);
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);

  // Load the connected displays when the picker is shown (and again each time
  // it is re-selected, so a monitor plugged in meanwhile appears).
  useEffect(() => {
    if (placement !== "display") return;
    let live = true;
    onListDisplays()
      .then((list) => live && setDisplays(list))
      .catch(() => live && setDisplays([]));
    return () => {
      live = false;
    };
  }, [placement, onListDisplays]);

  // The saved display, matched like the backend does: exact key, else the
  // same name (the arrangement changed), else shown as not connected.
  const savedName = display.includes("@") ? display.slice(0, display.lastIndexOf("@")) : display;
  const matched =
    displays.find((d) => d.key === display) ?? displays.find((d) => d.name === savedName);
  const selectedDisplay = matched?.key ?? display;

  const save = () => {
    onSave({
      ...settings,
      globalShortcut: shortcut.trim() || settings.globalShortcut,
      showWindowShortcut: showShortcut.trim(),
      windowPlacement: placement,
      windowDisplay: placement === "display" ? selectedDisplay : display,
      alwaysOnTop,
      showOnAllDesktops: allDesktops,
      packetTtlHours: clampInt(ttl, settings.packetTtlHours, 1),
      maxItemBytes: clampInt(itemMb, settings.maxItemBytes / MB, 1) * MB,
      maxPacketBytes: clampInt(packetMb, settings.maxPacketBytes / MB, 1) * MB,
      autoCleanup,
    });
  };

  return (
    <section className="panel settings-panel">
      <h2>Settings</h2>

      <label className="field">
        <span>Capture shortcut (Start / Stop Capture)</span>
        <input
          type="text"
          aria-label="Capture shortcut"
          value={shortcut}
          disabled={busy}
          onChange={(e) => setShortcut(e.target.value)}
        />
      </label>
      {!shortcutRegistered && (
        <p className="warning" role="alert" data-testid="shortcut-conflict">
          The capture shortcut could not be registered — it may conflict with another app.
          Choose a different shortcut and save.
        </p>
      )}

      <label className="field">
        <span>Show / hide window shortcut (empty = off)</span>
        <input
          type="text"
          aria-label="Show window shortcut"
          value={showShortcut}
          placeholder="e.g. CommandOrControl+Shift+0"
          disabled={busy}
          onChange={(e) => setShowShortcut(e.target.value)}
        />
      </label>
      {!showShortcutRegistered && settings.showWindowShortcut !== "" && (
        <p className="warning" role="alert" data-testid="show-shortcut-conflict">
          The show-window shortcut could not be registered — it may conflict with another app or
          be the same as the capture shortcut. Choose a different shortcut and save.
        </p>
      )}

      <label className="field">
        <span>Open window on</span>
        <select
          aria-label="Open window on"
          value={placement}
          disabled={busy}
          onChange={(e) => setPlacement(e.target.value as WindowPlacement)}
        >
          <option value="cursor">The screen with the mouse pointer</option>
          <option value="display">A specific display</option>
          <option value="last">Where it was last</option>
        </select>
      </label>

      {placement === "display" && (
        <label className="field">
          <span>Display</span>
          <select
            aria-label="Display"
            value={selectedDisplay}
            disabled={busy}
            onChange={(e) => setDisplay(e.target.value)}
          >
            {selectedDisplay === "" && <option value="">Choose a display…</option>}
            {displays.map((d) => (
              <option key={d.key} value={d.key}>
                {d.name} ({d.width}×{d.height}){d.primary ? " — main" : ""}
              </option>
            ))}
            {selectedDisplay !== "" && !matched && (
              <option value={selectedDisplay}>{savedName} (not connected)</option>
            )}
          </select>
        </label>
      )}
      {placement === "display" && selectedDisplay !== "" && !matched && (
        <p className="hint" data-testid="display-missing">
          That display is not connected; the window opens on the screen with the mouse pointer.
        </p>
      )}

      <label className="field field-inline">
        <input
          type="checkbox"
          aria-label="Always on top"
          checked={alwaysOnTop}
          disabled={busy}
          onChange={(e) => setAlwaysOnTop(e.target.checked)}
        />
        <span>Always on top (keep the window in front of other apps)</span>
      </label>

      <label className="field field-inline">
        <input
          type="checkbox"
          aria-label="Show on all desktops"
          checked={allDesktops}
          disabled={busy}
          onChange={(e) => setAllDesktops(e.target.checked)}
        />
        <span>Show on all desktops (Spaces)</span>
      </label>

      <label className="field">
        <span>Packet TTL (hours)</span>
        <input
          type="number"
          aria-label="Packet TTL (hours)"
          min={1}
          value={ttl}
          disabled={busy}
          onChange={(e) => setTtl(e.target.value)}
        />
      </label>

      <label className="field">
        <span>Max item size (MB)</span>
        <input
          type="number"
          aria-label="Max item size (MB)"
          min={1}
          value={itemMb}
          disabled={busy}
          onChange={(e) => setItemMb(e.target.value)}
        />
      </label>

      <label className="field">
        <span>Max packet size (MB)</span>
        <input
          type="number"
          aria-label="Max packet size (MB)"
          min={1}
          value={packetMb}
          disabled={busy}
          onChange={(e) => setPacketMb(e.target.value)}
        />
      </label>

      <label className="field field-inline">
        <input
          type="checkbox"
          aria-label="Auto cleanup"
          checked={autoCleanup}
          disabled={busy}
          onChange={(e) => setAutoCleanup(e.target.checked)}
        />
        <span>Automatically clean up expired packets</span>
      </label>

      <button type="button" className="btn btn-save" disabled={busy} onClick={save}>
        Save Settings
      </button>

      <div className="settings-actions">
        <button type="button" className="btn btn-small" disabled={busy} onClick={onOpenDataFolder}>
          Open data folder
        </button>
      </div>

      <details className="privacy-info">
        <summary>Privacy information</summary>
        <p>
          Context Drop is local-only. It has no telemetry, analytics, or cloud backend, and it
          transmits nothing over the network. Clipboard items are captured only while Capture is
          ON. The only time packet content leaves your machine is when Claude Code itself sends it
          to its configured model provider as the isolated subagent reads it.
        </p>
      </details>
    </section>
  );
}

/** Local form state that is reset only when its saved (primitive) value changes. */
function useSavedField<T>(saved: T): [T, (value: T) => void] {
  const [value, setValue] = useState(saved);
  useEffect(() => setValue(saved), [saved]);
  return [value, setValue];
}

function clampInt(value: string, fallback: number, min: number): number {
  const n = Number.parseInt(value, 10);
  if (Number.isNaN(n) || n < min) return Math.max(min, Math.round(fallback));
  return n;
}
