#!/usr/bin/env bash
set -euo pipefail
command -v brew >/dev/null || { echo "Homebrew is required: https://brew.sh"; exit 1; }
brew install node rust yt-dlp ffmpeg
npm install
echo "Ready. Run: npm run tauri dev"
