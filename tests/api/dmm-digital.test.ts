/**
 * Pure-mapper tests for the FANZA digital VR source (src/api/sources/dmm-digital.ts).
 * No network: feed mapDigitalContents the GraphQL content nodes directly.
 */
import { beforeEach, describe, expect, it, vi } from "vitest"
import {
  fetchDmmDigitalAv,
  fetchDmmDigitalVr,
  mapDigitalContents,
} from "@/api/sources/dmm-digital"

const httpJson = vi.hoisted(() => vi.fn())
vi.mock("@/net/http", () => ({ httpJson }))

const node = (id: string, pl?: string) => ({
  id,
  title: `t-${id}`,
  packageImage: pl ? { largeUrl: pl } : null,
})

/** The {query, variables} of the Nth GraphQL POST the source issued. */
const sentBody = (n = 0) => JSON.parse(httpJson.mock.calls[n]![1]!.body as string)

describe("mapDigitalContents", () => {
  it("maps cid -> code, vrc cat, and the ps cover variant", () => {
    const items = mapDigitalContents([
      node("vrkm01577", "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/vrkm01577/vrkm01577pl.jpg"),
      node("sivr00490", "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/sivr00490/sivr00490pl.jpg"),
    ])
    expect(items.map((i) => i.code)).toEqual(["VRKM-1577", "SIVR-490"])
    expect(items[0]!.cat).toBe("vrc")
    expect(items[0]!.id).toBe("dmm_vrkm01577")
    // pl -> ps (smaller jacket for the grid)
    expect(items[0]!.cover).toBe(
      "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/vrkm01577/vrkm01577ps.jpg"
    )
  })

  it("strips the FANZA VR digit prefix via dmmCidToCode (13dsvr01947 -> DSVR-1947)", () => {
    expect(mapDigitalContents([node("13dsvr01947")])[0]!.code).toBe("DSVR-1947")
  })

  it("dedupes repeated cids and tolerates a missing cover", () => {
    const items = mapDigitalContents([node("vrkm01577", undefined), node("vrkm01577")])
    expect(items).toHaveLength(1)
    expect(items[0]!.cover).toBe("")
  })

  it("returns [] for empty input", () => {
    expect(mapDigitalContents([])).toEqual([])
  })

  it("tags items with the given category (ad => cat ad, no VR sub badge)", () => {
    const items = mapDigitalContents([node("sone00123")], "ad")
    expect(items[0]!.cat).toBe("ad")
    expect(items[0]!.sub).toBe("")
    expect(items[0]!.code).toBe("SONE-123")
  })
})

describe("fetchDmmDigital list -> GraphQL mapping", () => {
  beforeEach(() => httpJson.mockReset())

  it("ad 'popular' = legacySearchPPV RECOMMENDED over 2D (= website sort=suggest), cat ad", async () => {
    httpJson.mockResolvedValue({
      data: { legacySearchPPV: { result: { contents: [node("sone00123")] } } },
    })
    const items = await fetchDmmDigitalAv("popular")
    const body = sentBody()
    expect(body.query).toContain("legacySearchPPV")
    expect(body.variables.sort).toBe("RECOMMENDED") // /av/list/?media_type=2d&sort=suggest
    expect(body.variables.filter.contentType).toBe("TWO_DIMENSION")
    expect(items[0]!.cat).toBe("ad")
    expect(items[0]!.code).toBe("SONE-123")
  })

  it("vr 'popular' = legacySearchPPV RECOMMENDED over VR (= website media_type=vr&sort=suggest)", async () => {
    httpJson.mockResolvedValue({
      data: { legacySearchPPV: { result: { contents: [node("sivr00490")] } } },
    })
    const items = await fetchDmmDigitalVr("popular")
    const body = sentBody()
    expect(body.variables.sort).toBe("RECOMMENDED")
    expect(body.variables.filter.contentType).toBe("VR")
    expect(items[0]!.cat).toBe("vrc")
  })

  it("ad 'newest'/'top_rated' map to RELEASE_DATE / REVIEW_RANK_SCORE (2D)", async () => {
    httpJson.mockResolvedValue({ data: { legacySearchPPV: { result: { contents: [] } } } })
    await fetchDmmDigitalAv("newest")
    expect(sentBody().variables.sort).toBe("RELEASE_DATE")
    httpJson.mockReset()
    httpJson.mockResolvedValue({ data: { legacySearchPPV: { result: { contents: [] } } } })
    await fetchDmmDigitalAv("top_rated")
    expect(sentBody().variables.sort).toBe("REVIEW_RANK_SCORE")
    expect(sentBody().variables.filter.contentType).toBe("TWO_DIMENSION")
  })

  it("ad 'trending' = ppvContentRanking SALES_BEST_SELLERS over 2D", async () => {
    httpJson.mockResolvedValue({
      data: { ppvContentRanking: { items: [{ content: node("sone00123") }] } },
    })
    await fetchDmmDigitalAv("trending")
    const q = sentBody().query as string
    expect(q).toContain("ppvContentRanking")
    expect(q).toContain("SALES_BEST_SELLERS")
    expect(q).toContain("contentType: TWO_DIMENSION")
  })

  it("vr path is unchanged: 'newest' RELEASE_DATE over VR, 'trending' VR best-sellers", async () => {
    httpJson.mockResolvedValue({ data: { legacySearchPPV: { result: { contents: [] } } } })
    await fetchDmmDigitalVr("newest")
    expect(sentBody().variables.sort).toBe("RELEASE_DATE")
    expect(sentBody().variables.filter.contentType).toBe("VR")
    httpJson.mockReset()
    httpJson.mockResolvedValue({ data: { ppvContentRanking: { items: [] } } })
    await fetchDmmDigitalVr("trending")
    expect(sentBody().query as string).toContain("contentType: VR")
  })
})
