import { describe, expect, it } from "vitest"
import {
  buildImdbQuery,
  parseImdbChart,
  type ImdbGqlResponse,
} from "@/api/sources/imdb"

// The fixture is a real api.graphql.imdb.com advancedTitleSearch response,
// recorded live with curl (see the gitignored tests/fixtures/imdb.json). It is
// pulled in as a raw string via Vite's import.meta.glob (no node built-ins, so
// the DOM-only tsconfig.app.json stays clean) and parsed with NO network
// access. If the fixture is absent the matched map is empty and the loader
// throws a clear message.
const rawFixtures = import.meta.glob("../fixtures/imdb.json", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>

function loadFixture(): ImdbGqlResponse {
  const raw = Object.values(rawFixtures)[0]
  if (!raw) throw new Error("missing tests/fixtures/imdb.json (record it first)")
  return JSON.parse(raw) as ImdbGqlResponse
}

describe("buildImdbQuery", () => {
  it("uses titleType movie for mov and tvSeries for tv", () => {
    expect(buildImdbQuery("mov", "popular")).toContain(
      'anyTitleTypeIds:["movie"]'
    )
    expect(buildImdbQuery("tv", "popular")).toContain(
      'anyTitleTypeIds:["tvSeries"]'
    )
  })

  it("maps popular -> POPULARITY ASC with no rating constraint", () => {
    const q = buildImdbQuery("mov", "popular")
    expect(q).toContain("sortBy:POPULARITY,sortOrder:ASC")
    expect(q).not.toContain("userRatingsConstraint")
  })

  it("maps top_rated -> USER_RATING DESC with a 25000 min-votes floor", () => {
    const q = buildImdbQuery("mov", "top_rated")
    expect(q).toContain("sortBy:USER_RATING,sortOrder:DESC")
    expect(q).toContain("userRatingsConstraint:{ratingsCountRange:{min:25000}}")
  })

  it("maps most_voted -> USER_RATING_COUNT DESC (no constraint)", () => {
    const q = buildImdbQuery("mov", "most_voted")
    expect(q).toContain("sortBy:USER_RATING_COUNT,sortOrder:DESC")
    expect(q).not.toContain("userRatingsConstraint")
  })

  it("maps newest -> RELEASE_DATE DESC (no constraint)", () => {
    const q = buildImdbQuery("mov", "newest")
    expect(q).toContain("sortBy:RELEASE_DATE,sortOrder:DESC")
    expect(q).not.toContain("userRatingsConstraint")
  })

  it("requests first:60 and the expected node fields", () => {
    const q = buildImdbQuery("mov", "popular")
    expect(q).toContain("advancedTitleSearch(first:60")
    expect(q).toContain(
      "id titleText{text} releaseYear{year} primaryImage{url} ratingsSummary{aggregateRating}"
    )
  })

  it("falls back to popular for an unknown sort", () => {
    // @ts-expect-error deliberately passing an out-of-range sort id
    const q = buildImdbQuery("mov", "bogus")
    expect(q).toContain("sortBy:POPULARITY,sortOrder:ASC")
  })
})

describe("parseImdbChart (against the recorded fixture)", () => {
  const items = parseImdbChart(loadFixture(), "mov")

  it("returns at least one item", () => {
    expect(items.length).toBeGreaterThan(0)
  })

  it("maps the first item to a DiscoverItem with IMDb conventions", () => {
    const first = items[0]!
    expect(first.cat).toBe("mov")
    expect(first.src).toBe("IMDb")
    expect(first.state).toBe("new")
    expect(first.ar).toBe(0.675)
    expect(first.seeders).toBe(0)
    expect(first.size).toBe("")
    expect(first.runtime).toBe(0)
    expect(first.date).toBe("")
    expect(first.added).toBe(0)
    // id == "imdb_" + tt id; code == the tt id; link from the tt id
    expect(first.code).toMatch(/^tt\d+$/)
    expect(first.id).toBe(`imdb_${first.code}`)
    expect(first.link).toBe(`https://www.imdb.com/title/${first.code}/`)
    // cover is the primaryImage.url, passed through verbatim
    expect(first.cover).toMatch(/^https:\/\/m\.media-amazon\.com\//)
    // year/sub are the release year as a string
    expect(first.year).toBe(first.sub)
    expect(typeof first.year).toBe("string")
  })

  it("preserves the feed order via `added`", () => {
    items.forEach((item, i) => expect(item.added).toBe(i))
  })

  it("rounds ratings to one decimal place", () => {
    for (const item of items) {
      expect(item.rating).toBe(Math.round(item.rating * 10) / 10)
      expect(item.rating).toBeGreaterThanOrEqual(0)
    }
  })

  it("threads the cat through to every item", () => {
    const tvItems = parseImdbChart(loadFixture(), "tv")
    for (const item of tvItems) expect(item.cat).toBe("tv")
  })
})

describe("parseImdbChart edge cases (hand-crafted)", () => {
  it("drops nodes with no primaryImage.url", () => {
    const json: ImdbGqlResponse = {
      data: {
        advancedTitleSearch: {
          edges: [
            {
              node: {
                title: {
                  id: "tt0000001",
                  titleText: { text: "No Cover" },
                  releaseYear: { year: 2020 },
                  primaryImage: null,
                  ratingsSummary: { aggregateRating: 9.9 },
                },
              },
            },
            {
              node: {
                title: {
                  id: "tt0000002",
                  titleText: { text: "Has Cover" },
                  releaseYear: { year: 2021 },
                  primaryImage: { url: "https://m.media-amazon.com/x.jpg" },
                  ratingsSummary: { aggregateRating: 7.25 },
                },
              },
            },
          ],
        },
      },
    }
    const items = parseImdbChart(json, "mov")
    expect(items).toHaveLength(1)
    expect(items[0]!.code).toBe("tt0000002")
    expect(items[0]!.added).toBe(0)
    expect(items[0]!.rating).toBe(7.3) // round(7.25, 1) -> 7.3
  })

  it("handles a null releaseYear (empty year/sub) and missing rating", () => {
    const json: ImdbGqlResponse = {
      data: {
        advancedTitleSearch: {
          edges: [
            {
              node: {
                title: {
                  id: "tt0000003",
                  titleText: { text: "Unknown Year" },
                  releaseYear: null,
                  primaryImage: { url: "https://m.media-amazon.com/y.jpg" },
                  ratingsSummary: null,
                },
              },
            },
          ],
        },
      },
    }
    const items = parseImdbChart(json, "mov")
    expect(items).toHaveLength(1)
    expect(items[0]!.year).toBe("")
    expect(items[0]!.sub).toBe("")
    expect(items[0]!.rating).toBe(0)
  })

  it("omits the link when the id is not a tt id", () => {
    const json: ImdbGqlResponse = {
      data: {
        advancedTitleSearch: {
          edges: [
            {
              node: {
                title: {
                  id: "xyz123",
                  titleText: { text: "Weird Id" },
                  releaseYear: { year: 2019 },
                  primaryImage: { url: "https://m.media-amazon.com/z.jpg" },
                  ratingsSummary: { aggregateRating: 6 },
                },
              },
            },
          ],
        },
      },
    }
    const items = parseImdbChart(json, "mov")
    expect(items).toHaveLength(1)
    expect(items[0]!.link).toBeUndefined()
    expect(items[0]!.code).toBe("xyz123")
  })

  it("returns [] for an empty / malformed response", () => {
    expect(parseImdbChart({}, "mov")).toEqual([])
    expect(parseImdbChart({ data: null }, "mov")).toEqual([])
    expect(
      parseImdbChart({ data: { advancedTitleSearch: { edges: null } } }, "mov")
    ).toEqual([])
  })
})
