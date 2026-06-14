/**
 * Pure-TypeScript API facade — the in-process replacement for the old Python
 * sidecar client. Same exported names, parameter objects, and response shapes
 * as the HTTP client had (src/api/types.ts), but every call now runs the TS
 * aggregators directly (no localhost HTTP):
 *
 *   - discover/seeders   -> src/api/discover.ts / src/api/seeders.ts
 *   - meta/cover/lookups -> src/api/meta.ts / src/api/covers.ts
 *   - paths/keys         -> the SQLite store (src/state/db.ts)
 *   - library/stats      -> src/api/library.ts (re-exported)
 *
 * Failures throw — ClientError for the feed aggregators (mirroring the old
 * `{ok:false, err}` replies) and the store's own DbUnavailableError on a
 * non-Tauri host — so TanStack Query still sees them as query errors.
 */
import { resolveJavCover } from "@/api/covers"
import { normalizeCodeNum } from "@/lib/codes"
import {
  discover as buildDiscover,
  resolveList,
  type DiscoverOpts,
} from "@/api/discover"
import { metaLookup, titleLookup } from "@/api/meta"
import { seeders as buildSeeders } from "@/api/seeders"
import { tmdbKey } from "@/api/sources/tmdb"
import {
  allKeys,
  allPaths,
  getKey,
  isDbAvailable,
  setKey,
  setPath,
} from "@/state/db"
import type {
  Cat,
  CoverResponse,
  DiscoverResponse,
  JavMeta,
  KeysRecord,
  PathsRecord,
  SaveKeyResponse,
  SavePathResponse,
  SeedersResponse,
  TitleLookupResponse,
} from "./types"

/** Thrown when a feed aggregator fails (the old `{ok:false, err}` reply). */
export class ClientError extends Error {
  constructor(message: string) {
    super(message)
    this.name = "ClientError"
  }
}

/** Re-throw any aggregator failure as a ClientError (preserving the cause). */
function toClientError(e: unknown): never {
  if (e instanceof ClientError) throw e
  const msg = e instanceof Error ? e.message : String(e)
  const err = new ClientError(msg)
  ;(err as { cause?: unknown }).cause = e
  throw err
}

/** Read the javbus cookie (provider key); "" when unset / DB unavailable. */
async function javbusCookie(): Promise<string> {
  if (!isDbAvailable()) return ""
  try {
    return (await getKey("javbus"))?.trim() ?? ""
  } catch {
    return ""
  }
}

// ------------------------------------------------------------------ feeds

export interface DiscoverParams {
  cat?: Cat
  /** Provider id, e.g. "tmdb", "imdb", "yts", "javdb", "mgstage". */
  source?: string
  /** List id within the provider, e.g. "trending", "popular", "newest". */
  list?: string
  /** 1..100, default 50. */
  n?: number
  /** Bypass the 5-minute listing cache. */
  fresh?: boolean
  /** Per-provider controls (JavDB VR year/month/sort). */
  opts?: DiscoverOpts
}

/**
 * Build a Discover feed. Unknown source/list fall back to the category's
 * default provider / the provider's default list (discover.ts resolveList);
 * `source` in the response reports the resolved "<provider>/<list>".
 */
export async function discover(
  params: DiscoverParams = {}
): Promise<DiscoverResponse> {
  const cat = params.cat ?? "mov"
  const n = Math.max(1, Math.min(100, params.n ?? 50))
  const fresh = params.fresh ?? false
  const source = (params.source ?? "").toLowerCase()
  const list = (params.list ?? "").toLowerCase()
  try {
    const items = await buildDiscover(cat, source, list, n, fresh, params.opts ?? {})
    const resolved = resolveList(cat, source, list)
    return {
      ok: true,
      items,
      count: items.length,
      updated: new Date().toISOString(),
      source: `${resolved.source}/${resolved.list}`,
    }
  } catch (e) {
    toClientError(e)
  }
}

export interface SeedersParams {
  cat?: Cat
  title?: string
  code?: string
  year?: string | number
  /** Accepted for API compatibility; the TS aggregator has no listing cache. */
  fresh?: boolean
}

/**
 * Real releases + magnets for one item, merged across the seeder sites.
 * `releases` is the top 25 by seeders; `count`/`totalSeed`/`sources` describe
 * the full set found.
 */
export async function seeders(
  params: SeedersParams = {}
): Promise<SeedersResponse> {
  const cat = params.cat ?? "mov"
  try {
    // already deduped + sorted by seeders desc
    const rels = await buildSeeders(
      cat,
      params.title ?? "",
      params.code ?? "",
      params.year
    )
    const sources: Record<string, number> = {}
    let totalSeed = 0
    for (const r of rels) {
      sources[r.source] = (sources[r.source] ?? 0) + 1
      totalSeed += r.seeders || 0
    }
    return {
      ok: true,
      releases: rels.slice(0, 25),
      count: rels.length,
      topSeed: rels.length > 0 ? rels[0]!.seeders || 0 : 0,
      totalSeed,
      sources,
    }
  } catch (e) {
    toClientError(e)
  }
}

// ------------------------------------------------------------------ library

export { library, stats } from "./library"

/** Configured folder per category, straight from the SQLite store. */
export function paths(): Promise<PathsRecord> {
  return allPaths()
}

/** Persist one category's folder; an empty path clears the entry. */
export async function savePath(
  cat: Cat,
  path: string
): Promise<SavePathResponse> {
  await setPath(cat, path) // setPath deletes the row when path === ""
  return { ok: true }
}

// ------------------------------------------------------------------ keys

/** Stored provider keys, straight from the SQLite store. */
export function keys(): Promise<KeysRecord> {
  return allKeys()
}

/** Persist a provider key (e.g. "tmdb", "javbus"); empty value clears it. */
export async function saveKey(
  provider: string,
  key: string
): Promise<SaveKeyResponse> {
  await setKey(provider, key) // setKey deletes the row when key === ""
  return { ok: true, tmdb: Boolean(await tmdbKey()) }
}

// ------------------------------------------------------------------ metadata

export interface MetaParams {
  /** r18.dev content id. */
  cid?: string
  /** JAV code, e.g. "ABCD-123". */
  code?: string
  cat?: Cat
}

/** JAV metadata (r18.dev / javdatabase). Returns {} when nothing was found. */
export function meta(params: MetaParams): Promise<JavMeta> {
  return metaLookup(params)
}

/**
 * Resolve a JAV cover by code. `ok:false` just means no cover found. `fresh` is
 * accepted for API compatibility; the resolver's persistent cover cache (1-day
 * TTL in covers.ts) applies either way.
 */
export async function cover(
  code: string,
  fresh?: boolean
): Promise<CoverResponse> {
  void fresh
  if (!code) return { ok: false, cover: "", ar: 0 }
  const jb = await javbusCookie()
  let r = await resolveJavCover(code, jb)
  if (!r.url) {
    // On-disk codes are sometimes over-padded (e.g. "MIVR-00081"); retry once
    // with the canonical 3-digit form. Fallback-only, so codes that already
    // resolve at their padded form are unaffected.
    const alt = normalizeCodeNum(code)
    if (alt && alt !== code) r = await resolveJavCover(alt, jb)
  }
  return { ok: r.url !== "", cover: r.url, ar: r.ar }
}

export interface TitleLookupParams {
  title: string
  year?: string | number
  fresh?: boolean
}

/** TMDB movie lookup. Inspect `haskey` before treating `ok:false` as missing. */
export function movieLookup(
  params: TitleLookupParams
): Promise<TitleLookupResponse> {
  return titleLookup(false, params.title, String(params.year ?? ""), {
    fresh: params.fresh,
  })
}

/** TMDB TV lookup (AniList cover fallback built into titleLookup). */
export function tvLookup(
  params: TitleLookupParams
): Promise<TitleLookupResponse> {
  return titleLookup(true, params.title, String(params.year ?? ""), {
    fresh: params.fresh,
  })
}
