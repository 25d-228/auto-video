import { useState, type ComponentProps, type ReactNode } from "react"
import { cn } from "@/lib/utils"
import { CoverImage } from "./CoverImage"

/** Card state badge: NEW / ✓ In library / downloading %. */
export type MediaState = "new" | "library" | "downloading"

/** Cover aspect-ratio placeholder (w/h) used before the image's own ratio is known. */
const DEFAULT_COVER_ASPECT = 0.7
/** Cover height in px when the caller doesn't pin one. */
const DEFAULT_COVER_HEIGHT_PX = 180

export interface MediaCardProps {
  title: string
  /** Sub line under the title, e.g. "2026" or "S02E04". */
  sub?: ReactNode
  /** Cover URL (possibly proxied via /img). */
  cover?: string
  /**
   * Cover aspect ratio (w/h) for the card's width before the image loads, a
   * per-source placeholder. Once the cover decodes the card snaps to the
   * image's intrinsic ratio (see CoverImage's onRatio).
   */
  ar?: number
  /** Cover height in px; cards sharing a grid should share it. */
  coverHeight?: number
  /** Tiny source tag in the sub line, e.g. "YTS" / "javdb". */
  source?: string
  /** Omit for no state badge. */
  state?: MediaState
  /** 0..1, shown when state is "downloading". */
  progress?: number
  /** Bottom-left seeder badge content, e.g. `▲ ${fseed(n)}` (lazy-fillable slot). */
  seedBadge?: ReactNode
  /** Hover action slot, centered near the cover bottom (see MediaCardAction). */
  action?: ReactNode
  onClick?: () => void
  className?: string
}

function StateBadge({ state, progress }: { state: MediaState; progress?: number }) {
  const label =
    state === "new"
      ? "NEW"
      : state === "library"
        ? "✓ In library"
        : `↓ ${Math.round((progress ?? 0) * 100)}%`
  return (
    <span
      className={cn(
        "absolute top-1.5 left-1.5 z-[2] rounded-full px-1.5 py-px text-[9.5px] font-bold tracking-wide text-white",
        state === "new" && "bg-blue-600",
        state === "library" && "bg-green-600",
        state === "downloading" && "bg-black/60"
      )}
    >
      {label}
    </span>
  )
}

/**
 * Poster card: cover with gradient scrim, state badge (top-left), seeder badge
 * (bottom-left), hover action, then a truncated title + sub line with a source
 * tag. Width is natural: round(coverHeight * ar). Lay out with CardGrid.
 */
export function MediaCard({
  title,
  sub,
  cover,
  ar = DEFAULT_COVER_ASPECT,
  coverHeight = DEFAULT_COVER_HEIGHT_PX,
  source,
  state,
  progress,
  seedBadge,
  action,
  onClick,
  className,
}: MediaCardProps) {
  // Width follows the cover's intrinsic ratio once it decodes; until then we
  // use the per-source `ar` placeholder. Reset when the cover URL changes so a
  // recycled card re-measures rather than keeping a stale ratio.
  const [measuredAr, setMeasuredAr] = useState<number | null>(null)
  const [coverKey, setCoverKey] = useState(cover)
  if (coverKey !== cover) {
    setCoverKey(cover)
    setMeasuredAr(null)
  }
  const width = Math.round(coverHeight * (measuredAr ?? ar))
  return (
    <div
      className={cn("group flex-none cursor-pointer", className)}
      style={{ width }}
      onClick={onClick}
    >
      <div
        className="relative overflow-hidden rounded-[10px] border"
        style={{ height: coverHeight }}
      >
        <CoverImage
          src={cover}
          title={title}
          className="absolute inset-0"
          onRatio={setMeasuredAr}
        />
        {/* scrim */}
        <div className="pointer-events-none absolute inset-0 z-[1] bg-[linear-gradient(180deg,rgba(0,0,0,.18),rgba(0,0,0,0)_35%,rgba(0,0,0,0)_60%,rgba(0,0,0,.35))]" />
        {state && <StateBadge state={state} progress={progress} />}
        {seedBadge != null && (
          <span className="absolute bottom-1.5 left-1.5 z-[2] rounded-full bg-black/55 px-1.5 py-0.5 text-[10px] font-semibold text-white">
            {seedBadge}
          </span>
        )}
        {action != null && (
          <div className="absolute inset-x-0 bottom-8 z-[3] flex translate-y-1 justify-center opacity-0 transition-all duration-150 group-hover:translate-y-0 group-hover:opacity-100">
            {action}
          </div>
        )}
      </div>
      <div className="mt-1.5">
        <div className="truncate text-xs font-medium">{title}</div>
        {(sub != null || source) && (
          <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-muted-foreground">
            {sub != null && <span className="truncate">{sub}</span>}
            {source && (
              <span className="flex-none rounded-[5px] border px-1 text-[9.5px] leading-[14px]">
                {source}
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * White hover pill, e.g. `<MediaCardAction onClick={…}>↓ Download</MediaCardAction>`.
 * Clicks don't bubble to the card's onClick.
 */
export function MediaCardAction({
  className,
  onClick,
  ...props
}: ComponentProps<"button">) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation()
        onClick?.(e)
      }}
      className={cn(
        "rounded-lg bg-white px-2.5 py-1 text-[11.5px] font-semibold text-zinc-900 shadow-[0_2px_8px_rgba(0,0,0,.3)] hover:bg-zinc-100",
        className
      )}
      {...props}
    />
  )
}
