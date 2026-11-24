/** " (checked at 09:05)" for a check that finished at `ms`, or "" for none. */
export function checkedAt(ms: number | null): string {
  if (!ms) return "";
  const time = new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return ` (checked at ${time})`;
}
