// Small text helpers shared by the shell and the pages (RFC 0261 §2.7).

/** A catalogue command name as a title: `init-and-compile` → "Init and compile". */
export function commandTitle(command: string): string {
  const words = command.replace(/[-_]/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** "12 minutes ago", "in 3 hours", "just now"; `now` is injectable for tests. */
export function relativeTime(iso: string, now: number = Date.now()): string {
  const t = Date.parse(/(?:[zZ]|[+-]\d\d:?\d\d)$/.test(iso) ? iso : `${iso}Z`);
  if (Number.isNaN(t)) return iso;
  const s = Math.round((t - now) / 1000);
  const abs = Math.abs(s);
  if (abs < 45) return "just now";
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["minute", 60],
    ["hour", 3600],
    ["day", 86400],
  ];
  const fmt = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  let unit: Intl.RelativeTimeFormatUnit = "minute";
  let size = 60;
  for (const [u, n] of units) {
    if (abs >= n) {
      unit = u;
      size = n;
    }
  }
  return fmt.format(Math.round(s / size), unit);
}
