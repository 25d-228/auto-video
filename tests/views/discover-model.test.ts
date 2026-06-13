/**
 * Unit tests for the Discover view model helpers (src/views/discover/model.ts).
 * Focused on availableRanks — the "make options meaningful" rule that offers a
 * Rank option only when it reorders the pool differently from one already on
 * offer (ordering-based dedupe, not just "does the data have seeders/ratings").
 */
import { describe, expect, it } from "vitest"
import type { DiscoverItem } from "@/api/types"
import { availableRanks } from "@/views/discover/model"

/** Minimal item — availableRanks reads id/seeders/rating (added defaults to feed pos). */
const mk = (id: string, seeders: number, rating: number) =>
  ({ id, seeders, rating } as DiscoverItem)

describe("availableRanks", () => {
  it("offers only popularity for an empty pool", () => {
    expect(availableRanks([])).toEqual(["popularity"])
  })

  it("collapses to popularity when no seeders or ratings (DMM/MGStage)", () => {
    expect(
      availableRanks([mk("a", 0, 0), mk("b", 0, 0), mk("c", 0, 0)])
    ).toEqual(["popularity"])
  })

  it("collapses to popularity when seeders are uniform (javdb VR, tpb trending)", () => {
    // Every item reports the same seeder count -> popularity/seeders/recency all
    // produce the feed order, so the control is pointless. This is the bug the
    // user hit on VR -> JavDB -> newest (uniform magnet count of 1).
    expect(
      availableRanks([mk("a", 1, 0), mk("b", 1, 0), mk("c", 1, 0)])
    ).toEqual(["popularity"])
  })

  it("adds rating when it reorders the pool (tmdb/imdb)", () => {
    // seeders all 0 -> popularity == seeders == recency == feed; ratings reorder.
    expect(
      availableRanks([mk("a", 0, 5), mk("b", 0, 9), mk("c", 0, 7)])
    ).toEqual(["popularity", "rating"])
  })

  it("adds recency when it differs from popularity (seeders shift the score)", () => {
    // c's seeders push it to the top of popularity (and seeders), so recency
    // (pure feed order) is a distinct ordering and is offered.
    expect(
      availableRanks([mk("a", 0, 0), mk("b", 0, 0), mk("c", 100, 0)])
    ).toEqual(["popularity", "recency"])
  })

  it("keeps results in canonical RANK_OPTIONS order with popularity first", () => {
    const ranks = availableRanks([mk("a", 0, 0), mk("b", 0, 9), mk("c", 100, 0)])
    expect(ranks[0]).toBe("popularity")
    const canonical = ["popularity", "seeders", "recency", "rating"]
    const idx = ranks.map((r) => canonical.indexOf(r))
    expect(idx).toEqual([...idx].sort((x, y) => x - y)) // strictly increasing
  })
})
