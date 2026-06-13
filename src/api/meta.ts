/**
 * Metadata resolvers — TypeScript port of the Python sidecar's /meta and
 * /movie//tv handlers and their helpers (from_r18, from_javdb, tmdb_lookup,
 * anilist_cover) in sidecar/av_proxy.py.
 *
 * Two surfaces, mirroring the sidecar HTTP endpoints:
 *
 *   metaLookup({cid?, code?})  -> JavMeta            (the /meta endpoint)
 *     Japanese title + date + runtime + Japanese cast from r18.dev (by FANZA
 *     content id) with a javdatabase fallback (by printed code). Faithful to
 *     the Python `(from_r18(cid) if cid else None) or (from_javdb(code) if code
 *     else None) or {}` chain, keyed and cached by `cid || code`.
 *
 *   titleLookup(tv, title, year) -> TitleLookupResponse   (the /movie + /tv endpoints)
 *     Title-addressable lookup via TMDB (reuses src/api/sources/tmdb.tmdbLookup);
 *     for tv, falls back to an AniList cover when TMDB finds none, exactly like
 *     the sidecar's /tv handler.
 *
 * Caching mirrors the sidecar: /meta lived in proxy_cache.json (the `cache`
 * dict, written permanently, never expiring) and /movie//tv lived in
 * discover_covers.json (cover_cached, also permanent). Both are ported onto the
 * SQLite meta_cache table with a very long TTL so a resolved record is treated
 * as permanent. Caching is best-effort and gated on isDbAvailable(), so on a
 * non-Tauri host (vitest / plain browser) the resolvers just hit the network.
 *
 * All network goes through src/net/http.ts. Every helper swallows fetch errors
 * to null/{} the same way the Python get_text / get_json / paced_get_json do.
 */
import { httpJson, httpText, DEFAULT_USER_AGENT } from "@/net/http"
import { getCached, setCached, isDbAvailable } from "@/state/db"
import { tmdbKey, tmdbLookup } from "@/api/sources/tmdb"
import { javdbDetail, javdbSearch } from "@/api/sources/javdb"
import { normalizeCodeNum } from "@/lib/codes"
import type { JavMeta, TitleLookupResponse, TitleMeta } from "@/api/types"

/**
 * Treat a cached meta/title record as permanent (the Python caches never
 * expire). One year is effectively forever for this app's lifetime; the row is
 * overwritten on a forced refetch.
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
 * Port of `from_r18(cid)`. Fetch the r18.dev combined detail JSON for a FANZA
 * content id and build a JavMeta: Japanese title, release date (YYYY-MM-DD),
 * runtime ("<n> min"), and the Japanese cast (kanji preferred, then kana, then
 * romaji). Returns null when the fetch fails or nothing useful is present.
 *
 * r18.dev is paced in the Python to respect its rate limit; the per-call pacing
 * lived in paced_get_json. We keep it a single request here (the aggregator can
 * pace a batch); a fetch failure just yields null like the Python.
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
 * Port of `from_javdb(code)`'s pure parsing. Given the javdatabase movie-page
 * HTML and the printed code, pull the romanized cast (from the <title>), the
 * first date (YYYY-MM-DD) on the page, and runtime ("<n> min"). Returns null
 * when the HTML is too small (< 2000 chars, the Python guard) or nothing is
 * found.
 *
 * NOTE on fidelity: the Python takes the FIRST `\d{4}-\d{2}-\d{2}` and the
 * FIRST `\d{2,3}\s*min` on the page verbatim — even though javdatabase's
 * current markup can surface a page-metadata date before the release date and
 * spells runtime as "... minutes" (so the `min` regex misses). This port
 * reproduces that behavior exactly rather than "fixing" it, so its output
 * matches the sidecar byte-for-byte.
 */
export function parseJavdb(html: string, code: string): JavMeta | null {
  if (!html || html.length < 2000) return null
  const rec: JavMeta = {}
  // <title> CODE - <cast> - JAV Database  (case-insensitive, non-greedy cast)
  const escaped = code.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
  const t = new RegExp(
    `<title>\\s*${escaped}\\s*-\\s*([\\s\\S]+?)\\s*-\\s*JAV Database`,
    "i"
  ).exec(html)
  if (t && !t[1].toLowerCase().includes("jav")) rec.cast = t[1].trim()
  const d = /(\d{4}-\d{2}-\d{2})/.exec(html)
  if (d) rec.date = d[1]
  const r = /(\d{2,3})\s*min/i.exec(html)
  if (r) rec.runtime = `${r[1]} min`
  // The page links the FANZA cover by its content id; capture it so metaLookup
  // can pull Japanese cast from r18.dev (javdatabase itself only has romaji).
  const c = /pics\.dmm\.co\.jp\/(?:digital\/video|mono\/movie)\/([a-z0-9_]+)\//i.exec(html)
  if (c) rec._cid = c[1]
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

/** Arguments mirroring the sidecar's /meta query string (cat is accepted but unused). */
export interface MetaArgs {
  cid?: string
  code?: string
  /** Accepted for API parity with the sidecar; the handler ignores it. */
  cat?: string
}

/**
 * Port of the sidecar `/meta` handler.
 *
 * `key = cid || code`; with neither, returns `{}` (no lookup, not cached).
 * Otherwise returns the cached record if present, else resolves
 * `fromR18(cid) || fromJavdb(code) || {}`, caches it under `key`, and returns
 * it. r18.dev (by FANZA content id) is preferred; javdatabase (by printed code)
 * is the fallback. Always resolves to a JavMeta object (possibly empty) — never
 * throws — matching the Python which always 200s with a (cached) dict.
 *
 * The empty `{}` result IS cached for a real key, exactly like the Python which
 * writes `cache[key] = rec` even when rec is `{}` (so a miss is not re-fetched).
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
  // passed in — and we want 出演/cast in Japanese everywhere. So when we don't
  // already have Japanese cast, fetch the javdatabase page (it also yields a
  // FANZA cid via parseJavdb._cid) and use that cid to pull Japanese cast from
  // r18. Merge order makes Japanese fields win, with romaji kept as a fallback.
  let rec: JavMeta = (cid ? await fromR18(cid) : null) ?? {}
  if (!rec.cast_ja && code) {
    const jdb = (await fromJavdb(code)) ?? {}
    const jcid = jdb._cid
    const r18ViaJdb = jcid ? (await fromR18(jcid)) ?? {} : {}
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
      // best-effort cache write (Python swallows the json.dump failure too)
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
 * Port of `anilist_cover(title)`. POST the AniList GraphQL API for an anime
 * matching `title` and return its cover image URL ("large", else "medium").
 * Returns "" on any failure — the Python swallows every exception to "".
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
 * Port of the sidecar `/movie` and `/tv` handlers (`tv` selects which).
 *
 * Mirrors the response shape `{ ok, haskey, meta? }`:
 *   - no TMDB key            -> { ok:false, haskey:false }            (no meta)
 *   - key but empty title    -> { ok:false, haskey:true }             (no meta)
 *   - otherwise              -> { ok:<bool cover>, haskey:true, meta }
 *
 * `meta` is `tmdbLookup(title, year, tv) or {}`. For tv, when TMDB yields no
 * cover, an AniList cover (ar 0.69) is tried as a fallback (no key needed),
 * exactly like the Python /tv handler. `ok` is true only when the final record
 * carries a cover.
 *
 * Results are cached (best-effort, permanent TTL) under
 * `("tmdbtv:" | "tmdb:") + title.toLowerCase() + "|" + year`, matching the
 * Python cover_cached key. `fresh` bypasses the cache and overwrites it.
 */
export async function titleLookup(
  tv: boolean,
  title: string,
  year: string,
  opts: { fresh?: boolean } = {}
): Promise<TitleLookupResponse> {
  const t = (title ?? "").trim()
  const y = (year ?? "").trim()

  if (!(await tmdbKey())) return { ok: false, haskey: false }
  if (!t) return { ok: false, haskey: true }

  const cacheKey = `${tv ? "tmdbtv:" : "tmdb:"}${t.toLowerCase()}|${y}`
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

  // tmdb_lookup(...) or {}  — Python coerces a null lookup to an empty dict.
  let rec: TitleMeta = (await tmdbLookup(t, y, tv)) ?? {}

  // anime fallback: AniList cover when TMDB found none (tv only, no key needed).
  if (tv && !rec.cover) {
    const c = await anilistCover(t)
    if (c) rec = { cover: c, ar: 0.69 }
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
