import { describe, expect, it } from "vitest"
import {
  parseJavdb,
  parseR18,
  type R18Combined,
} from "@/api/meta"
import type { JavMeta } from "@/api/types"

// Live fixtures recorded with curl (gitignored under tests/fixtures/):
//   r18-combined.json          : r18.dev .../combined=ssis00001/json (full browser
//                                 headers + --compressed; raw curl gets a CF 403, the
//                                 app runtime / a browser UA does not — this is the
//                                 genuine JSON body).
//   javdatabase-ssis001.html   : https://www.javdatabase.com/movies/ssis-001/ (HTTP 200)
// Loaded as raw text via Vite's import.meta.glob so the test parses the saved
// bytes only — no network, no node builtins (tsconfig.app.json is DOM-only).
const rawFixtures = import.meta.glob("../fixtures/{r18-combined.json,javdatabase-ssis001.html}", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>

function fixture(name: string): string {
  const entry = rawFixtures[`../fixtures/${name}`]
  if (entry === undefined) {
    throw new Error(
      `fixture ${name} not found — re-record it under tests/fixtures/${name}`
    )
  }
  return entry
}

// ------------------------------------------------------------------ parseR18

describe("parseR18 — r18.dev combined detail -> JavMeta", () => {
  const j = JSON.parse(fixture("r18-combined.json")) as R18Combined

  it("maps the live SSIS-001 record (jatitle/date/runtime/cast_ja)", () => {
    const rec = parseR18(j)
    expect(rec).not.toBeNull()
    // jatitle = title_ja verbatim
    expect(rec?.jatitle).toBe(
      "一ヶ月間の禁欲の果てに彼女のルームメイト2人と浮気SEXだけに没頭した彼女不在の3日間。 葵つかさ 乙白さやか"
    )
    // date = release_date[:10]
    expect(rec?.date).toBe("2021-02-19")
    // runtime = "<runtime_mins> min"
    expect(rec?.runtime).toBe("147 min")
    // cast_ja = actresses joined, name_kanji preferred
    expect(rec?.cast_ja).toBe("葵つかさ, 乙白さやか")
  })

  it("prefers name_kanji, then name_kana, then name_romaji", () => {
    const rec = parseR18({
      actresses: [
        { name_kanji: "漢字", name_kana: "かな", name_romaji: "Romaji" },
        { name_kana: "かなのみ", name_romaji: "KanaOnly" },
        { name_romaji: "RomajiOnly" },
        { name_kanji: "" }, // empty -> dropped
      ],
    })
    expect(rec?.cast_ja).toBe("漢字, かなのみ, RomajiOnly")
  })

  it("trims release_date to 10 chars and coerces runtime to string", () => {
    const rec = parseR18({
      release_date: "2021-02-19 10:00:00",
      runtime_mins: 120,
    })
    expect(rec?.date).toBe("2021-02-19")
    expect(rec?.runtime).toBe("120 min")
  })

  it("omits fields that are absent (only present fields set)", () => {
    const rec = parseR18({ title_ja: "邦題" })
    expect(rec).toEqual({ jatitle: "邦題" } satisfies JavMeta)
  })

  it("returns null for null/empty/no-useful-field input", () => {
    expect(parseR18(null)).toBeNull()
    expect(parseR18({})).toBeNull()
    expect(parseR18({ actresses: [] })).toBeNull()
    // an actress with no usable name yields no cast_ja -> null
    expect(parseR18({ actresses: [{}] })).toBeNull()
  })
})

// ------------------------------------------------------------------ parseJavdb

describe("parseJavdb — javdatabase movie page -> JavMeta", () => {
  const html = fixture("javdatabase-ssis001.html")

  it("extracts cast from the <title>, plus the first date on the live page", () => {
    const rec = parseJavdb(html, "SSIS-001")
    expect(rec).not.toBeNull()
    // cast = the names between the code and "JAV Database" in <title>
    expect(rec?.cast).toBe("Sayaka Otoshiro, Tsukasa Aoi")
    // FIDELITY: the Python takes the FIRST YYYY-MM-DD on the page verbatim,
    // which on the current markup is the page-metadata date, not the release
    // date. This port reproduces that exactly.
    expect(rec?.date).toBe("2025-01-17")
    // FIDELITY: javdatabase now spells runtime as "147 ... minutes" so the
    // Python's `\d{2,3}\s*min` regex misses — so runtime is absent here too.
    expect(rec?.runtime).toBeUndefined()
  })

  it("returns null when the HTML is shorter than 2000 chars (Python guard)", () => {
    expect(parseJavdb("<title>SSIS-001 - X - JAV Database</title>", "SSIS-001")).toBeNull()
    expect(parseJavdb("", "SSIS-001")).toBeNull()
  })

  it("parses date + runtime from a hand-crafted page using the Python regexes", () => {
    // Pad past the 2000-char guard, then include a real "NN min" token.
    const pad = "x".repeat(2100)
    const page =
      `<title>ABC-123 - Some Actress - JAV Database</title>` +
      `${pad} released 2020-05-04 and runs 118 min total.`
    const rec = parseJavdb(page, "ABC-123")
    expect(rec).toEqual({
      cast: "Some Actress",
      date: "2020-05-04",
      runtime: "118 min",
    } satisfies JavMeta)
  })

  it("drops the cast when the captured group contains 'jav' (Python guard)", () => {
    const pad = "y".repeat(2100)
    // A title with no real cast falls back to a 'JAV ...' string -> dropped.
    const page = `<title>XYZ-9 - JAV Movie - JAV Database</title>${pad} on 2019-01-01.`
    const rec = parseJavdb(page, "XYZ-9")
    expect(rec?.cast).toBeUndefined()
    expect(rec?.date).toBe("2019-01-01")
  })

  it("escapes regex metacharacters in the code (e.g. FC2-PPV-123456)", () => {
    const pad = "z".repeat(2100)
    const page =
      `<title>FC2-PPV-123456 - Someone - JAV Database</title>${pad} 2022-03-03`
    const rec = parseJavdb(page, "FC2-PPV-123456")
    expect(rec?.cast).toBe("Someone")
  })

  it("returns null when nothing matches even past the size guard", () => {
    const page = "<html>" + "q".repeat(2100) + "</html>"
    expect(parseJavdb(page, "SSIS-001")).toBeNull()
  })
})
