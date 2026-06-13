import { describe, expect, it } from "vitest"
import {
  isVr,
  normalizeCodeNum,
  parseCode,
  parseTvName,
  VR_LABELS,
} from "@/lib/codes"

describe("normalizeCodeNum", () => {
  it("drops excess leading zeros to the canonical 3-digit form", () => {
    expect(normalizeCodeNum("MIVR-00081")).toBe("MIVR-081")
    expect(normalizeCodeNum("AJVR-00277")).toBe("AJVR-277")
  })
  it("leaves already-canonical codes unchanged", () => {
    expect(normalizeCodeNum("ABF-032")).toBe("ABF-032")
    expect(normalizeCodeNum("SSIS-001")).toBe("SSIS-001")
    expect(normalizeCodeNum("TEN-048")).toBe("TEN-048")
  })
  it("pads short numbers up to 3 digits", () => {
    expect(normalizeCodeNum("ABC-7")).toBe("ABC-007")
  })
  it("leaves multi-segment / non-standard ids untouched", () => {
    expect(normalizeCodeNum("FC2-PPV-1234567")).toBe("FC2-PPV-1234567")
    expect(normalizeCodeNum("")).toBe("")
  })
})

describe("parseCode", () => {
  it("normalizes FC2-PPV ids", () => {
    expect(parseCode("FC2-PPV-1234567")).toBe("FC2-PPV-1234567")
    expect(parseCode("FC2 PPV 1234567 some amateur title")).toBe(
      "FC2-PPV-1234567"
    )
    expect(parseCode("fc2ppv_1234567")).toBe("FC2-PPV-1234567")
    // 6-digit ids are valid too
    expect(parseCode("[FC2-PPV-123456]")).toBe("FC2-PPV-123456")
  })

  it("extracts a plain label-number code", () => {
    expect(parseCode("ABCD-123")).toBe("ABCD-123")
    expect(parseCode("[Studio] ABCD-123 Some Title 1080p")).toBe("ABCD-123")
    expect(parseCode("abcd123 lowercase no dash")).toBe("ABCD-123")
  })

  it("keeps the leading digit for known VR labels (3DSVR)", () => {
    expect(parseCode("3DSVR-0123")).toBe("3DSVR-0123")
    expect(parseCode("[VR] 3DSVR-0123 title here")).toBe("3DSVR-0123")
  })

  it("keeps a 3-digit amateur maker prefix (a static parser can't re-derive it)", () => {
    expect(parseCode("459TEN-048.mp4")).toBe("459TEN-048")
    expect(parseCode("300MIUM-1380")).toBe("300MIUM-1380")
    expect(parseCode("[FANZA] 200GANA-3386 title")).toBe("200GANA-3386")
    // a single stray leading digit is still dropped (DMM-cid style), not a maker
    expect(parseCode("1NHDTB-456")).toBe("NHDTB-456")
  })

  it("drops the leading digit for non-VR labels", () => {
    expect(parseCode("1ABC-123")).toBe("ABC-123")
    expect(parseCode("1NHDTB-456")).toBe("NHDTB-456")
  })

  it("preserves the number's zero padding", () => {
    expect(parseCode("SIVR-00123")).toBe("SIVR-00123")
  })

  it("returns empty string when no code is present", () => {
    expect(parseCode("")).toBe("")
    expect(parseCode("no code here")).toBe("")
  })
})

describe("isVr", () => {
  it("detects [VR] bracketed titles", () => {
    expect(isVr("[VR] Premium Title ABCD-123", "ABCD-123")).toBe(true)
    expect(isVr("(VR) Title", "")).toBe(true)
  })

  it("detects the VR 専用 marker", () => {
    expect(isVr("タイトル VR専用", "")).toBe(true)
    expect(isVr("タイトル VR 専用機器対応", "")).toBe(true)
  })

  it("detects standalone VR tokens", () => {
    expect(isVr("Some VR title", "")).toBe(true)
    expect(isVr("VR experience", "")).toBe(true)
  })

  it("detects VR by code label even without a title hint", () => {
    expect(isVr("plain title", "SIVR-123")).toBe(true)
    expect(isVr("plain title", "3DSVR-0123")).toBe(true)
    for (const lab of VR_LABELS) {
      expect(isVr("", `${lab}-001`)).toBe(true)
    }
  })

  it("does not flag non-VR titles or embedded letters", () => {
    expect(isVr("OVERDRIVE 1080p", "ABCD-123")).toBe(false)
    expect(isVr("regular title", "")).toBe(false)
    // lowercase "vr" must not match (Python regex is case-sensitive)
    expect(isVr("some vr thing", "")).toBe(false)
  })
})

describe("parseTvName", () => {
  it("extracts series and zero-padded SxxEyy", () => {
    expect(parseTvName("Show.Name.S02E04.1080p.WEB")).toEqual([
      "Show Name",
      "S02E04",
    ])
  })

  it("pads single-digit season/episode and keeps 3-digit episodes", () => {
    expect(parseTvName("Show Name S1E3 720p")).toEqual(["Show Name", "S01E03"])
    expect(parseTvName("Long Runner S05E103 HDTV")).toEqual([
      "Long Runner",
      "S05E103",
    ])
  })

  it("cuts at the year marker", () => {
    expect(parseTvName("Great Show 2023 1080p BluRay x264-GROUP")).toEqual([
      "Great Show",
      "",
    ])
  })

  it("cuts at quality/codec markers without an episode tag", () => {
    expect(parseTvName("Some Show 1080p WEB-DL HEVC")).toEqual([
      "Some Show",
      "",
    ])
    expect(parseTvName("Another.Show.Complete.720p")).toEqual([
      "Another Show",
      "",
    ])
    expect(parseTvName("Mini Series Season 2 HDTV")).toEqual([
      "Mini Series",
      "",
    ])
  })

  it("uses the earliest marker as the cut point", () => {
    // S02E04 appears before 1080p — both match, earliest wins
    expect(parseTvName("The.Show.S02E04.2023.1080p")).toEqual([
      "The Show",
      "S02E04",
    ])
  })

  it("falls back to the raw name when the cut empties the series", () => {
    expect(parseTvName("2024 collection")).toEqual(["2024 collection", ""])
  })

  it("handles names with no markers at all", () => {
    expect(parseTvName("Just A Show")).toEqual(["Just A Show", ""])
  })
})
