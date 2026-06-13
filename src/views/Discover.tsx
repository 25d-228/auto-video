/**
 * Discover view — live trending feeds per category, ported from the old
 * renderDisc/discPaint (git show HEAD:ui-src/engine.js) onto the shared
 * media components. Toolbar (category chips, per-category provider + list,
 * rank, show-count, refresh) -> ranked grid with lazy live seeder badges ->
 * detail panel -> download dialog.
 */
import { useMemo, useRef, useState } from "react"
import { Loader2, RefreshCw } from "lucide-react"
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
import type { Cat } from "@/api/types"
import { useDiscover, useLibrary } from "@/state/queries"
import { useDownloads } from "@/state/downloads"
import {
  availableRanks,
  CAT_OPTIONS,
  LIMIT_OPTIONS,
  RANK_OPTIONS,
  defaultListFor,
  defaultSelection,
  ingest,
  itemState,
  listIsRecency,
  listsFor,
  ownedKeys,
  providerLabel,
  providersFor,
  sortList,
  type ListId,
  type ProviderId,
  type Rank,
  type ScoredItem,
} from "./discover/model"
import { DiscoverCard } from "./discover/DiscoverCard"
import { DiscoverDetail } from "./discover/DiscoverDetail"
import { DownloadDialog } from "./discover/DownloadDialog"

interface Selection {
  source: ProviderId
  list: ListId
}

/** Default {source, list} for every category — initial per-category state. */
function initialSelByCat(): Record<Cat, Selection> {
  return {
    mov: defaultSelection("mov"),
    tv: defaultSelection("tv"),
    ad: defaultSelection("ad"),
    vrc: defaultSelection("vrc"),
  }
}

export default function Discover() {
  const [cat, setCat] = useState<Cat>("mov")
  const [rank, setRank] = useState<Rank>("popularity")
  const [limit, setLimit] = useState(25)
  const [selByCat, setSelByCat] = useState<Record<Cat, Selection>>(
    initialSelByCat
  )
  // feed keys that have been force-refreshed at least once (fresh=1 stays
  // part of the query key afterwards, so later refreshes just refetch)
  const [freshKeys, setFreshKeys] = useState<ReadonlySet<string>>(
    () => new Set()
  )
  const [selected, setSelected] = useState<ScoredItem | null>(null)
  const [dlItem, setDlItem] = useState<ScoredItem | null>(null)

  const { source, list } = selByCat[cat]
  const feedKey = `${cat}|${source}|${list}`
  const isFresh = freshKeys.has(feedKey)

  const query = useDiscover(cat, source, list, 100, isFresh)
  const libraryQ = useLibrary()
  const { downloads } = useDownloads()

  const owned = useMemo(
    () => ownedKeys(libraryQ.data?.items),
    [libraryQ.data]
  )

  // Only offer ranks that actually reorder this pool; when just "popularity"
  // applies (DMM/MGStage carry no seeders or ratings, so every rank produces
  // the same order) the control hides. effRank clamps a now-inapplicable
  // selection (e.g. "seeders" after switching to DMM) back to popularity.
  const ranks = useMemo(
    () => availableRanks(query.data?.items ?? []),
    [query.data]
  )
  const effRank = ranks.includes(rank) ? rank : "popularity"

  const pool = useMemo(() => {
    const items = query.data?.items ?? []
    return sortList(ingest(items), effRank).slice(0, limit)
  }, [query.data, effRank, limit])

  // Fixed-height, non-scrolling card area: page size = however many covers fit.
  const gridBoxRef = useRef<HTMLDivElement>(null)
  const perPage = useFitPageSize(gridBoxRef, feedKey)
  const pager = usePager(pool, perPage)

  const providerOptions = providersFor(cat)
  const listOptions = listsFor(cat, source)

  // ---------------------------------------------------------- toolbar ops
  const closePanels = () => {
    setSelected(null)
    setDlItem(null)
  }

  const switchCat = (next: Cat) => {
    setCat(next)
    setSelByCat((prev) => ({ ...prev, [next]: defaultSelection(next) }))
    setRank("popularity")
    closePanels()
    pager.setPage(1)
  }

  // switching provider resets the list to that provider's first list
  const switchProvider = (next: string) => {
    const provider = next as ProviderId
    setSelByCat((prev) => ({
      ...prev,
      [cat]: { source: provider, list: defaultListFor(cat, provider) },
    }))
    closePanels()
    pager.setPage(1)
  }

  const switchList = (next: string) => {
    setSelByCat((prev) => ({
      ...prev,
      [cat]: { ...prev[cat], list: next as ListId },
    }))
    closePanels()
    pager.setPage(1)
  }

  const switchRank = (next: Rank) => {
    setRank(next)
    pager.setPage(1)
  }

  const switchLimit = (next: number) => {
    setLimit(next)
    pager.setPage(1)
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

  const showAdded = listIsRecency(list) || effRank === "recency"

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

        {ranks.length > 1 && (
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            Rank by
            <Select value={effRank} onValueChange={(v) => switchRank(v as Rank)}>
              <SelectTrigger size="sm" className="text-xs" aria-label="Rank by">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RANK_OPTIONS.filter((r) => ranks.includes(r.value)).map((r) => (
                  <SelectItem key={r.value} value={r.value}>
                    {r.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </span>
        )}

        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          Show
          <SegControl
            options={LIMIT_OPTIONS}
            value={limit}
            onChange={switchLimit}
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
              Fetching live results from {providerLabel(source)}…
            </div>
          ) : pool.length === 0 ? (
            <div className="py-12 text-sm text-muted-foreground">
              No live results right now — try Refresh.
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
      />
      <DownloadDialog item={dlItem} onClose={() => setDlItem(null)} />
    </section>
  )
}
