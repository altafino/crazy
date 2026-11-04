const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** "3 minutes ago" from a Date.now()-style millisecond timestamp. */
export function relTime(ms: number, now = Date.now()): string {
  const s = Math.round((ms - now) / 1000);
  if (Math.abs(s) < 60) return "just now";
  if (Math.abs(s) < 3600) return rtf.format(Math.round(s / 60), "minute");
  if (Math.abs(s) < 86400) return rtf.format(Math.round(s / 3600), "hour");
  return rtf.format(Math.round(s / 86400), "day");
}
