# Riptag

Desktop app that rips audio CDs and outputs files formatted for the **FiiO Snowsky Echo Mini**. Also repairs existing music libraries for the device. "Riptag" is a working name.

**Source of truth for design decisions:** `~/Knowledge/Projects/Riptag CD Ripper.md` (Obsidian vault). Read it at the start of a session; update it when a decision changes.

**Mockups:** "CD Ripper Mockups" design canvas on claude.ai. Page "v3 · Minimal terminal" is the chosen direction; v1/v2 are reference only.

## Status

Milestone 1 in progress. Fix Library scans a folder, shows a diagnosis per album and writes repaired copies to a separate folder, then fetches synced lyrics from LRCLIB into them. Next: convert unsupported formats (AIFF first), tag cleanup beyond FLAC/MP3, sync to device.

## Stack

- **Tauri v2**: Rust backend, React + Vite + TypeScript frontend, plain CSS
- Sidecar binaries, spawned as processes (not linked): `cd-paranoia` (libcdio), `flac`, `ffmpeg`
- In-process Rust crates for metadata: `lofty` (tags), `image` (covers). Sidecars are for ripping and audio encoding only
- macOS first (Apple Silicon). Windows later, once all functionality works. No Linux

## Commands

- `npm run tauri dev`: run the app
- `npm test`, `npm run build`: frontend tests, type-check and bundle
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`: run in `src-tauri/`. Needs `dist/` from `npm run build`
- CI runs all of the above on macOS and Windows
- `scripts/drive-app.sh start|key|pick|shot|stop`: launch the real app, press keys, screenshot its window. Use it to confirm UI changes, and read the screenshots. Needs Accessibility and Screen Recording for VS Code. The webview exposes no text to macOS accessibility, so screenshots are the only way to read the UI

## Milestones

1. **Fix-my-library mode:** scan a folder, flag Echo Mini problems, write repaired *copies* (never modify originals), sync to device. No drive code
2. Minimal menu UI shell around that flow
3. Ripping on Mac: TOC → MusicBrainz disc ID lookup → cd-paranoia rip (+ AccurateRip) → encode
4. Windows port

## Echo Mini output profile

- **Format:** FLAC 16/44.1, no transcode (CD source). MP3 V0 optional for the 8 GB internal storage
- **Cover art:** square JPEG only. Centre-crop, ~600×600, baseline (non-progressive). Embed + write `cover.jpg` in the album folder. Baseline/size are unverified assumptions; test on device
- **Tags:** whitelist only. Unsupported ID3v2 frames can hide supported ones. MP3 → **ID3v2.3** (v2.4 reportedly breaks art)
- **Lyrics:** `.lrc` next to each track, same base name (fetch from LRCLIB, synced only: unsynced text isn't written, as a `.lrc` needs timestamps)
- **Filesystem:** FAT32/exFAT (no NTFS). Strip illegal chars, keep paths short, normalise Unicode (Polish/Belarusian diacritics)
- **Layout:** `Artist/Year - Album/NN Title.flac`. Device browses by folder, so the layout is the UI. Multi-disc: `CD1/CD2` subfolders or a disc prefix
- **Unknown:** whether the player reads ReplayGain

## Architecture rules

- All platform-specific code behind one interface: `listDrives()`, `readToc()`, `ripTrack()`, `ejectDisc()`, `findPlayerVolumes()`. UI never knows the OS
- No hardcoded paths (`/Volumes/...` on Mac vs drive letters on Windows)
- Device profiles are pluggable (Echo Mini first; Rockbox iPod etc. later)
- Never write to the user's original files. Repair refuses an output folder overlapping the source, and never replaces a file already in the output

## macOS gotchas

- macOS auto-mounts audio CDs (AIFF files). This may hold the device; `diskutil unmount` before cd-paranoia, handle remount
- Copying the auto-mounted AIFFs skips error correction. Always rip with cd-paranoia
- Sidecars need arm64 builds (universal if shared). Signing/notarization only when distributing

## UI design (v3 "Minimal terminal")

- Colours: ground `#DAD4BC` with faint scanlines, ink `#3E3B33`, panels `#E3DECA`, unselected rows `#CFC8AE`
- 1px hairlines; no shadows, gradients or illustrations
- Menu rows with a square marker; selected row inverts to a solid ink bar
- Layout: menu list + detail panel with a dark title bar
- Footer key hints (`↵ SELECT`, `ESC BACK`, `⌘E EJECT`). **Keyboard-first:** arrow keys move through menu rows
- Fonts: Jost (light, wide tracking) for headings/menus; IBM Plex Mono for data, paths, logs
- Respect `prefers-reduced-motion`
- Inspired by NieR:Automata menus. Influence only: no game assets, logos or fonts

## Conventions

- Git author: Miko Zyzanski; default branch `main`
- Commit in small, working steps
