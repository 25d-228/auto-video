/**
 * javdb source, built on the mobile API documented in docs/javdb-api.md.
 *
 * The API exposes a tag browser (VR = tag id 212), a "Most Viewed"
 * rankings/playback endpoint, and slug-keyed detail/magnets.
 *
 * Every request is authenticated with the `jdsignature` header built by
 * signatureHeader() (src/api/javdb/signature.ts) and sent to JAVDB_API_HOST
 * with the Dart UA (JAVDB_UA). Covers come from tp.cmastd.com, which renders
 * directly, so we set `cover` to the cover_url verbatim (no per-code studio-cover
 * resolution).
 *
 * Mapping conventions:
 *   - cat = 'ad' (default) or 'vrc' (VR feeds / VR-detected titles),
 *   - title = code = the `number` field (e.g. "KAVR-508"),
 *   - cover = cover_url (tp.cmastd.com), ar = 1.48 (javdb jackets are wide),
 *   - seeders = magnets_count (javdb has no seeder counts; this is the magnet
 *     count),
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
import { quality } from "@/lib/quality"
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
  /** Size in megabytes (javdb reports MB, not bytes). */
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
    preview_images?: { thumb_url?: string; large_url?: string }[] | null
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

/** Bytes per MiB; javdb reports magnet sizes in MB, scaled to bytes for humanSize. */
const BYTES_PER_MIB = 1024 * 1024

/** Aspect ratio for javdb jackets (wide). */
const JAVDB_JACKET_AR = 1.48

/** javdb ranking windows. */
export type JavdbPeriod = "daily" | "weekly" | "monthly"

/** Magnet trackers appended to every built magnet link. */
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
 * an error envelope / network failure). Each call mints a fresh signature (the
 * server validates the embedded timestamp), and sends the Dart UA.
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

/** Bytes -> "7.6 GB". */
export function humanSize(bytes: number): string {
  let b = Number(bytes) || 0
  for (const u of ["B", "KB", "MB", "GB", "TB"]) {
    if (b < 1024) return u === "B" ? `${Math.floor(b)} ${u}` : `${b.toFixed(1)} ${u}`
    b /= 1024
  }
  return `${b.toFixed(1)} PB`
}

/** Build a magnet URI with the tracker tail. */
function buildMagnet(infohash: string, name: string): string {
  return `magnet:?xt=urn:btih:${infohash}&dn=${encodeURIComponent(name || "")}${TRACKERS}`
}

/** The public web permalink for a slug. */
export function javdbLink(slug: string): string {
  return slug ? `https://javdb.com/v/${slug}` : ""
}

/**
 * Map one javdb movie object to a DiscoverItem. Items with no cover are returned
 * with cover "" (the mapping stays pure; callers filter, and discover() does).
 *
 * `cat` is forced to 'vrc' when the title/code looks like VR (isVr), unless the
 * caller already pinned it to 'vrc'. `added` is the feed position.
 */
export function toDiscoverItem(
  movie: JavdbMovie,
  cat: Cat,
  index: number
): DiscoverItem {
  const code = (movie.number || "").trim()
  const date = movie.release_date || ""
  const cover = movie.cover_url || movie.thumb_url || ""
  // Promote to vrc when a VR title slips into an 'ad' feed. A caller-pinned 'vrc'
  // always stays 'vrc'.
  const resolvedCat: Cat =
    cat === "vrc" || isVr(movie.title || movie.origin_title || "", code) ? "vrc" : cat
  return {
    id: movie.id,
    cat: resolvedCat,
    title: code,
    sub: date,
    cover,
    ar: JAVDB_JACKET_AR,
    seeders: movie.magnets_count ?? 0,
    size: "",
    src: "javdb",
    state: "new",
    year: date.slice(0, 4),
    runtime: movie.duration ?? 0,
    rating: 0,
    code,
    date,
    added: index,
    link: javdbLink(movie.id),
  }
}

/** Map a movies array to DiscoverItem[], dropping coverless rows. */
function mapMovies(movies: JavdbMovie[], cat: Cat): DiscoverItem[] {
  const out: DiscoverItem[] = []
  for (const movie of movies) {
    if (!(movie.cover_url || movie.thumb_url)) continue
    out.push(toDiscoverItem(movie, cat, out.length))
  }
  return out
}

// --------------------------------------------------------------- public feeds

/**
 * Rankings: `/api/v1/rankings?type=<type>&period=<period>`.
 * `type` selects the catalog (0 = censored, others = uncensored/western);
 * `period` = daily|weekly|monthly.
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
 * Most viewed: `/api/v1/rankings/playback?filter_by=<filterBy>&period=<period>`
 * (the app's "HotWatching" / most-played list).
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
  /** Category tag id placed in the genre `filter_by` field (VR = "212"). */
  tagId?: string
  /** Year tag ("2024") for the year `filter_by` field; "" / undefined = all years. */
  year?: string
  /** Month tag ("6") for the month `filter_by` field; "" / undefined = all months. */
  month?: string
  /** Actor slug; builds `filter_by=0:a:<slug>` instead of the tag form. */
  actorSlug?: string
  /** release|update|score|hit|want_watch_count|watched_count (default "release"). */
  sortBy?: string
  /** desc|asc (default "desc"). */
  orderBy?: "desc" | "asc"
  /** 1-based page (default 1). */
  page?: number
  /** Page size (default 24). */
  limit?: number
}

/**
 * Filtered movie browser: `/api/v1/movies/tags`. Builds the colon-delimited
 * `filter_by` selector per docs/javdb-api.md (7 fields
 * `0:t:m:<genre>:<year>::<month>`):
 *   - actor:  `0:a:<actorSlug>`
 *   - tag:    `0:t:m:<tagId>:<year>::<month>`  (genre=field 4, year=field 5,
 *             month=field 7; e.g. VR 2024 June = `0:t:m:212:2024::6`)
 * An empty genre field (no tagId) means all genres incl. VR; with a year/month
 * that's the "all titles in that window" feed the Adult→Category browser subtracts
 * the VR set from; with no year/month it's the plain `0:t:m::::`. The category is
 * 'vrc' when browsing the VR tag (212), else 'ad'.
 */
export async function javdbTags(opts: JavdbTagsOpts = {}): Promise<DiscoverItem[]> {
  const {
    tagId,
    year = "",
    month = "",
    actorSlug,
    sortBy = "release",
    orderBy = "desc",
    page = 1,
    limit = 24,
  } = opts
  let filterBy: string
  if (actorSlug) filterBy = `0:a:${actorSlug}`
  // Empty genre (tagId falsy) -> all genres; with year/month set this is the
  // "all titles in that window" feed. All-empty collapses to `0:t:m::::`.
  else filterBy = `0:t:m:${tagId ?? ""}:${year}::${month}`
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
 * Newest releases: `/api/v1/movies/latest`. Defaults:
 * `type=all&filter_by=can_play&sort_by=update&page=1&limit=9`.
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
 * TOP250: `/api/v1/movies/top`. The server rejects this endpoint with
 * `JWTVerificationError` ("Invalid Signature") even with a fresh unauthenticated
 * signature, so it appears to require a logged-in JWT (like `/api/v1/lists`).
 * javdbApi() returns null on that envelope, so this resolves to `[]` until account
 * auth is added; the call is kept for completeness.
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

/** Full detail for a slug: `/api/v4/movies/<slug>?from_rankings=false`. */
export async function javdbDetail(slug: string): Promise<JavdbDetail | null> {
  if (!slug) return null
  return javdbApi<JavdbDetail>(`/api/v4/movies/${slug}?from_rankings=false`)
}

/**
 * Sample/preview images for a slug (raw tp.cmastd.com URLs, large variant) from
 * the detail payload's `preview_images`. cmastd is single-byte-XOR encrypted, so
 * callers route these through coverObjectUrl to decode + display.
 */
export async function javdbPreviews(slug: string): Promise<string[]> {
  const detail = await javdbDetail(slug)
  const imgs = detail?.movie?.preview_images ?? []
  return imgs.map((i) => i.large_url || i.thumb_url || "").filter(Boolean)
}

/** Raw movie hit from `/api/v2/search`. */
interface JavdbSearchData {
  movies?: { id?: string; number?: string }[]
}

/**
 * Search the mobile API by printed code (or keyword) and return the matching
 * movie's slug (`/api/v2/search?q=<q>&type=movie`). Prefers an exact `number`
 * match (case-insensitive), else the first hit. "" when nothing is found. Used to
 * fetch a title's Japanese cast/title via {@link javdbDetail}.
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
 * Magnets for a slug: `/api/v1/movies/<slug>/magnets` -> Release[].
 * javdb reports `size` in megabytes and has no seeder counts, so seeders is 0 and
 * size is humanSize(size * 1MiB). quality is "HD" when the magnet flags hd, else
 * parsed from the name.
 */
export async function javdbMagnets(slug: string): Promise<Release[]> {
  if (!slug) return []
  const data = await javdbApi<MagnetsData>(`/api/v1/movies/${slug}/magnets`)
  const out: Release[] = []
  for (const m of data?.magnets ?? []) {
    const ih = m.hash || ""
    if (!ih) continue
    const name = m.name || slug
    const size = humanSize((m.size ?? 0) * BYTES_PER_MIB)
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

/** Tag taxonomy: `/api/v2/tags?type=0` -> the grouped tag list (main/year/…/category). */
export async function javdbTagsTaxonomy(): Promise<JavdbTagGroup[]> {
  const data = await javdbApi<TaxonomyData>(`/api/v2/tags?type=0`)
  return data?.tags ?? []
}

// --------------------------------------------------------------- discover helper

/** Extra browser controls threaded from the Discover toolbar (year/month/sort). */
export interface JavdbDiscoverOpts {
  /** Year tag ("2024"); "" = all years. */
  year?: string
  /** Month tag ("6"); "" = all months. */
  month?: string
  /** sort_by token (release|update|score|hit|want_watch_count|watched_count). */
  sortBy?: string
  /** desc|asc (release only). */
  orderBy?: "desc" | "asc"
  /** "category" = the Adult year/month browser (all titles minus the VR set). */
  mode?: string
}

/**
 * Page through the filtered browser. The tags endpoint hard-caps at 50 items
 * per page (the `limit` param is ignored above 50), so fill the grid with 2
 * pages (~100). Stops early at the last page.
 */
async function javdbBrowse(
  opts: JavdbTagsOpts,
  pages = 2
): Promise<DiscoverItem[]> {
  const out: DiscoverItem[] = []
  for (let page = 1; page <= pages; page++) {
    const items = await javdbTags({ ...opts, page, limit: 50 })
    if (items.length === 0) break // past the last page
    out.push(...items)
  }
  return out
}

/**
 * Map a Discover catalog (cat, list) selection to the right javdb call.
 *
 *   vrc:           the Categories→Censored browser with Genre=VR (tag 212),
 *                  filtered by the chosen year/month and ordered by the sort.
 *   ad + category: all titles for the chosen year/month (empty genre) minus the
 *                  VR set for the same window. javdb has no "exclude VR" filter,
 *                  so the censored 2D feed is all − vr (by movie id).
 *   ad + ranking:  the Censored ranking for that window (`/api/v1/rankings?type=0`,
 *                  daily | weekly | monthly).
 *
 * Coverless rows are already dropped by the mappers.
 */
export async function discover(
  cat: Cat,
  list: string,
  opts: JavdbDiscoverOpts = {}
): Promise<DiscoverItem[]> {
  const browse = {
    year: opts.year,
    month: opts.month,
    sortBy: opts.sortBy ?? "release",
    orderBy: opts.orderBy ?? "desc",
  }
  if (cat === "vrc") {
    return javdbBrowse({ tagId: VR_TAG_ID, ...browse })
  }
  if (opts.mode === "category") {
    // Fetch the whole window (empty genre = all titles incl. VR) and the VR-only
    // set for the same window, then subtract the VR ids. Equal page counts make
    // the subtraction exact: any VR title within the top-N of "all" is necessarily
    // within the top-N of "VR" (VR ⊆ all, identical ordering).
    const [all, vr] = await Promise.all([
      javdbBrowse(browse),
      javdbBrowse({ tagId: VR_TAG_ID, ...browse }),
    ])
    const vrIds = new Set(vr.map((i) => i.id))
    // Drop the authoritative tag-212 set by id, and anything the title/code
    // heuristic already flagged VR (toDiscoverItem promotes those to cat 'vrc').
    // The cat guard keeps the feed strictly non-VR even if tag-212 missed a title,
    // and degrades gracefully if the VR sub-fetch transiently fails (most VR codes
    // carry a VR label, so isVr still catches them).
    return all.filter((i) => !vrIds.has(i.id) && i.cat !== "vrc")
  }
  const period: JavdbPeriod =
    list === "weekly" || list === "monthly" || list === "daily" ? list : "daily"
  return javdbRankings(0, period)
}
