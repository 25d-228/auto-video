/**
 * FANZA / DMM source — TypeScript port of the Python sidecar's DMM scraper
 * (sidecar/av_proxy.py: fetch_dmm, dmm_cover, dmm_cid_to_code, dmm_cid_variants,
 * img_dims, _cover_meta, _dmm_get).
 *
 * The FANZA digital/videoa floor is now a JS SPA with no server-rendered
 * products, so we scrape the physical-video floor www.dmm.co.jp/mono/dvd which
 * still serves full HTML listings (same adult catalog, same pics.dmm.co.jp
 * covers). The age gate passes with `age_check_done=1; ckcy=1` cookies and a
 * dmm.co.jp Referer. VR = the mono "VR動画" keyword section (article=keyword/id=6793).
 *
 *   fetchDmm(vr, list)  -> DiscoverItem[]  (cover resolved via coverObjectUrl)
 *   parseDmmList(html, vr) -> DmmListItem[]  (pure parser, raw pics URLs)
 *   dmmCover(code) -> { url, ar }  (cid variants × floors × suffixes, placeholder-rejected)
 *
 * Covers from pics.dmm.co.jp are hotlink-protected (need a dmm.co.jp Referer), so
 * every cover is fetched through {@link coverObjectUrl} which hands back a `blob:`
 * URL an <img> can render directly (replacing the sidecar's /img passthrough).
 */
import type { DiscoverItem } from "@/api/types"
import { httpBytes, httpText } from "@/net/http"

/** Sort token presented to the user, mapped onto the FANZA mono/dvd `sort=` param. */
export type DmmList = "trending" | "newest" | "top_rated"

/** FANZA mono "VR動画" keyword id (server-rendered VR listing). */
const DMM_VR_KEYWORD = "6793"

/** Cookie that satisfies the FANZA age gate. */
const DMM_AGE_COOKIE = "age_check_done=1; ckcy=1"

/** Referer every dmm.co.jp / pics.dmm.co.jp request needs. */
const DMM_REFERER = "https://www.dmm.co.jp/"

/**
 * FANZA mono/dvd sort tokens (verified off the page's 並び替え selector):
 *   ranking=人気順 (trending) · date=発売日順 (newest) · review_rank=評価順 (top_rated).
 * Unknown values fall back to `ranking`, matching the Python `.get(mode, 'ranking')`.
 */
const DMM_SORTS: Record<DmmList, string> = {
  trending: "ranking",
  newest: "date",
  top_rated: "review_rank",
}

function dmmSort(list: string): string {
  return (DMM_SORTS as Record<string, string>)[list] ?? "ranking"
}

// ---------------------------------------------------------------- cid → code

/**
 * Port of `dmm_cid_to_code(cid)`.
 *
 * The FANZA cid carries maker noise glued before the real LABEL+NUMBER
 * (k9snos258, tkipzz855, n_1428ss154, n_709maraa244tk, ovvr616). Peel it to a
 * best-effort printed code (display title only; the cover is taken verbatim
 * from the page so an imperfect code never breaks the image). Returns "" when
 * no trailing alpha-run + number can be found.
 */
export function dmmCidToCode(cid: string): string {
  let cid_ = (cid || "").toLowerCase()
  cid_ = cid_.replace(/^n_\d+/, "") // n_NNNN maker prefix
  cid_ = cid_.replace(/(btk|tk)$/, "") // trailing media tag
  const m = /([a-z]+)(\d+)$/.exec(cid_) // last alpha-run + trailing number
  if (!m) return ""
  let label = m[1]!
  let number = m[2]!
  for (const head of ["k9", "c9", "tk", "tn"]) {
    // peel a glued 2-char maker head if a real label remains
    if (label.startsWith(head) && label.length - head.length >= 3 && label.length - head.length <= 6) {
      label = label.slice(head.length)
      break
    }
  }
  number = number.replace(/^0+/, "") || "0"
  if (number.length < 3) number = number.padStart(3, "0")
  return label.toUpperCase() + "-" + number
}

// ---------------------------------------------------------------- cid variants (cover probing)

/** printed-label → FANZA cid label (extend over time). Port of DMM_ALIAS. */
const DMM_ALIAS: Record<string, string> = { ebon: "ebod" }

/**
 * Known FANZA maker prefixes (the h_NNNN before the label) that are NOT derivable
 * from the code. Small maintained table. Port of DMM_PREFIX.
 */
const DMM_PREFIX: Record<string, string> = {
  ccvr: "h_1270",
  devr: "h_1711",
  clot: "h_237",
}

/**
 * Port of `dmm_cid_variants(code)`. Builds the candidate FANZA cid strings to
 * probe for a cover, given a printed code like "ABCD-123" (a leading digit on
 * the label is allowed, e.g. 3DSVR). Returns a de-duplicated list (insertion
 * order preserved).
 */
export function dmmCidVariants(code: string): string[] {
  const m = /^(\d*[A-Za-z]+)-?(\d+)$/.exec(code || "") // allow leading digit (3DSVR)
  if (!m) return []
  let lab = m[1]!.toLowerCase()
  const num = m[2]!
  lab = DMM_ALIAS[lab] ?? lab
  const out = [
    lab + zfill(num, 5),
    lab + zfill(num, 3),
    lab + num,
    "1" + lab + zfill(num, 5),
    "1" + lab + zfill(num, 3), // FANZA prepends 1 to e.g. 3DSVR
    "13" + lab + zfill(num, 5),
    "13" + lab + zfill(num, 3), // FANZA VR: DSVR -> 13dsvr01911
  ]
  const pre = DMM_PREFIX[lab]
  if (pre) {
    // known maker prefix (h_NNNN) takes priority
    out.unshift(pre + lab + zfill(num, 5), pre + lab + zfill(num, 3))
  }
  return [...new Set(out)]
}

/** Python `str.zfill(width)` for a numeric string (left-pad with zeros). */
function zfill(s: string, width: number): string {
  return s.padStart(width, "0")
}

// ---------------------------------------------------------------- image dimensions

/** Read a big-endian u16 from `b` at offset `i`. */
function be16(b: Uint8Array, i: number): number {
  return (b[i]! << 8) | b[i + 1]!
}

/** Read a big-endian u32 from `b` at offset `i`. */
function be32(b: Uint8Array, i: number): number {
  return ((b[i]! << 24) | (b[i + 1]! << 16) | (b[i + 2]! << 8) | b[i + 3]!) >>> 0
}

/** Read a little-endian u16 from `b` at offset `i`. */
function le16(b: Uint8Array, i: number): number {
  return b[i]! | (b[i + 1]! << 8)
}

/**
 * Port of `img_dims(b)` — sniff (width, height) from the leading bytes of a
 * JPEG / PNG / WEBP image. Returns null when the format is unrecognized or the
 * buffer is too short. Exported for the parser test.
 */
export function imgDims(b: Uint8Array): [number, number] | null {
  if (!b || b.length < 24) return null
  // JPEG: scan SOF markers
  if (b[0] === 0xff && b[1] === 0xd8) {
    let i = 2
    const len = b.length
    while (i < len - 9) {
      if (b[i] !== 0xff) {
        i += 1
        continue
      }
      const marker = b[i + 1]!
      if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
        return [be16(b, i + 7), be16(b, i + 5)]
      }
      const segLen = be16(b, i + 2)
      i += 2 + segLen
    }
    return null
  }
  // PNG
  if (
    b[0] === 0x89 &&
    b[1] === 0x50 &&
    b[2] === 0x4e &&
    b[3] === 0x47 &&
    b[4] === 0x0d &&
    b[5] === 0x0a &&
    b[6] === 0x1a &&
    b[7] === 0x0a
  ) {
    return [be32(b, 16), be32(b, 20)]
  }
  // WEBP: "RIFF"...."WEBP"
  if (
    b[0] === 0x52 &&
    b[1] === 0x49 &&
    b[2] === 0x46 &&
    b[3] === 0x46 &&
    b[8] === 0x57 &&
    b[9] === 0x45 &&
    b[10] === 0x42 &&
    b[11] === 0x50
  ) {
    const tag = String.fromCharCode(b[12]!, b[13]!, b[14]!, b[15]!)
    if (tag === "VP8X") {
      return [
        1 + (b[24]! | (b[25]! << 8) | (b[26]! << 16)),
        1 + (b[27]! | (b[28]! << 8) | (b[29]! << 16)),
      ]
    }
    if (tag === "VP8 ") {
      return [le16(b, 26) & 0x3fff, le16(b, 28) & 0x3fff]
    }
    if (tag === "VP8L") {
      const bits = (b[21]! | (b[22]! << 8) | (b[23]! << 16) | (b[24]! << 24)) >>> 0
      return [(bits & 0x3fff) + 1, ((bits >>> 14) & 0x3fff) + 1]
    }
  }
  return null
}

/**
 * Port of `_cover_meta(b)` — given fetched image bytes, decide whether this is a
 * real cover (rejecting FANZA "now printing" / ps placeholders) and compute its
 * aspect ratio (w/h). Returns null when it should be rejected.
 *
 * Split out from the fetch so it is unit-testable. The Python predicate:
 *   - reject bodies < 6000 bytes (the ps placeholder is ~3.4 KB)
 *   - reject the 590×800 "now printing" placeholder (~19 KB)
 *   - otherwise ar = round(w/h, 3), defaulting to 0.72 when dims are unknown
 */
export function coverMeta(bytes: Uint8Array): { ar: number } | null {
  if (!bytes || bytes.length < 6000) return null
  const d = imgDims(bytes)
  if (d && d[0] === 590 && d[1] === 800) return null
  const ar = d && d[1] ? round3(d[0] / d[1]) : 0.72
  return { ar }
}

/**
 * Fetch one pics.dmm.co.jp cover and return a displayable `blob:` URL plus its
 * true aspect ratio (measured from the bytes, so the card box matches the
 * jacket and nothing is cropped). Returns null on any failure or when the body
 * isn't a real cover — {@link coverMeta} rejects the tiny "ps" placeholder
 * (< 6000 bytes) and the 590×800 "now printing" graphic, so coverless titles
 * resolve to null here and get dropped by {@link fetchDmm} (no blank cards).
 */
async function dmmBlobCover(
  url: string
): Promise<{ url: string; ar: number } | null> {
  try {
    const bytes = await httpBytes(url, { referer: DMM_REFERER })
    const meta = coverMeta(bytes)
    if (!meta) return null
    const blob = new Blob([bytes as BlobPart], { type: "image/jpeg" })
    return { url: URL.createObjectURL(blob), ar: meta.ar }
  } catch {
    return null
  }
}

/** Python `round(x, 3)` (round-half-to-even is not required for these inputs). */
function round3(x: number): number {
  return Math.round(x * 1000) / 1000
}

// ---------------------------------------------------------------- cover resolution

/** studio video AND amateur floor (POW etc.). Port of DMM_FLOORS. */
const DMM_FLOORS = ["digital/video", "digital/amateur"] as const
/** front portrait, amateur jacket, wide jacket. Port of DMM_SUFFIXES. */
const DMM_SUFFIXES = ["ps", "jp", "pl"] as const

/** A resolved cover: the raw pics.dmm.co.jp URL plus its aspect ratio. */
export interface DmmCover {
  /** Raw pics.dmm.co.jp URL ("" when no cover resolved). */
  url: string
  /** Aspect ratio (w/h); 0 when no cover resolved. */
  ar: number
}

/**
 * Port of `dmm_cover(code)`. Derives the FANZA cid candidates from a printed
 * code, then probes every (cid × floor × suffix) combination on pics.dmm.co.jp,
 * returning the first that fetches a real (non-placeholder) image. Returns the
 * RAW pics URL (callers route it through {@link coverObjectUrl} for display, as
 * the sidecar did via /img).
 *
 * Returns `{ url: "", ar: 0 }` when nothing resolves.
 */
export async function dmmCover(code: string): Promise<DmmCover> {
  for (const cid of dmmCidVariants(code)) {
    for (const floor of DMM_FLOORS) {
      for (const suf of DMM_SUFFIXES) {
        const url = `https://pics.dmm.co.jp/${floor}/${cid}/${cid}${suf}.jpg`
        let bytes: Uint8Array
        try {
          bytes = await httpBytes(url, { referer: DMM_REFERER, timeoutMs: 10_000 })
        } catch {
          continue
        }
        const meta = coverMeta(bytes)
        if (meta) return { url, ar: meta.ar }
      }
    }
  }
  return { url: "", ar: 0 }
}

// ---------------------------------------------------------------- listing parse

/** One parsed product cell from the FANZA mono/dvd listing (raw, pre-cover-resolution). */
export interface DmmListItem {
  /** FANZA content id from the detail link (e.g. "k9snos258"). */
  cid: string
  /** Best-effort printed code from {@link dmmCidToCode} (e.g. "SNOS-258"). */
  code: string
  /** Absolute pics.dmm.co.jp cover URL (https:-prefixed). */
  coverUrl: string
}

// each product cell: a /detail/=/cid=XXX/ link followed by its pics.dmm.co.jp ps.jpg cover.
const PRODUCT_RE = /\/detail\/=\/cid=([a-z0-9_]+)\/"[\s\S]{0,400}?(\/\/pics\.dmm\.co\.jp\/[^"' ]+?\.jpg)/g

/**
 * Pure parser: extract product cells from a FANZA mono/dvd listing page.
 * Port of the parsing loop inside `fetch_dmm`.
 *
 * Dedups by derived code (FANZA lists the same title in several media editions
 * -> same code, different cid like n_707mbdd2190 / ...b / ...btk); skips cells
 * whose code won't parse. No network — feed it the HTML to test offline.
 */
export function parseDmmList(html: string): DmmListItem[] {
  const out: DmmListItem[] = []
  const seen = new Set<string>()
  if (!html) return out
  PRODUCT_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = PRODUCT_RE.exec(html)) !== null) {
    const cid = m[1]!
    const code = dmmCidToCode(cid)
    if (!code) continue
    if (seen.has(code)) continue
    seen.add(code)
    let cov = m[2]!
    if (cov.startsWith("//")) cov = "https:" + cov
    out.push({ cid, code, coverUrl: cov })
  }
  return out
}

/** Build the DiscoverItem skeleton for one parsed cell (cover not yet resolved). */
function toDiscoverItem(it: DmmListItem, vr: boolean, added: number): DiscoverItem {
  return {
    id: "dmm_" + it.cid,
    cat: vr ? "vrc" : "ad",
    title: it.code || it.cid,
    sub: vr ? "VR" : "",
    cover: "", // filled in fetchDmm
    ar: 1.48, // wide jacket (pl); overwritten with the measured ratio in fetchDmm
    seeders: 0,
    size: "",
    src: "DMM",
    state: "new",
    year: "",
    runtime: 0,
    rating: 0,
    code: it.code,
    added,
    link: `https://www.dmm.co.jp/mono/dvd/-/detail/=/cid=${it.cid}/`, // FANZA detail page
  }
}

/** Build the listing URL for a (vr, sort) combination. Exported for testing. */
export function dmmListUrl(vr: boolean, list: string): string {
  const sort = dmmSort(list)
  if (vr) {
    return `https://www.dmm.co.jp/mono/dvd/-/list/=/article=keyword/id=${DMM_VR_KEYWORD}/sort=${sort}/`
  }
  return `https://www.dmm.co.jp/mono/dvd/-/list/=/sort=${sort}/`
}

/**
 * Port of `fetch_dmm(vr, mode)` → DiscoverItem[].
 *
 * Scrapes the FANZA mono/dvd floor (VR keyword section when `vr`), parses the
 * server-rendered product cells, then resolves each cover through
 * {@link coverObjectUrl} (verbatim off the listing page — no per-code cover
 * derivation, matching the sidecar's "no resolve_covers pass" for DMM). Items
 * whose cover fails to load are dropped (the sidecar keeps only items with a
 * cover). Covers are resolved in parallel.
 */
export async function fetchDmm(vr: boolean, list: DmmList): Promise<DiscoverItem[]> {
  const url = dmmListUrl(vr, list)
  let html: string
  try {
    html = await httpText(url, {
      referer: DMM_REFERER,
      cookie: DMM_AGE_COOKIE,
      timeoutMs: 20_000,
    })
  } catch {
    return []
  }
  if (!html || html.length < 4000) return []

  const parsed = parseDmmList(html)
  const items = parsed.map((it, i) => ({ item: toDiscoverItem(it, vr, i), coverUrl: it.coverUrl }))

  // Resolve covers in parallel; drop any whose fetch fails (kept items must have
  // a cover, mirroring the sidecar's `[x for x in data if x.get('cover')]`). The
  // listing cells carry the narrow portrait `ps` thumbnail; prefer the wide `pl`
  // jacket (front+back, ~1.49) so DMM matches the other adult sources, falling
  // back to `ps` when a title has no `pl`. The true aspect ratio is measured
  // from the fetched bytes so the card box matches the cover (no crop).
  const resolveCover = async (coverUrl: string): Promise<{ url: string; ar: number } | null> => {
    const plUrl = coverUrl.replace(/ps(\.jpe?g)(\?.*)?$/i, "pl$1$2")
    return (
      (plUrl !== coverUrl ? await dmmBlobCover(plUrl) : null) ??
      (await dmmBlobCover(coverUrl))
    )
  }
  await Promise.all(
    items.map(async ({ item, coverUrl }) => {
      const got = await resolveCover(coverUrl)
      if (got) {
        item.cover = got.url
        item.ar = got.ar
      } else {
        item.cover = ""
      }
    })
  )

  // Re-number `added` over the survivors so feed positions stay contiguous.
  const kept = items.filter(({ item }) => item.cover).map(({ item }) => item)
  kept.forEach((item, i) => {
    item.added = i
  })
  return kept
}

// ---------------------------------------------------------------- preview images

/**
 * Pure parser: pull a FANZA detail page's sample-image gallery, de-duped and
 * ordered by N, upgraded to the large variant.
 *
 * Samples live at `pics.dmm.co.jp/.../<sampleCid>/<sampleCid>-N.jpg` — note the
 * sample cid is NOT the listing cid for physical products (mono `k9snos258` →
 * samples under the digital cid `snos00258`). Related items on the page only
 * show a cover (no `-N` gallery), so we match the dir==filename-prefix gallery
 * pattern and keep the group for the listing `cid` if present, else the largest
 * gallery (the page's main product). Each thumbnail (`<cid>-N.jpg`, ~120×90) is
 * upgraded to the full sample (`<cid>jp-N.jpg`, ~600×800).
 */
export function parseDmmPreviews(html: string, cid: string): string[] {
  if (!html) return []
  const re =
    /(?:https?:)?\/\/pics\.dmm\.co\.jp\/[^"' ]*?\/([a-z0-9_]+)\/\1-(\d+)\.jpg/gi
  const groups = new Map<string, { url: string; n: number }[]>()
  let m: RegExpExecArray | null
  while ((m = re.exec(html)) !== null) {
    const sampleCid = m[1]!.toLowerCase()
    const n = parseInt(m[2]!, 10)
    let url = m[0]
    if (url.startsWith("//")) url = "https:" + url
    url = url.replace(/^http:/, "https:")
    const arr = groups.get(sampleCid) ?? []
    if (!arr.some((x) => x.n === n)) arr.push({ url, n })
    groups.set(sampleCid, arr)
  }
  if (groups.size === 0) return []
  const chosen =
    groups.get((cid || "").toLowerCase()) ??
    [...groups.values()].sort((a, b) => b.length - a.length)[0]!
  return chosen
    .sort((a, b) => a.n - b.n)
    .map((x) => x.url.replace(/-(\d+)\.jpg$/i, "jp-$1.jpg"))
}

/** Fetch a FANZA product's sample images (raw pics URLs). `cid` from the listing. */
export async function dmmPreviews(cid: string): Promise<string[]> {
  if (!cid) return []
  try {
    const html = await httpText(
      `https://www.dmm.co.jp/mono/dvd/-/detail/=/cid=${cid}/`,
      { referer: DMM_REFERER, cookie: DMM_AGE_COOKIE, timeoutMs: 15_000 }
    )
    return parseDmmPreviews(html, cid)
  } catch {
    return []
  }
}
