/**
 * sukebei.nyaa.si scraper. sukebei is the adult/VR torrent index; it serves full
 * HTML listings under category c=2_2 ("Real Life - Videos"), so we parse the
 * <tbody> table directly.
 *
 * Two public entry points:
 *   - fetchSukebei(list, query?, pages?) -> SukebeiItem[]  (the Discover feed;
 *     the aggregator later filters by `vr`, assigns `cat`, rewrites `sub`, and
 *     resolves a cover by `code`, so we leave cover empty and keep the code).
 *   - seedersSukebei(code) -> Release[]  (the per-item Download dialog rows).
 *
 * Network goes through src/net/http.ts (Tauri http plugin: bypasses CORS, sets a
 * Referer). Code / VR parsing reuses src/lib/codes.ts. Listings are cached in the
 * SQLite listing_cache (300s TTL) when the DB is available, degrading to a live
 * fetch otherwise.
 */
import type { DiscoverItem, Release } from "@/api/types"
import { isVr, parseCode } from "@/lib/codes"
import { quality } from "@/lib/quality"
import { httpText } from "@/net/http"
import { getCached, isDbAvailable, setCached } from "@/state/db"

// Re-exported for callers that historically imported it from this module
// (tpb.ts, the seeders helpers, the sukebei unit test).
export { quality }

/** sukebei view-page / Referer host. */
const SUKEBEI_BASE = "https://sukebei.nyaa.si"

/** Seconds a listing stays fresh before a refetch. */
const LIST_TTL_SEC = 300

/**
 * The list ids the Discover UI sends for sukebei, mapped to nyaa's `s=` sort
 * token. `most_seeded` (the default / "trending") -> seeders; `newest` -> id
 * (keep nyaa's id order); `most_downloaded` -> downloads (completed count).
 */
export type SukebeiList = "most_seeded" | "newest" | "most_downloaded"

function nyaaSort(list: string): "seeders" | "id" | "downloads" {
  if (list === "newest") return "id"
  if (list === "most_downloaded") return "downloads"
  return "seeders"
}

/**
 * One sukebei feed row. Extends the shared {@link DiscoverItem} with three
 * aggregator-internal fields the wiring phase consumes before emitting the public
 * shape:
 *   - `vr`        : true when isVr(title, code); used to split ad vs vrc.
 *   - `_rawtitle` : the untruncated release title (sub fallback + seeder names).
 *   - `_downloads`: the completed count (used to sort the most_downloaded list).
 *
 * `cat` is a placeholder ("ad") at fetch time; the aggregator overwrites it.
 */
export interface SukebeiItem extends DiscoverItem {
  /** true for VR titles (isVr on the raw title + parsed code). */
  vr: boolean
  /** Untruncated release title. */
  _rawtitle: string
  /** Completed/download count. */
  _downloads: number
}

// ----------------------------------------------------------------- helpers

/**
 * Minimal HTML-entity unescape covering what nyaa emits in titles/magnets
 * (&amp; &lt; &gt; &quot; &#39; &nbsp; + numeric refs).
 */
function unescapeHtml(s: string): string {
  if (!s) return ""
  return s
    .replace(/&#(\d+);/g, (_m, d: string) => String.fromCodePoint(Number(d)))
    .replace(/&#x([0-9a-fA-F]+);/g, (_m, h: string) =>
      String.fromCodePoint(parseInt(h, 16))
    )
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&apos;/g, "'")
    .replace(/&nbsp;/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
}

/** Strip non-digits and parse to int; 0 on failure. */
function numFromCell(x: string): number {
  const n = parseInt(x.replace(/[^\d]/g, ""), 10)
  return Number.isNaN(n) ? 0 : n
}

// ----------------------------------------------------------------- parser

/**
 * Parse a sukebei listing page's HTML into feed rows. Pure (no network). `seen`
 * dedups view-ids across pages.
 *
 * Returns the rows for this page (already deduped against `seen`), or [] when the
 * page has no <tbody>/<tr>.
 */
export function parseSukebeiList(
  html: string,
  seen: Set<string> = new Set()
): SukebeiItem[] {
  const items: SukebeiItem[] = []
  const tbody = /<tbody>([\s\S]*?)<\/tbody>/.exec(html)
  if (!tbody) return items
  const rows = tbody[1]!.match(/<tr[^>]*>[\s\S]*?<\/tr>/g)
  if (!rows) return items

  for (const row of rows) {
    // The view link + title: prefer the `title="..."` attribute, fall back to
    // the anchor text.
    const viewMatch =
      /\/view\/(\d+)"\s+title="([^"]*)"/.exec(row) ??
      /\/view\/(\d+)"[^>]*>([^<]+)</.exec(row)
    if (!viewMatch) continue
    const viewId = viewMatch[1]!
    if (seen.has(viewId)) continue
    seen.add(viewId)
    const title = unescapeHtml(viewMatch[2]!)

    const magnetMatch = /href="(magnet:[^"]+)"/.exec(row)
    const magnet = magnetMatch ? unescapeHtml(magnetMatch[1]!) : ""

    // nyaa's text-center columns are [Link, Size, Date, Seeders, Leechers,
    // Completed]; size is tds[-5], seeders tds[-3], completed/downloads tds[-1].
    const centerCells = [...row.matchAll(/<td class="text-center"[^>]*>([\s\S]*?)<\/td>/g)].map(
      (m) => m[1]!
    )
    const seeders =
      centerCells.length >= 3 ? numFromCell(centerCells[centerCells.length - 3]!) : 0
    const downloads =
      centerCells.length >= 1 ? numFromCell(centerCells[centerCells.length - 1]!) : 0
    const size =
      centerCells.length >= 5
        ? centerCells[centerCells.length - 5]!.replace(/<[^>]+>/g, "").trim()
        : ""

    const code = parseCode(title)
    items.push({
      id: `sk_${viewId}`,
      // `cat` is assigned by the aggregator after the vr split; placeholder here.
      cat: "ad",
      title: code || title.slice(0, 48),
      _rawtitle: title,
      sub: "",
      cover: "",
      ar: 0.72,
      seeders,
      _downloads: downloads,
      size,
      src: "sukebei",
      state: "new",
      year: "",
      runtime: 0,
      rating: 0,
      code,
      magnet,
      vr: isVr(title, code),
      // nyaa view page for the item's original listing.
      link: `${SUKEBEI_BASE}/view/${viewId}`,
    })
  }
  return items
}

/**
 * Sort a sukebei feed in place to match the requested nyaa sort:
 * seeders -> seeders desc; downloads -> completed desc; id (newest) -> leave the
 * page order (nyaa already returns newest-first).
 */
export function sortSukebei(items: SukebeiItem[], list: string): SukebeiItem[] {
  const sort = nyaaSort(list)
  if (sort === "seeders") {
    items.sort((a, b) => b.seeders - a.seeders)
  } else if (sort === "downloads") {
    items.sort((a, b) => b._downloads - a._downloads)
  }
  return items
}

// ----------------------------------------------------------------- fetch

/** Build a sukebei listing URL for one page. */
function listUrl(query: string, sort: string, page: number): string {
  const qs = query ? `&q=${encodeURIComponent(query)}` : ""
  return `${SUKEBEI_BASE}/?c=2_2${qs}&s=${sort}&o=desc&p=${page}`
}

/**
 * Fetch the sukebei feed for one Discover list.
 *   - `list` is the Discover list id (most_seeded|newest|most_downloaded),
 *     mapped to nyaa's s=seeders|id|downloads.
 *   - `query` filters by a search term (e.g. "VR" for the VR pool, or a code);
 *     "" pulls the whole category.
 *   - `pages` defaults to 1 when a query is set, else 3.
 *
 * Cached per (sort, query, pages) in listing_cache (300s TTL) when the DB is
 * available. Pages are fetched sequentially and deduped by view-id; the loop stops
 * at the first page with no rows.
 */
export async function fetchSukebei(
  list: string,
  query = "",
  pages?: number
): Promise<SukebeiItem[]> {
  const sort = nyaaSort(list)
  const pageCount = pages ?? (query ? 1 : 3)
  const cacheKey = `sukebei:${sort}:${query}:${pageCount}`

  if (isDbAvailable()) {
    const hit = await getCached<SukebeiItem[]>(
      "listing_cache",
      cacheKey,
      LIST_TTL_SEC
    )
    if (hit) return hit
  }

  const items: SukebeiItem[] = []
  const seen = new Set<string>()
  for (let p = 1; p <= pageCount; p++) {
    let html: string
    try {
      html = await httpText(listUrl(query, sort, p), {
        referer: `${SUKEBEI_BASE}/`,
      })
    } catch {
      break
    }
    const pageItems = parseSukebeiList(html, seen)
    if (pageItems.length === 0) break
    items.push(...pageItems)
  }

  sortSukebei(items, list)

  if (isDbAvailable()) {
    try {
      await setCached("listing_cache", cacheKey, items)
    } catch {
      // caching is best-effort; a write failure must not fail the fetch.
    }
  }
  return items
}

/**
 * Real sukebei releases for one JAV code, for the Download dialog: a single-page
 * code search ('trending' list = seeders sort, pages=1) mapped to Release rows.
 * Returns [] for an empty code.
 */
export async function seedersSukebei(code: string): Promise<Release[]> {
  if (!code) return []
  const items = await fetchSukebei("trending", code, 1)
  return items.map((x) => {
    const name = x._rawtitle || x.title || ""
    return {
      name,
      source: "sukebei",
      seeders: x.seeders || 0,
      size: x.size || "",
      magnet: x.magnet || "",
      quality: quality(name),
    }
  })
}
