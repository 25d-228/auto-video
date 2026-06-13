/**
 * TanStack Query hooks over the typed sidecar client (src/api/client.ts).
 *
 * Conventions:
 * - all keys are namespaced under ["sidecar", …] — see `qk` below; after a
 *   savePath/saveKey/scan, invalidate qk.paths() / qk.keys() / qk.library().
 * - listings (discover/seeders/library) stay fresh for 5 minutes, matching
 *   the sidecar's own listing cache; covers/meta/lookups never go stale.
 * - `fresh` is part of the discover key: flip it to true to bypass the
 *   sidecar's 5-minute listing cache (a forced live refetch).
 */
import { useQuery } from "@tanstack/react-query"
import {
  cover,
  discover,
  keys,
  library,
  meta,
  movieLookup,
  paths,
  seeders,
  stats,
  tvLookup,
} from "@/api/client"
import type { Cat } from "@/api/types"

export const LISTING_STALE_MS = 5 * 60 * 1000
const HOUR_MS = 60 * 60 * 1000

/** Query keys (exported so views can invalidate/prefetch). */
export const qk = {
  all: ["sidecar"] as const,
  discover: (cat: Cat, source: string, list: string, n: number, fresh: boolean) =>
    ["sidecar", "discover", cat, source, list, n, fresh] as const,
  seeders: (cat: Cat, title: string, code: string, year: string) =>
    ["sidecar", "seeders", cat, title, code, year] as const,
  library: () => ["sidecar", "library"] as const,
  stats: () => ["sidecar", "stats"] as const,
  paths: () => ["sidecar", "paths"] as const,
  keys: () => ["sidecar", "keys"] as const,
  meta: (cid: string, code: string, cat: Cat | "") =>
    ["sidecar", "meta", cid, code, cat] as const,
  cover: (code: string) => ["sidecar", "cover", code] as const,
  titleLookup: (tv: boolean, title: string, year: string) =>
    ["sidecar", "lookup", tv ? "tv" : "movie", title, year] as const,
}

// ------------------------------------------------------------------ feeds

/**
 * Live Discover feed. `source` is a provider id (e.g. "tmdb" | "yts" |
 * "javdb"); `list` is a list id within that provider (e.g. "trending" |
 * "popular" | "newest").
 */
export function useDiscover(
  cat: Cat,
  source: string,
  list: string,
  n = 100,
  fresh = false
) {
  return useQuery({
    queryKey: qk.discover(cat, source, list, n, fresh),
    queryFn: () => discover({ cat, source, list, n, fresh: fresh || undefined }),
    staleTime: LISTING_STALE_MS,
  })
}

/** The card/item shape useSeeders needs (DiscoverItem and LibraryItem both fit). */
export interface SeederSubject {
  cat: Cat
  title?: string
  code?: string
  year?: string | number
  /** Fallback year source — the old app's dlYear() scanned `year || sub`. */
  sub?: string
}

function subjectYear(item: SeederSubject): string {
  const m = /(19|20)\d{2}/.exec(String(item.year || item.sub || ""))
  return m ? m[0] : ""
}

/** Real releases + magnets for one item, merged across the seeder sites. */
export function useSeeders(item: SeederSubject | null | undefined) {
  const cat = item?.cat ?? "mov"
  const title = item?.title ?? ""
  const code = item?.code ?? ""
  const year = item ? subjectYear(item) : ""
  return useQuery({
    queryKey: qk.seeders(cat, title, code, year),
    queryFn: () =>
      seeders({ cat, title, code: code || undefined, year: year || undefined }),
    enabled: item != null && (title !== "" || code !== ""),
    staleTime: LISTING_STALE_MS,
  })
}

// ------------------------------------------------------------------ library

/** Recursive scan of the configured folders (real paths + lazy covers). */
export function useLibrary() {
  return useQuery({
    queryKey: qk.library(),
    queryFn: library,
    staleTime: LISTING_STALE_MS,
  })
}

/** Disk usage / file counts per category for the Dashboard. */
export function useStats() {
  return useQuery({
    queryKey: qk.stats(),
    queryFn: stats,
    staleTime: 60 * 1000,
  })
}

export const STATS_LIVE_REFETCH_MS = 30_000

/**
 * Live variant of useStats() for the Dashboard. Shares the cache entry
 * (same key + fetcher) but adds a 30 s refetch interval (paused while the
 * window is unfocused) and an always-refetch-on-focus override of the
 * app-wide `refetchOnWindowFocus: false` default. The interval only runs
 * while a consumer is mounted, i.e. while the view is actually visible.
 */
export function useStatsLive() {
  return useQuery({
    queryKey: qk.stats(),
    queryFn: stats,
    staleTime: STATS_LIVE_REFETCH_MS,
    refetchInterval: STATS_LIVE_REFETCH_MS,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: "always",
  })
}

/** Configured folder per category; invalidate qk.paths() after savePath(). */
export function usePaths() {
  return useQuery({
    queryKey: qk.paths(),
    queryFn: paths,
    staleTime: LISTING_STALE_MS,
  })
}

/** Stored provider keys; invalidate qk.keys() after saveKey(). */
export function useKeys() {
  return useQuery({
    queryKey: qk.keys(),
    queryFn: keys,
    staleTime: LISTING_STALE_MS,
  })
}

// ------------------------------------------------------------------ metadata

export interface MetaArgs {
  /** r18.dev content id (parsed from a proxied cover URL). */
  cid?: string
  /** JAV code, e.g. "ABCD-123". */
  code?: string
  cat?: Cat
}

/** JAV metadata (jatitle / cast / date / runtime). Disabled until cid or code exists. */
export function useMeta({ cid, code, cat }: MetaArgs) {
  const cidv = cid ?? ""
  const codev = code ?? ""
  return useQuery({
    queryKey: qk.meta(cidv, codev, cat ?? ""),
    queryFn: () =>
      meta({ cid: cidv || undefined, code: codev || undefined, cat }),
    enabled: cidv !== "" || codev !== "",
    staleTime: Infinity,
    gcTime: HOUR_MS,
  })
}

/** JAV cover by code; `data.ok === false` just means "no cover found". */
export function useCover(code: string | undefined) {
  const c = code ?? ""
  return useQuery({
    queryKey: qk.cover(c),
    queryFn: () => cover(c),
    enabled: c !== "",
    staleTime: Infinity,
    gcTime: HOUR_MS,
  })
}

/** TMDB lookup (tv=true -> /tv with AniList fallback). Check `data.haskey` before treating ok:false as missing. */
export function useTitleLookup(
  tv: boolean,
  title: string | undefined,
  year?: string | number
) {
  const t = title ?? ""
  const y = year === undefined || year === "" ? "" : String(year)
  return useQuery({
    queryKey: qk.titleLookup(tv, t, y),
    queryFn: () => (tv ? tvLookup : movieLookup)({ title: t, year: y || undefined }),
    enabled: t !== "",
    staleTime: Infinity,
    gcTime: HOUR_MS,
  })
}
