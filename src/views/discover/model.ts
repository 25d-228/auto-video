/**
 * Discover view domain helpers, ported from the old vanilla engine
 * (git show HEAD:ui-src/engine.js — DISC_SOURCES, ingestDisc, score,
 * sortList, ownedKeys, cidOf). Pure functions only; React lives in the
 * sibling components.
 */
import type { Cat, DiscoverItem, LibraryItem } from "@/api/types"
import type { ChipOption, MediaState } from "@/components/media"
import type { DownloadEntry } from "@/state/downloads"

export type Rank = "popularity" | "seeders" | "recency" | "rating"

export const CAT_OPTIONS: readonly ChipOption<Cat>[] = [
  { value: "mov", label: "Movies" },
  { value: "tv", label: "TV" },
  { value: "ad", label: "Adult" },
  { value: "vrc", label: "VR" },
]

/**
 * The Discover catalog — the single source of truth for which providers and
 * lists each category offers, and in what order. The order here defines the
 * DEFAULTS (the first provider is the category default, and each provider's
 * first list id is its default) and the fallback resolution in
 * src/api/discover.ts — the dropdown menus themselves are rendered
 * alphabetically by label (see providersFor/listsFor).
 */
export type ProviderId =
  | "tmdb"
  | "imdb"
  | "yts"
  | "tpb"
  | "javdb"
  | "dmm"
  | "mgstage"
  | "sukebei"

export type ListId =
  | "trending"
  | "popular"
  | "top_rated"
  | "now_playing"
  | "upcoming"
  | "airing"
  | "most_voted"
  | "newest"
  | "most_seeded"
  | "weekly"
  | "monthly"
  | "daily"
  | "most_downloaded"

/** Human labels for provider ids. */
export const PROVIDER_LABELS: Record<ProviderId, string> = {
  tmdb: "TMDB",
  imdb: "IMDb",
  yts: "YTS",
  tpb: "Pirate Bay",
  javdb: "JavDB",
  dmm: "DMM / FANZA",
  mgstage: "MGStage",
  sukebei: "Sukebei",
}

/** Human labels for list ids. */
export const LIST_LABELS: Record<ListId, string> = {
  trending: "Trending",
  popular: "Popular",
  top_rated: "Top Rated",
  now_playing: "Now Playing",
  upcoming: "Upcoming",
  airing: "Airing",
  most_voted: "Most Voted",
  newest: "Newest",
  most_seeded: "Most Seeded",
  weekly: "Weekly",
  monthly: "Monthly",
  daily: "Daily",
  most_downloaded: "Most Downloaded",
}

/** One provider entry within a category: its id and ordered list ids. */
export interface CatalogProvider {
  provider: ProviderId
  lists: readonly ListId[]
}

/**
 * Per-category provider catalog, in default-resolution order: the first
 * provider is the category default; each provider's first list is its
 * default list. Do not reorder — menus are sorted at render time instead.
 */
export const DISC_CATALOG: Record<Cat, readonly CatalogProvider[]> = {
  mov: [
    {
      provider: "tmdb",
      lists: ["trending", "popular", "top_rated", "now_playing", "upcoming"],
    },
    { provider: "imdb", lists: ["popular", "top_rated", "most_voted", "newest"] },
    { provider: "yts", lists: ["most_seeded", "trending", "newest", "top_rated"] },
  ],
  tv: [
    { provider: "tmdb", lists: ["trending", "popular", "top_rated", "airing"] },
    { provider: "imdb", lists: ["popular", "top_rated", "most_voted", "newest"] },
    { provider: "tpb", lists: ["trending", "newest"] },
  ],
  ad: [
    { provider: "javdb", lists: ["weekly", "monthly", "daily"] },
    { provider: "dmm", lists: ["trending", "newest", "top_rated"] },
    // MGStage ranking windows (day/week/month/popular); "total" serves no
    // products so it is omitted. Each window returns ~50 distinct items.
    { provider: "mgstage", lists: ["weekly", "monthly", "daily", "popular"] },
    {
      provider: "sukebei",
      lists: ["most_seeded", "newest", "most_downloaded"],
    },
  ],
  vrc: [
    { provider: "dmm", lists: ["trending", "newest", "top_rated"] },
    {
      provider: "sukebei",
      lists: ["most_seeded", "newest", "most_downloaded"],
    },
    // VR feeds below expose a SINGLE list on purpose: their upstream VR view
    // ignores the ranking window/sort, so extra options would be a no-op.
    // MGStage VR = the "VR popular" search (-> "popular"); JavDB VR = the
    // tag-212 browser sorted by release desc (-> "newest"). Verified by
    // tests/live/discover.live.test.ts (lists returned byte-identical results).
    { provider: "mgstage", lists: ["popular"] },
    { provider: "javdb", lists: ["newest"] },
  ],
}

export interface ProviderOption {
  value: ProviderId
  label: string
}

export interface ListOption {
  value: ListId
  label: string
}

/** Sort dropdown options alphabetically by display label (menu order only —
 * DISC_CATALOG order still defines the per-category defaults). */
function byLabel<T extends { label: string }>(a: T, b: T): number {
  return a.label.localeCompare(b.label)
}

/** Provider options for a category (for the Provider Select), A→Z by label. */
export function providersFor(cat: Cat): readonly ProviderOption[] {
  return DISC_CATALOG[cat]
    .map((p) => ({ value: p.provider, label: PROVIDER_LABELS[p.provider] }))
    .sort(byLabel)
}

/**
 * List options for a provider within a category (for the List Select),
 * A→Z by label. Returns [] when the provider is not part of the category
 * catalog.
 */
export function listsFor(cat: Cat, provider: string): readonly ListOption[] {
  const entry = DISC_CATALOG[cat].find((p) => p.provider === provider)
  if (!entry) return []
  return entry.lists
    .map((l) => ({ value: l, label: LIST_LABELS[l] }))
    .sort(byLabel)
}

/** Default {source, list} for a category: first provider, its first list. */
export function defaultSelection(cat: Cat): { source: ProviderId; list: ListId } {
  const first = DISC_CATALOG[cat][0]
  return { source: first.provider, list: first.lists[0] }
}

/** First (default) list id for a provider within a category. */
export function defaultListFor(cat: Cat, provider: string): ListId {
  const entry = DISC_CATALOG[cat].find((p) => p.provider === provider)
  return entry ? entry.lists[0] : defaultSelection(cat).list
}

export const RANK_OPTIONS: readonly { value: Rank; label: string }[] = [
  { value: "popularity", label: "Popularity" },
  { value: "seeders", label: "Seeders" },
  { value: "recency", label: "Recency" },
  { value: "rating", label: "Rating" },
]

/**
 * Which Rank options yield a DISTINCT ordering for THIS pool. "Rank by" is a
 * pure client-side sort, so an option is only meaningful when it reorders the
 * items differently from one already on offer. We sort the pool by each rank
 * (in {@link RANK_OPTIONS} order, popularity first) and keep a rank only when
 * its resulting id-sequence hasn't been produced by an earlier one.
 *
 * Checking the actual order — not just "does the data have seeders/ratings" —
 * is what makes this correct: an already seeder-sorted feed (tpb trending) or a
 * feed with uniform seeders (javdb VR reports the same magnet count for every
 * title) collapses popularity/seeders/recency to one order, so only
 * "Popularity" remains and the toolbar drops the now-pointless control.
 */
export function availableRanks(items: readonly DiscoverItem[]): Rank[] {
  if (items.length === 0) return ["popularity"]
  const scored = ingest(items)
  const kept: Rank[] = []
  const seen = new Set<string>()
  for (const { value: rank } of RANK_OPTIONS) {
    const order = sortList(scored, rank)
      .map((x) => x.id)
      .join("")
    if (!seen.has(order)) {
      seen.add(order)
      kept.push(rank)
    }
  }
  return kept
}

export const LIMIT_OPTIONS: readonly ChipOption<number>[] = [
  { value: 10, label: "10" },
  { value: 25, label: "25" },
  { value: 50, label: "50" },
  { value: 100, label: "100" },
]

/** A list id implies a recency display (old Newest mode) for added-date pills. */
export function listIsRecency(list: string): boolean {
  return list === "newest" || list === "most_seeded"
}

export function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider as ProviderId] ?? provider
}

export function listLabel(list: string): string {
  return LIST_LABELS[list as ListId] ?? list
}

/** Default poster aspect ratio per category (old defAr). */
export function defAr(cat: Cat): number {
  return cat === "mov" ? 0.675 : cat === "tv" ? 0.7 : 0.72
}

/** DiscoverItem with the synthetic ranking fields ingestDisc() used to add. */
export interface ScoredItem extends DiscoverItem {
  pop: number
  added: number
  rating: number
}

/** Old ingestDisc(): derive popularity from feed position + seeders. */
export function ingest(items: readonly DiscoverItem[]): ScoredItem[] {
  return items.map((it, i) => ({
    ...it,
    pop: (items.length - i) * 2 + (it.seeders || 0) / 50,
    added: it.added ?? i,
    rating: it.rating || 0,
  }))
}

/** Old score(): feed popularity + log-seeders + freshness boost. */
function score(it: ScoredItem): number {
  return (
    it.pop * 1.0 +
    Math.log((it.seeders || 0) + 1) * 9 +
    Math.max(0, 30 - it.added) * 0.6
  )
}

/** Old sortList(): rank the pool without mutating it. */
export function sortList(list: readonly ScoredItem[], rank: Rank): ScoredItem[] {
  const a = [...list]
  if (rank === "popularity") a.sort((x, y) => score(y) - score(x))
  else if (rank === "seeders") a.sort((x, y) => y.seeders - x.seeders)
  else if (rank === "recency") a.sort((x, y) => x.added - y.added)
  else a.sort((x, y) => (y.rating || 0) - (x.rating || 0))
  return a
}

/**
 * Old ownedKeys(): uppercase titles + codes of everything on disk, so a
 * Discover card can be flagged "✓ In library".
 */
export function ownedKeys(
  items: readonly LibraryItem[] | undefined
): ReadonlySet<string> {
  const s = new Set<string>()
  for (const it of items ?? []) {
    if (it.title) s.add(it.title.toUpperCase())
    if (it.code) s.add(it.code.toUpperCase())
    const m = /([A-Za-z]{2,6})[-_ ]?(\d{2,5})/.exec(it.fname || "")
    if (m) s.add(`${m[1]}-${m[2]}`.toUpperCase())
  }
  return s
}

export interface CardState {
  state: MediaState
  /** 0..1, only meaningful while downloading. */
  progress?: number
}

/** Badge state: downloading (live queue) beats in-library beats NEW. */
export function itemState(
  it: DiscoverItem,
  owned: ReadonlySet<string>,
  downloads: Record<string, DownloadEntry>
): CardState {
  const dl = downloads[String(it.id)]
  if (dl?.state === "downloading") {
    return { state: "downloading", progress: dl.progress }
  }
  if (dl?.state === "done") return { state: "library" }
  if (it.state === "own") return { state: "library" }
  const code = (it.code || "").toUpperCase()
  const title = (it.title || "").toUpperCase()
  if ((code !== "" && owned.has(code)) || (title !== "" && owned.has(title))) {
    return { state: "library" }
  }
  return { state: "new" }
}

/** Detail-panel / pill label for a card state. */
export function stateLabel(cs: CardState): string {
  if (cs.state === "library") return "In library"
  if (cs.state === "downloading") {
    return `Downloading ${Math.round((cs.progress ?? 0) * 100)}%`
  }
  return "New"
}
