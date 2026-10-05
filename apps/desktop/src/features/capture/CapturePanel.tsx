interface Props {
  capturing: boolean;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  /** Capture what is on the clipboard right now, once (no Capture session). */
  onCaptureNow: () => void;
}

/** Start/stop capture control with the always-visible privacy note. */
export function CapturePanel({ capturing, busy, onStart, onStop, onCaptureNow }: Props) {
  return (
    <section className="panel capture-panel">
      <p className="privacy-note">
        Context Drop watches the clipboard only while Capture is ON. Capture Clipboard Now adds
        what is on it right now, once.
      </p>
      <p className="drop-hint">
        Or drag &amp; drop files onto this window anytime — no need to copy.
      </p>
      <div className="capture-actions">
        {capturing ? (
          <button type="button" disabled={busy} onClick={onStop} className="btn btn-stop">
            Stop Capture
          </button>
        ) : (
          <button type="button" disabled={busy} onClick={onStart} className="btn btn-start">
            Start Capture
          </button>
        )}
        <button
          type="button"
          disabled={busy}
          onClick={onCaptureNow}
          className="btn"
          title="Add what is on the clipboard right now, without starting Capture"
        >
          Capture Clipboard Now
        </button>
      </div>
    </section>
  );
}
