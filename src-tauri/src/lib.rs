use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::Command,
    sync::{mpsc, Mutex},
};


#[derive(Debug, Deserialize)]
struct ExtensionLink {
    url: String,
}

async fn start_extension_bridge(app: AppHandle) {
    let listener = match TcpListener::bind("127.0.0.1:47653").await {
        Ok(listener) => listener,
        Err(_) => return,
    };

    loop {
        let Ok((mut stream, _)) = listener.accept().await else { continue };
        let app = app.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0_u8; 16 * 1024];
            let Ok(size) = stream.read(&mut buffer).await else { return };
            if size == 0 { return; }
            let request = String::from_utf8_lossy(&buffer[..size]);
            let header_end = request.find("\r\n\r\n").unwrap_or(request.len());
            let headers = &request[..header_end];

            if headers.starts_with("OPTIONS ") {
                let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: http://localhost\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }
            if headers.starts_with("GET /health ") {
                let body = serde_json::json!({"ok":true,"service":"CypherLinks","version":env!("CARGO_PKG_VERSION")}).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes()).await; return;
            }
            if headers.starts_with("GET /queue ") {
                let state = app.state::<DownloadState>();
                let queue = state.queue.lock().await.clone();
                let body = serde_json::json!({"queued":queue.len(),"active":state.active.load(Ordering::Relaxed),"items":queue}).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes()).await; return;
            }

            if !headers.starts_with("POST /add ") {
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }

            let body = request.get(header_end + 4..).unwrap_or("");
            let link = serde_json::from_str::<ExtensionLink>(body).ok();
            let valid = link.as_ref().map(|v| v.url.starts_with("http://") || v.url.starts_with("https://")).unwrap_or(false);
            if let Some(link) = link.filter(|_| valid) {
                let _ = app.emit("extension-url", serde_json::json!({ "url": link.url }));
                let body = r#"{"ok":true}"#;
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes()).await;
            } else {
                let body = r#"{"ok":false}"#;
                let response = format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
    }
}

#[derive(Clone)]
struct DownloadState {
    cancellation: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    queue: Arc<Mutex<Vec<DownloadRequest>>>,
    inflight: Arc<Mutex<HashMap<String, DownloadRequest>>>,
    active: Arc<AtomicUsize>,
    max_concurrent: Arc<AtomicUsize>,
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            cancellation: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(Mutex::new(Vec::new())),
            inflight: Arc::new(Mutex::new(HashMap::new())),
            active: Arc::new(AtomicUsize::new(0)),
            max_concurrent: Arc::new(AtomicUsize::new(2)),
        }
    }
}

#[derive(Debug, Serialize)]
struct FormatInfo {
    format_id: String,
    ext: Option<String>,
    height: Option<u64>,
    fps: Option<f64>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    vcodec: Option<String>,
    acodec: Option<String>,
}

#[derive(Debug, Serialize)]
struct VideoInfo {
    id: String,
    title: String,
    uploader: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    webpage_url: Option<String>,
    formats: Vec<FormatInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadRequest {
    id: String,
    url: String,
    output_dir: String,
    mode: String,
    quality: String,
    subtitles: bool,
    #[serde(default)]
    playlist: bool,
    #[serde(default)]
    cookies_browser: Option<String>,
    #[serde(default)]
    embed_metadata: bool,
    #[serde(default)]
    embed_thumbnail: bool,
    #[serde(default)]
    archive_path: Option<String>,
    #[serde(default)]
    filename_template: Option<String>,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    limit_rate: Option<String>,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default = "default_duplicate_policy")]
    duplicate_policy: String,
    #[serde(default)]
    split_chapters: bool,
    #[serde(default = "default_transcode_preset")]
    transcode_preset: String,
    #[serde(default = "default_post_action")]
    post_action: String,
    #[serde(default)]
    scheduled_at_ms: Option<u64>,
    #[serde(default)]
    checksum_sha256: Option<String>,
    #[serde(default)]
    provider: Option<String>,
}



#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadRule {
    id: String,
    host_pattern: String,
    #[serde(default)] quality: Option<String>,
    #[serde(default)] mode: Option<String>,
    #[serde(default)] limit_rate: Option<String>,
    #[serde(default)] priority: Option<i32>,
    #[serde(default)] transcode_preset: Option<String>,
    #[serde(default)] enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfig {
    #[serde(default)] rules: Vec<DownloadRule>,
    #[serde(default)] domain_rate_limits: HashMap<String, String>,
    #[serde(default)] provider_adapters: HashMap<String, Vec<String>>,
    #[serde(default)] telemetry_enabled: bool,
}

fn app_data_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("portable.flag").exists() {
                let dir = parent.join("cypherlinks-data");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                return Ok(dir);
            }
        }
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> { Ok(app_data_root(app)?.join("runtime-config.json")) }
fn queue_path(app: &AppHandle) -> Result<PathBuf, String> { Ok(app_data_root(app)?.join("queue-state.json")) }
fn telemetry_path(app: &AppHandle) -> Result<PathBuf, String> { Ok(app_data_root(app)?.join("telemetry.jsonl")) }

fn load_runtime_config_file(app: &AppHandle) -> RuntimeConfig {
    config_path(app).ok().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_runtime_config_file(app: &AppHandle, config: &RuntimeConfig) -> Result<(), String> {
    let path = config_path(app)?;
    std::fs::write(path, serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

async fn persist_queue(app: &AppHandle, state: &DownloadState) -> Result<(), String> {
    let mut requests = state.queue.lock().await.clone();
    requests.extend(state.inflight.lock().await.values().cloned());
    requests.sort_by(|a,b| a.id.cmp(&b.id)); requests.dedup_by(|a,b| a.id == b.id);
    std::fs::write(queue_path(app)?, serde_json::to_vec_pretty(&requests).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn host_from_url(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").split(':').next().unwrap_or("").trim_start_matches("www.").to_ascii_lowercase()
}

fn host_matches(host: &str, pattern: &str) -> bool {
    let p = pattern.trim().trim_start_matches("*.").to_ascii_lowercase();
    !p.is_empty() && (host == p || host.ends_with(&format!(".{p}")))
}

fn apply_runtime_rules(app: &AppHandle, mut request: DownloadRequest) -> DownloadRequest {
    let config = load_runtime_config_file(app);
    let host = host_from_url(&request.url);
    if request.limit_rate.as_deref().unwrap_or("").is_empty() {
        if let Some(rate) = config.domain_rate_limits.get(&host) { request.limit_rate = Some(rate.clone()); }
    }
    for rule in config.rules.iter().filter(|r| r.enabled && host_matches(&host, &r.host_pattern)) {
        if let Some(v) = &rule.quality { request.quality = v.clone(); }
        if let Some(v) = &rule.mode { request.mode = v.clone(); }
        if let Some(v) = &rule.limit_rate { request.limit_rate = Some(v.clone()); }
        if let Some(v) = rule.priority { request.priority = v.clamp(-10, 10); }
        if let Some(v) = &rule.transcode_preset { request.transcode_preset = v.clone(); }
    }
    if request.provider.is_none() {
        request.provider = config.provider_adapters.keys().find(|pattern| host_matches(&host, pattern)).cloned();
    }
    request
}

fn record_telemetry(app: &AppHandle, event: &str) {
    let config = load_runtime_config_file(app);
    if !config.telemetry_enabled { return; }
    let Ok(path) = telemetry_path(app) else { return; };
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let entry = serde_json::json!({"event": event, "timestampMs": unix_time_ms(), "version": env!("CARGO_PKG_VERSION")});
        let _ = writeln!(file, "{}", entry);
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop { let n = file.read(&mut buffer).map_err(|e| e.to_string())?; if n == 0 { break; } hasher.update(&buffer[..n]); }
    Ok(format!("{:x}", hasher.finalize()))
}

fn default_duplicate_policy() -> String { "skip".into() }
fn default_transcode_preset() -> String { "source".into() }
fn default_post_action() -> String { "none".into() }
fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


#[derive(Debug, Serialize, Clone)]
struct ProgressPayload {
    id: String,
    status: String,
    percent: Option<f64>,
    speed: Option<String>,
    eta: Option<String>,
    filename: Option<String>,
    message: Option<String>,
}

fn yt_dlp_command() -> Result<(String, Vec<String>), String> {
    if let Ok(path) = which::which("yt-dlp") {
        return Ok((path.to_string_lossy().to_string(), vec![]));
    }
    if let Ok(path) = which::which("yt_dlp") {
        return Ok((path.to_string_lossy().to_string(), vec![]));
    }
    if let Ok(python) = which::which("python3").or_else(|_| which::which("python")) {
        return Ok((
            python.to_string_lossy().to_string(),
            vec!["-m".into(), "yt_dlp".into()],
        ));
    }
    Err("yt-dlp is not installed. Run the included setup script first.".into())
}

fn ffmpeg_available() -> bool {
    which::which("ffmpeg").is_ok()
}

#[tauri::command]
async fn analyze_url(url: String, playlist: Option<bool>) -> Result<VideoInfo, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Please enter a valid http(s) URL.".into());
    }

    let (program, mut prefix) = yt_dlp_command()?;
    prefix.extend([
        "--dump-single-json".into(),
        if playlist.unwrap_or(false) { "--yes-playlist".into() } else { "--no-playlist".into() },
        "--no-warnings".into(),
        url,
    ]);

    let output = Command::new(program)
        .args(prefix)
        .output()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Could not analyze this URL.".into()
        } else {
            stderr
        });
    }

    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("yt-dlp returned invalid metadata: {e}"))?;

    let formats = value
        .get("formats")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|f| FormatInfo {
                    format_id: f.get("format_id").and_then(Value::as_str).unwrap_or("").to_string(),
                    ext: f.get("ext").and_then(Value::as_str).map(str::to_string),
                    height: f.get("height").and_then(Value::as_u64),
                    fps: f.get("fps").and_then(Value::as_f64),
                    filesize: f.get("filesize").and_then(Value::as_u64),
                    filesize_approx: f.get("filesize_approx").and_then(Value::as_u64),
                    vcodec: f.get("vcodec").and_then(Value::as_str).map(str::to_string),
                    acodec: f.get("acodec").and_then(Value::as_str).map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(VideoInfo {
        id: value.get("id").and_then(Value::as_str).unwrap_or("unknown").to_string(),
        title: value.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string(),
        uploader: value.get("uploader").and_then(Value::as_str).map(str::to_string),
        duration: value.get("duration").and_then(Value::as_f64),
        thumbnail: value.get("thumbnail").and_then(Value::as_str).map(str::to_string),
        webpage_url: value.get("webpage_url").and_then(Value::as_str).map(str::to_string),
        formats,
    })
}


fn spawn_open(target: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("explorer");
        c.arg(target);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(target);
        c
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    command.spawn().map_err(|e| format!("Could not open folder: {e}"))?;
    Ok(())
}

#[tauri::command]
fn open_extension_folder(app: AppHandle) -> Result<String, String> {
    let development = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("browser-extension");
    let bundled = app.path().resource_dir().map_err(|e| e.to_string())?.join("browser-extension");
    let folder = if development.exists() { development } else { bundled };
    if !folder.exists() {
        return Err("The bundled browser-extension folder could not be found.".into());
    }
    spawn_open(&folder)?;
    Ok(folder.to_string_lossy().to_string())
}

#[tauri::command]
async fn get_clipboard_text() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", "Get-Clipboard -Raw"]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = Command::new("pbpaste");
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = Command::new("sh");
        c.args(["-c", "command -v wl-paste >/dev/null && wl-paste -n || command -v xclip >/dev/null && xclip -selection clipboard -o || command -v xsel >/dev/null && xsel --clipboard --output"]);
        c
    };

    let output = command.output().await.map_err(|e| format!("Could not read clipboard: {e}"))?;
    if !output.status.success() {
        return Err("Clipboard integration is unavailable on this system.".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tauri::command]
fn default_download_dir() -> String {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

fn emit(app: &AppHandle, payload: ProgressPayload) {
    let _ = app.emit("download-progress", payload);
}

fn parse_progress(line: &str) -> Option<(f64, Option<String>, Option<String>)> {
    // yt-dlp emits our machine-readable template as:
    // LF_PROGRESS| 42.1%|12.3MiB/s|00:08
    if !line.contains("LF_PROGRESS|") {
        return None;
    }
    let raw = line.split("LF_PROGRESS|").nth(1)?.trim();
    let mut parts = raw.split('|');
    let percent = parts
        .next()?
        .trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()?;
    let speed = parts.next().map(str::trim).filter(|v| !v.is_empty() && *v != "NA").map(str::to_string);
    let eta = parts.next().map(str::trim).filter(|v| !v.is_empty() && *v != "NA").map(str::to_string);
    Some((percent, speed, eta))
}

fn output_filename(line: &str) -> Option<String> {
    let re = Regex::new(r#"\[download\] Destination: (.+)$"#).ok()?;
    re.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

fn final_filepath(line: &str) -> Option<String> {
    line.split_once("LF_FILE|").map(|(_, path)| path.trim().to_string()).filter(|path| !path.is_empty())
}

fn unique_variant_path(input: &Path, label: &str, extension: &str, overwrite: bool) -> PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("media");
    let first = parent.join(format!("{stem} [{label}].{extension}"));
    if overwrite || !first.exists() {
        return first;
    }
    for index in 2..10_000 {
        let candidate = parent.join(format!("{stem} [{label} {index}].{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem} [{label} copy].{extension}"))
}

async fn transcode_media(input: &str, preset: &str, overwrite: bool) -> Result<Option<String>, String> {
    if preset == "source" || preset.trim().is_empty() {
        return Ok(None);
    }
    if !ffmpeg_available() {
        return Err("FFmpeg is required for transcoding presets.".into());
    }

    let input_path = PathBuf::from(input);
    if !input_path.exists() {
        return Err("Downloaded file could not be found for transcoding.".into());
    }

    let (label, extension, args): (&str, &str, Vec<&str>) = match preset {
        "phone" => ("phone", "mp4", vec![
            "-map", "0:v:0?", "-map", "0:a:0?",
            "-vf", "scale=1280:-2:force_original_aspect_ratio=decrease",
            "-c:v", "libx264", "-preset", "medium", "-crf", "23",
            "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart",
        ]),
        "desktop" => ("desktop", "mp4", vec![
            "-map", "0:v:0?", "-map", "0:a:0?",
            "-c:v", "libx264", "-preset", "medium", "-crf", "19",
            "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart",
        ]),
        "archive" => ("archive", "mkv", vec![
            "-map", "0:v:0?", "-map", "0:a:0?",
            "-c:v", "libx264", "-preset", "slow", "-crf", "18", "-c:a", "flac",
        ]),
        "audio-library" => ("audio library", "m4a", vec![
            "-vn", "-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart",
        ]),
        _ => return Err("Unknown transcode preset.".into()),
    };

    let output = unique_variant_path(&input_path, label, extension, overwrite);
    let mut command = Command::new("ffmpeg");
    command.arg(if overwrite { "-y" } else { "-n" });
    command.arg("-i").arg(&input_path);
    command.args(args);
    command.arg(&output);
    let result = command.output().await.map_err(|e| format!("Could not start FFmpeg: {e}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let tail = stderr.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(format!("FFmpeg transcoding failed: {tail}"));
    }
    Ok(Some(output.to_string_lossy().to_string()))
}


async fn perform_post_action(path: &str, action: &str) -> Result<(), String> {
    if action == "none" || action.trim().is_empty() {
        return Ok(());
    }
    let file = PathBuf::from(path);
    let target = if action == "open-folder" {
        file.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
    } else {
        file.clone()
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("explorer");
        c.arg(&target);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg(&target);
        c
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = Command::new("xdg-open");
        c.arg(&target);
        c
    };

    command.spawn().map_err(|e| format!("Could not run post-download action: {e}"))?;
    Ok(())
}

async fn run_download(app: AppHandle, request: DownloadRequest, cancel: Arc<AtomicBool>) -> Result<(), String> {
    if !ffmpeg_available() {
        return Err("FFmpeg is not installed. Run the included setup script first.".into());
    }

    let (program, mut args) = yt_dlp_command()?;
    let requested_template = request.filename_template.as_deref().unwrap_or("%(title).180B [%(id)s].%(ext)s").trim();
    if requested_template.is_empty() || requested_template.len() > 220 || requested_template.contains("..") || requested_template.contains('/') || requested_template.contains('\\') {
        return Err("Filename template must be a simple filename under 220 characters.".into());
    }
    let mut output_template = if requested_template.contains("%(ext)s") {
        requested_template.to_string()
    } else {
        format!("{requested_template}.%(ext)s")
    };
    if request.duplicate_policy == "keep" {
        output_template = if let Some(stem) = output_template.strip_suffix(".%(ext)s") {
            format!("{stem} [%(epoch)s].%(ext)s")
        } else {
            format!("{output_template} [%(epoch)s]")
        };
    }

    args.extend([
        "--newline".into(),
        if request.playlist { "--yes-playlist".into() } else { "--no-playlist".into() },
        "--progress".into(),
        "--progress-template".into(),
        "download:LF_PROGRESS|%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s".into(),
        "--paths".into(),
        request.output_dir.clone(),
        "--output".into(),
        output_template,
        "--windows-filenames".into(),
        "--print".into(),
        "after_move:LF_FILE|%(filepath)s".into(),
    ]);

    if let Some(browser) = request.cookies_browser.as_ref().filter(|v| !v.is_empty() && *v != "none") {
        args.extend(["--cookies-from-browser".into(), browser.clone()]);
    }

    if let Some(proxy) = request.proxy.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
        let lower = proxy.to_ascii_lowercase();
        let allowed = ["http://", "https://", "socks4://", "socks5://", "socks5h://"].iter().any(|prefix| lower.starts_with(prefix));
        if !allowed {
            return Err("Proxy must use http, https, socks4, socks5, or socks5h.".into());
        }
        args.extend(["--proxy".into(), proxy.to_string()]);
    }

    if let Some(limit) = request.limit_rate.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
        let valid = Regex::new(r"(?i)^\d+(?:\.\d+)?[kmgt]?$").map_err(|e| e.to_string())?;
        if !valid.is_match(limit) {
            return Err("Bandwidth limit must look like 500K, 2M, or 1.5G.".into());
        }
        args.extend(["--limit-rate".into(), limit.to_string()]);
    }

    if request.mode == "audio" {
        args.extend([
            "--extract-audio".into(),
            "--audio-format".into(),
            "mp3".into(),
            "--audio-quality".into(),
            "0".into(),
        ]);
    } else {
        let selector = if request.quality == "best" {
            "bv*[vcodec^=avc1]+ba[acodec^=mp4a]/bv*+ba/b".to_string()
        } else {
            format!("bv*[height<={0}][vcodec^=avc1]+ba[acodec^=mp4a]/bv*[height<={0}]+ba/b[height<={0}]/b", request.quality)
        };
        args.extend([
            "--format".into(),
            selector,
            "--merge-output-format".into(),
            "mp4/mkv".into(),
        ]);
    }

    if request.embed_metadata {
        args.push("--embed-metadata".into());
    }
    if request.embed_thumbnail {
        args.push("--embed-thumbnail".into());
    }

    if request.split_chapters {
        args.extend(["--split-chapters".into(), "--force-keyframes-at-cuts".into()]);
    }

    if request.subtitles {
        args.extend([
            "--write-subs".into(),
            "--write-auto-subs".into(),
            "--sub-langs".into(),
            "en.*,en".into(),
        ]);
    }

    match request.duplicate_policy.as_str() {
        "overwrite" => args.push("--force-overwrites".into()),
        "keep" => {},
        _ => {
            args.push("--no-overwrites".into());
            if let Some(archive) = request.archive_path.as_ref().filter(|v| !v.is_empty()) {
                args.extend(["--download-archive".into(), archive.clone()]);
            }
        }
    }
    args.extend(["--continue".into(), "--retries".into(), "10".into(), "--fragment-retries".into(), "10".into()]);
    if let Some(provider) = request.provider.as_ref() {
        let config = load_runtime_config_file(&app);
        if let Some(extra) = config.provider_adapters.get(provider) { args.extend(extra.clone()); }
    }

    args.push(request.url.clone());

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;

    let stdout = child.stdout.take().ok_or("Could not read yt-dlp stdout")?;
    let stderr = child.stderr.take().ok_or("Could not read yt-dlp stderr")?;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(line);
            }
        });
    }
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(line);
            }
        });
    }
    drop(tx);

    let mut last_filename: Option<String> = None;
    let mut final_error: Option<String> = None;

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            emit(&app, ProgressPayload {
                id: request.id.clone(), status: "cancelled".into(), percent: None,
                speed: None, eta: None, filename: last_filename, message: Some("Cancelled".into()),
            });
            return Ok(());
        }

        tokio::select! {
            maybe_line = rx.recv() => {
                if let Some(line) = maybe_line {
                    if let Some(path) = final_filepath(&line) {
                        last_filename = Some(path);
                    } else if let Some(name) = output_filename(&line) {
                        last_filename = Some(name);
                    }
                    if let Some((percent, speed, eta)) = parse_progress(&line) {
                        emit(&app, ProgressPayload {
                            id: request.id.clone(), status: "downloading".into(), percent: Some(percent),
                            speed, eta, filename: last_filename.clone(), message: None,
                        });
                    } else if line.contains("[Merger]") || line.contains("[ExtractAudio]") || line.contains("[VideoConvertor]") || line.contains("[Fixup") {
                        emit(&app, ProgressPayload {
                            id: request.id.clone(), status: "processing".into(), percent: Some(100.0),
                            speed: None, eta: None, filename: last_filename.clone(), message: Some("Finalizing media".into()),
                        });
                    } else if line.to_ascii_lowercase().contains("error:") {
                        final_error = Some(line.trim().to_string());
                    }
                }
            }
            status = child.wait() => {
                let status = status.map_err(|e| format!("yt-dlp process failed: {e}"))?;
                if status.success() {
                    let mut final_name = last_filename.clone();
                    if request.transcode_preset != "source" {
                        let source = final_name.clone().ok_or("yt-dlp did not report the final media path.")?;
                        emit(&app, ProgressPayload {
                            id: request.id.clone(), status: "processing".into(), percent: Some(100.0),
                            speed: None, eta: None, filename: final_name.clone(), message: Some(format!("Creating {} preset", request.transcode_preset)),
                        });
                        if let Some(converted) = transcode_media(&source, &request.transcode_preset, request.duplicate_policy == "overwrite").await? {
                            final_name = Some(converted);
                        }
                    }
                    if let Some(path) = final_name.as_deref() {
                        if let Err(action_error) = perform_post_action(path, &request.post_action).await {
                            emit(&app, ProgressPayload {
                                id: request.id.clone(), status: "processing".into(), percent: Some(100.0),
                                speed: None, eta: None, filename: final_name.clone(), message: Some(action_error),
                            });
                        }
                    }
                    if let (Some(expected), Some(path)) = (request.checksum_sha256.as_deref(), final_name.as_deref()) {
                        let actual = sha256_file(Path::new(path))?;
                        if !expected.trim().eq_ignore_ascii_case(&actual) {
                            return Err(format!("SHA-256 verification failed. Expected {}, got {}", expected.trim(), actual));
                        }
                    }
                    record_telemetry(&app, "download_completed");
                    emit(&app, ProgressPayload {
                        id: request.id.clone(), status: "finished".into(), percent: Some(100.0),
                        speed: None, eta: None, filename: final_name, message: Some("Saved and verified successfully".into()),
                    });
                    return Ok(());
                }
                return Err(final_error.unwrap_or_else(|| format!("yt-dlp exited with status {status}")));
            }
        }
    }
}

fn schedule_queue_pump(app: AppHandle, state: DownloadState) {
    tauri::async_runtime::spawn(async move {
        pump_queue(app, state).await;
    });
}

async fn pump_queue(app: AppHandle, state: DownloadState) {
    loop {
        if state.active.load(Ordering::Relaxed) >= state.max_concurrent.load(Ordering::Relaxed) {
            break;
        }

        let (next, wait_ms) = {
            let mut queue = state.queue.lock().await;
            let now = unix_time_ms();
            let due_index = queue
                .iter()
                .enumerate()
                .filter(|(_, request)| request.scheduled_at_ms.unwrap_or(0) <= now)
                .max_by(|(a_index, a), (b_index, b)| {
                    a.priority.cmp(&b.priority).then_with(|| b_index.cmp(a_index))
                })
                .map(|(index, _)| index);

            if let Some(index) = due_index {
                (Some(queue.remove(index)), None)
            } else {
                let earliest = queue.iter().filter_map(|request| request.scheduled_at_ms).min();
                (None, earliest.map(|at| at.saturating_sub(now)))
            }
        };

        if let Some(request) = next.as_ref() { state.inflight.lock().await.insert(request.id.clone(), request.clone()); let _ = persist_queue(&app, &state).await; }
        let Some(request) = next else {
            if let Some(wait) = wait_ms {
                let delayed_app = app.clone();
                let delayed_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(wait.max(1))).await;
                    schedule_queue_pump(delayed_app, delayed_state);
                });
            }
            break;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        state.cancellation.lock().await.insert(request.id.clone(), cancel.clone());
        state.active.fetch_add(1, Ordering::Relaxed);

        let id = request.id.clone();
        let task_app = app.clone();
        let task_state = state.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(message) = run_download(task_app.clone(), request, cancel).await {
                emit(&task_app, ProgressPayload {
                    id: id.clone(), status: "error".into(), percent: None,
                    speed: None, eta: None, filename: None, message: Some(message),
                });
            }
            task_state.cancellation.lock().await.remove(&id);
            task_state.inflight.lock().await.remove(&id);
            task_state.active.fetch_sub(1, Ordering::Relaxed);
            let _ = persist_queue(&task_app, &task_state).await;
            schedule_queue_pump(task_app, task_state);
        });
    }
}

#[tauri::command]
async fn start_download(
    app: AppHandle,
    request: DownloadRequest,
    state: State<'_, DownloadState>,
) -> Result<(), String> {
    let request = apply_runtime_rules(&app, request);
    let scheduled = request.scheduled_at_ms.map(|value| value > unix_time_ms()).unwrap_or(false);
    emit(&app, ProgressPayload {
        id: request.id.clone(), status: if scheduled { "scheduled".into() } else { "queued".into() }, percent: Some(0.0),
        speed: None, eta: None, filename: None, message: Some(if scheduled { "Scheduled".into() } else { "Waiting in queue".into() }),
    });
    state.queue.lock().await.push(request);
    persist_queue(&app, state.inner()).await?;
    record_telemetry(&app, "download_queued");
    schedule_queue_pump(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
async fn cancel_download(app: AppHandle, id: String, state: State<'_, DownloadState>) -> Result<(), String> {
    {
        let mut queue = state.queue.lock().await;
        if let Some(index) = queue.iter().position(|request| request.id == id) {
            queue.remove(index);
            emit(&app, ProgressPayload {
                id, status: "cancelled".into(), percent: None,
                speed: None, eta: None, filename: None, message: Some("Removed from queue".into()),
            });
            return Ok(());
        }
    }

    if let Some(flag) = state.cancellation.lock().await.get(&id) {
        flag.store(true, Ordering::Relaxed);
        Ok(())
    } else {
        Err("Download is no longer active.".into())
    }
}

#[tauri::command]
fn set_max_concurrent(app: AppHandle, value: usize, state: State<'_, DownloadState>) -> Result<usize, String> {
    let value = value.clamp(1, 6);
    state.max_concurrent.store(value, Ordering::Relaxed);
    schedule_queue_pump(app, state.inner().clone());
    Ok(value)
}

#[tauri::command]
async fn update_queued_priority(app: AppHandle, id: String, priority: i32, state: State<'_, DownloadState>) -> Result<(), String> {
    let mut queue = state.queue.lock().await;
    if let Some(request) = queue.iter_mut().find(|request| request.id == id) {
        request.priority = priority.clamp(-10, 10);
        drop(queue);
        schedule_queue_pump(app, state.inner().clone());
        Ok(())
    } else {
        Err("Only queued or scheduled downloads can be reprioritized.".into())
    }
}

#[tauri::command]
async fn install_dependencies() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    let mut cmd = { let mut c = Command::new("powershell"); c.args(["-NoProfile", "-Command", "winget install --id yt-dlp.yt-dlp -e --accept-source-agreements --accept-package-agreements; winget install --id Gyan.FFmpeg -e --accept-source-agreements --accept-package-agreements"]); c };
    #[cfg(target_os = "macos")]
    let mut cmd = { let mut c = Command::new("brew"); c.args(["install", "yt-dlp", "ffmpeg"]); c };
    #[cfg(target_os = "linux")]
    let mut cmd = { let mut c = Command::new("sh"); c.args(["-c", "python3 -m pip install --user -U yt-dlp && (command -v ffmpeg >/dev/null || echo 'Install ffmpeg with your distribution package manager')"]); c };
    let output = cmd.output().await.map_err(|e| format!("Dependency installer could not start: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if output.status.success() { Ok(text.trim().to_string()) } else { Err(text.trim().to_string()) }
}

#[tauri::command]
async fn update_downloader() -> Result<String, String> {
    let (program, mut args) = yt_dlp_command()?;
    args.push("-U".into());
    let output = Command::new(program).args(args).output().await.map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if output.status.success() { Ok(text.trim().to_string()) } else { Err(text.trim().to_string()) }
}



#[tauri::command]
fn ingest_dropped_paths(paths: Vec<String>) -> Result<serde_json::Value, String> {
    let url_re = Regex::new(r#"https?://[^\\s<>\"']+"#).map_err(|e| e.to_string())?;
    let media_exts = ["mp4", "mkv", "webm", "mov", "avi", "mp3", "m4a", "aac", "wav", "flac", "ogg"];
    let mut urls = Vec::new();
    let mut media = Vec::new();
    for raw in paths {
        let path = PathBuf::from(&raw);
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_lowercase();
        if media_exts.contains(&ext.as_str()) {
            media.push(path.to_string_lossy().to_string());
        } else if ["txt", "url", "webloc"].contains(&ext.as_str()) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                urls.extend(url_re.find_iter(&text).map(|m| m.as_str().trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | '.')).to_string()));
            }
        }
    }
    urls.sort(); urls.dedup();
    Ok(serde_json::json!({ "urls": urls, "media": media }))
}

#[tauri::command]
fn report_error(app: AppHandle, source: String, message: String, details: Option<String>) -> Result<String, String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("cypherlinks-diagnostics.log");
    let timestamp = unix_time_ms();
    let safe_source = source.replace(['\n', '\r'], " ");
    let safe_message = message.replace(['\n', '\r'], " ");
    let entry = format!("[{timestamp}] source={safe_source} message={safe_message}\n{}\n---\n", details.unwrap_or_default());
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    file.write_all(entry.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn open_diagnostics_folder(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    spawn_open(&dir)?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
fn dependency_status() -> serde_json::Value {
    serde_json::json!({
        "ytDlp": yt_dlp_command().is_ok(),
        "ffmpeg": ffmpeg_available(),
    })
}


#[tauri::command]
fn load_runtime_config(app: AppHandle) -> RuntimeConfig { load_runtime_config_file(&app) }

#[tauri::command]
fn save_runtime_config(app: AppHandle, config: RuntimeConfig) -> Result<(), String> { save_runtime_config_file(&app, &config) }

#[tauri::command]
fn export_runtime_config(app: AppHandle, destination: String) -> Result<String, String> {
    let source = config_path(&app)?; if !source.exists() { save_runtime_config_file(&app, &RuntimeConfig::default())?; }
    std::fs::copy(source, &destination).map_err(|e| e.to_string())?; Ok(destination)
}

#[tauri::command]
fn import_runtime_config(app: AppHandle, source: String) -> Result<RuntimeConfig, String> {
    let raw = std::fs::read_to_string(&source).map_err(|e| e.to_string())?;
    let config: RuntimeConfig = serde_json::from_str(&raw).map_err(|e| format!("Invalid CypherLinks settings file: {e}"))?;
    save_runtime_config_file(&app, &config)?; Ok(config)
}

#[tauri::command]
fn portable_mode_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let marker = exe.parent().unwrap_or(Path::new(".")).join("portable.flag");
    Ok(serde_json::json!({"enabled": marker.exists(), "dataPath": app_data_root(&app)?.to_string_lossy()}))
}

#[tauri::command]
fn set_portable_mode(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let marker = exe.parent().unwrap_or(Path::new(".")).join("portable.flag");
    if enabled { std::fs::write(marker, b"CypherLinks portable mode
").map_err(|e| e.to_string())?; }
    else if marker.exists() { std::fs::remove_file(marker).map_err(|e| e.to_string())?; }
    Ok(())
}

#[tauri::command]
fn telemetry_summary(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = telemetry_path(&app)?; let raw = std::fs::read_to_string(path).unwrap_or_default();
    let mut counts: HashMap<String,u64> = HashMap::new();
    for line in raw.lines() { if let Ok(v)=serde_json::from_str::<Value>(line) { if let Some(e)=v.get("event").and_then(Value::as_str) { *counts.entry(e.to_string()).or_insert(0)+=1; } } }
    Ok(serde_json::json!({"events": counts, "localOnly": true}))
}

#[tauri::command]
fn compute_sha256(path: String) -> Result<String, String> { sha256_file(Path::new(&path)) }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(DownloadState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<DownloadState>().inner().clone();
            if let Ok(path) = queue_path(&handle) {
                if let Ok(raw) = std::fs::read_to_string(path) {
                    if let Ok(restored) = serde_json::from_str::<Vec<DownloadRequest>>(&raw) {
                        let state_clone = state.clone();
                        tauri::async_runtime::spawn(async move { *state_clone.queue.lock().await = restored; schedule_queue_pump(handle.clone(), state_clone); });
                    }
                }
            }
            tauri::async_runtime::spawn(start_extension_bridge(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            analyze_url,
            get_clipboard_text,
            open_extension_folder,
            default_download_dir,
            start_download,
            cancel_download,
            set_max_concurrent,
            update_queued_priority,
            update_downloader,
            dependency_status,
            install_dependencies,
            ingest_dropped_paths,
            report_error,
            open_diagnostics_folder,
            load_runtime_config,
            save_runtime_config,
            export_runtime_config,
            import_runtime_config,
            portable_mode_status,
            set_portable_mode,
            telemetry_summary,
            compute_sha256
        ])
        .run(tauri::generate_context!())
        .expect("error while running CypherLinks");
}
