import { useState } from "react";
import { StatusBadge } from "../components/StatusBadge";
import { CapturePanel } from "../features/capture/CapturePanel";
import { CurrentPacket } from "../features/packets/CurrentPacket";
import { ItemModal } from "../features/packets/ItemModal";
import { LastDispatch } from "../features/dispatch/LastDispatch";
import { SettingsPanel } from "../features/settings/SettingsPanel";
import type { RecentItem } from "../lib/api";
import { useAppState } from "../lib/useAppState";

/** The Context Drop popover UI. */
export function App() {
  const state = useAppState();
  const [message, setMessage] = useState<string | null>(null);
  const [tab, setTab] = useState<"capture" | "settings">("capture");
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

  return (
    <main className={state.dragOver ? "app is-dragover" : "app"}>
      {state.dragOver && (
        <div className="drop-overlay" role="status" aria-label="Drop to capture">
          <span>⤓ Drop to capture</span>
        </div>
      )}
      <StatusBadge capturing={snap.capturing} itemCount={itemCount} />

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

      {!snap.shortcutRegistered && (
        <p className="warning" role="alert" data-testid="shortcut-conflict-global">
          Global shortcut is not active — it may conflict with another app. Fix it in Settings.
        </p>
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
      </div>

      <div className="tab-panel" hidden={tab !== "settings"}>
        <SettingsPanel
          settings={snap.settings}
          shortcutRegistered={snap.shortcutRegistered}
          integrations={snap.integrations}
          busy={state.busy}
          onSave={(s) => void state.saveSettings(s)}
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
          onOpenDataFolder={() => void state.openDataFolder()}
        />

        <LastDispatch
          dispatch={snap.lastDispatch}
          busy={state.busy}
          onUndo={() => void state.undoLast()}
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
