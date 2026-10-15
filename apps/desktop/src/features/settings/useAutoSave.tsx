import { useCallback, useEffect, useRef, useState } from "react";
import type { Settings } from "../../lib/api";

/**
 * Auto-save for the Settings tabs: every change is saved on its own, at once,
 * as a patch of just that field (the backend applies it on top of the SAVED
 * settings, so nothing else is undone). Saves are sent one after another, in
 * the order they were made. `savedKey` names the field saved most recently,
 * for a short "Saved" mark; `error` is the last failure, shown in the tab.
 *
 * `pending` holds every change not yet saved+refreshed. The tabs show and
 * compare against `{...saved, ...pending}` (what the user last chose), so a
 * check made while an earlier save is still in flight — "is this number
 * different?", "is this key already used?" — sees that earlier change too.
 */
export function useAutoSave(onPatch: (patch: Partial<Settings>) => Promise<string | null>) {
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  const [savedKey, setSavedKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Partial<Settings>>({});
  // Per field: number of the latest save that set it; only that save clears it.
  const seq = useRef<Record<string, number>>({});
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const save = useCallback(
    (patch: Partial<Settings>, key: string): Promise<boolean> => {
      const fields = Object.keys(patch);
      const mine: Record<string, number> = {};
      for (const f of fields) mine[f] = seq.current[f] = (seq.current[f] ?? 0) + 1;
      setPending((p) => ({ ...p, ...patch }));
      const settle = () =>
        setPending((p) => {
          const next: Record<string, unknown> = { ...p };
          for (const f of fields) if (seq.current[f] === mine[f]) delete next[f];
          return next as Partial<Settings>;
        });
      const run = chain.current.then(() => onPatch(patch));
      chain.current = run.catch(() => null);
      run.then(settle, settle);
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

  return { save, savedKey, error, pending };
}

/** A short confirmation shown next to a setting right after it was saved. */
export function SavedMark({ show }: { show: boolean }) {
  return show ? (
    <span className="saved-mark" role="status">
      Saved
    </span>
  ) : null;
}
