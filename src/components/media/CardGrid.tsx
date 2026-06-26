import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
  type RefObject,
} from "react"
import { ChevronLeft, ChevronRight } from "lucide-react"
import { cn } from "@/lib/utils"

/**
 * Wrap layout for natural-width MediaCards. Tagged `data-cardgrid` so
 * {@link useFitPageSize} can measure how many fit in a fixed-height area.
 */
export function CardGrid({
  className,
  children,
}: {
  className?: string
  children: ReactNode
}) {
  return (
    <div
      data-cardgrid
      className={cn("flex flex-wrap content-start gap-x-3.5 gap-y-4", className)}
    >
      {children}
    </div>
  )
}

/**
 * Page size for a fixed-height, non-scrolling card area: how many cards fit in
 * `boxRef` without overflowing, so the rest paginate. Over-renders (caller
 * slices to a high probe count), measures how many fit, then settles. Re-probes
 * on resize and when `resetKey` changes. Cards must be inside a
 * `[data-cardgrid]` element.
 */
const FIT_PROBE = 60
const FIT_TOLERANCE_PX = 1 // sub-pixel rounding slack

export function useFitPageSize(
  boxRef: RefObject<HTMLElement | null>,
  resetKey: string
): number {
  const [perPage, setPerPage] = useState(FIT_PROBE)
  const perPageRef = useRef(perPage)
  perPageRef.current = perPage
  // Whether we've already re-probed after covers loaded, for this reset cycle.
  const reprobed = useRef(false)

  // Re-probe when the content set changes (recompute from a full grid).
  useLayoutEffect(() => {
    reprobed.current = false
    setPerPage(FIT_PROBE)
  }, [resetKey])

  // Re-probe on container resize.
  useLayoutEffect(() => {
    const box = boxRef.current
    if (!box || typeof ResizeObserver === "undefined") return
    const ro = new ResizeObserver(() => {
      reprobed.current = false
      setPerPage(FIT_PROBE)
    })
    ro.observe(box)
    return () => ro.disconnect()
  }, [boxRef])

  // Covers load lazily and snap to their intrinsic ratio, so card widths change
  // after the first settle. Re-probe once per reset cycle after a cover loads,
  // and only while already settled (perPage < FIT_PROBE), so the probe phase's
  // own loads can't retrigger it and it can't oscillate.
  useLayoutEffect(() => {
    const box = boxRef.current
    if (!box) return
    const onLoad = () => {
      if (perPageRef.current < FIT_PROBE && !reprobed.current) {
        reprobed.current = true
        setPerPage(FIT_PROBE)
      }
    }
    box.addEventListener("load", onLoad, true) // img load doesn't bubble
    return () => box.removeEventListener("load", onLoad, true)
  }, [boxRef, resetKey])

  // After each render, count cards whose bottom stays within the box; once
  // we've rendered more than fit, settle to the fitting count. Guarded so it
  // converges (no setState when already settled).
  useLayoutEffect(() => {
    const box = boxRef.current
    if (!box) return
    const grid = box.querySelector<HTMLElement>("[data-cardgrid]")
    if (!grid) return
    const cards = Array.from(grid.children) as HTMLElement[]
    if (cards.length === 0) return
    const limit = box.getBoundingClientRect().bottom + FIT_TOLERANCE_PX
    let fittingCount = 0
    for (const card of cards) {
      if (card.getBoundingClientRect().bottom <= limit) fittingCount++
      else break
    }
    fittingCount = Math.max(1, fittingCount)
    if (fittingCount < cards.length && fittingCount !== perPage) setPerPage(fittingCount)
  })

  return perPage
}

export interface PagerState<T> {
  /** Current page, 1-based and clamped to pageCount. */
  page: number
  pageCount: number
  itemCount: number
  /** The current page's slice of `items`. */
  pageItems: T[]
  setPage: (page: number) => void
}

/**
 * Fixed-page-size pagination over an in-memory pool. Reset to page 1 yourself
 * (`setPage(1)`) when the pool identity changes; shrinking pools are
 * auto-clamped.
 */
export function usePager<T>(items: readonly T[], pageSize: number): PagerState<T> {
  const [rawPage, setPage] = useState(1)
  const pageCount = Math.max(1, Math.ceil(items.length / pageSize))
  const page = Math.min(Math.max(1, rawPage), pageCount)
  const pageItems = useMemo(
    () => items.slice((page - 1) * pageSize, page * pageSize),
    [items, page, pageSize]
  )
  return { page, pageCount, itemCount: items.length, pageItems, setPage }
}

function PagerButton({ className, ...props }: ComponentProps<"button">) {
  return (
    <button
      type="button"
      className={cn(
        "flex h-7 min-w-7 items-center justify-center rounded-md border bg-background px-2 text-xs font-medium shadow-xs transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50",
        className
      )}
      {...props}
    />
  )
}

export interface PagerProps {
  /** 1-based. */
  page: number
  pageCount: number
  itemCount: number
  onPageChange: (page: number) => void
  className?: string
}

// ±PAGE_WINDOW numbers shown around the current page.
const PAGE_WINDOW = 2

/**
 * Numbered pager: prev/next arrows, a window of ±2 pages around the current one
 * with first/last + ellipsis, and an item count. Nothing for a single page.
 */
export function Pager({
  page,
  pageCount,
  itemCount,
  onPageChange,
  className,
}: PagerProps) {
  if (pageCount <= 1) return null
  const firstWindowPage = Math.max(1, page - PAGE_WINDOW)
  const lastWindowPage = Math.min(pageCount, page + PAGE_WINDOW)
  const pages: (number | "gap")[] = []
  if (firstWindowPage > 1) {
    pages.push(1)
    if (firstWindowPage > 2) pages.push("gap")
  }
  for (let i = firstWindowPage; i <= lastWindowPage; i++) pages.push(i)
  if (lastWindowPage < pageCount) {
    if (lastWindowPage < pageCount - 1) pages.push("gap")
    pages.push(pageCount)
  }
  return (
    <div className={cn("flex items-center gap-2 pt-3", className)}>
      <div className="flex flex-1 justify-start">
        <PagerButton
          aria-label="Previous page"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
        >
          <ChevronLeft className="size-3.5" />
        </PagerButton>
      </div>
      <div className="flex items-center gap-1">
        {pages.map((entry, i) =>
          entry === "gap" ? (
            <span key={`gap-${i}`} className="px-1 text-xs text-muted-foreground">
              …
            </span>
          ) : entry === page ? (
            <span
              key={entry}
              aria-current="page"
              className="flex h-7 min-w-7 items-center justify-center rounded-md bg-primary px-2 text-xs font-medium text-primary-foreground"
            >
              {entry}
            </span>
          ) : (
            <PagerButton key={entry} onClick={() => onPageChange(entry)}>
              {entry}
            </PagerButton>
          )
        )}
      </div>
      <div className="flex flex-1 items-center justify-end gap-2">
        <span className="text-xs text-muted-foreground">{itemCount} items</span>
        <PagerButton
          aria-label="Next page"
          disabled={page >= pageCount}
          onClick={() => onPageChange(page + 1)}
        >
          <ChevronRight className="size-3.5" />
        </PagerButton>
      </div>
    </div>
  )
}
