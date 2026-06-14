/**
 * Discover view domain helpers, ported from the old vanilla engine
 * (git show HEAD:ui-src/engine.js — DISC_SOURCES,
 * ownedKeys, cidOf). Pure functions only; React lives in the
 * sibling components.
 */
import type { Cat, DiscoverItem, LibraryItem } from "@/api/types"
import type { ChipOption, MediaState } from "@/components/media"
import { normalizeCodeNum } from "@/lib/codes"
import type { DownloadEntry } from "@/state/downloads"

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
    // JavDB Adult = the Censored ranking (/api/v1/rankings?type=0) per window.
    { provider: "javdb", lists: ["daily", "weekly", "monthly"] },
    // DMM Adult: in-list sorts + the dedicated best-seller ranking pages
    // (monthly/daily are distinct from "trending"=sort=ranking).
    { provider: "dmm", lists: ["trending", "newest", "top_rated", "monthly", "daily"] },
    // MGStage ranking windows (day/week/month/popular); "total" serves no
    // products so it is omitted. Each window returns ~50 distinct items.
    { provider: "mgstage", lists: ["weekly", "monthly", "daily", "popular"] },
    {
      provider: "sukebei",
      lists: ["most_seeded", "newest", "most_downloaded"],
    },
  ],
  vrc: [
    // DMM VR = the digital GraphQL rankings (real streaming VR, not the obscure
    // physical-disc HTML floor): trending=best-sellers, monthly=this-month.
    { provider: "dmm", lists: ["trending", "monthly", "newest", "top_rated"] },
    {
      provider: "sukebei",
      lists: ["most_seeded", "newest", "most_downloaded"],
    },
    // MGStage VR = the "VR popular" search (single list; the upstream VR view
    // ignores the ranking window/sort, so extra options would be a no-op).
    { provider: "mgstage", lists: ["popular"] },
    // JavDB VR = the Categories→Censored browser with Genre=VR. It has no fixed
    // "list" — the toolbar shows Year/Month/Sort selectors instead (see
    // JAVDB_VR_* below + Discover.tsx). The "newest" id here is a placeholder so
    // resolveList stays happy; the actual feed is driven by JavdbVrSel/opts.
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

export const LIMIT_OPTIONS: readonly ChipOption<number>[] = [
  { value: 10, label: "10" },
  { value: 25, label: "25" },
  { value: 50, label: "50" },
  { value: 100, label: "100" },
]

// ----------------------------------------------------- JavDB VR browser controls
// JavDB VR (Adult→VR→JavDB) is the Categories→Censored browser with Genre=VR
// (filter_by `0:t:m:212:<year>::<month>`), so the toolbar shows Year / Month /
// Sort selectors instead of a single list. Values map to the live API params
// (see src/api/sources/javdb.ts javdbTags + docs/javdb-api.md).

/** Year options ("" = all years), newest first (2026 → 2001). */
export const JAVDB_YEAR_OPTIONS: readonly { value: string; label: string }[] = [
  { value: "", label: "All years" },
  ...Array.from({ length: 2026 - 2001 + 1 }, (_, i) => {
    const y = String(2026 - i)
    return { value: y, label: y }
  }),
]

const MONTH_NAMES = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
]

/** Month options ("" = all months); value is the 1–12 month tag id. */
export const JAVDB_MONTH_OPTIONS: readonly { value: string; label: string }[] = [
  { value: "", label: "All months" },
  ...MONTH_NAMES.map((nm, i) => ({ value: String(i + 1), label: nm })),
]

/** One JavDB VR sort option (Image #5), mapped to the API sort_by/order_by. */
export interface JavdbSortOption {
  value: string
  label: string
  sortBy: string
  orderBy: "desc" | "asc"
}

/** The seven verified VR sorts (sort_by tokens confirmed live). */
export const JAVDB_SORT_OPTIONS: readonly JavdbSortOption[] = [
  { value: "release_desc", label: "Newest", sortBy: "release", orderBy: "desc" },
  { value: "release_asc", label: "Oldest", sortBy: "release", orderBy: "asc" },
  { value: "update", label: "Recently updated", sortBy: "update", orderBy: "desc" },
  { value: "score", label: "Top rated", sortBy: "score", orderBy: "desc" },
  { value: "hit", label: "Most viewed", sortBy: "hit", orderBy: "desc" },
  { value: "want_watch_count", label: "Most wanted", sortBy: "want_watch_count", orderBy: "desc" },
  { value: "watched_count", label: "Most watched", sortBy: "watched_count", orderBy: "desc" },
]

/** JavDB VR toolbar selection (year/month/sort). */
export interface JavdbVrSel {
  year: string
  month: string
  /** A {@link JAVDB_SORT_OPTIONS} value. */
  sort: string
}

export const DEFAULT_JAVDB_VR: JavdbVrSel = { year: "", month: "", sort: "release_desc" }

/** Resolve a {@link JavdbVrSel} into the discover() opts (year/month/sortBy/orderBy). */
export function javdbVrOpts(sel: JavdbVrSel): {
  year: string
  month: string
  sortBy: string
  orderBy: "desc" | "asc"
} {
  const s =
    JAVDB_SORT_OPTIONS.find((o) => o.value === sel.sort) ?? JAVDB_SORT_OPTIONS[0]!
  return { year: sel.year, month: sel.month, sortBy: s.sortBy, orderBy: s.orderBy }
}

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

/** Add a code to the owned set in both its raw and zero-pad-normalized form. */
function addCode(set: Set<string>, code: string): void {
  const c = code.trim().toUpperCase()
  if (!c) return
  set.add(c)
  // On-disk codes are often zero-padded ("AJVR-00306") while Discover/JavDB use
  // the unpadded form ("AJVR-306"); store both so either side matches.
  set.add(normalizeCodeNum(c).toUpperCase())
}

/**
 * Uppercase titles + codes of everything on disk, so a Discover card can be
 * flagged "✓ In library". Codes are stored both raw and zero-pad-normalized
 * (see {@link addCode}) so a padded on-disk code matches an unpadded feed code.
 */
export function ownedKeys(
  items: readonly LibraryItem[] | undefined
): ReadonlySet<string> {
  const s = new Set<string>()
  for (const it of items ?? []) {
    if (it.title) s.add(it.title.toUpperCase())
    if (it.code) addCode(s, it.code)
    const m = /([A-Za-z]{2,6})[-_ ]?(\d{2,5})/.exec(it.fname || "")
    if (m) addCode(s, `${m[1]}-${m[2]}`)
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
  const ncode = normalizeCodeNum(it.code || "").toUpperCase()
  const title = (it.title || "").toUpperCase()
  if (
    (code !== "" && owned.has(code)) ||
    (ncode !== "" && owned.has(ncode)) ||
    (title !== "" && owned.has(title))
  ) {
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
