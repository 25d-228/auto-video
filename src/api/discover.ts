/**
 * Discover aggregator — TypeScript port of the sidecar's build_discover()
 * (sidecar/av_proxy.py). Routes a (cat, source, list) selection from the
 * two-dropdown catalog (src/views/discover/model.ts DISC_CATALOG) to the right
 * source module, resolves covers where the source can't, drops coverless items,
 * caches the listing for 300s, and returns DiscoverItem[].
 *
 * The list id carries the trending/newest meaning (the legacy `mode` is gone);
 * an unknown/empty list falls back to the provider's first (default) list, and
 * an unknown provider falls back to the category's default provider — exactly
 * like the Python _resolve_list().
 *
 * Cover policy (mirrors the Python):
 *   - mov: TMDB / IMDb / YTS all ship a poster; just drop the rare coverless row.
 *   - tv (tmdb/imdb): ship a poster. tv (tpb): no cover -> resolveCovers('tv').
 *   - dmm: cover comes verbatim off the listing page (no resolve pass).
 *   - javdb: the source ships cmastd cover URLs; cmastd serves them as
 *     octet-stream so they're proxied to blob: URLs post-cache (coverObjectUrl).
 *   - mgstage/sukebei: resolve a portrait cover by code, then drop coverless.
 */
import { fetchDmm } from "@/api/sources/dmm"
import { fetchDmmDigitalVr } from "@/api/sources/dmm-digital"
import { fetchImdbChart, type ImdbSort } from "@/api/sources/imdb"
import {
  discover as javdbDiscover,
} from "@/api/sources/javdb"
import { fetchMgstage } from "@/api/sources/mgstage"
import {
  fetchSukebei,
  type SukebeiItem,
} from "@/api/sources/sukebei"
import {
  fetchTmdbList,
  fetchTmdbTrending,
  tmdbPath,
} from "@/api/sources/tmdb"
import { fetchTv } from "@/api/sources/tpb"
import { fetchMovies } from "@/api/sources/yts"
import { resolveCovers } from "@/api/covers"
import type { Cat, DiscoverItem } from "@/api/types"
import { coverObjectUrl } from "@/net/http"
import { DISC_CATALOG } from "@/views/discover/model"
import { getCached, getKey, isDbAvailable, setCached } from "@/state/db"

/** Seconds a discover listing is cached before re-fetching (sidecar LIST_TTL). */
export const LIST_TTL_SEC = 300

/**
 * Resolve the effective (source, list) for a (cat, source, list) request.
 * Port of _resolve_list(): an unknown source falls back to the category's first
 * provider; an unknown/empty list falls back to that provider's first list.
 */
export function resolveList(
  cat: Cat,
  source: string,
  list: string
): { source: string; list: string } {
  const providers = DISC_CATALOG[cat]
  let entry = providers.find((p) => p.provider === source)
  if (!entry) {
    entry = providers[0] // category default provider
  }
  if (!entry) return { source: "", list: "" }
  const lists = entry.lists
  if (list && (lists as readonly string[]).includes(list)) {
    return { source: entry.provider, list }
  }
  return { source: entry.provider, list: lists.length > 0 ? lists[0]! : "" }
}

/**
 * Read the javbus cookie (provider key) for the gated cover probe. Returns ""
 * when unset or the DB is unavailable, so the javbus step is skipped (matches
 * the sidecar reading keys_store['javbus']).
 */
async function javbusCookie(): Promise<string> {
  if (!isDbAvailable()) return ""
  try {
    return (await getKey("javbus"))?.trim() ?? ""
  } catch {
    return ""
  }
}


/**
 * Turn each item's cmastd cover URL into a displayable `blob:` URL in place,
 * decrypting the single-byte-XOR payload (handled inside coverObjectUrl). Runs
 * AFTER the listing cache so SQLite stores the raw cmastd URLs. A failed fetch
 * leaves the raw URL (CoverImage falls back to its placeholder). Parallel.
 */
async function proxyCovers(items: DiscoverItem[]): Promise<void> {
  await Promise.all(
    items.map(async (x) => {
      if (!x.cover || x.cover.startsWith("blob:")) return
      try {
        x.cover = await coverObjectUrl(x.cover)
      } catch {
        // keep raw URL; CoverImage shows the placeholder
      }
    })
  )
}

/** Keep only items that carry a cover, then cut to `n` (port of the Python list-comp). */
function keepCovered(items: DiscoverItem[], n: number): DiscoverItem[] {
  const out: DiscoverItem[] = []
  for (const x of items) {
    if (!x.cover) continue
    out.push(x)
    if (out.length >= n) break
  }
  return out
}

/** A cached listing tagged with the process that produced it (see SESSION_TAG). */
type CachedListing = { sid: string; data: DiscoverItem[] }

/**
 * Per-process tag stamped into every cached listing. dmm/mgstage covers are
 * `blob:` object URLs local to the webview document, so they die when the app
 * restarts; a listing cached by a PRIOR process must be re-fetched rather than
 * served with dead blobs (which render as blank cards). Entries that carry only
 * raw http(s) covers (tmdb/javdb/sukebei) stay valid across a restart.
 */
const SESSION_TAG = `${Date.now()}.${Math.random().toString(36).slice(2)}`

/**
 * Run `fn` behind the 300s listing cache (port of listing_cached). The cache key
 * is "<cat>|<src>|<lst>". `fresh` bypasses the cache. Caching is best-effort and
 * only active inside Tauri (isDbAvailable()); elsewhere it's a live fetch. A
 * prior-session hit that still carries session-local blob: covers is treated as
 * a miss and re-fetched, so dmm/mgstage covers can't go blank after a restart.
 */
async function cachedListing(
  key: string,
  fresh: boolean,
  fn: () => Promise<DiscoverItem[]>
): Promise<DiscoverItem[]> {
  if (!fresh && isDbAvailable()) {
    const hit = await getCached<CachedListing | DiscoverItem[]>(
      "listing_cache",
      key,
      LIST_TTL_SEC
    )
    if (hit) {
      // Legacy entries were a bare array (no session tag).
      const sid = Array.isArray(hit) ? "" : hit.sid
      const items = Array.isArray(hit) ? hit : hit.data
      const deadBlobs =
        sid !== SESSION_TAG && items.some((x) => x.cover.startsWith("blob:"))
      if (!deadBlobs) return items
    }
  }
  const data = (await fn()) || []
  if (isDbAvailable()) {
    try {
      await setCached("listing_cache", key, { sid: SESSION_TAG, data })
    } catch {
      // best-effort
    }
  }
  return data
}

/**
 * Build a Discover feed for the given two-dropdown selection.
 *
 * @param cat    library category: mov | tv | ad | vrc
 * @param source provider id (tmdb/imdb/yts/tpb/javdb/dmm/mgstage/sukebei)
 * @param list   list id within the provider (trending/popular/newest/…)
 * @param n      max items to return (default 50)
 * @param fresh  bypass the 300s listing cache
 */
/** Extra per-provider controls (currently the JavDB VR year/month/sort selectors). */
export interface DiscoverOpts {
  year?: string
  month?: string
  sortBy?: string
  orderBy?: "desc" | "asc"
}

export async function discover(
  cat: Cat,
  source: string,
  list: string,
  n = 50,
  fresh = false,
  opts: DiscoverOpts = {}
): Promise<DiscoverItem[]> {
  const resolved = resolveList(cat, (source || "").toLowerCase(), (list || "").toLowerCase())
  const src = resolved.source
  const lst = resolved.list
  const key = `${cat}|${src || "def"}|${lst || "def"}`

  // -------------------------------------------------------------- movies
  if (cat === "mov") {
    const data = await cachedListing(key, fresh, () => {
      if (src === "imdb") return fetchImdbChart("mov", lst as ImdbSort, { fresh })
      if (src === "yts") return fetchMovies(lst, fresh)
      if (lst === "trending") return fetchTmdbTrending("movie")
      return fetchTmdbList("mov", tmdbPath("mov", lst))
    })
    return keepCovered(data, n)
  }

  // -------------------------------------------------------------- TV
  if (cat === "tv") {
    if (src === "tmdb" || src === "imdb") {
      const data = await cachedListing(key, fresh, () => {
        if (src === "imdb") return fetchImdbChart("tv", lst as ImdbSort, { fresh })
        if (lst === "trending") return fetchTmdbTrending("tv")
        return fetchTmdbList("tv", tmdbPath("tv", lst))
      })
      return keepCovered(data, n)
    }
    // tpb: trending=top100 precompiled, newest=q.php category:205. No cover on
    // the rows -> resolve via tvmaze AFTER the listing cache (the Python caches
    // the raw scan, and resolveTvCover has its own persistent cover_cache).
    const scan = await cachedListing(key, fresh, () =>
      fetchTv(lst === "newest" ? "newest" : "trending")
    )
    await resolveCovers("tv", scan)
    return keepCovered(scan, n)
  }

  // -------------------------------------------------------------- adult / VR
  const wantVr = cat === "vrc"
  const cookie = await javbusCookie()

  if (src === "dmm") {
    if (wantVr) {
      // VR: the digital GraphQL API (real streaming-VR rankings) — covers are raw
      // awsimgsrc URLs, proxied to blob: AFTER the cache so SQLite keeps raw URLs.
      const data = await cachedListing(key, fresh, () => fetchDmmDigitalVr(lst))
      await proxyCovers(data)
      return keepCovered(data, n)
    }
    // Adult: the mono/dvd HTML floor (sort lists + best-seller ranking pages);
    // covers come verbatim off the page (resolved inside fetchDmm).
    const data = await cachedListing(key, fresh, () => fetchDmm(false, lst))
    return keepCovered(data, n)
  }

  if (src === "javdb") {
    // javdb source ships cmastd covers. ad -> the Censored ranking for the list
    // window (daily/weekly/monthly); vrc -> the Categories→Censored VR browser
    // (tag 212) filtered by the chosen year/month and ordered by the chosen
    // sort. The cmastd CDN "encrypts" the jacket with a trivial single-byte XOR;
    // coverObjectUrl decodes it, so we use the REAL javdb jacket directly. Proxy
    // AFTER the cache so SQLite keeps the raw cmastd URLs, not blob: URLs.
    // The vrc selection lives in opts (not the list id), so it's part of the key.
    const jkey =
      cat === "vrc"
        ? `${key}|${opts.year ?? ""}|${opts.month ?? ""}|${opts.sortBy ?? ""}|${opts.orderBy ?? ""}`
        : key
    const data = await cachedListing(jkey, fresh, () => javdbDiscover(cat, lst, opts))
    await proxyCovers(data)
    return keepCovered(data, n)
  }

  if (src === "mgstage") {
    // MGStage ranking windows (daily/weekly/monthly/popular). Every listed
    // product ships its own wide-jacket cover, so keep it directly. The old
    // "clear the jacket, resolve a portrait by code" pass dropped most
    // MG-exclusive labels (SIRO/LUXU/GANA/300MIUM have no portrait on
    // dmm/javbus, and a stale 'no cover' cache entry made it stick) — leaving
    // ~5 items where the page actually has 50. It was also slow (per-code
    // cascade). The intrinsic-ratio cover card renders the wide jacket fine.
    const data = await cachedListing(key, fresh, () => fetchMgstage(wantVr, lst))
    return keepCovered(data, n)
  }

  // sukebei: lst is most_seeded|newest|most_downloaded -> nyaa s=seeders|id|downloads.
  // The RAW pool is cached (like the Python listing_cached around fetch_sukebei);
  // the vr split / cover resolve / sub rewrite run per call, so a different `n`
  // within the TTL still re-derives from the full pool. resolveJavCover keeps its
  // own persistent cover_cache, so re-resolution on a cache hit stays cheap.
  const pool = (await cachedListing(key, fresh, () =>
    wantVr ? fetchSukebei(lst, "VR", 4) : fetchSukebei(lst, "", 8)
  )) as SukebeiItem[]
  // Split the pool by VR-ness, then assign the real category + sub + cover.
  const filtered = pool.filter((x) => Boolean(x.vr) === wantVr)
  await resolveCovers(cat, filtered, cookie)
  const out: DiscoverItem[] = []
  for (const x of filtered) {
    if (!x.cover) continue
    const code = x.code || ""
    const sub = ((wantVr ? "VR · " : "") + code).replace(/^[\s·]+|[\s·]+$/g, "")
    // Strip the aggregator-internal fields like the Python _clean (which drops
    // every "_"-prefixed key and "vr"; `magnet` is intentionally kept).
    const { vr: _omitVr, _rawtitle, _downloads: _omitDl, ...item } = x
    item.cat = cat
    item.sub = sub || (_rawtitle || "").slice(0, 30)
    out.push(item)
    if (out.length >= n) break
  }
  return out
}
