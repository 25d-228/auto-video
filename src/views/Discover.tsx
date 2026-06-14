/**
 * Discover view — live trending feeds per category, ported from the old
 * renderDisc/discPaint (git show HEAD:ui-src/engine.js) onto the shared
 * media components. Toolbar (category chips, per-category provider + list,
 * show-count, refresh) -> source-ordered grid with lazy live seeder badges ->
 * detail panel -> download dialog.
 */
import { useEffect, useMemo, useRef, useState } from "react"
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
import type { Cat, DiscoverItem } from "@/api/types"
import { useDiscover, useLibrary } from "@/state/queries"
import { useDownloads } from "@/state/downloads"
import {
  CAT_OPTIONS,
  DEFAULT_JAVDB_VR,
  JAVDB_MONTH_OPTIONS,
  JAVDB_SORT_OPTIONS,
  JAVDB_YEAR_OPTIONS,
  LIMIT_OPTIONS,
  defaultListFor,
  defaultSelection,
  itemState,
  javdbVrOpts,
  listIsRecency,
  listsFor,
  ownedKeys,
  providerLabel,
  providersFor,
  type JavdbVrSel,
  type ListId,
  type ProviderId,
} from "./discover/model"
import { DiscoverCard } from "./discover/DiscoverCard"
import { DiscoverDetail } from "./discover/DiscoverDetail"
import { DownloadDialog } from "./discover/DownloadDialog"
import { PreviewLightbox } from "./discover/PreviewLightbox"

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
  const [limit, setLimit] = useState(25)
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
  // JavDB VR uses Year/Month/Sort selectors instead of a single list.
  const [javdbVr, setJavdbVr] = useState<JavdbVrSel>(DEFAULT_JAVDB_VR)

  const { source, list } = selByCat[cat]
  const isJavdbVr = cat === "vrc" && source === "javdb"
  const vrOpts = isJavdbVr ? javdbVrOpts(javdbVr) : undefined
  // For JavDB VR the feed identity is the year/month/sort, not the list id.
  const feedKey = isJavdbVr
    ? `${cat}|${source}|${javdbVr.year}|${javdbVr.month}|${javdbVr.sort}`
    : `${cat}|${source}|${list}`
  const isFresh = freshKeys.has(feedKey)

  const query = useDiscover(cat, source, list, 100, isFresh, vrOpts)
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
  }, [cat, source, list, limit, javdbVr.year, javdbVr.month, javdbVr.sort, setPage])

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

  // JavDB VR year/month/sort change ("" sentinel "all" maps back to all).
  const setVr = (patch: Partial<JavdbVrSel>) => {
    setJavdbVr((prev) => ({ ...prev, ...patch }))
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

  const showAdded = listIsRecency(list)

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

        {isJavdbVr ? (
          <>
            <Select
              value={javdbVr.year || "all"}
              onValueChange={(v) => setVr({ year: v === "all" ? "" : v })}
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
              value={javdbVr.month || "all"}
              onValueChange={(v) => setVr({ month: v === "all" ? "" : v })}
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

            <Select value={javdbVr.sort} onValueChange={(v) => setVr({ sort: v })}>
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
      <DownloadDialog item={dlItem} onClose={() => setDlItem(null)} />
      <PreviewLightbox item={previewItem} onClose={() => setPreviewItem(null)} />
    </section>
  )
}
