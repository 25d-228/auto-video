/**
 * LIVE library cover hit-rate probe — NOT a normal unit test. Walks the real
 * library folders, groups them into items via buildItems (the structure-aware
 * scan/parse), and resolves a cover for each (JAV code -> cover(); movie/TV ->
 * TMDB lookup). Reports per-category hit-rate + the failing identifiers so we
 * can iterate until covers are stable. Skipped unless RUN_LIVE=1.
 *
 * Run:
 *   TMDB_KEY=$(sqlite3 "$HOME/Library/Application Support/com.plushie.autovideo/autovideo.db" \
 *     "SELECT value FROM provider_keys WHERE provider='tmdb';") \
 *   SAMPLE=12 CATS=ad,vrc,mov,tv RUN_LIVE=1 npx vitest run tests/live/library-covers.live.test.ts
 *   (SAMPLE unset/0 = every item; CATS unset = all four)
 */
import { appendFileSync, readdirSync, statSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { beforeAll, describe, it, vi } from "vitest"
import type { Cat } from "@/api/types"

const OUT = process.env.LIVE_OUT ?? "/tmp/av_libcov.jsonl"
const SAMPLE = Number(process.env.SAMPLE) || 0 // 0 = all
const CONC = Number(process.env.CONC) || 4

const H = vi.hoisted(() => {
  async function nodeFetch(
    url: string,
    init: (RequestInit & { connectTimeout?: number }) | undefined = {}
  ): Promise<Response> {
    const { connectTimeout, ...rest } = init ?? {}
    return globalThis.fetch(url, {
      ...rest,
      signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30_000)),
    })
  }
  return { nodeFetch }
})
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async () => {
    throw new Error("invoke not available in harness")
  },
}))
vi.mock("@/state/db", () => ({
  isDbAvailable: () => true,
  getKey: async (p: string) => (p === "tmdb" ? process.env.TMDB_KEY ?? null : null),
  allPaths: async () => ({}),
  getCached: async () => null,
  setCached: async () => undefined,
  getCachedCover: async () => null,
  setCachedCover: async () => undefined,
}))

import { buildItems } from "@/api/library"
import { cover, movieLookup, tvLookup } from "@/api/client"
import type { LibraryItem } from "@/api/types"

const ROOTS: Record<Cat, string> = {
  mov: "/Volumes/Be/films",
  tv: "/Volumes/Be/series",
  ad: "/Volumes/H/porn",
  vrc: "/Volumes/Be/vr",
}
const VIDEO = new Set([
  "mkv", "mp4", "avi", "wmv", "m4v", "ts", "mov", "flv", "iso", "rmvb", "webm", "mpg", "mpeg",
])

interface ScanFile {
  name: string
  path: string
  size: number
}

function walk(dir: string, out: ScanFile[], cap = 8000): void {
  if (out.length >= cap) return
  let ents
  try {
    ents = readdirSync(dir, { withFileTypes: true })
  } catch {
    return
  }
  for (const e of ents) {
    if (out.length >= cap) return
    if (e.name.startsWith(".")) continue
    const p = join(dir, e.name)
    if (e.isDirectory()) walk(p, out, cap)
    else if (e.isFile()) {
      const ext = e.name.split(".").pop()?.toLowerCase() ?? ""
      if (VIDEO.has(ext)) {
        let size = 0
        try {
          size = statSync(p).size
        } catch {
          /* ignore */
        }
        out.push({ name: e.name, path: p, size })
      }
    }
  }
}

async function mapLimit<T, R>(
  arr: T[],
  n: number,
  fn: (x: T) => Promise<R>
): Promise<R[]> {
  const ret: R[] = new Array(arr.length)
  let i = 0
  const worker = async () => {
    while (i < arr.length) {
      const idx = i++
      ret[idx] = await fn(arr[idx]!)
    }
  }
  await Promise.all(Array.from({ length: Math.min(n, arr.length) }, worker))
  return ret
}

async function resolveCover(
  it: LibraryItem
): Promise<{ ok: boolean; id: string; err?: string }> {
  try {
    if (it.cat === "ad" || it.cat === "vrc") {
      const r = await cover(it.code || "")
      return { ok: r.ok, id: it.code || it.title }
    }
    const r =
      it.cat === "tv"
        ? await tvLookup({ title: it.title })
        : await movieLookup({ title: it.title, year: it.year })
    // titleLookup returns { ok, haskey, meta:{cover} }; ok === Boolean(cover).
    return { ok: Boolean(r.ok), id: `${it.title}${it.year ? ` (${it.year})` : ""}` }
  } catch (e) {
    return { ok: false, id: it.code || it.title, err: e instanceof Error ? e.message : String(e) }
  }
}

const RUN = Boolean(process.env.RUN_LIVE)

describe.skipIf(!RUN)("library cover hit-rate", () => {
  beforeAll(() => {
    let n = 0
    globalThis.URL.createObjectURL = () => `blob:live/${n++}`
    globalThis.URL.revokeObjectURL = () => {}
    writeFileSync(OUT, "")
  })

  it("resolves covers for real library items", async () => {
    const cats: Cat[] = process.env.CATS
      ? (process.env.CATS.split(",") as Cat[])
      : ["mov", "tv", "ad", "vrc"]
    for (const cat of cats) {
      const files: ScanFile[] = []
      walk(ROOTS[cat], files)
      const all = buildItems(cat, ROOTS[cat], files)
      let items = all
      if (SAMPLE > 0 && all.length > SAMPLE) {
        const step = all.length / SAMPLE
        items = Array.from({ length: SAMPLE }, (_, k) => all[Math.floor(k * step)]!)
      }
      const t0 = performance.now()
      const res = await mapLimit(items, CONC, resolveCover)
      const ok = res.filter((r) => r.ok).length
      const fails = res.filter((r) => !r.ok).map((r) => r.id + (r.err ? ` [${r.err}]` : ""))
      appendFileSync(
        OUT,
        JSON.stringify({
          cat,
          totalItems: all.length,
          totalFiles: files.length,
          sampled: items.length,
          ok,
          rate: items.length ? +(ok / items.length).toFixed(2) : 0,
          ms: Math.round(performance.now() - t0),
          fails: fails.slice(0, 50),
        }) + "\n"
      )
    }
  }, 1_800_000)
})
