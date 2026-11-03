/** "12 KB", "1.5 MB": 1024-based, one decimal only below 10 of a unit. */
export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  const v = i > 0 && n < 10 ? Math.round(n * 10) / 10 : Math.round(n);
  return `${v} ${units[i]}`;
}

/** "Binary file changed (12 KB → 15 KB)" / "added (15 KB)" / "deleted (12 KB)". */
export function binaryCopy(oldSize: number | null | undefined, newSize: number | null | undefined): string {
  if (oldSize != null && newSize != null) return `Binary file changed (${formatSize(oldSize)} → ${formatSize(newSize)})`;
  if (newSize != null) return `Binary file added (${formatSize(newSize)})`;
  if (oldSize != null) return `Binary file deleted (${formatSize(oldSize)})`;
  return "Binary file changed";
}

/** Local HH:MM:SS of a unix-ms time; "" when unknown (0 or missing, e.g. old logs). */
export function clockTime(ms: number | null | undefined): string {
  if (!ms) return "";
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes(), d.getSeconds()].map((x) => String(x).padStart(2, "0")).join(":");
}
