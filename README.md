# CypherLinks

CypherLinks is a local-first desktop media downloader built with **Tauri 2, React, TypeScript, Rust, yt-dlp, and FFmpeg**. It is designed for media you own, public-domain/permitted media, or content you otherwise have authorization to download. It does not contain DRM or access-control bypass logic.

## Current feature set

### Download workspace

- Analyze supported media URLs before downloading
- Video and audio-only output modes
- Best / 2160p / 1440p / 1080p / 720p / 480p / 360p selection
- Playlist and channel downloads
- Batch URL queueing
- English subtitles and auto-caption downloads
- Browser-cookie support for content you are authorized to access
- Metadata and thumbnail embedding
- Custom yt-dlp filename templates
- Native destination-folder picker
- Scheduled downloads handled by the Rust queue
- Queue priority: High / Normal / Low
- Bandwidth limiting such as `500K`, `5M`, or `1.5G`
- Optional HTTP/HTTPS/SOCKS proxy routing
- Duplicate policy: skip known media, keep both, or overwrite
- Automatic chapter splitting
- Download retry/resume behavior
- Post-download actions: open destination folder or open finished media

### Transcoding presets

FFmpeg can create an additional optimized version after the source download finishes:

- **Phone** — H.264/AAC MP4 capped around 720p
- **Desktop** — higher-quality H.264/AAC MP4
- **Archive** — high-quality MKV output
- **Audio Library** — AAC 256 kbps M4A
- **Keep source** — no additional transcoding

### Queue and history

- Priority-aware native Rust queue
- Configurable 1–6 concurrent downloads
- Reprioritize queued/scheduled jobs after they are added
- Live progress, speed, ETA, processing state, completion, and errors
- Real cancellation for queued and active jobs
- Local history persisted in the desktop frontend
- Duplicate awareness for previously completed media
- Built-in local video/audio preview player

### Clipboard integration

Enable **Settings → Clipboard detection**. CypherLinks polls the local system clipboard and offers newly copied HTTP/HTTPS links in the Download workspace. It does not automatically download clipboard contents.

### Browser extension

The `browser-extension/` directory contains a Manifest V3 Chromium extension. It sends the current tab or a context-menu link to CypherLinks through a localhost-only bridge at `127.0.0.1:47653`.

To install it in Chrome or Edge:

1. Run CypherLinks.
2. Open **Settings → Browser extension → Open extension folder**.
3. Open the browser's Extensions page.
4. Enable **Developer mode**.
5. Choose **Load unpacked**.
6. Select the `browser-extension` directory.

You can then click the CypherLinks extension button or right-click a page/link and choose **Send … to CypherLinks**.

### Per-site presets

After configuring a download profile for a URL, click **Save site preset**. CypherLinks stores the profile against the hostname and automatically reapplies it the next time that site is analyzed. Saved presets can be applied or deleted from **Settings → Site presets**.

Proxy values are intentionally not stored inside site presets.

## Prerequisites

You need:

1. Node.js 20+
2. Rust stable toolchain
3. yt-dlp
4. FFmpeg
5. Tauri's platform build prerequisites

### Windows

Run the included PowerShell helper:

```powershell
./setup-windows.ps1
```

Or install manually:

```powershell
winget install OpenJS.NodeJS.LTS
winget install Rustlang.Rustup
winget install yt-dlp.yt-dlp
winget install Gyan.FFmpeg
```

Install Microsoft C++ Build Tools if Tauri requests them.

### macOS

```bash
./setup-macos.sh
```

Or:

```bash
xcode-select --install
brew install node rust yt-dlp ffmpeg
```

### Debian / Ubuntu

```bash
sudo apt update
sudo apt install -y build-essential curl wget file libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf ffmpeg python3-pip
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
python3 -m pip install --user -U yt-dlp
```

## Run in development

```bash
npm install
npm run tauri dev
```

## Build desktop installers

```bash
npm run tauri build
```

Tauri writes platform-specific bundles under:

```text
src-tauri/target/release/bundle/
```

## Architecture

```text
React + TypeScript UI
        │
        │ Tauri commands + events
        ▼
Rust desktop backend
  ├── metadata analysis
  ├── priority / scheduling queue
  ├── cancellation and progress events
  ├── clipboard integration
  ├── localhost extension bridge
  ├── local file actions
  └── dependency management
        │
        ├── yt-dlp
        │    ├── extraction / formats
        │    ├── playlists / channels
        │    ├── subtitles / metadata
        │    ├── cookies / proxy
        │    └── retry / resume / archive
        │
        └── FFmpeg
             ├── stream merging
             ├── audio extraction
             ├── chapter splitting
             └── transcoding presets
```

## Main project directories

```text
src/                    React / TypeScript frontend
src-tauri/              Rust / Tauri desktop backend
browser-extension/      Chromium extension
setup-windows.ps1       Windows dependency helper
setup-macos.sh           macOS dependency helper
```

## Validation notes

The frontend TypeScript/TSX syntax and JSON configuration files can be validated without platform dependencies. A full Tauri native build additionally requires the Rust toolchain and the platform-specific system libraries listed above.

## Usage note

Use CypherLinks only for media you own, public-domain/permitted media, or content you otherwise have authorization to download. CypherLinks intentionally does not implement DRM, paywall, or access-control bypassing.
