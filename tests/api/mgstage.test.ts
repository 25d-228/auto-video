/// <reference types="node" />
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { dirname, resolve } from "node:path"
import { describe, expect, it } from "vitest"
import {
  mgstageIds,
  parseMgstageCovers,
  parseMgstageList,
} from "@/api/sources/mgstage"

// Parser tests run against LIVE responses captured once with curl and saved
// (gitignored) under tests/fixtures/ — no network in the test. See the task
// brief: fetch one live response, unit-test the parser against the fixture.
const HERE = dirname(fileURLToPath(import.meta.url))
const FIXTURES = resolve(HERE, "../fixtures")
function fixture(name: string): string {
  return readFileSync(resolve(FIXTURES, name), "utf-8")
}

describe("parseMgstageList (ranking page)", () => {
  const html = fixture("mgstage-ranking-week.html")
  const items = parseMgstageList(html)

  it("extracts a full ranking page of products", () => {
    // The live week-ranking fixture carries 50 distinct products.
    expect(items.length).toBe(50)
  })

  it("pairs each product id with an image.mgstage.com cover", () => {
    for (const it of items) {
      expect(it.pid).toMatch(/^[0-9A-Za-z_-]+$/)
      expect(it.cover).toMatch(
        /^https?:\/\/image\.mgstage\.com\/images\/.+\.jpg$/
      )
    }
  })

  it("dedupes by uppercased product id, first-seen order", () => {
    const codes = items.map((it) => it.pid.toUpperCase())
    expect(new Set(codes).size).toBe(codes.length)
    // First product on the live fixture (matches the Python regex run).
    expect(items[0]!.pid).toBe("300MIUM-1380")
    expect(items[0]!.cover).toBe(
      "https://image.mgstage.com/images/prestigepremium/300mium/1380/pb_e_300mium-1380.jpg"
    )
  })
})

describe("parseMgstageList (VR search page)", () => {
  // The VR popular search redirects search.php -> cSearch.php; the captured
  // fixture is the followed landing page (what plugin-http fetch receives).
  const html = fixture("mgstage-search-vr.html")
  const items = parseMgstageList(html)

  it("extracts VR products from the search landing page", () => {
    expect(items.length).toBeGreaterThan(20)
  })

  it("includes known VR labels (PRVRSS / PRDVR / ...)", () => {
    const codes = items.map((it) => it.pid.toUpperCase())
    expect(codes.some((c) => c.startsWith("PRVRSS"))).toBe(true)
  })
})

describe("parseMgstageCovers (product-detail page)", () => {
  const html = fixture("mgstage-product-siro-5683.html")

  it("prefers the pb_e big jacket and returns up to 3 candidates", () => {
    const covers = parseMgstageCovers(html)
    expect(covers.length).toBeGreaterThan(0)
    expect(covers.length).toBeLessThanOrEqual(3)
    // The SIRO-5683 page exposes the pb_e package image first.
    expect(covers[0]).toBe(
      "https://image.mgstage.com/images/shirouto/siro/5683/pb_e_siro-5683.jpg"
    )
    for (const c of covers) expect(c).toContain("pb_e")
  })
})

describe("parseMgstageCovers (priority fallback)", () => {
  it("falls back to pf_o1 when no pb_e is present", () => {
    const html = `
      <img src="https://image.mgstage.com/images/x/abc/1/pf_t1_abc-1.jpg">
      <img src="https://image.mgstage.com/images/x/abc/1/pf_o1_abc-1.jpg">
    `
    expect(parseMgstageCovers(html)).toEqual([
      "https://image.mgstage.com/images/x/abc/1/pf_o1_abc-1.jpg",
    ])
  })

  it("falls back to any image when neither pb_e nor pf_o1 is present", () => {
    const html = `<img src="https://image.mgstage.com/images/x/abc/1/pf_t1_abc-1.jpg">`
    expect(parseMgstageCovers(html)).toEqual([
      "https://image.mgstage.com/images/x/abc/1/pf_t1_abc-1.jpg",
    ])
  })

  it("returns nothing when there is no mgstage image", () => {
    expect(parseMgstageCovers("<html>no covers here</html>")).toEqual([])
  })
})

describe("mgstageIds (leading-zero padding variants)", () => {
  it("drops leading zeros and tries common paddings, deduped", () => {
    // PRVRSS-00007 -> [original, PRVRSS-007, PRVRSS-7, PRVRSS-00007]
    expect(mgstageIds("PRVRSS-00007")).toEqual([
      "PRVRSS-00007",
      "PRVRSS-007",
      "PRVRSS-7",
    ])
  })

  it("uppercases the label; the greedy label split matches the Python", () => {
    // Greedy [0-9A-Za-z]+ on a hyphenless code keeps every char but the last
    // required digit in the label (verified against sidecar mgstage_ids):
    // "siro5683" -> lab "SIRO568", num "3".
    expect(mgstageIds("siro5683")).toEqual([
      "siro5683",
      "SIRO568-003",
      "SIRO568-3",
    ])
  })

  it("collapses duplicate paddings (already 3-digit) to a stable set", () => {
    // SIRO-007: 3-digit num -> %03d == 007 == raw, %d == 7; original stays first.
    expect(mgstageIds("SIRO-007")).toEqual(["SIRO-007", "SIRO-7"])
  })

  it("returns the code unchanged when it does not split", () => {
    expect(mgstageIds("FC2-PPV")).toEqual(["FC2-PPV"])
    expect(mgstageIds("")).toEqual([""])
  })
})
