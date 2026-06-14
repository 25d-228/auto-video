# javdb mobile API — captured map (2026-06-12)

Reverse-engineered by capturing the official javdb Android app (`xxx.pornhub.fuck`, v1.9.35)
in an emulator via Frida (cert-unpinning + connect-redirect) → mitmproxy. This is the authoritative
endpoint list for the app's backend; it supersedes the guesses in `sidecar/av_proxy.py`.

## Host & auth
- **Base host:** `https://apidd.spthgb.com`  ← the live host. (Our sidecar uses `jdforrepam.com`, which is a
  rotated-out alias. The host string rotates; treat it as configurable.)
- **Auth header:** `jdsignature: <unix_ts>.lpw6vgqzsp.<md5(ts + SECRET)>` — the SAME scheme our sidecar already
  implements (`jdb_signature()`); it works verbatim against `apidd.spthgb.com`.
- **Headers:** `User-Agent: Dart/3.5 (dart:io)`, `accept-language: en`, `accept-encoding: gzip`.
- Response envelope: `{"success":1,"action":null,"message":null,"data":{...}}`. `success:0` / `action:"JWTVerificationError"`
  on auth-required endpoints.

## Listing / browse endpoints (no login needed)
- `GET /api/v1/movies/latest?type=all&filter_by=can_play&sort_by=update&page=1&limit=9` — newest releases.
- `GET /api/v1/movies/recommend?period=-1` — home recommendations.
- `GET /api/v1/movies/top?start_rank=1&type=all&type_value=&ignore_watched=false&page=1&limit=25` — TOP250.
- `GET /api/v1/rankings?type=0&period=daily` — rankings. **`type` 0=Censored · 1=Uncensored · 2=Western · 3=FC2** (verified live; invalid/extra types fall back to 0). `period` = daily|weekly|monthly (yearly/year silently fall back to daily). Used for **Adult→JavDB = the Censored ranking (type=0)** per window.
- `GET /api/v1/rankings/playback?filter_by=all&period=daily` — **"Most Viewed" / HotWatching** (most-played). `filter_by` = all|high_score; `period` = daily|weekly|monthly. **Verified working.**
- `GET /api/v1/movies/tags?filter_by=<SEL>&filter_by_tags=&sort_by=release&order_by=desc&page=1&limit=24` — the
  filtered movie browser (Categories). See the `filter_by` selector below.

### The `filter_by` selector (colon-delimited) — 7 fields, verified live
Format `<cat>:t:m:<genre>:<year>:<?>:<month>` (split on `:` → 7 fields):
- field 0 `<cat>` = the **Categories tab**: `0`=Censored · `1`=Uncensored · `2`=Western · `3`=FC2 (Carton too). VR genre only exists under Censored (`0`).
- fields 1–2 `t:m` = constant prefix (tag / movie).
- field 3 `<genre>` = a Genre tag id (e.g. **VR=212**, Solowork=28, Classic=175 …).
- field 4 `<year>` = a Year tag (`2001`–`2026`); empty = all years.
- field 5 = unused/other group (left empty).
- field 6 `<month>` = a Month tag (`1`–`12`); empty = all months.
- Default / all: `0:t:m::::` · by actor: `0:a:<actorSlug>`.
- Examples (all verified): VR newest = `0:t:m:212:::` · VR 2024 = `0:t:m:212:2024::` · **VR 2024 June = `0:t:m:212:2024::6`** · VR June (any year) = `0:t:m:212:::6`.
- **`sort_by` tokens** (verified distinct): `release` (+`order_by=desc|asc`), `update`, `score`, `hit`, `want_watch_count`, `watched_count`. (`views`/`want`/`watched` are ignored — use the exact tokens above.) Paginate with `page` + `limit`.
- **Adult→VR→JavDB** uses this: Categories→Censored + Genre VR, with year/month/sort selectors → `0:t:m:212:<year>::<month>`.

## Tag taxonomy
- `GET /api/v2/tags?type=0` → groups with `category_id` + `tags[{id,name}]`:
  `main` (p=Playable, m=Downloadable, c=Has subtitles, s=Individual, i=preview images, v=preview video),
  `year` (2001–2026), `month` (1–12), `subject`, `role`, `cloth`, `body`, `behavior`, `play_method`,
  `category` (28=Solowork, 80=Debut, 175=Classic, 178=Love, **212=VR**, 213=Fan Appreciation, 236=For Women), `duration`.

## Detail / magnets
- `GET /api/v4/movies/<slug>?from_rankings=false` — full detail. `<slug>` (e.g. `r3z2Kz`) is the `id` field
  returned in listings AND the public web permalink (`https://javdb.com/v/<slug>`). Detail carries
  watched_count, want_watch_count, reviews_count, score, tags, etc.
- `GET /api/v1/movies/<slug>/magnets` — magnet links for a title.

## Actors
- `GET /api/v1/actors/recommend`
- `GET /api/v1/actors/<slug>?from_rankings=false` — actor page; their movies via `/api/v1/movies/tags?filter_by=0:a:<slug>`.

## Misc
- `GET /api/v1/startup?last_ad_id=&platform=android&app_channel=official&app_version=official&app_version_number=1.9.35`
- `GET /api/v1/ads` ; `POST /api/v2/logs/activated`
- **Covers:** `https://tp.cmastd.com/<path>/small_covers/<xx>/<id>.jpg` — these RENDER fine (contrary to the
  earlier "cmastd unrenderable" note); usable directly for thumbnails.

## Login-gated (NOT captured — needs account credentials)
- `GET /api/v1/lists` → `JWTVerificationError` without a logged-in JWT. Likely backs user lists / watch-later /
  account-scoped "most viewed by me". To capture: log into the app in the emulator and re-run the Frida capture.

## How this was captured (repro)
Emulator (API 33, rootable arm64) + mitmproxy CA as a system cert + `frida-server`; launch the app under the
HTTPToolkit `frida-interception-and-unpinning` scripts (`config.js` PROXY_HOST=10.0.2.2 PROXY_PORT=8083, our CA);
the connect-hook redirects Flutter's traffic (which ignores the system proxy) to mitmproxy in **regular mode**
(the app speaks HTTP CONNECT). Drive the UI; mitmproxy logs every request.
