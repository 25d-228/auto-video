# Auto-Video

Auto-Video is a Tauri 2 desktop application for macOS and Windows with a React and TypeScript interface built by Vite. It scans configured Movies, TV, Adult, and VR folders and shows their volume storage usage. TV files with one conservative `S01E02` or `1x02` identity are grouped by exact show title; ambiguous files remain visible and unassociated. Adult and VR files are grouped only when one canonical product code is established safely. Adult files with ambiguous identities remain visible under their complete filename-derived titles, and exact multipart labels are shown only when one existing Part, CD, Disc, or Disk marker is unambiguous. Open and Reveal accept only exact files from the latest trusted TV, Adult, or VR scan. Auto-Video also discovers and searches TMDB Movies and TV with exact-ID details and finds JavDB Adult and VR titles by exact product code. Exact verified Sukebei releases can be compared, inspected as bounded torrent metainfo, and saved as `.torrent` files for Adult and VR titles. After configuring the relevant Adult or VR folder, explicitly selected files can be started, paused, resumed, cancelled without deleting partial data, and resumed safely after relaunch. Completed Adult and VR transfers in their current category folder can be previewed and explicitly confirmed for safe canonical organization. Organized and recoverable partial results persist across relaunch, while dismissing any terminal transfer row leaves every media file untouched. Settings provides one persisted aggregate download limit for current Adult and VR transfers, while Downloads and Dashboard summarize their shared native activity and aggregate speed. Local metadata enrichment and destructive file actions are not available. TMDB TV season or episode guides, air schedules, playback, local-library enrichment, automatic metadata lookup, episode invention, media renaming, and destructive TV actions are not available. Automatic Adult or VR organization, organization templates, and destructive Adult or VR actions are not available.

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

## Adult torrent inspection and selected-file downloads

Discover accepts one exact Adult product code and compares only identity-verified Sukebei releases. A release exposes **Inspect torrent** only when its provider view, torrent URL, positive item ID, and expected BitTorrent v1 infohash form one complete same-item identity. Inspection verifies the exact fetched metainfo before showing its torrent name, lowercase infohash, total size, and complete file list.

Every inspected file begins unselected. **Start download** requires at least one explicit file selection and uses the native-persisted Adult folder; the interface sends only the current Adult inspection identity and selected file identities. Native code reparses the cached metainfo, verifies its infohash and exact files, rejects unsafe or conflicting targets, and writes only selected content. Inspection and selection do not contact trackers, peers, or DHT.

Adult and VR transfers share the existing Downloads session, polling snapshot, Dashboard summary, and persisted aggregate limit. Each row shows its category and exact release identity. Pause, resume, confirmed cancellation, and terminal dismissal affect only that transfer and retain media and partial data. Valid active Adult transfers revalidate their category, torrent, selected files, immutable destination, partial files, and retained boundary-piece bytes before restart resume. A completed transfer in the current Adult folder triggers one bounded Adult Library and storage refresh; a transfer for an older folder does not replace current results.

Completed Adult transfers expose **Organize files** only while they remain associated with the currently configured Adult folder. The native preview leaves non-media files unchanged and proposes every eligible media move into the exact product-code directory. A single media file becomes `CODE.ext`; multipart files retain one exact existing Part, CD, Disc, or Disk label, while ambiguous multipart names retain their exact basename. Apply revalidates the complete plan and moves without overwriting. Failures restore the original paths or persist an Attention state with the exact moved and unmoved paths for restart recovery. Organization is never automatic, and dismissing a terminal row never moves or deletes media.

**Save `.torrent`** still writes the exact bytes from the current verified inspection through the native destination dialog without refetching or overwriting an existing file. It is independent from **Start download**. No release, torrent, file, transfer, or organization plan is selected, started, or applied automatically. Destructive cleanup, cancel-and-delete, arbitrary torrent or magnet import, intentional seeding, per-transfer limits, priorities, schedules, organization templates, and bulk controls are not included.

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
