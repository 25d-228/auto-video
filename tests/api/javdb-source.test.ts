/**
 * Parser tests for src/api/sources/javdb.ts.
 *
 * No network: we mock `@/net/http`'s httpJson to return saved live fixtures
 * (recorded with curl into tests/fixtures/javdb-*.json — gitignored), so the
 * PARSER is exercised against real javdb response shapes. The signature module
 * is real (it is pure), but httpJson never reaches it because it is mocked.
 *
 * Fixtures are gitignored; if they are absent (fresh checkout / CI) the
 * fixture-backed cases self-skip and the pure-function cases still run.
 */
import { existsSync, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import path from "node:path"
import { afterEach, describe, expect, it, vi } from "vitest"

// Mock the HTTP layer BEFORE importing the source under test.
vi.mock("@/net/http", () => ({
  httpJson: vi.fn(),
}))

import { httpJson } from "@/net/http"
import {
  discover,
  humanSize,
  javdbLatest,
  javdbLink,
  javdbMagnets,
  javdbPlayback,
  javdbRankings,
  javdbTags,
  javdbTagsTaxonomy,
  toDiscoverItem,
  VR_TAG_ID,
  type JavdbMovie,
} from "@/api/sources/javdb"

const FIX_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../fixtures"
)

/** Load a fixture JSON or return undefined when the (gitignored) file is absent. */
function loadFixture<T>(name: string): T | undefined {
  const p = path.join(FIX_DIR, name)
  if (!existsSync(p)) return undefined
  return JSON.parse(readFileSync(p, "utf-8")) as T
}

const mockedHttpJson = vi.mocked(httpJson)

afterEach(() => {
  mockedHttpJson.mockReset()
})

/**
 * Serve a full javdb envelope from the mocked httpJson. Pass a recorded fixture
 * (already an envelope) verbatim, or a bare `data` payload which is wrapped in a
 * `{success:1, data}` envelope so javdbApi() unwraps it correctly.
 */
function serve(fixtureOrData: unknown): void {
  const isEnvelope =
    !!fixtureOrData &&
    typeof fixtureOrData === "object" &&
    "success" in (fixtureOrData as Record<string, unknown>)
  const envelope = isEnvelope
    ? fixtureOrData
    : { success: 1, action: null, message: null, data: fixtureOrData }
  mockedHttpJson.mockResolvedValue(envelope)
}

// --------------------------------------------------------------- pure helpers

describe("javdb pure helpers", () => {
  it("humanSize matches the Python human_size()", () => {
    expect(humanSize(0)).toBe("0 B")
    expect(humanSize(7741 * 1048576)).toBe("7.6 GB")
    expect(humanSize(1262 * 1048576)).toBe("1.2 GB")
    expect(humanSize(512)).toBe("512 B")
    expect(humanSize(1024)).toBe("1.0 KB")
  })

  it("javdbLink builds the /v/<slug> permalink", () => {
    expect(javdbLink("qAqKK3")).toBe("https://javdb.com/v/qAqKK3")
    expect(javdbLink("")).toBe("")
  })

  it("toDiscoverItem maps the documented fields", () => {
    const m: JavdbMovie = {
      id: "z4VAz7",
      number: "SNOS-234",
      title: "some title",
      cover_url: "https://tp.cmastd.com/x/covers/z4/z4VAz7.jpg",
      duration: 120,
      magnets_count: 4,
      release_date: "2026-06-09",
    }
    const item = toDiscoverItem(m, "ad", 2)
    expect(item).toMatchObject({
      id: "z4VAz7",
      cat: "ad",
      title: "SNOS-234",
      code: "SNOS-234",
      sub: "2026-06-09",
      date: "2026-06-09",
      year: "2026",
      cover: "https://tp.cmastd.com/x/covers/z4/z4VAz7.jpg",
      ar: 1.48,
      seeders: 4, // magnets_count -> seeders
      size: "",
      src: "javdb",
      state: "new",
      runtime: 120,
      rating: 0,
      added: 2,
      link: "https://javdb.com/v/z4VAz7",
    })
  })

  it("toDiscoverItem promotes a VR-looking title to vrc", () => {
    const m: JavdbMovie = {
      id: "1AxDAv",
      number: "KAVR-508",
      title: "【VR】...",
      cover_url: "https://tp.cmastd.com/x/covers/1a/1AxDAv.jpg",
      release_date: "2026-06-09",
    }
    expect(toDiscoverItem(m, "ad", 0).cat).toBe("vrc")
  })

  it("toDiscoverItem keeps a pinned vrc cat", () => {
    const m: JavdbMovie = { id: "x", number: "ABC-1", cover_url: "c", release_date: "2026" }
    expect(toDiscoverItem(m, "vrc", 0).cat).toBe("vrc")
  })
})

// --------------------------------------------------------------- feeds (fixtures)

describe("javdbPlayback (Most Viewed) parser", () => {
  const fixture = loadFixture("javdb-playback.json")
  it.skipIf(!fixture)("maps the playback movies array to DiscoverItem[]", async () => {
    serve(fixture)
    const items = await javdbPlayback("all", "daily")
    expect(items.length).toBeGreaterThan(0)
    expect(mockedHttpJson).toHaveBeenCalledTimes(1)
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/rankings/playback?filter_by=all&period=daily")
    for (const it of items) {
      expect(it.src).toBe("javdb")
      expect(it.cat).toMatch(/^(ad|vrc)$/)
      expect(it.cover).toContain("tp.cmastd.com")
      expect(it.ar).toBe(1.48)
      expect(it.link).toBe(`https://javdb.com/v/${it.id}`)
      expect(it.code).toBe(it.title)
    }
    // added is the (cover-filtered) feed index, sequential from 0.
    expect(items[0].added).toBe(0)
  })
})

describe("javdbRankings parser", () => {
  const fixture = loadFixture("javdb-rankings.json")
  it.skipIf(!fixture)("maps rankings movies and sends type/period", async () => {
    serve(fixture)
    const items = await javdbRankings(0, "daily")
    expect(items.length).toBeGreaterThan(0)
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/rankings?type=0&period=daily")
    expect(items.every((i) => i.src === "javdb")).toBe(true)
  })
})

describe("javdbTags (VR tag 212) parser", () => {
  const fixture = loadFixture("javdb-tags-vr.json")
  it.skipIf(!fixture)("builds the 212 filter_by and tags items as vrc", async () => {
    serve(fixture)
    const items = await javdbTags({ tagId: VR_TAG_ID })
    expect(items.length).toBeGreaterThan(0)
    const url = mockedHttpJson.mock.calls[0][0] as string
    // filter_by=0:t:m:212::: url-encoded
    expect(decodeURIComponent(url)).toContain("filter_by=0:t:m:212:::")
    expect(url).toContain("sort_by=release")
    expect(url).toContain("order_by=desc")
    expect(items.every((i) => i.cat === "vrc")).toBe(true)
    expect(items[0].code).toMatch(/VR/i)
  })

  it("builds an actor filter_by when actorSlug is given", async () => {
    serve({ movies: [] })
    await javdbTags({ actorSlug: "abc123" })
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(decodeURIComponent(url)).toContain("filter_by=0:a:abc123")
  })
})

describe("javdbLatest parser", () => {
  const fixture = loadFixture("javdb-latest.json")
  it.skipIf(!fixture)("maps latest movies and uses the captured defaults", async () => {
    serve(fixture)
    const items = await javdbLatest()
    expect(items.length).toBeGreaterThan(0)
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/movies/latest?type=all&filter_by=can_play")
    expect(url).toContain("sort_by=update")
  })
})

describe("javdbMagnets parser (replaces seeders_javdb)", () => {
  const fixture = loadFixture("javdb-magnets.json")
  it.skipIf(!fixture)("maps magnets to Release[] with MB-based sizes", async () => {
    serve(fixture)
    const rels = await javdbMagnets("qAqKK3")
    expect(rels.length).toBeGreaterThan(0)
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/movies/qAqKK3/magnets")
    const first = rels[0]
    expect(first.source).toBe("javdb")
    expect(first.seeders).toBe(0) // javdb has no seeder counts
    expect(first.magnet).toMatch(/^magnet:\?xt=urn:btih:[0-9a-f]+&dn=/)
    expect(first.magnet).toContain("tracker.opentrackr.org")
    // size 7741 MB -> "7.6 GB"; first row is HD-flagged.
    expect(first.size).toBe("7.6 GB")
    expect(first.quality).toBe("HD")
  })

  it("returns [] for an empty slug without calling the network", async () => {
    const rels = await javdbMagnets("")
    expect(rels).toEqual([])
    expect(mockedHttpJson).not.toHaveBeenCalled()
  })

  it("drops magnets with no infohash", async () => {
    serve({ magnets: [{ name: "no hash", size: 100 }, { name: "ok", hash: "abc", size: 100 }] })
    const rels = await javdbMagnets("slug")
    expect(rels).toHaveLength(1)
    expect(rels[0].name).toBe("ok")
  })
})

describe("javdbTagsTaxonomy parser", () => {
  const fixture = loadFixture("javdb-taxonomy.json")
  it.skipIf(!fixture)("returns the grouped tags including VR=212 under category", async () => {
    serve(fixture)
    const groups = await javdbTagsTaxonomy()
    expect(groups.length).toBeGreaterThan(0)
    const category = groups.find((g) => g.category_id === "category")
    expect(category).toBeDefined()
    const vr = category?.tags.find((t) => t.id === VR_TAG_ID)
    expect(vr?.name).toBe("VR")
  })
})

// --------------------------------------------------------------- discover helper

describe("discover() catalog mapping", () => {
  it("vrc -> VR tag browser (filter_by=0:t:m:212:::)", async () => {
    serve({ movies: [] })
    await discover("vrc", "weekly")
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/movies/tags")
    expect(decodeURIComponent(url)).toContain("filter_by=0:t:m:212:::")
  })

  it("vrc + year/month/sort -> filter_by=0:t:m:212:<year>::<month> and sort_by", async () => {
    serve({ movies: [] })
    await discover("vrc", "newest", {
      year: "2024",
      month: "6",
      sortBy: "score",
      orderBy: "desc",
    })
    const url = decodeURIComponent(mockedHttpJson.mock.calls[0][0] as string)
    expect(url).toContain("filter_by=0:t:m:212:2024::6")
    expect(url).toContain("sort_by=score")
  })

  it("ad weekly -> Censored ranking (type=0) for that window", async () => {
    serve({ movies: [] })
    await discover("ad", "weekly")
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/rankings?type=0&period=weekly")
  })

  it("ad unknown window -> Censored ranking, daily", async () => {
    serve({ movies: [] })
    await discover("ad", "most_viewed")
    const url = mockedHttpJson.mock.calls[0][0] as string
    expect(url).toContain("/api/v1/rankings?type=0&period=daily")
  })
})

// --------------------------------------------------------------- error envelope

describe("error-envelope handling", () => {
  it("returns [] when the API replies with a JWTVerificationError envelope", async () => {
    // This is exactly the /movies/top response captured live.
    mockedHttpJson.mockResolvedValue({
      success: 0,
      action: "JWTVerificationError",
      message: "Invalid Signature",
      data: null,
    })
    const items = await javdbPlayback("all", "daily")
    expect(items).toEqual([])
  })

  it("returns [] when httpJson throws (network failure)", async () => {
    mockedHttpJson.mockRejectedValue(new Error("network down"))
    const items = await javdbRankings(0, "daily")
    expect(items).toEqual([])
  })
})
