import { useState } from "react";
import { StatusBadge } from "../components/StatusBadge";
import { CapturePanel } from "../features/capture/CapturePanel";
import { CurrentPacket } from "../features/packets/CurrentPacket";
import { ItemModal } from "../features/packets/ItemModal";
import { LastDispatch } from "../features/dispatch/LastDispatch";
import { InstallationPanel } from "../features/settings/InstallationPanel";
import { SettingsView, type SettingsTab } from "../features/settings/SettingsView";
import { UpdateBanner } from "../features/updates/UpdateBanner";
import { UpdatesPanel } from "../features/updates/UpdatesPanel";
import { detectPlatform } from "../lib/accelerator";
import { onBackendEvent } from "../lib/events";
import { api } from "../lib/api";
import type { RecentItem, ShortcutActionId } from "../lib/api";
import { useAppState } from "../lib/useAppState";

const PLATFORM = detectPlatform();

const SHORTCUT_LABEL: Record<ShortcutActionId, string> = {
  capture: "Start / Stop Capture",
  window: "Show / hide window",
  onTop: "Always on Top",
};

// Stable (module-level) so a re-render never changes their identity.
function beginRecording() {
  void api.beginRecording().catch(() => undefined);
}
function endRecording() {
  void api.endRecording().catch(() => undefined);
}

/** Backend events while a shortcut is being recorded (see ShortcutRecorder). */
function subscribeRecording(
  onShortcutPressed: (accelerator: string) => void,
  onRecordingEnded: () => void,
): () => void {
  const a = onBackendEvent<string>("cd:shortcut-pressed", onShortcutPressed);
  const b = onBackendEvent<null>("cd:recording-ended", onRecordingEnded);
  return () => {
    a();
    b();
  };
}

/** The Context Drop popover UI. */
export function App() {
  const state = useAppState();
  const [message, setMessage] = useState<string | null>(null);
  const [tab, setTab] = useState<"capture" | "settings">("capture");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("shortcuts");
  const [openItem, setOpenItem] = useState<RecentItem | null>(null);

  if (state.loading && !state.snapshot) {
    return (
      <main className="app app-loading">
        <StatusBadge capturing={false} itemCount={0} />
        <p>Loading…</p>
      </main>
    );
  }

  const snap = state.snapshot;
  if (!snap) {
    return (
      <main className="app app-error">
        <StatusBadge capturing={false} itemCount={0} />
        <p className="error-banner" role="alert">
          {state.error ?? "Context Drop is unavailable."}
        </p>
        <button type="button" className="btn" onClick={() => void state.refresh()}>
          Retry
        </button>
      </main>
    );
  }

  const itemCount = snap.currentDraft?.itemCount ?? 0;
  // Shortcuts that are set but not working (an "off" one is not a problem).
  const brokenShortcuts = snap.shortcuts
    .filter((s) => s.state === "failed" || s.state === "duplicate" || s.state === "invalid")
    .map((s) => SHORTCUT_LABEL[s.action]);

  return (
    <main className={state.dragOver ? "app is-dragover" : "app"}>
      {state.dragOver && (
        <div className="drop-overlay" role="status" aria-label="Drop to capture">
          <span>⤓ Drop to capture</span>
        </div>
      )}
      <StatusBadge capturing={snap.capturing} itemCount={itemCount} version={snap.appVersion} />

      {state.error && (
        <p className="error-banner" role="alert" data-testid="error-banner">
          {state.error}
        </p>
      )}
      {snap.notice && (
        <p className="warning" role="status" data-testid="notice-banner">
          {snap.notice}
        </p>
      )}
      {message && (
        <p className="info-banner" role="status" data-testid="info-banner">
          {message}
        </p>
      )}

      <UpdateBanner
        update={snap.update}
        capturing={snap.capturing}
        onInstall={() => void state.installUpdate()}
      />

      {brokenShortcuts.length > 0 && (
        <p className="warning" role="alert" data-testid="shortcut-conflict-global">
          {brokenShortcuts.length === 1
            ? `The “${brokenShortcuts[0]}” shortcut is`
            : `The ${brokenShortcuts.map((l) => `“${l}”`).join(" and ")} shortcuts are`}{" "}
          not active. Fix it in Settings → Shortcuts.
        </p>
      )}

      {!snap.integrations.some((i) => i.enabled) && (
        <div className="setup-banner" data-testid="setup-banner">
          <span>⚙️ Finish setup — install the Context Drop plugin in Claude Code.</span>
          <button
            type="button"
            className="btn btn-small"
            onClick={() => {
              setTab("settings");
              setSettingsTab("claude");
            }}
          >
            Open Setup
          </button>
        </div>
      )}

      <nav className="tabs" role="tablist" aria-label="Views">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "capture"}
          className={tab === "capture" ? "tab is-active" : "tab"}
          onClick={() => setTab("capture")}
        >
          Capture
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "settings"}
          className={tab === "settings" ? "tab is-active" : "tab"}
          onClick={() => setTab("settings")}
        >
          Settings
        </button>
      </nav>

      {/* Both panels stay mounted (toggled with `hidden`) so unsaved settings and
          scroll position survive a tab switch. `hidden` also drops the inactive
          panel from the accessibility tree, so role queries only see the active tab. */}
      <div className="tab-panel" hidden={tab !== "capture"}>
        <CapturePanel
          capturing={snap.capturing}
          busy={state.busy}
          onStart={() => void state.startCapture()}
          onStop={() => void state.stopCapture()}
          onCaptureNow={() => void state.captureClipboardNow()}
        />

        <CurrentPacket
          draft={snap.currentDraft}
          busy={state.busy}
          onClear={() => void state.clearPacket()}
          onOpenItem={(item) => setOpenItem(item)}
          onDeleteItem={(itemId) => {
            if (snap.currentDraft) void state.deleteItem(snap.currentDraft.packetId, itemId);
          }}
        />

        <LastDispatch
          dispatches={snap.recentDispatches}
          busy={state.busy}
          onUndo={() => void state.undoLast()}
        />
      </div>

      <div className="tab-panel" hidden={tab !== "settings"}>
        <SettingsView
          visible={tab === "settings"}
          tab={settingsTab}
          onTabChange={setSettingsTab}
          settings={snap.settings}
          shortcuts={snap.shortcuts}
          platform={PLATFORM}
          onPatch={state.updateSettings}
          onListDisplays={api.listDisplays}
          onBeginRecording={beginRecording}
          onEndRecording={endRecording}
          subscribeRecording={subscribeRecording}
          onOpenDataFolder={() => void state.openDataFolder()}
          updates={
            <UpdatesPanel
              appVersion={snap.appVersion}
              update={snap.update}
              onCheck={() => void state.checkForUpdates()}
              onInstall={() => void state.installUpdate()}
            />
          }
          claudeSetup={
            <InstallationPanel
              integrations={snap.integrations}
              marketplaceGithub={snap.marketplaceGithub}
              shortAliasInstalled={snap.settings.shortAliasInstalled}
              busy={state.busy}
              onInstallIntegration={(dir) => {
                // Clear any stale banner first, then show the outcome. On failure the
                // detailed error also appears in the error banner (see useAppState).
                setMessage(null);
                void state
                  .installIntegration(dir)
                  .then((r) => setMessage(r ?? "Integration install failed — see the error above."));
              }}
              onInstallShortAlias={() => {
                setMessage(null);
                void state
                  .installShortAlias()
                  .then((r) => setMessage(r ?? "Short alias install failed — see the error above."));
              }}
            />
          }
        />
      </div>

      {openItem && snap.currentDraft && (
        <ItemModal
          packetId={snap.currentDraft.packetId}
          item={openItem}
          onClose={() => setOpenItem(null)}
        />
      )}
    </main>
  );
}
