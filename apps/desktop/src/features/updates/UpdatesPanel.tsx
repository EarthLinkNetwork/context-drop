import type { UpdateStatus } from "../../lib/api";

interface Props {
  appVersion: string;
  update: UpdateStatus;
  onCheck: () => void;
  onInstall: () => void;
}

/** Settings → Advanced → Updates: the version, Check for Updates, the result. */
export function UpdatesPanel({ appVersion, update, onCheck, onInstall }: Props) {
  if (!update.supported) {
    return <p className="hint">In-app updates are available on macOS only.</p>;
  }
  const working =
    update.state === "checking" || update.state === "downloading" || update.state === "installing";
  return (
    <div className="updates-panel" data-testid="updates-panel">
      <div className="settings-actions">
        <span>Context Drop v{appVersion}</span>
        <button type="button" className="btn btn-small" disabled={working} onClick={onCheck}>
          {update.state === "checking" ? "Checking…" : "Check for Updates"}
        </button>
      </div>
      <p className="hint" role="status" data-testid="updates-status">
        {statusText(update)}
      </p>
      {update.state === "available" && !update.blockedReason && (
        <button type="button" className="btn btn-small btn-start" onClick={onInstall}>
          Install v{update.version} &amp; Restart
        </button>
      )}
      {update.blockedReason && update.state === "available" && (
        <p className="hint">{update.blockedReason}</p>
      )}
      {update.error && (
        <p className="warning" role="alert" data-testid="updates-error">
          {update.error}
        </p>
      )}
    </div>
  );
}

function statusText(update: UpdateStatus): string {
  switch (update.state) {
    case "checking":
      return "Checking for updates…";
    case "upToDate":
      return `You have the latest version${checkedAt(update.checkedAt)}.`;
    case "available":
      return `Version ${update.version} is available.`;
    case "downloading":
      return `Downloading version ${update.version}…`;
    case "installing":
      return `Installing version ${update.version}…`;
    case "idle":
      return update.checkedAt ? `Last checked${checkedAt(update.checkedAt)}.` : "Not checked yet.";
  }
}

function checkedAt(ms: number | null): string {
  if (!ms) return "";
  const time = new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return ` (checked at ${time})`;
}
