/**
 * Metadata resolvers, two surfaces:
 *
 *   metaLookup({cid?, code?}) -> JavMeta
 *     Japanese title + date + runtime + Japanese cast from r18.dev (by FANZA
 *     content id), with a javdatabase fallback (by printed code). Keyed and
 *     cached by `cid || code`.
 *
 *   titleLookup(tv, title, year) -> TitleLookupResponse
 *     Title lookup via TMDB (tmdbLookup); for tv, falls back to an AniList cover
 *     when TMDB finds none.
 *
 * Both cache into the SQLite meta_cache table with a very long TTL (treated as
 * permanent). Caching is best-effort and gated on isDbAvailable(), so on a
 * non-Tauri host the resolvers just hit the network. Every helper swallows fetch
 * errors to null/{}.
 */
import { httpJson, httpText, DEFAULT_USER_AGENT } from "@/net/http"
import { getCached, setCached, isDbAvailable } from "@/state/db"
import { tmdbKey, tmdbLookup } from "@/api/sources/tmdb"
import { javdbDetail, javdbSearch } from "@/api/sources/javdb"
import { normalizeCodeNum } from "@/lib/codes"
import type { JavMeta, TitleLookupResponse, TitleMeta } from "@/api/types"

/**
 * Treat a cached meta/title record as permanent. One year is effectively forever
 * for this app's lifetime; the row is overwritten on a forced refetch.
 */
const META_TTL_SEC = 365 * 24 * 60 * 60

// ------------------------------------------------------------------ r18.dev (/meta primary)

/** One actress entry in an r18.dev combined detail response. */
interface R18Actress {
  name_kanji?: string
  name_kana?: string
  name_romaji?: string
}

/** The r18.dev `combined=<cid>/json` detail shape (only the fields /meta reads). */
export interface R18Combined {
  title_ja?: string
  release_date?: string | number
  runtime_mins?: string | number
  actresses?: R18Actress[]
}

/**
 * Build a JavMeta from an r18.dev combined detail: Japanese title, release date
 * (YYYY-MM-DD), runtime ("<n> min"), Japanese cast (kanji, then kana, then
 * romaji). Returns null when nothing useful is present.
 */
export function parseR18(j: R18Combined | null): JavMeta | null {
  if (!j || typeof j !== "object") return null
  const rec: JavMeta = {}
  if (j.title_ja) rec.jatitle = j.title_ja
  if (j.release_date) rec.date = String(j.release_date).slice(0, 10)
  if (j.runtime_mins) rec.runtime = `${j.runtime_mins} min`
  const acts = (j.actresses ?? [])
    .map((a) => a.name_kanji || a.name_kana || a.name_romaji)
    .filter((a): a is string => Boolean(a))
  if (acts.length > 0) rec.cast_ja = acts.join(", ")
  return Object.keys(rec).length > 0 ? rec : null
}

/** Network half of {@link parseR18}: GET the r18.dev combined detail JSON. */
export async function fromR18(cid: string): Promise<JavMeta | null> {
  let j: R18Combined | null = null
  try {
    j = await httpJson<R18Combined>(
      `https://r18.dev/videos/vod/movies/detail/-/combined=${cid}/json`,
      {
        referer: "https://r18.dev/",
        headers: { Accept: "application/json" },
      }
    )
  } catch {
    return null
  }
  return parseR18(j)
}

// ------------------------------------------------------------------ javdatabase (/meta fallback)

/**
 * From the javdatabase movie-page HTML and the printed code, pull the romanized
 * cast (from the <title>), the first date (YYYY-MM-DD) on the page, and runtime
 * ("<n> min"). Returns null when the HTML is too small (< 2000 chars) or nothing
 * is found.
 *
 * Gotcha: we take the first `\d{4}-\d{2}-\d{2}` and first `\d{2,3}\s*min`
 * verbatim, even though javdatabase's current markup can surface a page-metadata
 * date before the release date and spells runtime as "... minutes" (so the `min`
 * regex misses). Kept as-is for stable output.
 */
export function parseJavdb(html: string, code: string): JavMeta | null {
  if (!html || html.length < 2000) return null
  const rec: JavMeta = {}
  // <title> CODE - <cast> - JAV Database  (case-insensitive, non-greedy cast)
  const escaped = code.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
  const titleMatch = new RegExp(
    `<title>\\s*${escaped}\\s*-\\s*([\\s\\S]+?)\\s*-\\s*JAV Database`,
    "i"
  ).exec(html)
  if (titleMatch && !titleMatch[1].toLowerCase().includes("jav")) rec.cast = titleMatch[1].trim()
  const dateMatch = /(\d{4}-\d{2}-\d{2})/.exec(html)
  if (dateMatch) rec.date = dateMatch[1]
  const runtimeMatch = /(\d{2,3})\s*min/i.exec(html)
  if (runtimeMatch) rec.runtime = `${runtimeMatch[1]} min`
  // The page links the FANZA cover by its content id; capture it so metaLookup
  // can pull Japanese cast from r18.dev (javdatabase itself only has romaji).
  const cidMatch = /pics\.dmm\.co\.jp\/(?:digital\/video|mono\/movie)\/([a-z0-9_]+)\//i.exec(html)
  if (cidMatch) rec._cid = cidMatch[1]
  return Object.keys(rec).length > 0 ? rec : null
}

/**
 * Japanese cast/title from the javdb mobile API (search the printed code ->
 * slug -> detail). javdb stores Japanese actor names natively and covers
 * titles javdatabase/r18 miss (amateur 459TEN-…, VR labels, etc.), so this is
 * the most comprehensive Japanese source. Returns null on any miss.
 */
export async function fromJavdbApi(code: string): Promise<JavMeta | null> {
  if (!code) return null
  let slug = ""
  try {
    slug = await javdbSearch(code)
    if (!slug) {
      // over-padded on-disk codes (AJVR-00277) are indexed canonically (AJVR-277)
      const alt = normalizeCodeNum(code)
      if (alt && alt !== code) slug = await javdbSearch(alt)
    }
  } catch {
    return null
  }
  if (!slug) return null
  const detail = await javdbDetail(slug).catch(() => null)
  const movie = detail?.movie
  if (!movie) return null
  const rec: JavMeta = {}
  if (movie.title) rec.jatitle = movie.title
  const acts = (movie.actors ?? [])
    .map((a) => a.name)
    .filter((a): a is string => Boolean(a))
  if (acts.length > 0) rec.cast_ja = acts.join(", ")
  return Object.keys(rec).length > 0 ? rec : null
}

/** Network half of {@link parseJavdb}: GET the javdatabase movie page HTML. */
export async function fromJavdb(code: string): Promise<JavMeta | null> {
  let html = ""
  try {
    html = await httpText(
      `https://www.javdatabase.com/movies/${code.toLowerCase()}/`
    )
  } catch {
    return null
  }
  return parseJavdb(html, code)
}

// ------------------------------------------------------------------ /meta

/** Arguments for /meta (cat is accepted but unused). */
export interface MetaArgs {
  cid?: string
  code?: string
  /** Accepted for API parity; ignored. */
  cat?: string
}

/**
 * `key = cid || code`; with neither, returns `{}` (no lookup, not cached).
 * Otherwise returns the cached record if present, else resolves r18.dev (by FANZA
 * content id, preferred) then javdatabase (by printed code, fallback), caches
 * under `key`, and returns it. Always resolves to a JavMeta (possibly empty),
 * never throws.
 *
 * The empty `{}` result is cached for a real key, so a miss is not re-fetched.
 */
export async function metaLookup(args: MetaArgs): Promise<JavMeta> {
  const cid = (args.cid ?? "").trim()
  const code = (args.code ?? "").trim()
  const key = cid || code
  if (!key) return {}

  if (isDbAvailable()) {
    try {
      const hit = await getCached<JavMeta>("meta_cache", `meta:${key}`, META_TTL_SEC)
      if (hit) return hit
    } catch {
      // best-effort cache read
    }
  }

  // r18.dev (by FANZA cid) carries Japanese cast (cast_ja); javdatabase (by
  // code) only has romaji. Cover URLs are now blob: URLs, so a cid is rarely
  // passed in, and we want cast in Japanese everywhere. So when we don't already
  // have Japanese cast, fetch the javdatabase page (it also yields a FANZA cid
  // via parseJavdb._cid) and use that cid to pull Japanese cast from r18. Merge
  // order makes Japanese fields win, with romaji kept as a fallback.
  let rec: JavMeta = (cid ? await fromR18(cid) : null) ?? {}
  if (!rec.cast_ja && code) {
    const jdb = (await fromJavdb(code)) ?? {}
    const javdbCid = jdb._cid
    const r18ViaJdb = javdbCid ? (await fromR18(javdbCid)) ?? {} : {}
    rec = { ...jdb, ...rec, ...r18ViaJdb }
  }
  // Final Japanese-cast fallback: the javdb mobile API (native Japanese actor
  // names), which covers amateur/VR titles javdatabase & r18 miss.
  if (!rec.cast_ja && code) {
    const api = (await fromJavdbApi(code)) ?? {}
    rec = { ...rec, ...api }
  }
  delete rec._cid // internal hint, never persisted/returned

  if (isDbAvailable()) {
    try {
      await setCached("meta_cache", `meta:${key}`, rec)
    } catch {
      // best-effort cache write
    }
  }
  return rec
}

// ------------------------------------------------------------------ AniList (tv cover fallback)

/** Minimal AniList GraphQL response shape (only the cover image is read). */
interface AnilistResponse {
  data?: {
    Media?: {
      coverImage?: { large?: string; medium?: string }
    } | null
  } | null
}

/**
 * POST the AniList GraphQL API for an anime matching `title` and return its cover
 * image URL ("large", else "medium"). Returns "" on any failure.
 */
export async function anilistCover(title: string): Promise<string> {
  try {
    const body = JSON.stringify({
      query:
        "query($s:String){Media(search:$s,type:ANIME){coverImage{large medium}}}",
      variables: { s: title },
    })
    const j = await httpJson<AnilistResponse>("https://graphql.anilist.co", {
      method: "POST",
      body,
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      userAgent: DEFAULT_USER_AGENT,
    })
    const ci = j?.data?.Media?.coverImage ?? {}
    return ci.large || ci.medium || ""
  } catch {
    return ""
  }
}

// ------------------------------------------------------------------ /movie + /tv

/**
 * Movie/TV title lookup (`tv` selects which).
 *
 * Response shape `{ ok, haskey, meta? }`:
 *   - no TMDB key         -> { ok:false, haskey:false }            (no meta)
 *   - key but empty title -> { ok:false, haskey:true }             (no meta)
 *   - otherwise           -> { ok:<bool cover>, haskey:true, meta }
 *
 * `meta` is `tmdbLookup(title, year, tv) or {}`. For tv, when TMDB yields no
 * cover, an AniList cover (ar 0.69) is tried as a fallback (no key needed). `ok`
 * is true only when the final record carries a cover.
 *
 * Results are cached (best-effort, permanent TTL) under
 * `("tmdbtv:" | "tmdb:") + title.toLowerCase() + "|" + year`. `fresh` bypasses
 * the cache and overwrites it.
 */
export async function titleLookup(
  tv: boolean,
  title: string,
  year: string,
  opts: { fresh?: boolean } = {}
): Promise<TitleLookupResponse> {
  const trimmedTitle = (title ?? "").trim()
  const trimmedYear = (year ?? "").trim()

  if (!(await tmdbKey())) return { ok: false, haskey: false }
  if (!trimmedTitle) return { ok: false, haskey: true }

  const cacheKey = `${tv ? "tmdbtv:" : "tmdb:"}${trimmedTitle.toLowerCase()}|${trimmedYear}`
  const fresh = opts.fresh ?? false

  if (!fresh && isDbAvailable()) {
    try {
      const hit = await getCached<TitleMeta>("meta_cache", cacheKey, META_TTL_SEC)
      if (hit) {
        return { ok: Boolean(hit.cover), haskey: true, meta: hit }
      }
    } catch {
      // best-effort cache read
    }
  }

  // tmdbLookup(...) or {}: coerce a null lookup to an empty record.
  let rec: TitleMeta = (await tmdbLookup(trimmedTitle, trimmedYear, tv)) ?? {}

  // anime fallback: AniList cover when TMDB found none (tv only, no key needed).
  if (tv && !rec.cover) {
    const anilistCoverUrl = await anilistCover(trimmedTitle)
    if (anilistCoverUrl) rec = { cover: anilistCoverUrl, ar: 0.69 }
  }

  if (isDbAvailable()) {
    try {
      await setCached("meta_cache", cacheKey, rec)
    } catch {
      // best-effort cache write
    }
  }

  return { ok: Boolean(rec.cover), haskey: true, meta: rec }
}
