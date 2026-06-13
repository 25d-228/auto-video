/**
 * LIVE diagnostic for MGStage item counts — why does ad·trending show so few?
 * Skipped unless RUN_LIVE=1. Measures, for the week & day ranking pages:
 *   raw    = products parsed from the page
 *   jacket = items whose own wide-jacket cover proxies OK (fetchMgstage)
 *   shipped= items the current discover() path actually returns (clear jacket +
 *            resolve portrait by code + keepCovered) — the lossy path.
 * Also lists candidate ranking/list URLs found in the page markup.
 */
import { appendFileSync, writeFileSync } from "node:fs"
import { beforeAll, describe, it, vi } from "vitest"

const OUT = process.env.LIVE_OUT ?? "/tmp/av_mg_diag.jsonl"

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
vi.mock("@/state/db", () => ({
  isDbAvailable: () => true,
  getKey: async () => null,
  getCached: async () => null,
  setCached: async () => undefined,
  getCachedCover: async () => null, // bypass the negative cover-cache
  setCachedCover: async () => undefined,
}))

import { httpText } from "@/net/http"
import { discover } from "@/api/discover"
import { fetchMgstage, parseMgstageList } from "@/api/sources/mgstage"

const RUN = Boolean(process.env.RUN_LIVE)

describe.skipIf(!RUN)("mgstage diagnostic", () => {
  beforeAll(() => {
    let n = 0
    globalThis.URL.createObjectURL = () => `blob:live/${n++}`
    globalThis.URL.revokeObjectURL = () => {}
    writeFileSync(OUT, "")
  })

  it("counts raw / jacket / shipped for the ranking lists", async () => {
    const rec = (o: Record<string, unknown>) =>
      appendFileSync(OUT, JSON.stringify(o) + "\n")

    for (const [label, id] of [
      ["trending", "week"],
      ["newest", "day"],
    ] as const) {
      const url = `https://www.mgstage.com/ranking/ranking.php?id=${id}`
      const html = await httpText(url, {
        cookie: "adc=1",
        referer: "https://www.mgstage.com/",
      }).catch(() => "")
      const raw = parseMgstageList(html).length

      const fetched = await fetchMgstage(false, label)
      const jacket = fetched.filter((x) => x.cover).length

      const shipped = (await discover("ad", "mgstage", label, 100, true)).length

      // any other ranking/list endpoints linked on the page
      const links = Array.from(
        new Set(
          (html.match(/ranking\.php\?[^"'\s]*/g) ?? []).map((s) => s.slice(0, 60))
        )
      ).slice(0, 12)

      rec({ list: label, id, htmlBytes: html.length, raw, jacket, shipped, links })
    }
  }, 600_000)
})
