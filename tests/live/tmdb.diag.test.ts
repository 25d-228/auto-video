import { writeFileSync, appendFileSync } from "node:fs"
import { describe, it, vi } from "vitest"

const OUT = process.env.LIVE_OUT ?? "/tmp/av_tmdb_diag.jsonl"
const H = vi.hoisted(() => {
  async function nodeFetch(url: string, init: any = {}) {
    const { connectTimeout, ...rest } = init ?? {}
    return globalThis.fetch(url, { ...rest, signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30_000)) })
  }
  return { nodeFetch }
})
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@/state/db", () => ({
  isDbAvailable: () => true,
  getKey: async (p: string) => (p === "tmdb" ? process.env.TMDB_KEY ?? null : null),
  getCached: async () => null,
  setCached: async () => undefined,
  getCachedCover: async () => null,
  setCachedCover: async () => undefined,
}))
import { tmdbLookup } from "@/api/sources/tmdb"

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("tmdb diag", () => {
  it("looks up failing titles", async () => {
    writeFileSync(OUT, "")
    const movs: [string, string][] = [
      ["パーフェクトブルー", "1997"], ["東京ゴッドファーザーズ", "2003"], ["パプリカ", "2006"],
      ["サマーウォーズ", "2009"], ["バケモノの子", "2015"], ["時をかける少女", "2006"],
    ]
    const tvs: string[] = ["カウボーイビバップ", "サムライチャンプルー", "ピンポン", "化物語"]
    for (const [t, y] of movs) {
      const rec = await tmdbLookup(t, y, false)
      appendFileSync(OUT, JSON.stringify({ kind: "mov", t, y, ok: Boolean(rec?.cover), id: rec?.tmdb_id ?? null }) + "\n")
    }
    for (const t of tvs) {
      const rec = await tmdbLookup(t, "", true)
      appendFileSync(OUT, JSON.stringify({ kind: "tv", t, ok: Boolean(rec?.cover), id: rec?.tmdb_id ?? null }) + "\n")
    }
  }, 300_000)
})
