import type { CurrentDraft, RecentItem } from "../../lib/api";
import { formatBytes, kindLabel } from "../../lib/format";

interface Props {
  draft: CurrentDraft | null;
  busy: boolean;
  onClear: () => void;
  /** Open the full-content viewer for an item. */
  onOpenItem: (item: RecentItem) => void;
  /** Remove a single item from the packet. */
  onDeleteItem: (itemId: string) => void;
}

/** Shows the current DRAFT packet: item count, recent items, and Clear. */
export function CurrentPacket({ draft, busy, onClear, onOpenItem, onDeleteItem }: Props) {
  if (!draft || draft.itemCount === 0) {
    return (
      <section className="panel current-packet">
        <h2>Current Packet</h2>
        <p className="empty">
          No items captured yet. Start Capture and copy — or drag &amp; drop files here.
        </p>
      </section>
    );
  }

  return (
    <section className="panel current-packet">
      <h2>Current Packet</h2>
      <p className="item-count" data-testid="item-count">
        {draft.itemCount} item{draft.itemCount === 1 ? "" : "s"}
      </p>
      <ul className="recent-items" aria-label="Recent items">
        {draft.recentItems.map((item, i) => (
          <li key={item.id || i} className="recent-item">
            <button
              type="button"
              className="recent-item-open"
              title="View full content"
              onClick={() => onOpenItem(item)}
            >
              {item.imageThumb ? (
                <img className="item-thumb" src={item.imageThumb} alt="" />
              ) : (
                <span className="item-badge">{kindLabel(item.kind)}</span>
              )}
              <span className="item-body">
                {item.preview && <span className="item-preview">{item.preview}</span>}
              </span>
              <span className="item-size">{formatBytes(item.byteSize)}</span>
            </button>
            <button
              type="button"
              className="btn-trash"
              aria-label="Remove item"
              title="Remove from packet"
              disabled={busy}
              onClick={() => onDeleteItem(item.id)}
            >
              🗑
            </button>
          </li>
        ))}
      </ul>
      <button type="button" className="btn btn-clear" disabled={busy} onClick={onClear}>
        Clear Packet
      </button>
    </section>
  );
}
