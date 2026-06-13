/**
 * LIVE diagnostic: do popularity/seeders/recency/rating actually produce
 * different orderings for a given Discover feed? Skipped unless RUN_LIVE=1.
 * COMBOS='[["vrc","javdb","newest"],...]' to choose feeds (default that one).
 */
import { appendFileSync, writeFileSync } from "node:fs"
import { beforeAll, describe, it, vi } from "vitest"
import type { Cat } from "@/api/types"

const OUT = process.env.LIVE_OUT ?? "/tmp/av_rank_diag.jsonl"

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
  getCachedCover: async () => null,
  setCachedCover: async () => undefined,
}))

import { discover } from "@/api/discover"
import { ingest, sortList, type Rank } from "@/views/discover/model"

const RUN = Boolean(process.env.RUN_LIVE)

describe.skipIf(!RUN)("rank ordering diagnostic", () => {
  beforeAll(() => {
    let n = 0
    globalThis.URL.createObjectURL = () => `blob:live/${n++}`
    globalThis.URL.revokeObjectURL = () => {}
    writeFileSync(OUT, "")
  })

  it("compares orderings per rank", async () => {
    const combos: [Cat, string, string][] = process.env.COMBOS
      ? JSON.parse(process.env.COMBOS)
      : [["vrc", "javdb", "newest"]]
    for (const [cat, prov, list] of combos) {
      const items = await discover(cat, prov, list, 25, true)
      const scored = ingest(items)
      const ranks: Rank[] = ["popularity", "seeders", "recency", "rating"]
      const orders: Record<string, string> = {}
      for (const r of ranks) {
        orders[r] = sortList(scored, r).map((x) => x.code || x.id).join(",")
      }
      const seeders = scored.map((x) => x.seeders)
      const ratings = scored.map((x) => x.rating)
      appendFileSync(
        OUT,
        JSON.stringify({
          k: `${cat}|${prov}|${list}`,
          n: items.length,
          distinctSeeders: [...new Set(seeders)].sort((a, b) => a - b),
          distinctRatings: [...new Set(ratings)],
          samePopSeed: orders.popularity === orders.seeders,
          samePopRec: orders.popularity === orders.recency,
          sameSeedRec: orders.seeders === orders.recency,
        }) + "\n"
      )
    }
  }, 300_000)
})
