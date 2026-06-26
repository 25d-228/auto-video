/**
 * One Discover grid cell: MediaCard plus a lazily-upgraded seeder badge.
 * Mounted only for the current page, so the gate fetches per-page.
 */
import { MediaCard, MediaCardAction } from "@/components/media"
import type { DiscoverItem } from "@/api/types"
import { hasPreviews } from "@/api/previews"
import { fseed, relAdded } from "@/lib/format"
import { defAr, providerLabel, type CardState } from "./model"
import { useGatedSeeders } from "./useGatedSeeders"

export interface DiscoverCardProps {
  item: DiscoverItem
  cardState: CardState
  /** Append "· N days ago" to the sub line (Newest mode / recency rank). */
  showAdded: boolean
  onOpen: () => void
  onDownload: () => void
  onPreview: () => void
}

export function DiscoverCard({
  item,
  cardState,
  showAdded,
  onOpen,
  onDownload,
  onPreview,
}: DiscoverCardProps) {
  const seedQ = useGatedSeeders(item)
  const live = seedQ.data
  const seeders = live ? live.topSeed || 0 : item.seeders
  const sourcesNote = live
    ? Object.entries(live.sources)
        .map(([k, v]) => `${k}: ${v}`)
        .join(", ")
    : ""
  const seedTitle = live
    ? `${live.topSeed || 0} top seeders${sourcesNote ? ` · ${sourcesNote}` : ""}`
    : undefined

  return (
    <MediaCard
      title={item.title}
      sub={item.sub + (showAdded ? ` · ${relAdded(item.added ?? 0)}` : "")}
      cover={item.cover || undefined}
      ar={item.ar || defAr(item.cat)}
      source={providerLabel(item.src)}
      state={cardState.state}
      progress={cardState.progress}
      seedBadge={<span title={seedTitle}>▲ {fseed(seeders)}</span>}
      action={
        hasPreviews(item.src) || cardState.state === "new" ? (
          <div className="flex flex-col items-center gap-1.5">
            {hasPreviews(item.src) && (
              <MediaCardAction onClick={onPreview}>Preview</MediaCardAction>
            )}
            {cardState.state === "new" && (
              <MediaCardAction onClick={onDownload}>Download</MediaCardAction>
            )}
          </div>
        ) : undefined
      }
      onClick={onOpen}
    />
  )
}
