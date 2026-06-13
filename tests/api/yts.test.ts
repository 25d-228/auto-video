import { describe, expect, it } from "vitest"
import {
  parseMovies,
  parseSeeders,
  resolveSort,
  ytsMagnet,
  YTS_BASES,
  type YtsListResponse,
  type YtsMovie,
} from "@/api/sources/yts"

// The fixtures are real yts.bz `list_movies.json` responses recorded live with
// curl (see the gitignored tests/fixtures/yts_list.json — a sort_by listing —
// and tests/fixtures/yts_query.json — a query_term search). They are pulled in
// as raw strings via Vite's import.meta.glob (no node built-ins, so the
// DOM-only tsconfig.app.json stays clean) and parsed with NO network access.
const rawFixtures = import.meta.glob("../fixtures/yts_*.json", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>

function loadFixture(name: "list" | "query"): YtsListResponse {
  const key = Object.keys(rawFixtures).find((k) => k.endsWith(`yts_${name}.json`))
  const raw = key ? rawFixtures[key] : undefined
  if (!raw)
    throw new Error(`missing tests/fixtures/yts_${name}.json (record it first)`)
  return JSON.parse(raw) as YtsListResponse
}

function listMovies(): YtsMovie[] {
  return loadFixture("list").data?.movies ?? []
}
function queryMovies(): YtsMovie[] {
  return loadFixture("query").data?.movies ?? []
}

describe("YTS_BASES", () => {
  it("lists the four mirrors in the sidecar's order", () => {
    expect(YTS_BASES).toEqual([
      "https://yts.bz/api/v2/",
      "https://movies-api.accel.li/api/v2/",
      "https://yts.lt/api/v2/",
      "https://yts.mx/api/v2/",
    ])
  })
})

describe("resolveSort", () => {
  it("maps the four public sort ids to YTS sort_by values", () => {
    expect(resolveSort("most_seeded")).toBe("seeds")
    expect(resolveSort("trending")).toBe("download_count")
    expect(resolveSort("newest")).toBe("date_added")
    expect(resolveSort("top_rated")).toBe("rating")
  })

  it("defaults newest -> date_added and everything else -> download_count", () => {
    expect(resolveSort("bogus")).toBe("download_count")
    expect(resolveSort("")).toBe("download_count")
  })
})

describe("ytsMagnet", () => {
  it("builds a btih magnet with the dn and the four trackers (Python-quoted)", () => {
    const m = ytsMagnet("ABC123", "Some Movie")
    expect(m.startsWith("magnet:?xt=urn:btih:ABC123&dn=Some%20Movie")).toBe(true)
    // Python quotes each tracker with default safe='/', so `/` stays raw and
    // only `:` is percent-encoded (%3A).
    expect(m).toContain("&tr=udp%3A//tracker.opentrackr.org%3A1337/announce")
    expect(m).toContain("&tr=udp%3A//open.demonii.com%3A1337/announce")
    expect(m).toContain("&tr=udp%3A//tracker.openbittorrent.com%3A6969/announce")
    expect(m).toContain("&tr=udp%3A//exodus.desync.com%3A6969/announce")
  })

  it("encodes the dn name like Python urllib.parse.quote (default safe='/')", () => {
    // ( ) ! * ' are encoded; / and ~ are left raw; space -> %20.
    const m = ytsMagnet("H", "Inception (2010) [1080p] a/b c!d*e'f~g")
    const dn = m.slice(m.indexOf("&dn=") + 4, m.indexOf("&tr="))
    expect(dn).toBe(
      "Inception%20%282010%29%20%5B1080p%5D%20a/b%20c%21d%2Ae%27f~g"
    )
  })
})

describe("parseMovies (against the recorded sort_by fixture)", () => {
  const items = parseMovies(listMovies())

  it("returns one item per fixture movie", () => {
    expect(items.length).toBe(listMovies().length)
    expect(items.length).toBeGreaterThan(0)
  })

  it("maps the first item to a DiscoverItem with YTS conventions", () => {
    const m = listMovies()[0]!
    const first = items[0]!
    expect(first.cat).toBe("mov")
    expect(first.src).toBe("YTS")
    expect(first.state).toBe("new")
    expect(first.ar).toBe(0.675)
    expect(first.id).toBe(`mov_${m.id}`)
    expect(first.code).toBe(m.imdb_code || "")
    expect(first.cover).toBe(m.large_cover_image || "")
    expect(first.year).toBe(m.year ?? "")
    expect(first.rating).toBe(m.rating || 0)
    expect(first.link).toBe((m.url || "").trim())
  })

  it("uses the max seeds across torrents and the first torrent's size", () => {
    listMovies().forEach((m, i) => {
      const tors = m.torrents ?? []
      const expectedSeeds = tors.length
        ? Math.max(...tors.map((t) => Math.trunc(Number(t.seeds) || 0)))
        : 0
      const expectedSize = (tors.length ? tors[0]!.size : "") || ""
      expect(items[i]!.seeders).toBe(expectedSeeds)
      expect(items[i]!.size).toBe(expectedSize)
    })
  })

  it("formats sub as '<year> · <h>h <mm>m' when runtime > 0, else the bare year", () => {
    listMovies().forEach((m, i) => {
      const rt = Math.trunc(Number(m.runtime) || 0)
      const yr = m.year ?? ""
      const expected = rt
        ? `${yr} · ${Math.trunc(rt / 60)}h ${String(rt % 60).padStart(2, "0")}m`
        : String(yr)
      expect(items[i]!.sub).toBe(expected)
    })
  })

  it("titles fall back from title to title_long to empty string", () => {
    items.forEach((it) => expect(typeof it.title).toBe("string"))
  })
})

describe("parseMovies edge cases (hand-crafted)", () => {
  it("zero seeds + empty size when there are no torrents", () => {
    const items = parseMovies([
      { id: 1, title: "No Torrents", year: 2020, runtime: 0, torrents: [] },
    ])
    expect(items[0]!.seeders).toBe(0)
    expect(items[0]!.size).toBe("")
    expect(items[0]!.sub).toBe("2020")
    expect(items[0]!.link).toBeUndefined()
  })

  it("formats a 113-minute runtime as '1h 53m'", () => {
    const items = parseMovies([
      { id: 2, title: "T", year: 2007, runtime: 113, torrents: [] },
    ])
    expect(items[0]!.sub).toBe("2007 · 1h 53m")
  })

  it("zero-pads sub minutes (120 min -> '2h 00m')", () => {
    const items = parseMovies([
      { id: 3, title: "T", year: 2001, runtime: 120, torrents: [] },
    ])
    expect(items[0]!.sub).toBe("2001 · 2h 00m")
  })

  it("falls back to title_long when title is missing", () => {
    const items = parseMovies([
      { id: 4, title_long: "Long Title (2020)", year: 2020, torrents: [] },
    ])
    expect(items[0]!.title).toBe("Long Title (2020)")
  })

  it("only sets link when url is present and non-blank", () => {
    expect(parseMovies([{ id: 5, url: "  " }])[0]!.link).toBeUndefined()
    expect(parseMovies([{ id: 6, url: "https://yts.bz/movies/x" }])[0]!.link).toBe(
      "https://yts.bz/movies/x"
    )
  })
})

describe("parseSeeders (against the recorded query_term fixture)", () => {
  const movies = queryMovies()
  const rels = parseSeeders(movies, "Inception")

  it("returns one release per torrent with a hash", () => {
    const expected = movies.flatMap((m) =>
      (m.torrents ?? []).filter((t) => t.hash)
    ).length
    expect(rels.length).toBe(expected)
    expect(rels.length).toBeGreaterThan(0)
  })

  it("maps each release to the YTS Release shape", () => {
    for (const r of rels) {
      expect(r.source).toBe("YTS")
      expect(typeof r.seeders).toBe("number")
      expect(typeof r.size).toBe("string")
      expect(r.magnet.startsWith("magnet:?xt=urn:btih:")).toBe(true)
    }
  })

  it("names each release '<title_long> [<quality> <type>]'", () => {
    const m = movies[0]!
    const t = (m.torrents ?? []).find((x) => x.hash)!
    const r = rels[0]!
    expect(r.name).toBe(
      `${m.title_long || "Inception"} [${t.quality || ""} ${t.type || ""}]`.trim()
    )
    expect(r.quality).toBe(t.quality || "")
  })
})

describe("parseSeeders edge cases (hand-crafted)", () => {
  const fixture: YtsMovie[] = [
    {
      title_long: "Old Film (1999)",
      year: 1999,
      torrents: [{ hash: "AAA", seeds: 5, size: "700 MB", quality: "720p", type: "bluray" }],
    },
    {
      title_long: "New Film (2020)",
      year: 2020,
      torrents: [
        { hash: "BBB", seeds: 9, size: "1.4 GB", quality: "1080p", type: "web" },
        { seeds: 3, size: "x", quality: "2160p" }, // no hash -> skipped
      ],
    },
  ]

  it("filters by year when a year is supplied", () => {
    const rels = parseSeeders(fixture, "Film", 2020)
    expect(rels).toHaveLength(1)
    expect(rels[0]!.name).toBe("New Film (2020) [1080p web]")
    expect(rels[0]!.seeders).toBe(9)
  })

  it("compares year as a string (numeric vs string year match)", () => {
    expect(parseSeeders(fixture, "Film", "1999")).toHaveLength(1)
    expect(parseSeeders(fixture, "Film", 1999)[0]!.name).toBe(
      "Old Film (1999) [720p bluray]"
    )
  })

  it("returns all torrents (with a hash) when no year filter is given", () => {
    const rels = parseSeeders(fixture, "Film")
    expect(rels).toHaveLength(2) // the hashless torrent is dropped
  })

  it("skips torrents without an info hash", () => {
    const rels = parseSeeders(
      [{ title_long: "X", year: 2020, torrents: [{ seeds: 1, size: "y" }] }],
      "X"
    )
    expect(rels).toHaveLength(0)
  })

  it("falls back to the search title when title_long is missing", () => {
    const rels = parseSeeders(
      [{ year: 2020, torrents: [{ hash: "C", quality: "1080p", type: "web" }] }],
      "Fallback"
    )
    expect(rels[0]!.name).toBe("Fallback [1080p web]")
  })
})
