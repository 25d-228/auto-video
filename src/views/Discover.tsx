/**
 * Discover: live trending feeds per category. Toolbar (category chips,
 * per-category provider + list, show-count, refresh) -> source-ordered grid
 * with lazy seeder badges -> detail panel -> download dialog.
 */
import { useEffect, useMemo, useRef, useState } from "react"
import { Loader2, RefreshCw, Search, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  CardGrid,
  ChipRow,
  Pager,
  SegControl,
  useFitPageSize,
  usePager,
} from "@/components/media"
import { cn } from "@/lib/utils"
import type { Cat, DiscoverItem } from "@/api/types"
import { useDiscover, useLibrary } from "@/state/queries"
import { useDownloads } from "@/state/downloads"
import {
  CAT_OPTIONS,
  DEFAULT_JAVDB_BROWSE,
  JAVDB_AD_MODE_OPTIONS,
  JAVDB_MONTH_OPTIONS,
  JAVDB_SORT_OPTIONS,
  JAVDB_YEAR_OPTIONS,
  LIMIT_OPTIONS,
  defaultListFor,
  defaultSelection,
  itemState,
  javdbBrowseOpts,
  listIsRecency,
  listsFor,
  ownedKeys,
  providerLabel,
  providersFor,
  type JavdbAdMode,
  type JavdbBrowseSel,
  type ListId,
  type ProviderId,
} from "./discover/model"
import { DiscoverCard } from "./discover/DiscoverCard"
import { DiscoverDetail } from "./discover/DiscoverDetail"
import { DownloadDialog } from "./discover/DownloadDialog"
import { PreviewLightbox } from "./discover/PreviewLightbox"

// How many items to pull per feed; the pool is later capped to `limit`.
const FEED_FETCH_LIMIT = 100
// Default "Show" count until the user picks another LIMIT_OPTIONS value.
const DEFAULT_SHOW_LIMIT = 25

interface Selection {
  source: ProviderId
  list: ListId
}

/** Default {source, list} for every category. */
function initialSelByCat(): Record<Cat, Selection> {
  return {
    mov: defaultSelection("mov"),
    tv: defaultSelection("tv"),
    ad: defaultSelection("ad"),
    vrc: defaultSelection("vrc"),
  }
}

export default function Discover({ active = true }: { active?: boolean }) {
  const [cat, setCat] = useState<Cat>("mov")
  const [limit, setLimit] = useState(DEFAULT_SHOW_LIMIT)
  const [selByCat, setSelByCat] = useState<Record<Cat, Selection>>(
    initialSelByCat
  )
  // feed keys that have been force-refreshed at least once (fresh=1 stays
  // part of the query key afterwards, so later refreshes just refetch)
  const [freshKeys, setFreshKeys] = useState<ReadonlySet<string>>(
    () => new Set()
  )
  const [selected, setSelected] = useState<DiscoverItem | null>(null)
  const [dlItem, setDlItem] = useState<DiscoverItem | null>(null)
  const [previewItem, setPreviewItem] = useState<DiscoverItem | null>(null)
  // The JavDB Year/Month/Sort browser, shared by VR and Adult→Category.
  const [javdbBrowse, setJavdbBrowse] =
    useState<JavdbBrowseSel>(DEFAULT_JAVDB_BROWSE)
  // Adult→JavDB toggles between the ranking windows and the category browser.
  const [javdbAdMode, setJavdbAdMode] = useState<JavdbAdMode>("ranking")
  // Free-text title search (mov/tv only). The debounced value drives the feed so
  // typing doesn't fire a TMDB request per keystroke.
  const [search, setSearch] = useState("")
  const [debouncedSearch, setDebouncedSearch] = useState("")
  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(search.trim()), 300)
    return () => clearTimeout(t)
  }, [search])

  const { source, list } = selByCat[cat]
  const isJavdbVr = cat === "vrc" && source === "javdb"
  const isJavdbAd = cat === "ad" && source === "javdb"
  const isJavdbCategory = isJavdbAd && javdbAdMode === "category"
  // The year/month/sort browser drives both VR and the Adult→Category feed.
  const isBrowser = isJavdbVr || isJavdbCategory
  const browseOpts = isBrowser
    ? {
        ...javdbBrowseOpts(javdbBrowse),
        ...(isJavdbCategory ? { mode: "category" } : {}),
      }
    : undefined
  // A non-empty search box (mov/tv) overrides the browse list and hits TMDB
  // search; otherwise the feed is the selected provider/list (or, for the JavDB
  // browser, year/month/sort + mode).
  const canSearch = cat === "mov" || cat === "tv"
  const searching = canSearch && debouncedSearch !== ""
  const effectiveOpts = searching ? { query: debouncedSearch } : browseOpts
  // For the browser the feed identity is year/month/sort (+ mode), not the list.
  const feedKey = searching
    ? `${cat}|search|${debouncedSearch}`
    : isBrowser
      ? `${cat}|${source}|${isJavdbCategory ? "category" : "vr"}|${javdbBrowse.year}|${javdbBrowse.month}|${javdbBrowse.sort}`
      : `${cat}|${source}|${list}`
  const isFresh = freshKeys.has(feedKey)

  const query = useDiscover(cat, source, list, FEED_FETCH_LIMIT, isFresh, effectiveOpts)
  const libraryQ = useLibrary()
  const { downloads } = useDownloads()

  const owned = useMemo(
    () => ownedKeys(libraryQ.data?.items),
    [libraryQ.data]
  )

  // No client-side re-ranking: every feed is shown in its source/server order
  // (rankings, most-viewed, newest, JavDB VR sort, …), just capped to `limit`.
  const pool = useMemo(
    () => (query.data?.items ?? []).slice(0, limit),
    [query.data, limit]
  )

  // Fixed-height, non-scrolling card area: page size = however many covers fit.
  const gridBoxRef = useRef<HTMLDivElement>(null)
  const perPage = useFitPageSize(gridBoxRef, feedKey)
  const pager = usePager(pool, perPage)
  // Reset to page 1 whenever the pool identity changes (see Library.tsx).
  const { setPage } = pager
  useEffect(() => {
    setPage(1)
  }, [
    cat,
    source,
    list,
    limit,
    debouncedSearch,
    javdbAdMode,
    javdbBrowse.year,
    javdbBrowse.month,
    javdbBrowse.sort,
    setPage,
  ])

  // Discover stays mounted across tab switches so the category/selectors/page
  // are remembered, but its dialogs render in a body portal. Close them while
  // the view is hidden so they can't linger over another tab.
  useEffect(() => {
    if (active) return
    setSelected(null)
    setDlItem(null)
    setPreviewItem(null)
  }, [active])

  const providerOptions = providersFor(cat)
  const listOptions = listsFor(cat, source)

  // ---------------------------------------------------------- toolbar ops
  const closePanels = () => {
    setSelected(null)
    setDlItem(null)
    setPreviewItem(null)
  }

  const switchCat = (next: Cat) => {
    setCat(next)
    setSelByCat((prev) => ({ ...prev, [next]: defaultSelection(next) }))
    setSearch("")
    setDebouncedSearch("")
    closePanels()
  }

  // switching provider resets the list to that provider's first list
  const switchProvider = (next: string) => {
    const provider = next as ProviderId
    setSelByCat((prev) => ({
      ...prev,
      [cat]: { source: provider, list: defaultListFor(cat, provider) },
    }))
    closePanels()
  }

  const switchList = (next: string) => {
    setSelByCat((prev) => ({
      ...prev,
      [cat]: { ...prev[cat], list: next as ListId },
    }))
    closePanels()
  }

  // JavDB browser year/month/sort change (the "all" sentinel maps back to "").
  const setBrowse = (patch: Partial<JavdbBrowseSel>) => {
    setJavdbBrowse((prev) => ({ ...prev, ...patch }))
    closePanels()
  }

  // Adult→JavDB Ranking ⇄ Category toggle.
  const switchAdMode = (next: JavdbAdMode) => {
    setJavdbAdMode(next)
    closePanels()
  }

  const onRefresh = () => {
    closePanels()
    pager.setPage(1)
    if (isFresh) {
      void query.refetch()
    } else {
      setFreshKeys((prev) => new Set(prev).add(feedKey))
    }
  }

  const showAdded = !searching && listIsRecency(list)

  return (
    <section className="flex h-full min-h-0 flex-col p-5">
      <div className="mb-4 flex items-baseline gap-2.5">
        <h1 className="text-lg font-semibold">Discover</h1>
        <span className="text-xs text-muted-foreground">
          Browse trending releases
        </span>
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-x-2.5 gap-y-2">
        <ChipRow options={CAT_OPTIONS} value={cat} onChange={switchCat} />

        <Select value={source} onValueChange={switchProvider}>
          <SelectTrigger size="sm" className="text-xs" aria-label="Provider">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {providerOptions.map((p) => (
              <SelectItem key={p.value} value={p.value}>
                {p.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {isJavdbAd && (
          <Select
            value={javdbAdMode}
            onValueChange={(v) => switchAdMode(v as JavdbAdMode)}
          >
            <SelectTrigger size="sm" className="text-xs" aria-label="Mode">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {JAVDB_AD_MODE_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={o.value}>
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}

        {isBrowser ? (
          <>
            <Select
              value={javdbBrowse.year || "all"}
              onValueChange={(v) => setBrowse({ year: v === "all" ? "" : v })}
            >
              <SelectTrigger size="sm" className="text-xs" aria-label="Year">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {JAVDB_YEAR_OPTIONS.map((o) => (
                  <SelectItem key={o.value || "all"} value={o.value || "all"}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <Select
              value={javdbBrowse.month || "all"}
              onValueChange={(v) => setBrowse({ month: v === "all" ? "" : v })}
            >
              <SelectTrigger size="sm" className="text-xs" aria-label="Month">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {JAVDB_MONTH_OPTIONS.map((o) => (
                  <SelectItem key={o.value || "all"} value={o.value || "all"}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <Select
              value={javdbBrowse.sort}
              onValueChange={(v) => setBrowse({ sort: v })}
            >
              <SelectTrigger size="sm" className="text-xs" aria-label="Sort">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {JAVDB_SORT_OPTIONS.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </>
        ) : (
          <Select value={list} onValueChange={switchList}>
            <SelectTrigger size="sm" className="text-xs" aria-label="List">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {listOptions.map((l) => (
                <SelectItem key={l.value} value={l.value}>
                  {l.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}

        {canSearch && (
          <div className="relative">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={`Search ${cat === "mov" ? "movies" : "TV"}…`}
              aria-label="Search title"
              className="h-8 w-44 rounded-md border border-input bg-transparent pl-7 pr-7 text-xs shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
            />
            {search && (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => setSearch("")}
                className="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
        )}

        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          Show
          <SegControl
            options={LIMIT_OPTIONS}
            value={limit}
            onChange={setLimit}
          />
        </span>

        <span className="ml-auto flex items-center gap-2">
          {query.data && !query.isFetching && (
            <span className="text-[11px] text-muted-foreground">
              updated {query.data.updated}
            </span>
          )}
          <Button
            variant="outline"
            size="sm"
            disabled={query.isFetching}
            onClick={onRefresh}
          >
            <RefreshCw
              className={cn("size-3.5", query.isFetching && "animate-spin")}
            />
            {query.isFetching ? "refreshing…" : "Refresh"}
          </Button>
        </span>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div ref={gridBoxRef} className="min-h-0 flex-1 overflow-hidden">
          {query.isPending ? (
            <div className="flex items-center gap-2.5 py-12 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" />
              {searching
                ? `Searching for “${debouncedSearch}”…`
                : `Fetching live results from ${providerLabel(source)}…`}
            </div>
          ) : pool.length === 0 ? (
            <div className="py-12 text-sm text-muted-foreground">
              {searching
                ? `No matches for “${debouncedSearch}”.`
                : "No live results right now — try Refresh."}
            </div>
          ) : (
            <CardGrid>
              {pager.pageItems.map((it) => (
                <DiscoverCard
                  key={it.id}
                  item={it}
                  cardState={itemState(it, owned, downloads)}
                  showAdded={showAdded}
                  onOpen={() => setSelected(it)}
                  onDownload={() => setDlItem(it)}
                  onPreview={() => setPreviewItem(it)}
                />
              ))}
            </CardGrid>
          )}
        </div>
        {!query.isPending && pool.length > 0 && (
          <Pager
            className="flex-none pt-3"
            page={pager.page}
            pageCount={pager.pageCount}
            itemCount={pager.itemCount}
            onPageChange={pager.setPage}
          />
        )}
      </div>

      <DiscoverDetail
        item={selected}
        owned={owned}
        onClose={() => setSelected(null)}
        onDownload={(it) => {
          setSelected(null)
          setDlItem(it)
        }}
        onPreview={(it) => setPreviewItem(it)}
      />
      {/* These render in a body portal, so the hidden wrapper can't hide them
          while Discover stays mounted on another tab. Gate on `active` so they
          unmount immediately instead of animating out over the new tab. */}
      {active && (
        <>
          <DownloadDialog item={dlItem} onClose={() => setDlItem(null)} />
          <PreviewLightbox
            item={previewItem}
            onClose={() => setPreviewItem(null)}
          />
        </>
      )}
    </section>
  )
}
