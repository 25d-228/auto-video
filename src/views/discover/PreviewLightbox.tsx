/**
 * Preview viewer: one sample image at a time, paged with the ‹ / › buttons or
 * ← / → keys, wrapping around. Images are fetched lazily via usePreviews when
 * it opens.
 */
import { useEffect, useState } from "react"
import { ChevronLeft, ChevronRight, Loader2 } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import type { DiscoverItem } from "@/api/types"
import { usePreviews } from "@/state/queries"

export interface PreviewLightboxProps {
  /** null = closed. */
  item: DiscoverItem | null
  onClose: () => void
}

export function PreviewLightbox({ item, onClose }: PreviewLightboxProps) {
  const previewsQ = usePreviews(item)
  const images = previewsQ.data ?? []
  const count = images.length
  const [idx, setIdx] = useState(0)

  // Restart at the first image whenever a different item is opened.
  const itemId = item?.id
  useEffect(() => setIdx(0), [itemId])

  // Wrap-around paging, clamped to the count.
  const go = (delta: number) =>
    setIdx((i) => (count > 0 ? (i + delta + count) % count : 0))

  // ← / → to page, while open.
  useEffect(() => {
    if (item === null) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") go(-1)
      else if (e.key === "ArrowRight") go(1)
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item, count])

  const safeIdx = count > 0 ? Math.min(idx, count - 1) : 0

  return (
    <Dialog
      open={item !== null}
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle className="text-sm font-medium">
            {item?.title ?? "Preview"}
            {count > 0 && (
              <span className="ml-2 text-xs font-normal text-muted-foreground">
                {safeIdx + 1} / {count}
              </span>
            )}
          </DialogTitle>
        </DialogHeader>

        {previewsQ.isFetching && count === 0 ? (
          <div className="flex h-72 items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Loading previews…
          </div>
        ) : count === 0 ? (
          <div className="flex h-72 items-center justify-center text-sm text-muted-foreground">
            No preview images for this title.
          </div>
        ) : (
          <div className="relative flex items-center justify-center">
            <img
              src={images[safeIdx]}
              alt=""
              className="max-h-[72vh] w-auto rounded-md object-contain"
            />
            {count > 1 && (
              <>
                <button
                  type="button"
                  aria-label="Previous"
                  onClick={() => go(-1)}
                  className="absolute left-2 flex size-9 items-center justify-center rounded-full bg-black/55 text-white transition-colors hover:bg-black/75"
                >
                  <ChevronLeft className="size-5" />
                </button>
                <button
                  type="button"
                  aria-label="Next"
                  onClick={() => go(1)}
                  className="absolute right-2 flex size-9 items-center justify-center rounded-full bg-black/55 text-white transition-colors hover:bg-black/75"
                >
                  <ChevronRight className="size-5" />
                </button>
              </>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
