import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

/**
 * In-memory fake of @tauri-apps/plugin-sql's Database. It records every
 * execute()/select() call (sql + params) so we can assert the exact SQL and
 * bind values the db.ts CRUD helpers emit, and it lets a test queue up canned
 * select() results to exercise the read paths + TTL logic — no real DB needed.
 */
interface Call {
  sql: string
  params: unknown[]
}

class FakeDatabase {
  static loadShouldThrow = false
  static instances: FakeDatabase[] = []

  executeCalls: Call[] = []
  selectCalls: Call[] = []
  // FIFO queue of canned results returned by successive select() calls.
  private selectResults: unknown[] = []

  static async load(_path: string): Promise<FakeDatabase> {
    if (FakeDatabase.loadShouldThrow) {
      throw new Error("no Tauri bridge")
    }
    const inst = new FakeDatabase()
    FakeDatabase.instances.push(inst)
    return inst
  }

  /** Test helper: queue the rows the next select() should resolve to. */
  queueSelect(rows: unknown): void {
    this.selectResults.push(rows)
  }

  async execute(sql: string, params: unknown[] = []) {
    this.executeCalls.push({ sql, params })
    return { rowsAffected: 1 }
  }

  async select<T>(sql: string, params: unknown[] = []): Promise<T> {
    this.selectCalls.push({ sql, params })
    const next = this.selectResults.shift()
    return (next ?? []) as T
  }
}

vi.mock("@tauri-apps/plugin-sql", () => ({
  default: FakeDatabase,
}))

// Imported after the mock is registered. Re-imported fresh per test via
// vi.resetModules() so the module-level connection singleton starts clean.
type DbModule = typeof import("@/state/db")

async function loadDbModule(): Promise<{
  db: FakeDatabase
  mod: DbModule
}> {
  vi.resetModules()
  FakeDatabase.loadShouldThrow = false
  FakeDatabase.instances = []
  const mod = await import("@/state/db")
  const handle = await mod.initDb()
  expect(handle).not.toBeNull()
  const db = FakeDatabase.instances[0]
  return { db, mod }
}

/** Normalize SQL whitespace so multi-line template strings compare by tokens. */
function norm(sql: string): string {
  return sql.replace(/\s+/g, " ").trim()
}

function lastExecute(db: FakeDatabase): Call {
  return db.executeCalls[db.executeCalls.length - 1]
}

beforeEach(() => {
  vi.useRealTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe("initDb / isDbAvailable", () => {
  it("loads the DB and runs the CREATE TABLE migrations", async () => {
    const { db, mod } = await loadDbModule()
    expect(mod.isDbAvailable()).toBe(true)

    const ddl = db.executeCalls.map((c) => norm(c.sql))
    expect(ddl.some((s) => s.includes("CREATE TABLE IF NOT EXISTS provider_keys"))).toBe(true)
    expect(ddl.some((s) => s.includes("CREATE TABLE IF NOT EXISTS library_paths"))).toBe(true)
    expect(ddl.some((s) => s.includes("CREATE TABLE IF NOT EXISTS listing_cache"))).toBe(true)
    expect(ddl.some((s) => s.includes("CREATE TABLE IF NOT EXISTS cover_cache"))).toBe(true)
    expect(ddl.some((s) => s.includes("CREATE TABLE IF NOT EXISTS meta_cache"))).toBe(true)
    expect(ddl.some((s) => s.includes("CREATE TABLE IF NOT EXISTS library_index"))).toBe(true)
  })

  it("loads the DB exactly once across repeated initDb() calls", async () => {
    const { mod } = await loadDbModule()
    const before = FakeDatabase.instances.length
    await mod.initDb()
    await mod.initDb()
    expect(FakeDatabase.instances.length).toBe(before)
  })

  it("degrades gracefully when Database.load throws (non-Tauri host)", async () => {
    vi.resetModules()
    FakeDatabase.loadShouldThrow = true
    FakeDatabase.instances = []
    const mod = await import("@/state/db")

    const handle = await mod.initDb()
    expect(handle).toBeNull()
    expect(mod.isDbAvailable()).toBe(false)

    // CRUD helpers throw DbUnavailableError rather than hitting a missing DB.
    await expect(mod.getKey("tmdb")).rejects.toBeInstanceOf(mod.DbUnavailableError)
  })
})

describe("provider_keys CRUD", () => {
  it("getKey selects by provider and returns the value", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([{ value: "secret-key" }])
    const v = await mod.getKey("tmdb")
    expect(v).toBe("secret-key")
    const call = db.selectCalls[db.selectCalls.length - 1]
    expect(norm(call.sql)).toBe("SELECT value FROM provider_keys WHERE provider = $1")
    expect(call.params).toEqual(["tmdb"])
  })

  it("getKey returns null on a miss", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([])
    expect(await mod.getKey("missing")).toBeNull()
  })

  it("setKey upserts with ON CONFLICT", async () => {
    const { db, mod } = await loadDbModule()
    await mod.setKey("javbus", "cookie=abc")
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe(
      "INSERT INTO provider_keys (provider, value) VALUES ($1, $2) " +
        "ON CONFLICT(provider) DO UPDATE SET value = excluded.value"
    )
    expect(call.params).toEqual(["javbus", "cookie=abc"])
  })

  it("setKey with an empty value DELETEs the row", async () => {
    const { db, mod } = await loadDbModule()
    await mod.setKey("javbus", "")
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe("DELETE FROM provider_keys WHERE provider = $1")
    expect(call.params).toEqual(["javbus"])
  })

  it("allKeys maps rows to a record", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([
      { provider: "tmdb", value: "k1" },
      { provider: "javbus", value: "k2" },
    ])
    expect(await mod.allKeys()).toEqual({ tmdb: "k1", javbus: "k2" })
    expect(norm(db.selectCalls[db.selectCalls.length - 1].sql)).toBe(
      "SELECT provider, value FROM provider_keys"
    )
  })
})

describe("library_paths CRUD", () => {
  it("getPath selects by cat", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([{ path: "/Volumes/movies" }])
    expect(await mod.getPath("mov")).toBe("/Volumes/movies")
    const call = db.selectCalls[db.selectCalls.length - 1]
    expect(norm(call.sql)).toBe("SELECT path FROM library_paths WHERE cat = $1")
    expect(call.params).toEqual(["mov"])
  })

  it("setPath upserts with ON CONFLICT", async () => {
    const { db, mod } = await loadDbModule()
    await mod.setPath("vrc", "/Volumes/vr")
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe(
      "INSERT INTO library_paths (cat, path) VALUES ($1, $2) " +
        "ON CONFLICT(cat) DO UPDATE SET path = excluded.path"
    )
    expect(call.params).toEqual(["vrc", "/Volumes/vr"])
  })

  it("setPath with an empty path DELETEs the row", async () => {
    const { db, mod } = await loadDbModule()
    await mod.setPath("vrc", "")
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe("DELETE FROM library_paths WHERE cat = $1")
    expect(call.params).toEqual(["vrc"])
  })

  it("allPaths maps rows to a partial record", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([
      { cat: "mov", path: "/m" },
      { cat: "tv", path: "/t" },
    ])
    expect(await mod.allPaths()).toEqual({ mov: "/m", tv: "/t" })
  })
})

describe("TTL JSON cache (getCached / setCached)", () => {
  it("setCached upserts the JSON blob with a fetched_at timestamp", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    vi.setSystemTime(new Date(1_700_000_000_000)) // -> 1_700_000_000 s
    await mod.setCached("listing_cache", "discover:mov:yts:trending", { items: [1, 2] })
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe(
      "INSERT INTO listing_cache (key, json, fetched_at) VALUES ($1, $2, $3) " +
        "ON CONFLICT(key) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at"
    )
    expect(call.params).toEqual([
      "discover:mov:yts:trending",
      JSON.stringify({ items: [1, 2] }),
      1_700_000_000,
    ])
  })

  it("getCached returns the parsed value on a fresh hit", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    const now = 1_700_000_000
    vi.setSystemTime(now * 1000)
    // fetched 100s ago, TTL 300s -> fresh
    db.queueSelect([{ json: JSON.stringify({ a: 1 }), fetched_at: now - 100 }])
    const v = await mod.getCached<{ a: number }>("listing_cache", "k", 300)
    expect(v).toEqual({ a: 1 })
    const call = db.selectCalls[db.selectCalls.length - 1]
    expect(norm(call.sql)).toBe("SELECT json, fetched_at FROM listing_cache WHERE key = $1")
    expect(call.params).toEqual(["k"])
  })

  it("getCached returns null on a stale entry (older than TTL)", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    const now = 1_700_000_000
    vi.setSystemTime(now * 1000)
    // fetched 400s ago, TTL 300s -> stale
    db.queueSelect([{ json: JSON.stringify({ a: 1 }), fetched_at: now - 400 }])
    expect(await mod.getCached("meta_cache", "k", 300)).toBeNull()
  })

  it("getCached treats the TTL boundary as stale (>=)", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    const now = 1_700_000_000
    vi.setSystemTime(now * 1000)
    db.queueSelect([{ json: "{}", fetched_at: now - 300 }]) // exactly TTL old
    expect(await mod.getCached("listing_cache", "k", 300)).toBeNull()
  })

  it("getCached returns null on a miss without parsing", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([])
    expect(await mod.getCached("listing_cache", "nope", 300)).toBeNull()
  })

  it("getCached with ttl<=0 is always a miss and never queries", async () => {
    const { db, mod } = await loadDbModule()
    const before = db.selectCalls.length
    expect(await mod.getCached("listing_cache", "k", 0)).toBeNull()
    expect(db.selectCalls.length).toBe(before) // no SELECT issued
  })

  it("getCached returns null when stored JSON is corrupt", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    const now = 1_700_000_000
    vi.setSystemTime(now * 1000)
    db.queueSelect([{ json: "{not json", fetched_at: now - 1 }])
    expect(await mod.getCached("listing_cache", "k", 300)).toBeNull()
  })
})

describe("TTL cover cache (getCachedCover / setCachedCover)", () => {
  it("setCachedCover upserts url + ar + fetched_at", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    vi.setSystemTime(1_700_000_000_000)
    await mod.setCachedCover("ABCD-123", "https://x/cover.jpg", 0.72)
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe(
      "INSERT INTO cover_cache (code, url, ar, fetched_at) VALUES ($1, $2, $3, $4) " +
        "ON CONFLICT(code) DO UPDATE SET url = excluded.url, ar = excluded.ar, fetched_at = excluded.fetched_at"
    )
    expect(call.params).toEqual(["ABCD-123", "https://x/cover.jpg", 0.72, 1_700_000_000])
  })

  it("getCachedCover returns {url, ar} on a fresh hit", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    const now = 1_700_000_000
    vi.setSystemTime(now * 1000)
    db.queueSelect([{ url: "https://x.jpg", ar: 0.675, fetched_at: now - 10 }])
    expect(await mod.getCachedCover("ABCD-123", 86_400)).toEqual({
      url: "https://x.jpg",
      ar: 0.675,
    })
    const call = db.selectCalls[db.selectCalls.length - 1]
    expect(norm(call.sql)).toBe("SELECT url, ar, fetched_at FROM cover_cache WHERE code = $1")
    expect(call.params).toEqual(["ABCD-123"])
  })

  it("getCachedCover returns null on a stale entry", async () => {
    const { db, mod } = await loadDbModule()
    vi.useFakeTimers()
    const now = 1_700_000_000
    vi.setSystemTime(now * 1000)
    db.queueSelect([{ url: "https://x.jpg", ar: 0.7, fetched_at: now - 100 }])
    expect(await mod.getCachedCover("ABCD-123", 50)).toBeNull()
  })
})

describe("library_index CRUD", () => {
  const row = {
    path: "/Volumes/ad/ABCD-123.mp4",
    cat: "ad" as const,
    fname: "ABCD-123.mp4",
    size: "4.6 GB",
    code: "ABCD-123",
    title: "ABCD-123",
    vr: 0 as const,
    year: "",
    sub: "",
    cover: "",
    ar: 0.72,
  }

  it("libraryUpsert inserts all columns in order with ON CONFLICT", async () => {
    const { db, mod } = await loadDbModule()
    await mod.libraryUpsert(row)
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe(
      "INSERT INTO library_index (path, cat, fname, size, code, title, vr, year, sub, cover, ar) " +
        "VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) " +
        "ON CONFLICT(path) DO UPDATE SET cat = excluded.cat, fname = excluded.fname, " +
        "size = excluded.size, code = excluded.code, title = excluded.title, vr = excluded.vr, " +
        "year = excluded.year, sub = excluded.sub, cover = excluded.cover, ar = excluded.ar"
    )
    expect(call.params).toEqual([
      "/Volumes/ad/ABCD-123.mp4",
      "ad",
      "ABCD-123.mp4",
      "4.6 GB",
      "ABCD-123",
      "ABCD-123",
      0,
      "",
      "",
      "",
      0.72,
    ])
  })

  it("libraryQueryByCat selects by cat ordered by fname", async () => {
    const { db, mod } = await loadDbModule()
    db.queueSelect([row])
    const rows = await mod.libraryQueryByCat("ad")
    expect(rows).toEqual([row])
    const call = db.selectCalls[db.selectCalls.length - 1]
    expect(norm(call.sql)).toBe("SELECT * FROM library_index WHERE cat = $1 ORDER BY fname")
    expect(call.params).toEqual(["ad"])
  })

  it("libraryClear(cat) deletes one category", async () => {
    const { db, mod } = await loadDbModule()
    await mod.libraryClear("tv")
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe("DELETE FROM library_index WHERE cat = $1")
    expect(call.params).toEqual(["tv"])
  })

  it("libraryClear() with no arg truncates the whole index", async () => {
    const { db, mod } = await loadDbModule()
    await mod.libraryClear()
    const call = lastExecute(db)
    expect(norm(call.sql)).toBe("DELETE FROM library_index")
    expect(call.params).toEqual([])
  })
})
