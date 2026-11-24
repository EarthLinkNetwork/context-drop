import type { UpdateStatus } from "../../lib/api";
import { checkedAt } from "./checkedAt";

interface Props {
  appVersion: string;
  update: UpdateStatus;
  capturing: boolean;
  /**
   * Also show a check that found no update (running, "latest version", the
   * failure): the user asked for it from the menu-bar menu.
   */
  showResult: boolean;
  onInstall: () => void;
  onCheck: () => void;
  onDismiss: () => void;
}

/**
 * "vX.Y.Z is available — Install & Restart", then the install's progress.
 * With `showResult`, a check that found no update is shown too.
 */
export function UpdateBanner(props: Props) {
  const { update, capturing, onInstall } = props;
  const { state, version } = update;
  if (!version || (state !== "available" && state !== "downloading" && state !== "installing")) {
    return props.showResult ? <CheckResult {...props} /> : null;
  }
  return (
    <div className="update-banner" role="status" data-testid="update-banner">
      {state === "available" && (
        <>
          <div className="update-banner-row">
            <span>⬆️ Context Drop v{version} is available.</span>
            {!update.blockedReason && (
              <button
                type="button"
                className="btn btn-small btn-start"
                disabled={update.checking}
                onClick={onInstall}
              >
                Install &amp; Restart
              </button>
            )}
          </div>
          {update.blockedReason && <p className="hint">{update.blockedReason}</p>}
          {!update.blockedReason && capturing && (
            <p className="hint">The app restarts and Capture turns off; the current packet is kept.</p>
          )}
          {update.errorDuring === "install" && update.error && (
            <p className="warning" role="alert" data-testid="update-install-error">
              {update.error}
            </p>
          )}
        </>
      )}
      {state === "downloading" && (
        <span>
          Downloading v{version}…{" "}
          {update.total ? `${Math.min(100, Math.floor((update.downloaded / update.total) * 100))}%` : ""}
        </span>
      )}
      {state === "installing" && <span>Installing v{version}… Context Drop will restart.</span>}
    </div>
  );
}

/** The outcome of a check the user asked for, when it found no update. */
function CheckResult({ appVersion, update, onCheck, onDismiss }: Props) {
  if (update.checking) {
    return (
      <div className="update-banner" role="status" data-testid="update-check-result">
        <span>Checking for updates…</span>
      </div>
    );
  }
  if (update.errorDuring === "check" && update.error) {
    return (
      <div className="update-banner is-failed" role="alert" data-testid="update-check-result">
        <div className="update-banner-row">
          <span>{update.error}</span>
          <span className="update-banner-actions">
            <button type="button" className="btn btn-small" onClick={onCheck}>
              Try Again
            </button>
            <button type="button" className="btn btn-small" onClick={onDismiss}>
              Dismiss
            </button>
          </span>
        </div>
      </div>
    );
  }
  if (update.state === "upToDate") {
    return (
      <div className="update-banner" role="status" data-testid="update-check-result">
        <div className="update-banner-row">
          <span>
            ✓ Context Drop v{appVersion} is the latest version{checkedAt(update.checkedAt)}.
          </span>
          <button type="button" className="btn btn-small" onClick={onDismiss}>
            OK
          </button>
        </div>
      </div>
    );
  }
  return null;
}
