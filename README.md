# Auto-Video

Auto-Video is a Tauri 2 desktop application for macOS and Windows with a React and TypeScript interface built by Vite. It scans configured Movies and VR folders, shows their volume storage usage, groups safely identified VR files by canonical product code, and opens or reveals exact trusted files. It also discovers and searches TMDB Movies with exact-ID details and finds JavDB VR titles by exact product code. Verified Sukebei releases can be compared, inspected as exact torrent metainfo, and saved as `.torrent` files. After configuring a VR folder, selected files can be started, paused, resumed, cancelled without deleting partial data, and resumed safely after relaunch. Completed transfers in the current VR folder can be previewed and explicitly confirmed for safe canonical organization. Organized and recoverable partial results persist across relaunch, while dismissing their terminal rows leaves every media file untouched. Settings provides one persisted aggregate download limit for all VR transfers, while Downloads and Dashboard summarize current native transfer activity and aggregate speed. Automatic VR organization, organization templates, and destructive VR actions are not available.

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
