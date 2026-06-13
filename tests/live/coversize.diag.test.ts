import { writeFileSync, appendFileSync } from "node:fs"
import { describe, it, vi } from "vitest"
const OUT = "/tmp/av_coversize.jsonl"
const H = vi.hoisted(() => ({ nodeFetch: async (u: string, i: any = {}) => { const { connectTimeout, ...r } = i ?? {}; return globalThis.fetch(u, { ...r, signal: AbortSignal.timeout(Math.max(connectTimeout ?? 0, 30000)) }) } }))
vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))
vi.mock("@/state/db", () => ({ isDbAvailable: () => false, getKey: async () => null, getCached: async () => null, setCached: async () => undefined, getCachedCover: async () => null, setCachedCover: async () => undefined }))
import { dmmCover, imgDims } from "@/api/sources/dmm"
import { mgstageCover } from "@/api/sources/mgstage"
import { javdbApi } from "@/api/sources/javdb"
import { httpBytes, decryptCmastd, isCmastdCover } from "@/net/http"

async function measure(url: string, referer?: string) {
  try {
    let b = await httpBytes(url, referer ? { referer } : {})
    if (isCmastdCover(url)) b = decryptCmastd(b)
    const d = imgDims(b)
    return d ? { w: d[0], h: d[1], ar: +(d[0] / d[1]).toFixed(2), kb: Math.round(b.length / 1024) } : { err: "no-dims", kb: Math.round(b.length / 1024) }
  } catch (e: any) { return { err: String(e?.message || e).slice(0, 40) } }
}

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("cover size by source", () => {
  it("measures DMM ps/pl, MGStage, cmastd per code", async () => {
    writeFileSync(OUT, "")
    for (const code of (process.env.CODES || "SSIS-001,GOJU-261,DASS-813,SIVR-490").split(",")) {
      const out: any = { code }
      // DMM: dmmCover picks a suffix; also try the wide pl explicitly
      const dmm = await dmmCover(code).catch(() => ({ url: "" }))
      out.dmm_chosen = dmm.url ? { url: dmm.url.replace(/^https:\/\/pics\.dmm\.co\.jp\//, ""), ...(await measure(dmm.url, "https://www.dmm.co.jp/")) } : null
      if (dmm.url) {
        const pl = dmm.url.replace(/p[sl]\.jpg$/i, "pl.jpg")
        const ps = dmm.url.replace(/p[sl]\.jpg$/i, "ps.jpg")
        out.dmm_pl = await measure(pl, "https://www.dmm.co.jp/")
        out.dmm_ps = await measure(ps, "https://www.dmm.co.jp/")
      }
      // MGStage raw
      const mg = await mgstageCover(code).catch(() => ({ url: "" }))
      out.mgstage = mg.url ? await measure(mg.url, "https://www.mgstage.com/") : null
      // javdb cmastd cover_url (and small thumb)
      const s: any = await javdbApi(`/api/v2/search?q=${encodeURIComponent(code)}&type=movie`).catch(() => null)
      const m = (s?.movies ?? []).find((x: any) => (x.number || "").toUpperCase() === code.toUpperCase()) ?? (s?.movies ?? [])[0]
      out.cmastd_cover = m?.cover_url ? await measure(m.cover_url) : null
      out.cmastd_thumb = m?.thumb_url ? await measure(m.thumb_url) : null
      appendFileSync(OUT, JSON.stringify(out) + "\n")
    }
  }, 300_000)
})
