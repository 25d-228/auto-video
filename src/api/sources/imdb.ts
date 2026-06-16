/**
 * IMDb chart source — TypeScript port of the Python sidecar's
 * `fetch_imdb_chart` / `imdb_gql` / `IMDB_SORTS` (sidecar/av_proxy.py).
 *
 * IMDb has no public listing JSON, but its app/web frontend talks to a GraphQL
 * endpoint at https://api.graphql.imdb.com/. The sidecar issues an
 * `advancedTitleSearch` query — the same query the IMDb "Advanced Title Search"
 * page uses — to produce ranked charts (popular / top rated / most voted /
 * newest) for movies (titleType `movie`) or TV (`tvSeries`), and maps each hit
 * to a {@link DiscoverItem}.
 *
 * Field conventions match the Python exactly:
 *   - `cover` = `primaryImage.url` verbatim (m.media-amazon.com is NOT
 *     hotlink-protected, so it is passed through untouched — no /img proxy).
 *   - `link`  = https://www.imdb.com/title/<ttid>/ (only when the id starts
 *     with "tt"; an item with no proper id has no link).
 *   - `rating` = round(aggregateRating, 1).
 *   - items with no `primaryImage.url` are dropped.
 *   - `added` = the item's position in the filtered output.
 *   - `code` = the IMDb tt id (same as the Python `'code': t.get('id')`).
 *   - `date` = "" (IMDb gives no full release date here).
 *
 * Caching mirrors the sidecar's `listing_cached(... LIST_TTL=300)`: a fetched
 * chart is cached in the SQLite `listing_cache` table for {@link LIST_TTL_SEC}
 * seconds, keyed by cat+sort, and degrades gracefully when the DB is
 * unavailable (non-Tauri host) or the network fails.
 */
import { httpJson } from "@/net/http"
import { getCached, isDbAvailable, setCached } from "@/state/db"
import type { Cat, DiscoverItem } from "@/api/types"

/** IMDb GraphQL endpoint (POST a `{query}` JSON body). */
export const IMDB_GRAPHQL_URL = "https://api.graphql.imdb.com/"

/** Listing cache TTL in seconds — matches the sidecar's LIST_TTL. */
export const LIST_TTL_SEC = 300

/** advancedTitleSearch page size (the `first:` arg in the GraphQL query). */
const IMDB_SEARCH_FIRST = 60

/** Sort ids accepted by {@link fetchImdbChart} (mirrors the sidecar UI sorts). */
export type ImdbSort = "popular" | "top_rated" | "most_voted" | "newest"

/** One IMDb advancedTitleSearch sort: GraphQL sortBy + sortOrder + min-votes floor. */
interface ImdbSortSpec {
  sortBy: string
  sortOrder: "ASC" | "DESC"
  /** Minimum user-rating count; 0 means "no constraint". */
  minVotes: number
}

/**
 * IMDb advancedTitleSearch sort + constraint per chart id. `top_rated` adds a
 * min-votes floor so it surfaces well-known titles, not obscure
 * 9.9/10-with-12-votes entries. Verbatim from the Python `IMDB_SORTS`.
 */
const IMDB_SORTS: Record<ImdbSort, ImdbSortSpec> = {
  popular: { sortBy: "POPULARITY", sortOrder: "ASC", minVotes: 0 },
  top_rated: { sortBy: "USER_RATING", sortOrder: "DESC", minVotes: 25000 },
  most_voted: { sortBy: "USER_RATING_COUNT", sortOrder: "DESC", minVotes: 0 },
  newest: { sortBy: "RELEASE_DATE", sortOrder: "DESC", minVotes: 0 },
}

// ----------------------------------------------------------------- GraphQL shapes

/** Minimal shape of the advancedTitleSearch response we read. */
export interface ImdbTitleNode {
  id?: string | null
  titleText?: { text?: string | null } | null
  releaseYear?: { year?: number | null } | null
  primaryImage?: { url?: string | null } | null
  ratingsSummary?: { aggregateRating?: number | null } | null
}

interface ImdbEdge {
  node?: { title?: ImdbTitleNode | null } | null
}

export interface ImdbGqlResponse {
  data?: {
    advancedTitleSearch?: {
      edges?: (ImdbEdge | null)[] | null
    } | null
  } | null
}

// ----------------------------------------------------------------- query builder

/**
 * Build the GraphQL query string for one chart. Port of the f-string the
 * sidecar builds in `fetch_imdb_chart` (first:60, the sort, the title-type
 * constraint, and an optional userRatingsConstraint when minVotes > 0).
 * Exported so the test can assert the query is well-formed without a network
 * call.
 */
export function buildImdbQuery(cat: Cat, sort: ImdbSort = "popular"): string {
  const titleType = cat === "mov" ? "movie" : "tvSeries"
  const spec = IMDB_SORTS[sort] ?? IMDB_SORTS.popular
  const extra = spec.minVotes
    ? `,userRatingsConstraint:{ratingsCountRange:{min:${spec.minVotes}}}`
    : ""
  return (
    "query{advancedTitleSearch(first:" +
    IMDB_SEARCH_FIRST +
    ",sort:{sortBy:" +
    spec.sortBy +
    ",sortOrder:" +
    spec.sortOrder +
    "}," +
    'constraints:{titleTypeConstraint:{anyTitleTypeIds:["' +
    titleType +
    '"]}' +
    extra +
    "}){edges{node{title{" +
    "id titleText{text} releaseYear{year} primaryImage{url} ratingsSummary{aggregateRating}}}}}}"
  )
}

// ----------------------------------------------------------------- parser

/** Round to one decimal place (mirrors Python `round(x, 1)`). */
function round1(n: number): number {
  return Math.round(n * 10) / 10
}

/**
 * Parse a raw advancedTitleSearch response into {@link DiscoverItem}s. Pure (no
 * network / no DB) — the unit test runs this against the recorded fixture.
 *
 * Faithful to the sidecar's loop: skip nodes with no `primaryImage.url`; the
 * IMDb link is only attached when the id starts with "tt"; `added` is the item's
 * index in the surviving output.
 */
export function parseImdbChart(json: ImdbGqlResponse, cat: Cat): DiscoverItem[] {
  const edges = json?.data?.advancedTitleSearch?.edges ?? []
  const out: DiscoverItem[] = []
  for (const edge of edges) {
    const title = edge?.node?.title
    if (!title) continue
    const imageUrl = title.primaryImage?.url ?? ""
    if (!imageUrl) continue // Python: `if not img: continue`
    const year = title.releaseYear?.year ?? ""
    const yearText = year === "" ? "" : String(year)
    const titleId = (title.id ?? "").trim()
    const rating = round1(title.ratingsSummary?.aggregateRating ?? 0)
    const item: DiscoverItem = {
      id: `imdb_${title.id ?? ""}`,
      cat,
      title: title.titleText?.text ?? "",
      sub: yearText,
      cover: imageUrl,
      ar: 0.675,
      seeders: 0,
      size: "",
      src: "IMDb",
      state: "new",
      year: yearText,
      runtime: 0,
      rating,
      code: title.id ?? "",
      date: "",
      added: out.length,
    }
    if (titleId.startsWith("tt")) {
      item.link = `https://www.imdb.com/title/${titleId}/`
    }
    out.push(item)
  }
  return out
}

// ----------------------------------------------------------------- fetch

/** POST the GraphQL query to IMDb and return the parsed JSON response. */
export async function imdbGql(query: string): Promise<ImdbGqlResponse> {
  return httpJson<ImdbGqlResponse>(IMDB_GRAPHQL_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query }),
  })
}

/**
 * Fetch one IMDb chart and return it as {@link DiscoverItem}[].
 *
 * Port of `fetch_imdb_chart(cat, sort)`. Results are cached in `listing_cache`
 * for {@link LIST_TTL_SEC} seconds (keyed by cat+sort), mirroring the sidecar's
 * `listing_cached`. Pass `fresh: true` to bypass the cache (force a refetch).
 *
 * Network/parse failures resolve to `[]` (the sidecar returned [] on any error),
 * and the empty result is NOT cached so a transient failure self-heals.
 */
export async function fetchImdbChart(
  cat: Cat,
  sort: ImdbSort = "popular",
  opts: { fresh?: boolean } = {}
): Promise<DiscoverItem[]> {
  const cacheKey = `imdb:${cat}:${sort}`
  const dbOn = isDbAvailable()

  if (dbOn && !opts.fresh) {
    const cached = await getCached<DiscoverItem[]>(
      "listing_cache",
      cacheKey,
      LIST_TTL_SEC
    )
    if (cached) return cached
  }

  let items: DiscoverItem[] = []
  try {
    const json = await imdbGql(buildImdbQuery(cat, sort))
    items = parseImdbChart(json, cat)
  } catch {
    items = []
  }

  if (dbOn && items.length > 0) {
    try {
      await setCached("listing_cache", cacheKey, items)
    } catch {
      // best-effort cache write; ignore (e.g. DB write race)
    }
  }
  return items
}
