/**
 * YTS movie source.
 *
 * YTS exposes a stable JSON API (`/api/v2/list_movies.json`) mirrored across a
 * handful of hosts. `fetchMovies(sort)` returns a Discover feed of cat="mov"
 * cards (trending / newest / top_rated / most_seeded), and `seedersYts` returns
 * the torrent releases for a single title (used by the download dialog / per-item
 * seed aggregation).
 *
 * Network goes through src/net/http.ts (Rust-backed fetch, bypasses CORS). The
 * parsers are pure so they can be unit-tested against a recorded fixture.
 */
import { httpJson } from "@/net/http"
import { getCached, setCached, isDbAvailable } from "@/state/db"
import type { DiscoverItem, Release } from "@/api/types"

// --------------------------------------------------------------- API shapes

/** One torrent row inside a YTS movie object (only the fields we use). */
export interface YtsTorrent {
  hash?: string
  /** Seeder count (named `seeds` in YTS, not `seeders`). */
  seeds?: number
  size?: string
  quality?: string
  type?: string
}

/** One movie object from `list_movies.json` (only the fields we use). */
export interface YtsMovie {
  id?: number | string
  title?: string
  title_long?: string
  year?: number | string
  runtime?: number
  rating?: number
  imdb_code?: string
  large_cover_image?: string
  /** YTS movie page URL; present in list_movies, captured into item.link. */
  url?: string
  torrents?: YtsTorrent[]
}

/** Top-level `list_movies.json` envelope. */
export interface YtsListResponse {
  status?: string
  data?: {
    movies?: YtsMovie[]
  }
}

// --------------------------------------------------------------- constants

/**
 * YTS API mirrors, tried in order until one returns a valid payload. yts.mx is
 * frequently geo/ISP-blocked, so it is last.
 */
export const YTS_BASES = [
  "https://yts.bz/api/v2/",
  "https://movies-api.accel.li/api/v2/",
  "https://yts.lt/api/v2/",
  "https://yts.mx/api/v2/",
] as const

/**
 * The `fetchMovies` sort id -> YTS `sort_by` query value. The legacy mode strings
 * "trending"/"newest" are also accepted for backward compatibility (see
 * {@link resolveSort}).
 */
const YTS_SORTS: Record<string, string> = {
  most_seeded: "seeds",
  trending: "download_count",
  newest: "date_added",
  top_rated: "rating",
}

/** Public sort ids accepted by {@link fetchMovies}. */
export type YtsSort =
  | "trending"
  | "newest"
  | "top_rated"
  | "most_seeded"
  | "seeds"
  | "download_count"
  | "date_added"
  | "rating"

/**
 * BitTorrent trackers appended to every synthesised magnet (YTS torrents carry
 * only an info-hash).
 */
// quotePlusName keeps the `/` in each tracker raw.
const TRACKERS = [
  "udp://tracker.opentrackr.org:1337/announce",
  "udp://open.demonii.com:1337/announce",
  "udp://tracker.openbittorrent.com:6969/announce",
  "udp://exodus.desync.com:6969/announce",
]
  .map((t) => "&tr=" + quotePlusName(t))
  .join("")

/** TTL for a cached movie listing (300s). */
const LIST_TTL_SEC = 300

/** YTS poster aspect ratio (width/height) used for every Discover card. */
const YTS_POSTER_AR = 0.675

// --------------------------------------------------------------- helpers

/**
 * Map a public sort id to a YTS `sort_by` value, accepting the legacy
 * "trending"/"newest" strings and defaulting unknown values to "download_count"
 * (newest -> "date_added").
 */
export function resolveSort(sort: string): string {
  return (
    YTS_SORTS[sort] ?? (sort === "newest" ? "date_added" : "download_count")
  )
}

/**
 * Build a magnet URI from an info-hash and a display name, with the shared
 * tracker list appended. `name` is URL-encoded keeping everything except
 * A-Z a-z 0-9 _.-~ and `/` raw.
 */
export function ytsMagnet(infoHash: string, name: string): string {
  return (
    "magnet:?xt=urn:btih:" +
    infoHash +
    "&dn=" +
    quotePlusName(name) +
    TRACKERS
  )
}

/**
 * URL-encode keeping the always-safe set (letters, digits, `_.-~`) plus `/` raw.
 *
 * `encodeURIComponent` needs two fix-ups to get there:
 *   - it encodes `/`            -> decode `%2F` back to `/`
 *   - it leaves `! * ' ( )` raw -> re-encode them
 * (`~` is left raw already, so it needs no fix-up.)
 */
function quotePlusName(s: string): string {
  return encodeURIComponent(s ?? "")
    .replace(/%2F/g, "/")
    .replace(/!/g, "%21")
    .replace(/\*/g, "%2A")
    .replace(/'/g, "%27")
    .replace(/\(/g, "%28")
    .replace(/\)/g, "%29")
}

// --------------------------------------------------------------- parsers (pure)

/**
 * Given the raw movie objects (already merged across pages), produce the Discover
 * cards.
 *
 * Field conventions:
 *   id        = "mov_<id>"
 *   seeders   = max seeds across torrents (0 if none)
 *   size      = first torrent's size string ("" if none)
 *   cover     = large_cover_image (raw URL; the UI proxies it via coverObjectUrl)
 *   ar        = 0.675  (YTS poster aspect ratio)
 *   sub       = "<year> · <h>h <mm>m"  when runtime>0, else "<year>"
 *   year      = the raw YTS year (a number)
 *   code      = imdb_code
 *   link      = movie page url (only when present)
 */
export function parseMovies(movies: YtsMovie[]): DiscoverItem[] {
  const out: DiscoverItem[] = []
  for (const movie of movies) {
    const torrents = movie.torrents ?? []
    const seeds = torrents.length
      ? Math.max(...torrents.map((t) => Math.trunc(Number(t.seeds) || 0)))
      : 0
    const size = (torrents.length ? torrents[0]!.size : "") || ""
    const runtimeMinutes = Math.trunc(Number(movie.runtime) || 0)
    const year = movie.year ?? ""
    const sub = runtimeMinutes
      ? `${year} · ${Math.trunc(runtimeMinutes / 60)}h ${pad2(
          runtimeMinutes % 60
        )}m`
      : String(year)
    const item: DiscoverItem = {
      id: `mov_${movie.id}`,
      cat: "mov",
      title: movie.title || movie.title_long || "",
      sub,
      cover: movie.large_cover_image || "",
      ar: YTS_POSTER_AR,
      seeders: seeds,
      size,
      src: "YTS",
      state: "new",
      year,
      runtime: runtimeMinutes,
      rating: movie.rating || 0,
      code: movie.imdb_code || "",
    }
    const url = (movie.url || "").trim() // YTS movie page url, present in list_movies
    if (url) item.link = url
    out.push(item)
  }
  return out
}

/** Zero-pad to two digits. */
function pad2(n: number): string {
  return String(n).padStart(2, "0")
}

/**
 * Given the raw movies returned for a `query_term` search and the optional `year`
 * filter, produce the torrent releases.
 *
 * Field conventions:
 *   name    = "<title_long|title> [<quality> <type>]" (trimmed)
 *   source  = "YTS"
 *   seeders = torrent.seeds
 *   size    = torrent.size string
 *   magnet  = magnet built from torrent.hash + the tracker list
 *   quality = torrent.quality
 * Movies whose year mismatches `year` are skipped; torrents without a hash are
 * skipped.
 */
export function parseSeeders(
  movies: YtsMovie[],
  title: string,
  year?: string | number
): Release[] {
  const out: Release[] = []
  for (const movie of movies) {
    if (year && movie.year && String(movie.year) !== String(year)) continue
    for (const t of movie.torrents ?? []) {
      const infoHash = t.hash || ""
      if (!infoHash) continue
      const releaseName = `${movie.title_long || title} [${t.quality || ""} ${
        t.type || ""
      }]`.trim()
      out.push({
        name: releaseName,
        source: "YTS",
        seeders: Math.trunc(Number(t.seeds) || 0),
        size: t.size || "",
        magnet: ytsMagnet(infoHash, releaseName),
        quality: t.quality || "",
      })
    }
  }
  return out
}

// --------------------------------------------------------------- network

/**
 * Fetch one page of `list_movies.json` across the mirror list, returning the
 * first valid (`status == "ok"` with movies) payload's movie array, or null if
 * every mirror failed.
 */
async function fetchMoviesPage(
  sortBy: string,
  page: number
): Promise<YtsMovie[] | null> {
  for (const base of YTS_BASES) {
    const url = `${base}list_movies.json?limit=50&page=${page}&sort_by=${sortBy}&order_by=desc`
    try {
      const j = await httpJson<YtsListResponse>(url)
      const movies = j?.data?.movies
      if (j?.status === "ok" && movies && movies.length > 0) return movies
    } catch {
      // A dead mirror / non-2xx / JSON parse error -> try the next mirror.
    }
  }
  return null
}

/**
 * Pull up to two pages (YTS caps `limit` at 50, so 2 pages -> up to 100 movies)
 * across the mirror list, then build the Discover cards. Returns [] when no mirror
 * served any movies.
 *
 * Results are cached in `listing_cache` (TTL = {@link LIST_TTL_SEC}) keyed by the
 * resolved sort. Caching is skipped (still works, just always live) when the DB is
 * unavailable.
 *
 * @param sort  one of trending | newest | top_rated | most_seeded (or a raw
 *              YTS sort_by value / legacy mode); see {@link resolveSort}.
 * @param fresh when true, bypass the cache and force a live refetch.
 */
export async function fetchMovies(
  sort: YtsSort | string,
  fresh = false
): Promise<DiscoverItem[]> {
  const sortBy = resolveSort(sort)
  const cacheKey = `yts:mov:${sortBy}`

  if (!fresh && isDbAvailable()) {
    const cached = await getCached<DiscoverItem[]>(
      "listing_cache",
      cacheKey,
      LIST_TTL_SEC
    )
    if (cached) return cached
  }

  const movies: YtsMovie[] = []
  for (const page of [1, 2]) {
    const got = await fetchMoviesPage(sortBy, page)
    if (!got) break
    movies.push(...got)
  }

  const items = movies.length ? parseMovies(movies) : []

  if (isDbAvailable()) {
    try {
      await setCached("listing_cache", cacheKey, items)
    } catch {
      // Best-effort cache write; a failure must not break the live result.
    }
  }
  return items
}

/**
 * Query the YTS API for the title and return the matching torrent releases.
 * Unlike {@link fetchMovies} this hits yts.mx directly (a single host) and is not
 * cached.
 *
 * Returns [] on any fetch/parse failure (the per-item seeder aggregation must
 * never throw).
 */
export async function seedersYts(
  title: string,
  year?: string | number
): Promise<Release[]> {
  const url =
    "https://yts.mx/api/v2/list_movies.json?limit=4&query_term=" +
    encodeURIComponent(title)
  let j: YtsListResponse | null = null
  try {
    j = await httpJson<YtsListResponse>(url)
  } catch {
    return []
  }
  const movies = j?.data?.movies ?? []
  return parseSeeders(movies, title, year)
}
