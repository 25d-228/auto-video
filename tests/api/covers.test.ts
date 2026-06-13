/**
 * Tests for src/api/covers.ts — the JAV cover cascade + TV/anime cover resolution.
 *
 * No network: `@/net/http` and `@/state/db` are mocked. The HTTP mock is driven
 * by a small per-URL routing table so the REAL cascade (covers.ts + the real
 * dmm.ts / mgstage.ts source modules + codes.ts) runs end-to-end against either
 * recorded live fixtures (tests/fixtures/*, gitignored) or hand-crafted blobs.
 *
 * Live fixtures recorded with curl (all HTTP 200 from this network):
 *   r18-detail.json            r18.dev dvd_id=SSIS-001 detail json (jacket urls)
 *   tvmaze-lookup.json         api.tvmaze.com lookup by imdb (Game of Thrones)
 *   tvmaze-search.json         api.tvmaze.com singlesearch by name (Breaking Bad)
 *   anilist-naruto.json        graphql.anilist.co search "Naruto"
 *   javdatabase-ssis001.html   javdatabase.com SSIS-001 page (dmm cid + webp)
 *   dmm_cover_real.jpg         a real >6KB JPEG (passes the placeholder filter)
 *
 * The db is mocked as unavailable so resolution always runs live (no cache layer),
 * matching the browser/vitest degrade path. If a fixture is absent the case
 * self-skips so the suite stays green on a fresh checkout.
 */
import { existsSync, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import path from "node:path"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

// Mock the network + db layers BEFORE importing the modules under test.
vi.mock("@/net/http", () => ({
  httpJson: vi.fn(),
  httpText: vi.fn(),
  httpBytes: vi.fn(),
  coverObjectUrl: vi.fn(),
  HttpError: class HttpError extends Error {},
}))
vi.mock("@/state/db", () => ({
  isDbAvailable: vi.fn(() => false),
  getCachedCover: vi.fn(async () => null),
  setCachedCover: vi.fn(async () => {}),
  getKey: vi.fn(async () => null),
}))

import { coverObjectUrl, httpBytes, httpJson, httpText } from "@/net/http"
import { isDbAvailable } from "@/state/db"
import {
  anilistCover,
  javCover,
  javbusCover,
  javdbCover,
  pickAnilistImage,
  r18Cover,
  resolveCovers,
  tvmazeCover,
} from "@/api/covers"
import type { DiscoverItem } from "@/api/types"

const FIX_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../fixtures"
)

function fixPath(name: string): string {
  return path.join(FIX_DIR, name)
}
function hasFix(name: string): boolean {
  return existsSync(fixPath(name))
}
function readText(name: string): string {
  return readFileSync(fixPath(name), "utf-8")
}
function readJson<T>(name: string): T {
  return JSON.parse(readText(name)) as T
}
function readBytes(name: string): Uint8Array {
  return new Uint8Array(readFileSync(fixPath(name)))
}

const mockJson = vi.mocked(httpJson)
const mockText = vi.mocked(httpText)
const mockBytes = vi.mocked(httpBytes)
const mockProxy = vi.mocked(coverObjectUrl)
const mockDbAvail = vi.mocked(isDbAvailable)

/** A real >6KB JPEG that passes coverMeta (or a synthetic one if the fixture is absent). */
function realJpegBytes(): Uint8Array {
  if (hasFix("dmm_cover_real.jpg")) return readBytes("dmm_cover_real.jpg")
  // Hand-crafted: SOI + APP0 + a SOF0 declaring 800x590, padded past 6000 bytes.
  const head = [
    0xff, 0xd8, // SOI
    0xff, 0xc0, 0x00, 0x11, 0x08, 0x02, 0x4e, 0x03, 0x20, // SOF0 h=590 w=800
    0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01,
  ]
  const buf = new Uint8Array(7000)
  buf.set(head, 0)
  return buf
}

beforeEach(() => {
  mockDbAvail.mockReturnValue(false)
  // By default every cover proxy just echoes a blob: URL for the source.
  mockProxy.mockImplementation(async (u: string) => `blob:${u}`)
})

afterEach(() => {
  mockJson.mockReset()
  mockText.mockReset()
  mockBytes.mockReset()
  mockProxy.mockReset()
})

// ============================================================ r18Cover
describe("r18Cover", () => {
  it("picks the first non-blank jacket url and proxies it (live fixture)", async () => {
    if (!hasFix("r18-detail.json")) return
    mockJson.mockResolvedValue(readJson("r18-detail.json"))
    // probeCover fetches bytes; serve a real JPEG so coverMeta accepts it.
    mockBytes.mockResolvedValue(realJpegBytes())

    const r = await r18Cover("SSIS-001")
    // The live fixture has large=" " (blank) and a valid large2 pics.dmm url.
    expect(r.url).toContain("pics.dmm.co.jp")
    expect(r.ar).toBeGreaterThan(0)
    expect(r.proxy).toBe(true)
  })

  it("trims a whitespace-only large url and falls back to large2", async () => {
    mockJson.mockResolvedValue({
      images: { jacket_image: { large: "   ", large2: "https://pics.dmm.co.jp/x/y/ypl.jpg" } },
    })
    mockBytes.mockResolvedValue(realJpegBytes())
    const r = await r18Cover("ABC-1")
    expect(r.url).toBe("https://pics.dmm.co.jp/x/y/ypl.jpg")
  })

  it("returns no cover when both jacket urls are blank", async () => {
    mockJson.mockResolvedValue({ images: { jacket_image: { large: " ", large2: "" } } })
    const r = await r18Cover("ABC-1")
    expect(r.url).toBe("")
    expect(r.ar).toBe(0)
  })

  it("returns no cover when the fetch throws", async () => {
    mockJson.mockRejectedValue(new Error("network"))
    const r = await r18Cover("ABC-1")
    expect(r.url).toBe("")
  })

  it("rejects a placeholder image (coverMeta fails on <6KB)", async () => {
    mockJson.mockResolvedValue({
      images: { jacket_image: { large: "https://pics.dmm.co.jp/x/y/yps.jpg" } },
    })
    mockBytes.mockResolvedValue(new Uint8Array(100)) // too small -> placeholder
    const r = await r18Cover("ABC-1")
    expect(r.url).toBe("")
  })
})

// ============================================================ javbusCover
describe("javbusCover", () => {
  it("returns no cover without a cookie (gated, never fetches)", async () => {
    const r = await javbusCover("ABC-123", "")
    expect(r.url).toBe("")
    expect(mockText).not.toHaveBeenCalled()
  })

  it("parses the bigImage href, normalizes // and proxies (dmm-hosted)", async () => {
    mockText.mockResolvedValue(
      `<html><a class="bigImage" href="//pics.dmm.co.jp/digital/video/abc123/abc123pl.jpg">x</a></html>`
    )
    mockBytes.mockResolvedValue(realJpegBytes())
    const r = await javbusCover("ABC-123", "cookie=1")
    expect(r.url).toBe("https://pics.dmm.co.jp/digital/video/abc123/abc123pl.jpg")
    expect(r.proxy).toBe(true)
  })

  it("normalizes a root-relative href to a javbus url", async () => {
    mockText.mockResolvedValue(
      `<a class="bigImage" href="/pics/cover/abc.jpg">x</a> bigImage`
    )
    mockBytes.mockResolvedValue(realJpegBytes())
    const r = await javbusCover("ABC-123", "ck")
    expect(r.url).toBe("https://www.javbus.com/pics/cover/abc.jpg")
  })

  it("returns no cover when there is no bigImage anchor", async () => {
    mockText.mockResolvedValue("<html>Age Verification</html>")
    const r = await javbusCover("ABC-123", "ck")
    expect(r.url).toBe("")
  })
})

// ============================================================ javdbCover (javdatabase)
describe("javdbCover", () => {
  it("extracts the dmm cid and probes ps/pl (live fixture)", async () => {
    if (!hasFix("javdatabase-ssis001.html")) return
    mockText.mockResolvedValue(readText("javdatabase-ssis001.html"))
    mockBytes.mockResolvedValue(realJpegBytes())
    const r = await javdbCover("SSIS-001")
    // The fixture exposes pics.dmm.co.jp/digital/video/ssis00001/...
    expect(r.url).toContain("pics.dmm.co.jp/digital/video/ssis00001/")
    expect(r.proxy).toBe(true)
  })

  it("falls back to the webp mirror when no dmm cid matches", async () => {
    const html = `<html>${"x".repeat(1100)} https://www.javdatabase.com/covers/full/ab/abc.webp </html>`
    mockText.mockResolvedValue(html)
    mockBytes.mockResolvedValue(realJpegBytes())
    const r = await javdbCover("ABC-1")
    expect(r.url).toBe("https://www.javdatabase.com/covers/full/ab/abc.webp")
  })

  it("returns no cover for a too-short page", async () => {
    mockText.mockResolvedValue("tiny")
    const r = await javdbCover("ABC-1")
    expect(r.url).toBe("")
  })

  it("returns no cover when the fetch throws", async () => {
    mockText.mockRejectedValue(new Error("network"))
    const r = await javdbCover("ABC-1")
    expect(r.url).toBe("")
  })
})

// ============================================================ javCover cascade
describe("javCover cascade", () => {
  it("short-circuits FC2 codes (no studio cover)", async () => {
    const r = await javCover("FC2-PPV-1234567")
    expect(r.url).toBe("")
    expect(mockJson).not.toHaveBeenCalled()
    expect(mockText).not.toHaveBeenCalled()
    expect(mockBytes).not.toHaveBeenCalled()
  })

  it("returns no cover for an empty code", async () => {
    const r = await javCover("")
    expect(r.url).toBe("")
  })

  it("resolves via dmm first (cid grid hit) without consulting later sources", async () => {
    // dmmCover probes pics.dmm.co.jp via httpBytes; the FIRST candidate succeeds.
    mockBytes.mockResolvedValue(realJpegBytes())
    const r = await javCover("ABCD-123")
    expect(r.url).toContain("pics.dmm.co.jp")
    expect(r.proxy).toBe(true)
    // r18/javbus/javdatabase (httpJson/httpText) never reached.
    expect(mockJson).not.toHaveBeenCalled()
    expect(mockText).not.toHaveBeenCalled()
  })

  it("falls through dmm -> r18 when dmm finds nothing", async () => {
    // Every dmm cid probe must miss; only r18's marked jacket url passes. We
    // route by URL: the dmm candidates are derived from the code, r18's jacket
    // carries a distinctive marker we feed below.
    mockBytes.mockImplementation(async (u: string) => {
      // dmm candidates are pics.dmm.co.jp/<floor>/<cid>/<cid><suf>.jpg derived
      // from the code; r18's fixture jacket is a /digital/video/.../pl.jpg url too.
      // Distinguish by the r18 marker url we feed below.
      if (u.includes("R18JACKET")) return realJpegBytes()
      throw new Error("404")
    })
    mockJson.mockResolvedValue({
      images: { jacket_image: { large2: "https://pics.dmm.co.jp/R18JACKET/x/ypl.jpg" } },
    })
    const r = await javCover("ZZZZ-999")
    expect(r.url).toBe("https://pics.dmm.co.jp/R18JACKET/x/ypl.jpg")
    expect(mockJson).toHaveBeenCalled() // r18 was consulted
  })
})

// ============================================================ tvmazeCover
describe("tvmazeCover", () => {
  it("returns the medium image from an imdb lookup (live fixture)", async () => {
    if (!hasFix("tvmaze-lookup.json")) return
    mockJson.mockResolvedValue(readJson("tvmaze-lookup.json"))
    const url = await tvmazeCover("tt0944947", "")
    expect(url).toContain("static.tvmaze.com")
    expect(url).toContain("medium")
  })

  it("falls back to a name search when imdb lookup yields no image (live fixture)", async () => {
    if (!hasFix("tvmaze-search.json")) return
    mockJson
      .mockResolvedValueOnce({}) // imdb lookup: no image
      .mockResolvedValueOnce(readJson("tvmaze-search.json"))
    const url = await tvmazeCover("ttX", "Breaking Bad")
    expect(url).toContain("static.tvmaze.com")
  })

  it("returns '' when both lookups fail", async () => {
    mockJson.mockRejectedValue(new Error("network"))
    const url = await tvmazeCover("ttX", "Nope")
    expect(url).toBe("")
  })
})

// ============================================================ anilistCover
describe("anilistCover", () => {
  it("pulls the large cover from a live AniList response", async () => {
    if (!hasFix("anilist-naruto.json")) return
    mockJson.mockResolvedValue(readJson("anilist-naruto.json"))
    const url = await anilistCover("Naruto")
    expect(url).toContain("anilistcdn")
  })

  it("returns '' for an empty title without fetching", async () => {
    const url = await anilistCover("")
    expect(url).toBe("")
    expect(mockJson).not.toHaveBeenCalled()
  })

  it("returns '' when the request throws", async () => {
    mockJson.mockRejectedValue(new Error("network"))
    expect(await anilistCover("Naruto")).toBe("")
  })

  it("pickAnilistImage prefers large then medium then ''", () => {
    expect(pickAnilistImage({ data: { Media: { coverImage: { large: "L", medium: "M" } } } })).toBe("L")
    expect(pickAnilistImage({ data: { Media: { coverImage: { medium: "M" } } } })).toBe("M")
    expect(pickAnilistImage({ data: { Media: { coverImage: {} } } })).toBe("")
    expect(pickAnilistImage(null)).toBe("")
    expect(pickAnilistImage({})).toBe("")
  })
})

// ============================================================ resolveCovers
describe("resolveCovers", () => {
  function item(over: Partial<DiscoverItem>): DiscoverItem {
    return {
      id: "x",
      cat: "ad",
      title: "",
      sub: "",
      cover: "",
      ar: 0,
      seeders: 0,
      size: "",
      src: "",
      state: "new",
      year: "",
      runtime: 0,
      rating: 0,
      code: "",
      ...over,
    }
  }

  it("fills tv covers via tvmaze (by name) in place", async () => {
    if (!hasFix("tvmaze-search.json")) return
    mockJson.mockResolvedValue(readJson("tvmaze-search.json"))
    const items = [item({ id: "tv_1", cat: "tv", title: "Breaking Bad", cover: "" })]
    await resolveCovers("tv", items)
    expect(items[0]!.cover).toContain("static.tvmaze.com")
  })

  it("uses the anilist fallback for tv when tvmaze has no poster", async () => {
    if (!hasFix("anilist-naruto.json")) return
    // tvmaze (single name search) returns no image, then anilist supplies one.
    mockJson
      .mockResolvedValueOnce({}) // tvmaze name search: no image
      .mockResolvedValueOnce(readJson("anilist-naruto.json"))
    const items = [item({ id: "tv_n", cat: "tv", title: "Naruto", cover: "" })]
    await resolveCovers("tv", items)
    expect(items[0]!.cover).toContain("anilistcdn")
  })

  it("skips tv items that already carry a cover (no fetch)", async () => {
    const items = [item({ id: "tv_1", cat: "tv", title: "X", cover: "already" })]
    await resolveCovers("tv", items)
    expect(items[0]!.cover).toBe("already")
    expect(mockJson).not.toHaveBeenCalled()
  })

  it("fills ad covers via the jav cascade and sets ar", async () => {
    mockBytes.mockResolvedValue(realJpegBytes()) // dmm grid first-hit
    const items = [item({ id: "sk_1", cat: "ad", code: "ABCD-123", cover: "" })]
    await resolveCovers("ad", items)
    expect(items[0]!.cover).toContain("pics.dmm.co.jp")
    expect(items[0]!.ar).toBeGreaterThan(0)
  })

  it("derives a code from the title when code is empty (ad/vrc)", async () => {
    mockBytes.mockResolvedValue(realJpegBytes())
    const items = [item({ id: "sk_2", cat: "vrc", code: "", title: "Some SSIS-123 title", cover: "" })]
    await resolveCovers("vrc", items)
    expect(items[0]!.cover).toContain("pics.dmm.co.jp")
  })

  it("leaves cover empty but ar defaulted when no code can be found", async () => {
    const items = [item({ id: "sk_3", cat: "ad", code: "", title: "no code here", cover: "" })]
    await resolveCovers("ad", items)
    expect(items[0]!.cover).toBe("")
    expect(items[0]!.ar).toBe(0.72)
  })

  it("does nothing for mov (posters arrive on the card)", async () => {
    const items = [item({ id: "mov_1", cat: "mov", cover: "" })]
    await resolveCovers("mov", items)
    expect(items[0]!.cover).toBe("")
    expect(mockJson).not.toHaveBeenCalled()
    expect(mockBytes).not.toHaveBeenCalled()
  })
})
