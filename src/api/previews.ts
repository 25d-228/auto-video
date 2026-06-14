/**
 * Preview (sample) images for a Discover item — the strip of screenshots the
 * source sites show on a product page. Lazily fetched when the detail panel
 * opens (NOT for Library items). Supported sources:
 *   - javdb:   the detail payload's preview_images (tp.cmastd.com)
 *   - dmm:     the FANZA detail page (pics.dmm.co.jp/.../<cid>-N.jpg)
 *   - mgstage: the product detail page (image.mgstage.com/.../cap_e_N_<code>.jpg)
 * Every image is hotlink-protected (and cmastd is XOR-encrypted), so each raw
 * URL is routed through {@link coverObjectUrl} (referer + cmastd decode) into a
 * displayable blob: URL.
 */
import { coverObjectUrl } from "@/net/http"
import { dmmPreviews } from "@/api/sources/dmm"
import { dmmDigitalPreviews } from "@/api/sources/dmm-digital"
import { javdbPreviews, javdbSearch } from "@/api/sources/javdb"
import { mgstagePreviews } from "@/api/sources/mgstage"
import type { DiscoverItem } from "@/api/types"

/** Cap so a detail-open never fetches an unbounded image set. */
const MAX_PREVIEWS = 24

/**
 * True when a Discover source exposes sample/preview images. javdb/dmm/mgstage
 * carry them directly; sukebei (a torrent index) has none of its own, but its
 * items are coded JAV so we resolve previews by code via JavDB.
 */
export function hasPreviews(src: string): boolean {
  return ["javdb", "dmm", "mgstage", "sukebei"].includes((src || "").toLowerCase())
}

/** JavDB preview images for a printed code (search -> slug -> preview_images). */
async function previewsByCode(code: string): Promise<string[]> {
  const slug = await javdbSearch(code)
  return slug ? javdbPreviews(slug) : []
}

/** Raw (pre-proxy) preview URLs for an item, dispatched by source. */
async function rawPreviews(item: DiscoverItem): Promise<string[]> {
  switch ((item.src || "").toLowerCase()) {
    case "javdb":
      return javdbPreviews(item.id)
    case "dmm": {
      const cid = item.id.replace(/^dmm_/, "")
      // VR = digital cids (vrkm…) -> GraphQL sampleImages; Adult = mono/dvd scrape.
      return item.cat === "vrc" ? dmmDigitalPreviews(cid) : dmmPreviews(cid)
    }
    case "mgstage":
      return mgstagePreviews(item.code || item.title)
    case "sukebei":
      // No native previews — borrow JavDB's by the parsed code.
      return item.code ? previewsByCode(item.code) : []
    default:
      return []
  }
}

/**
 * Resolve an item's preview images to displayable blob: URLs (capped, parallel,
 * failures dropped). Returns [] for unsupported sources or on any failure.
 */
export async function fetchPreviews(item: DiscoverItem | null): Promise<string[]> {
  if (!item) return []
  const raw = (await rawPreviews(item)).slice(0, MAX_PREVIEWS)
  const resolved = await Promise.all(
    raw.map((u) => coverObjectUrl(u).catch(() => ""))
  )
  return resolved.filter(Boolean)
}
