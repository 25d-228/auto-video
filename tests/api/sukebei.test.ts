import { describe, expect, it } from "vitest"
import {
  parseSukebeiList,
  quality,
  sortSukebei,
  type SukebeiItem,
} from "@/api/sources/sukebei"

// The fixtures are real sukebei.nyaa.si listing pages recorded live with curl
// (see the gitignored tests/fixtures/sukebei-*.html):
//   sukebei-list.html   -> /?c=2_2&s=seeders&o=desc&p=1  (whole category)
//   sukebei-search.html -> /?c=2_2&q=SSIS-001&s=seeders   (a code search)
// They are pulled in as raw strings via Vite's import.meta.glob (no node
// built-ins, so the DOM-only tsconfig.app.json stays clean) and parsed with NO
// network access. Absent fixtures make the loader throw a clear message.
const rawFixtures = import.meta.glob("../fixtures/sukebei-*.html", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>

function loadFixture(name: string): string {
  const key = Object.keys(rawFixtures).find((k) => k.endsWith(name))
  const raw = key ? rawFixtures[key] : undefined
  if (!raw) throw new Error(`missing tests/fixtures/${name} (record it first)`)
  return raw
}

describe("parseSukebeiList (listing fixture)", () => {
  const items = parseSukebeiList(loadFixture("sukebei-list.html"))

  it("parses every row in the <tbody>", () => {
    expect(items.length).toBe(75)
  })

  it("maps the first row to a faithful DiscoverItem", () => {
    const first = items[0]!
    expect(first.id).toBe("sk_4621826")
    expect(first.src).toBe("sukebei")
    expect(first.state).toBe("new")
    expect(first.ar).toBe(0.72)
    // size is the [-5] text-center cell, seeders [-3], completed [-1].
    expect(first.size).toBe("5.5 GiB")
    expect(first.seeders).toBe(968)
    expect(first._downloads).toBe(5651)
    // code parsed off the title; title falls back to the code.
    expect(first.code).toBe("ABF-358")
    expect(first.title).toBe("ABF-358")
    expect(first._rawtitle).toBe("+++ [FHD] ABF-358 究極のぬるぬるオーガズム 涼森れむ")
    // link is the nyaa view page.
    expect(first.link).toBe("https://sukebei.nyaa.si/view/4621826")
  })

  it("unescapes the magnet (&amp; -> &) and keeps the info hash", () => {
    const magnet = items[0]!.magnet!
    expect(magnet.startsWith("magnet:?xt=urn:btih:")).toBe(true)
    expect(magnet).toContain("c8b44cc0e1148e51c4cfb799891764050cef5f89")
    // the raw HTML had &amp; between params; unescape must collapse them.
    expect(magnet).not.toContain("&amp;")
    expect(magnet).toContain("&dn=")
  })

  it("normalizes FC2-PPV codes via parseCode", () => {
    const fc2 = items.find((x) => x.id === "sk_4621910")!
    expect(fc2.code).toBe("FC2-PPV-4916530")
  })

  it("leaves cover empty (the aggregator resolves it later) but keeps the code", () => {
    for (const x of items) {
      expect(x.cover).toBe("")
      if (x.code) expect(typeof x.code).toBe("string")
    }
  })

  it("flags VR titles via isVr", () => {
    // every item carries a boolean vr flag derived from title + code.
    for (const x of items) expect(typeof x.vr).toBe("boolean")
  })

  it("dedups view-ids across pages via the shared seen set", () => {
    const html = loadFixture("sukebei-list.html")
    const seen = new Set<string>()
    const a = parseSukebeiList(html, seen)
    const b = parseSukebeiList(html, seen)
    expect(a.length).toBe(75)
    // second pass over the same page yields no new rows (all ids already seen).
    expect(b.length).toBe(0)
  })
})

describe("parseSukebeiList (search fixture)", () => {
  const items = parseSukebeiList(loadFixture("sukebei-search.html"))

  it("parses the code-search result page", () => {
    expect(items.length).toBe(25)
    const first = items[0]!
    expect(first.id).toBe("sk_3388693")
    expect(first.size).toBe("7.0 GiB")
    expect(first.seeders).toBe(4)
    expect(first._downloads).toBe(20334)
    expect(first.magnet!).toContain("f47e23b5bc6a5ded3bfd30b94c3c7d7a896a866a")
  })
})

describe("parseSukebeiList (malformed HTML)", () => {
  it("returns [] when there is no <tbody>", () => {
    expect(parseSukebeiList("<html><body>no table</body></html>")).toEqual([])
  })

  it("returns [] for an empty <tbody>", () => {
    expect(parseSukebeiList("<table><tbody></tbody></table>")).toEqual([])
  })
})

describe("sortSukebei", () => {
  function make(seeders: number, downloads: number, id: string): SukebeiItem {
    return {
      id,
      cat: "ad",
      title: id,
      _rawtitle: id,
      sub: "",
      cover: "",
      ar: 0.72,
      seeders,
      _downloads: downloads,
      size: "",
      src: "sukebei",
      state: "new",
      year: "",
      runtime: 0,
      rating: 0,
      code: "",
      magnet: "",
      vr: false,
      link: "",
    }
  }

  it("most_seeded sorts by seeders desc", () => {
    const out = sortSukebei(
      [make(1, 9, "a"), make(5, 1, "b"), make(3, 2, "c")],
      "most_seeded"
    )
    expect(out.map((x) => x.id)).toEqual(["b", "c", "a"])
  })

  it("most_downloaded sorts by completed (downloads) desc", () => {
    const out = sortSukebei(
      [make(1, 9, "a"), make(5, 1, "b"), make(3, 2, "c")],
      "most_downloaded"
    )
    expect(out.map((x) => x.id)).toEqual(["a", "c", "b"])
  })

  it("newest keeps the page (id) order", () => {
    const out = sortSukebei(
      [make(1, 9, "a"), make(5, 1, "b"), make(3, 2, "c")],
      "newest"
    )
    expect(out.map((x) => x.id)).toEqual(["a", "b", "c"])
  })
})

describe("quality", () => {
  it("extracts the first known quality token, uppercased", () => {
    expect(quality("Some.Title.1080p.x264")).toBe("1080p".toUpperCase())
    expect(quality("Movie 2160p HEVC")).toBe("2160P")
    expect(quality("clip 720p")).toBe("720P")
  })

  it("returns '' when no quality token is present", () => {
    expect(quality("ABF-358 究極のぬるぬる")).toBe("")
    expect(quality("")).toBe("")
  })
})
