/**
 * Library view — the React port of the old engine's renderLib()/libFill():
 * category chips, debounced title/filename search, Title|Release rank with
 * direction, a paged grid of lazily-covered cards, a right detail panel and
 * the OS actions (Play / Reveal in Finder / Delete to Trash) under Tauri.
 */
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { useQueryClient } from "@tanstack/react-query"
import { invoke } from "@tauri-apps/api/core"
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener"
import type { Cat, LibraryItem } from "@/api/types"
import {
  CardGrid,
  ChipRow,
  Pager,
  SegControl,
  useFitPageSize,
  usePager,
  type ChipOption,
} from "@/components/media"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { isTauri, useDownloads } from "@/state/downloads"
import { qk, useLibrary, usePaths } from "@/state/queries"
import { LibraryCard } from "./library/LibraryCard"
import { LibraryDetail } from "./library/LibraryDetail"
import { itemDate } from "./library/useLibraryArt"

const CATS: readonly ChipOption<Cat>[] = [
  { value: "mov", label: "Movies" },
  { value: "tv", label: "TV" },
  { value: "ad", label: "Adult" },
  { value: "vrc", label: "VR" },
]
const CAT_LABEL = Object.fromEntries(
  CATS.map((c) => [c.value, c.label])
) as Record<Cat, string>

type Rank = "alpha" | "release"
type Dir = "asc" | "desc"

const RANKS: readonly ChipOption<Rank>[] = [
  { value: "alpha", label: "Title" },
  { value: "release", label: "Release" },
]
const DIRS: readonly ChipOption<Dir>[] = [
  { value: "asc", label: "↑ Asc" },
  { value: "desc", label: "↓ Desc" },
]

const SEARCH_DEBOUNCE_MS = 200


function EmptyState({ children }: { children: ReactNode }) {
  return (
    <div className="flex flex-1 items-center justify-center rounded-xl border border-dashed p-10 text-center text-sm text-muted-foreground">
      {children}
    </div>
  )
}

export default function Library() {
  const [cat, setCat] = useState<Cat>("mov")
  const [search, setSearch] = useState("")
  const [debouncedSearch, setDebouncedSearch] = useState("")
  const [rank, setRank] = useState<Rank>("alpha")
  const [dir, setDir] = useState<Dir>("asc")
  const [selected, setSelected] = useState<LibraryItem | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<LibraryItem | null>(null)

  const libQ = useLibrary()
  const pathsQ = usePaths()
  const { notify } = useDownloads()
  const queryClient = useQueryClient()
  const tauri = isTauri()

  // debounce the search box into the filter (the old oninput -> libFill)
  useEffect(() => {
    const timeoutId = window.setTimeout(
      () => setDebouncedSearch(search),
      SEARCH_DEBOUNCE_MS
    )
    return () => window.clearTimeout(timeoutId)
  }, [search])

  const catItems = useMemo(
    () => (libQ.data?.items ?? []).filter((i) => i.cat === cat),
    [libQ.data, cat]
  )

  // the old libFilteredPool(): search on display title or filename, then sort
  const pool = useMemo(() => {
    const needle = debouncedSearch.trim().toLowerCase()
    const filtered = needle
      ? catItems.filter(
          (i) =>
            i.title.toLowerCase().includes(needle) ||
            i.fname.toLowerCase().includes(needle)
        )
      : catItems
    return [...filtered].sort((a, b) => {
      let cmp = rank === "release" ? itemDate(a).localeCompare(itemDate(b)) : 0
      if (cmp === 0) cmp = a.title.localeCompare(b.title)
      return dir === "desc" ? -cmp : cmp
    })
  }, [catItems, debouncedSearch, rank, dir])

  // Fixed-height, non-scrolling card area: page size = however many fit.
  const gridBoxRef = useRef<HTMLDivElement>(null)
  const perPage = useFitPageSize(gridBoxRef, cat)
  const pager = usePager(pool, perPage)
  const { setPage } = pager
  useEffect(() => {
    setPage(1)
  }, [cat, debouncedSearch, rank, dir, setPage])

  const changeCat = (next: Cat) => {
    setCat(next)
    setSearch("") // the old app cleared the query on category switch
    setDebouncedSearch("")
    setDetailOpen(false)
  }

  const play = async (item: LibraryItem) => {
    if (!isTauri()) {
      notify("Playing files needs the desktop app")
      return
    }
    try {
      await openPath(item.path)
    } catch (e) {
      notify(`Could not open the file: ${String(e)}`)
    }
  }

  const reveal = async (item: LibraryItem) => {
    if (!isTauri()) {
      notify("Revealing files needs the desktop app")
      return
    }
    try {
      await revealItemInDir(item.path)
    } catch (e) {
      notify(`Could not reveal the file: ${String(e)}`)
    }
  }

  const requestDelete = (item: LibraryItem) => {
    if (!isTauri()) {
      notify("Deleting files needs the desktop app")
      return
    }
    setPendingDelete(item)
  }

  const confirmDelete = async () => {
    const item = pendingDelete
    if (!item) return
    setPendingDelete(null)
    try {
      await invoke("trash_delete", { path: item.path })
      notify("Moved to Trash")
      if (selected?.path === item.path) setDetailOpen(false)
      await queryClient.invalidateQueries({ queryKey: qk.library() })
      await queryClient.invalidateQueries({ queryKey: qk.stats() })
    } catch (e) {
      notify(`Delete failed: ${String(e)}`)
    }
  }

  let body: ReactNode
  if (libQ.isLoading) {
    body = <EmptyState>Scanning library…</EmptyState>
  } else if (libQ.isError) {
    body = (
      <EmptyState>
        Library scan failed ({libQ.error.message})
      </EmptyState>
    )
  } else if (catItems.length === 0) {
    const folder = pathsQ.data?.[cat]
    body = (
      <EmptyState>
        {folder
          ? `No videos found in ${folder}.`
          : `No ${CAT_LABEL[cat]} folder configured yet — set one in Settings → Library.`}
      </EmptyState>
    )
  } else if (pool.length === 0) {
    body = <EmptyState>No matches for “{debouncedSearch.trim()}”.</EmptyState>
  } else {
    body = (
      <CardGrid>
        {pager.pageItems.map((item) => (
          <LibraryCard
            key={item.path}
            item={item}
            tauri={tauri}
            onOpen={(it) => {
              setSelected(it)
              setDetailOpen(true)
            }}
            onPlay={play}
            onReveal={reveal}
            onDelete={requestDelete}
          />
        ))}
      </CardGrid>
    )
  }

  return (
    <section className="flex h-full min-h-0 flex-col p-5">
      <div className="mb-4 flex items-baseline gap-2.5">
        <h1 className="text-lg font-semibold">Library</h1>
        <span className="text-xs text-muted-foreground">Your collection</span>
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-2.5">
        <ChipRow options={CATS} value={cat} onChange={changeCat} />
        <div className="ml-auto flex flex-wrap items-center gap-2.5">
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search title or filename…"
            className="h-8 w-52"
          />
          <span className="text-xs text-muted-foreground">Rank</span>
          <SegControl options={RANKS} value={rank} onChange={setRank} />
          <SegControl options={DIRS} value={dir} onChange={setDir} />
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div
          ref={gridBoxRef}
          className="flex min-h-0 flex-1 flex-col overflow-hidden"
        >
          {body}
        </div>
        {pool.length > 0 && (
          <Pager
            className="flex-none pt-3"
            page={pager.page}
            pageCount={pager.pageCount}
            itemCount={pager.itemCount}
            onPageChange={pager.setPage}
          />
        )}
      </div>

      <LibraryDetail
        item={selected}
        open={detailOpen}
        onClose={() => setDetailOpen(false)}
        tauri={tauri}
        onPlay={play}
        onReveal={reveal}
        onDelete={requestDelete}
      />

      <Dialog
        open={pendingDelete != null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null)
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Move to Trash?</DialogTitle>
            <DialogDescription className="break-all">
              “{pendingDelete?.title}” ({pendingDelete?.fname}) will be moved
              to the macOS Trash.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingDelete(null)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={() => void confirmDelete()}>
              Move to Trash
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  )
}
