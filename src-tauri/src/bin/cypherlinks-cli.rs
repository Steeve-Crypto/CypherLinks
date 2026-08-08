use std::{env, process::{Command, exit}};

fn main() {
    let mut args = env::args().skip(1);
    let Some(url) = args.next() else {
        eprintln!("Usage: cypherlinks-cli <url> [output-directory] [quality]");
        exit(2);
    };
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        eprintln!("CypherLinks CLI requires an HTTP(S) URL."); exit(2);
    }
    let output = args.next().unwrap_or_else(|| ".".into());
    let quality = args.next().unwrap_or_else(|| "1080".into());
    let format = if quality == "best" { "bestvideo+bestaudio/best".to_string() } else { format!("bestvideo[height<={0}]+bestaudio/best[height<={0}]", quality) };
    let status = Command::new("yt-dlp")
        .args(["--continue", "--retries", "10", "--fragment-retries", "10", "-f", &format, "--merge-output-format", "mp4", "--paths", &output, &url])
        .status();
    match status {
        Ok(code) if code.success() => {}
        Ok(code) => { eprintln!("yt-dlp exited with {code}"); exit(code.code().unwrap_or(1)); }
        Err(error) => { eprintln!("Could not start yt-dlp: {error}"); exit(1); }
    }
}
