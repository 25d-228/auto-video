# Auto-Video

Auto-Video is a Tauri 2 desktop application for macOS and Windows with a React and TypeScript interface built by Vite. It scans configured Movies, TV, Adult, and VR folders and shows their volume storage usage. TV files with one conservative `S01E02` or `1x02` identity are grouped by exact show title; ambiguous files remain visible and unassociated. Adult and VR files are grouped only when one canonical product code is established safely. Adult files with ambiguous identities remain visible under their complete filename-derived titles, and exact multipart labels are shown only when one existing Part, CD, Disc, or Disk marker is unambiguous. Open and Reveal accept only exact files from the latest trusted scan. Movie files and grouped TV shows can be explicitly associated with manually selected, IMDb-verified TMDB metadata without changing local files or episode identities. Auto-Video also discovers and searches TMDB Movies and TV with exact-ID details and season guides, and finds JavDB Adult and VR titles by exact product code. Exact IMDb-verified YTS Movie torrents and product-code-verified Sukebei Adult and VR torrents can be compared, inspected as bounded torrent metainfo, and saved as `.torrent` files. An exact IMDb-verified API Bay TV release can also be inspected and saved as Auto-Video-generated verified metainfo. After configuring the relevant Movies, TV, Adult, or VR folder, explicitly selected files can be started, paused, resumed, cancelled without deleting partial data, and resumed safely after relaunch. Completed Movie, TV, Adult, and VR transfers in their current category folder can be previewed and explicitly confirmed for safe canonical organization. Organized and recoverable partial results persist across relaunch, while dismissing any terminal transfer row leaves every media file untouched. Settings provides one persisted aggregate download limit for current Movie, TV, Adult, and VR transfers, while Downloads and Dashboard summarize their shared native activity and aggregate speed. Exact current Movie, TV, Adult, and VR files can be moved to Finder Trash or the Windows Recycle Bin only after explicit confirmation. Automatic organization, organization templates, playback, automatic metadata lookup, bulk metadata matching, episode invention, and air schedules are not available.

## Toolchain

Tauri 2 provides the maintained macOS and Windows native shell. React 19, TypeScript 6, and Vite 8 provide the interface foundation. The interface uses the shadcn/ui Base UI Mira preset and its semantic tokens. The native selected-file transfer engine uses librqbit with upload disabled so completed downloads do not seed.

## Visual preset

The approved shadcn/ui preset is `b3wnVLs40m`. Base UI is the single component base.

| Setting | Value |
|---|---|
| Style | Mira |
| Base color | Neutral |
| Theme | Red |
| Chart color | Rose |
| Heading | Geist Mono |
| Font | Inter |
| Icon library | Phosphor Icons |
| Radius | None |
| Menu | Default (solid) |
| Menu accent | Subtle |

## TV Library TMDB show metadata matching

Each conservatively grouped TV Library show exposes **Match show metadata** only as an explicit action. Opening it suggests the complete exact local show title in an editable query, but it does not contact TMDB or select a result. Search uses the existing native-stored TMDB token. The user must choose one positive TMDB TV candidate manually; native code then verifies matching details and external-ID responses for that exact show ID and requires one canonical IMDb series ID before **Save show metadata match** is available. Unassociated files cannot enter this workflow.

The durable show-level association is bound to the current canonical TV folder, exact parsed local show title, and a nonempty set of trusted member anchors. A changed or replaced anchor invalidates it. Genuinely missing anchors are reconciled, and new local members may join the local group without gaining invented TMDB episode identity. Accepted text metadata remains available after relaunch and while TMDB is offline. **Clear show metadata match** removes only the accepted metadata; it does not change credentials, episodes, media files, transfers, organization or recovery records, cleanup, or Trash state.

The accepted TMDB name becomes the card's primary title for display, copy, search, sorting, and pagination while the exact local title and current members remain visible in details and searchable. This Library action is separate from Discover and its provider season and episode guides, local filename parsing, release comparison, generated-metainfo Save, downloads, organization, Open, Reveal, and per-member Trash. Matching is never automatic or bulk, and it never authorizes a file action.

## TMDB TV season guides

**View details** on a TMDB TV card verifies the exact positive show ID before exposing provider season summaries. Only seasons with a positive provider season ID and positive season number are shown; specials at season zero and malformed identities are excluded, while duplicate season identities fail safely.

**View seasons** does not request episodes. Selecting one verified season requests only that exact show ID, provider season ID, and season number. Auto-Video accepts a completed guide only when the response and every episode retain those identities. Duplicate, cross-season, missing, and malformed episode identities fail safely. Exact provider names and valid optional dates, runtimes, overviews, posters, and stills are preserved without inventing episodes from provider totals or local filenames. The guide does not inspect torrents, start downloads, enrich the local TV Library, rename media, or provide playback or air schedules.

## TV episode release comparison

**Find releases** on a completed positive episode first resolves the exact TMDB show to a canonical IMDb series identity with the native-stored TMDB token. Native code rebuilds the trusted show, season, and episode context from their positive provider IDs, then queries API Bay with the exact provider show name and canonical `SxxExx` marker in only TV Shows and HD TV Shows. The interface cannot supply a provider URL, raw query, category, IMDb identifier, show name, season number, or episode number.

Only rows with the exact IMDb series identity, an approved TV category, a positive unique item ID, a canonical infohash, and exactly one unambiguous matching `SxxExx` or `NxNN` episode marker contribute to counts and comparison. Raw canonical item IDs and infohashes are checked independently across both category responses before other identity filters can discard a conflict. Wrong, missing, extended, embedded, pack, multipart, continuation, conflicting, malformed, and duplicate identities are excluded or fail safely. Exact accepted names and available provider metadata are preserved. No request occurs before the explicit action, no release is selected automatically, and a no-match result does not fall back to another show or episode.

A manually selected accepted row exposes **Inspect torrent**. Native code retains the complete TMDB show, season, episode, IMDb series, API Bay item, category, exact release name, and lowercase v1 infohash chain, then uses the existing shared librqbit session to retrieve only that infohash's metadata. Inspection accepts only strict BitTorrent v1 metainfo whose exact encoded `info` value matches the accepted hash, and presents the complete exact torrent name and ordered file list without allocating media or creating a Downloads row.

Every inspected file begins unselected. **Start download** requires at least one explicit file selection and uses the current native-persisted TV folder; the interface sends only the current verified TV inspection identity and selected file identities. Native code reparses the exact generated metainfo and verifies the complete show, season, episode, IMDb, API Bay item, category, release-name, infohash, selected-file, destination, and transfer identity before the shared torrent engine can start. It writes only selected content and retains boundary-piece data without creating deselected placeholders. Active TV transfers use the shared aggregate limit, Downloads snapshot, Pause, Resume, non-destructive Cancel, durable Dismiss, and restart recovery lifecycle. Only a newly durable completion in the current TV folder triggers one TV Library and TV storage refresh; old-folder, cancelled, failed, offline, and recovery rows do not.

API Bay supplies the accepted infohash, not an original `.torrent` artifact. Auto-Video generates the verified metainfo container from retrieved exact-infohash metadata. **Save generated metainfo** writes only those exact cached verified bytes through the native destination dialog without refetching or overwriting. Save remains independent from Start. Trash from a transfer, automatic cleanup, and partial-file cleanup remain unavailable; permanent cleanup is a separate action available only after a transfer is durably cancelled.

## TV episode organization

An exact durable completed TV transfer in the current TV folder exposes **Organize files** only when every selected media file has a safe destination that parses back to the persisted show, season, and episode through the native TV Library contract. Native code rebuilds the plan from the complete persisted identity, metainfo, selected files, destination, fingerprints, current paths, and retained boundary pieces. One selected media file uses `Exact Show/Season NN/Exact Show - SxxExx - Exact Episode.ext`, preserving extension casing. Two or more selected media files retain every exact basename and extension under `Exact Show/Season NN/`; one ambiguous, unsafe, conflicting, or non-round-trippable media member rejects the complete plan. Selected non-media files stay at their existing paths.

Preview never mutates files. Apply revalidates the complete native plan and moves without overwriting, crossing the TV folder or volume, following symlinks, or silently changing an unsafe or over-limit name. Failures restore original paths when possible or persist exact moved and unmoved paths as **Organization needs attention** for restart recovery. Dismiss removes only the exact transfer and recovery metadata and never changes media. Durable success refreshes only the TV Library and TV storage once; a later reconciliation failure leaves the organized state truthful and retryable. Organization is never automatic, and it does not invent episode, part, cut, sample, edition, or ordering labels.

## TV Library Trash

Each exact grouped episode or unassociated TV file has its own **Move to Trash** action. Auto-Video requires an accessible confirmation, then native code revalidates the current configured TV folder, latest trusted scan generation, exact scanned path, supported regular-file type, unchanged file fingerprint, canonical containment, absence of symlink components, and absence of current transfer, organization-plan, or durable-recovery ownership across Movie, TV, Adult, and VR records. Missing or unreadable ownership state fails closed without dispatch. macOS uses Finder Trash behavior and Windows uses the Recycle Bin; there is no permanent-delete fallback.

After an accepted move, only that member is removed and the affected show is regrouped. Complete TV Dashboard totals, search, sorting, pagination, and TV storage reconcile without refreshing other media categories. If a later scan or storage refresh fails, the successful file move remains truthful and the interface offers a reconciliation retry. This action never removes a whole show or directory, performs bulk or automatic cleanup, deletes transfer data, organizes or renames TV media, or restores files from Trash or the Recycle Bin.

## Adult Library Trash

Each exact member of a grouped Adult title and each unassociated Adult file has its own **Move to Trash** action. Auto-Video requires an accessible confirmation, then native code revalidates the configured Adult folder, latest trusted scan generation, exact scanned path, supported regular-file type, unchanged file fingerprint, canonical containment, and absence of symlink components. macOS uses Finder Trash behavior and Windows uses the Recycle Bin; there is no permanent-delete fallback.

After an accepted move, only that file is removed and the affected product-code group is reconciled while preserving exact multipart labels and ambiguous basenames. Complete Adult Dashboard totals, search, sorting, pagination, and Adult storage reconcile without refreshing other media categories. If a later scan or storage refresh fails, the successful file move remains truthful and the interface offers a reconciliation retry. This action never removes a whole product-code group or directory, performs bulk or automatic cleanup, deletes transfer data, organizes or renames Adult media, changes provider behavior, or restores files from Trash or the Recycle Bin.

## VR Library Trash

Each exact member of a grouped VR title and each unassociated VR file has its own **Move to Trash** action. Auto-Video requires an accessible confirmation, then native code revalidates the configured VR folder, latest trusted scan generation, exact scanned path, supported regular-file type, unchanged file fingerprint, canonical containment, absence of symlink components, and absence of current transfer, organization-plan, or durable-recovery ownership across Movie, TV, Adult, and VR records. Missing or unreadable ownership state fails closed without dispatch. macOS uses Finder Trash behavior and Windows uses the Recycle Bin; there is no permanent-delete fallback.

After an accepted move, only that file is removed and the affected product-code group is reconciled while preserving exact Part, PT, CD, Disc, and Disk labels and ambiguous basenames. Complete VR Dashboard totals, search, sorting, pagination, and VR storage reconcile without refreshing other media categories. If a later scan or storage refresh fails, the successful file move remains truthful and the interface offers a reconciliation retry. This action never removes a whole VR title or directory and never changes transfers, partial files, organization, recovery, providers, limits, or another category.

## Movie Library TMDB metadata matching

Each Movie Library file exposes **Match metadata** only as an explicit action. Opening it suggests the exact filename-derived title in an editable query, but it does not contact TMDB, select a result, or change the file. Search uses the native-stored TMDB token. The user must choose one candidate manually, after which native code verifies the exact positive TMDB Movie ID through its details and external-ID responses and requires one canonical IMDb Movie ID before **Save metadata match** becomes available.

The durable association binds the current configured Movies folder identity, relative path, exact regular-file identity, size, trusted scan generation, TMDB Movie ID, and IMDb Movie ID. A moved, changed, replaced, stale, symlinked, or cross-folder file cannot inherit it. Corrupt, oversized, duplicate, or conflicting association data fails closed while local Movie cards and exact file actions remain available. A saved association can be shown after relaunch without a provider request; **Clear metadata match** removes only that metadata and never opens, reveals, moves, renames, trashes, or deletes the file.

Accepted TMDB titles and dates become the Library card's primary display, search, sort, pagination, and copy-title values while the filename remains searchable and visible in details. Provider and local-file information are labeled separately, and a failed poster, offline provider, or failed metadata request does not hide the local Movie or affect Open, Reveal, Trash, Dashboard totals, storage, organization, transfers, cleanup, or another category. Matching is never automatic or bulk.

## Movie torrent comparison, inspection, and selected-file downloads

**Find releases** on a Movie card or its exact-ID details resolves that TMDB Movie to its IMDb identity through the native-stored TMDB token, then queries YTS by the literal IMDb identifier. Only a YTS Movie object with that exact IMDb identity contributes rows; missing, wrong, unrelated, malformed, and conflicting identities are excluded or fail safely. No request is made before the explicit action, and no release is selected automatically.

A manually selected row exposes **Inspect torrent** only when it contains a complete YTS URL and expected BitTorrent v1 infohash. Native code binds the exact TMDB, IMDb, YTS Movie, row, URL, and hash context before fetching the bounded artifact. Inspection verifies the infohash and shows the complete torrent name, size, and file list.

Every inspected file begins unselected. **Start download** requires at least one explicit file selection and uses the native-persisted Movies folder; the interface sends only the current Movie inspection identity and selected file identities. Native code reparses the exact cached metainfo, verifies the complete TMDB, IMDb, YTS, torrent-row, infohash, and file identity, rejects unsafe or conflicting targets, and writes only selected content. Tracker, peer, and DHT activity can begin only after Start. Valid active Movie transfers retain their immutable destination and selected boundary-piece data across relaunch. A completion in the current Movies folder triggers one bounded Movies Library and storage refresh; a transfer targeting an older folder cannot replace current results.

Completed verified Movie transfers expose **Organize files** only in the currently configured Movies folder. The preview uses the exact TMDB title and the year from its complete release date as `TITLE (YYYY)`. A single media file becomes `TITLE (YYYY).ext`; multiple media files retain their exact basenames, and non-media files remain unchanged. Apply revalidates the persisted Movie and torrent identity, selected files, current folder, sources, and destinations before moving without overwriting. Failures restore the original paths or persist an Attention state with exact moved and unmoved paths for restart recovery. Unsafe titles are rejected rather than changed, and organization never crosses the configured Movies folder or volume.

**Save `.torrent`** remains independent and writes the exact cached verified bytes through the native destination dialog without refetching or overwriting an existing file. Movie downloads do not select a release, torrent, file, or organization plan automatically, seed intentionally, or delete or move media on Cancel or Dismiss.

## Adult torrent inspection and selected-file downloads

Discover accepts one exact Adult product code and compares only identity-verified Sukebei releases. A release exposes **Inspect torrent** only when its provider view, torrent URL, positive item ID, and expected BitTorrent v1 infohash form one complete same-item identity. Inspection verifies the exact fetched metainfo before showing its torrent name, lowercase infohash, total size, and complete file list.

Every inspected file begins unselected. **Start download** requires at least one explicit file selection and uses the native-persisted Adult folder; the interface sends only the current Adult inspection identity and selected file identities. Native code reparses the cached metainfo, verifies its infohash and exact files, rejects unsafe or conflicting targets, and writes only selected content. Inspection and selection do not contact trackers, peers, or DHT.

Movie, TV, Adult, and VR transfers share the existing Downloads session, polling snapshot, Dashboard summary, and persisted aggregate limit. Each row shows its category and exact release identity. Pause, resume, confirmed cancellation, and terminal dismissal affect only that transfer and retain media and partial data. Valid active transfers revalidate their category, identity, torrent, selected files, immutable destination, partial files, and retained boundary-piece bytes before restart resume. Completed or failed state is shown only after its exact terminal authority is durable. If the primary Downloads file cannot be updated, a bounded recovery record reloads as a non-running, non-organizable attention row without changing media or retained boundary-piece data. Dismiss removes only that transfer's durable records and remains retryable after a persistence failure. A completed transfer in its current category folder triggers one bounded matching Library and storage refresh only after durable completion; a transfer for an older folder does not replace current results.

## Cancelled transfer cleanup

**Cancel download** keeps every selected file, partial byte, and retained boundary piece on both macOS and Windows. **Dismiss** removes terminal metadata only and never deletes media. Library **Move to Trash** remains a separate member action that uses Finder Trash or the Windows Recycle Bin.

On macOS and Windows, an exact durably cancelled Movie, TV, Adult, or VR row can expose **Permanently clean transfer files**. After a second accessible confirmation lists every selected relative path, the renderer sends only the transfer ID. Native state revalidates the exact persisted transfer, selected media and non-media paths, fingerprints, containment, symlink and directory boundaries, and all transfer, organization, and recovery ownership before it writes cleanup authority. Windows deletion is bound to the validated file handle. macOS uses a bounded durable mutation record whose exact staging identity and phase are persisted before each pathname change, then reconciles that record after interruption. Neither platform uses Finder Trash or the Recycle Bin. Deselected, unrelated, replaced, cross-owned, Library-only, and organization- or recovery-owned files are not deleted.

Each completed file is recorded durably so an interrupted cleanup reloads as a stopped attention row and retries only exact remaining files. On macOS, every app-owned staging object remains distinguishable from a replacement after restart and is removed before cleanup authority can disappear. One cleanup can run at a time. The row disappears only after cleanup completion and durable record removal; only a matching current category then refreshes its Library and storage.

Completed Adult transfers expose **Organize files** only while they remain associated with the currently configured Adult folder. The native preview leaves non-media files unchanged and proposes every eligible media move into the exact product-code directory. A single media file becomes `CODE.ext`; multipart files retain one exact existing Part, CD, Disc, or Disk label, while ambiguous multipart names retain their exact basename. Apply revalidates the complete plan and moves without overwriting. Failures restore the original paths or persist an Attention state with the exact moved and unmoved paths for restart recovery. Organization is never automatic, and dismissing a terminal row never moves or deletes media.

**Save `.torrent`** still writes the exact bytes from the current verified inspection through the native destination dialog without refetching or overwriting an existing file. It is independent from **Start download**. No release, torrent, file, transfer, or organization plan is selected, started, or applied automatically. Automatic cleanup, arbitrary torrent or magnet import, intentional seeding, per-transfer limits, priorities, schedules, organization templates, and bulk controls are not included.

## Requirements

- Node.js 24.17.0 and npm 11.13.0
- Rust 1.96.0 with `clippy` and `rustfmt`
- macOS: Xcode Command Line Tools
- Windows: Microsoft C++ Build Tools and Microsoft Edge WebView2

Node.js is pinned in `.nvmrc`, npm is pinned in `package.json`, and Rust is pinned in `rust-toolchain.toml`. The platform prerequisites are detailed in the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Setup

Install the locked dependencies from a fresh checkout:

```sh
npm ci
```

## Commands

Launch the desktop application in development mode:

```sh
npm run dev
```

Run the TypeScript, React, formatting, and Rust lints:

```sh
npm run lint
```

Run the interface tests:

```sh
npm test
```

Run the native scanner tests:

```sh
npm run test:native
```

Create the production web assets:

```sh
npm run build
```

Compile the production native application without creating an installer or distribution bundle:

```sh
npm run build:native
```
