import { useState } from "react"
import { cn } from "@/lib/utils"

/**
 * Deterministic 32-bit string hash — the old engine's `h()`. Drives the
 * hue of the no-art placeholder so the same title always gets the same
 * gradient as the old app.
 */
export function titleHash(s: string): number {
  let n = 0
  for (let i = 0; i < s.length; i++) n = (n * 31 + s.charCodeAt(i)) >>> 0
  return n
}

/** The old engine's no-art gradient: two hsl stops 40° apart, seeded by title. */
export function placeholderGradient(title: string): string {
  const x = titleHash(title || "")
  return `linear-gradient(160deg,hsl(${x % 360},34%,42%),hsl(${(x + 40) % 360},34%,26%))`
}

/**
 * If `src` is a sidecar-proxied cover (`…/img?u=<original>`), return the
 * decoded original URL so it can be retried directly when the proxy fails.
 * Returns "" for non-proxied URLs.
 */
export function rawCoverUrl(src: string): string {
  if (!src.includes("/img?")) return ""
  try {
    const params = new URLSearchParams(src.slice(src.indexOf("?") + 1))
    return params.get("u") ?? ""
  } catch {
    return ""
  }
}

/**
 * The DMM content id hiding in a cover URL — the old engine's cidOf().
 * Proxied covers (`/img?u=…`) are decoded first so the path segments are
 * visible. Returns "" when there is no /digital/video/<cid>/ segment.
 */
export function cidOf(cover: string): string {
  const raw = rawCoverUrl(cover || "") || cover || ""
  const m = /\/digital\/video\/([^/]+)\//.exec(raw)
  return m ? m[1] : ""
}

export interface CoverImageProps {
  /** Cover URL; may be proxied via the sidecar /img endpoint. Empty/undefined -> placeholder. */
  src?: string
  /** Title shown inside the gradient placeholder (and used as its hash seed). */
  title: string
  /** Applied to whichever root renders (the <img> or the placeholder <div>). */
  className?: string
  /**
   * Fired once the real image decodes, with its intrinsic aspect ratio (w/h).
   * Lets the card size to the cover's *original* proportions regardless of
   * source. Not fired for the gradient placeholder.
   */
  onRatio?: (ar: number) => void
}

/**
 * Lazy cover image with the old app's fallback chain:
 * proxied URL -> raw original URL (data-raw equivalent) -> hue-gradient
 * placeholder with the title text.
 */
export function CoverImage({ src, title, className, onRatio }: CoverImageProps) {
  const raw = src ? rawCoverUrl(src) : ""
  const chain = [src ?? "", raw !== src ? raw : ""].filter((u) => u !== "")
  // reset the fallback chain whenever the cover URL changes
  const [key, setKey] = useState(src)
  const [idx, setIdx] = useState(0)
  if (key !== src) {
    setKey(src)
    setIdx(0)
  }
  const url = idx < chain.length ? chain[idx] : undefined

  if (!url) {
    return (
      <div
        className={cn(
          "flex h-full w-full items-center justify-center overflow-hidden p-3 text-center",
          className
        )}
        style={{ background: placeholderGradient(title) }}
      >
        <span className="text-[13px] leading-snug font-semibold text-white [text-shadow:0_1px_4px_rgba(0,0,0,.45)]">
          {title}
        </span>
      </div>
    )
  }
  return (
    <img
      loading="lazy"
      src={url}
      alt=""
      draggable={false}
      onError={() => setIdx((i) => i + 1)}
      onLoad={(e) => {
        const { naturalWidth: w, naturalHeight: h } = e.currentTarget
        if (w > 0 && h > 0) onRatio?.(w / h)
      }}
      className={cn("h-full w-full object-cover", className)}
    />
  )
}
