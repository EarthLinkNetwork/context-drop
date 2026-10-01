interface Props {
  capturing: boolean;
  itemCount: number;
}

/**
 * The capture indicator, mirroring the tray title:
 * "○ Context Drop" (idle) / "● Context Drop · N" (capturing).
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
