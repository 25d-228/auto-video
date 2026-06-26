/**
 * Cover resolution. Fills in posters for discover feeds whose source ships none
 * (sukebei, javdb, mgstage adult listings, TPB TV rows): ad/vrc by code (dmm ->
 * r18 -> javbus -> mgstage -> javdb, first non-placeholder image wins), tv via
 * tvmaze.
 *
 * Hotlink-protected hosts (pics.dmm.co.jp, image.mgstage.com, javbus, the
 * javdatabase webp mirror) can't render via a bare <img src>, so
 * {@link coverObjectUrl} fetches the bytes (with the right Referer) and returns
 * a blob: URL.
 *
 * The SQLite cover_cache table stores the raw resolved URL + aspect ratio (keyed
 * by code / "tv:<imdb|name|id>") so CDN probing happens once. blob: URLs are
 * per-session, so we cache the raw URL and re-proxy it for display. Outside Tauri
 * the DB is unavailable and resolution falls back to a live probe, no caching.
 */
import { coverMeta, dmmCover, type DmmCover } from "@/api/sources/dmm"
import { mgstageCover } from "@/api/sources/mgstage"
import type { Cat, DiscoverItem } from "@/api/types"
import { parseCode } from "@/lib/codes"
import { coverObjectUrl, httpBytes, httpJson, httpText } from "@/net/http"
import {
  getCachedCover,
  isDbAvailable,
  setCachedCover,
} from "@/state/db"

/** Cover cache TTL (seconds). Long, so a resolved cover survives a day before re-probing. */
export const COVER_TTL_SEC = 86_400

/** Default JAV cover aspect ratio (w/h) when none is known; the portrait DMM jacket ratio. */
const JAV_DEFAULT_AR = 0.72

/** Referer pics.dmm.co.jp / r18 jacket images need. */
const DMM_REFERER = "https://www.dmm.co.jp/"
/** Referer javdatabase pages / its webp mirror need. */
const JAVDB_DATABASE_REFERER = "https://www.javdatabase.com/"
/** Referer javbus pages / javbus-hosted covers need. */
const JAVBUS_REFERER = "https://www.javbus.com/"

/** A resolved cover: a raw source URL + aspect ratio. "" url = not found. */
export interface ResolvedCover {
  /** Raw source URL (not yet proxied). "" when nothing resolved. */
  url: string
  /** Aspect ratio w/h; 0 when nothing resolved. */
  ar: number
  /** Whether the URL is hotlink-protected and must be proxied for display. */
  proxy: boolean
}

const NONE: ResolvedCover = { url: "", ar: 0, proxy: false }

// ----------------------------------------------------------------- r18.dev

interface R18Detail {
  images?: {
    jacket_image?: {
      large?: string
      large2?: string
    }
  }
}

/**
 * r18.dev is ungated and stores the exact DMM jacket URL (incl. amateur floor /
 * maker prefix we can't otherwise derive). Returns a raw pics.dmm.co.jp URL
 * (hotlink-protected, proxy) or NONE.
 */
export async function r18Cover(code: string): Promise<ResolvedCover> {
  let detail: R18Detail | null
  try {
    detail = await httpJson<R18Detail>(
      `https://r18.dev/videos/vod/movies/detail/-/dvd_id=${code}/json`,
      { referer: "https://r18.dev/", timeoutMs: 15_000 }
    )
  } catch {
    return NONE
  }
  const jacket = detail?.images?.jacket_image ?? {}
  for (const u of [(jacket.large ?? "").trim(), (jacket.large2 ?? "").trim()]) {
    if (!u) continue
    const meta = await probeCover(u, DMM_REFERER)
    if (meta) return { url: u, ar: meta.ar, proxy: true }
  }
  return NONE
}

// ----------------------------------------------------------------- javbus

/**
 * Gated index. Uses the user's javbus cookie (provider key "javbus") to read the
 * product page's <a class="bigImage">, which carries the cover for h_NNNN titles
 * no free source lists. javbus-hosted covers need a javbus Referer (proxy).
 */
export async function javbusCover(
  code: string,
  cookie: string
): Promise<ResolvedCover> {
  const trimmedCookie = (cookie || "").trim()
  if (!trimmedCookie) return NONE
  let html = ""
  for (const u of [
    `https://www.javbus.com/${code}`,
    `https://www.javbus.com/en/${code}`,
  ]) {
    try {
      html = await httpText(u, {
        cookie: trimmedCookie,
        referer: JAVBUS_REFERER,
        timeoutMs: 20_000,
      })
    } catch {
      html = ""
    }
    if (html && !html.includes("Age Verification") && html.includes("bigImage")) {
      break
    }
  }
  const m = /<a class="bigImage"[^>]*href="([^"]+)"/.exec(html || "")
  if (!m) return NONE
  let coverUrl = m[1]!
  if (coverUrl.startsWith("//")) coverUrl = "https:" + coverUrl
  else if (coverUrl.startsWith("/")) coverUrl = "https://www.javbus.com" + coverUrl
  const isJavbusHosted = coverUrl.includes("javbus.com")
  const meta = await probeCover(coverUrl, isJavbusHosted ? JAVBUS_REFERER : DMM_REFERER)
  if (!meta) return NONE
  // javbus-hosted covers route through the referer proxy; DMM-hosted ones are
  // also hotlink-protected, so both are proxied.
  return { url: coverUrl, ar: meta.ar, proxy: true }
}

// ----------------------------------------------------------------- javdatabase

/**
 * javdatabase exposes the correct DMM cid (incl. h_/118/maker prefixes) + a webp
 * mirror. Prefer DMM ps (portrait), then pl (wide jacket), then the webp. Returns
 * a raw URL (proxy) or NONE.
 */
export async function javdbCover(code: string): Promise<ResolvedCover> {
  let html = ""
  try {
    html = await httpText(
      `https://www.javdatabase.com/movies/${code.toLowerCase()}/`,
      { referer: JAVDB_DATABASE_REFERER, timeoutMs: 20_000 }
    )
  } catch {
    return NONE
  }
  if (!html || html.length < 1000) return NONE
  const m =
    /https:\/\/pics\.dmm\.co\.jp\/digital\/video\/([a-z0-9_]+)\/[a-z0-9_]+p[sl]\.jpg/.exec(
      html
    )
  if (m) {
    const cid = m[1]!
    for (const suffix of ["ps", "pl"]) {
      const url = `https://pics.dmm.co.jp/digital/video/${cid}/${cid}${suffix}.jpg`
      const meta = await probeCover(url, DMM_REFERER)
      if (meta) return { url, ar: meta.ar, proxy: true }
    }
  }
  const webpMatch = /https:\/\/www\.javdatabase\.com\/covers\/[^"']+?\.webp/.exec(html)
  if (webpMatch) {
    const meta = await probeCover(webpMatch[0], JAVDB_DATABASE_REFERER)
    if (meta) return { url: webpMatch[0], ar: meta.ar, proxy: true }
  }
  return NONE
}

// ----------------------------------------------------------------- jav chain

/**
 * Tries each studio-cover source in priority order: dmm -> r18 -> javbus ->
 * mgstage -> javdb(webp). Returns the first real cover, else NONE. FC2 titles
 * have no studio cover.
 *
 * `javbusCookie` is the user's javbus cookie (provider key); omit/empty skips the
 * gated javbus probe.
 */
export async function javCover(
  code: string,
  javbusCookie = ""
): Promise<ResolvedCover> {
  if (!code || code.startsWith("FC2")) return NONE
  // dmmCover returns a raw pics URL (hotlink-protected, proxy).
  const dmm: DmmCover = await dmmCover(code)
  if (dmm.url) return { url: dmm.url, ar: dmm.ar, proxy: true }
  const r18Result = await r18Cover(code)
  if (r18Result.url) return r18Result
  const javbusResult = await javbusCover(code, javbusCookie)
  if (javbusResult.url) return javbusResult
  // mgstageCover returns a raw image.mgstage.com URL (hotlink-protected, proxied
  // at display time with a host-derived Referer).
  const mgstageResult = await mgstageCover(code)
  if (mgstageResult.url) return { url: mgstageResult.url, ar: mgstageResult.ar, proxy: true }
  return javdbCover(code)
}

// ----------------------------------------------------------------- tvmaze

interface TvmazeShow {
  image?: { medium?: string; original?: string }
}

/**
 * Look up a show by IMDb id, then by name. tvmaze covers aren't hotlink-protected,
 * so they render directly (no proxy).
 */
export async function tvmazeCover(imdb: string, name: string): Promise<string> {
  if (imdb) {
    try {
      const j = await httpJson<TvmazeShow>(
        `https://api.tvmaze.com/lookup/shows?imdb=${encodeURIComponent(imdb)}`
      )
      const img = j?.image
      if (img) return img.medium || img.original || ""
    } catch {
      // fall through to a name search
    }
  }
  if (name) {
    try {
      const j = await httpJson<TvmazeShow>(
        `https://api.tvmaze.com/singlesearch/shows?q=${encodeURIComponent(name)}`
      )
      const img = j?.image
      if (img) return img.medium || img.original || ""
    } catch {
      return ""
    }
  }
  return ""
}

// ----------------------------------------------------------------- anilist

interface AnilistResponse {
  data?: {
    Media?: {
      coverImage?: { large?: string; medium?: string }
    }
  }
}

/** GraphQL query for an anime cover by search title. */
const ANILIST_QUERY =
  "query($s:String){Media(search:$s,type:ANIME){coverImage{large medium}}}"

/**
 * Anime fallback for a TV title when tvmaze/TMDB has no poster: POST a GraphQL
 * search to anilist.co and return the large (or medium) cover image. AniList CDN
 * images aren't hotlink-protected, so they render directly (no proxy). "" on any
 * error / no match.
 */
export async function anilistCover(title: string): Promise<string> {
  if (!title) return ""
  try {
    const j = await httpJson<AnilistResponse>("https://graphql.anilist.co", {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ query: ANILIST_QUERY, variables: { s: title } }),
      timeoutMs: 12_000,
    })
    return pickAnilistImage(j)
  } catch {
    return ""
  }
}

/** Pure helper: pull large||medium out of an AniList response. Exported for tests. */
export function pickAnilistImage(j: AnilistResponse | null): string {
  const ci = j?.data?.Media?.coverImage
  if (!ci) return ""
  return ci.large || ci.medium || ""
}

// ----------------------------------------------------------------- probe + cache

/**
 * GET image bytes (with a host Referer), reject placeholders, return aspect ratio.
 * coverMeta() rejects <6KB and the 590x800 "now printing" placeholder. Returns
 * null on fetch failure / placeholder.
 */
async function probeCover(
  url: string,
  referer: string
): Promise<{ ar: number } | null> {
  let bytes: Uint8Array
  try {
    bytes = await httpBytes(url, { referer, timeoutMs: 10_000 })
  } catch {
    return null
  }
  return coverMeta(bytes)
}

/**
 * Turn a {@link ResolvedCover} into a displayable blob: URL by proxying the raw
 * source URL through {@link coverObjectUrl}. The Referer is derived from the
 * URL's host (DMM / mgstage / javbus / cmastd / javdb) so each host gets the one
 * it needs. A non-proxied or empty URL passes through. "" when the proxy fails.
 */
async function displayUrl(r: ResolvedCover): Promise<string> {
  if (!r.url) return ""
  if (!r.proxy) return r.url
  try {
    return await coverObjectUrl(r.url) // referer auto-derived per host
  } catch {
    return ""
  }
}

/**
 * Resolve one JAV cover by code, with the persistent cover_cache. Returns the
 * displayable URL + ar. The raw source URL + ar are cached; the blob is
 * re-proxied per call so it stays a valid object URL for this session.
 */
export async function resolveJavCover(
  code: string,
  javbusCookie = ""
): Promise<{ url: string; ar: number }> {
  if (!code) return { url: "", ar: 0 }
  const key = `jav2:${code}`
  if (isDbAvailable()) {
    const hit = await getCachedCover(key, COVER_TTL_SEC)
    if (hit) {
      // Empty cached url = "known to have no cover" (don't re-probe).
      if (!hit.url) return { url: "", ar: hit.ar || JAV_DEFAULT_AR }
      // A stale blob: URL from an older build is dead this session; re-resolve.
      if (!hit.url.startsWith("blob:")) {
        const url = await displayUrl({ url: hit.url, ar: hit.ar, proxy: true })
        return { url, ar: hit.ar || JAV_DEFAULT_AR }
      }
    }
  }
  const resolved = await javCover(code, javbusCookie)
  // Persist the raw url + ar (best-effort). Never cache a blob: URL: it's a
  // session-scoped reference that would be a dead link in the next app run.
  if (isDbAvailable() && !resolved.url.startsWith("blob:")) {
    try {
      await setCachedCover(key, resolved.url, resolved.ar || JAV_DEFAULT_AR)
    } catch {
      // best-effort
    }
  }
  if (!resolved.url) return { url: "", ar: JAV_DEFAULT_AR }
  const url = resolved.proxy ? await displayUrl(resolved) : resolved.url
  return { url, ar: resolved.ar || JAV_DEFAULT_AR }
}

/**
 * Resolve one TV cover (imdb/name) with the persistent cover_cache. tvmaze URLs
 * render directly (no proxy).
 */
export async function resolveTvCover(
  imdb: string,
  name: string,
  id: string
): Promise<string> {
  const key = `tv:${imdb || name || id}`
  if (isDbAvailable()) {
    const hit = await getCachedCover(key, COVER_TTL_SEC)
    if (hit) return hit.url
  }
  let url = await tvmazeCover(imdb, name)
  // Anime fallback: when tvmaze has no poster, try AniList by the show name.
  // Costs no API key.
  if (!url && name) url = await anilistCover(name)
  if (isDbAvailable()) {
    try {
      await setCachedCover(key, url, 0.7)
    } catch {
      // best-effort
    }
  }
  return url
}

// ----------------------------------------------------------------- resolve_covers

/**
 * Per-item internal fields the TPB parser leaves on the row for cover lookup.
 * (tpb.ts strips these, so the fallback uses title.)
 */
interface CoverScanItem extends DiscoverItem {
  _imdb?: string
  _name?: string
}

/**
 * Fill item.cover (and ar for ad/vrc) in place for items a source can't cover
 * itself: tv -> tvmaze by imdb/name, ad/vrc -> javCover by code. Items that
 * already carry a cover are skipped. Parallel across items. mov never needs this
 * (TMDB/IMDb/YTS ship posters).
 */
export async function resolveCovers(
  cat: Cat,
  items: DiscoverItem[],
  javbusCookie = ""
): Promise<void> {
  if (cat === "tv") {
    await Promise.all(
      items.map(async (x) => {
        if (x.cover) return
        const it = x as CoverScanItem
        const imdb = it._imdb || it.code || ""
        const name = it._name || it.title || ""
        x.cover = await resolveTvCover(imdb, name, x.id)
      })
    )
    return
  }
  if (cat === "ad" || cat === "vrc") {
    await Promise.all(
      items.map(async (x) => {
        if (x.cover) return
        const code = x.code || parseCode(x.title || "")
        if (!code) {
          x.cover = ""
          x.ar = x.ar || JAV_DEFAULT_AR
          return
        }
        const r = await resolveJavCover(code, javbusCookie)
        x.cover = r.url
        x.ar = r.ar || JAV_DEFAULT_AR
      })
    )
  }
}
