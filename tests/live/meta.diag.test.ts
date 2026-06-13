import { writeFileSync, appendFileSync } from "node:fs"
import { describe, it, vi } from "vitest"
const OUT = "/tmp/av_meta_diag.jsonl"
const H = vi.hoisted(() => ({ nodeFetch: async (u: string, i: any = {}) => { const { connectTimeout, ...r } = i ?? {}; return globalThis.fetch(u, { ...r, signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30000)) }) } }))
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@/state/db", () => ({ isDbAvailable: () => false, getKey: async () => null, getCached: async () => null, setCached: async () => undefined, getCachedCover: async () => null, setCachedCover: async () => undefined }))
import { metaLookup } from "@/api/meta"

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("meta cast diag", () => {
  it("reports cast_ja/cast per code (the 出演 source)", async () => {
    writeFileSync(OUT, "")
    for (const code of (process.env.CODES || "ABF-032,SSIS-001,459TEN-048,AJVR-00277,MIVR-081").split(",")) {
      const m = await metaLookup({ code })
      appendFileSync(OUT, JSON.stringify({ code, jatitle: m.jatitle ?? null, cast_ja: m.cast_ja ?? null, cast: m.cast ?? null }) + "\n")
    }
  }, 300_000)
})
