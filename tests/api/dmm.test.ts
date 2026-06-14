/**
 * Parser tests for src/api/sources/dmm.ts. No network: every test runs against a
 * recorded fixture under tests/fixtures/ (gitignored) or a hand-crafted blob.
 *
 * The live HTML fixture (tests/fixtures/dmm.html) is recorded with:
 *   curl -H 'Cookie: age_check_done=1; ckcy=1' -H 'Referer: https://www.dmm.co.jp/' \
 *     --compressed 'https://www.dmm.co.jp/mono/dvd/-/list/=/sort=ranking/' -o tests/fixtures/dmm.html
 * The test falls back to an inline hand-crafted snippet when that file is absent.
 */
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { dirname, join } from "node:path"
import { describe, expect, it } from "vitest"
import {
  coverMeta,
  dmmCidToCode,
  dmmCidVariants,
  dmmListUrl,
  imgDims,
  parseDmmList,
  parseDmmPreviews,
} from "@/api/sources/dmm"

const FIX = join(dirname(fileURLToPath(import.meta.url)), "..", "fixtures")

function readFix(name: string): Buffer | null {
  try {
    return readFileSync(join(FIX, name))
  } catch {
    return null
  }
}

// A hand-crafted fragment mirroring the live FANZA markup (used when the live
// dmm.html fixture is unavailable on this network).
const HAND_HTML = `
<a href="https://www.dmm.co.jp/mono/dvd/-/detail/=/cid=k9snos258/" data-dmmref="aMonoDvd_List">
  <span class="img"><img src="//pics.dmm.co.jp/mono/movie/adult/k9snos258/k9snos258ps.jpg" alt="x"></span>
</a>
<a href="https://www.dmm.co.jp/mono/dvd/-/detail/=/cid=tkipzz855/">
  <img src="//pics.dmm.co.jp/mono/movie/adult/tkipzz855/tkipzz855ps.jpg">
</a>
<a href="https://www.dmm.co.jp/mono/dvd/-/detail/=/cid=n_1428ss154/">
  <img src="//pics.dmm.co.jp/mono/movie/adult/n_1428ss154/n_1428ss154ps.jpg">
</a>
<!-- duplicate edition of SNOS-258, different cid -> deduped -->
<a href="https://www.dmm.co.jp/mono/dvd/-/detail/=/cid=k9snos258btk/">
  <img src="//pics.dmm.co.jp/mono/movie/adult/k9snos258btk/k9snos258btkps.jpg">
</a>
`

describe("dmmCidToCode", () => {
  // parity values captured from the Python sidecar dmm_cid_to_code
  const cases: [string, string][] = [
    ["k9snos258", "SNOS-258"],
    ["k9fway100", "FWAY-100"],
    ["tksnos314", "SNOS-314"],
    ["tkipzz855", "IPZZ-855"],
    ["n_1428ss154", "SS-154"],
    ["n_709maraa244tk", "MARAA-244"],
    ["ovvr616", "OVVR-616"],
    ["13dsvr01911", "DSVR-1911"],
    ["ebod123", "EBOD-123"],
    ["3dsvr0911", "DSVR-911"],
  ]
  for (const [cid, code] of cases) {
    it(`maps ${cid} -> ${code}`, () => {
      expect(dmmCidToCode(cid)).toBe(code)
    })
  }
  it("returns '' when no trailing alpha+number", () => {
    expect(dmmCidToCode("123456")).toBe("")
    expect(dmmCidToCode("")).toBe("")
  })
})

describe("dmmCidVariants", () => {
  it("expands a plain code into zero-padded cid candidates", () => {
    const v = dmmCidVariants("SNOS-258")
    expect(v).toContain("snos00258")
    expect(v).toContain("snos258")
    expect(v[0]).toBe("snos00258") // 5-pad first
    expect(new Set(v).size).toBe(v.length) // de-duplicated
  })
  it("applies the maker-prefix table first (devr -> h_1711)", () => {
    const v = dmmCidVariants("DEVR-123")
    expect(v[0]).toBe("h_1711devr00123")
    expect(v[1]).toBe("h_1711devr123")
  })
  it("applies the alias table (ebon -> ebod)", () => {
    expect(dmmCidVariants("EBON-123")).toContain("ebod00123")
  })
  it("keeps a leading digit for VR labels (3DSVR -> 13dsvr…)", () => {
    const v = dmmCidVariants("3DSVR-1911")
    expect(v).toContain("13dsvr01911")
  })
  it("returns [] for an unparseable code", () => {
    expect(dmmCidVariants("not a code")).toEqual([])
  })
})

describe("dmmListUrl", () => {
  it("maps list tokens to FANZA sort params", () => {
    expect(dmmListUrl(false, "trending")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/list/=/sort=ranking/"
    )
    expect(dmmListUrl(false, "newest")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/list/=/sort=date/"
    )
    expect(dmmListUrl(false, "top_rated")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/list/=/sort=review_rank/"
    )
  })
  it("falls back to ranking for an unknown list", () => {
    expect(dmmListUrl(false, "whatever")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/list/=/sort=ranking/"
    )
  })
  it("uses the VR keyword section when vr=true", () => {
    expect(dmmListUrl(true, "trending")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/list/=/article=keyword/id=6793/sort=ranking/"
    )
  })
  it("maps Adult best-seller ranking lists to the /ranking/ pages", () => {
    expect(dmmListUrl(false, "monthly")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/ranking/=/term=monthly/"
    )
    expect(dmmListUrl(false, "daily")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/ranking/=/term=daily/"
    )
    expect(dmmListUrl(false, "weekly")).toBe(
      "https://www.dmm.co.jp/mono/dvd/-/ranking/=/term=week/"
    )
  })
})

describe("parseDmmList", () => {
  const live = readFix("dmm.html")
  const html = live ? live.toString("utf8") : HAND_HTML

  it("extracts product cells with cid, code and absolute cover url", () => {
    const items = parseDmmList(html)
    expect(items.length).toBeGreaterThan(0)
    for (const it of items) {
      expect(it.cid).toMatch(/^[a-z0-9_]+$/)
      expect(it.code).toMatch(/^[A-Z0-9]+-\d+$/)
      expect(it.coverUrl).toMatch(/^https:\/\/pics\.dmm\.co\.jp\/.+\.jpg$/)
    }
  })

  it("dedups editions that share a derived code", () => {
    const items = parseDmmList(html)
    const codes = items.map((x) => x.code)
    expect(new Set(codes).size).toBe(codes.length)
  })

  it("returns [] for empty input", () => {
    expect(parseDmmList("")).toEqual([])
  })

  if (!live) {
    it("(hand fixture) yields the expected first three codes", () => {
      const items = parseDmmList(HAND_HTML)
      expect(items.map((x) => x.code)).toEqual(["SNOS-258", "IPZZ-855", "SS-154"])
    })
  }
})

describe("imgDims", () => {
  it("reads JPEG width/height from the SOF marker", () => {
    const real = readFix("dmm_cover_real.jpg")
    if (!real) return
    expect(imgDims(new Uint8Array(real))).toEqual([800, 590])
  })
  it("reads the 590x800 placeholder dims", () => {
    const ph = readFix("dmm_cover_placeholder.jpg")
    if (!ph) return
    expect(imgDims(new Uint8Array(ph))).toEqual([590, 800])
  })
  it("returns null for a too-short buffer", () => {
    expect(imgDims(new Uint8Array(4))).toBeNull()
  })
  it("decodes a synthetic PNG IHDR", () => {
    const png = new Uint8Array(24)
    png.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0)
    // width=320 (0x140) at byte 16..19, height=240 (0xF0) at 20..23
    png[16] = 0
    png[17] = 0
    png[18] = 0x01
    png[19] = 0x40
    png[20] = 0
    png[21] = 0
    png[22] = 0
    png[23] = 0xf0
    expect(imgDims(png)).toEqual([320, 240])
  })
})

describe("coverMeta", () => {
  it("accepts a real cover and computes the aspect ratio", () => {
    const real = readFix("dmm_cover_real.jpg")
    if (!real) return
    const meta = coverMeta(new Uint8Array(real))
    expect(meta).not.toBeNull()
    expect(meta!.ar).toBeCloseTo(800 / 590, 3)
  })
  it("rejects the 590x800 'now printing' placeholder", () => {
    const ph = readFix("dmm_cover_placeholder.jpg")
    if (!ph) return
    expect(coverMeta(new Uint8Array(ph))).toBeNull()
  })
  it("rejects a body under 6000 bytes (ps placeholder)", () => {
    const tiny = readFix("dmm_cover_tiny.jpg")
    if (tiny) {
      expect(coverMeta(new Uint8Array(tiny))).toBeNull()
    }
    expect(coverMeta(new Uint8Array(100))).toBeNull()
  })
  it("defaults ar to 0.72 when dims are unknown but the body is large", () => {
    // 7000 bytes of non-image data -> imgDims null -> default ar
    const blob = new Uint8Array(7000)
    expect(coverMeta(blob)).toEqual({ ar: 0.72 })
  })
})

describe("parseDmmPreviews", () => {
  // Real FANZA detail markup uses pics.dmm.co.jp/digital/video/<cid>/<cid>-N.jpg
  const html = `
    <img src="https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616ps.jpg">
    <a href="https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616-2.jpg">
    <a href="https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616-10.jpg">
    <a href="https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616-1.jpg">
    <a href="https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616-2.jpg">
    <a href="https://pics.dmm.co.jp/digital/video/other999/other999-1.jpg">
  `
  it("returns this cid's LARGE samples (jp- infix), deduped and ordered by N", () => {
    expect(parseDmmPreviews(html, "ovvr616")).toEqual([
      "https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616jp-1.jpg",
      "https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616jp-2.jpg",
      "https://pics.dmm.co.jp/digital/video/ovvr616/ovvr616jp-10.jpg",
    ])
  })

  it("does not match the ps cover or another product's samples", () => {
    const urls = parseDmmPreviews(html, "ovvr616")
    expect(urls.some((u) => u.includes("ps.jpg"))).toBe(false)
    expect(urls.some((u) => u.includes("other999"))).toBe(false)
  })

  it("falls back to the largest gallery when the listing cid differs (2D mono)", () => {
    // Physical mono products: listing cid k9snos258, samples under snos00258.
    const html2d = `
      <a href="https://pics.dmm.co.jp/digital/video/snos00258/snos00258-1.jpg">
      <a href="https://pics.dmm.co.jp/digital/video/snos00258/snos00258-2.jpg">
      <img src="https://pics.dmm.co.jp/mono/movie/adult/k9snos258/k9snos258ps.jpg">
    `
    expect(parseDmmPreviews(html2d, "k9snos258")).toEqual([
      "https://pics.dmm.co.jp/digital/video/snos00258/snos00258jp-1.jpg",
      "https://pics.dmm.co.jp/digital/video/snos00258/snos00258jp-2.jpg",
    ])
  })

  it("returns [] for empty html", () => {
    expect(parseDmmPreviews("", "ovvr616")).toEqual([])
  })
})
