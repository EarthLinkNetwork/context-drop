interface Props {
  capturing: boolean;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
}

/** Start/stop capture control with the always-visible privacy note. */
export function CapturePanel({ capturing, busy, onStart, onStop }: Props) {
  return (
    <section className="panel capture-panel">
      <p className="privacy-note">
        Context Drop captures clipboard items only while Capture is ON.
      </p>
      <p className="drop-hint">
        Or drag &amp; drop files onto this window anytime — no need to copy.
      </p>
      {capturing ? (
        <button type="button" disabled={busy} onClick={onStop} className="btn btn-stop">
          Stop Capture
        </button>
      ) : (
        <button type="button" disabled={busy} onClick={onStart} className="btn btn-start">
          Start Capture
        </button>
      )}
    </section>
  );
}
