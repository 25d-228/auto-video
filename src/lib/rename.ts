/**
 * Post-download rename planner (pure). After a torrent finishes, its files
 * carry arbitrary, often spam-laden names (`489155.com@SNOS-239-C.mp4`) plus
 * bundled junk. Given the torrent's files, the canonical base name the app
 * already knows (JAV code for ad/vrc, `YEAR.Title` for mov, show name for tv)
 * and the category, this decides how to rename the KEPT video file(s) into the
 * library's naming convention — the same shape the scan/cover algorithm expects.
 *
 * Returns {from,to} relative paths within the destination folder. Non-video
 * files, named samples and tiny extras are dropped (no rename). The actual
 * filesystem moves are applied in Rust on the download's `done` transition.
 */
export type RenameCat = "mov" | "tv" | "ad" | "vrc"

export interface TorrentFileInfo {
  /** Path within the torrent (slash-joined for multi-file torrents). */
  name: string
  size: number
}

export interface RenameOp {
  /** Source path relative to the destination folder (the torrent's layout). */
  from: string
  /** Canonical target path relative to the destination folder. */
  to: string
}

const RENAME_VIDEO_RE = /\.(mkv|mp4|avi|wmv|m4v|ts|mov|flv|iso|rmvb|webm|mpg|mpeg)$/i
/** Fraction of the largest video below which a video is treated as a sample/extra. */
const MAIN_VIDEO_MIN_FRACTION = 0.15

function fileExt(name: string): string {
  const m = /\.[^./\\]+$/.exec(name)
  return m ? m[0].toLowerCase() : ""
}

function baseName(p: string): string {
  const parts = p.split("/")
  return parts[parts.length - 1] ?? p
}

/** Strip filesystem-hostile characters from a base name (keep it one path segment). */
function safeBase(base: string): string {
  return base.trim().replace(/[/\\]+/g, "-").replace(/\s+/g, " ").trim()
}

/**
 * Plan the canonical renames for a finished torrent. `files` is the full
 * torrent file list (or the user-picked subset). Empty plan = leave as-is.
 */
export function planRename(
  cat: RenameCat,
  base: string,
  files: readonly TorrentFileInfo[]
): RenameOp[] {
  const cleanBase = safeBase(base)
  if (!cleanBase) return []

  const videos = files.filter((f) => RENAME_VIDEO_RE.test(f.name))
  if (videos.length === 0) return []

  // Keep the "main" video(s): drop named samples and anything tiny vs the
  // largest (trailers, "making of" clips, the spam .mp4 ads bundled in JAV
  // torrents). If that leaves nothing, fall back to every video.
  const maxSize = Math.max(...videos.map((v) => v.size), 1)
  const mains = videos.filter(
    (v) => !/\bsample\b/i.test(v.name) && v.size >= maxSize * MAIN_VIDEO_MIN_FRACTION
  )
  const kept = mains.length > 0 ? mains : videos

  // Stable, natural order so multi-disc parts label A/B/C deterministically.
  const sorted = [...kept].sort((a, b) =>
    baseName(a.name).localeCompare(baseName(b.name), undefined, { numeric: true })
  )

  if (cat === "tv") {
    // Series: collect episodes under a `<Show>/` folder, keep their own names.
    return sorted.map((v) => ({ from: v.name, to: `${cleanBase}/${baseName(v.name)}` }))
  }

  // Movies / adult / VR: a flat canonical file in the category folder.
  if (sorted.length === 1) {
    return [{ from: sorted[0]!.name, to: `${cleanBase}${fileExt(sorted[0]!.name)}` }]
  }
  // Multi-disc: <base>-A.ext, <base>-B.ext, …
  return sorted.map((v, i) => ({
    from: v.name,
    to: `${cleanBase}-${String.fromCharCode(65 + i)}${fileExt(v.name)}`,
  }))
}
