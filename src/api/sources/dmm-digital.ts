/**
 * FANZA digital video via the public GraphQL API (api.video.dmm.co.jp). No auth,
 * cookies, or referer needed (see docs/dmm-digital-api.md). Serves two Discover
 * feeds: the digital VR ranking (vrc) and the non-VR digital AV feed (ad,
 * contentType TWO_DIMENSION).
 *
 * Covers are raw awsimgsrc.dmm.co.jp URLs; that CDN isn't hotlink-protected
 * (returns 200 with any/no Referer), so discover() renders them directly in <img>.
 */
import type { Cat, DiscoverItem } from "@/api/types"
import { httpJson } from "@/net/http"
import { dmmCidToCode } from "./dmm"

const DMM_GQL = "https://api.video.dmm.co.jp/graphql"

/** Digital floor content filter: VR titles vs 2D (non-VR) titles. */
type DigitalContentType = "VR" | "TWO_DIMENSION"

/** Wide digital jacket (~1600x1000); CoverImage measures the real ratio. */
const DIGITAL_WIDE_JACKET_AR = 1.6

/** One content node from the digital API (`content.id` is the cid). */
export interface DigitalContent {
  id: string
  title?: string
  packageImage?: { largeUrl?: string } | null
}

interface RankingData {
  ppvContentRanking?: { items?: { content: DigitalContent }[] } | null
}
interface SearchData {
  legacySearchPPV?: { result?: { contents?: DigitalContent[] } | null } | null
}

/** POST a GraphQL query; returns the `data` payload or null on any failure. */
async function gql<T>(query: string, variables?: Record<string, unknown>): Promise<T | null> {
  try {
    const response = await httpJson<{ data?: T }>(DMM_GQL, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        // The DMM WAF 403s any non-dmm Origin (the webview's tauri:// origin);
        // present the SPA's own origin so the request is accepted.
        Origin: "https://video.dmm.co.jp",
        Referer: "https://video.dmm.co.jp/",
      },
      body: JSON.stringify({ query, variables }),
      timeoutMs: 15_000,
    })
    return response?.data ?? null
  } catch {
    return null
  }
}

/**
 * Map digital content nodes to DiscoverItems for `cat` (vrc default, or ad for the
 * non-VR feed). Cover uses the smaller `ps` jacket (the API returns the ~600 KB
 * `pl`; `ps` is about half that, fine for the grid).
 */
export function mapDigitalContents(
  contents: readonly DigitalContent[],
  cat: Cat = "vrc"
): DiscoverItem[] {
  const out: DiscoverItem[] = []
  const seen = new Set<string>()
  for (const content of contents) {
    const cid = content.id || ""
    if (!cid || seen.has(cid)) continue
    seen.add(cid)
    const code = dmmCidToCode(cid)
    const cover = (content.packageImage?.largeUrl || "").replace(/pl\.jpg(\?.*)?$/i, "ps.jpg$1")
    out.push({
      id: "dmm_" + cid,
      cat,
      title: code || cid,
      sub: cat === "vrc" ? "VR" : "",
      cover,
      ar: DIGITAL_WIDE_JACKET_AR, // CoverImage measures the real ratio
      seeders: 0,
      size: "",
      src: "DMM",
      state: "new",
      year: "",
      runtime: 0,
      rating: 0,
      code,
      added: out.length,
      link: `https://video.dmm.co.jp/av/content/?id=${cid}`,
    })
  }
  return out
}

const RANKING_Q = (type: string, contentType: DigitalContentType) =>
  `{ ppvContentRanking(floor: AV, type: ${type}, limit: 100, contentType: ${contentType}) ` +
  `{ items { content { id title packageImage { largeUrl } } } } }`

const SEARCH_Q =
  `query S($limit:Int!,$floor:PPVFloor,$sort:ContentSearchPPVSort!,$filter:ContentSearchPPVFilterInput){` +
  ` legacySearchPPV(limit:$limit,floor:$floor,sort:$sort,filter:$filter,includeExplicit:true){` +
  ` result { contents { id title packageImage { largeUrl } } } } }`

/**
 * Sample/preview images for a digital content id (raw awsimgsrc large URLs). The
 * mono/dvd detail-page scrape (dmmPreviews) can't resolve digital cids like
 * vrkm01577, so the digital VR cards use this instead.
 */
export async function dmmDigitalPreviews(cid: string): Promise<string[]> {
  const sanitizedCid = (cid || "").replace(/[^a-z0-9_]/gi, "")
  if (!sanitizedCid) return []
  const data = await gql<{ ppvContent?: { sampleImages?: { largeImageUrl?: string }[] } | null }>(
    `{ ppvContent(id: "${sanitizedCid}") { sampleImages { largeImageUrl } } }`
  )
  return (data?.ppvContent?.sampleImages ?? [])
    .map((s) => s.largeImageUrl || "")
    .filter(Boolean)
}

/**
 * Digital feed for a Discover list id, for one contentType (VR or 2D) + category.
 * Search axis (legacySearchPPV):
 *   popular   -> RECOMMENDED   (the website's /av/list/?sort=suggest default)
 *   newest    -> RELEASE_DATE  · top_rated -> REVIEW_RANK_SCORE
 * Ranking axis (ppvContentRanking):
 *   trending  -> SALES_BEST_SELLERS · monthly  -> SALES_MONTHLY
 */
async function fetchDmmDigital(
  contentType: DigitalContentType,
  list: string,
  cat: Cat
): Promise<DiscoverItem[]> {
  if (list === "popular" || list === "newest" || list === "top_rated") {
    const sort =
      list === "newest"
        ? "RELEASE_DATE"
        : list === "top_rated"
          ? "REVIEW_RANK_SCORE"
          : "RECOMMENDED" // popular = the website's おすすめ/suggest default
    const searchData = await gql<SearchData>(SEARCH_Q, {
      limit: 100,
      floor: "AV",
      sort,
      filter: { contentType },
    })
    return mapDigitalContents(searchData?.legacySearchPPV?.result?.contents ?? [], cat)
  }
  const type = list === "monthly" ? "SALES_MONTHLY" : "SALES_BEST_SELLERS"
  const rankingData = await gql<RankingData>(RANKING_Q(type, contentType))
  return mapDigitalContents(
    (rankingData?.ppvContentRanking?.items ?? []).map((i) => i.content),
    cat
  )
}

/** Digital VR feed (contentType VR), the vrc FANZA source. */
export function fetchDmmDigitalVr(list: string): Promise<DiscoverItem[]> {
  return fetchDmmDigital("VR", list, "vrc")
}

/** Digital non-VR AV feed (contentType TWO_DIMENSION), the ad FANZA source. */
export function fetchDmmDigitalAv(list: string): Promise<DiscoverItem[]> {
  return fetchDmmDigital("TWO_DIMENSION", list, "ad")
}
