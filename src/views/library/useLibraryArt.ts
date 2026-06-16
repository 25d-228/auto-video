/**
 * Lazy cover + identification for one library file — the React port of the
 * old engine's lazyLibCovers(): ad/vr items resolve a cover by JAV code via
 * /cover, mov/tv items via the TMDB /movie|/tv lookup. Everything is cached
 * by the query layer (staleTime Infinity), so only mounted (= current page)
 * cards ever fetch.
 */
import type { Cat, LibraryItem, TitleMeta } from "@/api/types"
import { useCover, useTitleLookup } from "@/state/queries"

/** Poster aspect used when neither the scan nor a lookup supplies one. */
const DEFAULT_POSTER_ASPECT = 0.72

/** ad/vrc files use the JAV pipeline (code -> cover -> r18 meta). */
export function isJavCat(cat: Cat): boolean {
  return cat === "ad" || cat === "vrc"
}

/** Sort key for Rank=Release — the old libDate(): a year found in year||sub. */
export function itemDate(item: LibraryItem): string {
  const yearMatch = /(19|20)\d{2}/.exec(String(item.year || item.sub || ""))
  return yearMatch ? yearMatch[0] : ""
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
  const fallbackAr = item?.ar ?? DEFAULT_POSTER_ASPECT

  if (jav) {
    const coverData = coverQ.data
    return {
      cover: coverData?.ok ? coverData.cover : undefined,
      ar: coverData?.ok ? coverData.ar : fallbackAr,
      pending: coverQ.isLoading,
      identified: coverData ? coverData.ok : undefined,
    }
  }
  const lookupData = lookupQ.data
  return {
    cover: lookupData?.meta?.cover || undefined,
    ar: lookupData?.meta?.ar ?? fallbackAr,
    titleMeta: lookupData?.meta,
    haskey: lookupData?.haskey,
    pending: lookupQ.isLoading,
    identified: lookupData ? lookupData.ok : undefined,
  }
}
