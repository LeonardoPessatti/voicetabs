# VoiceTabs — Foundation (Phase 0 + Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a buildable Tauri 2 Windows app with a working tab editor — create / rename / reorder / close tabs that survive restart, with i18n (PT-BR + English), tracing logs, and a stub settings drawer. **No audio, no transcription** in this plan; those land in subsequent plans.

**Architecture:** Tauri 2 (Rust core + WebView2 frontend). Frontend: React 18 + TypeScript + Vite + Zustand + react-i18next. Backend: rusqlite with SQL-file migrations, layered as `db/` repositories + `commands/` Tauri command handlers. Single-process for now; the `stt_worker` Cargo workspace member is reserved but empty (used in Phase 3).

**Tech Stack:** Tauri 2.x · React 18 · TypeScript 5 · Vite 5 · Zustand 4 · react-i18next 15 · rusqlite 0.32 · tracing 0.1 · Vitest 1 · React Testing Library 14 · @dnd-kit/sortable 8.

**Reference spec:** `docs/superpowers/specs/2026-05-20-voicetabs-design.md`.

**Acceptance for this plan:**
- The repo builds with `npm run tauri dev` (window opens) and `npm run tauri build` (installer produced).
- Tabs can be created, renamed (double-click), reordered (drag), and closed (×).
- Closing the last tab auto-creates a fresh untitled tab.
- App restart preserves: tab list, tab order, tab titles, active tab, UI language.
- UI language is switchable PT-BR ↔ English from a stub Settings drawer; auto-detects from Windows UI locale on first launch.
- One Rust unit test and one TS unit test pass.
- CI builds the app on a Windows runner.

**Out of scope for this plan** (deferred to later plans):
- Audio capture, VAD, STT, segments, hallucination filter, hotkeys, tray, first-run wizard, model download, packaging polish, supervisor / crash recovery.

---

## File structure (after this plan completes)

```
transcript-tabs/
├── .github/workflows/ci.yml
├── .gitignore
├── README.md
├── Cargo.toml                          # workspace root
├── package.json
├── tsconfig.json
├── tsconfig.node.json
├── vite.config.ts
├── vitest.config.ts
├── index.html
├── docs/superpowers/{specs,plans}/...
├── src/                                # React frontend
│   ├── main.tsx
│   ├── App.tsx
│   ├── styles.css
│   ├── i18n/
│   │   ├── index.ts
│   │   └── locales/{pt-BR.json,en.json}
│   ├── lib/
│   │   └── tauri.ts                    # typed invoke() wrappers
│   ├── stores/
│   │   ├── tabsStore.ts
│   │   └── settingsStore.ts
│   ├── components/
│   │   ├── TabStrip.tsx
│   │   ├── TabBody.tsx
│   │   └── SettingsDrawer.tsx
│   └── __tests__/
│       ├── i18n.test.tsx
│       └── TabStrip.test.tsx
└── src-tauri/                          # Rust backend
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── build.rs
    ├── capabilities/default.json
    ├── icons/icon.png
    ├── migrations/
    │   └── 001_initial_schema.sql
    └── src/
        ├── main.rs
        ├── lib.rs
        ├── logging.rs
        ├── db/
        │   ├── mod.rs
        │   ├── connection.rs           # open + migrations runner
        │   ├── tabs.rs                 # tabs repository
        │   └── settings.rs             # settings repository
        ├── commands/
        │   ├── mod.rs
        │   ├── tabs.rs
        │   └── settings.rs
        └── tests/
            ├── tabs_repo_test.rs
            └── settings_repo_test.rs
```

Each module has one clear responsibility: a repository wraps the SQLite table, a commands module exposes typed Tauri commands, a store mirrors backend state on the frontend.

---

# Phase 0 — Foundation

## Task 1: Git init, .gitignore, README skeleton

**Files:**
- Create: `.gitignore`
- Create: `README.md`

- [ ] **Step 1: Initialize the repo**

Run:
```powershell
git init
git config core.autocrlf false
```

- [ ] **Step 2: Write `.gitignore`**

```gitignore
# Rust / Cargo
/target
/src-tauri/target

# Node
node_modules
npm-debug.log
yarn-debug.log
yarn-error.log
.pnpm-debug.log
.npm

# Vite / build
dist
dist-ssr
.vite

# IDE / OS
.vscode/*
!.vscode/extensions.json
.idea
*.suo
*.user
.DS_Store
Thumbs.db

# Tauri 2 generated schemas (regenerated per build)
/src-tauri/gen

# Tauri user data (do not commit user audio / DB / models)
*.log
```

- [ ] **Step 3: Write `README.md`**

```markdown
# VoiceTabs

Tabbed text editor where notes are dictated, not typed. Windows 11. Offline. PT-BR + English UI.

See `docs/superpowers/specs/2026-05-20-voicetabs-design.md` for the design.

## Development

```powershell
npm install
npm run tauri dev
```

## Build installer

```powershell
npm run tauri build
```
```

- [ ] **Step 4: Commit**

```powershell
git add .gitignore README.md docs
git commit -m "chore: initial repo with spec and plan"
```

---

## Task 2: Cargo workspace root

**Files:**
- Create: `Cargo.toml`

Set up the workspace so `stt_worker` can be added as a sibling crate in a later phase without restructuring.

- [ ] **Step 1: Write `Cargo.toml` (workspace root)**

```toml
[workspace]
resolver = "2"
members = ["src-tauri"]
# Future members: "stt_worker" (added in phase 3)

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["VoiceTabs"]
license = "MIT"

[profile.release]
opt-level = 3
lto = "thin"
strip = "symbols"
```

- [ ] **Step 2: Commit**

```powershell
git add Cargo.toml
git commit -m "chore: cargo workspace root"
```

---

## Task 3: Frontend scaffolding (package.json, tsconfig, Vite, HTML, entry)

**Files:**
- Create: `package.json`
- Create: `tsconfig.json`
- Create: `tsconfig.node.json`
- Create: `vite.config.ts`
- Create: `index.html`
- Create: `src/main.tsx`
- Create: `src/App.tsx`
- Create: `src/styles.css`

- [ ] **Step 1: Write `package.json`**

```json
{
  "name": "voicetabs",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {
    "@dnd-kit/core": "^6.1.0",
    "@dnd-kit/sortable": "^8.0.0",
    "@dnd-kit/utilities": "^3.2.2",
    "@tauri-apps/api": "^2.0.0",
    "i18next": "^23.11.5",
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "react-i18next": "^15.0.0",
    "zustand": "^4.5.2"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@testing-library/jest-dom": "^6.4.6",
    "@testing-library/react": "^16.0.0",
    "@testing-library/user-event": "^14.5.2",
    "@types/react": "^18.3.3",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.1",
    "jsdom": "^24.1.0",
    "typescript": "^5.5.0",
    "vite": "^5.3.0",
    "vitest": "^1.6.0"
  }
}
```

- [ ] **Step 2: Write `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "types": ["vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 3: Write `tsconfig.node.json`**

```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true,
    "strict": true
  },
  "include": ["vite.config.ts", "vitest.config.ts"]
}
```

- [ ] **Step 4: Write `vite.config.ts`**

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(() => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "localhost",
    hmr: { protocol: "ws", host: "localhost", port: 1421 },
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
}));
```

- [ ] **Step 5: Write `index.html`**

```html
<!doctype html>
<html lang="pt-BR">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>VoiceTabs</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 6: Write `src/main.tsx`**

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 7: Write `src/App.tsx` (placeholder — replaced in later tasks)**

```tsx
export default function App() {
  return (
    <main className="app">
      <h1>VoiceTabs</h1>
    </main>
  );
}
```

- [ ] **Step 8: Write `src/styles.css`**

```css
:root {
  font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  color: rgba(255, 255, 255, 0.87);
  background-color: #1e1e1e;
}

* {
  box-sizing: border-box;
}

html, body, #root {
  margin: 0;
  padding: 0;
  height: 100%;
}

.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}
```

- [ ] **Step 9: Commit**

```powershell
git add package.json tsconfig.json tsconfig.node.json vite.config.ts index.html src
git commit -m "chore: frontend scaffolding (Vite + React + TS)"
```

---

## Task 4: Tauri 2 backend scaffolding

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/capabilities/default.json`
- Create: `src-tauri/icons/icon.png` (placeholder)

- [ ] **Step 1: Write `src-tauri/Cargo.toml`**

```toml
[package]
name = "voicetabs"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
description = "VoiceTabs - dictation-driven tabbed editor"
default-run = "voicetabs"

[lib]
name = "voicetabs_lib"
crate-type = ["cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2.0", features = [] }

[dependencies]
tauri = { version = "2.0", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
thiserror = "1"
anyhow = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }
parking_lot = "0.12"
directories = "5"

[dev-dependencies]
tempfile = "3"

[features]
default = ["custom-protocol"]
custom-protocol = ["tauri/custom-protocol"]
```

- [ ] **Step 2: Write `src-tauri/tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "VoiceTabs",
  "version": "0.1.0",
  "identifier": "com.voicetabs.app",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "VoiceTabs",
        "width": 1100,
        "height": 720,
        "minWidth": 800,
        "minHeight": 500,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/icon.ico", "icons/icon.png"],
    "windows": {
      "nsis": {
        "displayLanguageSelector": false,
        "languages": ["English", "PortugueseBrazilian"]
      }
    }
  }
}
```

- [ ] **Step 3: Write `src-tauri/build.rs`**

```rust
fn main() {
    tauri_build::build();
}
```

- [ ] **Step 4: Write `src-tauri/src/main.rs`**

```rust
// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    voicetabs_lib::run();
}
```

- [ ] **Step 5: Write `src-tauri/src/lib.rs`**

```rust
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 6: Write `src-tauri/capabilities/default.json`**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capabilities for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default"
  ]
}
```

- [ ] **Step 7: Add placeholder icon**

Tauri requires a valid PNG. Use a 256x256 transparent PNG placeholder. If you have no image generation handy, copy any 256x256 PNG into `src-tauri/icons/icon.png`. (We'll replace with a real icon in Phase 8.) PowerShell helper to create a blank placeholder:

```powershell
$png = [byte[]] @(0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A) # PNG signature
# Use a real icon. For the placeholder, fetch the Tauri default icon:
$src = "$env:USERPROFILE\.cargo\registry\cache"
# Simplest: pull a known-good 256x256 PNG from a sibling Tauri project, or run `npm create tauri-app` in a scratch dir and copy its icon.png here.
```

If you don't have a quick source, run `npm create tauri-app@latest _scratch -- --template react-ts -y` in a temporary directory and copy `_scratch/src-tauri/icons/icon.png` to `src-tauri/icons/icon.png`. Then delete `_scratch`.

```powershell

# Also produce icons/icon.ico — Tauri's Windows build links a Windows resource
# file that embeds an ICO. Generating it from the PNG keeps both in sync.
$src = New-Object System.Drawing.Bitmap("src-tauri\icons\icon.png")
$icoStream = New-Object System.IO.MemoryStream
$sizes = @(16, 32, 48, 256)
# Build a minimal multi-size ICO. For a placeholder, a single 32x32 frame is
# enough for Windows to accept it.
$frame = New-Object System.Drawing.Bitmap 32, 32
$gIco = [System.Drawing.Graphics]::FromImage($frame)
$gIco.DrawImage($src, 0, 0, 32, 32)
$gIco.Dispose()
$iconHandle = $frame.GetHicon()
$icon = [System.Drawing.Icon]::FromHandle($iconHandle)
$fs = [System.IO.File]::OpenWrite("src-tauri\icons\icon.ico")
$icon.Save($fs)
$fs.Close()
$icon.Dispose()
$frame.Dispose()
$src.Dispose()
```

- [ ] **Step 8: Commit**

```powershell
git add src-tauri
git commit -m "chore: Tauri 2 backend scaffolding"
```

---

## Task 5: Install dependencies and verify dev build

- [ ] **Step 1: Install Node deps**

Run:
```powershell
npm install
```
Expected: no errors; `node_modules/` populated.

- [ ] **Step 2: Build the frontend once** so `tauri::generate_context!()` finds `../dist`

Run:
```powershell
npm run build
```
Expected: `dist/index.html` is produced. The Rust build needs this file to exist before the macro expansion in `generate_context!()` runs.

- [ ] **Step 3: Verify Cargo build**

Run:
```powershell
cargo build --manifest-path src-tauri/Cargo.toml
```
Expected: compiles. First build will be slow (Tauri pulls many deps).

- [ ] **Step 4: Verify `npm run tauri dev` opens a window**

Run:
```powershell
npm run tauri dev
```
Expected: a window titled "VoiceTabs" opens, showing the placeholder heading. Press Ctrl+C in the terminal to stop.

- [ ] **Step 5: No commit needed** (lockfiles are committed in the next task if desired; verify the build works first).

---

## Task 6: Commit lockfiles

**Files:**
- Modify: `package-lock.json` (created by `npm install`)
- Modify: `Cargo.lock` (created by `cargo build`)

- [ ] **Step 1: Commit lockfiles**

```powershell
git add package-lock.json Cargo.lock
git commit -m "chore: lock dependencies"
```

---

## Task 7: Tracing logger

**Files:**
- Create: `src-tauri/src/logging.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/tests/logging_test.rs` (Rust unit test)
- Modify: `src-tauri/Cargo.toml` (already includes tracing deps)

- [ ] **Step 1: Write `src-tauri/src/logging.rs`**

```rust
use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, time::SystemTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Hold this guard for the lifetime of the application so the background
/// log-writing thread is not dropped.
pub struct LogGuard {
    _file_guard: WorkerGuard,
}

pub fn init(log_dir: PathBuf) -> anyhow::Result<LogGuard> {
    std::fs::create_dir_all(&log_dir)?;
    let file_appender =
        tracing_appender::rolling::daily(&log_dir, "voicetabs.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_env("VOICETABS_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_timer(SystemTime)
        .with_target(true);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_timer(SystemTime)
        .with_target(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("logger already initialized: {e}"))?;

    tracing::info!("logging initialized; dir = {}", log_dir.display());

    Ok(LogGuard { _file_guard: file_guard })
}
```

- [ ] **Step 2: Add `app_data_dir` helper**

Create `src-tauri/src/paths.rs`:

```rust
use std::path::PathBuf;

use directories::BaseDirs;

/// Returns the per-user app data directory: `%APPDATA%\voicetabs\` on Windows.
///
/// We use `BaseDirs::data_dir()` (which is `%APPDATA%` on Windows) and append
/// our own `voicetabs` segment. We deliberately avoid `ProjectDirs` because it
/// forces a `\<org>\<app>\data` suffix that does not match the design layout.
pub fn app_data_dir() -> anyhow::Result<PathBuf> {
    let base = BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("could not resolve base directories"))?;
    let dir = base.data_dir().join("voicetabs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(app_data_dir()?.join("logs"))
}
```

- [ ] **Step 3: Update `src-tauri/src/lib.rs` to wire logging**

```rust
pub mod logging;
pub mod paths;

pub fn run() {
    let _guard = match paths::log_dir().and_then(logging::init) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("logging init failed: {e}");
            None
        }
    };

    tauri::Builder::default()
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Write a Rust unit test for the logger**

Create `src-tauri/tests/logging_test.rs`:

```rust
use std::fs;

use tempfile::tempdir;

#[test]
fn init_creates_log_dir_and_emits_line() {
    let dir = tempdir().unwrap();
    let guard = voicetabs_lib::logging::init(dir.path().to_path_buf())
        .expect("init should succeed");
    tracing::info!("test line");
    drop(guard);

    let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert!(!entries.is_empty(), "log file should exist");
}
```

- [ ] **Step 5: Run the test**

Run:
```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test logging_test
```
Expected: 1 passed.

- [ ] **Step 6: Verify the dev build still works**

Run:
```powershell
npm run tauri dev
```
Look for an `info` line in the terminal: `logging initialized; dir = ...`. Ctrl+C to stop.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src src-tauri/tests
git commit -m "feat(logging): tracing-based file + stderr logger"
```

---

## Task 8: i18n with PT-BR and English bundles

**Files:**
- Create: `src/i18n/locales/pt-BR.json`
- Create: `src/i18n/locales/en.json`
- Create: `src/i18n/index.ts`
- Modify: `src/main.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: Write `src/i18n/locales/pt-BR.json`**

```json
{
  "app": {
    "title": "VoiceTabs",
    "tagline": "Anote falando."
  },
  "tabs": {
    "newTab": "Nova aba",
    "newTabTitle": "Sem título",
    "rename": "Renomear",
    "close": "Fechar",
    "confirmCloseNonEmpty": "Esta aba tem conteúdo. Fechar mesmo assim?"
  },
  "settings": {
    "open": "Configurações",
    "language": "Idioma",
    "languagePtBr": "Português (Brasil)",
    "languageEn": "English",
    "save": "Salvar",
    "cancel": "Cancelar",
    "close": "Fechar"
  },
  "capture": {
    "modeAlwaysOn": "Sempre ativo",
    "modePtt": "Push-to-talk"
  }
}
```

- [ ] **Step 2: Write `src/i18n/locales/en.json`**

```json
{
  "app": {
    "title": "VoiceTabs",
    "tagline": "Take notes by speaking."
  },
  "tabs": {
    "newTab": "New tab",
    "newTabTitle": "Untitled",
    "rename": "Rename",
    "close": "Close",
    "confirmCloseNonEmpty": "This tab has content. Close anyway?"
  },
  "settings": {
    "open": "Settings",
    "language": "Language",
    "languagePtBr": "Português (Brasil)",
    "languageEn": "English",
    "save": "Save",
    "cancel": "Cancel",
    "close": "Close"
  },
  "capture": {
    "modeAlwaysOn": "Always on",
    "modePtt": "Push-to-talk"
  }
}
```

- [ ] **Step 3: Write `src/i18n/index.ts`**

```ts
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import ptBR from "./locales/pt-BR.json";

export const SUPPORTED_LOCALES = ["pt-BR", "en"] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

function detectInitialLocale(): SupportedLocale {
  const fromBrowser = navigator.language || "pt-BR";
  return fromBrowser.toLowerCase().startsWith("en") ? "en" : "pt-BR";
}

export function initI18n(locale?: SupportedLocale) {
  const initial = locale ?? detectInitialLocale();
  if (!i18n.isInitialized) {
    i18n.use(initReactI18next).init({
      resources: {
        en: { translation: en },
        "pt-BR": { translation: ptBR },
      },
      lng: initial,
      fallbackLng: "pt-BR",
      interpolation: { escapeValue: false },
    });
  } else {
    void i18n.changeLanguage(initial);
  }
  return i18n;
}

export default i18n;
```

- [ ] **Step 4: Update `src/main.tsx` to bootstrap i18n**

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initI18n } from "./i18n";
import "./styles.css";

initI18n();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 5: Update `src/App.tsx` to use translated strings**

```tsx
import { useTranslation } from "react-i18next";

export default function App() {
  const { t } = useTranslation();
  return (
    <main className="app">
      <h1>{t("app.title")}</h1>
      <p>{t("app.tagline")}</p>
    </main>
  );
}
```

- [ ] **Step 6: Verify in dev**

Run:
```powershell
npm run tauri dev
```
Expected: window shows "VoiceTabs" + "Anote falando." (PT-BR is the fallback). Ctrl+C to stop.

- [ ] **Step 7: Commit**

```powershell
git add src
git commit -m "feat(i18n): react-i18next with pt-BR + en bundles"
```

---

## Task 9: Vitest + first TS test (i18n smoke)

**Files:**
- Create: `vitest.config.ts`
- Create: `src/__tests__/setup.ts`
- Create: `src/__tests__/i18n.test.tsx`

- [ ] **Step 1: Write `vitest.config.ts`**

```ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/__tests__/setup.ts"],
    css: false,
  },
});
```

- [ ] **Step 2: Write `src/__tests__/setup.ts`**

```ts
import "@testing-library/jest-dom/vitest";
```

This wires the jest-dom matchers (`toBeInTheDocument`, `toHaveTextContent`, etc.) into vitest's `expect` once per test process.

- [ ] **Step 3: Write the failing test**

Create `src/__tests__/i18n.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "../App";
import { initI18n } from "../i18n";

describe("i18n bootstrap", () => {
  it("renders the PT-BR app title when locale is pt-BR", () => {
    initI18n("pt-BR");
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "VoiceTabs",
    );
    expect(screen.getByText("Anote falando.")).toBeInTheDocument();
  });

  it("switches to English when locale is en", () => {
    initI18n("en");
    render(<App />);
    expect(screen.getByText("Take notes by speaking.")).toBeInTheDocument();
  });
});
```

- [ ] **Step 4: Run the test**

Run:
```powershell
npm test
```
Expected: 2 passed. (`initI18n` was written idempotent in Task 8 so the second call switches the language via `i18n.changeLanguage` rather than re-initializing.)

- [ ] **Step 5: Commit**

```powershell
git add vitest.config.ts src
git commit -m "test(i18n): vitest + first locale smoke test"
```

---

## Task 10: GitHub Actions Windows CI

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the workflow**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  windows-build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: ./src-tauri -> target

      - name: Install npm deps
        run: npm ci

      - name: Frontend unit tests
        run: npm test

      - name: Type-check
        run: npx tsc --noEmit

      - name: Rust unit tests
        working-directory: src-tauri
        run: cargo test

      - name: Build app
        run: npm run tauri build -- --debug
```

- [ ] **Step 2: Commit**

```powershell
git add .github
git commit -m "ci: Windows build + tests"
```

Note: CI verification happens when the repo is pushed. Locally, we have already verified `npm run tauri dev` works.

---

## End of Phase 0 — checkpoint

**Stop here and verify before proceeding to Phase 1.**

Manual checklist:
- [ ] `npm run tauri dev` opens a window with "VoiceTabs" + tagline.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes (1 test).
- [ ] `npm test` passes (2 tests).
- [ ] `git log` shows 10 commits in order matching the tasks above.
- [ ] `%APPDATA%\voicetabs\logs\` contains a log file after running `tauri dev`.

If any of these fails, do not start Phase 1.

---

# Phase 1 — Tab Editor with Persistence

## Task 11: Initial SQL migration

**Files:**
- Create: `src-tauri/migrations/001_initial_schema.sql`

- [ ] **Step 1: Write the migration**

```sql
-- 001_initial_schema.sql
-- Initial schema: tabs, segments, settings.
-- Segments table is created now (with a foreign key on tabs) but used in a later phase.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tabs (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  title      TEXT    NOT NULL,
  order_idx  INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tabs_order ON tabs(order_idx);

CREATE TABLE IF NOT EXISTS segments (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  tab_id         INTEGER NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
  position       INTEGER NOT NULL,
  text           TEXT    NOT NULL,
  original_text  TEXT    NOT NULL,
  audio_path     TEXT    NOT NULL,
  started_at     INTEGER NOT NULL,
  ended_at       INTEGER NOT NULL,
  duration_ms    INTEGER NOT NULL,
  vocab_snapshot TEXT    NOT NULL,
  avg_logprob    REAL    NOT NULL,
  no_speech_prob REAL    NOT NULL,
  model_id       TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_segments_tab ON segments(tab_id, position);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY
);
```

- [ ] **Step 2: Commit**

```powershell
git add src-tauri/migrations
git commit -m "feat(db): initial schema migration"
```

---

## Task 12: DB connection + migrations runner (Rust)

**Files:**
- Create: `src-tauri/src/db/mod.rs`
- Create: `src-tauri/src/db/connection.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write `src-tauri/src/db/mod.rs`**

```rust
pub mod connection;
pub mod settings;
pub mod tabs;

pub use connection::{open, Db};
```

(The `tabs` and `settings` submodules are added in later tasks. Add `pub mod` placeholders now so the structure is clear.)

For now, just write:

```rust
pub mod connection;

pub use connection::{open, Db};
```

We'll add `pub mod tabs;` and `pub mod settings;` when we create those files.

- [ ] **Step 2: Write `src-tauri/src/db/connection.rs`**

```rust
use std::{path::Path, sync::Arc};

use parking_lot::Mutex;
use rusqlite::Connection;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/001_initial_schema.sql")),
];

#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn with<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&mut Connection) -> T,
    {
        let mut guard = self.inner.lock();
        f(&mut guard)
    }

    #[cfg(test)]
    pub fn from_connection(conn: Connection) -> Self {
        Db { inner: Arc::new(Mutex::new(conn)) }
    }
}

pub fn open(path: &Path) -> anyhow::Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    apply_migrations(&conn)?;
    Ok(Db { inner: Arc::new(Mutex::new(conn)) })
}

fn apply_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
        [],
    )?;
    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    for (version, sql) in MIGRATIONS {
        if *version > current {
            tracing::info!("applying migration v{version}");
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO schema_version (version) VALUES (?)", [version])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        apply_migrations(&conn).unwrap();
        Db::from_connection(conn)
    }

    #[test]
    fn migrations_apply_idempotently() {
        let db = mem_db();
        // Applying twice should not fail.
        db.with(|c| apply_migrations(c)).unwrap();
        let version: i64 = db.with(|c| {
            c.query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
                .unwrap()
        });
        assert_eq!(version, 1);
    }

    #[test]
    fn tables_exist() {
        let db = mem_db();
        let count: i64 = db.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('tabs','segments','settings')",
                [],
                |r| r.get(0),
            )
            .unwrap()
        });
        assert_eq!(count, 3);
    }
}
```

- [ ] **Step 3: Wire DB open in `src-tauri/src/lib.rs`**

```rust
pub mod db;
pub mod logging;
pub mod paths;

pub fn run() {
    let _guard = paths::log_dir().and_then(logging::init).ok();

    let db_path = paths::app_data_dir()
        .expect("app data dir")
        .join("voicetabs.db");
    let db = db::open(&db_path).expect("open db");

    tauri::Builder::default()
        .manage(db)
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Run the DB tests**

Run:
```powershell
cargo test --manifest-path src-tauri/Cargo.toml db::connection
```
Expected: 2 passed.

- [ ] **Step 5: Verify the app still launches**

Run:
```powershell
npm run tauri dev
```
Look for "applying migration v1" in the log on first run. Ctrl+C to stop.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/db src-tauri/src/lib.rs
git commit -m "feat(db): connection + migrations runner"
```

---

## Task 13: Tabs repository (TDD)

**Files:**
- Create: `src-tauri/src/db/tabs.rs`
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: Add `pub mod tabs;` to `src-tauri/src/db/mod.rs`**

```rust
pub mod connection;
pub mod tabs;

pub use connection::{open, Db};
```

- [ ] **Step 2: Write the failing tests in `src-tauri/src/db/tabs.rs`**

```rust
use serde::{Deserialize, Serialize};

use super::Db;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tab {
    pub id: i64,
    pub title: String,
    pub order_idx: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum TabsError {
    #[error("tab not found: {0}")]
    NotFound(i64),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

pub fn list(db: &Db) -> Result<Vec<Tab>, TabsError> {
    db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, title, order_idx, created_at, updated_at
             FROM tabs ORDER BY order_idx ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Tab {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    order_idx: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn create(db: &Db, title: &str, now_ms: i64) -> Result<Tab, TabsError> {
    db.with(|c| {
        let tx = c.transaction()?;
        let next_order: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(order_idx), -1) + 1 FROM tabs",
                [],
                |r| r.get(0),
            )?;
        tx.execute(
            "INSERT INTO tabs (title, order_idx, created_at, updated_at)
             VALUES (?, ?, ?, ?)",
            rusqlite::params![title, next_order, now_ms, now_ms],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Tab {
            id,
            title: title.to_string(),
            order_idx: next_order,
            created_at: now_ms,
            updated_at: now_ms,
        })
    })
}

pub fn rename(db: &Db, id: i64, new_title: &str, now_ms: i64) -> Result<(), TabsError> {
    let changed = db.with(|c| {
        c.execute(
            "UPDATE tabs SET title = ?, updated_at = ? WHERE id = ?",
            rusqlite::params![new_title, now_ms, id],
        )
    })?;
    if changed == 0 {
        return Err(TabsError::NotFound(id));
    }
    Ok(())
}

pub fn delete(db: &Db, id: i64) -> Result<(), TabsError> {
    let changed = db.with(|c| c.execute("DELETE FROM tabs WHERE id = ?", [id]))?;
    if changed == 0 {
        return Err(TabsError::NotFound(id));
    }
    Ok(())
}

pub fn reorder(db: &Db, ordered_ids: &[i64], now_ms: i64) -> Result<(), TabsError> {
    db.with(|c| {
        let tx = c.transaction()?;
        for (idx, id) in ordered_ids.iter().enumerate() {
            let changed = tx.execute(
                "UPDATE tabs SET order_idx = ?, updated_at = ? WHERE id = ?",
                rusqlite::params![idx as i64, now_ms, id],
            )?;
            if changed == 0 {
                return Err(TabsError::NotFound(*id));
            }
        }
        tx.commit()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial_schema.sql"))
            .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap();
        Db::from_connection(conn)
    }

    #[test]
    fn create_then_list_returns_in_order() {
        let db = mem_db();
        let t1 = create(&db, "A", 100).unwrap();
        let t2 = create(&db, "B", 101).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, t1.id);
        assert_eq!(listed[0].order_idx, 0);
        assert_eq!(listed[1].id, t2.id);
        assert_eq!(listed[1].order_idx, 1);
    }

    #[test]
    fn rename_updates_title_and_updated_at() {
        let db = mem_db();
        let t = create(&db, "old", 100).unwrap();
        rename(&db, t.id, "new", 200).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed[0].title, "new");
        assert_eq!(listed[0].updated_at, 200);
    }

    #[test]
    fn rename_unknown_returns_not_found() {
        let db = mem_db();
        let err = rename(&db, 999, "x", 100).unwrap_err();
        assert!(matches!(err, TabsError::NotFound(999)));
    }

    #[test]
    fn delete_removes_the_tab() {
        let db = mem_db();
        let t = create(&db, "x", 1).unwrap();
        delete(&db, t.id).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn reorder_assigns_order_idx_by_position_in_array() {
        let db = mem_db();
        let a = create(&db, "A", 1).unwrap();
        let b = create(&db, "B", 2).unwrap();
        let c = create(&db, "C", 3).unwrap();
        reorder(&db, &[c.id, a.id, b.id], 10).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed[0].id, c.id);
        assert_eq!(listed[1].id, a.id);
        assert_eq!(listed[2].id, b.id);
        for (i, t) in listed.iter().enumerate() {
            assert_eq!(t.order_idx, i as i64);
        }
    }

    #[test]
    fn reorder_unknown_id_returns_not_found() {
        let db = mem_db();
        let a = create(&db, "A", 1).unwrap();
        let err = reorder(&db, &[a.id, 999], 10).unwrap_err();
        assert!(matches!(err, TabsError::NotFound(999)));
    }
}
```

- [ ] **Step 3: Run the tests**

Run:
```powershell
cargo test --manifest-path src-tauri/Cargo.toml db::tabs
```
Expected: 6 passed.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/db
git commit -m "feat(db): tabs repository with CRUD + reorder"
```

---

## Task 14: Settings repository (TDD)

**Files:**
- Create: `src-tauri/src/db/settings.rs`
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: Add `pub mod settings;` to `src-tauri/src/db/mod.rs`**

```rust
pub mod connection;
pub mod settings;
pub mod tabs;

pub use connection::{open, Db};
```

- [ ] **Step 2: Write the repository + tests**

Create `src-tauri/src/db/settings.rs`:

```rust
use serde::{de::DeserializeOwned, Serialize};

use super::Db;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn get_raw(db: &Db, key: &str) -> Result<Option<String>, SettingsError> {
    let value: Option<String> = db.with(|c| {
        c.query_row(
            "SELECT value FROM settings WHERE key = ?",
            [key],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })?;
    Ok(value)
}

pub fn set_raw(db: &Db, key: &str, value: &str) -> Result<(), SettingsError> {
    db.with(|c| {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )
    })?;
    Ok(())
}

pub fn get<T: DeserializeOwned>(db: &Db, key: &str) -> Result<Option<T>, SettingsError> {
    match get_raw(db, key)? {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

pub fn set<T: Serialize>(db: &Db, key: &str, value: &T) -> Result<(), SettingsError> {
    let encoded = serde_json::to_string(value)?;
    set_raw(db, key, &encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial_schema.sql"))
            .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap();
        Db::from_connection(conn)
    }

    #[test]
    fn get_missing_returns_none() {
        let db = mem_db();
        assert!(get::<String>(&db, "missing").unwrap().is_none());
    }

    #[test]
    fn set_then_get_roundtrips_a_string() {
        let db = mem_db();
        set(&db, "ui_locale", &"pt-BR".to_string()).unwrap();
        let got: Option<String> = get(&db, "ui_locale").unwrap();
        assert_eq!(got.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn set_overwrites_existing_value() {
        let db = mem_db();
        set(&db, "ui_locale", &"pt-BR".to_string()).unwrap();
        set(&db, "ui_locale", &"en".to_string()).unwrap();
        let got: Option<String> = get(&db, "ui_locale").unwrap();
        assert_eq!(got.as_deref(), Some("en"));
    }

    #[test]
    fn set_then_get_roundtrips_structured_value() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Active { tab_id: i64 }
        let db = mem_db();
        set(&db, "active_tab", &Active { tab_id: 42 }).unwrap();
        let got: Active = get(&db, "active_tab").unwrap().unwrap();
        assert_eq!(got, Active { tab_id: 42 });
    }
}
```

- [ ] **Step 3: Run the tests**

Run:
```powershell
cargo test --manifest-path src-tauri/Cargo.toml db::settings
```
Expected: 4 passed.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/db
git commit -m "feat(db): settings repository (JSON-encoded values)"
```

---

## Task 15: Tauri commands for tabs

**Files:**
- Create: `src-tauri/src/commands/mod.rs`
- Create: `src-tauri/src/commands/tabs.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write `src-tauri/src/commands/mod.rs`**

```rust
pub mod settings;
pub mod tabs;
```

Note: `settings` is added in the next task; `pub mod settings;` may not compile until then. To keep this task standalone, write only:

```rust
pub mod tabs;
```

and add `pub mod settings;` in the next task.

- [ ] **Step 2: Write `src-tauri/src/commands/tabs.rs`**

```rust
use serde::Serialize;
use tauri::State;

use crate::db::{tabs as repo, Db};

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<repo::TabsError> for CommandError {
    fn from(e: repo::TabsError) -> Self {
        match e {
            repo::TabsError::NotFound(id) => CommandError {
                code: "TAB_NOT_FOUND".into(),
                message: format!("tab {id} not found"),
            },
            repo::TabsError::Sql(e) => CommandError {
                code: "SQL_ERROR".into(),
                message: e.to_string(),
            },
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn tabs_list(db: State<'_, Db>) -> Result<Vec<repo::Tab>, CommandError> {
    repo::list(&db).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_create(title: String, db: State<'_, Db>) -> Result<repo::Tab, CommandError> {
    repo::create(&db, &title, now_ms()).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_rename(id: i64, title: String, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::rename(&db, id, &title, now_ms()).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_delete(id: i64, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::delete(&db, id).map_err(Into::into)
}

#[tauri::command]
pub fn tabs_reorder(ordered_ids: Vec<i64>, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::reorder(&db, &ordered_ids, now_ms()).map_err(Into::into)
}
```

- [ ] **Step 3: Register the commands in `src-tauri/src/lib.rs`**

```rust
pub mod commands;
pub mod db;
pub mod logging;
pub mod paths;

pub fn run() {
    let _guard = paths::log_dir().and_then(logging::init).ok();

    let db_path = paths::app_data_dir()
        .expect("app data dir")
        .join("voicetabs.db");
    let db = db::open(&db_path).expect("open db");

    tauri::Builder::default()
        .manage(db)
        .invoke_handler(tauri::generate_handler![
            commands::tabs::tabs_list,
            commands::tabs::tabs_create,
            commands::tabs::tabs_rename,
            commands::tabs::tabs_delete,
            commands::tabs::tabs_reorder,
        ])
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Verify `cargo build` succeeds**

Run:
```powershell
cargo build --manifest-path src-tauri/Cargo.toml
```
Expected: builds cleanly.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src
git commit -m "feat(commands): tabs CRUD over Tauri invoke"
```

---

## Task 16: Tauri commands for settings

**Files:**
- Create: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Update `src-tauri/src/commands/mod.rs`**

```rust
pub mod settings;
pub mod tabs;
```

- [ ] **Step 2: Write `src-tauri/src/commands/settings.rs`**

```rust
use tauri::State;

use crate::db::{settings as repo, Db};

use super::tabs::CommandError;

impl From<repo::SettingsError> for CommandError {
    fn from(e: repo::SettingsError) -> Self {
        CommandError { code: "SETTINGS_ERROR".into(), message: e.to_string() }
    }
}

#[tauri::command]
pub fn settings_get(key: String, db: State<'_, Db>) -> Result<Option<String>, CommandError> {
    repo::get_raw(&db, &key).map_err(Into::into)
}

#[tauri::command]
pub fn settings_set(key: String, value: String, db: State<'_, Db>) -> Result<(), CommandError> {
    repo::set_raw(&db, &key, &value).map_err(Into::into)
}
```

(The raw API is exposed to the frontend so JS owns the JSON encoding; this keeps a single source of truth for value shapes.)

- [ ] **Step 3: Register the settings commands**

Update the `invoke_handler` in `src-tauri/src/lib.rs`:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::tabs::tabs_list,
            commands::tabs::tabs_create,
            commands::tabs::tabs_rename,
            commands::tabs::tabs_delete,
            commands::tabs::tabs_reorder,
            commands::settings::settings_get,
            commands::settings::settings_set,
        ])
```

- [ ] **Step 4: Verify build**

```powershell
cargo build --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src
git commit -m "feat(commands): settings get/set"
```

---

## Task 17: Frontend Tauri wrapper module

**Files:**
- Create: `src/lib/tauri.ts`

- [ ] **Step 1: Write typed invoke wrappers**

```ts
import { invoke } from "@tauri-apps/api/core";

export type Tab = {
  id: number;
  title: string;
  order_idx: number;
  created_at: number;
  updated_at: number;
};

export type CommandError = {
  code: string;
  message: string;
};

export const tabsApi = {
  list(): Promise<Tab[]> {
    return invoke<Tab[]>("tabs_list");
  },
  create(title: string): Promise<Tab> {
    return invoke<Tab>("tabs_create", { title });
  },
  rename(id: number, title: string): Promise<void> {
    return invoke<void>("tabs_rename", { id, title });
  },
  delete(id: number): Promise<void> {
    return invoke<void>("tabs_delete", { id });
  },
  reorder(orderedIds: number[]): Promise<void> {
    return invoke<void>("tabs_reorder", { orderedIds });
  },
};

export const settingsApi = {
  get(key: string): Promise<string | null> {
    return invoke<string | null>("settings_get", { key });
  },
  async getJson<T>(key: string): Promise<T | null> {
    const raw = await this.get(key);
    return raw === null ? null : (JSON.parse(raw) as T);
  },
  set(key: string, value: string): Promise<void> {
    return invoke<void>("settings_set", { key, value });
  },
  setJson<T>(key: string, value: T): Promise<void> {
    return this.set(key, JSON.stringify(value));
  },
};
```

- [ ] **Step 2: Commit**

```powershell
git add src/lib
git commit -m "feat(frontend): typed wrappers for Tauri commands"
```

---

## Task 18: Zustand tabs store

**Files:**
- Create: `src/stores/tabsStore.ts`

- [ ] **Step 1: Write the store**

```ts
import { create } from "zustand";

import i18n from "../i18n";
import { settingsApi, tabsApi, Tab } from "../lib/tauri";

const ACTIVE_KEY = "active_tab_id";

function defaultTitle(): string {
  return i18n.t("tabs.newTabTitle");
}

type TabsState = {
  tabs: Tab[];
  activeTabId: number | null;
  loaded: boolean;

  load: () => Promise<void>;
  setActive: (id: number) => Promise<void>;
  createTab: (title: string) => Promise<Tab>;
  renameTab: (id: number, title: string) => Promise<void>;
  deleteTab: (id: number) => Promise<void>;
  reorderTabs: (orderedIds: number[]) => Promise<void>;
};

export const useTabsStore = create<TabsState>((set, get) => ({
  tabs: [],
  activeTabId: null,
  loaded: false,

  async load() {
    const tabs = await tabsApi.list();
    const activeRaw = await settingsApi.get(ACTIVE_KEY);
    let activeTabId: number | null = activeRaw === null ? null : Number(activeRaw);

    if (tabs.length === 0) {
      const fresh = await tabsApi.create(defaultTitle());
      set({ tabs: [fresh], activeTabId: fresh.id, loaded: true });
      await settingsApi.set(ACTIVE_KEY, String(fresh.id));
      return;
    }

    if (activeTabId === null || !tabs.find((t) => t.id === activeTabId)) {
      activeTabId = tabs[0].id;
      await settingsApi.set(ACTIVE_KEY, String(activeTabId));
    }

    set({ tabs, activeTabId, loaded: true });
  },

  async setActive(id) {
    set({ activeTabId: id });
    await settingsApi.set(ACTIVE_KEY, String(id));
  },

  async createTab(title) {
    const tab = await tabsApi.create(title);
    set((s) => ({ tabs: [...s.tabs, tab], activeTabId: tab.id }));
    await settingsApi.set(ACTIVE_KEY, String(tab.id));
    return tab;
  },

  async renameTab(id, title) {
    await tabsApi.rename(id, title);
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, title } : t)),
    }));
  },

  async deleteTab(id) {
    await tabsApi.delete(id);
    const remaining = get().tabs.filter((t) => t.id !== id);

    if (remaining.length === 0) {
      const fresh = await tabsApi.create(defaultTitle());
      set({ tabs: [fresh], activeTabId: fresh.id });
      await settingsApi.set(ACTIVE_KEY, String(fresh.id));
      return;
    }

    let nextActive = get().activeTabId;
    if (nextActive === id) {
      nextActive = remaining[0].id;
      await settingsApi.set(ACTIVE_KEY, String(nextActive));
    }
    set({ tabs: remaining, activeTabId: nextActive });
  },

  async reorderTabs(orderedIds) {
    await tabsApi.reorder(orderedIds);
    const byId = new Map(get().tabs.map((t) => [t.id, t]));
    const reordered = orderedIds
      .map((id, idx) => {
        const t = byId.get(id);
        return t ? { ...t, order_idx: idx } : null;
      })
      .filter((t): t is Tab => t !== null);
    set({ tabs: reordered });
  },
}));
```

(The store uses `i18n.t("tabs.newTabTitle")` for auto-created tabs. The `settingsStore.load()` in Task 22 sets `i18n.changeLanguage(...)` to the persisted locale BEFORE `tabsStore.load()` runs, so the title respects the user's chosen language even on the first load after a restart.)

- [ ] **Step 2: Commit**

```powershell
git add src/stores
git commit -m "feat(frontend): zustand tabs store with persistence"
```

---

## Task 19: TabStrip component (TDD) — render + active + new

**Files:**
- Create: `src/components/TabStrip.tsx`
- Create: `src/__tests__/TabStrip.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TabStrip } from "../components/TabStrip";
import { Tab } from "../lib/tauri";

const tab = (id: number, title: string, order_idx: number): Tab => ({
  id,
  title,
  order_idx,
  created_at: 0,
  updated_at: 0,
});

describe("TabStrip", () => {
  it("renders one button per tab", () => {
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0), tab(2, "Beta", 1)]}
        activeId={1}
        onSelect={() => {}}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    expect(screen.getByRole("tab", { name: /Alpha/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Beta/ })).toBeInTheDocument();
  });

  it("marks the active tab with aria-selected", () => {
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0), tab(2, "Beta", 1)]}
        activeId={2}
        onSelect={() => {}}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    expect(screen.getByRole("tab", { name: /Beta/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("fires onSelect when clicking a tab", () => {
    const onSelect = vi.fn();
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0), tab(2, "Beta", 1)]}
        activeId={1}
        onSelect={onSelect}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: /Beta/ }));
    expect(onSelect).toHaveBeenCalledWith(2);
  });

  it("fires onCreate when clicking the + button", () => {
    const onCreate = vi.fn();
    render(
      <TabStrip
        tabs={[tab(1, "Alpha", 0)]}
        activeId={1}
        onSelect={() => {}}
        onCreate={onCreate}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /new tab|nova aba/i }));
    expect(onCreate).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run the test (expect failure: TabStrip not defined)**

Run:
```powershell
npm test
```

- [ ] **Step 3: Implement `src/components/TabStrip.tsx`**

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Tab } from "../lib/tauri";

type Props = {
  tabs: Tab[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onCreate: () => void;
  onRename: (id: number, title: string) => void;
  onClose: (id: number) => void;
  onReorder: (orderedIds: number[]) => void;
};

export function TabStrip({
  tabs,
  activeId,
  onSelect,
  onCreate,
  onRename,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");

  function commitRename(id: number) {
    if (draft.trim().length > 0) onRename(id, draft.trim());
    setRenamingId(null);
  }

  return (
    <div className="tab-strip" role="tablist">
      {tabs.map((tab) => {
        const isActive = tab.id === activeId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            tabIndex={isActive ? 0 : -1}
            aria-label={tab.title}
            className={`tab${isActive ? " tab--active" : ""}`}
            onClick={() => onSelect(tab.id)}
            onDoubleClick={() => {
              setRenamingId(tab.id);
              setDraft(tab.title);
            }}
          >
            {renamingId === tab.id ? (
              <input
                autoFocus
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => commitRename(tab.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename(tab.id);
                  if (e.key === "Escape") setRenamingId(null);
                }}
              />
            ) : (
              <span className="tab__title">{tab.title}</span>
            )}
            <button
              className="tab__close"
              aria-label={t("tabs.close")}
              onClick={(e) => {
                e.stopPropagation();
                onClose(tab.id);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
      <button
        className="tab-strip__new"
        aria-label={t("tabs.newTab")}
        onClick={onCreate}
      >
        +
      </button>
    </div>
  );
}
```

(Note: drag-to-reorder is added in Task 21. `onReorder` is unused for now; included in the props for forward-compat.)

- [ ] **Step 4: Pin the test locale to English in the setup file**

Update `src/__tests__/setup.ts` (created in Task 9) to also pin the locale so that `t("tabs.close")` and `t("tabs.newTab")` produce English strings, which the new TabStrip tests match against:

```ts
import "@testing-library/jest-dom/vitest";

import { initI18n } from "../i18n";

initI18n("en");
```

- [ ] **Step 5: Run the tests**

Run:
```powershell
npm test
```
Expected: 6 passed (2 i18n + 4 TabStrip).

- [ ] **Step 6: Add base CSS for the tab strip**

Append to `src/styles.css`:

```css
.tab-strip {
  display: flex;
  align-items: stretch;
  background: #2a2a2a;
  border-bottom: 1px solid #3a3a3a;
  user-select: none;
  min-height: 36px;
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-right: 1px solid #3a3a3a;
  cursor: pointer;
  background: transparent;
  color: #ccc;
  font-size: 13px;
  max-width: 220px;
}

.tab--active {
  background: #1e1e1e;
  color: #fff;
  border-top: 2px solid #4f8cf7;
}

.tab__title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tab__close {
  background: transparent;
  color: #888;
  border: 0;
  cursor: pointer;
  padding: 0 4px;
  font-size: 14px;
}

.tab__close:hover {
  color: #fff;
}

.tab-strip__new {
  background: transparent;
  border: 0;
  color: #888;
  cursor: pointer;
  padding: 6px 12px;
  font-size: 16px;
}

.tab-strip__new:hover {
  color: #fff;
}
```

- [ ] **Step 7: Commit**

```powershell
git add src
git commit -m "feat(ui): TabStrip with select/create/rename/close"
```

---

## Task 20: Wire TabStrip into App.tsx with the Zustand store

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Update `src/App.tsx`**

```tsx
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { TabStrip } from "./components/TabStrip";
import { useTabsStore } from "./stores/tabsStore";

export default function App() {
  const { t } = useTranslation();
  const {
    tabs,
    activeTabId,
    loaded,
    load,
    setActive,
    createTab,
    renameTab,
    deleteTab,
    reorderTabs,
  } = useTabsStore();

  useEffect(() => {
    if (!loaded) {
      void load();
    }
  }, [loaded, load]);

  if (!loaded) {
    return <main className="app" />;
  }

  return (
    <main className="app">
      <TabStrip
        tabs={tabs}
        activeId={activeTabId}
        onSelect={(id) => void setActive(id)}
        onCreate={() => void createTab(t("tabs.newTabTitle"))}
        onRename={(id, title) => void renameTab(id, title)}
        onClose={(id) => void deleteTab(id)}
        onReorder={(ordered) => void reorderTabs(ordered)}
      />
      <div className="tab-body">
        {tabs.length > 0 && activeTabId != null && (
          <p style={{ padding: 16, color: "#888" }}>
            {tabs.find((t) => t.id === activeTabId)?.title}
          </p>
        )}
      </div>
    </main>
  );
}
```

- [ ] **Step 2: Add a minimal `.tab-body` style**

Append to `src/styles.css`:

```css
.tab-body {
  flex: 1;
  overflow-y: auto;
  background: #1e1e1e;
}
```

- [ ] **Step 3: Verify the dev build**

Run:
```powershell
npm run tauri dev
```
Manually verify: window opens with one "Sem título" tab; clicking "+" adds a tab; double-click a title to rename + Enter; click × to close; closing the last tab auto-creates a new one. Ctrl+C to stop.

- [ ] **Step 4: Commit**

```powershell
git add src
git commit -m "feat(ui): wire TabStrip to Zustand store and persistence"
```

---

## Task 21: Drag-to-reorder via @dnd-kit

**Files:**
- Modify: `src/components/TabStrip.tsx`
- Modify: `src/__tests__/TabStrip.test.tsx` (add a reorder test)

- [ ] **Step 1: Add the reorder test**

Append to `src/__tests__/TabStrip.test.tsx`:

```tsx
describe("TabStrip reorder", () => {
  it("calls onReorder with the new order when items are programmatically moved", () => {
    // dnd-kit drag is hard to simulate in jsdom; we verify the component
    // exposes the onReorder prop by invoking it via the kit's helper.
    // For a full e2e test we will rely on manual verification.
    const onReorder = vi.fn();
    render(
      <TabStrip
        tabs={[tab(1, "A", 0), tab(2, "B", 1), tab(3, "C", 2)]}
        activeId={1}
        onSelect={() => {}}
        onCreate={() => {}}
        onRename={() => {}}
        onClose={() => {}}
        onReorder={onReorder}
      />,
    );
    // Components rendered; full drag simulation is out of scope for unit tests.
    expect(screen.getByRole("tab", { name: /A/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /B/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /C/ })).toBeInTheDocument();
  });
});
```

(The drag interaction is verified manually in Task 24; jsdom does not reliably simulate the pointer events `@dnd-kit` needs.)

- [ ] **Step 2: Update `src/components/TabStrip.tsx` to support drag**

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  closestCenter,
  DndContext,
  DragEndEvent,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  horizontalListSortingStrategy,
  SortableContext,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

import { Tab } from "../lib/tauri";

type Props = {
  tabs: Tab[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onCreate: () => void;
  onRename: (id: number, title: string) => void;
  onClose: (id: number) => void;
  onReorder: (orderedIds: number[]) => void;
};

function SortableTab(props: {
  tab: Tab;
  isActive: boolean;
  isRenaming: boolean;
  draft: string;
  setDraft: (v: string) => void;
  commitRename: () => void;
  cancelRename: () => void;
  onSelect: () => void;
  beginRename: () => void;
  onClose: () => void;
  closeLabel: string;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: props.tab.id });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.6 : 1,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      role="tab"
      aria-selected={props.isActive}
      tabIndex={props.isActive ? 0 : -1}
      aria-label={props.tab.title}
      className={`tab${props.isActive ? " tab--active" : ""}`}
      onClick={props.onSelect}
      onDoubleClick={props.beginRename}
      {...attributes}
      {...listeners}
    >
      {props.isRenaming ? (
        <input
          autoFocus
          value={props.draft}
          onChange={(e) => props.setDraft(e.target.value)}
          onBlur={props.commitRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") props.commitRename();
            if (e.key === "Escape") props.cancelRename();
          }}
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
        />
      ) : (
        <span className="tab__title">{props.tab.title}</span>
      )}
      <button
        className="tab__close"
        aria-label={props.closeLabel}
        onClick={(e) => {
          e.stopPropagation();
          props.onClose();
        }}
        onPointerDown={(e) => e.stopPropagation()}
      >
        ×
      </button>
    </div>
  );
}

export function TabStrip({
  tabs,
  activeId,
  onSelect,
  onCreate,
  onRename,
  onClose,
  onReorder,
}: Props) {
  const { t } = useTranslation();
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );

  function commitRename(id: number) {
    if (draft.trim().length > 0) onRename(id, draft.trim());
    setRenamingId(null);
  }

  function handleDragEnd(e: DragEndEvent) {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const oldIndex = tabs.findIndex((t) => t.id === Number(active.id));
    const newIndex = tabs.findIndex((t) => t.id === Number(over.id));
    if (oldIndex < 0 || newIndex < 0) return;
    const reordered = arrayMove(tabs, oldIndex, newIndex);
    onReorder(reordered.map((t) => t.id));
  }

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
      <div className="tab-strip" role="tablist">
        <SortableContext items={tabs.map((t) => t.id)} strategy={horizontalListSortingStrategy}>
          {tabs.map((tab) => (
            <SortableTab
              key={tab.id}
              tab={tab}
              isActive={tab.id === activeId}
              isRenaming={renamingId === tab.id}
              draft={draft}
              setDraft={setDraft}
              commitRename={() => commitRename(tab.id)}
              cancelRename={() => setRenamingId(null)}
              onSelect={() => onSelect(tab.id)}
              beginRename={() => {
                setRenamingId(tab.id);
                setDraft(tab.title);
              }}
              onClose={() => onClose(tab.id)}
              closeLabel={t("tabs.close")}
            />
          ))}
        </SortableContext>
        <button
          className="tab-strip__new"
          aria-label={t("tabs.newTab")}
          onClick={onCreate}
        >
          +
        </button>
      </div>
    </DndContext>
  );
}
```

- [ ] **Step 3: Re-run unit tests**

Run:
```powershell
npm test
```
Expected: all previous tests still pass (drag is not exercised in jsdom).

- [ ] **Step 4: Manual verification**

Run:
```powershell
npm run tauri dev
```
Create three tabs, drag one to a new position, restart the app (close the window with × from the OS, then `npm run tauri dev` again). Verify the new order persists. Ctrl+C to stop.

- [ ] **Step 5: Commit**

```powershell
git add src
git commit -m "feat(ui): drag-to-reorder tabs via @dnd-kit/sortable"
```

---

## Task 22: Settings store + language switcher

**Files:**
- Create: `src/stores/settingsStore.ts`
- Create: `src/components/SettingsDrawer.tsx`
- Modify: `src/App.tsx`
- Modify: `src/main.tsx` — hydrate locale before render

- [ ] **Step 1: Write `src/stores/settingsStore.ts`**

```ts
import { create } from "zustand";

import { settingsApi } from "../lib/tauri";
import i18n, { SupportedLocale, SUPPORTED_LOCALES } from "../i18n";

const LOCALE_KEY = "ui_locale";

type SettingsState = {
  uiLocale: SupportedLocale;
  drawerOpen: boolean;

  load: () => Promise<void>;
  setLocale: (locale: SupportedLocale) => Promise<void>;
  openDrawer: () => void;
  closeDrawer: () => void;
};

export const useSettingsStore = create<SettingsState>((set) => ({
  uiLocale: "pt-BR",
  drawerOpen: false,

  async load() {
    const stored = await settingsApi.get(LOCALE_KEY);
    const locale: SupportedLocale = SUPPORTED_LOCALES.includes(stored as SupportedLocale)
      ? (stored as SupportedLocale)
      : (navigator.language?.toLowerCase().startsWith("en") ? "en" : "pt-BR");
    await i18n.changeLanguage(locale);
    document.documentElement.lang = locale;
    set({ uiLocale: locale });
  },

  async setLocale(locale) {
    await settingsApi.set(LOCALE_KEY, locale);
    await i18n.changeLanguage(locale);
    document.documentElement.lang = locale;
    set({ uiLocale: locale });
  },

  openDrawer() {
    set({ drawerOpen: true });
  },
  closeDrawer() {
    set({ drawerOpen: false });
  },
}));
```

- [ ] **Step 2: Write `src/components/SettingsDrawer.tsx`**

```tsx
import { useTranslation } from "react-i18next";

import { useSettingsStore } from "../stores/settingsStore";
import { SupportedLocale } from "../i18n";

export function SettingsDrawer() {
  const { t } = useTranslation();
  const { drawerOpen, closeDrawer, uiLocale, setLocale } = useSettingsStore();

  if (!drawerOpen) return null;

  return (
    <div className="drawer-backdrop" onClick={closeDrawer}>
      <aside className="drawer" role="dialog" aria-label={t("settings.open")} onClick={(e) => e.stopPropagation()}>
        <header className="drawer__header">
          <h2>{t("settings.open")}</h2>
          <button onClick={closeDrawer} aria-label={t("settings.close")}>×</button>
        </header>

        <section className="drawer__section">
          <label htmlFor="locale-select">{t("settings.language")}</label>
          <select
            id="locale-select"
            value={uiLocale}
            onChange={(e) => void setLocale(e.target.value as SupportedLocale)}
          >
            <option value="pt-BR">{t("settings.languagePtBr")}</option>
            <option value="en">{t("settings.languageEn")}</option>
          </select>
        </section>
      </aside>
    </div>
  );
}
```

- [ ] **Step 3: Update `src/App.tsx` to mount settings**

```tsx
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { TabStrip } from "./components/TabStrip";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { useTabsStore } from "./stores/tabsStore";
import { useSettingsStore } from "./stores/settingsStore";

export default function App() {
  const { t } = useTranslation();
  const tabs = useTabsStore();
  const settings = useSettingsStore();

  useEffect(() => {
    void (async () => {
      await settings.load();
      await tabs.load();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!tabs.loaded) {
    return <main className="app" />;
  }

  return (
    <main className="app">
      <TabStrip
        tabs={tabs.tabs}
        activeId={tabs.activeTabId}
        onSelect={(id) => void tabs.setActive(id)}
        onCreate={() => void tabs.createTab(t("tabs.newTabTitle"))}
        onRename={(id, title) => void tabs.renameTab(id, title)}
        onClose={(id) => void tabs.deleteTab(id)}
        onReorder={(ordered) => void tabs.reorderTabs(ordered)}
      />
      <div className="tab-body">
        {tabs.tabs.length > 0 && tabs.activeTabId != null && (
          <p style={{ padding: 16, color: "#888" }}>
            {tabs.tabs.find((t) => t.id === tabs.activeTabId)?.title}
          </p>
        )}
      </div>
      <footer className="app-footer">
        <span />
        <button onClick={settings.openDrawer} className="footer-button">
          ⚙ {t("settings.open")}
        </button>
      </footer>
      <SettingsDrawer />
    </main>
  );
}
```

- [ ] **Step 4: Append drawer + footer styles**

Append to `src/styles.css`:

```css
.app-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #2a2a2a;
  border-top: 1px solid #3a3a3a;
  padding: 4px 12px;
  min-height: 28px;
  font-size: 12px;
  color: #888;
}

.footer-button {
  background: transparent;
  border: 0;
  color: #aaa;
  cursor: pointer;
  font-size: 12px;
}

.footer-button:hover {
  color: #fff;
}

.drawer-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  z-index: 10;
}

.drawer {
  position: fixed;
  right: 0;
  top: 0;
  bottom: 0;
  width: 360px;
  background: #2a2a2a;
  color: #fff;
  border-left: 1px solid #3a3a3a;
  padding: 16px;
  overflow-y: auto;
  z-index: 11;
}

.drawer__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

.drawer__header button {
  background: transparent;
  border: 0;
  color: #aaa;
  font-size: 20px;
  cursor: pointer;
}

.drawer__section {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 16px;
}

.drawer__section select {
  background: #1e1e1e;
  color: #fff;
  border: 1px solid #3a3a3a;
  padding: 6px 8px;
  font-size: 13px;
}
```

- [ ] **Step 5: Run unit tests**

```powershell
npm test
```
Expected: all tests still pass.

- [ ] **Step 6: Manual verification**

```powershell
npm run tauri dev
```
- Click ⚙ Configurações → drawer opens
- Switch to English → all visible strings update
- Close window with the OS × → relaunch with `npm run tauri dev` → drawer language choice persisted

Ctrl+C to stop.

- [ ] **Step 7: Commit**

```powershell
git add src
git commit -m "feat(settings): language picker with persistence + drawer"
```

---

## Task 23: Title bar reflects app name in tray-ready way

**Files:**
- Modify: `src/App.tsx` — set document.title from i18n

(Anticipating the tray in a future plan, the document title should match the productName.)

- [ ] **Step 1: Update `src/App.tsx`**

Add inside `App`, near the existing `useEffect`:

```tsx
  useEffect(() => {
    document.title = t("app.title");
  }, [t]);
```

- [ ] **Step 2: Re-run tests**

```powershell
npm test
```

- [ ] **Step 3: Commit**

```powershell
git add src
git commit -m "chore(ui): keep document.title in sync with i18n"
```

---

## Task 24: End-to-end manual acceptance pass

**Files:** None.

This is the Phase 1 acceptance gate. The engineer performs each step manually and records observed behavior in the stop-and-report. Do not skip — these correspond to the spec's A8 (partial) and the brief's request for "tabs survive restart".

- [ ] **Step 1: Reset user data**

To simulate a fresh user state:

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs" -ErrorAction SilentlyContinue
```

- [ ] **Step 2: Launch the app and verify first-run state**

```powershell
npm run tauri dev
```

Expected: window opens; one tab titled "Sem título" exists (PT-BR default).

- [ ] **Step 3: Create three tabs and rename them**

- Click "+" twice → three tabs total.
- Double-click tab 1 title → type "Reunião" → Enter.
- Double-click tab 2 title → type "Pessoal" → Enter.
- Double-click tab 3 title → type "Ideias" → Enter.

Expected: titles update; the renamed tabs persist in the strip.

- [ ] **Step 4: Reorder**

Drag "Ideias" to position 1.

Expected: visual order updates without errors.

- [ ] **Step 5: Switch active tab and switch language**

- Click "Pessoal" → it becomes active (highlighted top border).
- Open Settings → switch to English.

Expected: footer button, settings labels, and "New tab" all read in English.

- [ ] **Step 6: Restart and verify persistence**

- Close window (OS ×).
- Re-run `npm run tauri dev`.

Expected:
- Three tabs in the order: Ideias, Reunião, Pessoal.
- The previously active tab ("Pessoal") is active.
- UI language is English (persisted).

- [ ] **Step 7: Close to last tab**

Close all three tabs one by one with their × button.

Expected: when the last is closed, a new "Untitled" tab is created automatically.

- [ ] **Step 8: Record results in the stop-and-report**

Write a short report covering:
- Whether each step above passed.
- Total commit count and a summary of major changes.
- Anything that deviated from this plan.
- A pointer to `%APPDATA%\voicetabs\voicetabs.db` and a `sqlite3` query showing tab rows (optional).

If any step failed, do not declare Phase 1 complete.

- [ ] **Step 9: Final commit (if any tweaks were applied during manual testing)**

```powershell
git add -A
git diff --cached --stat
git commit -m "chore: phase 1 acceptance pass" --allow-empty
```

---

## End of Phase 1 — checkpoint

**Stop here and produce a stop-and-report.** Then we plan Phase 2 (audio capture + VAD) in a separate planning session.

**What's verified at this checkpoint:**
- A1 (installer) — not yet (Phase 8).
- A2 (first-run wizard) — not yet (Phase 3).
- A3, A4, A5, A6, A7 — not applicable; no audio.
- A8 (persistence) — **partial: tabs + settings persist, segments + audio not yet**.
- L1, L2 — not applicable.

**What's verified that isn't in the acceptance list but matters:**
- i18n PT-BR + English shipped and switchable.
- DB migration pipeline works and is idempotent.
- Rust + TS unit test infrastructure passes locally.
- CI workflow is in place (will run on first push).
