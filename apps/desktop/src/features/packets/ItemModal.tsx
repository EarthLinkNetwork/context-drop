import { useEffect, useState } from "react";
import { api, type RecentItem } from "../../lib/api";
import { formatBytes, kindLabel } from "../../lib/format";

interface Props {
  packetId: string;
  item: RecentItem;
  onClose: () => void;
}

/**
 * A modal that shows an item's FULL content: the whole text (scrollable) for
 * text-like items, or a large image for image items. Content is fetched on open
 * via the backend (the list only carries previews/thumbnails).
 */
export function ItemModal({ packetId, item, onClose }: Props) {
  const isImage = Boolean(item.imageThumb) || item.kind === "image";
  const [text, setText] = useState<string | null>(null);
  const [image, setImage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const load = isImage
      ? api.itemFullImage(packetId, item.id).then((d) => !cancelled && setImage(d))
      : api.itemFullText(packetId, item.id).then((t) => !cancelled && setText(t));
    void load
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [packetId, item.id, isImage]);

  // Close on Escape.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span className="modal-title">
            {kindLabel(item.kind)} · {formatBytes(item.byteSize)}
          </span>
          <button
            type="button"
            className="btn btn-small modal-close"
            aria-label="Close"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className="modal-body">
          {loading && <p className="empty">Loading…</p>}
          {!loading && error && (
            <p className="warning" role="alert">
              {error}
            </p>
          )}
          {!loading && !error && isImage && image && (
            <img className="modal-image" src={image} alt="" />
          )}
          {!loading && !error && !isImage && text !== null && (
            <pre className="modal-text">{text}</pre>
          )}
        </div>
      </div>
    </div>
  );
}
