import { useCallback, useEffect, useState, type ReactNode } from "react";
import type {
  DisplayInfo,
  Settings,
  ShortcutActionId,
  ShortcutStatus,
  WindowPlacement,
} from "../../lib/api";
import type { Platform } from "../../lib/accelerator";
import { ShortcutRecorder } from "./ShortcutRecorder";
import { SavedMark, useAutoSave } from "./useAutoSave";

export type SettingsTab = "shortcuts" | "window" | "claude" | "advanced";

/** Defaults, mirrored from context_drop_core::settings (shown by "Reset"). */
export const DEFAULT_SHORTCUTS: Record<ShortcutActionId, string> = {
  capture: "CommandOrControl+Shift+9",
  window: "CommandOrControl+Shift+0",
  onTop: "CommandOrControl+Shift+8",
};

const SHORTCUT_ROWS: { action: ShortcutActionId; field: keyof Settings; label: string }[] = [
  { action: "capture", field: "globalShortcut", label: "Start / Stop Capture" },
  { action: "window", field: "showWindowShortcut", label: "Show / hide window" },
  { action: "onTop", field: "onTopShortcut", label: "Always on Top" },
];

interface Props {
  /** The Settings view is on screen (its top-level tab is selected). */
  visible: boolean;
  tab: SettingsTab;
  onTabChange: (tab: SettingsTab) => void;
  settings: Settings;
  shortcuts: ShortcutStatus[];
  platform: Platform;
  /** Auto-save one or more fields; resolves null when saved, else the error. */
  onPatch: (patch: Partial<Settings>) => Promise<string | null>;
  onListDisplays: () => Promise<DisplayInfo[]>;
  onBeginRecording: () => void;
  onEndRecording: () => void;
  subscribeRecording: (
    onShortcutPressed: (accelerator: string) => void,
    onRecordingEnded: () => void,
  ) => () => void;
  onOpenDataFolder: () => void;
  /** The Claude Code setup panel (rendered in its own tab). */
  claudeSetup: ReactNode;
  /** Settings → Advanced → Updates: version, Check for Updates, the result. */
  updates: ReactNode;
  /** In-app updates exist on this platform (else no auto-check setting). */
  updatesSupported: boolean;
}

const TABS: { id: SettingsTab; label: string }[] = [
  { id: "shortcuts", label: "Shortcuts" },
  { id: "window", label: "Window" },
  { id: "claude", label: "Claude Code" },
  { id: "advanced", label: "Advanced" },
];

/** Settings, split into tabs. Every change is saved at once (no Save button). */
export function SettingsView(props: Props) {
  const { tab, onTabChange, onPatch } = props;
  const { save, savedKey, error, pending } = useAutoSave(onPatch);
  // What the user last chose: saved settings plus changes still being saved.
  const settings = { ...props.settings, ...pending };
  return (
    <section className="panel settings-panel">
      <nav className="subtabs" role="tablist" aria-label="Settings sections">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={tab === t.id ? "subtab is-active" : "subtab"}
            onClick={() => onTabChange(t.id)}
          >
            {t.label}
          </button>
        ))}
      </nav>
      <p className="hint autosave-note">Changes are saved automatically.</p>
      {error && (
        <p className="warning" role="alert" data-testid="autosave-error">
          Could not save: {error}
        </p>
      )}

      <div className="subtab-panel" hidden={tab !== "shortcuts"}>
        <ShortcutsTab {...props} settings={settings} save={save} savedKey={savedKey} />
      </div>
      <div className="subtab-panel" hidden={tab !== "window"}>
        <WindowTab
          settings={settings}
          onListDisplays={props.onListDisplays}
          save={save}
          savedKey={savedKey}
          active={tab === "window"}
        />
      </div>
      <div className="subtab-panel" hidden={tab !== "claude"}>
        {props.claudeSetup}
      </div>
      <div className="subtab-panel" hidden={tab !== "advanced"}>
        <AdvancedTab
          settings={settings}
          save={save}
          savedKey={savedKey}
          onOpenDataFolder={props.onOpenDataFolder}
          updates={props.updates}
          updatesSupported={props.updatesSupported}
        />
      </div>
    </section>
  );
}

type Save = (patch: Partial<Settings>, key: string) => Promise<boolean>;

function ShortcutsTab({
  visible,
  tab,
  settings,
  shortcuts,
  platform,
  onBeginRecording,
  onEndRecording,
  subscribeRecording,
  save,
  savedKey,
}: Props & { save: Save; savedKey: string | null }) {
  // Only one row records at a time (one key press must not land in two rows).
  const [recordingAction, setRecordingAction] = useState<ShortcutActionId | null>(null);
  return (
    <div className="shortcut-list">
      <p className="hint">
        These work from any app. Choose “Change”, then press the keys you want.
      </p>
      {SHORTCUT_ROWS.map((row) => {
        const others = SHORTCUT_ROWS.filter((o) => o.action !== row.action).map((o) => ({
          label: o.label,
          accelerator: String(settings[o.field] ?? ""),
        }));
        return (
          <ShortcutRecorder
            key={row.action}
            label={row.label}
            value={String(settings[row.field] ?? "")}
            defaultValue={DEFAULT_SHORTCUTS[row.action]}
            status={shortcuts.find((s) => s.action === row.action)}
            others={others}
            platform={platform}
            saved={savedKey === row.field}
            onSave={(acc) => save({ [row.field]: acc }, row.field)}
            onBeginRecording={onBeginRecording}
            onEndRecording={onEndRecording}
            locked={recordingAction !== null && recordingAction !== row.action}
            visible={visible && tab === "shortcuts"}
            onRecordingChange={(on) =>
              setRecordingAction((cur) => (on ? row.action : cur === row.action ? null : cur))
            }
            subscribe={subscribeRecording}
          />
        );
      })}
    </div>
  );
}

function WindowTab({
  settings,
  onListDisplays,
  save,
  savedKey,
  active,
}: {
  settings: Settings;
  onListDisplays: () => Promise<DisplayInfo[]>;
  save: Save;
  savedKey: string | null;
  active: boolean;
}) {
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const placement = settings.windowPlacement;
  const display = settings.windowDisplay;

  // Load the connected displays when the picker is visible (re-loaded each
  // time, so a monitor plugged in meanwhile appears).
  useEffect(() => {
    if (!active || placement !== "display") return;
    let live = true;
    onListDisplays()
      .then((list) => live && setDisplays(list))
      .catch(() => live && setDisplays([]));
    return () => {
      live = false;
    };
  }, [active, placement, onListDisplays]);

  // The saved display, matched like the backend does: exact key, else the
  // same name (the arrangement changed), else shown as not connected.
  const savedName = display.includes("@") ? display.slice(0, display.lastIndexOf("@")) : display;
  const matched =
    displays.find((d) => d.key === display) ?? displays.find((d) => d.name === savedName);
  const selectedDisplay = matched?.key ?? display;

  return (
    <div>
      <label className="field">
        <span>
          Open window on <SavedMark show={savedKey === "windowPlacement"} />
        </span>
        <select
          aria-label="Open window on"
          value={placement}
          onChange={(e) =>
            void save({ windowPlacement: e.target.value as WindowPlacement }, "windowPlacement")
          }
        >
          <option value="cursor">The screen with the mouse pointer</option>
          <option value="display">A specific display</option>
          <option value="last">Where it was last</option>
        </select>
      </label>

      {placement === "display" && (
        <label className="field">
          <span>
            Display <SavedMark show={savedKey === "windowDisplay"} />
          </span>
          <select
            aria-label="Display"
            value={selectedDisplay}
            onChange={(e) => void save({ windowDisplay: e.target.value }, "windowDisplay")}
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
      {placement === "display" && selectedDisplay !== "" && !matched && displays.length > 0 && (
        <p className="hint" data-testid="display-missing">
          That display is not connected; the window opens on the screen with the mouse pointer.
        </p>
      )}

      <Toggle
        label="Always on top"
        text="Keep the window in front of other apps"
        checked={settings.alwaysOnTop}
        saved={savedKey === "alwaysOnTop"}
        onChange={(v) => save({ alwaysOnTop: v }, "alwaysOnTop")}
      />
      <Toggle
        label="Show on all desktops"
        text="Show the window on every desktop (Space)"
        checked={settings.showOnAllDesktops}
        saved={savedKey === "showOnAllDesktops"}
        onChange={(v) => save({ showOnAllDesktops: v }, "showOnAllDesktops")}
      />
    </div>
  );
}

function Toggle({
  label,
  text,
  checked,
  saved,
  onChange,
}: {
  label: string;
  text: string;
  checked: boolean;
  saved: boolean;
  onChange: (value: boolean) => Promise<boolean>;
}) {
  // `checked` already includes a change still being saved, so a quick second
  // click toggles back instead of re-sending the same value.
  return (
    <label className="field field-inline">
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        onChange={(e) => void onChange(e.target.checked)}
      />
      <span>
        {text} <SavedMark show={saved} />
      </span>
    </label>
  );
}

const MB = 1024 * 1024;

function AdvancedTab({
  settings,
  save,
  savedKey,
  onOpenDataFolder,
  updates,
  updatesSupported,
}: {
  settings: Settings;
  save: Save;
  savedKey: string | null;
  onOpenDataFolder: () => void;
  updates: ReactNode;
  updatesSupported: boolean;
}) {
  return (
    <div>
      <h3 className="settings-section-title">Updates</h3>
      {updatesSupported && (
        <Toggle
          label="Check for updates automatically"
          text="Check for updates automatically (at launch and about once a day)"
          checked={settings.autoCheckUpdates}
          saved={savedKey === "autoCheckUpdates"}
          onChange={(v) => save({ autoCheckUpdates: v }, "autoCheckUpdates")}
        />
      )}
      {updates}

      <h3 className="settings-section-title">Packets</h3>
      <NumberSetting
        label="Packet TTL (hours)"
        value={settings.packetTtlHours}
        saved={savedKey === "packetTtlHours"}
        onCommit={(n) => save({ packetTtlHours: n }, "packetTtlHours")}
      />
      <NumberSetting
        label="Max item size (MB)"
        value={Math.round(settings.maxItemBytes / MB)}
        saved={savedKey === "maxItemBytes"}
        onCommit={(n) => save({ maxItemBytes: n * MB }, "maxItemBytes")}
      />
      <NumberSetting
        label="Max packet size (MB)"
        value={Math.round(settings.maxPacketBytes / MB)}
        saved={savedKey === "maxPacketBytes"}
        onCommit={(n) => save({ maxPacketBytes: n * MB }, "maxPacketBytes")}
      />
      <Toggle
        label="Auto cleanup"
        text="Automatically clean up expired packets"
        checked={settings.autoCleanup}
        saved={savedKey === "autoCleanup"}
        onChange={(v) => save({ autoCleanup: v }, "autoCleanup")}
      />

      <div className="settings-actions">
        <button type="button" className="btn btn-small" onClick={onOpenDataFolder}>
          Open data folder
        </button>
      </div>

      <details className="privacy-info">
        <summary>Privacy information</summary>
        <p>
          Context Drop keeps your packets on this machine. It has no cloud backend, and no
          telemetry or analytics beyond the anonymous update check: its only network use is
          reading the latest release&apos;s manifest from GitHub (at launch and about once a day,
          unless you turn it off above) and downloading an update when you choose Install. No
          identifiers or packet data are sent; GitHub sees your IP address, and the maintainers
          see only GitHub&apos;s public per-file download counts. Clipboard items are captured only
          while Capture is ON. The only time packet content leaves your machine is when Claude
          Code itself sends it to its configured model provider as the isolated subagent reads it.
        </p>
      </details>
    </div>
  );
}

/**
 * A whole-number setting saved when the field is left (blur) or Enter is
 * pressed. An empty, non-numeric or below-1 value is not saved: the field
 * goes back to the saved value.
 */
function NumberSetting({
  label,
  value,
  saved,
  onCommit,
}: {
  label: string;
  value: number;
  saved: boolean;
  onCommit: (n: number) => Promise<boolean>;
}) {
  // While the field is being edited it shows the typed text; otherwise it
  // always shows the SAVED value (so a value the backend clamped, e.g. an item
  // limit above the packet limit, is never left on screen as if saved).
  const [editing, setEditing] = useState<string | null>(null);

  const commit = useCallback(() => {
    const text = (editing ?? "").trim();
    setEditing(null);
    const n = Number(text);
    if (!/^\d+$/.test(text) || !Number.isSafeInteger(n) || n < 1) return;
    if (n !== value) void onCommit(n);
  }, [editing, value, onCommit]);

  return (
    <label className="field">
      <span>
        {label} <SavedMark show={saved} />
      </span>
      <input
        type="number"
        aria-label={label}
        min={1}
        value={editing ?? String(value)}
        onFocus={() => setEditing(String(value))}
        onChange={(e) => setEditing(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        }}
      />
    </label>
  );
}
