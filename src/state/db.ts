/**
 * SQLite store layer over @tauri-apps/plugin-sql.
 *
 * Phase-2 foundation: this is the eventual replacement for the Python sidecar's
 * JSON side-files (sidecar/av_proxy.py) — av_keys.json, av_paths.json,
 * proxy_cache.json (listings), discover_covers.json (covers), and the scanned
 * library. Nothing here is wired into the app yet; it only ADDS the storage
 * primitives the TS scrapers (next step) will use.
 *
 * Schema (all `CREATE TABLE IF NOT EXISTS`):
 *   provider_keys(provider PK, value)            -- keys_store: tmdb/javbus/...
 *   library_paths(cat PK, path)                  -- paths_store: cat -> folder
 *   listing_cache(key PK, json, fetched_at)      -- TTL cache of a feed page
 *   cover_cache(code PK, url, ar, fetched_at)    -- TTL cache of a resolved cover
 *   meta_cache(key PK, json, fetched_at)         -- TTL cache of a /meta record
 *   library_index(path PK, cat, fname, size,     -- the scanned library
 *                 code, title, vr, year, sub, cover, ar)
 *
 * Tauri guard: Database.load() throws outside a Tauri webview (plain browser /
 * vitest / SSR). initDb() swallows that and leaves the module unavailable;
 * isDbAvailable() lets callers degrade gracefully (fall back to the sidecar).
 */
import Database from "@tauri-apps/plugin-sql"
import type { Cat } from "@/api/types"

export const DB_URL = "sqlite:autovideo.db"

// SQLite stores booleans as 0/1; library_index.vr round-trips through this.
type SqlBool = 0 | 1

/**
 * One row of the scanned library. Mirrors the sidecar's scan_library() output
 * (av_proxy.py) and the LibraryItem API shape, but flattened for one SQL row:
 * `vr` is 0/1 here (not boolean) and every column is always present.
 */
export interface LibraryRow {
  /** Absolute file path — primary key. */
  path: string
  cat: Cat
  fname: string
  /** Human-readable size, e.g. "4.6 GB". */
  size: string
  /** Parsed JAV code (ad/vrc); "" otherwise. */
  code: string
  title: string
  /** 1 for VR titles, else 0. */
  vr: SqlBool
  /** Release year (mov); "" otherwise. */
  year: string
  sub: string
  cover: string
  /** Cover aspect ratio (w/h). */
  ar: number
}

/** A TTL cache table whose value is a single JSON blob keyed by `key`. */
type JsonCacheTable = "listing_cache" | "meta_cache"

// ------------------------------------------------------------------ migrations

/**
 * DDL run on every initDb(). Idempotent (IF NOT EXISTS). Kept as one string per
 * statement because plugin-sql's execute() runs a single statement at a time.
 */
const MIGRATIONS: readonly string[] = [
  `CREATE TABLE IF NOT EXISTS provider_keys (
     provider TEXT PRIMARY KEY,
     value    TEXT NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS library_paths (
     cat  TEXT PRIMARY KEY,
     path TEXT NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS listing_cache (
     key        TEXT PRIMARY KEY,
     json       TEXT NOT NULL,
     fetched_at INTEGER NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS cover_cache (
     code       TEXT PRIMARY KEY,
     url        TEXT NOT NULL,
     ar         REAL NOT NULL,
     fetched_at INTEGER NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS meta_cache (
     key        TEXT PRIMARY KEY,
     json       TEXT NOT NULL,
     fetched_at INTEGER NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS library_index (
     path  TEXT PRIMARY KEY,
     cat   TEXT NOT NULL,
     fname TEXT NOT NULL,
     size  TEXT NOT NULL,
     code  TEXT NOT NULL,
     title TEXT NOT NULL,
     vr    INTEGER NOT NULL,
     year  TEXT NOT NULL,
     sub   TEXT NOT NULL,
     cover TEXT NOT NULL,
     ar    REAL NOT NULL
   )`,
  `CREATE INDEX IF NOT EXISTS library_index_cat ON library_index (cat)`,
  `CREATE TABLE IF NOT EXISTS downloads (
     id         TEXT PRIMARY KEY,
     magnet     TEXT NOT NULL,
     dest       TEXT NOT NULL,
     title      TEXT NOT NULL,
     only_files TEXT,
     renames    TEXT,
     added_at   INTEGER NOT NULL
   )`,
]

/** Idempotent column adds for tables that predate a field (SQLite lacks IF NOT
 *  EXISTS on ADD COLUMN, so we run these tolerantly — a duplicate just errors). */
const COLUMN_ADDITIONS: readonly string[] = [
  "ALTER TABLE downloads ADD COLUMN only_files TEXT",
  "ALTER TABLE downloads ADD COLUMN renames TEXT",
]

// ------------------------------------------------------------------ connection

let dbPromise: Promise<Database> | null = null
let available = false

/**
 * Load the DB (once) and run the CREATE-TABLE migrations. Safe to call many
 * times — the load + migration only happen on the first call; later calls
 * return the same connection. On any failure (e.g. running outside Tauri, where
 * Database.load throws) it resolves to null and isDbAvailable() stays false so
 * callers can degrade to the sidecar.
 */
export async function initDb(): Promise<Database | null> {
  if (dbPromise) {
    try {
      return await dbPromise
    } catch {
      return null
    }
  }
  dbPromise = (async () => {
    const db = await Database.load(DB_URL)
    for (const sql of MIGRATIONS) {
      await db.execute(sql)
    }
    // Tolerant column adds (ignore "duplicate column" on already-migrated DBs).
    for (const sql of COLUMN_ADDITIONS) {
      try {
        await db.execute(sql)
      } catch {
        /* column already exists */
      }
    }
    return db
  })()
  try {
    const db = await dbPromise
    available = true
    return db
  } catch {
    // Plain browser / vitest / SSR: no Tauri bridge. Reset so a later call
    // (e.g. once running inside Tauri) can retry the load.
    dbPromise = null
    available = false
    return null
  }
}

/** True once initDb() has successfully loaded the DB in this process. */
export function isDbAvailable(): boolean {
  return available
}

/** Resolve the live connection, or throw if the DB is unavailable. */
async function requireDb(): Promise<Database> {
  const db = await initDb()
  if (!db) throw new DbUnavailableError()
  return db
}

/** Thrown by CRUD helpers when the DB could not be loaded (non-Tauri host). */
export class DbUnavailableError extends Error {
  constructor() {
    super("SQLite store unavailable (not running inside Tauri)")
    this.name = "DbUnavailableError"
  }
}

// ------------------------------------------------------------------ provider_keys

/** Read one provider key/cookie (tmdb, javbus, …). Returns null if unset. */
export async function getKey(provider: string): Promise<string | null> {
  const db = await requireDb()
  const rows = await db.select<{ value: string }[]>(
    "SELECT value FROM provider_keys WHERE provider = $1",
    [provider]
  )
  return rows.length > 0 ? rows[0].value : null
}

/** Persist a provider key; an empty value DELETES the entry (matches the sidecar). */
export async function setKey(provider: string, value: string): Promise<void> {
  const db = await requireDb()
  if (value === "") {
    await db.execute("DELETE FROM provider_keys WHERE provider = $1", [provider])
    return
  }
  await db.execute(
    `INSERT INTO provider_keys (provider, value) VALUES ($1, $2)
     ON CONFLICT(provider) DO UPDATE SET value = excluded.value`,
    [provider, value]
  )
}

/** All provider keys as a plain record (mirrors keys_store / KeysRecord). */
export async function allKeys(): Promise<Record<string, string>> {
  const db = await requireDb()
  const rows = await db.select<{ provider: string; value: string }[]>(
    "SELECT provider, value FROM provider_keys"
  )
  const out: Record<string, string> = {}
  for (const r of rows) out[r.provider] = r.value
  return out
}

// ------------------------------------------------------------------ library_paths

/** Read one category's configured library folder. Returns null if unset. */
export async function getPath(cat: Cat): Promise<string | null> {
  const db = await requireDb()
  const rows = await db.select<{ path: string }[]>(
    "SELECT path FROM library_paths WHERE cat = $1",
    [cat]
  )
  return rows.length > 0 ? rows[0].path : null
}

/** Persist a category's folder; an empty path DELETES the entry. */
export async function setPath(cat: Cat, path: string): Promise<void> {
  const db = await requireDb()
  if (path === "") {
    await db.execute("DELETE FROM library_paths WHERE cat = $1", [cat])
    return
  }
  await db.execute(
    `INSERT INTO library_paths (cat, path) VALUES ($1, $2)
     ON CONFLICT(cat) DO UPDATE SET path = excluded.path`,
    [cat, path]
  )
}

/** All configured paths as a partial record (mirrors paths_store / PathsRecord). */
export async function allPaths(): Promise<Partial<Record<Cat, string>>> {
  const db = await requireDb()
  const rows = await db.select<{ cat: Cat; path: string }[]>(
    "SELECT cat, path FROM library_paths"
  )
  const out: Partial<Record<Cat, string>> = {}
  for (const r of rows) out[r.cat] = r.path
  return out
}

// ------------------------------------------------------------------ TTL caches

/** Seconds → ms helper kept local so the TTL math reads clearly. */
function nowSeconds(): number {
  return Math.floor(Date.now() / 1000)
}

/**
 * Read a JSON cache entry if it is younger than `ttlSeconds`. Returns the parsed
 * value (typed as T) on a fresh hit, or null on a miss / stale / parse error.
 * The stale row is left in place; setCached overwrites it on the next refetch.
 *
 * `ttlSeconds <= 0` is treated as "never fresh" — always a miss (forced refetch).
 */
export async function getCached<T>(
  table: JsonCacheTable,
  key: string,
  ttlSeconds: number
): Promise<T | null> {
  if (ttlSeconds <= 0) return null
  const db = await requireDb()
  const rows = await db.select<{ json: string; fetched_at: number }[]>(
    `SELECT json, fetched_at FROM ${table} WHERE key = $1`,
    [key]
  )
  if (rows.length === 0) return null
  const { json, fetched_at } = rows[0]
  if (nowSeconds() - fetched_at >= ttlSeconds) return null
  try {
    return JSON.parse(json) as T
  } catch {
    return null
  }
}

/** Write (upsert) a JSON cache entry, stamping it with the current time. */
export async function setCached(
  table: JsonCacheTable,
  key: string,
  value: unknown
): Promise<void> {
  const db = await requireDb()
  await db.execute(
    `INSERT INTO ${table} (key, json, fetched_at) VALUES ($1, $2, $3)
     ON CONFLICT(key) DO UPDATE SET
       json = excluded.json,
       fetched_at = excluded.fetched_at`,
    [key, JSON.stringify(value), nowSeconds()]
  )
}

/** A resolved cover row (cover_cache uses `code` as PK and carries the aspect ratio). */
export interface CachedCover {
  url: string
  ar: number
}

/**
 * Read a cached cover for a JAV code if younger than `ttlSeconds`. Returns null
 * on miss/stale. (cover_cache is its own table because covers store url + ar
 * rather than a JSON blob.)
 */
export async function getCachedCover(
  code: string,
  ttlSeconds: number
): Promise<CachedCover | null> {
  if (ttlSeconds <= 0) return null
  const db = await requireDb()
  const rows = await db.select<{ url: string; ar: number; fetched_at: number }[]>(
    "SELECT url, ar, fetched_at FROM cover_cache WHERE code = $1",
    [code]
  )
  if (rows.length === 0) return null
  const { url, ar, fetched_at } = rows[0]
  if (nowSeconds() - fetched_at >= ttlSeconds) return null
  return { url, ar }
}

/** Write (upsert) a resolved cover, stamping it with the current time. */
export async function setCachedCover(
  code: string,
  url: string,
  ar: number
): Promise<void> {
  const db = await requireDb()
  await db.execute(
    `INSERT INTO cover_cache (code, url, ar, fetched_at) VALUES ($1, $2, $3, $4)
     ON CONFLICT(code) DO UPDATE SET
       url = excluded.url,
       ar = excluded.ar,
       fetched_at = excluded.fetched_at`,
    [code, url, ar, nowSeconds()]
  )
}

// ------------------------------------------------------------------ downloads

/** A planned post-download rename (source -> target, relative to dest). */
export interface DownloadRename {
  from: string
  to: string
}

/** One persisted, resumable download (the magnet + where it writes). */
export interface DownloadRow {
  id: string
  magnet: string
  dest: string
  title: string
  /** Torrent file indices the user picked; undefined/empty = all files. */
  onlyFiles?: number[]
  /** Canonical renames to apply on completion (from planRename). */
  renames?: DownloadRename[]
}

/**
 * Remember an in-flight download so it can be resumed after the app quits.
 * Re-adding the magnet to librqbit with the same `dest` (and the same file
 * selection + rename plan) resumes from the partial files already on disk.
 */
export async function saveDownload(rec: DownloadRow): Promise<void> {
  const db = await requireDb()
  const only = rec.onlyFiles && rec.onlyFiles.length > 0 ? JSON.stringify(rec.onlyFiles) : null
  const ren = rec.renames && rec.renames.length > 0 ? JSON.stringify(rec.renames) : null
  await db.execute(
    `INSERT INTO downloads (id, magnet, dest, title, only_files, renames, added_at) VALUES ($1, $2, $3, $4, $5, $6, $7)
     ON CONFLICT(id) DO UPDATE SET
       magnet = excluded.magnet, dest = excluded.dest, title = excluded.title,
       only_files = excluded.only_files, renames = excluded.renames`,
    [rec.id, rec.magnet, rec.dest, rec.title, only, ren, Date.now()]
  )
}

/** Forget a download (completed or cancelled) so it is not resumed next launch. */
export async function removeDownload(id: string): Promise<void> {
  const db = await requireDb()
  await db.execute("DELETE FROM downloads WHERE id = $1", [id])
}

/** Every persisted download, oldest first — the queue to resume on launch. */
export async function allDownloads(): Promise<DownloadRow[]> {
  const db = await requireDb()
  const rows = await db.select<
    {
      id: string
      magnet: string
      dest: string
      title: string
      only_files: string | null
      renames: string | null
    }[]
  >("SELECT id, magnet, dest, title, only_files, renames FROM downloads ORDER BY added_at ASC")
  const parse = <T>(s: string | null): T | undefined => {
    if (!s) return undefined
    try {
      return JSON.parse(s) as T
    } catch {
      return undefined
    }
  }
  return rows.map((r) => ({
    id: r.id,
    magnet: r.magnet,
    dest: r.dest,
    title: r.title,
    onlyFiles: parse<number[]>(r.only_files),
    renames: parse<DownloadRename[]>(r.renames),
  }))
}

// ------------------------------------------------------------------ library_index

/**
 * Upsert one scanned library row (keyed by absolute path). Re-scanning the same
 * file overwrites its row (size/code/cover may have changed).
 */
export async function libraryUpsert(row: LibraryRow): Promise<void> {
  const db = await requireDb()
  await db.execute(
    `INSERT INTO library_index
       (path, cat, fname, size, code, title, vr, year, sub, cover, ar)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
     ON CONFLICT(path) DO UPDATE SET
       cat = excluded.cat,
       fname = excluded.fname,
       size = excluded.size,
       code = excluded.code,
       title = excluded.title,
       vr = excluded.vr,
       year = excluded.year,
       sub = excluded.sub,
       cover = excluded.cover,
       ar = excluded.ar`,
    [
      row.path,
      row.cat,
      row.fname,
      row.size,
      row.code,
      row.title,
      row.vr,
      row.year,
      row.sub,
      row.cover,
      row.ar,
    ]
  )
}

/** All scanned rows for one category (sorted by filename for a stable list). */
export async function libraryQueryByCat(cat: Cat): Promise<LibraryRow[]> {
  const db = await requireDb()
  return db.select<LibraryRow[]>(
    "SELECT * FROM library_index WHERE cat = $1 ORDER BY fname",
    [cat]
  )
}

/**
 * Clear the scanned library. With a `cat` it clears just that category (e.g.
 * before re-scanning one folder); with no argument it truncates the whole index.
 */
export async function libraryClear(cat?: Cat): Promise<void> {
  const db = await requireDb()
  if (cat === undefined) {
    await db.execute("DELETE FROM library_index")
    return
  }
  await db.execute("DELETE FROM library_index WHERE cat = $1", [cat])
}
