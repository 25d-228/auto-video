/**
 * FANZA digital video rankings via the public GraphQL API (api.video.dmm.co.jp).
 * No auth / cookies / referer needed (see docs/dmm-digital-api.md). This is the
 * ONLY source of a real digital VR ranking — the scrapable mono/dvd VR floor
 * (fetchDmm) only carries obscure physical discs (ovvr…), while this returns the
 * popular streaming VR titles (vrkm/sivr/dsvr…).
 *
 * Covers are raw awsimgsrc.dmm.co.jp URLs (hotlink-protected → a dmm Referer,
 * derived automatically by coverObjectUrl). discover() proxies them to blob:
 * URLs AFTER the listing cache, so SQLite keeps the raw URLs.
 */
import type { DiscoverItem } from "@/api/types"
import { httpJson } from "@/net/http"
import { dmmCidToCode } from "./dmm"

const DMM_GQL = "https://api.video.dmm.co.jp/graphql"

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
    const r = await httpJson<{ data?: T }>(DMM_GQL, {
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
    return r?.data ?? null
  } catch {
    return null
  }
}

/**
 * Map digital content nodes to VR DiscoverItems. Cover = the smaller `ps` jacket
 * (the API hands back the ~600 KB `pl`; `ps` is ~half that, fine for the grid).
 * Pure (no network) so it can be unit-tested.
 */
export function mapDigitalContents(contents: readonly DigitalContent[]): DiscoverItem[] {
  const out: DiscoverItem[] = []
  const seen = new Set<string>()
  for (const c of contents) {
    const cid = c.id || ""
    if (!cid || seen.has(cid)) continue
    seen.add(cid)
    const code = dmmCidToCode(cid)
    const cover = (c.packageImage?.largeUrl || "").replace(/pl\.jpg(\?.*)?$/i, "ps.jpg$1")
    out.push({
      id: "dmm_" + cid,
      cat: "vrc",
      title: code || cid,
      sub: "VR",
      cover,
      ar: 1.6, // wide digital jacket (~1600x1000); CoverImage measures the real ratio
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

const RANKING_Q = (type: string) =>
  `{ ppvContentRanking(floor: AV, type: ${type}, limit: 100, contentType: VR) ` +
  `{ items { content { id title packageImage { largeUrl } } } } }`

const SEARCH_Q =
  `query S($limit:Int!,$floor:PPVFloor,$sort:ContentSearchPPVSort!,$filter:ContentSearchPPVFilterInput){` +
  ` legacySearchPPV(limit:$limit,floor:$floor,sort:$sort,filter:$filter,includeExplicit:true){` +
  ` result { contents { id title packageImage { largeUrl } } } } }`

/**
 * Digital VR feed for a Discover list id:
 *   trending  -> SALES_BEST_SELLERS · monthly -> SALES_MONTHLY   (ppvContentRanking)
 *   newest    -> RELEASE_DATE       · top_rated -> REVIEW_RANK_SCORE (legacySearchPPV)
 */
export async function fetchDmmDigitalVr(list: string): Promise<DiscoverItem[]> {
  if (list === "newest" || list === "top_rated") {
    const sort = list === "newest" ? "RELEASE_DATE" : "REVIEW_RANK_SCORE"
    const d = await gql<SearchData>(SEARCH_Q, {
      limit: 100,
      floor: "AV",
      sort,
      filter: { contentType: "VR" },
    })
    return mapDigitalContents(d?.legacySearchPPV?.result?.contents ?? [])
  }
  const type = list === "monthly" ? "SALES_MONTHLY" : "SALES_BEST_SELLERS"
  const d = await gql<RankingData>(RANKING_Q(type))
  return mapDigitalContents((d?.ppvContentRanking?.items ?? []).map((i) => i.content))
}
