interface Props {
  capturing: boolean;
  itemCount: number;
  /** The running app version, shown next to the title once known. */
  version?: string;
}

/**
 * The in-window capture indicator (the menu bar itself shows only
 * the outlined (idle) / filled green (capturing) drop icon).
 */
export function StatusBadge({ capturing, itemCount, version }: Props) {
  return (
    <div className="status-badge" role="status" aria-live="polite">
      <span className={capturing ? "dot dot-on" : "dot dot-off"} aria-hidden="true">
        {capturing ? "●" : "○"}
      </span>
      <span className="status-title">Context Drop</span>
      {version && (
        <span className="status-version" data-testid="app-version">
          v{version}
        </span>
      )}
      {capturing && (
        <span className="status-count" data-testid="badge-count">
          {" · "}
          {itemCount}
        </span>
      )}
    </div>
  );
}
