/**
 * Cover resolution — TypeScript port of the sidecar's cover chain
 * (sidecar/av_proxy.py: jav_cover, r18_cover, javbus_cover, javdb_cover,
 * tvmaze_cover, resolve_covers + the cover_cached wrapper).
 *
 * Discover feeds whose source does NOT ship a usable poster (sukebei/javdb/
 * mgstage adult listings carry no portrait cover; TPB TV rows carry none) get
 * their cover filled in here:
 *   - ad/vrc  -> jav_cover(code): dmm -> r18 -> javbus -> mgstage -> javdb,
 *                first that returns a real (non-placeholder) image wins.
 *   - tv      -> tvmaze_cover(imdb, name).
 *
 * Hotlink-protected hosts (pics.dmm.co.jp, image.mgstage.com, javbus, the
 * javdatabase webp mirror) can't be rendered with a bare <img src>; the sidecar
 * proxied them through its /img endpoint. The TS equivalent is
 * {@link coverObjectUrl}, which fetches the bytes (with the right Referer) and
 * returns a `blob:` URL — so every resolved cover here is a displayable URL,
 * exactly like the dmm.ts / mgstage.ts source modules already produce.
 *
 * Persistent caching mirrors the sidecar's cover_cached(): the RAW resolved
 * source URL + aspect ratio are stored in the SQLite cover_cache table (keyed by
 * code / "tv:<imdb|name|id>"), so the per-code CDN probing only happens once.
 * blob: URLs are per-session, so we cache the raw URL and re-proxy it for
 * display. Outside Tauri (vitest/browser) the DB is unavailable and resolution
 * falls back to a live probe with no caching.
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

/** Cover cache TTL (seconds). The sidecar's cover_cached never expired; we use a
 * long TTL so a resolved cover survives for a day before re-probing. */
export const COVER_TTL_SEC = 86_400

/** Default JAV cover aspect ratio (w/h) used when a resolved/cached cover has
 * none — the portrait DMM jacket ratio the sidecar assumed. */
const JAV_DEFAULT_AR = 0.72

/** Referer pics.dmm.co.jp / r18 jacket images need. */
const DMM_REFERER = "https://www.dmm.co.jp/"
/** Referer javdatabase pages / its webp mirror need. */
const JAVDB_DATABASE_REFERER = "https://www.javdatabase.com/"
/** Referer javbus pages / javbus-hosted covers need. */
const JAVBUS_REFERER = "https://www.javbus.com/"

/** A resolved cover: a raw source URL + aspect ratio. "" url = not found. */
export interface ResolvedCover {
  /** RAW source URL (not yet proxied). "" when nothing resolved. */
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
 * Port of r18_cover(code). r18.dev is ungated and stores the exact DMM jacket
 * URL (incl. amateur floor / maker prefix we can't otherwise derive). Returns a
 * raw pics.dmm.co.jp URL (hotlink-protected -> proxy) or NONE.
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
 * Port of javbus_cover(code). Gated index — uses the user's verified javbus
 * cookie (provider key "javbus") to read the product page's <a class="bigImage">,
 * which carries the cover for h_NNNN titles no free source lists. javbus-hosted
 * covers are hotlink-protected (need a javbus Referer) -> proxy.
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
 * Port of javdb_cover(code). javdatabase exposes the correct DMM cid (incl.
 * h_/118/maker prefixes) + a webp mirror. Prefer DMM ps (portrait), then pl
 * (wide jacket), then the webp. Returns a raw URL (proxy) or NONE.
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
 * Port of jav_cover(code). Tries each studio-cover source in the same priority
 * order as the sidecar: dmm -> r18 -> javbus -> mgstage -> javdb(webp). Returns
 * the first real cover, else NONE. FC2 titles truly have no studio cover.
 *
 * `javbusCookie` is the user's javbus cookie (provider key); omit/empty skips the
 * gated javbus probe, exactly as the Python did when keys_store['javbus'] was unset.
 */
export async function javCover(
  code: string,
  javbusCookie = ""
): Promise<ResolvedCover> {
  if (!code || code.startsWith("FC2")) return NONE
  // dmm_cover returns a RAW pics URL (hotlink-protected -> proxy).
  const dmm: DmmCover = await dmmCover(code)
  if (dmm.url) return { url: dmm.url, ar: dmm.ar, proxy: true }
  const r18Result = await r18Cover(code)
  if (r18Result.url) return r18Result
  const javbusResult = await javbusCover(code, javbusCookie)
  if (javbusResult.url) return javbusResult
  // mgstage_cover returns a RAW image.mgstage.com URL (hotlink-protected ->
  // proxy at display time, with a host-derived Referer).
  const mgstageResult = await mgstageCover(code)
  if (mgstageResult.url) return { url: mgstageResult.url, ar: mgstageResult.ar, proxy: true }
  return javdbCover(code)
}

// ----------------------------------------------------------------- tvmaze

interface TvmazeShow {
  image?: { medium?: string; original?: string }
}

/**
 * Port of tvmaze_cover(imdb, name). Looks up a show by IMDb id, then by name.
 * tvmaze covers are not hotlink-protected, so they render directly (no proxy).
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

/** GraphQL query the sidecar's anilist_cover sends (anime cover by search title). */
const ANILIST_QUERY =
  "query($s:String){Media(search:$s,type:ANIME){coverImage{large medium}}}"

/**
 * Port of anilist_cover(title). The sidecar's anime fallback for a TV title when
 * tvmaze/TMDB has no poster: POST a GraphQL search to anilist.co and return the
 * large (or medium) cover image. AniList CDN images are not hotlink-protected, so
 * they render directly (no proxy). Returns "" on any error / no match.
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
 * GET image bytes (with a host Referer), reject placeholders, return its aspect
 * ratio. Port of _cover_meta — coverMeta() already rejects <6KB and the 590x800
 * "now printing" placeholder. Returns null on fetch failure / placeholder.
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
 * Turn a {@link ResolvedCover} into a displayable (blob:) URL by proxying the
 * raw, hotlink-protected source URL through {@link coverObjectUrl}. The Referer
 * is derived from the URL's host (DMM / mgstage / javbus / cmastd / javdb) so
 * each host gets the one it requires — never hardcoded. A non-proxied or empty
 * URL passes through. Returns "" when the proxy fetch fails.
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
 * Resolve one JAV cover by code with the persistent cover_cache (port of
 * cover_cached('jav2:'+code, ...)). Returns the displayable URL + ar. The raw
 * source URL + ar are cached; the blob is re-proxied per call so it stays a
 * valid object URL for this session.
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
      // A stale blob: URL from an older build is dead this session — re-resolve.
      if (!hit.url.startsWith("blob:")) {
        const url = await displayUrl({ url: hit.url, ar: hit.ar, proxy: true })
        return { url, ar: hit.ar || JAV_DEFAULT_AR }
      }
    }
  }
  const resolved = await javCover(code, javbusCookie)
  // Persist the RAW url + ar (best-effort). Never cache a blob: URL — it is a
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
 * Resolve one TV cover (imdb/name) with the persistent cover_cache (port of
 * cover_cached('tv:'+key, ...)). tvmaze URLs render directly (no proxy).
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
  // Anime fallback (port of the sidecar /tv handler): when tvmaze has no poster,
  // try AniList by the show name. Costs no API key.
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
 * (The TS tpb.ts intentionally strips these, so the fallback uses title.)
 */
interface CoverScanItem extends DiscoverItem {
  _imdb?: string
  _name?: string
}

/**
 * Port of resolve_covers(cat, items). Fills item.cover (and ar for ad/vrc) in
 * place for the items a source can't cover itself:
 *   - tv      -> tvmaze by imdb/name.
 *   - ad/vrc  -> jav_cover by code.
 * Items that already carry a cover are skipped (no wasted probe). Resolution is
 * parallel across items. mov never needs this (TMDB/IMDb/YTS ship posters).
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
