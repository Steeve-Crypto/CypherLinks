use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Emitter, State};
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
                let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }

            if !headers.starts_with("POST /add ") {
                let response = "HTTP/1.1 404 Not Found\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\n\r\n";
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
    active: Arc<AtomicUsize>,
    max_concurrent: Arc<AtomicUsize>,
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            cancellation: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(Mutex::new(Vec::new())),
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

#[derive(Debug, Deserialize, Clone)]
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
async fn analyze_url(url: String) -> Result<VideoInfo, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Please enter a valid http(s) URL.".into());
    }

    let (program, mut prefix) = yt_dlp_command()?;
    prefix.extend([
        "--dump-single-json".into(),
        "--no-playlist".into(),
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

async fn run_download(app: AppHandle, request: DownloadRequest, cancel: Arc<AtomicBool>) -> Result<(), String> {
    if !ffmpeg_available() {
        return Err("FFmpeg is not installed. Run the included setup script first.".into());
    }

    let (program, mut args) = yt_dlp_command()?;
    let requested_template = request.filename_template.as_deref().unwrap_or("%(title).180B [%(id)s].%(ext)s").trim();
    if requested_template.is_empty() || requested_template.len() > 220 || requested_template.contains("..") || requested_template.contains('/') || requested_template.contains('\\') {
        return Err("Filename template must be a simple filename under 220 characters.".into());
    }
    let output_template = if requested_template.contains("%(ext)s") {
        requested_template.to_string()
    } else {
        format!("{requested_template}.%(ext)s")
    };

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
    ]);

    if let Some(browser) = request.cookies_browser.as_ref().filter(|v| !v.is_empty() && *v != "none") {
        args.extend(["--cookies-from-browser".into(), browser.clone()]);
    }

    if let Some(limit) = request.limit_rate.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
        let valid = Regex::new(r"(?i)^\d+(?:\.\d+)?[kmgt]?$ ".trim()).map_err(|e| e.to_string())?;
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

    if request.subtitles {
        args.extend([
            "--write-subs".into(),
            "--write-auto-subs".into(),
            "--sub-langs".into(),
            "en.*,en".into(),
        ]);
    }

    if let Some(archive) = request.archive_path.as_ref().filter(|v| !v.is_empty()) {
        args.extend(["--download-archive".into(), archive.clone()]);
    }
    args.extend(["--continue".into(), "--retries".into(), "10".into(), "--fragment-retries".into(), "10".into()]);

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
                    if let Some(name) = output_filename(&line) {
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
                    emit(&app, ProgressPayload {
                        id: request.id.clone(), status: "finished".into(), percent: Some(100.0),
                        speed: None, eta: None, filename: last_filename, message: Some("Saved successfully".into()),
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

        let next = {
            let mut queue = state.queue.lock().await;
            let Some((index, _)) = queue
                .iter()
                .enumerate()
                .max_by(|(a_index, a), (b_index, b)| {
                    a.priority.cmp(&b.priority).then_with(|| b_index.cmp(a_index))
                })
            else {
                break;
            };
            Some(queue.remove(index))
        };

        let Some(request) = next else { break };
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
            task_state.active.fetch_sub(1, Ordering::Relaxed);
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
    emit(&app, ProgressPayload {
        id: request.id.clone(), status: "queued".into(), percent: Some(0.0),
        speed: None, eta: None, filename: None, message: Some("Waiting in queue".into()),
    });
    state.queue.lock().await.push(request);
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
fn dependency_status() -> serde_json::Value {
    serde_json::json!({
        "ytDlp": yt_dlp_command().is_ok(),
        "ffmpeg": ffmpeg_available(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DownloadState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(start_extension_bridge(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            analyze_url,
            get_clipboard_text,
            default_download_dir,
            start_download,
            cancel_download,
            set_max_concurrent,
            update_downloader,
            dependency_status,
            install_dependencies
        ])
        .run(tauri::generate_context!())
        .expect("error while running LinkForge");
}
