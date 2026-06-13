import { writeFileSync, appendFileSync } from "node:fs"
import { describe, it, vi } from "vitest"
const OUT = "/tmp/av_jdbsearch.jsonl"
const H = vi.hoisted(() => ({ nodeFetch: async (u: string, i: any = {}) => { const { connectTimeout, ...r } = i ?? {}; return globalThis.fetch(u, { ...r, signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30000)) }) } }))
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@/state/db", () => ({ isDbAvailable: () => false, getKey: async () => null, getCached: async () => null, setCached: async () => undefined, getCachedCover: async () => null, setCachedCover: async () => undefined }))
import { javdbApi } from "@/api/sources/javdb"

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("javdb search diag", () => {
  it("probes search endpoints for a code, then detail->actors", async () => {
    writeFileSync(OUT, "")
    const code = process.env.CODE || "MIVR-081"
    const paths = [
      `/api/v1/search/movies?q=${encodeURIComponent(code)}`,
      `/api/v2/search?q=${encodeURIComponent(code)}&type=movie`,
      `/api/v1/movies/search?q=${encodeURIComponent(code)}`,
      `/api/v1/search?q=${encodeURIComponent(code)}`,
    ]
    for (const p of paths) {
      let shape = "null"
      try {
        const r: any = await javdbApi(p)
        if (r) shape = JSON.stringify(r).slice(0, 300)
      } catch (e: any) { shape = "ERR " + (e?.message || e) }
      appendFileSync(OUT, JSON.stringify({ path: p, shape }) + "\n")
    }
  }, 120_000)
})
