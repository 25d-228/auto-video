/**
 * Download modal: live releases from /seeders as radio rows sorted by seeders,
 * a file picker for the chosen release, and a summary with the destination
 * folder from /paths. Start hands the magnet to the Rust downloader; outside
 * Tauri the button is disabled.
 */
import { useEffect, useMemo, useState } from "react"
import { Check, Loader2 } from "lucide-react"
import { useQuery } from "@tanstack/react-query"
import { invoke } from "@tauri-apps/api/core"
import { save as saveDialog } from "@tauri-apps/plugin-dialog"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { cn } from "@/lib/utils"
import { fseed } from "@/lib/format"
import { humanSize } from "@/api/sources/tpb"
import {
  planRename,
  RENAME_VIDEO_RE,
  type RenameCat,
  type RenameOp,
} from "@/lib/rename"
import type { DiscoverItem, Release } from "@/api/types"
import { usePaths, useSeeders } from "@/state/queries"
import { useDownloads } from "@/state/downloads"
import { getKey, setKey } from "@/state/db"
import { providerLabel } from "./model"

export interface DownloadDialogProps {
  /** null = closed. */
  item: DiscoverItem | null
  onClose: () => void
}

/** One file inside a torrent (from the Rust list_torrent_files command). */
interface TorrentFile {
  index: number
  name: string
  size: number
}

function SectionLabel({ children }: { children: string }) {
  return (
    <div className="text-[10.5px] font-semibold tracking-wider text-muted-foreground uppercase">
      {children}
    </div>
  )
}

/**
 * Canonical base the downloaded file(s) should be renamed to: JAV code for
 * ad/vrc, "YEAR.Title" for movies, show name for tv.
 */
function renameBase(cat: RenameCat, item: DiscoverItem): string {
  if (cat === "ad" || cat === "vrc") return item.code || item.title
  if (cat === "mov") return item.year ? `${item.year}.${item.title}` : item.title
  return item.title
}

/**
 * Build the download plan: which file indices to fetch (a real subset only;
 * all/unknown means the whole torrent) and the renames to apply once it
 * finishes.
 */
function buildPlan(
  item: DiscoverItem,
  files: TorrentFile[] | undefined,
  picked: ReadonlySet<number>,
  multiFile: boolean
): { onlyFiles: number[] | undefined; renames: RenameOp[] | undefined } {
  const onlyFiles =
    files && multiFile && picked.size < files.length
      ? [...picked].sort((a, b) => a - b)
      : undefined
  const cat = item.cat as RenameCat
  const base = renameBase(cat, item)
  const pickedFiles = files ? files.filter((f) => picked.has(f.index)) : []
  const renames =
    pickedFiles.length > 0
      ? planRename(
          cat,
          base,
          pickedFiles.map((f) => ({ name: f.name, size: f.size }))
        )
      : undefined
  return { onlyFiles, renames }
}

export function DownloadDialog({ item, onClose }: DownloadDialogProps) {
  const seedQ = useSeeders(item)
  const pathsQ = usePaths()
  const { tauri, startDownload, notify } = useDownloads()

  const [selIdx, setSelIdx] = useState(0)
  const [starting, setStarting] = useState(false)
  const [savingTorrent, setSavingTorrent] = useState(false)
  const itemId = item?.id
  useEffect(() => {
    setSelIdx(0)
    setStarting(false)
    setSavingTorrent(false)
  }, [itemId])

  const releases = useMemo<Release[]>(() => {
    if (!item) return []
    const rels = [...(seedQ.data?.releases ?? [])].sort(
      (a, b) => b.seeders - a.seeders
    )
    // sukebei items carry their own magnet; use it when /seeders comes back
    // empty so the download still works
    if (rels.length === 0 && item.magnet) {
      rels.push({
        name: item.title,
        quality: "",
        size: item.size || "",
        source: item.src,
        seeders: item.seeders,
        magnet: item.magnet,
      })
    }
    return rels
  }, [seedQ.data, item])

  const sel = releases.length > 0 ? releases[Math.min(selIdx, releases.length - 1)] : undefined

  // Resolve the selected release's file list (list_only, no download) so the
  // user can pick which files to fetch. Cached per-magnet; metadata for a bare
  // magnet is fetched from peers (bounded by the Rust-side timeout).
  const selMagnet = sel?.magnet ?? ""
  const filesQ = useQuery({
    queryKey: ["torrent-files", selMagnet],
    enabled: item !== null && tauri && selMagnet !== "",
    queryFn: () => invoke<TorrentFile[]>("list_torrent_files", { magnet: selMagnet }),
    staleTime: Infinity,
    retry: false,
  })
  const files = filesQ.data
  const [picked, setPicked] = useState<ReadonlySet<number>>(new Set())
  // Default selection: the video files (else everything) whenever the list loads.
  useEffect(() => {
    if (!files) return
    const videoFileIndices = files.filter((f) => RENAME_VIDEO_RE.test(f.name)).map((f) => f.index)
    setPicked(new Set(videoFileIndices.length > 0 ? videoFileIndices : files.map((f) => f.index)))
  }, [files])
  const toggleFile = (i: number) =>
    setPicked((prev) => {
      const next = new Set(prev)
      if (next.has(i)) next.delete(i)
      else next.add(i)
      return next
    })
  const multiFile = (files?.length ?? 0) > 1
  const nothingPicked = multiFile && picked.size === 0

  const dest = item ? (pathsQ.data?.[item.cat] ?? "") : ""
  const destLabel = dest || "Downloads/auto-video"
  const searching = item !== null && seedQ.isFetching && seedQ.data === undefined

  const seed = seedQ.data
  const subLine = !item
    ? ""
    : seed && seed.releases.length > 0
      ? `${item.sub} · ${seed.count} live releases (${Object.entries(seed.sources)
          .map(([k, v]) => `${k} ${v}`)
          .join(" · ")})`
      : `${item.sub} · from ${providerLabel(item.src)}`

  const onStart = async () => {
    if (!item || !sel?.magnet || starting || nothingPicked) return
    setStarting(true)
    const { onlyFiles, renames } = buildPlan(item, files, picked, multiFile)
    const ok = await startDownload({
      magnet: sel.magnet,
      dest,
      id: String(item.id),
      title: item.title,
      onlyFiles,
      renames: renames && renames.length > 0 ? renames : undefined,
    })
    setStarting(false)
    if (ok) {
      notify("Download started")
      onClose()
    }
  }

  // Save just the .torrent file (metadata only, no content), to load into
  // another client or archive. Independent of the file picker.
  const onSaveTorrent = async () => {
    if (!item || !sel?.magnet || savingTorrent || starting) return
    if (!tauri) {
      notify("Saving the .torrent needs the desktop app")
      return
    }
    const base =
      (item.code || item.title || "download").replace(/[/\\:*?"<>|]+/g, "_").trim() ||
      "download"
    setSavingTorrent(true)
    try {
      // Default to the directory used last time; fall back to the category
      // folder on first use.
      const lastDir = (await getKey("torrentSaveDir").catch(() => null)) ?? ""
      const baseDir = lastDir || dest
      const defaultPath = baseDir ? `${baseDir}/${base}.torrent` : `${base}.torrent`
      const outPath = await saveDialog({
        title: "Save .torrent file",
        defaultPath,
        filters: [{ name: "Torrent", extensions: ["torrent"] }],
      })
      if (!outPath) return // cancelled
      await invoke("save_torrent", { magnet: sel.magnet, outPath })
      // Remember the chosen directory (strip the trailing /filename) for next time.
      const dir = outPath.replace(/[/\\][^/\\]*$/, "")
      if (dir && dir !== outPath) await setKey("torrentSaveDir", dir).catch(() => {})
      notify("Saved .torrent file")
      onClose()
    } catch (e) {
      notify(`Couldn't save .torrent: ${String(e)}`)
    } finally {
      setSavingTorrent(false)
    }
  }

  return (
    <Dialog
      open={item !== null}
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Download</DialogTitle>
          <DialogDescription>
            {item ? `${item.title} · ${subLine}` : ""}
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-w-0 flex-col gap-2">
          <SectionLabel>Releases</SectionLabel>
          {searching ? (
            <div className="flex items-center gap-2 rounded-lg border px-3 py-4 text-xs text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> Searching seeders…
            </div>
          ) : releases.length === 0 ? (
            <div className="rounded-lg border px-3 py-4 text-xs text-muted-foreground">
              No live releases found — try again later.
            </div>
          ) : (
            <div className="max-h-52 divide-y overflow-y-auto rounded-lg border">
              {releases.map((r, i) => (
                <button
                  key={`${r.magnet}-${i}`}
                  type="button"
                  onClick={() => setSelIdx(i)}
                  className={cn(
                    "flex w-full items-start gap-2.5 px-3 py-2 text-left transition-colors",
                    i === selIdx ? "bg-accent" : "hover:bg-muted/60"
                  )}
                >
                  <span
                    aria-hidden
                    className={cn(
                      "mt-0.5 size-3.5 flex-none rounded-full border",
                      i === selIdx && "border-[4.5px] border-primary"
                    )}
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs font-medium">
                      {r.name}
                    </span>
                    <span className="mt-0.5 flex flex-wrap items-center gap-x-1.5 text-[11px] text-muted-foreground">
                      {r.quality && (
                        <span className="rounded-[5px] border px-1 text-[9.5px] leading-[14px]">
                          {r.quality}
                        </span>
                      )}
                      <span>{r.size || "?"}</span>
                      <span>· {r.source}</span>
                      <span>· ▲ {fseed(r.seeders)} seeders</span>
                    </span>
                  </span>
                </button>
              ))}
            </div>
          )}

          {sel &&
            (filesQ.isFetching ? (
              <>
                <SectionLabel>Files</SectionLabel>
                <div className="flex items-center gap-2 rounded-lg border px-3 py-4 text-xs text-muted-foreground">
                  <Loader2 className="size-3.5 animate-spin" /> Reading files…
                </div>
              </>
            ) : files && files.length > 1 ? (
              <>
                <SectionLabel>Files · pick what to download</SectionLabel>
                <div className="max-h-44 divide-y overflow-y-auto rounded-lg border">
                  {files.map((f) => {
                    const on = picked.has(f.index)
                    return (
                      <button
                        key={f.index}
                        type="button"
                        onClick={() => toggleFile(f.index)}
                        className="flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-muted/60"
                      >
                        <span
                          aria-hidden
                          className={cn(
                            "flex size-4 flex-none items-center justify-center rounded-[4px] border",
                            on && "border-primary bg-primary text-primary-foreground"
                          )}
                        >
                          {on && <Check className="size-3" />}
                        </span>
                        <span className="min-w-0 flex-1 truncate text-xs font-medium">
                          {f.name}
                        </span>
                        <span className="flex-none text-[11px] text-muted-foreground">
                          {humanSize(f.size)}
                        </span>
                      </button>
                    )
                  })}
                </div>
              </>
            ) : (
              <>
                <SectionLabel>File</SectionLabel>
                <div className="flex items-center justify-between gap-3 rounded-lg border px-3 py-2 text-xs">
                  <span className="min-w-0 truncate font-medium">
                    {files?.[0]?.name ?? sel.name}
                  </span>
                  <span className="flex-none text-muted-foreground">
                    {files?.[0] ? humanSize(files[0].size) : sel.size || "?"}
                  </span>
                </div>
                {filesQ.isError && (
                  <p className="text-[11px] text-muted-foreground">
                    Couldn’t read the file list — the whole torrent will download.
                  </p>
                )}
              </>
            ))}

          <div className="mt-1 grid grid-cols-[88px_minmax(0,1fr)] gap-y-1 rounded-lg bg-muted/50 px-3 py-2.5 text-xs">
            <span className="text-muted-foreground">Title</span>
            <span className="min-w-0 truncate font-semibold">{item?.title ?? ""}</span>
            <span className="text-muted-foreground">Size</span>
            <span className="font-semibold">{sel?.size || "?"}</span>
            <span className="text-muted-foreground">Magnet</span>
            <span className="font-semibold">
              {sel?.magnet ? "ready ✓" : "missing"}
            </span>
            <span className="text-muted-foreground">Destination</span>
            <span className="min-w-0 truncate font-semibold">{destLabel}</span>
          </div>

          {!tauri && (
            <p className="text-[11px] text-muted-foreground">
              Downloads need the desktop app — this browser preview is
              read-only.
            </p>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            className="sm:mr-auto"
            disabled={!tauri || !sel?.magnet || savingTorrent || starting}
            onClick={() => void onSaveTorrent()}
            title="Save the .torrent file only — no video is downloaded"
          >
            {savingTorrent ? "Saving…" : "Save .torrent"}
          </Button>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={!tauri || !sel?.magnet || starting || savingTorrent || nothingPicked}
            onClick={() => void onStart()}
          >
            {starting ? "Starting…" : "Start download"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
