/**
 * Tests for src/api/discover.ts — the Discover aggregator (build_discover port).
 *
 * No network: every source module, the cover resolver and the db are mocked, so
 * what's exercised is the AGGREGATOR's own behavior: (cat, source, list)
 * routing per DISC_CATALOG, list/provider fallbacks, the listing cache, the
 * cover-resolve passes, the sukebei vr-split/sub-rewrite/_clean, and the
 * coverless-drop + n-cap.
 */
import { beforeEach, describe, expect, it, vi } from "vitest"

// Mock every dependency BEFORE importing the module under test. (The tmdb mock
// keeps the real tmdbPath — a pure list-id -> API-path mapper — so the routing
// assertions below also pin the _tmdb_path port.)
vi.mock("@/api/sources/dmm", () => ({ fetchDmm: vi.fn(async () => []) }))
vi.mock("@/api/sources/imdb", () => ({ fetchImdbChart: vi.fn(async () => []) }))
vi.mock("@/api/sources/javdb", () => ({ discover: vi.fn(async () => []) }))
vi.mock("@/api/sources/mgstage", () => ({ fetchMgstage: vi.fn(async () => []) }))
vi.mock("@/api/sources/sukebei", () => ({ fetchSukebei: vi.fn(async () => []) }))
vi.mock("@/api/sources/tmdb", async (importOriginal) => {
  const real = await importOriginal<typeof import("@/api/sources/tmdb")>()
  return {
    ...real,
    fetchTmdbTrending: vi.fn(async () => []),
    fetchTmdbList: vi.fn(async () => []),
  }
})
vi.mock("@/api/sources/tpb", () => ({ fetchTv: vi.fn(async () => []) }))
vi.mock("@/api/sources/yts", () => ({ fetchMovies: vi.fn(async () => []) }))
vi.mock("@/api/covers", () => ({ resolveCovers: vi.fn(async () => {}) }))
vi.mock("@/state/db", () => ({
  isDbAvailable: vi.fn(() => false),
  getCached: vi.fn(async () => null),
  setCached: vi.fn(async () => {}),
  getKey: vi.fn(async () => ""),
}))
vi.mock("@/net/http", () => ({
  httpJson: vi.fn(),
  httpText: vi.fn(),
  httpBytes: vi.fn(),
  coverObjectUrl: vi.fn(),
  HttpError: class HttpError extends Error {},
}))

import { resolveCovers } from "@/api/covers"
import { discover, resolveList } from "@/api/discover"
import { coverObjectUrl } from "@/net/http"
import { fetchDmm } from "@/api/sources/dmm"
import { fetchImdbChart } from "@/api/sources/imdb"
import { discover as javdbDiscover } from "@/api/sources/javdb"
import { fetchMgstage } from "@/api/sources/mgstage"
import { fetchSukebei, type SukebeiItem } from "@/api/sources/sukebei"
import { fetchTmdbList, fetchTmdbTrending } from "@/api/sources/tmdb"
import { fetchTv } from "@/api/sources/tpb"
import { fetchMovies } from "@/api/sources/yts"
import type { DiscoverItem } from "@/api/types"
import { getCached, getKey, isDbAvailable, setCached } from "@/state/db"

// ------------------------------------------------------------------ helpers

function di(over: Partial<DiscoverItem> = {}): DiscoverItem {
  return {
    id: "x1",
    cat: "mov",
    title: "Title",
    sub: "",
    cover: "https://img.example/p.jpg",
    ar: 0.7,
    seeders: 0,
    size: "",
    src: "S",
    state: "new",
    year: "",
    runtime: 0,
    rating: 0,
    code: "",
    ...over,
  }
}

function sk(over: Partial<SukebeiItem> = {}): SukebeiItem {
  return {
    ...di({ id: "sk_1", cat: "ad", ar: 0.72, src: "sukebei", cover: "" }),
    magnet: "magnet:?xt=urn:btih:aaa",
    vr: false,
    _rawtitle: "RAW sukebei row title that is long enough to slice",
    _downloads: 3,
    ...over,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(isDbAvailable).mockReturnValue(false)
  vi.mocked(getCached).mockResolvedValue(null)
  vi.mocked(setCached).mockResolvedValue(undefined)
  vi.mocked(getKey).mockResolvedValue("")
  vi.mocked(resolveCovers).mockImplementation(async () => {})
  vi.mocked(coverObjectUrl).mockImplementation(async (url) => "blob:" + url)
  vi.mocked(fetchDmm).mockResolvedValue([])
  vi.mocked(fetchImdbChart).mockResolvedValue([])
  vi.mocked(javdbDiscover).mockResolvedValue([])
  vi.mocked(fetchMgstage).mockResolvedValue([])
  vi.mocked(fetchSukebei).mockResolvedValue([])
  vi.mocked(fetchTmdbTrending).mockResolvedValue([])
  vi.mocked(fetchTmdbList).mockResolvedValue([])
  vi.mocked(fetchTv).mockResolvedValue([])
  vi.mocked(fetchMovies).mockResolvedValue([])
})

// ------------------------------------------------------------- resolveList

describe("resolveList (_resolve_list port)", () => {
  it("defaults to the category's first provider and its first list", () => {
    expect(resolveList("mov", "", "")).toEqual({ source: "tmdb", list: "trending" })
    expect(resolveList("tv", "", "")).toEqual({ source: "tmdb", list: "trending" })
    expect(resolveList("ad", "", "")).toEqual({ source: "javdb", list: "weekly" })
    expect(resolveList("vrc", "", "")).toEqual({ source: "dmm", list: "trending" })
  })

  it("passes a valid (source, list) through unchanged", () => {
    expect(resolveList("ad", "sukebei", "most_downloaded")).toEqual({
      source: "sukebei",
      list: "most_downloaded",
    })
    expect(resolveList("mov", "yts", "top_rated")).toEqual({
      source: "yts",
      list: "top_rated",
    })
  })

  it("maps an invalid list to the provider's default list", () => {
    expect(resolveList("mov", "imdb", "weekly")).toEqual({
      source: "imdb",
      list: "popular",
    })
    expect(resolveList("vrc", "mgstage", "daily")).toEqual({
      source: "mgstage",
      list: "popular", // VR mgstage now offers a single "popular" list
    })
  })

  it("maps an unknown provider to the category default (list re-checked)", () => {
    // "newest" is not a tmdb mov list, so it falls to tmdb's default too.
    expect(resolveList("mov", "sukebei", "newest")).toEqual({
      source: "tmdb",
      list: "trending",
    })
  })
})

// ------------------------------------------------------------------ movies

describe("discover mov routing", () => {
  it("tmdb trending -> fetchTmdbTrending('movie')", async () => {
    vi.mocked(fetchTmdbTrending).mockResolvedValue([di()])
    const out = await discover("mov", "tmdb", "trending")
    expect(fetchTmdbTrending).toHaveBeenCalledWith("movie")
    expect(fetchTmdbList).not.toHaveBeenCalled()
    expect(out).toHaveLength(1)
  })

  it("tmdb popular -> fetchTmdbList('mov', 'movie/popular') via the real tmdbPath", async () => {
    await discover("mov", "tmdb", "popular")
    expect(fetchTmdbList).toHaveBeenCalledWith("mov", "movie/popular")
  })

  it("tmdb upcoming -> movie/upcoming", async () => {
    await discover("mov", "tmdb", "upcoming")
    expect(fetchTmdbList).toHaveBeenCalledWith("mov", "movie/upcoming")
  })

  it("imdb -> fetchImdbChart('mov', sort) with the fresh flag", async () => {
    await discover("mov", "imdb", "top_rated")
    expect(fetchImdbChart).toHaveBeenCalledWith("mov", "top_rated", { fresh: false })
    await discover("mov", "imdb", "most_voted", 50, true)
    expect(fetchImdbChart).toHaveBeenCalledWith("mov", "most_voted", { fresh: true })
  })

  it("yts -> fetchMovies(list, fresh)", async () => {
    await discover("mov", "yts", "most_seeded")
    expect(fetchMovies).toHaveBeenCalledWith("most_seeded", false)
    await discover("mov", "yts", "newest", 50, true)
    expect(fetchMovies).toHaveBeenCalledWith("newest", true)
  })

  it("drops coverless items and caps at n", async () => {
    vi.mocked(fetchTmdbTrending).mockResolvedValue([
      di({ id: "a" }),
      di({ id: "b", cover: "" }),
      di({ id: "c" }),
      di({ id: "d" }),
    ])
    const out = await discover("mov", "tmdb", "trending", 2)
    expect(out.map((x) => x.id)).toEqual(["a", "c"])
  })
})

// ---------------------------------------------------------------------- tv

describe("discover tv routing", () => {
  it("tmdb trending -> fetchTmdbTrending('tv'); airing -> tv/on_the_air", async () => {
    await discover("tv", "tmdb", "trending")
    expect(fetchTmdbTrending).toHaveBeenCalledWith("tv")
    await discover("tv", "tmdb", "airing")
    expect(fetchTmdbList).toHaveBeenCalledWith("tv", "tv/on_the_air")
  })

  it("imdb -> fetchImdbChart('tv', sort)", async () => {
    await discover("tv", "imdb", "popular")
    expect(fetchImdbChart).toHaveBeenCalledWith("tv", "popular", { fresh: false })
  })

  it("tpb trending/newest -> fetchTv mode, then resolveCovers fills + filters", async () => {
    const rows = [
      di({ id: "tv_1", cat: "tv", cover: "" }),
      di({ id: "tv_2", cat: "tv", cover: "" }),
    ]
    vi.mocked(fetchTv).mockResolvedValue(rows)
    // tvmaze knows only the first show.
    vi.mocked(resolveCovers).mockImplementation(async (_cat, items) => {
      for (const x of items) if (x.id === "tv_1") x.cover = "https://tvmaze/1.jpg"
    })
    const out = await discover("tv", "tpb", "trending")
    expect(fetchTv).toHaveBeenCalledWith("trending")
    expect(resolveCovers).toHaveBeenCalledWith("tv", rows)
    expect(out.map((x) => x.id)).toEqual(["tv_1"])

    await discover("tv", "tpb", "newest")
    expect(fetchTv).toHaveBeenLastCalledWith("newest")
  })
})

// -------------------------------------------------------------- adult / VR

describe("discover ad/vrc routing", () => {
  it("ad default -> javdb weekly (Most Viewed)", async () => {
    await discover("ad", "", "")
    expect(javdbDiscover).toHaveBeenCalledWith("ad", "weekly")
  })

  it("vrc default -> dmm trending; vrc javdb -> the VR tag browser path", async () => {
    await discover("vrc", "", "")
    expect(fetchDmm).toHaveBeenCalledWith(true, "trending")
    await discover("vrc", "javdb", "newest")
    expect(javdbDiscovered()).toEqual(["vrc", "newest"])
  })

  it("dmm ad lists pass straight through", async () => {
    await discover("ad", "dmm", "top_rated")
    expect(fetchDmm).toHaveBeenCalledWith(false, "top_rated")
  })

  it("javdb: cmastd covers decode through coverObjectUrl (no resolve pass)", async () => {
    // cmastd jackets are single-byte-XOR encrypted; coverObjectUrl decodes them,
    // so the real javdb cover is used directly (proxyCovers, not resolveCovers).
    vi.mocked(javdbDiscover).mockResolvedValue([
      di({ id: "slug1", cat: "ad", code: "SSIS-001", cover: "https://tp.cmastd.com/x.jpg", ar: 1.48 }),
    ])
    const out = await discover("ad", "javdb", "daily")
    expect(resolveCovers).not.toHaveBeenCalled()
    expect(coverObjectUrl).toHaveBeenCalledWith("https://tp.cmastd.com/x.jpg")
    expect(out[0]!.cover).toBe("blob:https://tp.cmastd.com/x.jpg")
    expect(out[0]!.ar).toBe(1.48) // javdb wide-jacket aspect kept
  })

  it("javdb: a failed cover decode keeps the raw URL (kept, not dropped)", async () => {
    vi.mocked(javdbDiscover).mockResolvedValue([
      di({ id: "slug1", cat: "vrc", code: "KAVR-508", cover: "https://tp.cmastd.com/x.jpg" }),
    ])
    vi.mocked(coverObjectUrl).mockRejectedValueOnce(new Error("net down"))
    const out = await discover("vrc", "javdb", "monthly")
    expect(out[0]!.cover).toBe("https://tp.cmastd.com/x.jpg") // raw kept (CoverImage placeholder)
  })

  it("javdb: the listing cache keeps the RAW cmastd URLs; hits re-decode", async () => {
    vi.mocked(isDbAvailable).mockReturnValue(true)
    vi.mocked(javdbDiscover).mockResolvedValue([
      di({ id: "slug1", cat: "ad", code: "SSIS-001", cover: "https://tp.cmastd.com/x.jpg" }),
    ])
    // Snapshot the cover AT setCached time: caching happens inside cachedListing,
    // BEFORE proxyCovers turns the cmastd URL into a blob.
    let coverAtCacheWrite = "unset"
    vi.mocked(setCached).mockImplementation(async (_table, _key, value) => {
      coverAtCacheWrite = (value as DiscoverItem[])[0]!.cover
    })
    const live = await discover("ad", "javdb", "daily")
    expect(coverAtCacheWrite).toBe("https://tp.cmastd.com/x.jpg") // raw cmastd, cached
    expect(live[0]!.cover).toBe("blob:https://tp.cmastd.com/x.jpg") // decoded for display

    // A later cache hit re-decodes from the raw cached row.
    vi.mocked(getCached).mockResolvedValue([
      di({ id: "slug1", cat: "ad", code: "SSIS-001", cover: "https://tp.cmastd.com/x.jpg" }),
    ])
    const cached = await discover("ad", "javdb", "daily")
    expect(javdbDiscover).toHaveBeenCalledTimes(1) // second call was a cache hit
    expect(cached[0]!.cover).toBe("blob:https://tp.cmastd.com/x.jpg")
  })

  it("mgstage: keeps the listing's own wide jacket (no resolve-by-code pass)", async () => {
    vi.mocked(fetchMgstage).mockResolvedValue([
      di({ id: "mg_SIRO-5683", cat: "ad", code: "SIRO-5683", cover: "blob:wide" }),
    ])
    const out = await discover("ad", "mgstage", "daily")
    expect(fetchMgstage).toHaveBeenCalledWith(false, "daily")
    expect(resolveCovers).not.toHaveBeenCalled() // jacket kept, no portrait probe
    expect(out[0]!.cover).toBe("blob:wide") // the MGStage jacket survives
  })

  it("mgstage vrc resolves the single 'popular' VR list", async () => {
    // VR mgstage offers only "popular"; any other list id falls back to it.
    await discover("vrc", "mgstage", "trending")
    expect(fetchMgstage).toHaveBeenCalledWith(true, "popular")
  })
})

describe("discover sukebei branch", () => {
  it("ad scans 8 pages unqueried; vrc scans 4 pages of 'VR'", async () => {
    await discover("ad", "sukebei", "most_seeded")
    expect(fetchSukebei).toHaveBeenCalledWith("most_seeded", "", 8)
    await discover("vrc", "sukebei", "newest")
    expect(fetchSukebei).toHaveBeenLastCalledWith("newest", "VR", 4)
  })

  it("splits the pool by vr-ness, rewrites sub, strips internals, keeps magnet", async () => {
    const pool = [
      sk({ id: "sk_1", code: "ABC-123", vr: false }),
      sk({ id: "sk_2", code: "KAVR-001", vr: true }),
      sk({ id: "sk_3", code: "", vr: false }),
    ]
    vi.mocked(fetchSukebei).mockResolvedValue(pool)
    vi.mocked(resolveCovers).mockImplementation(async (_cat, items) => {
      for (const x of items) x.cover = "blob:" + x.id
    })

    const ad = await discover("ad", "sukebei", "most_seeded")
    expect(ad.map((x) => x.id)).toEqual(["sk_1", "sk_3"]) // vr row excluded
    expect(ad[0]!.cat).toBe("ad")
    expect(ad[0]!.sub).toBe("ABC-123")
    // no code -> first 30 chars of the raw title
    expect(ad[1]!.sub).toBe("RAW sukebei row title that is ")
    // _clean: internals stripped, magnet kept
    expect(ad[0]!).not.toHaveProperty("vr")
    expect(ad[0]!).not.toHaveProperty("_rawtitle")
    expect(ad[0]!).not.toHaveProperty("_downloads")
    expect(ad[0]!.magnet).toBe("magnet:?xt=urn:btih:aaa")

    const vr = await discover("vrc", "sukebei", "most_seeded")
    expect(vr.map((x) => x.id)).toEqual(["sk_2"])
    expect(vr[0]!.cat).toBe("vrc")
    expect(vr[0]!.sub).toBe("VR · KAVR-001")
  })

  it("drops rows whose cover never resolves, and caps at n", async () => {
    vi.mocked(fetchSukebei).mockResolvedValue([
      sk({ id: "sk_1", code: "AAA-001" }),
      sk({ id: "sk_2", code: "BBB-002" }),
      sk({ id: "sk_3", code: "CCC-003" }),
    ])
    vi.mocked(resolveCovers).mockImplementation(async (_cat, items) => {
      for (const x of items) if (x.id !== "sk_2") x.cover = "blob:" + x.id
    })
    const out = await discover("ad", "sukebei", "most_seeded", 1)
    expect(out.map((x) => x.id)).toEqual(["sk_1"])
  })

  it("passes the javbus cookie (provider key) into the cover resolve", async () => {
    vi.mocked(isDbAvailable).mockReturnValue(true)
    vi.mocked(getKey).mockResolvedValue("  jbcookie  ")
    vi.mocked(fetchSukebei).mockResolvedValue([sk({ code: "AAA-001" })])
    await discover("ad", "sukebei", "most_seeded")
    expect(resolveCovers).toHaveBeenCalledWith("ad", expect.any(Array), "jbcookie")
  })
})

// ------------------------------------------------------------------ caching

describe("listing cache (listing_cached port)", () => {
  it("serves a cache hit without calling the source", async () => {
    vi.mocked(isDbAvailable).mockReturnValue(true)
    vi.mocked(getCached).mockResolvedValue([di({ id: "cached" })])
    const out = await discover("mov", "tmdb", "trending")
    expect(getCached).toHaveBeenCalledWith("listing_cache", "mov|tmdb|trending", 300)
    expect(fetchTmdbTrending).not.toHaveBeenCalled()
    expect(out.map((x) => x.id)).toEqual(["cached"])
  })

  it("caches the raw fetch result on a miss, keyed cat|src|lst", async () => {
    vi.mocked(isDbAvailable).mockReturnValue(true)
    const rows = [di({ id: "a" }), di({ id: "b", cover: "" })]
    vi.mocked(fetchTmdbTrending).mockResolvedValue(rows)
    const out = await discover("mov", "tmdb", "trending")
    // raw (pre-filter) listing is what's cached
    expect(setCached).toHaveBeenCalledWith("listing_cache", "mov|tmdb|trending", rows)
    expect(out.map((x) => x.id)).toEqual(["a"])
  })

  it("fresh=true bypasses the read but still writes", async () => {
    vi.mocked(isDbAvailable).mockReturnValue(true)
    vi.mocked(fetchTmdbTrending).mockResolvedValue([di()])
    await discover("mov", "tmdb", "trending", 50, true)
    expect(getCached).not.toHaveBeenCalled()
    expect(setCached).toHaveBeenCalled()
  })

  it("outside Tauri the cache is a no-op", async () => {
    await discover("mov", "tmdb", "trending")
    expect(getCached).not.toHaveBeenCalled()
    expect(setCached).not.toHaveBeenCalled()
  })
})

// little helper so the assertion above reads cleanly
function javdbDiscovered(): unknown[] {
  const calls = vi.mocked(javdbDiscover).mock.calls
  return calls[calls.length - 1] ?? []
}
