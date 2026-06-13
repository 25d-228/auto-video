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
 * Horizontal-wrap layout for natural-width MediaCards: cards flow left→right
 * and wrap to the next row. Tagged `data-cardgrid` so {@link useFitPageSize}
 * can measure how many fit in a fixed-height, non-scrolling area.
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
 * Dynamic page size for a fixed-height, NON-scrolling card area: returns how
 * many cards fit in `boxRef` without overflowing, so the rest paginate. It
 * over-renders (the caller slices to a high probe count), measures how many
 * rows fit, then settles to that count. Re-probes on container resize and when
 * `resetKey` changes (e.g. category/source switch). Cards must be inside a
 * `[data-cardgrid]` element (see {@link CardGrid}).
 */
const FIT_PROBE = 60

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

  // Covers load lazily and snap to their intrinsic ratio (so card widths
  // change after the first settle). Re-probe ONCE per reset cycle after a
  // cover loads — only while already settled (perPage < FIT_PROBE), so the
  // probe phase's own loads can't retrigger it and it can't oscillate.
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
    const limit = box.getBoundingClientRect().bottom + 1
    let fit = 0
    for (const c of cards) {
      if (c.getBoundingClientRect().bottom <= limit) fit++
      else break
    }
    fit = Math.max(1, fit)
    if (fit < cards.length && fit !== perPage) setPerPage(fit)
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
 * Fixed-page-size pagination over an in-memory pool (the old app's
 * DOM-measuring row packer is intentionally not ported — pick a clean
 * pageSize like 14/21/28 instead).
 *
 * Reset to page 1 yourself (`setPage(1)`) when the pool identity changes
 * (e.g. category/source switch); shrinking pools are auto-clamped.
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

/**
 * Numbered jump-to-page pager like the old app's pgNav: prev/next arrows,
 * a window of ±2 numbers around the current page with first/last + ellipsis,
 * and an item count on the right. Renders nothing for a single page.
 */
export function Pager({
  page,
  pageCount,
  itemCount,
  onPageChange,
  className,
}: PagerProps) {
  if (pageCount <= 1) return null
  const win = 2
  const lo = Math.max(1, page - win)
  const hi = Math.min(pageCount, page + win)
  const pages: (number | "gap")[] = []
  if (lo > 1) {
    pages.push(1)
    if (lo > 2) pages.push("gap")
  }
  for (let i = lo; i <= hi; i++) pages.push(i)
  if (hi < pageCount) {
    if (hi < pageCount - 1) pages.push("gap")
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
        {pages.map((m, i) =>
          m === "gap" ? (
            <span key={`gap-${i}`} className="px-1 text-xs text-muted-foreground">
              …
            </span>
          ) : m === page ? (
            <span
              key={m}
              aria-current="page"
              className="flex h-7 min-w-7 items-center justify-center rounded-md bg-primary px-2 text-xs font-medium text-primary-foreground"
            >
              {m}
            </span>
          ) : (
            <PagerButton key={m} onClick={() => onPageChange(m)}>
              {m}
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
