/**
 * Unit tests for the Discover view model helpers (src/views/discover/model.ts).
 * Discover shows every feed in its source order (no client-side re-ranking), so
 * the only model logic with branches left is the in-library detection.
 */
import { describe, expect, it } from "vitest"
import type { DiscoverItem, LibraryItem } from "@/api/types"
import { itemState, ownedKeys } from "@/views/discover/model"

describe("ownedKeys / itemState in-library detection", () => {
  const lib = (code: string, fname: string): LibraryItem =>
    ({ code, fname, title: "" }) as LibraryItem
  const disc = (code: string): DiscoverItem =>
    ({ id: `d_${code}`, code, title: code }) as DiscoverItem

  it("matches a zero-padded on-disk JAV code against an unpadded feed code", () => {
    // disk "AJVR-00306-A.mp4" -> parseCode "AJVR-00306"; JavDB feed "AJVR-306".
    const owned = ownedKeys([lib("AJVR-00306", "AJVR-00306-A.mp4")])
    expect(itemState(disc("AJVR-306"), owned, {}).state).toBe("library")
  })

  it("matches when the feed code is padded and the disk code is not", () => {
    const owned = ownedKeys([lib("SIVR-476", "SIVR-476.mp4")])
    expect(itemState(disc("SIVR-00476"), owned, {}).state).toBe("library")
  })

  it("still flags a genuinely-absent code as new", () => {
    const owned = ownedKeys([lib("AJVR-00306", "AJVR-00306-A.mp4")])
    expect(itemState(disc("SIVR-999"), owned, {}).state).toBe("new")
  })

  it("matches on an exact uppercased title (movies/TV)", () => {
    const owned = ownedKeys([
      { code: "", fname: "GoodFellas.mkv", title: "GoodFellas" } as LibraryItem,
    ])
    expect(
      itemState(
        { id: "m1", code: "", title: "GoodFellas" } as DiscoverItem,
        owned,
        {}
      ).state
    ).toBe("library")
  })
})
