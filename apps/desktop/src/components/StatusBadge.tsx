interface Props {
  capturing: boolean;
  itemCount: number;
}

/**
 * The in-window capture indicator (the menu bar itself shows only
 * the outlined (idle) / filled green (capturing) drop icon).
 */
export function StatusBadge({ capturing, itemCount }: Props) {
  return (
    <div className="status-badge" role="status" aria-live="polite">
      <span className={capturing ? "dot dot-on" : "dot dot-off"} aria-hidden="true">
        {capturing ? "●" : "○"}
      </span>
      <span className="status-title">Context Drop</span>
      {capturing && (
        <span className="status-count" data-testid="badge-count">
          {" · "}
          {itemCount}
        </span>
      )}
    </div>
  );
}
