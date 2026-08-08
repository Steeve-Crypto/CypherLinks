# LinkForge

A local-first desktop media downloader built with Tauri 2, React, TypeScript, Rust, yt-dlp, and FFmpeg.

## What works

- Analyze a supported media URL before downloading
- Shows title, uploader, duration, thumbnail, and available resolutions
- Video mode with Best / 2160p / 1440p / 1080p / 720p / 480p / 360p choices
- Audio-only MP3 extraction
- Optional English subtitles / auto-subs when available
- Native folder picker
- Live progress, speed, ETA, post-processing state, success/error state
- Multiple concurrent downloads
- Real cancellation of active downloads
- Local output only; no server or account required
- No DRM/access-control bypass logic

## Prerequisites

You need:

1. Node.js 20+
2. Rust stable toolchain
3. yt-dlp
4. FFmpeg
5. Tauri's OS build prerequisites

### macOS

```bash
xcode-select --install
brew install node rust yt-dlp ffmpeg
```

### Windows (PowerShell)

```powershell
winget install OpenJS.NodeJS.LTS
winget install Rustlang.Rustup
winget install yt-dlp.yt-dlp
winget install Gyan.FFmpeg
```

Install Microsoft C++ Build Tools if Tauri prompts for them.

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

## Build a desktop installer

```bash
npm run tauri build
```

Tauri will place platform-specific bundles under `src-tauri/target/release/bundle/`.

## Architecture

```text
React + TypeScript UI
        │
        │ Tauri commands + events
        ▼
Rust process manager
  ├── URL validation / metadata normalization
  ├── async download lifecycle
  ├── cancellation flags
  └── progress streaming
        │
        ├── yt-dlp
        └── FFmpeg
```

`yt-dlp` is invoked as an external process. LinkForge first looks for a `yt-dlp` executable on PATH and otherwise tries `python -m yt_dlp`.

## Usage note

Use LinkForge only for media you own, public-domain/permitted media, or content you otherwise have authorization to download. It intentionally contains no code for bypassing DRM, paywalls, authentication, or other access controls.
