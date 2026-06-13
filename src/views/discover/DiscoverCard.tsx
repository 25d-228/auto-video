/**
 * One Discover grid cell: MediaCard plus the lazily-upgraded seeder badge
 * (old discCard + the discPaint loadSeeds pass). Mounted only for the
 * current page, so the gate naturally fetches per-page.
 */
import { MediaCard, MediaCardAction } from "@/components/media"
import { fseed, relAdded } from "@/lib/format"
import { defAr, providerLabel, type CardState, type ScoredItem } from "./model"
import { useGatedSeeders } from "./useGatedSeeders"

export interface DiscoverCardProps {
  item: ScoredItem
  cardState: CardState
  /** Append "· N days ago" to the sub line (Newest mode / recency rank). */
  showAdded: boolean
  onOpen: () => void
  onDownload: () => void
}

export function DiscoverCard({
  item,
  cardState,
  showAdded,
  onOpen,
  onDownload,
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
      sub={item.sub + (showAdded ? ` · ${relAdded(item.added)}` : "")}
      cover={item.cover || undefined}
      ar={item.ar || defAr(item.cat)}
      source={providerLabel(item.src)}
      state={cardState.state}
      progress={cardState.progress}
      seedBadge={<span title={seedTitle}>▲ {fseed(seeders)}</span>}
      action={
        cardState.state === "new" ? (
          <MediaCardAction onClick={onDownload}>Download</MediaCardAction>
        ) : undefined
      }
      onClick={onOpen}
    />
  )
}
