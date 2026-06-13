/**
 * Lazy cover + identification for one library file — the React port of the
 * old engine's lazyLibCovers(): ad/vr items resolve a cover by JAV code via
 * /cover, mov/tv items via the TMDB /movie|/tv lookup. Everything is cached
 * by the query layer (staleTime Infinity), so only mounted (= current page)
 * cards ever fetch.
 */
import type { Cat, LibraryItem, TitleMeta } from "@/api/types"
import { useCover, useTitleLookup } from "@/state/queries"

/** ad/vrc files use the JAV pipeline (code -> cover -> r18 meta). */
export function isJavCat(cat: Cat): boolean {
  return cat === "ad" || cat === "vrc"
}

/** Sort key for Rank=Release — the old libDate(): a year found in year||sub. */
export function itemDate(item: LibraryItem): string {
  const m = /(19|20)\d{2}/.exec(String(item.year || item.sub || ""))
  return m ? m[0] : ""
}

/** "a, b / c" -> ["a","b","c"] for Chiplet sections — the old chips(). */
export function splitChips(s: string | undefined): string[] {
  return (s || "")
    .split(/[,/]/)
    .map((t) => t.trim())
    .filter((t) => t !== "")
}

export interface LibraryArt {
  /** Resolved cover URL ("" / undefined -> gradient placeholder). */
  cover?: string
  /** Aspect ratio to lay the card out with (lookup ar wins over the scan default). */
  ar: number
  /** TMDB meta for mov/tv items (date / runtime / genre / cast / overview). */
  titleMeta?: TitleMeta
  /** False when no TMDB key is configured (mov/tv only). */
  haskey?: boolean
  /** True while the cover/lookup request is in flight. */
  pending: boolean
  /** Lookup verdict; undefined until the request settles. */
  identified?: boolean
}

/** Accepts null so detail panels can keep hook order while nothing is selected. */
export function useLibraryArt(item: LibraryItem | null): LibraryArt {
  const jav = item != null && isJavCat(item.cat)
  const coverQ = useCover(jav ? item.code : undefined)
  const lookupQ = useTitleLookup(
    item?.cat === "tv",
    item != null && !jav ? item.title : undefined,
    item?.year
  )
  const fallbackAr = item?.ar ?? 0.72

  if (jav) {
    const d = coverQ.data
    return {
      cover: d?.ok ? d.cover : undefined,
      ar: d?.ok ? d.ar : fallbackAr,
      pending: coverQ.isLoading,
      identified: d ? d.ok : undefined,
    }
  }
  const d = lookupQ.data
  return {
    cover: d?.meta?.cover || undefined,
    ar: d?.meta?.ar ?? fallbackAr,
    titleMeta: d?.meta,
    haskey: d?.haskey,
    pending: lookupQ.isLoading,
    identified: d ? d.ok : undefined,
  }
}
