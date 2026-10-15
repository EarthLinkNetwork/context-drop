import { useEffect, useRef, useState } from "react";
import type { ShortcutStatus } from "../../lib/api";
import {
  formatAccelerator,
  normalizeAccelerator,
  recordKey,
  type Platform,
} from "../../lib/accelerator";
import { SavedMark } from "./useAutoSave";

/** Slightly under the backend's 20 s recording limit (see begin_recording). */
export const RECORDING_TIMEOUT_MS = 19_000;

export interface OtherShortcut {
  label: string;
  accelerator: string;
}

interface Props {
  label: string;
  /** The saved accelerator ("" = off). */
  value: string;
  defaultValue: string;
  status?: ShortcutStatus;
  /** The app's other shortcuts, to refuse a combination already in use. */
  others: OtherShortcut[];
  platform: Platform;
  saved: boolean;
  /** Save a new accelerator ("" = off). Resolves false when it failed. */
  onSave: (accelerator: string) => Promise<boolean>;
  /** Tell the backend a recording started / ended (see begin_recording). */
  onBeginRecording: () => void;
  onEndRecording: () => void;
  /** Another row is recording: this row's buttons are disabled. */
  locked: boolean;
  /** The row is on screen (its tab is shown); hiding it ends a recording. */
  visible: boolean;
  /** This row started (true) or stopped (false) recording. */
  onRecordingChange: (recording: boolean) => void;
  /**
   * Subscribe to backend events: a registered shortcut pressed while
   * recording (payload: its accelerator), and the backend ending a recording
   * (window lost focus). Returns an unsubscribe function.
   */
  subscribe: (
    onShortcutPressed: (accelerator: string) => void,
    onRecordingEnded: () => void,
  ) => () => void;
}

/**
 * One global shortcut: shows it, and sets it by recording a key combination
 * ("Change" → press the keys), clears it (off) or resets it to the default.
 * No typing of "CommandOrControl+..." by hand.
 */
export function ShortcutRecorder({
  label,
  value,
  defaultValue,
  status,
  others,
  platform,
  saved,
  onSave,
  onBeginRecording,
  onEndRecording,
  locked,
  visible,
  onRecordingChange,
  subscribe,
}: Props) {
  const [recording, setRecording] = useState(false);
  const [preview, setPreview] = useState("");
  const [hint, setHint] = useState<string | null>(null);
  const recordingRef = useRef(false);
  // Everything the recording reads from props goes through this ref, so the
  // recording effect depends on `recording` ONLY: a parent re-render (the
  // 1.5 s snapshot poll hands down new arrays / callbacks) can never end,
  // restart or re-time a recording.
  const latest = useRef({ others, platform, onSave, onEndRecording, onRecordingChange, subscribe });
  latest.current = { others, platform, onSave, onEndRecording, onRecordingChange, subscribe };

  // A combination already used by another of the app's shortcuts.
  const conflictWith = (acc: string): string | null => {
    const { others: list, platform: pf } = latest.current;
    const n = normalizeAccelerator(acc, pf);
    const hit = list.find(
      (o) => o.accelerator.trim() !== "" && normalizeAccelerator(o.accelerator, pf) === n,
    );
    return hit ? hit.label : null;
  };

  const stopRef = useRef(() => {
    if (!recordingRef.current) return;
    recordingRef.current = false;
    setRecording(false);
    setPreview("");
    latest.current.onEndRecording();
    latest.current.onRecordingChange(false);
  });
  const stop = stopRef.current;

  const acceptRef = useRef((acc: string) => {
    const owner = conflictWithRef.current(acc);
    if (owner) {
      setPreview(formatAccelerator(acc, latest.current.platform));
      setHint(`Already used for “${owner}”. Press another combination.`);
      return;
    }
    stopRef.current();
    setHint(null);
    void latest.current.onSave(acc);
  });
  const conflictWithRef = useRef(conflictWith);
  conflictWithRef.current = conflictWith;

  // While recording: every keydown/keyup is ours (no button re-press, no Tab
  // focus move, no typing), interpreted by recordKey.
  useEffect(() => {
    if (!recording) return;
    const stopNow = stopRef.current;
    const accept = acceptRef.current;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat || e.isComposing) return;
      const r = recordKey(e, latest.current.platform);
      switch (r.kind) {
        case "pending":
          setPreview(r.preview);
          return;
        case "cancel":
          setHint(null);
          stopNow();
          return;
        case "invalid":
          setPreview(r.preview);
          setHint(r.reason);
          return;
        case "ok":
          accept(r.accelerator);
          return;
      }
    };
    const swallow = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
    };
    const onBlur = () => stopNow();
    // The backend stops forwarding after 20 s; end here a little earlier so
    // the row never shows "recording" while presses act again. Set once per
    // recording (this effect does not re-run on re-renders).
    const timeout = setTimeout(() => {
      setHint(null);
      stopNow();
    }, RECORDING_TIMEOUT_MS);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", swallow, true);
    window.addEventListener("blur", onBlur);
    // A registered shortcut never reaches the page (the OS takes it); the
    // backend forwards it instead, and ends the recording on focus loss or at
    // its time limit.
    const unsubscribe = latest.current.subscribe(
      (acc) => {
        if (recordingRef.current) accept(acc);
      },
      () => stopNow(),
    );
    return () => {
      clearTimeout(timeout);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", swallow, true);
      window.removeEventListener("blur", onBlur);
      unsubscribe();
    };
  }, [recording]);

  // Leaving the tab (or the Settings view) ends a recording: a hidden row must
  // not keep taking every key press.
  useEffect(() => {
    if (!visible) stopRef.current();
  }, [visible]);

  // Never leave the backend recording if this row goes away mid-recording.
  useEffect(() => () => stopRef.current(), []);

  const start = () => {
    if (locked) return;
    (document.activeElement as HTMLElement | null)?.blur?.();
    recordingRef.current = true;
    setRecording(true);
    setPreview("");
    setHint(null);
    onRecordingChange(true);
    onBeginRecording();
  };

  const reset = () => {
    const owner = conflictWith(defaultValue);
    if (owner) {
      setHint(`The default is already used for “${owner}”. Change that one first.`);
      return;
    }
    setHint(null);
    void onSave(defaultValue);
  };

  const shown = formatAccelerator(value, platform);
  const problem =
    status?.state === "failed"
      ? "Not active — another app may be using this combination."
      : status?.state === "duplicate"
        ? `Not active — same as “${status.duplicateOf ?? "another shortcut"}”.`
        : status?.state === "invalid"
          ? "Not active — this shortcut can't be used."
          : null;

  return (
    <div className="shortcut-row" data-testid={`shortcut-${label}`}>
      <div className="shortcut-head">
        <span className="shortcut-label">{label}</span>
        <SavedMark show={saved} />
      </div>
      <div className="shortcut-controls">
        <span
          className={recording ? "shortcut-keys is-recording" : "shortcut-keys"}
          aria-label={`${label} shortcut`}
          aria-live="polite"
        >
          {recording ? preview || "Press keys…" : shown || "Off"}
        </span>
        {recording ? (
          <button
            type="button"
            className="btn btn-small"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              setHint(null);
              stop();
            }}
          >
            Cancel
          </button>
        ) : (
          <>
            <button type="button" className="btn btn-small" disabled={locked} onClick={start}>
              Change
            </button>
            <button
              type="button"
              className="btn btn-small"
              disabled={locked || value === ""}
              onClick={() => {
                setHint(null);
                void onSave("");
              }}
            >
              Clear
            </button>
            <button
              type="button"
              className="btn btn-small"
              disabled={locked || value === defaultValue}
              onClick={reset}
            >
              Reset
            </button>
          </>
        )}
      </div>
      {recording && !hint && (
        <p className="hint">
          Press the new combination (with {platform === "mac" ? "⌘, ⌃ or ⌥" : "Ctrl or Alt"}). Esc
          cancels.
        </p>
      )}
      {hint && (
        <p className="hint hint-warn" role="alert">
          {hint}
        </p>
      )}
      {!recording && problem && (
        <p className="warning" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}
