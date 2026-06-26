# Library cover stabilization (2026-06-12)

Goal: make the Movies/TV/Adult/VR library resolve covers stably, with permission to
rename/move files. No files were deleted. Every move is logged to
`~/auto-video-library-moves.tsv` and is reversible.

## TL;DR
Cover hit-rate against the real library: 744 / 746 = 99.7%.

| Category | Folder | Items | Covers |
|---|---|---|---|
| Movies | `/Volumes/Be/films` | 76 | 76 (100%) |
| TV | `/Volumes/Be/series` | 11 | 11 (100%) |
| Adult | `/Volumes/H/porn` | 507 | 505 (99.6%) |
| VR | `/Volumes/Be/vr` | 152 | 152 (100%) |

The 2 misses are TEN-048 and TEN-055, not present on any cover source I query (DMM, r18,
MGStage, javdatabase). They show a clean title placeholder. See "Remaining" below.

## What was actually wrong
The files were less chaotic than expected; names were mostly good (`YEAR.Title`, show
folders, JAV codes). The cover failures were in the code, not the file names:

1. macOS NFD filenames (the big one). macOS stores names decomposed, so Japanese voiced
   kana (゛゜) arrive as separate combining marks. The cover matcher stripped those marks,
   turning "ゴ"→"コ" and breaking the TMDB match for ~every anime with voiced kana (Cowboy
   Bebop, Tokyo Godfathers, Summer Wars, Perfect Blue...). Fixed by composing to NFC before
   matching (`norm()` in `tmdb.ts`, and titles in `library.ts`).
2. Movies-in-folders / TV episodes. The scanner read the gnarly inner release filename
   instead of the clean `YEAR.Title` folder, and listed every TV episode as its own item.
   Fixed with structure-aware grouping (`buildItems`): one item per top-level entry for
   mov/tv, identifier from the clean folder name; episodes collapse into one show.
3. Multi-part JAV files (`ABP-601-A/-B`, `AJVR-...-A/-B/-C`) became duplicate items. Now
   merged into one item per code.
4. Romanized-only TMDB titles (パーフェクトブルー is listed only as "PERFECT BLUE").
   `pickMatch` now accepts the top hit when its year matches, even if the title won't
   string-match.
5. Over-padded JAV codes (`MIVR-00081` vs canonical `MIVR-081`). `cover()` retries once with
   the 3-digit-normalized code (`normalizeCodeNum`). Fallback-only, so nothing that already
   resolved can regress.

## Files moved (reversible, see `~/auto-video-library-moves.tsv`)
Only one relocation was needed: two anime films were misfiled under series.
- `series/ジーニアス・パーティ/Genius.Party.mp4` → `films/2007.Genius Party.mp4`
- `series/ジーニアス・パーティ/…Genius.Party.Beyond….mp4` → `films/2008.Genius Party Beyond.mp4`

Both now resolve covers. The leftover `ジーニアス・パーティ` folder has only a stray `.jpg`
(no video), so it no longer appears in the library. To undo, move them back per the manifest.

## Remaining (2 items, not an algorithm bug)
`TEN-048`, `TEN-055` aren't on any cover source I query. `javbus` is the one source I don't
query, because no javbus cookie is configured; adding one in Settings may resolve them.
Otherwise they're upstream gaps (no cover exists to fetch).

## Not touched (on purpose)
Junk that doesn't affect covers was left alone (the scanner already ignores it): macOS `._*`
sidecars, `.torrent`/`.js` client metadata, external `.flac`/`.ass`/fonts. These weren't
deleted or moved; they can be quarantined into a folder on request.

## Naming convention (the contract the static parser relies on)
The point of clean names is that a dumb static parser (no LLM) can fetch covers/info
deterministically. The identifier must be in the name/folder; a static parser cannot invent a
dropped maker prefix or a missing year.

| Category | Folder | Name a file/folder like | Static parse → source |
|---|---|---|---|
| Movies | `films/` | `YEAR.Title.ext` (year first), or a `YEAR.Title/` folder | `parseMovieEntry` → TMDB |
| TV | `series/` | `Show Name/` folder, episodes inside | folder name → TMDB/tvmaze/AniList |
| Adult | `porn/` | `<CANONICAL-CODE>.ext` (flat) | `parseCode` → DMM/MGStage/javdb cascade |
| VR | `vr/` | `<CANONICAL-CODE>[-A/-B/-C].ext` | `parseCode` → cascade |

Examples: `1990.GoodFellas.mkv`, `2007.Genius Party.mp4`, `series/カウボーイビバップ/…`,
`ABF-032.mp4`, `459TEN-048.mp4`, `AJVR-00277-A.mp4`.

What the static parser tolerates (no rename needed):
- Over-padded codes: `MIVR-00081` is auto-normalized to `MIVR-081`.
- Multi-part: `-A/-B/-C` files merge into one item.
- Japanese NFD names: composed to NFC automatically.
- Junk (sidecars/subs/torrents/fonts): ignored by the scanner.

What requires a rename (a static parser can't fix it):
- Dropped maker prefix: `TEN-048` must be `459TEN-048` (the `459` is part of the canonical
  product number; only a lookup can recover it).
- Missing year on a movie, or a film misfiled under series.
- Wrong/garbled title that doesn't match the real release.

So when an item won't resolve, look up its canonical id (FANZA/MGStage/av-wiki for JAV; TMDB
for film/TV) and rename the file to match the convention above. Then the static parser handles
it forever after, no LLM.

## Cast (出演) in Japanese
The JAV detail panels show 出演/cast; it was coming back romaji because covers are now `blob:`
URLs, so the FANZA cid (which `r18.dev` needs for Japanese names) could never be derived from
the cover. Now `metaLookup` resolves Japanese cast through a cascade, preferring Japanese at
every step:
1. `r18.dev` by cid (when one is known) → `name_kanji`.
2. javdatabase page → also yields a FANZA cid → `r18.dev` → Japanese.
3. javdb mobile API (`/api/v2/search` → slug → detail → `actors`): native Japanese names, also
   tried with the canonical code (`AJVR-00277`→`AJVR-277`). Covers amateur/VR titles the others
   miss.

Romaji is kept only as a last-resort fallback (display uses `cast_ja || cast`). Examples:
SSIS-001 → 葵つかさ・乙白さやか, MIVR-081 → 小花のん・希代あみ, ABF-032 → 北川あみ...,
AJVR-00277 → 胡桃さくら, etc. The only JAV titles with no Japanese cast are amateur labels
absent from all sources (e.g. 459TEN; cover works, cast blank). (Movie/TV cast stays as TMDB
provides it; that's a separate "Cast" section, not 出演.)

## How this was verified
A live harness (`tests/live/library-covers.live.test.ts`, `RUN_LIVE=1`) walks the real folders,
runs the exact scan+parse+resolve the app uses, and reports per-item hits. Run across several
samples plus two full passes, converging to 99.7%. Unit suite: 350 pass. The app was rebuilt
with all changes and relaunched.
