/**
 * TMDB source.
 *
 * TMDB is the movie/TV authority. The discover feeds (trending + the curated
 * lists) build DiscoverItem cards from poster_path; tmdbLookup() resolves a
 * title+year to full metadata (poster/runtime/genre/cast/overview) for a
 * title-addressable file.
 *
 * The API key lives in the SQLite store under provider key "tmdb"
 * (db.getKey("tmdb")). Every public entry point returns [] / null (not an error)
 * when no key is configured. Network calls go through src/net/http.ts.
 *
 * Posters are 2:3 (ar 0.667) and built from TMDB_IMG + poster_path. Item links
 * point at themoviedb.org/<movie|tv>/<id>.
 */
import { httpJson } from "@/net/http"
import { getKey } from "@/state/db"
import type { Cat, DiscoverItem, TitleMeta } from "@/api/types"

/** Poster CDN base. w780 posters are 2:3. */
export const TMDB_IMG = "https://image.tmdb.org/t/p/w780"

/** Aspect ratio of a TMDB poster (w/h). */
const TMDB_AR = 0.667

const TMDB_API = "https://api.themoviedb.org/3"

/** Max number of pages walked per paged feed. */
const MAX_FEED_PAGES = 5

/** Years a match may differ from the requested year and still count. */
const YEAR_MATCH_TOLERANCE = 1

/** "movie" trending pulls the week window; "tv" pulls the day window. */
export type TmdbKind = "movie" | "tv"

// ------------------------------------------------------------------ raw shapes

/** One result row from a TMDB list/trending/search response. */
export interface TmdbResult {
  id?: number
  title?: string
  name?: string
  original_title?: string
  original_name?: string
  poster_path?: string | null
  release_date?: string
  first_air_date?: string
  vote_average?: number
}

/** A paged list/trending/search envelope. */
export interface TmdbListResponse {
  results?: TmdbResult[]
  total_pages?: number
}

/** A genre entry from a detail response. */
interface TmdbGenre {
  name?: string
}

/** A cast entry from the appended credits. */
interface TmdbCastMember {
  name?: string
}

/** Detail response (with append_to_response=credits). */
export interface TmdbDetail {
  id?: number
  title?: string
  name?: string
  poster_path?: string | null
  release_date?: string
  first_air_date?: string
  runtime?: number
  episode_run_time?: number[]
  genres?: TmdbGenre[]
  credits?: { cast?: TmdbCastMember[] }
  overview?: string
}

// ------------------------------------------------------------------ key + fetch

/**
 * Resolve the configured TMDB api key from the SQLite store under provider
 * "tmdb". Returns "" when unset or when the DB is unavailable.
 */
export async function tmdbKey(): Promise<string> {
  try {
    const v = await getKey("tmdb")
    return (v ?? "").trim()
  } catch {
    // DB unavailable (non-Tauri host); behave as "no key".
    return ""
  }
}

/**
 * GET api.themoviedb.org/3/<path> with the api key appended, returning the parsed
 * JSON or null on any failure. Returns null when no key is configured.
 */
async function tmdbGet<T>(
  path: string,
  params: Record<string, string | number> = {}
): Promise<T | null> {
  const key = await tmdbKey()
  if (!key) return null
  const qs = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) qs.set(k, String(v))
  qs.set("api_key", key)
  const url = `${TMDB_API}/${path}?${qs.toString()}`
  try {
    return await httpJson<T>(url)
  } catch {
    return null
  }
}

// ------------------------------------------------------------------ discover feeds

/** Round to one decimal. */
function round1(n: number): number {
  return Math.round(n * 10) / 10
}

/**
 * Shared card builder for trending + list feeds. `idPrefix` is "tmdbt_"
 * (trending) or "tmdbp_" (list); `cat` is the DiscoverItem category and also
 * decides the link kind (mov -> movie, else tv). `position` becomes `added` (the
 * running index in the de-duped output).
 */
export function buildItem(
  m: TmdbResult,
  poster: string,
  cat: Cat,
  idPrefix: string,
  position: number
): DiscoverItem {
  const title = m.title || m.name || ""
  const date = m.release_date || m.first_air_date || ""
  const tkind = cat === "mov" ? "movie" : "tv"
  const id = m.id
  const item: DiscoverItem = {
    id: `${idPrefix}${id}`,
    cat,
    title,
    sub: date ? date.slice(0, 4) : "",
    cover: TMDB_IMG + poster,
    ar: TMDB_AR,
    seeders: 0,
    size: "",
    src: "TMDB",
    state: "new",
    year: date.slice(0, 4),
    runtime: 0,
    rating: round1(m.vote_average ?? 0),
    code: String(id),
    date,
    added: position,
  }
  if (id) item.link = `https://www.themoviedb.org/${tkind}/${id}`
  return item
}

/**
 * Pure parser for the discover feeds: turn an ordered list of fetched pages into
 * the de-duped DiscoverItem[]. De-dupes results by id across pages (TMDB repeats
 * titles across pages) and skips rows without a poster. `added` is the running
 * index in the de-duped output.
 */
export function parseFeedPages(
  pages: TmdbListResponse[],
  cat: Cat,
  idPrefix: string
): DiscoverItem[] {
  const out: DiscoverItem[] = []
  const seen = new Set<number>()
  for (const page of pages) {
    const results = page.results ?? []
    for (const result of results) {
      const poster = result.poster_path || ""
      if (!poster) continue
      const id = result.id
      if (id === undefined || seen.has(id)) continue
      seen.add(id)
      out.push(buildItem(result, poster, cat, idPrefix, out.length))
    }
  }
  return out
}

/**
 * Walk up to 5 pages of a paged TMDB endpoint, collecting raw pages (stop on an
 * empty page or once the last page is reached), then hand them to
 * {@link parseFeedPages}. `extra` adds fixed query params on every page (e.g. the
 * search `query`).
 */
async function collectPaged(
  path: string,
  cat: Cat,
  idPrefix: string,
  extra: Record<string, string | number> = {}
): Promise<DiscoverItem[]> {
  const pages: TmdbListResponse[] = []
  for (let page = 1; page <= MAX_FEED_PAGES; page++) {
    const pageResponse = (await tmdbGet<TmdbListResponse>(path, { ...extra, page })) ?? {}
    const results = pageResponse.results ?? []
    if (results.length === 0) break
    pages.push(pageResponse)
    if (page >= (pageResponse.total_pages ?? 1)) break
  }
  return parseFeedPages(pages, cat, idPrefix)
}

/**
 * Trending feed for movies (week window) or TV (day window). Cards use id prefix
 * "tmdbt_". Returns [] when no key.
 */
export async function fetchTmdbTrending(kind: TmdbKind): Promise<DiscoverItem[]> {
  if (!(await tmdbKey())) return []
  const win = kind === "movie" ? "week" : "day"
  const cat: Cat = kind === "movie" ? "mov" : "tv"
  return collectPaged(`trending/${kind}/${win}`, cat, "tmdbt_")
}

/**
 * Curated-list feed for one of the TMDB list paths (movie/popular,
 * movie/top_rated, movie/now_playing, movie/upcoming, tv/popular, tv/top_rated,
 * tv/on_the_air). Cards use id prefix "tmdbp_". Returns [] when no key.
 */
export async function fetchTmdbList(cat: Cat, path: string): Promise<DiscoverItem[]> {
  if (!(await tmdbKey())) return []
  return collectPaged(path, cat, "tmdbp_")
}

/**
 * Free-text title search for movies (cat "mov") or TV (cat "tv") via TMDB
 * search/{movie,tv}, ordered by TMDB relevance. Cards use id prefix "tmdbs_".
 * Returns [] for a blank query or when no key is configured.
 */
export async function searchTmdb(cat: Cat, query: string): Promise<DiscoverItem[]> {
  const q = query.trim()
  if (!q) return []
  if (!(await tmdbKey())) return []
  const kind = cat === "mov" ? "movie" : "tv"
  return collectPaged(`search/${kind}`, cat, "tmdbs_", { query: q })
}

/**
 * Map a list id to its kind-aware TMDB path. Returns "" for unknown lists
 * (trending is handled separately).
 */
export function tmdbPath(cat: Cat, lst: string): string {
  const kind = cat === "mov" ? "movie" : "tv"
  if (lst === "popular") return `${kind}/popular`
  if (lst === "top_rated") return `${kind}/top_rated`
  if (lst === "now_playing") return "movie/now_playing"
  if (lst === "upcoming") return "movie/upcoming"
  if (lst === "airing") return "tv/on_the_air"
  return ""
}

// ------------------------------------------------------------------ title lookup

/**
 * Lowercase and strip everything that is not a word char, keeping CJK. JS `\w` is
 * ASCII-only, so we use the Unicode-aware character class (drop spaces/punctuation,
 * keep letters/digits/CJK).
 */
function norm(s: string | undefined): string {
  // NFC first: macOS filenames are NFD-decomposed, so Japanese dakuten/
  // handakuten (゛゜) arrive as combining marks (\p{M}); the strip below would
  // drop them and turn e.g. "ゴ" into "コ", breaking the match against TMDB's
  // composed original_title. Composing first keeps them inside the base char.
  return (s || "").normalize("NFC").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "")
}

/** Normalized equality or substring either way. */
function titleMatch(a: string | undefined, b: string | undefined): boolean {
  const na = norm(a)
  const nb = norm(b)
  return Boolean(na) && Boolean(nb) && (na === nb || na.includes(nb) || nb.includes(na))
}

/** Pull the leading 4-digit year out of a TMDB date string ("" if absent). */
function yearOf(m: TmdbResult): string {
  return (m.release_date || m.first_air_date || "").slice(0, 4)
}

/**
 * Best-match picker, factored out of {@link tmdbLookup} for testing. Prefers a
 * row matching both title (any of its name fields) and year (within 1), else
 * falls back to the top result only if its title roughly matches. Returns null
 * when nothing qualifies.
 */
export function pickMatch(
  results: TmdbResult[],
  title: string,
  year: string
): TmdbResult | null {
  const titleNames = (m: TmdbResult) => [m.title, m.name, m.original_title, m.original_name]
  for (const m of results) {
    const dt = yearOf(m)
    const okYear =
      !year || (Boolean(dt) && Math.abs(Number(dt) - Number(year)) <= YEAR_MATCH_TOLERANCE)
    const okTitle = titleNames(m).some((x) => titleMatch(title, x))
    if (okYear && okTitle) return m
  }
  const top = results[0]
  if (!top) return null
  if (titleNames(top).some((x) => titleMatch(title, x))) return top
  // Year-anchored fallback: a precise title search whose top hit lands on the
  // requested year is trustworthy even when TMDB only stores a romanized name
  // that won't title-match the query (e.g. "パーフェクトブルー" is listed solely
  // as "PERFECT BLUE"). Guarded by the year so it can't grab an unrelated hit.
  if (year) {
    const dt = yearOf(top)
    if (dt && Math.abs(Number(dt) - Number(year)) <= YEAR_MATCH_TOLERANCE) return top
  }
  return null
}

/**
 * Metadata builder, factored out of {@link tmdbLookup} for testing. Combines the
 * picked search hit with its detail response into a TitleMeta, setting fields only
 * when present. Cover is TMDB_IMG + poster_path with ar 0.667; runtime is
 * "<n> min"; genre is up to 3 names; cast is up to 5 names.
 */
export function buildMeta(pick: TmdbResult, det: TmdbDetail): TitleMeta {
  const meta: TitleMeta = { tmdb_id: pick.id }

  const poster = det.poster_path || pick.poster_path || ""
  if (poster) {
    meta.cover = TMDB_IMG + poster
    meta.ar = TMDB_AR // TMDB posters are 2:3
  }

  const date =
    det.release_date ||
    det.first_air_date ||
    pick.release_date ||
    pick.first_air_date ||
    ""
  if (date) {
    meta.date = date.slice(0, 10)
    meta.year = date.slice(0, 4)
  }

  // movie runtime, else first tv episode_run_time entry
  const runtimeMinutes = det.runtime || (det.episode_run_time && det.episode_run_time[0]) || 0
  if (runtimeMinutes) meta.runtime = `${runtimeMinutes} min`

  const genres = (det.genres ?? [])
    .map((g) => g.name)
    .filter((n): n is string => Boolean(n))
  if (genres.length > 0) meta.genre = genres.slice(0, 3).join(", ")

  const cast = (det.credits?.cast ?? [])
    .slice(0, 5)
    .map((c) => c.name)
    .filter((n): n is string => Boolean(n))
  if (cast.length > 0) meta.cast = cast.join(", ")

  const titleText = det.title || det.name || ""
  if (titleText) meta.tmdb_title = titleText
  if (det.overview) meta.overview = det.overview

  return meta
}

/**
 * Search the TMDB catalog for title (+year guard), take the best match
 * (title+year preferred, else the top popularity hit if its title roughly
 * matches), pull full details with credits, and build the metadata record:
 * cover (+ar), date, year, runtime, genre (<=3), cast (<=5), tmdb_id, tmdb_title,
 * overview.
 *
 * Returns null when no key, no title, or no acceptable match.
 */
export async function tmdbLookup(
  title: string,
  year: string,
  tv = false
): Promise<TitleMeta | null> {
  if (!(await tmdbKey()) || !title) return null
  const kind = tv ? "tv" : "movie"
  const yearParam = tv ? "first_air_date_year" : "year"

  // attempts: (title+year) first when a year is given, then bare title.
  const attempts: Record<string, string>[] = []
  if (year) attempts.push({ query: title, [yearParam]: year })
  attempts.push({ query: title })

  let results: TmdbResult[] = []
  for (const params of attempts) {
    const searchResponse = await tmdbGet<TmdbListResponse>(`search/${kind}`, {
      include_adult: "false",
      ...params,
    })
    results = searchResponse?.results ?? []
    if (results.length > 0) break
  }
  if (results.length === 0) return null

  const pick = pickMatch(results, title, year)
  if (!pick) return null

  const detail =
    (await tmdbGet<TmdbDetail>(`${kind}/${pick.id}`, {
      append_to_response: "credits",
    })) ?? {}

  return buildMeta(pick, detail)
}
