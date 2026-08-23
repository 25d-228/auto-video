import { describe, expect, it } from "vitest";

import {
  naturalWidthGalleryPages,
  providerGalleryMetrics,
} from "@/gallery-pagination";

const items = Array.from({ length: 40 }, (_, index) => ({
  id: `item-${index + 1}`,
  ratio: index % 3 === 0 ? 0.5 : index % 3 === 1 ? 0.72 : 0.9,
}));

function pages(width: number, height: number, ratios = new Map<string, number>()) {
  return naturalWidthGalleryPages(
    items,
    (item) => item.id,
    (item) => item.ratio,
    ratios,
    width,
    height,
  );
}

describe("bounded natural-width gallery pagination", () => {
  it("uses the same finite three-row capacity for Discover and Library", () => {
    const threeRows =
      providerGalleryMetrics.cardHeight * 3 + providerGalleryMetrics.rowGap * 2;
    const discover = pages(980, threeRows);
    const library = pages(980, threeRows);

    expect(discover[0]).toEqual(library[0]);
    expect(discover[0]).toHaveLength(20);
    expect(pages(980, threeRows + providerGalleryMetrics.cardHeight - 1)[0]).toHaveLength(20);
  });

  it.each([
    [720, 520],
    [1024, 520],
    [1234, 812],
    [1440, 900],
    [1600, 1000],
  ])("fits complete source-ordered cards within %d × %d", (width, height) => {
    const result = pages(width, height);
    expect(result.flat()).toEqual(items);
    expect(result.every((page) => page.length > 0)).toBe(true);
  });

  it("updates capacity immediately when a retained ratio changes", () => {
    const height = providerGalleryMetrics.cardHeight * 2 + providerGalleryMetrics.rowGap;
    const before = pages(720, height);
    const after = pages(720, height, new Map([["item-1", 2]]));

    expect(after[0].length).toBeLessThan(before[0].length);
    expect(after.flat()).toEqual(items);
  });
});
