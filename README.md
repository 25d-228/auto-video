# Auto-Video

Auto-Video is being rebuilt as a Tauri 2 desktop application with a React and TypeScript interface built by Vite. This foundation opens one native window on macOS and Windows and intentionally contains no product features.

## Toolchain

Tauri 2 provides the maintained macOS and Windows native shell. React 19, TypeScript 6, and Vite 8 provide a standard DOM and CSS interface that is compatible with shadcn/ui without adding the component system before the application-shell work.

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
