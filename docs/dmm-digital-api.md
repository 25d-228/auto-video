# FANZA digital video API (video.dmm.co.jp GraphQL) — captured map (2026-06)

The legacy scrapable digital floor (`/digital/videoa/`) is dead (301 → JS SPA). The
SPA at `https://video.dmm.co.jp/` is backed by a **public GraphQL endpoint** that
needs **no login, no cookies (not even the age cookie), no referer/origin** —
verified returning HTTP 200 with all three stripped. This is the only source of a
**real digital VR ranking** (the scrapable mono/dvd VR floor only has obscure
physical discs like `ovvr616`).

## Endpoint
- `POST https://api.video.dmm.co.jp/graphql`
- `Content-Type: application/json`, body `{"query": "...", "variables": {...}}`
- Not persisted-query locked (arbitrary queries accepted). Introspection is OFF,
  but every operation/enum/field is in the public Next.js bundles at
  `https://assets.video.dmm.co.jp/_next/static/chunks/*` (the endpoint constant is
  in chunk 2768). Undocumented → could change; treat as best-effort.

## Key queries
- **`ppvContentRanking(floor, type, limit, contentType)`** — the rankings.
  - `floor`: `AV` | `ANIME` | `CINEMA` | `AMATEUR`
  - `type`: `SALES_BEST_SELLERS` | `SALES_MONTHLY`
  - `contentType`: `VR` | `TWO_DIMENSION` (omit = whole floor, mixes 2D+VR)
  - Returns `items { rank content { id title packageImage { largeUrl } } }`.
  - **`content.id` IS the cid** (e.g. `vrkm01577`, `sivr00490`) — directly usable.
  - Covers: `packageImage.largeUrl` = `https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/<cid>/...`.
- **`legacySearchPPV(floor, sort, filter{contentType,...}, limit, offset)`** — search/browse.
  - `sort`: `SALES_RANK_SCORE` | `RELEASE_DATE` | `REVIEW_RANK_SCORE` | `PRICE_MIN` | `PRICE_MAX` | `NAME_ASC`
  - `filter.contentType: VR` etc.; `includeExplicit: true`.
  - Returns `result { contents { id title } }`.
- ~90 other named queries exist (FloorTopVr, ContentRankingPage, AvSearch,
  GenreRankingData, SeriesRanking, …), all hittable the same way.

### Verified examples
```
# Digital VR best-sellers (the headline new list — popular STREAMING VR)
{ ppvContentRanking(floor: AV, type: SALES_BEST_SELLERS, limit: 40, contentType: VR) {
    items { rank content { id title packageImage { largeUrl } } } } }
# -> vrkm01577, sivr00490, 13dsvr01947, vrkm01866, dsvr00069 ...

# Digital VR this-month best-sellers (distinct ordering)
{ ppvContentRanking(floor: AV, type: SALES_MONTHLY, limit: 40, contentType: VR) { items { rank content { id title } } } }

# 2D best-sellers (≈ the existing Adult "trending" axis)
{ ppvContentRanking(floor: AV, type: SALES_BEST_SELLERS, limit: 40, contentType: TWO_DIMENSION) { items { rank content { id title } } } }
```

## Why it matters
- **Digital VR ranking has no scrapable-HTML equivalent** — the mono/dvd VR floor
  (`article=keyword/id=6793/sort=ranking`) returns physical discs (`ovvr616…`),
  not the popular streaming titles. This API is the real thing.
- Stable: back-to-back fetches returned byte-identical top-10 (ranking content
  rotates day-to-day, as expected).

## Separately: mono/dvd dedicated RANKING pages (server HTML, for Adult/non-VR)
- `https://www.dmm.co.jp/mono/dvd/-/ranking/=/term=daily|week|monthly/` — real
  numbered best-seller rankings (server HTML, same cookie/referer recipe, clean
  canonical URL; `term` must precede `rank`/`mode` or it 301s).
  - **monthly** is strongly distinct from trending/newest/top_rated (0/20 top-20
    overlap) → worth adding. **daily** also distinct (7/19 vs trending). **week**
    ≈ the existing `sort=ranking` trending → redundant.
  - `mode=actress` ranks PEOPLE (actresses), not products — different UI surface.
- Curated keyword lists work via `article=keyword/id=<N>/sort=ranking` (NOT
  `article=genre`): e.g. **Debut** id=6006 (new actresses' first works), 4HR+ id=6012.
- Rejected facets: `list_type=reserve` (≈ newest), `format=4kuhd` (only ~20 items).
