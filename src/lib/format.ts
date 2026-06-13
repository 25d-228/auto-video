/**
 * Display formatters ported from the old vanilla engine (ui-src/engine.js,
 * now only in git history). Keep output byte-identical so the new UI reads
 * exactly like the old one.
 */

/** Seeder counts: 1234 -> "1.2k", 845 -> "845". */
export function fseed(n: number): string {
  return n >= 1000 ? (n / 1000).toFixed(1) + "k" : String(n)
}

/** Bytes -> "412 GB" style; GB and TB keep one decimal, smaller units round. */
export function fmtBytes(bytes: number): string {
  let b = bytes || 0
  const units = ["B", "KB", "MB", "GB", "TB"]
  let i = 0
  while (b >= 1024 && i < units.length - 1) {
    b /= 1024
    i++
  }
  return (i >= 3 ? b.toFixed(1) : String(Math.round(b))) + " " + units[i]
}

/** Days-ago -> "just now" / "1 day ago" / "12 days ago" / "3 mo ago". */
export function relAdded(days: number): string {
  if (days <= 0) return "just now"
  if (days === 1) return "1 day ago"
  if (days < 30) return days + " days ago"
  return Math.round(days / 30) + " mo ago"
}
