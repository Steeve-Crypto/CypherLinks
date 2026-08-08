$ErrorActionPreference = "Stop"
winget install --id OpenJS.NodeJS.LTS --accept-package-agreements --accept-source-agreements
winget install --id Rustlang.Rustup --accept-package-agreements --accept-source-agreements
winget install --id yt-dlp.yt-dlp --accept-package-agreements --accept-source-agreements
winget install --id Gyan.FFmpeg --accept-package-agreements --accept-source-agreements
Write-Host "Dependencies installed. Open a new PowerShell window, cd back here, run npm install, then npm run tauri dev."
