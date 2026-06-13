import { writeFileSync, appendFileSync } from "node:fs"
import { describe, it, vi } from "vitest"
const OUT = "/tmp/av_mgcover.jsonl"
const H = vi.hoisted(() => ({ nodeFetch: async (u: string, i: any = {}) => { const { connectTimeout, ...r } = i ?? {}; return globalThis.fetch(u, { ...r, signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30000)) }) } }))
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@/state/db", () => ({ isDbAvailable: () => false, getKey: async () => null, getCached: async () => null, setCached: async () => undefined, getCachedCover: async () => null, setCachedCover: async () => undefined }))
import { mgstageCover } from "@/api/sources/mgstage"

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("mgstage cover raw-url", () => {
  it("returns a RAW image.mgstage.com url (not a blob)", async () => {
    globalThis.URL.createObjectURL = () => "blob:should-not-be-returned"
    globalThis.URL.revokeObjectURL = () => {}
    writeFileSync(OUT, "")
    for (const code of (process.env.CODES || "ABF-358,300MIUM-1380,SIRO-2342").split(",")) {
      const r = await mgstageCover(code).catch((e) => ({ url: "ERR:" + e?.message, ar: 0 }))
      appendFileSync(OUT, JSON.stringify({ code, url: r.url.slice(0, 60), isBlob: r.url.startsWith("blob:"), isRaw: r.url.startsWith("http") }) + "\n")
    }
  }, 180_000)
})
