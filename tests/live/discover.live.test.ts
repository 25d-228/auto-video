/**
 * LIVE Discover matrix probe — NOT a normal unit test. It hits the real
 * upstreams (TMDB / IMDb / YTS / TPB / javdb / DMM / MGStage / Sukebei) to
 * confirm every (category × provider × list) combo in DISC_CATALOG still
 * returns reasonable data. Skipped unless RUN_LIVE=1, so the normal suite
 * never touches the network.
 *
 * Run (all 45 combos, sequential & polite):
 *   TMDB_KEY=$(sqlite3 "$HOME/Library/Application Support/com.plushie.autovideo/autovideo.db" \
 *     "SELECT value FROM provider_keys WHERE provider='tmdb';") \
 *   RUN_LIVE=1 npx vitest run tests/live/discover.live.test.ts --reporter=basic
 *
 * Run a slice (host-grouped fan-out):
 *   COMBOS='[["mov","tmdb","trending"],["tv","tmdb","popular"]]' TMDB_KEY=... RUN_LIVE=1 npx vitest run ...
 *
 * Plumbing: @tauri-apps/plugin-http -> Node global fetch (same machine/IP as
 * the app; undici handles gzip + arbitrary headers). @/state/db -> stub that
 * supplies the real tmdb key and forces cache misses (always live). Cover
 * resolution calls URL.createObjectURL (no DOM in Node) -> stubbed so DMM /
 * javdb cover steps "succeed" and items aren't dropped for a missing blob.
 */
import { appendFileSync, writeFileSync } from "node:fs"
import { beforeAll, describe, it, vi } from "vitest"
import type { Cat } from "@/api/types"

/** Where each RESULT line is appended (vitest v4 buffers console when piped). */
const OUT = process.env.LIVE_OUT ?? "/tmp/av_live_results.jsonl"

const H = vi.hoisted(() => {
  // Map the plugin's connectTimeout onto a (generous) total AbortSignal so a
  // slow scrape doesn't get cut off; strip it before handing init to fetch.
  async function nodeFetch(
    url: string,
    init: (RequestInit & { connectTimeout?: number }) | undefined = {}
  ): Promise<Response> {
    const { connectTimeout, ...rest } = init ?? {}
    const ms = Math.max(connectTimeout ?? 0, 30_000)
    return globalThis.fetch(url, { ...rest, signal: AbortSignal.timeout(ms) })
  }
  return { nodeFetch }
})

vi.mock("@tauri-apps/plugin-http", () => ({ fetch: H.nodeFetch }))

vi.mock("@/state/db", () => ({
  isDbAvailable: () => true,
  getKey: async (p: string) => (p === "tmdb" ? process.env.TMDB_KEY ?? null : null),
  getCached: async () => null, // force a live fetch every time
  setCached: async () => undefined,
  getCachedCover: async () => null,
  setCachedCover: async () => undefined,
}))

import { discover } from "@/api/discover"
import { DISC_CATALOG } from "@/views/discover/model"

const RUN = Boolean(process.env.RUN_LIVE)

describe.skipIf(!RUN)("live discover matrix", () => {
  beforeAll(() => {
    // jsdom/Node has no Blob URL factory; stub so cover resolution succeeds.
    let n = 0
    globalThis.URL.createObjectURL = () => `blob:live/${n++}`
    globalThis.URL.revokeObjectURL = () => {}
    if (!process.env.LIVE_APPEND) writeFileSync(OUT, "") // truncate on a full run
  })

  it("fetches every (cat × provider × list) combo", async () => {
    const combos: [Cat, string, string][] = []
    if (process.env.COMBOS) {
      for (const c of JSON.parse(process.env.COMBOS)) combos.push(c)
    } else {
      for (const cat of Object.keys(DISC_CATALOG) as Cat[]) {
        for (const prov of DISC_CATALOG[cat]) {
          for (const list of prov.lists) combos.push([cat, prov.provider, list])
        }
      }
    }

    for (const [cat, prov, list] of combos) {
      const t0 = performance.now()
      let line: Record<string, unknown>
      try {
        const items = await discover(cat, prov, list, 12, true)
        line = {
          k: `${cat}|${prov}|${list}`,
          n: items.length,
          cov: items.filter((x) => x.cover).length,
          ms: Math.round(performance.now() - t0),
          s: items.slice(0, Number(process.env.SAMPLES) || 3).map((x) => x.title),
        }
      } catch (e) {
        line = {
          k: `${cat}|${prov}|${list}`,
          n: -1,
          err: e instanceof Error ? e.message : String(e),
          ms: Math.round(performance.now() - t0),
        }
      }
      appendFileSync(OUT, "RESULT " + JSON.stringify(line) + "\n")
      await new Promise((r) => setTimeout(r, 250)) // be polite between hosts
    }
  }, 900_000)
})
