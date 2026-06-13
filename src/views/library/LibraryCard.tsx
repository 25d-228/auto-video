/**
 * One library grid card: MediaCard with a lazily resolved cover plus the
 * old engine's right-click menu (Play / Reveal in Finder / Delete to Trash).
 * OS actions are disabled in a plain browser (no window.__TAURI__).
 */
import { FolderOpen, Play, Trash2 } from "lucide-react"
import type { LibraryItem } from "@/api/types"
import { MediaCard, MediaCardAction } from "@/components/media"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { useLibraryArt } from "./useLibraryArt"

export interface LibraryCardProps {
  item: LibraryItem
  /** False in plain-browser dev — Play/Reveal/Delete are disabled. */
  tauri: boolean
  onOpen: (item: LibraryItem) => void
  onPlay: (item: LibraryItem) => void
  onReveal: (item: LibraryItem) => void
  onDelete: (item: LibraryItem) => void
}

export function LibraryCard({
  item,
  tauri,
  onOpen,
  onPlay,
  onReveal,
  onDelete,
}: LibraryCardProps) {
  const art = useLibraryArt(item)
  // the old mk-tag: TV on tv items, VR on vr files; movies stay untagged
  const tag = item.cat === "tv" ? "TV" : item.vr ? "VR" : undefined

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        {/* plain wrapper so the trigger can attach its ref + onContextMenu */}
        <div className="flex-none">
          <MediaCard
            title={item.title}
            sub={item.sub || item.size}
            cover={art.cover}
            ar={art.cover ? art.ar : 0.7}
            source={tag}
            onClick={() => onOpen(item)}
            action={
              tauri ? (
                <MediaCardAction onClick={() => onPlay(item)}>
                  ▶ Play
                </MediaCardAction>
              ) : undefined
            }
          />
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem disabled={!tauri} onSelect={() => onPlay(item)}>
          <Play /> Play
        </ContextMenuItem>
        <ContextMenuItem disabled={!tauri} onSelect={() => onReveal(item)}>
          <FolderOpen /> Reveal in Finder
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          variant="destructive"
          disabled={!tauri}
          onSelect={() => onDelete(item)}
        >
          <Trash2 /> Delete to Trash
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}
