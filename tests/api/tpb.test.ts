import { describe, expect, it } from "vitest"
import {
  humanSize,
  parseSeeders,
  parseTv,
} from "@/api/sources/tpb"

// Live fixtures recorded with curl (gitignored under tests/fixtures/):
//   tpb-newest.json  : apibay q.php?q=category:205            (newest TV feed)
//   tpb-top100.json  : apibay precompiled data_top100_205.json (trending TV feed)
//   tpb-seeders.json : apibay q.php?q=Inception               (a seeders search)
// Loaded as raw text via Vite's import.meta.glob (eager, default export) so the
// test stays free of node builtins — it just parses the saved bytes, no network.
const rawFixtures = import.meta.glob("../fixtures/tpb-*.json", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>

function loadFixture(name: string): unknown {
  const entry = rawFixtures[`../fixtures/${name}`]
  if (entry === undefined) {
    throw new Error(
      `fixture ${name} not found — record it with: curl ... -o tests/fixtures/${name}`
    )
  }
  return JSON.parse(entry)
}

const newest = loadFixture("tpb-newest.json")
const top100 = loadFixture("tpb-top100.json")
const seedersRaw = loadFixture("tpb-seeders.json")

// A "No results returned" sentinel response (apibay returns this for misses):
// id="0", all-zero info_hash, name="No results returned".
const sentinel = [
  {
    id: "0",
    name: "No results returned",
    info_hash: "0000000000000000000000000000000000000000",
    seeders: "0",
    size: "0",
    imdb: "",
  },
]

describe("humanSize", () => {
  it("formats bytes with %d and KB+ with one decimal", () => {
    expect(humanSize(0)).toBe("0 B")
    expect(humanSize(512)).toBe("512 B")
    expect(humanSize(1024)).toBe("1.0 KB")
    expect(humanSize(677429500)).toBe("646.0 MB")
    expect(humanSize(1991613584)).toBe("1.9 GB")
  })
})

describe("parseTv — newest (q.php category:205, string fields)", () => {
  const items = parseTv(newest, "newest")

  it("returns DiscoverItem[] with TV-source conventions", () => {
    expect(items.length).toBeGreaterThan(0)
    const it0 = items[0]!
    expect(it0.cat).toBe("tv")
    expect(it0.src).toBe("TPB")
    expect(it0.state).toBe("new")
    expect(it0.ar).toBe(0.7)
    expect(it0.cover).toBe("") // covers resolved by tvmaze, not here
    expect(it0.year).toBe("")
    expect(it0.runtime).toBe(0)
    expect(it0.rating).toBe(0)
    expect(it0.id).toMatch(/^tv_\d+$/)
  })

  it("keeps q.php's newest-first order (does not sort by seeders)", () => {
    const arr = newest as { id: string; seeders: string }[]
    expect(items[0]!.id).toBe(`tv_${arr[0]!.id}`)
  })

  it("derives series + SxxEyy via parseTvName for an SxxEyy title", () => {
    const idx = (newest as { name: string }[]).findIndex((x) =>
      /\bS\d{1,2}E\d{1,3}\b/i.test(x.name)
    )
    expect(idx).toBeGreaterThanOrEqual(0)
    const item = items[idx]!
    // se appears as the leading token of sub, e.g. "S02E17 · tt..." or "S02E17".
    expect(item.sub).toMatch(/^S\d{2}E\d{2}/)
    // title is the cleaned series name (no SxxEyy / quality markers).
    expect(item.title).not.toMatch(/\bS\d{2}E\d{2}\b/i)
  })

  it("sets code from the imdb id and builds a TPB description link", () => {
    const withImdb = items.find((x) => x.code.startsWith("tt"))
    expect(withImdb).toBeDefined()
    expect(withImdb!.code).toMatch(/^tt\d+$/)
    expect(withImdb!.link).toMatch(
      /^https:\/\/thepiratebay\.org\/description\.php\?id=\d+$/
    )
  })

  it("computes seeders and human-readable size", () => {
    const arr = newest as { id: string; seeders: string; size: string }[]
    const it0 = items[0]!
    expect(it0.seeders).toBe(parseInt(arr[0]!.seeders, 10))
    expect(it0.size).toBe(humanSize(parseInt(arr[0]!.size, 10)))
  })
})

describe("parseTv — trending (precompiled top100, numeric fields)", () => {
  const items = parseTv(top100, "trending")

  it("coerces numeric id/seeders/size from the precompiled file", () => {
    expect(items.length).toBeGreaterThan(0)
    expect(items[0]!.id).toMatch(/^tv_\d+$/)
    expect(typeof items[0]!.seeders).toBe("number")
    expect(items[0]!.seeders).toBeGreaterThan(0)
    expect(items[0]!.size).toMatch(/\d/)
  })

  it("sorts by seeders descending", () => {
    for (let i = 1; i < items.length; i++) {
      expect(items[i - 1]!.seeders).toBeGreaterThanOrEqual(items[i]!.seeders)
    }
  })

  it("caps the feed at 100 items", () => {
    expect(items.length).toBeLessThanOrEqual(100)
  })
})

describe("parseTv — edge cases", () => {
  it("drops the No-results sentinel row", () => {
    expect(parseTv(sentinel, "newest")).toEqual([])
  })

  it("returns [] for non-array input", () => {
    expect(parseTv(null, "newest")).toEqual([])
    expect(parseTv({ not: "an array" }, "trending")).toEqual([])
  })
})

describe("parseSeeders (apibay q.php search -> Release[])", () => {
  const releases = parseSeeders(seedersRaw)

  it("returns Release rows tagged source=TPB", () => {
    expect(releases.length).toBeGreaterThan(0)
    const r0 = releases[0]!
    expect(r0.source).toBe("TPB")
    expect(r0.name).toBeTruthy()
    expect(r0.seeders).toBeGreaterThanOrEqual(0)
    expect(r0.size).toMatch(/\d/)
  })

  it("builds a magnet with btih, an encoded dn, and the four trackers", () => {
    const arr = seedersRaw as { info_hash: string; name: string }[]
    const r0 = releases[0]!
    expect(r0.magnet).toContain(`magnet:?xt=urn:btih:${arr[0]!.info_hash}`)
    expect(r0.magnet).toContain("&dn=")
    expect(r0.magnet).toContain("tracker.opentrackr.org")
    expect(r0.magnet).toContain("open.demonii.com")
    expect(r0.magnet).toContain("tracker.openbittorrent.com")
    expect(r0.magnet).toContain("exodus.desync.com")
    // "/" is preserved but ":" is encoded (Python quote default safe="/"),
    // so "udp://" becomes "udp%3A//".
    expect(r0.magnet).toContain("udp%3A//tracker.opentrackr.org")
  })

  it("derives a quality tag when the name carries a resolution", () => {
    const hd = releases.find((r) => /1080p/i.test(r.name))
    expect(hd).toBeDefined()
    expect(hd!.quality).toBe("1080P")
  })

  it("skips the all-zero info_hash / No-results sentinel", () => {
    expect(parseSeeders(sentinel)).toEqual([])
  })

  it("returns [] for non-array input", () => {
    expect(parseSeeders(null)).toEqual([])
  })
})
