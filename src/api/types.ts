/**
 * Shared API response shapes used by client.ts and the aggregators
 * (discover/seeders/meta/covers/library).
 */

/** Library/category id. */
export type Cat = "mov" | "tv" | "ad" | "vrc"

export type DiscoverMode = "trending" | "newest"

/** Failing endpoints reply with this shape (HTTP status stays 200). */
export interface ApiError {
  ok: false
  err: string
}

/** One card in the Discover feed. */
export interface DiscoverItem {
  id: string
  cat: Cat
  title: string
  sub: string
  cover: string
  /** Cover aspect ratio (w/h), e.g. 0.675 for YTS posters. */
  ar: number
  seeders: number
  size: string
  src: string
  state: string
  /** YTS sends a number, every other source a string (possibly ""). */
  year: string | number
  runtime: number
  rating: number
  code: string
  /** Set by javdb / TMDB / IMDb sources ("" on IMDb items). */
  date?: string
  /** Original feed position; only set by javdb / TMDB / IMDb / MGStage. */
  added?: number
  /** Only present on sukebei items. */
  magnet?: string
  /** URL of the item's original page on the source site (built during fetch). */
  link?: string
}

export interface DiscoverResponse {
  ok: true
  items: DiscoverItem[]
  count: number
  updated: string
  source: string
}

/** One release row in the download dialog (/seeders). */
export interface Release {
  name: string
  source: string
  seeders: number
  size: string
  magnet: string
  quality: string
}

export interface SeedersResponse {
  ok: true
  /** Top 25 by seeders. */
  releases: Release[]
  /** Total releases found (before the cut to 25). */
  count: number
  topSeed: number
  totalSeed: number
  /** Release count per source name, e.g. { TPB: 12, YTS: 6 }. */
  sources: Record<string, number>
}

/** One scanned file from the configured library folders (/library). */
export interface LibraryItem {
  fname: string
  path: string
  cat: Cat
  /** Human-readable, e.g. "4.6 GB". */
  size: string
  state: string
  cover: string
  ar: number
  /** Parsed JAV code; ad/vrc items only. */
  code?: string
  title: string
  /** ad/vrc items only. */
  vr?: boolean
  sub: string
  /** mov items only. */
  year?: string
}

export interface LibraryResponse {
  ok: true
  items: LibraryItem[]
  count: number
  /** File count per category. */
  counts: Partial<Record<Cat, number>>
}

/** Disk/folder facts for one category (/stats). */
export interface DiskStats {
  path: string
  online: boolean
  /** Bytes. */
  free: number
  /** Bytes. */
  total: number
  files: number
}

export interface StatsResponse {
  ok: true
  disks: Record<Cat, DiskStats>
}

/** /paths returns the raw store: category -> absolute folder path. */
export type PathsRecord = Partial<Record<Cat, string>>

/** /keys returns the raw store: provider name -> key/cookie value. */
export type KeysRecord = Record<string, string>

export interface SavePathResponse {
  ok: true
}

export interface SaveKeyResponse {
  ok: true
  /** Whether a TMDB key is configured after the save. */
  tmdb: boolean
}

export interface CoverResponse {
  /** false simply means "no cover found", not an error. */
  ok: boolean
  cover: string
  ar: number
}

/** TMDB metadata record for /movie and /tv. */
export interface TitleMeta {
  tmdb_id?: number
  cover?: string
  ar?: number
  /** "YYYY-MM-DD". */
  date?: string
  year?: string
  /** e.g. "118 min". */
  runtime?: string
  /** Up to 3 genres, comma-joined. */
  genre?: string
  /** Up to 5 names, comma-joined. */
  cast?: string
  tmdb_title?: string
  overview?: string
}

export interface TitleLookupResponse {
  /** true only when a cover was resolved. */
  ok: boolean
  /** false when no TMDB key is configured. */
  haskey: boolean
  /** Missing in the no-key / empty-title replies. */
  meta?: TitleMeta
}

/** /meta record (r18.dev or javdatabase). May be empty. */
export interface JavMeta {
  /** Japanese title (r18.dev). */
  jatitle?: string
  /** "YYYY-MM-DD". */
  date?: string
  /** e.g. "120 min". */
  runtime?: string
  /** Japanese cast names (r18.dev). */
  cast_ja?: string
  /** Romanized cast (javdatabase). */
  cast?: string
  /** Internal: FANZA content id scraped from the javdatabase page, used to
   *  fetch Japanese cast from r18.dev. Stripped before the record is returned. */
  _cid?: string
}
