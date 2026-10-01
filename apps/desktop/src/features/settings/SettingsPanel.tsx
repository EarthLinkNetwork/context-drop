import { useEffect, useState } from "react";
import type { Settings } from "../../lib/api";

interface Props {
  settings: Settings;
  shortcutRegistered: boolean;
  busy: boolean;
  onSave: (settings: Settings) => void;
  onOpenDataFolder: () => void;
}

const MB = 1024 * 1024;

/** Settings editor: shortcut, TTL, size limits (Claude setup lives in InstallationPanel). */
export function SettingsPanel({
  settings,
  shortcutRegistered,
  busy,
  onSave,
  onOpenDataFolder,
}: Props) {
  const [shortcut, setShortcut] = useState(settings.globalShortcut);
  const [ttl, setTtl] = useState(String(settings.packetTtlHours));
  const [itemMb, setItemMb] = useState(String(Math.round(settings.maxItemBytes / MB)));
  const [packetMb, setPacketMb] = useState(String(Math.round(settings.maxPacketBytes / MB)));
  const [autoCleanup, setAutoCleanup] = useState(settings.autoCleanup);

  // Resync the form when the SAVED settings actually change (e.g. the backend
  // clamped a value on save). Depending on the primitive values — not the object
  // identity — means the ~1.5s snapshot poll (a new object each time with equal
  // values) does NOT clobber the user's unsaved edits; only a real change does.
  useEffect(() => {
    setShortcut(settings.globalShortcut);
    setTtl(String(settings.packetTtlHours));
    setItemMb(String(Math.round(settings.maxItemBytes / MB)));
    setPacketMb(String(Math.round(settings.maxPacketBytes / MB)));
    setAutoCleanup(settings.autoCleanup);
  }, [
    settings.globalShortcut,
    settings.packetTtlHours,
    settings.maxItemBytes,
    settings.maxPacketBytes,
    settings.autoCleanup,
  ]);

  const save = () => {
    onSave({
      ...settings,
      globalShortcut: shortcut.trim() || settings.globalShortcut,
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
        <span>Global shortcut</span>
        <input
          type="text"
          aria-label="Global shortcut"
          value={shortcut}
          disabled={busy}
          onChange={(e) => setShortcut(e.target.value)}
        />
      </label>
      {!shortcutRegistered && (
        <p className="warning" role="alert" data-testid="shortcut-conflict">
          The global shortcut could not be registered — it may conflict with another app.
          Choose a different shortcut and save.
        </p>
      )}

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

function clampInt(value: string, fallback: number, min: number): number {
  const n = Number.parseInt(value, 10);
  if (Number.isNaN(n) || n < min) return Math.max(min, Math.round(fallback));
  return n;
}
