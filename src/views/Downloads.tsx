/**
 * Downloads management view — active transfers (pause/resume/cancel over the
 * Rust librqbit commands) on top, settled ones (open/reveal/remove) below.
 * In a plain browser the rows still render but every OS-touching control is
 * disabled with a tooltip; cancel-and-delete goes through a confirm dialog.
 */
import { useState, type ReactNode } from "react"
import { Compass } from "lucide-react"
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useDownloads, type DownloadEntry } from "@/state/downloads"
import { ActiveDownloadRow } from "./downloads/ActiveDownloadRow"
import { FinishedDownloadRow } from "./downloads/FinishedDownloadRow"

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="mb-2 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
      {children}
    </div>
  )
}

export default function Downloads({
  onGoDiscover,
}: {
  onGoDiscover: () => void
}) {
  const {
    list,
    active,
    totalSpeedMbps,
    tauri,
    pauseDownload,
    resumeDownload,
    cancelDownload,
    removeEntry,
    notify,
  } = useDownloads()
  const [pendingDelete, setPendingDelete] = useState<DownloadEntry | null>(null)

  const finished = list.filter((d) => d.state === "done" || d.state === "error")

  const openFolder = async (entry: DownloadEntry) => {
    try {
      await openPath(entry.dest)
    } catch (e) {
      notify(`Could not open the folder: ${String(e)}`)
    }
  }

  const reveal = async (entry: DownloadEntry) => {
    try {
      await revealItemInDir(entry.dest)
    } catch (e) {
      notify(`Could not reveal the folder: ${String(e)}`)
    }
  }

  const confirmCancelDelete = async () => {
    const entry = pendingDelete
    if (!entry) return
    setPendingDelete(null)
    if (await cancelDownload(entry.id, true)) {
      notify("Download cancelled — files deleted")
    }
  }

  const cancelKeep = async (entry: DownloadEntry) => {
    if (await cancelDownload(entry.id, false)) {
      notify("Download cancelled — files kept")
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col p-5">
      <div className="mb-4 flex flex-none items-baseline gap-2.5">
        <h1 className="text-lg font-semibold">Downloads</h1>
        <span className="text-xs text-muted-foreground">
          {active.length > 0
            ? `${active.length} active · ↓ ${totalSpeedMbps.toFixed(1)} MB/s`
            : "Manage your transfers"}
        </span>
      </div>

      {list.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 rounded-xl border border-dashed p-10 text-center">
          <p className="text-sm text-muted-foreground">
            Nothing downloading — find something in Discover.
          </p>
          <Button variant="outline" size="sm" onClick={onGoDiscover}>
            <Compass />
            Open Discover
          </Button>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto pb-5">
          {active.length > 0 && (
            <div>
              <SectionLabel>Active · {active.length}</SectionLabel>
              <div className="flex flex-col gap-2">
                {active.map((entry) => (
                  <ActiveDownloadRow
                    key={entry.id}
                    entry={entry}
                    tauri={tauri}
                    onPause={(e) => void pauseDownload(e.id)}
                    onResume={(e) => void resumeDownload(e.id)}
                    onCancelKeep={(e) => void cancelKeep(e)}
                    onCancelDelete={setPendingDelete}
                    onOpenFolder={(e) => void openFolder(e)}
                  />
                ))}
              </div>
            </div>
          )}
          {finished.length > 0 && (
            <div>
              <SectionLabel>Finished · {finished.length}</SectionLabel>
              <div className="flex flex-col gap-2">
                {finished.map((entry) => (
                  <FinishedDownloadRow
                    key={entry.id}
                    entry={entry}
                    tauri={tauri}
                    onOpenFolder={(e) => void openFolder(e)}
                    onReveal={(e) => void reveal(e)}
                    onRemove={(e) => removeEntry(e.id)}
                  />
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      <Dialog
        open={pendingDelete != null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null)
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Cancel and delete files?</DialogTitle>
            <DialogDescription className="break-all">
              “{pendingDelete?.title}” will be removed from the session and its
              files in {pendingDelete?.dest} deleted from disk.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingDelete(null)}>
              Keep downloading
            </Button>
            <Button
              variant="destructive"
              onClick={() => void confirmCancelDelete()}
            >
              Cancel and delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  )
}
