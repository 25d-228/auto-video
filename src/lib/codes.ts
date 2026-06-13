/**
 * Pure parsing helpers ported faithfully from sidecar/av_proxy.py
 * (parse_code, is_vr, parse_tv_name). Keep behavior in sync with the
 * Python originals — the sidecar still uses them for /library and /discover.
 */

/** Known VR labels; a leading digit is part of the label only for these. */
export const VR_LABELS = [
  "SIVR",
  "IPVR",
  "DSVR",
  "CRVR",
  "VRKM",
  "3DSVR",
  "VRTM",
  "EXVR",
  "KAVR",
  "TMAVR",
  "MAXVR",
  "AJVR",
  "JUVR",
  "HNVR",
  "WPVR",
  "TPVR",
  "DOVR",
  "SAVR",
  "VDVR",
  "MDVR",
  "VOVS",
  "CBIKMV",
  "URVRSP",
  "KMVR",
  "FSVSS",
] as const

const VR_LABEL_RE = new RegExp(`\\b(${VR_LABELS.join("|")})\\b`, "i")

/** Python: is_vr(title, code) */
export function isVr(title: string, code: string): boolean {
  const t = title || ""
  // case-sensitive on purpose (matches the Python regex without re.I)
  if (/(^|[\s[(])VR([\s\])]|専用|$)/.test(t)) return true
  if (t.toUpperCase().includes("[VR]")) return true
  if (code && VR_LABEL_RE.test(code)) return true
  return false
}

/**
 * Python: parse_code(title). Extracts a JAV code like "ABCD-123" from a
 * release title. FC2-PPV ids are normalized; a leading digit on the label
 * (e.g. 3DSVR) survives only for known VR labels.
 */
export function parseCode(title: string): string {
  const t = title || ""
  const fc2Match = /FC2[-\s_]?PPV[-\s_]?(\d{6,7})/i.exec(t)
  if (fc2Match) return "FC2-PPV-" + fc2Match[1]
  // Amateur "maker-prefix" labels: a 3-digit maker prefix is part of the
  // canonical code (459TEN-048, 300MIUM-1380, 200GANA-3386, 230ORECZ-553).
  // Keep it — a static parser cannot re-derive a dropped prefix, so files must
  // be named with it. (?<!\d) stops the prefix grabbing the tail of a longer
  // number; this branch is tried before the generic one below.
  const makerMatch = /(?<!\d)(\d{3}[A-Za-z]{2,6})[-_\s]?(\d{2,5})/.exec(t)
  if (makerMatch) return makerMatch[1]!.toUpperCase() + "-" + makerMatch[2]!
  const genericMatch = /(\d?[A-Za-z]{2,6})[-_\s]?(\d{2,5})/.exec(t)
  if (!genericMatch) return ""
  let label = genericMatch[1]!.toUpperCase()
  const num = genericMatch[2]!
  if (/^\d/.test(label) && !VR_LABEL_RE.test(label)) {
    label = label.slice(1)
  }
  return label + "-" + num
}

/**
 * Canonicalise a JAV code's numeric part to the usual 3-digit form by dropping
 * excess leading zeros (e.g. a 5-digit on-disk pad "MIVR-00081" -> "MIVR-081").
 * Used as a FALLBACK only — try the original code first, then this — so it can
 * never regress a code that already resolves at its padded form. FC2-PPV and
 * other multi-segment ids are left untouched.
 */
export function normalizeCodeNum(code: string): string {
  const m = /^([A-Za-z0-9]+)-(\d+)$/.exec(code || "")
  if (!m) return code || ""
  const n = m[2]!.replace(/^0+/, "") || "0"
  return `${m[1]}-${n.padStart(3, "0")}`
}

// markers that end the series-name part of a TV release name
const TV_MARKERS: RegExp[] = [
  /\bS\d{1,2}E\d{1,3}\b/i,
  /\bS\d{1,2}\b/i,
  /\bSeason\s*\d+/i,
  /\b(19|20)\d{2}\b/i,
  /\b\d{3,4}p\b/i,
  /\bx26[45]\b/i,
  /\bWEB/i,
  /\bBluRay/i,
  /\bHDTV/i,
  /\bDVDRip/i,
  /\bComplete/i,
  /\bREPACK/i,
  /\bHEVC/i,
  /\b720|\b1080|\b2160/i,
]

/** Equivalent of Python's str.strip(chars): trim a char set from both ends. */
function stripChars(s: string, chars: string): string {
  let start = 0
  let end = s.length
  while (start < end && chars.includes(s[start]!)) start++
  while (end > start && chars.includes(s[end - 1]!)) end--
  return s.slice(start, end)
}

/**
 * Python: parse_tv_name(name). Returns [series, se] where se is "SxxEyy"
 * (zero-padded to at least 2 digits) or "". The series name is everything
 * before the earliest marker, with dots/underscores turned into spaces.
 */
export function parseTvName(name: string): [series: string, se: string] {
  const s = name || ""
  const m = /\bS(\d{1,2})E(\d{1,3})\b/i.exec(s)
  const se = m
    ? "S" +
      String(parseInt(m[1]!, 10)).padStart(2, "0") +
      "E" +
      String(parseInt(m[2]!, 10)).padStart(2, "0")
    : ""
  let cut = s.length
  for (const marker of TV_MARKERS) {
    const hit = marker.exec(s)
    if (hit && hit.index < cut) cut = hit.index
  }
  let series = s.slice(0, cut).replace(/[._]/g, " ")
  series = stripChars(series.replace(/\s+/g, " "), " -:.[]")
  return [series || s, se]
}
