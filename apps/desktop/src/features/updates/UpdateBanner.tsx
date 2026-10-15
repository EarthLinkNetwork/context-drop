import type { UpdateStatus } from "../../lib/api";

interface Props {
  update: UpdateStatus;
  capturing: boolean;
  onInstall: () => void;
}

/** "vX.Y.Z is available — Install & Restart", then the install's progress. */
export function UpdateBanner({ update, capturing, onInstall }: Props) {
  const { state, version } = update;
  if (!version || (state !== "available" && state !== "downloading" && state !== "installing")) {
    return null;
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
