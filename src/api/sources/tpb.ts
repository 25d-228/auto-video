/**
 * The Pirate Bay (apibay.org) source.
 *
 * Two responsibilities:
 *   1. fetchTv(mode) -> DiscoverItem[] for the TV Discover feed (cat="tv").
 *      - mode "newest"   : apibay q.php?q=category:205 (TV category, newest first)
 *      - mode "trending" : the precompiled data_top100_205.json, sorted by seeders
 *      Series / SxxEyy come from parseTvName(). Covers are resolved elsewhere
 *      (tvmaze), so cover is left "" here.
 *   2. seedersApibay(query) -> Release[] for the Download dialog / per-item seed
 *      badge: a free-text q.php search turned into magnet rows (with trackers).
 *
 * apibay's q.php returns every field as a string; the precompiled top100 file
 * returns id/size/seeders as numbers. Both are coerced defensively here.
 *
 * Network goes through src/net/http.ts; TTL caching uses src/state/db.ts's
 * listing_cache.
 */
import { httpJson } from "@/net/http"
import { getCached, setCached, isDbAvailable } from "@/state/db"
import type { DiscoverItem, DiscoverMode, Release } from "@/api/types"
import { parseTvName } from "@/lib/codes"
import { quality } from "@/api/sources/sukebei"

/** apibay TV category id (category:205). */
const TV_CATEGORY = 205

/** Base host for the apibay JSON API. */
const APIBAY = "https://apibay.org"

/** Seconds a listing is cached before re-fetch. */
const LIST_TTL = 300

/** Cap on the number of TV rows returned by parseTv. */
const MAX_TV_ITEMS = 100

/** Aspect ratio assigned to TV DiscoverItems. */
const TV_POSTER_AR = 0.7

/**
 * One raw apibay torrent row. Every field arrives as a string from q.php and as
 * a string-or-number from the precompiled top100 file, so the numeric fields are
 * typed loosely and coerced at the boundary.
 */
interface ApibayRow {
  id?: string | number
  name?: string
  info_hash?: string
  seeders?: string | number
  size?: string | number
  imdb?: string
}

// ----------------------------------------------------------------- coercion

/** parse to int, 0 on failure. */
function toInt(v: unknown): number {
  const n = parseInt(String(v ?? ""), 10)
  return Number.isFinite(n) ? n : 0
}

/**
 * Bytes use "%d B"; KB and up use one decimal place (646.0 MB, 1.9 GB, 512 B,
 * 0 B).
 */
export function humanSize(bytes: number): string {
  let b = Number(bytes) || 0
  const units = ["B", "KB", "MB", "GB", "TB"]
  for (const u of units) {
    if (b < 1024) {
      return u === "B" ? `${Math.trunc(b)} ${u}` : `${b.toFixed(1)} ${u}`
    }
    b /= 1024
  }
  return `${b.toFixed(1)} PB`
}

/**
 * URL-encode but keep "/" raw (the magnet `dn` and q.php query depend on it).
 * encodeURIComponent escapes "/", so we decode it back.
 */
function pyQuote(s: string): string {
  return encodeURIComponent(s).replace(/%2F/g, "/")
}

// ----------------------------------------------------------------- magnets

/** Trackers appended to every built magnet. */
const TRACKERS = [
  "udp://tracker.opentrackr.org:1337/announce",
  "udp://open.demonii.com:1337/announce",
  "udp://tracker.openbittorrent.com:6969/announce",
  "udp://exodus.desync.com:6969/announce",
]
  .map((t) => "&tr=" + pyQuote(t))
  .join("")

function buildMagnet(infoHash: string, name: string): string {
  return `magnet:?xt=urn:btih:${infoHash}&dn=${pyQuote(name || "")}${TRACKERS}`
}

/** True when an info_hash is all zeros (apibay's "no results" sentinel hash). */
function isZeroHash(ih: string): boolean {
  return ih.length > 0 && /^0+$/.test(ih)
}

// ----------------------------------------------------------------- fetch_tv

/**
 * Parse a raw apibay TV category response into DiscoverItem[] (cat="tv"). Pure
 * (no network). Drops sentinel/empty rows, parses series + SxxEyy via parseTvName,
 * sorts by seeders for trending (q.php is already newest-first), caps at 100,
 * leaves cover "" (resolved later by tvmaze). The link points at the TPB
 * description page keyed by the apibay numeric id.
 */
export function parseTv(arr: unknown, mode: DiscoverMode): DiscoverItem[] {
  if (!Array.isArray(arr)) return []

  const rows = (arr as ApibayRow[]).filter(
    (x) =>
      String(x?.id ?? "0") !== "0" &&
      !!(x?.name ?? "") &&
      x?.name !== "No results returned"
  )

  // trending: precompiled top100 isn't strictly sorted -> sort by seeders desc.
  // newest: keep q.php's natural (newest-first) order.
  if (mode !== "newest") {
    rows.sort((a, b) => toInt(b.seeders) - toInt(a.seeders))
  }

  const out: DiscoverItem[] = []
  for (const row of rows.slice(0, MAX_TV_ITEMS)) {
    const name = row.name ?? ""
    const imdb = (row.imdb ?? "").trim()
    const [series, seasonEpisode] = parseTvName(name)
    const seeders = toInt(row.seeders)
    const size = humanSize(toInt(row.size))
    const tid = String(row.id ?? "").trim()

    // sub = "<SxxEyy> · <imdb>" with empty parts trimmed.
    const sub = ((seasonEpisode ? seasonEpisode + " · " : "") + (imdb || "")).replace(/^[\s·]+|[\s·]+$/g, "")

    const item: DiscoverItem = {
      id: `tv_${tid}`,
      cat: "tv",
      title: series || name,
      sub,
      cover: "",
      ar: TV_POSTER_AR,
      seeders,
      size,
      src: "TPB",
      state: "new",
      year: "",
      runtime: 0,
      rating: 0,
      code: imdb,
    }
    // Only build a description link for a real numeric id (never id=0).
    if (tid && tid !== "0") {
      item.link = `https://thepiratebay.org/description.php?id=${tid}`
    }
    out.push(item)
  }
  return out
}

/**
 * Fetch the TV Discover feed from apibay and return DiscoverItem[] (cat="tv").
 *
 * mode "newest" hits q.php?q=category:205 (newest first); any other mode hits the
 * precompiled top100 file and sorts by seeders. Results are cached in
 * listing_cache for {@link LIST_TTL} seconds. On a network failure this resolves
 * to [].
 */
export async function fetchTv(mode: DiscoverMode): Promise<DiscoverItem[]> {
  const url =
    mode === "newest"
      ? `${APIBAY}/q.php?q=category:${TV_CATEGORY}`
      : `${APIBAY}/precompiled/data_top100_${TV_CATEGORY}.json`
  const cacheKey = `tpb:tv:${mode}`

  if (isDbAvailable()) {
    const hit = await getCached<DiscoverItem[]>("listing_cache", cacheKey, LIST_TTL)
    if (hit) return hit
  }

  let raw: unknown
  try {
    raw = await httpJson<unknown>(url)
  } catch {
    return []
  }
  const items = parseTv(raw, mode)

  if (isDbAvailable()) {
    try {
      await setCached("listing_cache", cacheKey, items)
    } catch {
      // caching is best-effort; never fail the fetch over a write error.
    }
  }
  return items
}

// ----------------------------------------------------------------- seeders

/**
 * Parse a raw apibay q.php search response into Release[]. Pure (no network).
 * Skips rows with no info_hash, an all-zero hash, or the "No results returned"
 * sentinel; builds a magnet (with trackers) and a quality tag for each survivor.
 */
export function parseSeeders(arr: unknown): Release[] {
  const out: Release[] = []
  if (!Array.isArray(arr)) return out
  for (const t of arr as ApibayRow[]) {
    const ih = t?.info_hash ?? ""
    const name = t?.name ?? ""
    if (!ih || isZeroHash(ih) || name === "No results returned") continue
    out.push({
      name,
      source: "TPB",
      seeders: toInt(t.seeders),
      size: humanSize(toInt(t.size)),
      magnet: buildMagnet(ih, name),
      quality: quality(name),
    })
  }
  return out
}

/**
 * Free-text apibay search -> Release[] for the Download dialog / seed badge. On a
 * network failure this resolves to [].
 */
export async function seedersApibay(query: string): Promise<Release[]> {
  let raw: unknown
  try {
    raw = await httpJson<unknown>(`${APIBAY}/q.php?q=${pyQuote(query)}`)
  } catch {
    return []
  }
  return parseSeeders(raw)
}
