/**
 * javdb source — TypeScript port of the sidecar's javdb scraping
 * (sidecar/av_proxy.py: fetch_javdb, fetch_javdb_vr, seeders_javdb,
 * _jdb_link, jdb_api), rebuilt on the CAPTURED mobile API documented in
 * docs/javdb-api.md.
 *
 * This REPLACES three old sidecar hacks:
 *   - fetch_javdb (the /api/v1/rankings?type=movie&category=c guess),
 *   - the studio-label VR workaround (fetch_javdb_vr over /api/v2/search), and
 *   - seeders_javdb (which needed a JDB_CODE2ID side-table because the old
 *     rankings object exposed no usable magnet key).
 * The new API exposes a real tag browser (VR = tag id 212), a verified
 * "Most Viewed" rankings/playback endpoint, and slug-keyed detail/magnets, so
 * none of those hacks are needed.
 *
 * Every request is authenticated with the `jdsignature` header built by
 * signatureHeader() (src/api/javdb/signature.ts) and sent to JAVDB_API_HOST
 * with the Dart UA (JAVDB_UA). Covers come from tp.cmastd.com, which RENDERS
 * directly (per docs/javdb-api.md), so we set `cover` to the cover_url verbatim
 * — no per-code studio-cover resolution and no /img proxy needed.
 *
 * Mapping conventions (kept identical to the Python so DiscoverItem consumers
 * don't change):
 *   - cat = 'ad' (default) or 'vrc' (VR feeds / VR-detected titles),
 *   - title = code = the `number` field (e.g. "KAVR-508"),
 *   - cover = cover_url (tp.cmastd.com), ar = 1.48 (javdb jackets are wide),
 *   - seeders = magnets_count (javdb has no seeder counts; this is the magnet
 *     count, exactly as the Python emitted),
 *   - sub = date = release_date, year = release_date[:4],
 *   - runtime = duration, rating = 0,
 *   - id = slug, link = https://javdb.com/v/<slug>,
 *   - added = original feed position.
 */
import { httpJson } from "@/net/http"
import {
  JAVDB_API_HOST,
  JAVDB_UA,
  signatureHeader,
} from "@/api/javdb/signature"
import type { Cat, DiscoverItem, Release } from "@/api/types"
import { isVr } from "@/lib/codes"

// --------------------------------------------------------------- API envelope

/** Standard javdb response envelope: `{success, action, message, data}`. */
interface Envelope<T> {
  success: number
  action: string | null
  message: string | null
  data: T | null
}

/** A javdb movie object as it appears in listing/ranking/tag responses. */
export interface JavdbMovie {
  id: string
  number?: string
  title?: string
  origin_title?: string
  thumb_url?: string
  cover_url?: string
  duration?: number
  magnets_count?: number
  release_date?: string
  has_cnsub?: boolean
  can_play?: boolean
  score?: string
}

/** Wrapper carrying a `movies` array (rankings, playback, tags, latest). */
interface MoviesData {
  movies?: JavdbMovie[]
  current_page?: number
}

/** A raw magnet row from `/api/v1/movies/<slug>/magnets`. */
export interface JavdbMagnet {
  name?: string
  hash?: string
  /** Size in MEGABYTES (javdb reports MB, not bytes). */
  size?: number
  cnsub?: boolean
  hd?: boolean
  files_count?: number
  created_at?: string
}

interface MagnetsData {
  magnets?: JavdbMagnet[]
}

/** The full detail payload from `/api/v4/movies/<slug>`. */
export interface JavdbDetail {
  share_info?: string
  show_vip_banner?: boolean
  movie?: JavdbMovie & {
    summary?: string | null
    score?: string
    reviews_count?: number
    comments_count?: number
    want_watch_count?: number
    watched_count?: number
    maker_name?: string | null
    director_name?: string | null
    publisher_name?: string | null
    series_name?: string | null
    tags?: { id: string; name: string }[] | null
    actors?: { id: string; name: string }[] | null
  }
}

/** One tag inside a taxonomy group. */
export interface JavdbTag {
  id: string
  name: string
}

/** One taxonomy group (main/year/month/.../category/duration). */
export interface JavdbTagGroup {
  category: string
  category_id: string
  tags: JavdbTag[]
}

interface TaxonomyData {
  tags?: JavdbTagGroup[]
}

// --------------------------------------------------------------- constants

/** VR is category tag id 212 (docs/javdb-api.md). */
export const VR_TAG_ID = "212"

/** Default browse `filter_by` (all titles). */
const FILTER_ALL = "0:t:m::::"

/** javdb ranking windows. */
export type JavdbPeriod = "daily" | "weekly" | "monthly"

/** Magnet trackers appended to every built magnet link (mirrors TRACKERS in av_proxy.py). */
const TRACKERS = [
  "udp://tracker.opentrackr.org:1337/announce",
  "udp://open.demonii.com:1337/announce",
  "udp://tracker.openbittorrent.com:6969/announce",
  "udp://exodus.desync.com:6969/announce",
]
  .map((t) => "&tr=" + encodeURIComponent(t))
  .join("")

// --------------------------------------------------------------- low-level fetch

/**
 * GET a signed javdb API path and return the parsed `data` payload (or null on
 * an error envelope / network failure). Mirrors the Python `jdb_api()` which
 * swallowed every error and returned None. Each call mints a FRESH signature
 * (the server validates the embedded timestamp), and sends the Dart UA.
 */
export async function javdbApi<T>(path: string): Promise<T | null> {
  try {
    const env = await httpJson<Envelope<T>>(
      `https://${JAVDB_API_HOST}${path}`,
      {
        userAgent: JAVDB_UA,
        headers: {
          "accept-language": "en",
          jdsignature: signatureHeader(),
        },
      }
    )
    if (!env || env.success !== 1) return null
    return env.data ?? null
  } catch {
    return null
  }
}

// --------------------------------------------------------------- helpers

/** Port of human_size(b): bytes -> "7.6 GB". */
export function humanSize(bytes: number): string {
  let b = Number(bytes) || 0
  for (const u of ["B", "KB", "MB", "GB", "TB"]) {
    if (b < 1024) return u === "B" ? `${Math.floor(b)} ${u}` : `${b.toFixed(1)} ${u}`
    b /= 1024
  }
  return `${b.toFixed(1)} PB`
}

/** Port of _magnet(ih, name): build a magnet URI with the tracker tail. */
function buildMagnet(infohash: string, name: string): string {
  return `magnet:?xt=urn:btih:${infohash}&dn=${encodeURIComponent(name || "")}${TRACKERS}`
}

/** Port of _quality(name): pick a resolution token from a name, "" if none. */
function quality(name: string): string {
  const n = (name || "").toLowerCase()
  for (const q of ["2160p", "4k", "8k", "1080p", "720p", "480p"]) {
    if (n.includes(q)) return q.toUpperCase()
  }
  return ""
}

/** The public web permalink for a slug (Python _jdb_link). */
export function javdbLink(slug: string): string {
  return slug ? `https://javdb.com/v/${slug}` : ""
}

/**
 * Map one javdb movie object to a DiscoverItem. Items with no cover are
 * returned with cover "" (the Python dropped coverless items downstream; we
 * keep the mapping pure and let callers filter — discover() filters here).
 *
 * `cat` is forced to 'vrc' when the title/code looks like VR (isVr), unless the
 * caller already pinned it to 'vrc'. `added` is the feed position.
 */
export function toDiscoverItem(
  m: JavdbMovie,
  cat: Cat,
  index: number
): DiscoverItem {
  const code = (m.number || "").trim()
  const date = m.release_date || ""
  const cover = m.cover_url || m.thumb_url || ""
  // Promote to vrc when a VR title slips into an 'ad' feed (defensive parity
  // with the Python is_vr check used in fetch_javdb_vr). A caller-pinned 'vrc'
  // always stays 'vrc'.
  const resolvedCat: Cat =
    cat === "vrc" || isVr(m.title || m.origin_title || "", code) ? "vrc" : cat
  return {
    id: m.id,
    cat: resolvedCat,
    title: code,
    sub: date,
    cover,
    ar: 1.48,
    seeders: m.magnets_count ?? 0,
    size: "",
    src: "javdb",
    state: "new",
    year: date.slice(0, 4),
    runtime: m.duration ?? 0,
    rating: 0,
    code,
    date,
    added: index,
    link: javdbLink(m.id),
  }
}

/** Map a movies array to DiscoverItem[], dropping coverless rows (Python parity). */
function mapMovies(movies: JavdbMovie[], cat: Cat): DiscoverItem[] {
  const out: DiscoverItem[] = []
  for (const m of movies) {
    if (!(m.cover_url || m.thumb_url)) continue
    out.push(toDiscoverItem(m, cat, out.length))
  }
  return out
}

// --------------------------------------------------------------- public feeds

/**
 * Rankings — `/api/v1/rankings?type=<type>&period=<period>`.
 * `type` selects the catalog (0 = censored, others = uncensored/western per the
 * captured map); `period` = daily|weekly|monthly. Returns DiscoverItem[].
 */
export async function javdbRankings(
  type: number = 0,
  period: JavdbPeriod = "daily"
): Promise<DiscoverItem[]> {
  const data = await javdbApi<MoviesData>(
    `/api/v1/rankings?type=${type}&period=${period}`
  )
  return mapMovies(data?.movies ?? [], "ad")
}

/**
 * MOST VIEWED — `/api/v1/rankings/playback?filter_by=<filterBy>&period=<period>`
 * (the app's "HotWatching" / most-played list, VERIFIED working).
 * `filterBy` = all|high_score; `period` = daily|weekly|monthly.
 */
export async function javdbPlayback(
  filterBy: "all" | "high_score" = "all",
  period: JavdbPeriod = "daily"
): Promise<DiscoverItem[]> {
  const data = await javdbApi<MoviesData>(
    `/api/v1/rankings/playback?filter_by=${filterBy}&period=${period}`
  )
  return mapMovies(data?.movies ?? [], "ad")
}

/** Options for the filtered movie browser (`/api/v1/movies/tags`). */
export interface JavdbTagsOpts {
  /** Category tag id placed in the 4th `filter_by` field (VR = "212"). */
  tagId?: string
  /** Actor slug — builds `filter_by=0:a:<slug>` instead of the tag form. */
  actorSlug?: string
  /** release|update|… (default "release"). */
  sortBy?: string
  /** desc|asc (default "desc"). */
  orderBy?: "desc" | "asc"
  /** 1-based page (default 1). */
  page?: number
  /** Page size (default 24). */
  limit?: number
}

/**
 * Filtered movie browser — `/api/v1/movies/tags`. Builds the colon-delimited
 * `filter_by` selector per docs/javdb-api.md:
 *   - actor:  `0:a:<actorSlug>`
 *   - tag:    `0:t:m:<tagId>:::`  (e.g. VR = tag id "212")
 *   - default: `0:t:m::::`
 * The category is 'vrc' when browsing the VR tag (212), else 'ad'.
 */
export async function javdbTags(opts: JavdbTagsOpts = {}): Promise<DiscoverItem[]> {
  const {
    tagId,
    actorSlug,
    sortBy = "release",
    orderBy = "desc",
    page = 1,
    limit = 24,
  } = opts
  let filterBy: string
  if (actorSlug) filterBy = `0:a:${actorSlug}`
  else if (tagId) filterBy = `0:t:m:${tagId}:::`
  else filterBy = FILTER_ALL
  const cat: Cat = tagId === VR_TAG_ID ? "vrc" : "ad"
  const data = await javdbApi<MoviesData>(
    `/api/v1/movies/tags?filter_by=${encodeURIComponent(filterBy)}` +
      `&filter_by_tags=&sort_by=${sortBy}&order_by=${orderBy}&page=${page}&limit=${limit}`
  )
  return mapMovies(data?.movies ?? [], cat)
}

/** Options for the newest-releases feed (`/api/v1/movies/latest`). */
export interface JavdbLatestOpts {
  type?: string
  filterBy?: string
  sortBy?: string
  page?: number
  limit?: number
}

/**
 * Newest releases — `/api/v1/movies/latest`. Defaults mirror the captured call
 * (`type=all&filter_by=can_play&sort_by=update&page=1&limit=9`).
 */
export async function javdbLatest(opts: JavdbLatestOpts = {}): Promise<DiscoverItem[]> {
  const {
    type = "all",
    filterBy = "can_play",
    sortBy = "update",
    page = 1,
    limit = 9,
  } = opts
  const data = await javdbApi<MoviesData>(
    `/api/v1/movies/latest?type=${type}&filter_by=${filterBy}` +
      `&sort_by=${sortBy}&page=${page}&limit=${limit}`
  )
  return mapMovies(data?.movies ?? [], "ad")
}

/** Options for the TOP250 feed (`/api/v1/movies/top`). */
export interface JavdbTopOpts {
  startRank?: number
  type?: string
  typeValue?: string
  ignoreWatched?: boolean
  page?: number
  limit?: number
}

/**
 * TOP250 — `/api/v1/movies/top`. NOTE: the captured server rejected this
 * endpoint with `JWTVerificationError` ("Invalid Signature") even with a fresh
 * unauthenticated signature, so it appears to require a logged-in JWT (like
 * `/api/v1/lists`). javdbApi() returns null on that envelope, so this resolves
 * to `[]` until account auth is added; the call is kept for completeness/parity.
 */
export async function javdbTop(opts: JavdbTopOpts = {}): Promise<DiscoverItem[]> {
  const {
    startRank = 1,
    type = "all",
    typeValue = "",
    ignoreWatched = false,
    page = 1,
    limit = 25,
  } = opts
  const data = await javdbApi<MoviesData>(
    `/api/v1/movies/top?start_rank=${startRank}&type=${type}&type_value=${typeValue}` +
      `&ignore_watched=${ignoreWatched}&page=${page}&limit=${limit}`
  )
  return mapMovies(data?.movies ?? [], "ad")
}

/** Full detail for a slug — `/api/v4/movies/<slug>?from_rankings=false`. */
export async function javdbDetail(slug: string): Promise<JavdbDetail | null> {
  if (!slug) return null
  return javdbApi<JavdbDetail>(`/api/v4/movies/${slug}?from_rankings=false`)
}

/** Raw movie hit from `/api/v2/search`. */
interface JavdbSearchData {
  movies?: { id?: string; number?: string }[]
}

/**
 * Search the mobile API by printed code (or keyword) and return the matching
 * movie's slug — `/api/v2/search?q=<q>&type=movie`. Prefers an exact `number`
 * match (case-insensitive), else the first hit. "" when nothing is found. Used
 * to fetch a title's Japanese cast/title via {@link javdbDetail}.
 */
export async function javdbSearch(query: string): Promise<string> {
  const q = (query || "").trim()
  if (!q) return ""
  const data = await javdbApi<JavdbSearchData>(
    `/api/v2/search?q=${encodeURIComponent(q)}&type=movie`
  )
  const movies = data?.movies ?? []
  const want = q.toUpperCase()
  const hit = movies.find((m) => (m.number || "").toUpperCase() === want) ?? movies[0]
  return hit?.id ?? ""
}

/**
 * Magnets for a slug — `/api/v1/movies/<slug>/magnets` -> Release[].
 * Replaces seeders_javdb(): javdb reports `size` in MEGABYTES and has no seeder
 * counts, so seeders is 0 and size is human_size(size * 1MiB), exactly as the
 * Python emitted. quality is "HD" when the magnet flags hd, else parsed from the
 * name.
 */
export async function javdbMagnets(slug: string): Promise<Release[]> {
  if (!slug) return []
  const data = await javdbApi<MagnetsData>(`/api/v1/movies/${slug}/magnets`)
  const out: Release[] = []
  for (const m of data?.magnets ?? []) {
    const ih = m.hash || ""
    if (!ih) continue
    const name = m.name || slug
    const size = humanSize((m.size ?? 0) * 1048576)
    out.push({
      name,
      source: "javdb",
      seeders: 0,
      size,
      magnet: buildMagnet(ih, name),
      quality: m.hd ? "HD" : quality(name),
    })
  }
  return out
}

/** Tag taxonomy — `/api/v2/tags?type=0` -> the grouped tag list (main/year/…/category). */
export async function javdbTagsTaxonomy(): Promise<JavdbTagGroup[]> {
  const data = await javdbApi<TaxonomyData>(`/api/v2/tags?type=0`)
  return data?.tags ?? []
}

// --------------------------------------------------------------- discover helper

/**
 * Map a Discover catalog (cat, list) selection to the right javdb call.
 *
 *   ad:  weekly | monthly | daily -> playback (Most Viewed) for that window
 *        most_viewed                -> playback all/daily
 *   vrc: anything                   -> the VR tag browser (tag 212)
 *
 * 'ad' lists use the VERIFIED rankings/playback ("Most Viewed") endpoint; the
 * window is taken from the list id (weekly/monthly/daily). 'vrc' uses the real
 * tag browser (filter_by=0:t:m:212:::), replacing the old studio-label search
 * workaround. Coverless rows are already dropped by the mappers.
 */
export async function discover(cat: Cat, list: string): Promise<DiscoverItem[]> {
  if (cat === "vrc") {
    return javdbTags({ tagId: VR_TAG_ID, sortBy: "release", orderBy: "desc", limit: 24 })
  }
  // 'ad' (and any other cat): the Most Viewed list, windowed by the list id.
  const period: JavdbPeriod =
    list === "weekly" || list === "monthly" || list === "daily" ? list : "daily"
  return javdbPlayback("all", period)
}
