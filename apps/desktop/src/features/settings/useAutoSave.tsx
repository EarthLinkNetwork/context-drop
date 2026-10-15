import { useCallback, useEffect, useRef, useState } from "react";
import type { Settings } from "../../lib/api";

/**
 * Auto-save for the Settings tabs: every change is saved on its own, at once,
 * as a patch of just that field (the backend applies it on top of the SAVED
 * settings, so nothing else is undone). Saves are sent one after another, in
 * the order they were made. `savedKey` names the field saved most recently,
 * for a short "Saved" mark; `error` is the last failure, shown in the tab.
 */
export function useAutoSave(onPatch: (patch: Partial<Settings>) => Promise<string | null>) {
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  const [savedKey, setSavedKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const save = useCallback(
    (patch: Partial<Settings>, key: string): Promise<boolean> => {
      const run = chain.current.then(() => onPatch(patch));
      chain.current = run.catch(() => null);
      return run.then(
        (err) => {
          if (err) {
            setError(err);
            return false;
          }
          setError(null);
          setSavedKey(key);
          if (timer.current) clearTimeout(timer.current);
          timer.current = setTimeout(() => setSavedKey(null), 1500);
          return true;
        },
        (e: unknown) => {
          setError(String(e));
          return false;
        },
      );
    },
    [onPatch],
  );

  return { save, savedKey, error };
}

/** A short confirmation shown next to a setting right after it was saved. */
export function SavedMark({ show }: { show: boolean }) {
  return show ? (
    <span className="saved-mark" role="status">
      Saved
    </span>
  ) : null;
}
