/**
 * One settled download. Done rows show Open folder / Reveal in Finder (both
 * Tauri-gated); errored rows go red with the failure message. Remove is
 * local-only, so always enabled.
 */
import { CircleAlert, CircleCheck, FolderOpen, FolderSearch, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { DownloadEntry } from "@/state/downloads"
import { Hint, NEEDS_DESKTOP } from "./Hint"

export function FinishedDownloadRow({
  entry,
  tauri,
  onOpenFolder,
  onReveal,
  onRemove,
}: {
  entry: DownloadEntry
  tauri: boolean
  onOpenFolder: (entry: DownloadEntry) => void
  onReveal: (entry: DownloadEntry) => void
  onRemove: (entry: DownloadEntry) => void
}) {
  const errored = entry.state === "error"

  return (
    <div className="flex items-center gap-3 rounded-xl border bg-card px-4 py-3">
      {errored ? (
        <CircleAlert className="size-4 flex-none text-destructive" />
      ) : (
        <CircleCheck className="size-4 flex-none text-green-600 dark:text-green-500" />
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{entry.title}</div>
        {errored ? (
          <div className="truncate text-[11px] text-destructive">
            {entry.error ?? "Download failed"}
          </div>
        ) : (
          <div className="truncate font-mono text-[11px] text-muted-foreground">
            {entry.dest}
          </div>
        )}
      </div>
      <div className="flex flex-none items-center gap-1.5">
        {!errored && (
          <>
            <Hint show={!tauri} title={NEEDS_DESKTOP}>
              <Button
                variant="outline"
                size="sm"
                disabled={!tauri}
                onClick={() => onOpenFolder(entry)}
              >
                <FolderOpen />
                Open folder
              </Button>
            </Hint>
            <Hint show={!tauri} title={NEEDS_DESKTOP}>
              <Button
                variant="outline"
                size="sm"
                disabled={!tauri}
                onClick={() => onReveal(entry)}
              >
                <FolderSearch />
                Reveal in Finder
              </Button>
            </Hint>
          </>
        )}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Remove from list"
          title="Remove from list"
          onClick={() => onRemove(entry)}
        >
          <X />
        </Button>
      </div>
    </div>
  )
}
