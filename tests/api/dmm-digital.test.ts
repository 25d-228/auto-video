/**
 * Pure-mapper tests for the FANZA digital VR source (src/api/sources/dmm-digital.ts).
 * No network: feed mapDigitalContents the GraphQL content nodes directly.
 */
import { describe, expect, it } from "vitest"
import { mapDigitalContents } from "@/api/sources/dmm-digital"

const node = (id: string, pl?: string) => ({
  id,
  title: `t-${id}`,
  packageImage: pl ? { largeUrl: pl } : null,
})

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
})
