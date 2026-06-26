/** One in-flight download: progress, Pause/Resume, and the Cancel dropdown. */
import { ChevronDown, FolderOpen, Pause, Play } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { cn } from "@/lib/utils"
import type { DownloadEntry } from "@/state/downloads"
import { Hint, NEEDS_DESKTOP } from "./Hint"

export function ActiveDownloadRow({
  entry,
  tauri,
  onPause,
  onResume,
  onCancelKeep,
  onCancelDelete,
  onOpenFolder,
}: {
  entry: DownloadEntry
  tauri: boolean
  onPause: (entry: DownloadEntry) => void
  onResume: (entry: DownloadEntry) => void
  /** Cancel, keep files. Non-destructive, runs directly. */
  onCancelKeep: (entry: DownloadEntry) => void
  /** Cancel and delete files. Parent confirms first. */
  onCancelDelete: (entry: DownloadEntry) => void
  /** Open the destination folder. */
  onOpenFolder: (entry: DownloadEntry) => void
}) {
  const paused = entry.state === "paused"
  const pct = Math.round(entry.progress * 100)

  return (
    <div className="rounded-xl border bg-card px-4 py-3">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-medium">{entry.title}</span>
            {paused && (
              <Badge variant="secondary" className="flex-none">
                Paused
              </Badge>
            )}
          </div>
          <div className="truncate font-mono text-[11px] text-muted-foreground">
            {entry.dest}
          </div>
        </div>
        <div className="flex flex-none items-center gap-1.5">
          <Hint show={!tauri} title={NEEDS_DESKTOP}>
            <Button
              variant="ghost"
              size="sm"
              disabled={!tauri}
              aria-label="Open folder"
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
              onClick={() => (paused ? onResume(entry) : onPause(entry))}
            >
              {paused ? <Play /> : <Pause />}
              {paused ? "Resume" : "Pause"}
            </Button>
          </Hint>
          <DropdownMenu>
            <Hint show={!tauri} title={NEEDS_DESKTOP}>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" disabled={!tauri}>
                  Cancel
                  <ChevronDown />
                </Button>
              </DropdownMenuTrigger>
            </Hint>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => onCancelKeep(entry)}>
                Cancel, keep files
              </DropdownMenuItem>
              <DropdownMenuItem
                variant="destructive"
                onSelect={() => onCancelDelete(entry)}
              >
                Cancel and delete files
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div className="mt-2.5 flex items-center gap-3">
        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
          <div
            className={cn(
              "h-full rounded-full transition-[width]",
              paused ? "bg-muted-foreground" : "bg-foreground"
            )}
            style={{ width: `${pct}%` }}
          />
        </div>
        <span className="w-9 flex-none text-right text-xs tabular-nums text-muted-foreground">
          {pct}%
        </span>
        <span className="w-20 flex-none text-right text-xs tabular-nums text-muted-foreground">
          {paused ? "paused" : `${entry.speedMbps.toFixed(1)} MB/s`}
        </span>
      </div>
    </div>
  )
}
