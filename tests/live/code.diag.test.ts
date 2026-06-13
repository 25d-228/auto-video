import { writeFileSync, appendFileSync } from "node:fs"
import { describe, it, vi } from "vitest"

const OUT = process.env.LIVE_OUT ?? "/tmp/av_code_diag.jsonl"
const H = vi.hoisted(() => {
  async function nodeFetch(url: string, init: any = {}) {
    const { connectTimeout, ...rest } = init ?? {}
    return globalThis.fetch(url, { ...rest, signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30_000)) })
  }
  return { nodeFetch }
})
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => { throw new Error("no invoke") } }))
vi.mock("@/state/db", () => ({
  isDbAvailable: () => true,
  getKey: async () => null,
  getCached: async () => null,
  setCached: async () => undefined,
  getCachedCover: async () => null,
  setCachedCover: async () => undefined,
}))
import { cover } from "@/api/client"

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("code diag", () => {
  it("probes code forms", async () => {
    writeFileSync(OUT, "")
    const codes = (process.env.CODES || "TEN-048,TEN-055,MIVR-00081,MIVR-081,MIVR-0081,MIVR-81").split(",")
    for (const c of codes) {
      let ok = false
      try { ok = (await cover(c)).ok } catch { ok = false }
      appendFileSync(OUT, JSON.stringify({ code: c, ok }) + "\n")
    }
  }, 300_000)
})
