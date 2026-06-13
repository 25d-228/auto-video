/**
 * Tests for planRename (src/lib/rename.ts). Two layers:
 *  - inline cases built from REAL torrent structures observed from live seeds
 *    (commit-safe), and
 *  - a loop over tests/fixtures/torrent-files-real.json — real .torrent file
 *    lists captured per category (gitignored). Skipped if the fixtures aren't
 *    present (CI), so the inline cases always validate the algorithm.
 */
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { dirname, join } from "node:path"
import { describe, expect, it } from "vitest"
import { planRename, type RenameCat } from "@/lib/rename"
import { parseCode } from "@/lib/codes"

const VIDEO_RE = /\.(mkv|mp4|avi|wmv|m4v|ts|mov|flv|iso|rmvb|webm|mpg|mpeg)$/i
const JUNK_RE = /\.(url|txt|jpg|jpeg|png|gif|nfo|srt|ass|html?|sfv|cue|log|7z|zip|rar)$/i
const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")

describe("planRename — real-structure cases", () => {
  it("adult: single spam-named video -> <code>.ext", () => {
    expect(
      planRename("ad", "SSIS-816", [
        { name: "hhd800.com@SSIS-816.mp4", size: 13_115_273_615 },
      ])
    ).toEqual([{ from: "hhd800.com@SSIS-816.mp4", to: "SSIS-816.mp4" }])
  })

  it("adult: drops a bundled ad video + .url, keeps only the main", () => {
    expect(
      planRename("ad", "SNOS-209", [
        { name: "18+游戏大全(996gg.cc).mp4", size: 2_001_226 },
        { name: "hhd800.com@SNOS-209.mp4", size: 7_382_415_346 },
        { name: "x18r.tv.url", size: 39 },
      ])
    ).toEqual([{ from: "hhd800.com@SNOS-209.mp4", to: "SNOS-209.mp4" }])
  })

  it("vr: two large parts -> -A/-B, spam clip + .url dropped", () => {
    expect(
      planRename("vrc", "SIVR-490", [
        { name: "2026 世足官方指定网站.url", size: 175 },
        { name: "4k2.me@sivr00490_1_8k.mp4", size: 7_631_791_971 },
        { name: "4k2.me@sivr00490_2_8k.mp4", size: 10_016_419_811 },
        { name: "三上悠亚想要跟你决胜负.mp4", size: 19_545_140 },
      ])
    ).toEqual([
      { from: "4k2.me@sivr00490_1_8k.mp4", to: "SIVR-490-A.mp4" },
      { from: "4k2.me@sivr00490_2_8k.mp4", to: "SIVR-490-B.mp4" },
    ])
  })

  it("movie: keeps the video, drops srt/jpg/txt", () => {
    expect(
      planRename("mov", "2015.Chinatown", [
        { name: "Chinatown.2015.720p.WEBRip.x264.AAC-[YTS.BZ].mp4", size: 1_064_161_619 },
        { name: "Chinatown.2015.720p.WEBRip.x264.AAC-[YTS.BZ].srt", size: 52_335 },
        { name: "YTS.BZ - Official site.jpg", size: 38_690 },
        { name: "YTSYifyUP (TOR).txt", size: 840 },
      ])
    ).toEqual([
      {
        from: "Chinatown.2015.720p.WEBRip.x264.AAC-[YTS.BZ].mp4",
        to: "2015.Chinatown.mp4",
      },
    ])
  })

  it("tv: folders episodes under the show, keeps episode names", () => {
    const eps = [
      "[SubsPlease] Bocchi the Rock! - 01 (1080p).mkv",
      "[SubsPlease] Bocchi the Rock! - 02 (1080p).mkv",
    ]
    expect(
      planRename(
        "tv",
        "Bocchi the Rock",
        eps.map((n) => ({ name: n, size: 1_400_000_000 }))
      )
    ).toEqual(eps.map((n) => ({ from: n, to: `Bocchi the Rock/${n}` })))
  })

  it("returns nothing when there's no video or no base", () => {
    expect(planRename("ad", "SSIS-1", [{ name: "readme.txt", size: 10 }])).toEqual([])
    expect(planRename("ad", "", [{ name: "x.mp4", size: 10 }])).toEqual([])
  })
})

// ---- against real captured torrents (gitignored fixtures) -----------------

interface Fixture {
  cat: RenameCat
  torrentName: string
  base: string
  files: { name: string; size: number }[]
}

function loadFixtures(): Fixture[] {
  try {
    const p = join(dirname(fileURLToPath(import.meta.url)), "..", "fixtures", "torrent-files-real.json")
    return JSON.parse(readFileSync(p, "utf8")) as Fixture[]
  } catch {
    return []
  }
}

const fixtures = loadFixtures()

describe("planRename — real-seed fixtures", () => {
  ;(fixtures.length > 0 ? it : it.skip)(
    "canonical-names every category's real torrent + drops junk",
    () => {
      for (const fx of fixtures) {
        const cat = fx.cat
        // ad/vrc: the canonical code the app would pass (Discover item); else the title.
        const base = cat === "ad" || cat === "vrc" ? parseCode(fx.torrentName) : fx.base
        const plan = planRename(cat, base, fx.files)
        const ctx = `[${cat}] ${fx.torrentName} (base=${base})`

        expect(plan.length, `${ctx}: a main video is kept`).toBeGreaterThan(0)
        for (const op of plan) {
          expect(VIDEO_RE.test(op.from), `${ctx}: only videos renamed (${op.from})`).toBe(true)
          expect(JUNK_RE.test(op.from), `${ctx}: no junk renamed (${op.from})`).toBe(false)
        }
        // the largest video must be among the kept files
        const largest = [...fx.files]
          .filter((f) => VIDEO_RE.test(f.name))
          .sort((a, b) => b.size - a.size)[0]!
        expect(plan.some((op) => op.from === largest.name), `${ctx}: largest video kept`).toBe(true)

        if (cat === "tv") {
          for (const op of plan)
            expect(op.to.startsWith(`${base}/`), `${ctx}: foldered (${op.to})`).toBe(true)
        } else {
          const re = new RegExp(`^${escapeRe(base)}(-[A-Z])?\\.[a-z0-9]+$`)
          for (const op of plan)
            expect(re.test(op.to), `${ctx}: canonical flat name (${op.to})`).toBe(true)
        }
      }
    }
  )
})
