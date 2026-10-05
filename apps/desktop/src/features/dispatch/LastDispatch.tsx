import type { LastDispatchInfo } from "../../lib/api";
import { timeAgo } from "../../lib/format";

interface Props {
  /** Recent dispatches, newest first. */
  dispatches: LastDispatchInfo[];
  busy: boolean;
  onUndo: () => void;
}

/** The last path segment (e.g. the account name of a Claude config dir). */
function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** The last two path segments, so a working directory stays short. */
function shortPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length <= 2 ? path : `…/${parts.slice(-2).join("/")}`;
}

/**
 * The recent dispatches (claims), newest first, with enough session detail
 * (instruction, terminal tab, session id, account) to tell apart two sessions
 * working on the same project. Undo is offered only on the newest one, and only
 * while that packet is still eligible (not CONSUMED). Undo affects routing only.
 */
export function LastDispatch({ dispatches, busy, onUndo }: Props) {
  if (dispatches.length === 0) {
    return (
      <section className="panel last-dispatch">
        <h2>Recent Dispatches</h2>
        <p className="empty">No dispatches yet.</p>
      </section>
    );
  }

  return (
    <section className="panel last-dispatch">
      <h2>Recent Dispatches</h2>
      <ol className="dispatch-list">
        {dispatches.map((d, i) => {
          const undoable = i === 0 && (d.state === "CLAIMED" || d.state === "PROCESSING");
          return (
            <li
              key={`${d.packetId}-${d.claimedAt}`}
              className="dispatch-entry"
              data-testid="dispatch-entry"
            >
              <div className="dispatch-head">
                <span className="dispatch-project" data-testid="dispatch-project">
                  {d.projectName}
                </span>
                <span className="dispatch-state">{d.state}</span>
                {undoable && (
                  <button
                    type="button"
                    className="btn btn-small btn-undo"
                    disabled={busy}
                    onClick={onUndo}
                  >
                    Undo
                  </button>
                )}
              </div>
              {d.note && (
                <p className="dispatch-note" data-testid="dispatch-note" title={d.note}>
                  “{d.note}”
                </p>
              )}
              <p className="dispatch-meta" title={d.cwd}>
                {d.itemCount} item{d.itemCount === 1 ? "" : "s"} · {timeAgo(d.claimedAt)}
                {d.terminal && <> · {d.terminal}</>} · session …{d.sessionId.slice(-6)}
                {d.configDir && <> · {baseName(d.configDir)}</>} · {shortPath(d.cwd)}
              </p>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
