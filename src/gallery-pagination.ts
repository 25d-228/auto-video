export const providerGalleryMetrics = {
  cardHeight: 260,
  coverHeight: 180,
  columnGap: 14,
  rowGap: 16,
} as const;

export function naturalWidthGalleryPages<Item>(
  items: Item[],
  itemKey: (item: Item) => string,
  sourceRatio: (item: Item) => number,
  retainedRatios: ReadonlyMap<string, number>,
  viewportWidth: number,
  viewportHeight: number,
) {
  if (items.length === 0) return [[]];
  if (viewportWidth <= 0 || viewportHeight <= 0) {
    return items.map((item) => [item]);
  }

  const completeRows = Math.max(
    1,
    Math.floor(
      (viewportHeight + providerGalleryMetrics.rowGap) /
        (providerGalleryMetrics.cardHeight + providerGalleryMetrics.rowGap),
    ),
  );
  const pages: Item[][] = [];
  let page: Item[] = [];
  let rows = 1;
  let rowWidth = 0;

  for (const item of items) {
    const ratio = retainedRatios.get(itemKey(item)) ?? sourceRatio(item);
    const cardWidth = Math.min(
      viewportWidth,
      Math.round(providerGalleryMetrics.coverHeight * ratio),
    );
    const nextWidth =
      rowWidth === 0
        ? cardWidth
        : rowWidth + providerGalleryMetrics.columnGap + cardWidth;
    if (rowWidth !== 0 && nextWidth > viewportWidth) {
      if (rows === completeRows) {
        pages.push(page);
        page = [];
        rows = 1;
      } else {
        rows += 1;
      }
      rowWidth = cardWidth;
    } else {
      rowWidth = nextWidth;
    }
    page.push(item);
  }
  if (page.length > 0) pages.push(page);
  return pages;
}
