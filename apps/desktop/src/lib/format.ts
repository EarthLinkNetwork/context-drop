/** Human-readable byte size. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** A short, friendly label for an item kind. */
export function kindLabel(kind: string): string {
  switch (kind) {
    case "text":
      return "Text";
    case "json":
      return "JSON";
    case "html":
      return "HTML";
    case "url":
      return "URL";
    case "image":
      return "Image";
    case "file":
      return "File";
    default:
      return "Item";
  }
}

/** A coarse "time ago" from an RFC3339 timestamp. Never throws. */
export function timeAgo(iso: string, now: number = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const secs = Math.max(0, Math.floor((now - then) / 1000));
  // Round the sub-minute window to "just now" so the label doesn't tick every
  // second on each snapshot refresh — minute granularity is enough here.
  if (secs < 60) return "just now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}
