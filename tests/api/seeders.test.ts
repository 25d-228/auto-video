/**
 * Tests for src/api/seeders.ts — the release aggregator (build_seeders port).
 *
 * No network: the per-source release fetchers, the javdb signed API, the db and
 * the raw http layer are all mocked. What's exercised is the aggregator's own
 * behavior: source routing per cat, query building (code||title, "title year"),
 * best-effort error isolation, the javbus gid/uc ajax scrape, the javdb
 * code -> slug search, and the infohash dedup + seeders sort.
 *
 * Fixture (gitignored): tests/fixtures/javdb-search.json — LIVE response of the
 * signed app API /api/v2/search?q=SSIS-001 (recorded 2026-06-12, HTTP 200,
 * success:1). Loaded via import.meta.glob so the test needs no node builtins;
 * the javdb case self-skips if the fixture is absent on a fresh checkout.
 */
import { beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("@/api/sources/sukebei", () => ({ seedersSukebei: vi.fn(async () => []) }))
vi.mock("@/api/sources/tpb", () => ({ seedersApibay: vi.fn(async () => []) }))
vi.mock("@/api/sources/yts", () => ({ seedersYts: vi.fn(async () => []) }))
vi.mock("@/api/sources/javdb", () => ({
  javdbApi: vi.fn(async () => null),
  javdbMagnets: vi.fn(async () => []),
}))
vi.mock("@/state/db", () => ({
  isDbAvailable: vi.fn(() => false),
  getKey: vi.fn(async () => ""),
}))
vi.mock("@/net/http", () => ({
  httpText: vi.fn(async () => ""),
  httpJson: vi.fn(),
  httpBytes: vi.fn(),
  coverObjectUrl: vi.fn(),
  HttpError: class HttpError extends Error {},
}))

import { dedupeSort, seeders, seedersJavbus, seedersJavdb } from "@/api/seeders"
import { javdbApi, javdbMagnets } from "@/api/sources/javdb"
import { seedersSukebei } from "@/api/sources/sukebei"
import { seedersApibay } from "@/api/sources/tpb"
import { seedersYts } from "@/api/sources/yts"
import type { Release } from "@/api/types"
import { httpText } from "@/net/http"
import { getKey, isDbAvailable } from "@/state/db"

// ------------------------------------------------------------------ fixture

const fixtures = import.meta.glob("../fixtures/javdb-search.json", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>

interface JdbSearchFixture {
  success: number
  data: { movies: { id: string; number: string }[] }
}

function searchFixture(): JdbSearchFixture | null {
  const raw = fixtures["../fixtures/javdb-search.json"]
  if (!raw) return null
  return JSON.parse(raw) as JdbSearchFixture
}

// ------------------------------------------------------------------ helpers

function rel(over: Partial<Release> = {}): Release {
  return {
    name: "Some.Release.1080p",
    source: "TPB",
    seeders: 1,
    size: "1.0 GB",
    magnet: "magnet:?xt=urn:btih:aaaa1111",
    quality: "1080P",
    ...over,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(seedersSukebei).mockResolvedValue([])
  vi.mocked(seedersApibay).mockResolvedValue([])
  vi.mocked(seedersYts).mockResolvedValue([])
  vi.mocked(javdbApi).mockResolvedValue(null)
  vi.mocked(javdbMagnets).mockResolvedValue([])
  vi.mocked(isDbAvailable).mockReturnValue(false)
  vi.mocked(getKey).mockResolvedValue("")
  vi.mocked(httpText).mockResolvedValue("")
})

// ------------------------------------------------------------- routing: mov

describe("seeders mov routing", () => {
  it("queries apibay with 'title year' and yts with (title, year)", async () => {
    vi.mocked(seedersApibay).mockResolvedValue([rel({ name: "A", seeders: 5 })])
    vi.mocked(seedersYts).mockResolvedValue([
      rel({
        name: "B",
        source: "YTS",
        seeders: 9,
        magnet: "magnet:?xt=urn:btih:bbbb2222",
      }),
    ])
    const out = await seeders("mov", "Inception", "", 2010)
    expect(seedersApibay).toHaveBeenCalledWith("Inception 2010")
    expect(seedersYts).toHaveBeenCalledWith("Inception", 2010)
    expect(seedersSukebei).not.toHaveBeenCalled()
    // sorted by seeders desc
    expect(out.map((r) => r.name)).toEqual(["B", "A"])
  })

  it("uses the bare title when no year, and survives a yts failure", async () => {
    vi.mocked(seedersApibay).mockResolvedValue([rel()])
    vi.mocked(seedersYts).mockRejectedValue(new Error("yts down"))
    const out = await seeders("mov", "Inception", "")
    expect(seedersApibay).toHaveBeenCalledWith("Inception")
    expect(out).toHaveLength(1)
  })
})

// -------------------------------------------------------------- routing: tv

describe("seeders tv routing", () => {
  it("queries apibay only", async () => {
    vi.mocked(seedersApibay).mockResolvedValue([rel()])
    const out = await seeders("tv", "Severance", "", 2025)
    // tv never appends the year (Python: cat=='mov' only)
    expect(seedersApibay).toHaveBeenCalledWith("Severance")
    expect(seedersYts).not.toHaveBeenCalled()
    expect(out).toHaveLength(1)
  })
})

// ----------------------------------------------------------- routing: ad/vrc

describe("seeders ad/vrc routing", () => {
  it("queries sukebei + javdb + javbus with code||title", async () => {
    vi.mocked(seedersSukebei).mockResolvedValue([
      rel({ source: "sukebei", seeders: 7, magnet: "magnet:?xt=urn:btih:cccc3333" }),
    ])
    const out = await seeders("ad", "ignored title", "SSIS-001")
    expect(seedersSukebei).toHaveBeenCalledWith("SSIS-001")
    // javdb slug search attempted with the same query
    expect(javdbApi).toHaveBeenCalledWith("/api/v2/search?q=SSIS-001")
    // no javbus cookie (db unavailable) -> the javbus probe is skipped entirely
    expect(httpText).not.toHaveBeenCalled()
    expect(out).toHaveLength(1)
  })

  it("falls back to the title when there is no code (vrc too)", async () => {
    await seeders("vrc", "Some VR Thing", "")
    expect(seedersSukebei).toHaveBeenCalledWith("Some VR Thing")
  })

  it("a javdb/javbus failure never sinks the sukebei rows", async () => {
    vi.mocked(seedersSukebei).mockResolvedValue([rel({ source: "sukebei" })])
    vi.mocked(javdbApi).mockRejectedValue(new Error("api down"))
    vi.mocked(isDbAvailable).mockReturnValue(true)
    vi.mocked(getKey).mockResolvedValue("ck")
    vi.mocked(httpText).mockRejectedValue(new Error("javbus down"))
    const out = await seeders("ad", "", "ABC-123")
    expect(out).toHaveLength(1)
    expect(out[0]!.source).toBe("sukebei")
  })
})

// ---------------------------------------------------------------- javdb step

describe("seedersJavdb (code -> slug -> magnets)", () => {
  it("resolves the slug from the LIVE search fixture, exact number match", async () => {
    const fix = searchFixture()
    if (!fix) return // fixture absent on a fresh checkout — self-skip
    expect(fix.success).toBe(1)
    // javdbApi returns the envelope's `data` member.
    vi.mocked(javdbApi).mockResolvedValue(fix.data)
    vi.mocked(javdbMagnets).mockResolvedValue([rel({ source: "javdb" })])
    const out = await seedersJavdb("SSIS-001")
    // fixture rows: SSIS-001=ZY5eq, PSIS-001=RM29z, SHIS-001=a8W0W
    expect(javdbMagnets).toHaveBeenCalledWith("ZY5eq")
    expect(out).toHaveLength(1)
  })

  it("prefers the exact number over an earlier fuzzy hit", async () => {
    vi.mocked(javdbApi).mockResolvedValue({
      movies: [
        { id: "wrong", number: "SSIS-0011" },
        { id: "right", number: "ssis-001" },
      ],
    })
    await seedersJavdb("SSIS-001")
    expect(javdbMagnets).toHaveBeenCalledWith("right")
  })

  it("falls back to the first hit when nothing matches exactly", async () => {
    vi.mocked(javdbApi).mockResolvedValue({
      movies: [{ id: "first", number: "OTHER-1" }],
    })
    await seedersJavdb("SSIS-001")
    expect(javdbMagnets).toHaveBeenCalledWith("first")
  })

  it("returns [] for empty code / no hits / api error", async () => {
    expect(await seedersJavdb("")).toEqual([])
    expect(javdbApi).not.toHaveBeenCalled()
    vi.mocked(javdbApi).mockResolvedValue({ movies: [] })
    expect(await seedersJavdb("XXX-1")).toEqual([])
    vi.mocked(javdbApi).mockResolvedValue(null)
    expect(await seedersJavdb("XXX-1")).toEqual([])
    expect(javdbMagnets).not.toHaveBeenCalled()
  })
})

// --------------------------------------------------------------- javbus step

/** Hand-crafted product page + ajax payload shaped like the live javbus markup
 * (the gid/uc inline script and the uncledatools magnet table, where every
 * magnet href is repeated across the title/size/date cells of its row). */
const JB_PAGE = `<script>var gid = 46252; var uc = 0; var img = '/pics/cover/9byq_b.jpg';</script>`
const JB_AJAX = `
<tr>
  <td><a href="magnet:?xt=urn:btih:AAAA0000BBBB1111&amp;dn=ABC-123-C">ABC-123-C 1080p</a></td>
  <td><a href="magnet:?xt=urn:btih:AAAA0000BBBB1111&amp;dn=ABC-123-C">5.94GB</a></td>
  <td><a href="magnet:?xt=urn:btih:AAAA0000BBBB1111&amp;dn=ABC-123-C">2026-01-02</a></td>
</tr>
<tr>
  <td><a href="magnet:?xt=urn:btih:CCCC2222DDDD3333&amp;dn=ABC-123">ABC-123 720p</a></td>
  <td><a href="magnet:?xt=urn:btih:CCCC2222DDDD3333&amp;dn=ABC-123">980.5MB</a></td>
</tr>`

describe("seedersJavbus (gid/uc ajax scrape)", () => {
  it("returns [] without a cookie and never touches the network", async () => {
    expect(await seedersJavbus("ABC-123", "")).toEqual([])
    expect(await seedersJavbus("ABC-123", "   ")).toEqual([])
    expect(httpText).not.toHaveBeenCalled()
  })

  it("returns [] when the product page exposes no gid", async () => {
    vi.mocked(httpText).mockResolvedValue("<html>Age Verification</html>")
    expect(await seedersJavbus("ABC-123", "ck")).toEqual([])
    expect(httpText).toHaveBeenCalledTimes(1)
  })

  it("scrapes + dedups the ajax magnets (one row per infohash)", async () => {
    vi.mocked(httpText).mockImplementation(async (url: string) => {
      if (url.includes("uncledatoolsbyajax")) return JB_AJAX
      return JB_PAGE
    })
    const out = await seedersJavbus("ABC-123", "ck")

    // ajax URL carries the scraped gid + uc
    const ajaxUrl = vi.mocked(httpText).mock.calls[1]![0]
    expect(ajaxUrl).toBe(
      "https://www.javbus.com/ajax/uncledatoolsbyajax.php?gid=46252&lang=en&img=&uc=0&floor="
    )

    expect(out).toHaveLength(2) // 5 hrefs -> 2 unique hashes
    expect(out[0]).toEqual({
      name: "ABC-123",
      source: "JavBus",
      seeders: 0,
      size: "5.94GB",
      magnet: "magnet:?xt=urn:btih:AAAA0000BBBB1111&dn=ABC-123-C", // &amp; unescaped
      quality: "1080P",
    })
    expect(out[1]!.size).toBe("980.5MB")
    expect(out[1]!.quality).toBe("720P")
  })
})

// --------------------------------------------------------------- dedupeSort

describe("dedupeSort (build_seeders tail)", () => {
  it("dedups by infohash case-insensitively, first occurrence wins", async () => {
    const out = dedupeSort([
      rel({ name: "first", seeders: 1, magnet: "magnet:?xt=urn:btih:ABCD1234" }),
      rel({ name: "dupe", seeders: 99, magnet: "magnet:?xt=urn:btih:abcd1234" }),
      rel({ name: "other", seeders: 5, magnet: "magnet:?xt=urn:btih:ffff0000" }),
    ])
    expect(out.map((r) => r.name)).toEqual(["other", "first"])
  })

  it("falls back to the name as the key for hashless rows", async () => {
    const out = dedupeSort([
      rel({ name: "same", seeders: 2, magnet: "" }),
      rel({ name: "same", seeders: 8, magnet: "" }),
      rel({ name: "diff", seeders: 4, magnet: "" }),
    ])
    expect(out.map((r) => r.seeders)).toEqual([4, 2])
  })

  it("sorts by seeders desc and keeps insertion order on ties (stable)", async () => {
    const out = dedupeSort([
      rel({ name: "a", seeders: 3, magnet: "magnet:?xt=urn:btih:aa11" }),
      rel({ name: "b", seeders: 7, magnet: "magnet:?xt=urn:btih:bb22" }),
      rel({ name: "c", seeders: 3, magnet: "magnet:?xt=urn:btih:cc33" }),
    ])
    expect(out.map((r) => r.name)).toEqual(["b", "a", "c"])
  })
})

// -------------------------------------------------------------- aggregation

describe("seeders aggregation", () => {
  it("merges all ad sources, dedups across them, sorts by seeders", async () => {
    vi.mocked(seedersSukebei).mockResolvedValue([
      rel({ name: "sk", source: "sukebei", seeders: 12, magnet: "magnet:?xt=urn:btih:1111aaaa" }),
      rel({ name: "sk-dupe", source: "sukebei", seeders: 2, magnet: "magnet:?xt=urn:btih:2222BBBB" }),
    ])
    vi.mocked(javdbApi).mockResolvedValue({ movies: [{ id: "slug1", number: "ABC-123" }] })
    vi.mocked(javdbMagnets).mockResolvedValue([
      // same infohash as the second sukebei row (case differs) -> dropped
      rel({ name: "jd-dupe", source: "javdb", seeders: 0, magnet: "magnet:?xt=urn:btih:2222bbbb" }),
      rel({ name: "jd", source: "javdb", seeders: 0, magnet: "magnet:?xt=urn:btih:3333cccc" }),
    ])
    const out = await seeders("ad", "", "ABC-123")
    expect(out.map((r) => r.name)).toEqual(["sk", "sk-dupe", "jd"])
  })
})
