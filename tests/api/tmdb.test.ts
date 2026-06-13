import { describe, expect, it } from "vitest"
import {
  TMDB_IMG,
  buildItem,
  buildMeta,
  parseFeedPages,
  pickMatch,
  tmdbPath,
  type TmdbDetail,
  type TmdbListResponse,
  type TmdbResult,
} from "@/api/sources/tmdb"

// Live fixtures recorded with curl against api.themoviedb.org (gitignored under
// tests/fixtures/), plus one hand-crafted blob:
//   tmdb-trending-movie-p1.json / -p2.json : /trending/movie/week pages 1+2
//   tmdb-movie-popular-p1.json             : /movie/popular page 1
//   tmdb-search-inception.json             : /search/movie?query=Inception&year=2010
//   tmdb-detail-inception.json             : /movie/27205?append_to_response=credits
//   tmdb-dedupe-synthetic.json             : hand-crafted pages with a cross-page
//                                            duplicate id + null/empty poster rows,
//                                            to exercise the Python `seen` dedupe and
//                                            the poster-skip (live windows happen not
//                                            to repeat ids, but the code must dedupe).
// Loaded as raw text via Vite's import.meta.glob (eager, default export) so the
// test parses the saved bytes only — no network.
const rawFixtures = import.meta.glob("../fixtures/tmdb-*.json", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>

function loadFixture<T>(name: string): T {
  const entry = rawFixtures[`../fixtures/${name}`]
  if (entry === undefined) {
    throw new Error(
      `fixture ${name} not found — record it with: ` +
        `curl 'https://api.themoviedb.org/3/...&api_key=...' -o tests/fixtures/${name}`
    )
  }
  return JSON.parse(entry) as T
}

const trendingP1 = loadFixture<TmdbListResponse>("tmdb-trending-movie-p1.json")
const trendingP2 = loadFixture<TmdbListResponse>("tmdb-trending-movie-p2.json")
const popularP1 = loadFixture<TmdbListResponse>("tmdb-movie-popular-p1.json")
const searchInception = loadFixture<TmdbListResponse>("tmdb-search-inception.json")
const detailInception = loadFixture<TmdbDetail>("tmdb-detail-inception.json")
const synthetic = loadFixture<{ pages: TmdbListResponse[] }>(
  "tmdb-dedupe-synthetic.json"
)

describe("tmdbPath — list id -> kind-aware path", () => {
  it("maps movie lists", () => {
    expect(tmdbPath("mov", "popular")).toBe("movie/popular")
    expect(tmdbPath("mov", "top_rated")).toBe("movie/top_rated")
    expect(tmdbPath("mov", "now_playing")).toBe("movie/now_playing")
    expect(tmdbPath("mov", "upcoming")).toBe("movie/upcoming")
  })
  it("maps tv lists", () => {
    expect(tmdbPath("tv", "popular")).toBe("tv/popular")
    expect(tmdbPath("tv", "top_rated")).toBe("tv/top_rated")
    expect(tmdbPath("tv", "airing")).toBe("tv/on_the_air")
  })
  it("now_playing / upcoming are movie-only regardless of cat", () => {
    expect(tmdbPath("tv", "now_playing")).toBe("movie/now_playing")
    expect(tmdbPath("tv", "upcoming")).toBe("movie/upcoming")
  })
  it("returns '' for unknown / trending (handled separately)", () => {
    expect(tmdbPath("mov", "trending")).toBe("")
    expect(tmdbPath("mov", "bogus")).toBe("")
  })
})

describe("buildItem — DiscoverItem card from a TMDB result", () => {
  const first = trendingP1.results![0]!
  const item = buildItem(first, first.poster_path!, "mov", "tmdbt_", 0)

  it("builds id with the prefix and the raw id; code is the id", () => {
    expect(item.id).toBe(`tmdbt_${first.id}`)
    expect(item.code).toBe(String(first.id))
  })
  it("cover is TMDB_IMG + poster_path; ar is 0.667", () => {
    expect(item.cover).toBe(TMDB_IMG + first.poster_path)
    expect(item.ar).toBeCloseTo(0.667, 3)
  })
  it("src/state and seeders/size/runtime are the TMDB constants", () => {
    expect(item.src).toBe("TMDB")
    expect(item.state).toBe("new")
    expect(item.seeders).toBe(0)
    expect(item.size).toBe("")
    expect(item.runtime).toBe(0)
  })
  it("rating is vote_average rounded to one decimal", () => {
    expect(item.rating).toBe(Math.round((first.vote_average ?? 0) * 10) / 10)
  })
  it("date / year / sub derive from release_date", () => {
    const date = first.release_date || first.first_air_date || ""
    expect(item.date).toBe(date)
    expect(item.year).toBe(date.slice(0, 4))
    expect(item.sub).toBe(date ? date.slice(0, 4) : "")
  })
  it("link points at themoviedb.org/movie/<id>; added is the position", () => {
    expect(item.link).toBe(`https://www.themoviedb.org/movie/${first.id}`)
    expect(item.added).toBe(0)
  })
  it("tv cat builds a /tv link", () => {
    const tv = buildItem(first, first.poster_path!, "tv", "tmdbp_", 3)
    expect(tv.cat).toBe("tv")
    expect(tv.link).toBe(`https://www.themoviedb.org/tv/${first.id}`)
    expect(tv.id).toBe(`tmdbp_${first.id}`)
    expect(tv.added).toBe(3)
  })
})

describe("parseFeedPages — live trending pages", () => {
  const items = parseFeedPages([trendingP1, trendingP2], "mov", "tmdbt_")

  it("yields one card per unique poster-bearing result across both pages", () => {
    const ids = new Set<number>()
    let posterRows = 0
    for (const p of [trendingP1, trendingP2]) {
      for (const m of p.results ?? []) {
        if (m.poster_path && m.id !== undefined && !ids.has(m.id)) {
          ids.add(m.id)
          posterRows++
        }
      }
    }
    expect(items).toHaveLength(posterRows)
  })
  it("`added` is a contiguous 0..n-1 sequence", () => {
    expect(items.map((x) => x.added)).toEqual(items.map((_, i) => i))
  })
  it("every card is a TMDB movie card with a themoviedb link", () => {
    for (const x of items) {
      expect(x.src).toBe("TMDB")
      expect(x.cat).toBe("mov")
      expect(x.id.startsWith("tmdbt_")).toBe(true)
      expect(x.link).toMatch(/^https:\/\/www\.themoviedb\.org\/movie\/\d+$/)
      expect(x.cover.startsWith(TMDB_IMG)).toBe(true)
    }
  })
})

describe("parseFeedPages — dedupe + poster skip (synthetic)", () => {
  const items = parseFeedPages(synthetic.pages, "mov", "tmdbp_")

  it("drops the cross-page duplicate id and the null/empty-poster rows", () => {
    // Synthetic ids: 100 (dup across pages), 101, 102(null poster), 103(empty
    // poster), 104. Expected survivors: 100, 101, 104.
    expect(items.map((x) => x.code)).toEqual(["100", "101", "104"])
  })
  it("keeps the FIRST occurrence of a duplicate id (page-1 Alpha, not page-2)", () => {
    expect(items[0]!.title).toBe("Alpha")
  })
  it("`added` stays contiguous after the skips", () => {
    expect(items.map((x) => x.added)).toEqual([0, 1, 2])
  })
})

describe("parseFeedPages — live popular list", () => {
  it("parses the curated /movie/popular page into TMDB cards", () => {
    const items = parseFeedPages([popularP1], "mov", "tmdbp_")
    expect(items.length).toBeGreaterThan(0)
    expect(items.every((x) => x.id.startsWith("tmdbp_"))).toBe(true)
    expect(items.every((x) => x.src === "TMDB")).toBe(true)
  })
})

describe("pickMatch — title+year best match (live search)", () => {
  const results = searchInception.results ?? []

  it("picks the title+year match from the live Inception search", () => {
    const pick = pickMatch(results, "Inception", "2010")
    expect(pick).not.toBeNull()
    expect(pick!.id).toBe(27205)
    expect(pick!.title).toBe("Inception")
  })
  it("tolerates a year off by one", () => {
    const pick = pickMatch(results, "Inception", "2011")
    expect(pick?.id).toBe(27205)
  })
  it("matches with no year (bare title)", () => {
    const pick = pickMatch(results, "Inception", "")
    expect(pick?.id).toBe(27205)
  })
  it("returns null when the title does not match at all", () => {
    const pick = pickMatch(results, "Totally Unrelated Film XYZ", "")
    expect(pick).toBeNull()
  })
  it("returns null on empty results", () => {
    expect(pickMatch([], "Inception", "2010")).toBeNull()
  })
  it("falls back to the top hit when title roughly matches but year is far off", () => {
    const fake: TmdbResult[] = [
      { id: 1, title: "Inception", release_date: "1990-01-01", poster_path: "/a.jpg" },
    ]
    // year 2010 vs 1990 fails the year guard, but the title matches the top hit
    // so the fallback branch picks it.
    expect(pickMatch(fake, "Inception", "2010")?.id).toBe(1)
  })
  it("substring match works (normalized, case/punct-insensitive)", () => {
    const fake: TmdbResult[] = [
      { id: 9, original_title: "Inception: The Dream", poster_path: "/x.jpg" },
    ]
    expect(pickMatch(fake, "inception", "")?.id).toBe(9)
  })
})

describe("buildMeta — TitleMeta from pick + detail (live Inception)", () => {
  const pick = searchInception.results![0]!
  const meta = buildMeta(pick, detailInception)

  it("tmdb_id and tmdb_title come through", () => {
    expect(meta.tmdb_id).toBe(27205)
    expect(meta.tmdb_title).toBe("Inception")
  })
  it("cover is TMDB_IMG + poster_path, ar 0.667", () => {
    expect(meta.cover).toBe(TMDB_IMG + detailInception.poster_path)
    expect(meta.ar).toBeCloseTo(0.667, 3)
  })
  it("date is the 10-char date, year the leading 4", () => {
    expect(meta.date).toBe("2010-07-15")
    expect(meta.year).toBe("2010")
  })
  it("runtime is '<n> min'", () => {
    expect(meta.runtime).toBe(`${detailInception.runtime} min`)
    expect(meta.runtime).toBe("148 min")
  })
  it("genre is up to 3 names, comma-joined", () => {
    expect(meta.genre).toBe("Action, Science Fiction, Adventure")
    expect(meta.genre!.split(", ").length).toBeLessThanOrEqual(3)
  })
  it("cast is up to 5 names, comma-joined", () => {
    expect(meta.cast!.split(", ").length).toBeLessThanOrEqual(5)
    expect(meta.cast!.split(", ")[0]).toBe("Leonardo DiCaprio")
  })
  it("overview is carried through", () => {
    expect(typeof meta.overview).toBe("string")
    expect(meta.overview!.length).toBeGreaterThan(0)
  })
})

describe("buildMeta — falls back to pick fields and skips absent ones", () => {
  it("uses pick.poster_path / pick dates when detail lacks them, omits empties", () => {
    const pick: TmdbResult = {
      id: 555,
      title: "Fallback",
      poster_path: "/pick.jpg",
      release_date: "1999-12-31",
    }
    const meta = buildMeta(pick, {}) // empty detail
    expect(meta.tmdb_id).toBe(555)
    expect(meta.cover).toBe(TMDB_IMG + "/pick.jpg")
    expect(meta.date).toBe("1999-12-31")
    expect(meta.year).toBe("1999")
    // no runtime / genre / cast / tmdb_title / overview when detail is empty
    expect(meta.runtime).toBeUndefined()
    expect(meta.genre).toBeUndefined()
    expect(meta.cast).toBeUndefined()
    expect(meta.tmdb_title).toBeUndefined()
    expect(meta.overview).toBeUndefined()
  })
  it("uses tv episode_run_time[0] when there is no movie runtime", () => {
    const pick: TmdbResult = { id: 7, name: "Show", poster_path: "/s.jpg" }
    const det: TmdbDetail = {
      name: "Show",
      first_air_date: "2015-04-01",
      episode_run_time: [42],
    }
    const meta = buildMeta(pick, det)
    expect(meta.runtime).toBe("42 min")
    expect(meta.tmdb_title).toBe("Show")
    expect(meta.year).toBe("2015")
  })
})
