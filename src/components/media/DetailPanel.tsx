import { useEffect, useRef, useState, type ReactNode } from "react"
import { X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { Chiplet } from "./ChipRow"
import { CoverImage } from "./CoverImage"

const DEFAULT_POSTER_ASPECT = 0.67

export interface DetailFact {
  label: string
  value: ReactNode
}

export interface DetailSection {
  label: string
  chips: string[]
}

export interface DetailPanelProps {
  open: boolean
  /** Called by the ✕ button and the Escape key. */
  onClose: () => void
  title: string
  /** Line under the title, e.g. "2026 · YTS". */
  sub?: ReactNode
  /** Cover URL; falls back to the deterministic gradient placeholder. */
  cover?: string
  /** Cover aspect ratio (width/height) so the poster fills the panel width without cropping. */
  coverAspect?: number
  /** Header pill, e.g. the state label ("New" / "In library" / "Downloading 62%"). */
  pill?: ReactNode
  /** Two-column facts grid (Date / Runtime / Seeders / Source / …). */
  facts?: DetailFact[]
  /** Chip sections (Cast / Genres / 出演 …). */
  sections?: DetailSection[]
  /** Footer action row; children stretch to equal widths. */
  actions?: ReactNode
  /** Extra body content rendered under the sections. */
  children?: ReactNode
  className?: string
}

/**
 * Slide-in right detail panel: header with state pill + close, scrollable body
 * (poster, title, sub, facts grid, chip sections, children), sticky action
 * footer. Stays mounted while closed so the slide transition can play.
 */
export function DetailPanel({
  open,
  onClose,
  title,
  sub,
  cover,
  coverAspect,
  pill,
  facts,
  sections,
  actions,
  children,
  className,
}: DetailPanelProps) {
  const panelRef = useRef<HTMLElement>(null)
  // Keep the cover's true aspect ratio: start from the passed placeholder
  // (coverAspect) and snap to the image's intrinsic ratio once it loads, so a
  // wide DVD jacket isn't cropped into a portrait box.
  const [measuredAr, setMeasuredAr] = useState<number | null>(null)
  const [coverKey, setCoverKey] = useState(cover)
  if (coverKey !== cover) {
    setCoverKey(cover)
    setMeasuredAr(null)
  }
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    const onPointerDown = (e: PointerEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
        onClose()
      }
    }
    window.addEventListener("keydown", onKey)
    // defer attaching the outside-click listener so the same click that opened
    // the panel can't immediately close it
    const id = window.setTimeout(
      () => document.addEventListener("pointerdown", onPointerDown),
      0
    )
    return () => {
      window.clearTimeout(id)
      window.removeEventListener("keydown", onKey)
      document.removeEventListener("pointerdown", onPointerDown)
    }
  }, [open, onClose])

  return (
    <aside
      ref={panelRef}
      aria-hidden={!open}
      className={cn(
        "fixed top-0 right-0 bottom-0 z-30 flex w-[444px] flex-col border-l bg-background shadow-[-12px_0_32px_rgba(0,0,0,.1)] transition-transform duration-200 ease-out",
        open ? "translate-x-0" : "pointer-events-none translate-x-[105%]",
        className
      )}
    >
      <div className="flex items-center justify-between border-b px-3.5 py-2.5">
        <div className="min-w-0">{pill}</div>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Close details"
          onClick={onClose}
        >
          <X className="size-4" />
        </Button>
      </div>
      <div className="flex-1 overflow-y-auto p-3.5">
        <div
          className="w-full overflow-hidden rounded-[10px] border"
          style={{ aspectRatio: String(measuredAr ?? coverAspect ?? DEFAULT_POSTER_ASPECT) }}
        >
          <CoverImage src={cover} title={title} onRatio={setMeasuredAr} />
        </div>
        <div className="mt-3 text-[15px] font-semibold">{title}</div>
        {sub != null && (
          <div className="mt-0.5 text-xs text-muted-foreground">{sub}</div>
        )}
        {facts && facts.length > 0 && (
          <div className="mt-3 grid grid-cols-2 gap-x-3 gap-y-2 rounded-[10px] border px-3 py-2.5">
            {facts.map((f) => (
              <div key={f.label}>
                <div className="text-[10.5px] tracking-wider text-muted-foreground uppercase">
                  {f.label}
                </div>
                <div className="mt-px text-xs font-medium">{f.value}</div>
              </div>
            ))}
          </div>
        )}
        {sections?.map((s) => (
          <div key={s.label} className="mt-3.5">
            <div className="mb-1.5 text-[10.5px] font-semibold tracking-wider text-muted-foreground uppercase">
              {s.label}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {s.chips.map((c) => (
                <Chiplet key={c}>{c}</Chiplet>
              ))}
            </div>
          </div>
        ))}
        {children}
      </div>
      {actions != null && (
        <div className="flex gap-2 border-t px-3.5 py-3 *:flex-1">{actions}</div>
      )}
    </aside>
  )
}
