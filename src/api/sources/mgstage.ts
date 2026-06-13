/**
 * MGStage source — TypeScript port of the Python sidecar's mgstage code
 * (sidecar/av_proxy.py: fetch_mgstage / _mg_get, plus mgstage_cover / mgstage_ids).
 *
 * MGStage (www.mgstage.com) is an adult-video storefront whose ranking pages
 * (week / day) and VR search list MGStage-exclusive labels (SIRO / LUXU / GANA /
 * PRVRSS / DSVR …). Every request needs the age cookie `adc=1` and an mgstage
 * Referer; the package images on image.mgstage.com are hotlink-protected, so a
 * cover URL is fetched through {@link coverObjectUrl} (the TS replacement for the
 * sidecar's `/img` proxy) which sends the mgstage Referer and hands back a
 * `blob:` URL an <img> can render directly.
 *
 *   fetchMgstage(vr, mode) -> DiscoverItem[]   // ranking (ad) or VR search (vrc)
 *   mgstageCover(code)     -> { url, ar }      // resolve a package cover by code
 *
 * Cover resolution (jav_cover / resolve_covers) is the AGGREGATOR's job; this
 * module only provides the listing parse + the per-code cover probe, matching the
 * Python field conventions exactly.
 */
import type { DiscoverItem } from "@/api/types"
import { coverObjectUrl, httpText, HttpError } from "@/net/http"

/** Age-gate cookie MGStage requires to serve product/listing HTML. */
const MGSTAGE_COOKIE = "adc=1"
/** Referer hotlink-protected image.mgstage.com / product pages expect. */
const MGSTAGE_REFERER = "https://www.mgstage.com/"

/**
 * One product parsed out of a ranking/search page: the product id (as it appears
 * in the URL, e.g. "PRVRSS-007") and the wide-jacket package image URL.
 * Exported so the parser can be unit-tested against a saved fixture (no network).
 */
export interface MgstageListItem {
  /** Product id straight from the URL (case as found, e.g. "300MIUM-1380"). */
  pid: string
  /** image.mgstage.com package image URL (the wide jacket, hotlink-protected). */
  cover: string
}

// Each product block is a /product/product_detail/<pid>/ link followed (within
// ~400 chars) by its image.mgstage.com .jpg cover. Mirrors the Python
// re.finditer pattern in fetch_mgstage verbatim. The `[\s\S]{0,400}?` lazily
// spans the markup between the link and the image. `g` so we can iterate matches.
const PRODUCT_RE =
  /\/product\/product_detail\/([0-9A-Za-z_-]+)\/"[\s\S]{0,400}?(https?:\/\/image\.mgstage\.com\/images\/[^"']+?\.jpg)/g

/**
 * Pure parser: extract the de-duplicated product list (pid + cover) from a
 * ranking or search HTML page. Dedup is by uppercased pid, preserving first-seen
 * order — exactly like the Python `seen` set keyed on `code = pid.upper()`.
 *
 * Exported and side-effect-free so the unit test can run it on a saved fixture.
 */
export function parseMgstageList(html: string): MgstageListItem[] {
  const out: MgstageListItem[] = []
  const seen = new Set<string>()
  // Reset lastIndex defensively (PRODUCT_RE is a module-level /g regex).
  PRODUCT_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = PRODUCT_RE.exec(html)) !== null) {
    const pid = m[1]!
    const cover = m[2]!
    const code = pid.toUpperCase()
    if (seen.has(code)) continue
    seen.add(code)
    out.push({ pid, cover })
  }
  return out
}

/**
 * Build the DiscoverItem for one parsed product. Mirrors the dict the Python
 * fetch_mgstage emits item-for-item: code is the uppercased pid, ar 0.72 for VR
 * else 0.7, sub "VR" for VR else "", `added` is the feed position, and `link`
 * is the product page. `cover` is the proxied wide-jacket blob URL (the Python's
 * `_img(cov)`); the aggregator may later overwrite it with a portrait resolved
 * by {@link mgstageCover}.
 */
function toDiscoverItem(
  item: MgstageListItem,
  index: number,
  vr: boolean,
  coverUrl: string
): DiscoverItem {
  const code = item.pid.toUpperCase()
  return {
    id: `mg_${code}`,
    cat: vr ? "vrc" : "ad",
    title: code,
    sub: vr ? "VR" : "",
    cover: coverUrl,
    ar: vr ? 0.72 : 0.7,
    seeders: 0,
    size: "",
    src: "MGStage",
    state: "new",
    year: "",
    runtime: 0,
    rating: 0,
    code,
    added: index,
    link: `https://www.mgstage.com/product/product_detail/${item.pid}/`,
  }
}

/**
 * Our Discover list ids -> MGStage `ranking.php?id=` window. MGStage offers
 * day / week / month / popular rankings (each ~50 products); `total` exists in
 * the page nav but serves no products, so it is intentionally not mapped.
 * Legacy aliases (trending/newest) kept for defensive fallback.
 */
const MG_RANKING_ID: Record<string, string> = {
  daily: "day",
  weekly: "week",
  monthly: "month",
  popular: "popular",
  trending: "week",
  newest: "day",
}

/** GET an mgstage page with the age cookie + Referer; "" on any failure (Python _mg_get). */
async function mgGet(url: string): Promise<string> {
  try {
    return await httpText(url, {
      cookie: MGSTAGE_COOKIE,
      referer: MGSTAGE_REFERER,
    })
  } catch {
    return ""
  }
}

/**
 * Port of `fetch_mgstage(vr, mode)`.
 *
 * VR (vr=true) -> the VR popular search (cat "vrc"); otherwise the ranking page
 * for the window named by `mode` (see {@link MG_RANKING_ID}: daily/weekly/
 * monthly/popular), defaulting to the weekly ranking. Returns DiscoverItem[] in
 * feed order with the wide-jacket cover routed through {@link coverObjectUrl}
 * (mgstage Referer) — every listed product ships its own jacket, so the
 * aggregator keeps it directly instead of resolving a portrait by code.
 */
export async function fetchMgstage(
  vr: boolean,
  mode: string
): Promise<DiscoverItem[]> {
  const url = vr
    ? "https://www.mgstage.com/search/search.php?search_word=VR&sort=popular&type=top"
    : `https://www.mgstage.com/ranking/ranking.php?id=${MG_RANKING_ID[mode] ?? "week"}`
  const html = await mgGet(url)
  const parsed = parseMgstageList(html)

  // Resolve each wide-jacket cover blob in parallel (independent fetches).
  const items = await Promise.all(
    parsed.map(async (p, i) => {
      let coverUrl = ""
      try {
        coverUrl = await coverObjectUrl(p.cover, { referer: MGSTAGE_REFERER })
      } catch {
        coverUrl = ""
      }
      return toDiscoverItem(p, i, vr, coverUrl)
    })
  )
  return items
}

// ----------------------------------------------------------------- cover by code

/** A resolved cover: the (proxied blob) URL and its aspect ratio (w/h). */
export interface MgstageCover {
  url: string
  ar: number
}

// `mgstage_cover` couldn't measure dimensions here (coverObjectUrl returns a
// blob, not bytes), so VR-vs-flat aspect can't be probed; the package jackets
// are landscape wide jackets, matching the Python's flat default. The Python
// `_cover_meta` returned ~0.72 for portrait probes that succeeded; for the
// by-code package image we use the same conservative default the sidecar fell
// back to so the card layout stays stable.
const MGSTAGE_DEFAULT_AR = 0.72

const MGSTAGE_ID_RE = /^([0-9A-Za-z]+)-?(\d+)$/

/**
 * Port of `mgstage_ids(code)`. MGStage product ids drop leading zeros
 * (PRVRSS-00007 -> PRVRSS-007), so probe the common paddings:
 *   [original code, "LAB-%03d", "LAB-%d", "LAB-<rawnum>"] — de-duplicated,
 *   first-seen order. Falls back to [code] when the code doesn't split.
 *
 * Exported for unit testing.
 */
export function mgstageIds(code: string): string[] {
  const c = code || ""
  const m = MGSTAGE_ID_RE.exec(c)
  if (!m) return [c]
  const lab = m[1]!.toUpperCase()
  const num = m[2]!
  const n = parseInt(num, 10)
  const candidates = [
    c,
    `${lab}-${String(n).padStart(3, "0")}`,
    `${lab}-${n}`,
    `${lab}-${num}`,
  ]
  // dict.fromkeys order-preserving dedup
  return Array.from(new Set(candidates))
}

/** Big-image markers, tried in priority order (Python: pb_e, then pf_o1, then any). */
function pickBigCovers(cands: string[]): string[] {
  const pbE = cands.filter((x) => x.includes("pb_e"))
  if (pbE.length) return pbE
  const pfO1 = cands.filter((x) => x.includes("pf_o1"))
  if (pfO1.length) return pfO1
  return cands
}

const IMG_RE = /https?:\/\/image\.mgstage\.com\/images\/[^"'\s]+?\.jpg/g

/**
 * Pure parser for the product-detail page: pull the package image candidates and
 * pick the best (pb_e > pf_o1 > any), returning up to the first 3. Exported for
 * unit testing without network.
 */
export function parseMgstageCovers(html: string): string[] {
  const cands = html.match(IMG_RE) ?? []
  return pickBigCovers(cands).slice(0, 3)
}

/**
 * Port of `mgstage_cover(code)`.
 *
 * For each padded product id (see {@link mgstageIds}) fetch the product-detail
 * page with the age cookie + Referer. A missing product REDIRECTS to a different
 * path (not a 404); we detect that the way the Python did — by checking the final
 * URL still contains the requested product path — and skip it. From the first
 * surviving page, pick the best package image (pb_e > pf_o1 > any) and route it
 * through {@link coverObjectUrl} with the mgstage Referer (hotlink-protected).
 *
 * Returns { url: "", ar: 0 } when nothing resolves — the "no cover" sentinel the
 * Python returned as `('', 0)`.
 */
export async function mgstageCover(code: string): Promise<MgstageCover> {
  for (const mid of mgstageIds(code)) {
    const path = `/product/product_detail/${mid}/`
    const html = await fetchProductDetail(path)
    if (html === null) continue // request failed or redirected away (not found)
    for (const cov of parseMgstageCovers(html)) {
      try {
        // Validate that the image actually loads (hotlink-protected), but
        // return the RAW image.mgstage.com URL — NOT the blob: URL. The
        // aggregator persists this in cover_cache; a blob: URL would be a
        // dead session-scoped reference in the next app run. It is re-proxied
        // for display each session (the bytes are warm in coverCache here).
        await coverObjectUrl(cov, { referer: MGSTAGE_REFERER })
        return { url: cov, ar: MGSTAGE_DEFAULT_AR }
      } catch {
        // proxy failed for this candidate; try the next
      }
    }
  }
  return { url: "", ar: 0 }
}

/**
 * Fetch a product-detail page. Returns the HTML, or null when the request fails
 * or MGStage redirected the request away from `path` (a missing product redirects
 * to the home page rather than 404). plugin-http follows redirects, so we use the
 * Response URL to detect that the final landing still matches the product path.
 *
 * We need the final URL, which httpText hides, so we issue the request via fetch
 * directly through httpText and fall back to a same-path assumption: because
 * httpText does not surface the redirected URL, we instead detect the redirect by
 * the page NOT containing the product path in its product-detail link/markup.
 */
async function fetchProductDetail(path: string): Promise<string | null> {
  let html: string
  try {
    html = await httpText(`https://www.mgstage.com${path}`, {
      cookie: MGSTAGE_COOKIE,
      referer: MGSTAGE_REFERER,
    })
  } catch (err) {
    // A hard non-2xx (rare here) is a miss, like the Python's bare except.
    if (err instanceof HttpError) return null
    return null
  }
  // Redirect-to-home detection: the genuine product page references its own
  // product path (canonical link / og:url / the detail link); the home page it
  // redirects to does not. This is the behavioural equivalent of the Python
  // `if path not in r.geturl(): continue` guard, since httpText does not expose
  // the final URL.
  if (!html.includes(path)) return null
  return html
}
