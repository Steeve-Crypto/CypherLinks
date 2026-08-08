# CypherLinks Local API

CypherLinks exposes a localhost-only HTTP interface on `127.0.0.1:47653`. It is intended for browser extensions, local scripts, and desktop automation on the same machine.

## Endpoints

- `GET /health` — service and version health check.
- `GET /queue` — current queued and active job summary.
- `POST /add` — submits a URL to the running desktop application. JSON body: `{ "url": "https://…" }`.

The API binds exclusively to the IPv4 loopback interface and is not a remote-control service. Do not expose this port through a reverse proxy or public tunnel.

## Headless CLI

After building the Rust workspace, `cypherlinks-cli` provides direct scripted downloads:

```bash
cypherlinks-cli "https://example.com/media" "/path/to/output" 1080
```

The CLI expects `yt-dlp` and FFmpeg to be available in `PATH`.
