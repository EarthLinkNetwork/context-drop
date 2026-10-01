import type { LastDispatchInfo } from "../../lib/api";
import { timeAgo } from "../../lib/format";

interface Props {
  dispatch: LastDispatchInfo | null;
  busy: boolean;
  onUndo: () => void;
}

/**
 * The most recent dispatch (claim). Undo is offered only while the packet is
 * still eligible (not CONSUMED). Undo affects routing only.
 */
export function LastDispatch({ dispatch, busy, onUndo }: Props) {
  if (!dispatch) {
    return (
      <section className="panel last-dispatch">
        <h2>Last Dispatch</h2>
        <p className="empty">No dispatches yet.</p>
      </section>
    );
  }

  const undoable = dispatch.state === "CLAIMED" || dispatch.state === "PROCESSING";

  return (
    <section className="panel last-dispatch">
      <h2>Last Dispatch</h2>
      <p className="dispatch-project" data-testid="dispatch-project">
        {dispatch.projectName}
      </p>
      <p className="dispatch-meta">
        {dispatch.itemCount} item{dispatch.itemCount === 1 ? "" : "s"} · {timeAgo(dispatch.claimedAt)} ·{" "}
        <span className="dispatch-state">{dispatch.state}</span>
      </p>
      {undoable && (
        <button type="button" className="btn btn-undo" disabled={busy} onClick={onUndo}>
          Undo
        </button>
      )}
    </section>
  );
}
