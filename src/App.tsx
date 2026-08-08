import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import {
  CheckCircle2,
  Download,
  FolderOpen,
  Link2,
  Loader2,
  Music2,
  Pause,
  Play,
  RefreshCw,
  Square,
  Trash2,
  Video,
  XCircle,
} from 'lucide-react';

type VideoInfo = {
  id: string;
  title: string;
  uploader?: string;
  duration?: number;
  thumbnail?: string;
  webpage_url?: string;
  formats: Array<{
    format_id: string;
    ext?: string;
    height?: number;
    fps?: number;
    filesize?: number;
    filesize_approx?: number;
    vcodec?: string;
    acodec?: string;
  }>;
};

type ProgressEvent = {
  id: string;
  status: 'downloading' | 'processing' | 'finished' | 'error' | 'cancelled';
  percent?: number;
  speed?: string;
  eta?: string;
  filename?: string;
  message?: string;
};

type DownloadItem = {
  id: string;
  url: string;
  title: string;
  mode: 'video' | 'audio';
  quality: string;
  progress: number;
  status: ProgressEvent['status'];
  speed?: string;
  eta?: string;
  filename?: string;
  message?: string;
};

const qualityOptions = ['best', '2160', '1440', '1080', '720', '480', '360'];

function formatDuration(seconds?: number) {
  if (!seconds || Number.isNaN(seconds)) return '—';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  return h > 0
    ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
    : `${m}:${String(s).padStart(2, '0')}`;
}

function App() {
  const [url, setUrl] = useState('');
  const [batchUrls, setBatchUrls] = useState('');
  const [info, setInfo] = useState<VideoInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<'video' | 'audio'>('video');
  const [quality, setQuality] = useState('1080');
  const [subtitles, setSubtitles] = useState(false);
  const [playlist, setPlaylist] = useState(false);
  const [cookiesBrowser, setCookiesBrowser] = useState('none');
  const [embedMetadata, setEmbedMetadata] = useState(true);
  const [embedThumbnail, setEmbedThumbnail] = useState(true);
  const [downloadDir, setDownloadDir] = useState('');
  const [items, setItems] = useState<DownloadItem[]>(() => { try { return JSON.parse(localStorage.getItem('linkforge-history') || '[]'); } catch { return []; } });
  const [scheduledAt, setScheduledAt] = useState('');
  const [deps, setDeps] = useState<{ytDlp:boolean; ffmpeg:boolean} | null>(null);
  const [updateMessage, setUpdateMessage] = useState('');
  const [watchClipboard, setWatchClipboard] = useState(() => localStorage.getItem('linkforge-watch-clipboard') === 'true');
  const [clipboardCandidate, setClipboardCandidate] = useState('');

  useEffect(() => { localStorage.setItem('linkforge-history', JSON.stringify(items.slice(0, 100))); }, [items]);
  useEffect(() => { localStorage.setItem('linkforge-watch-clipboard', String(watchClipboard)); }, [watchClipboard]);

  useEffect(() => {
    if (!watchClipboard) return;
    let lastSeen = '';
    const timer = window.setInterval(async () => {
      try {
        const value = (await invoke<string>('get_clipboard_text')).trim();
        if (/^https?:\/\//i.test(value) && value !== lastSeen && value !== url.trim()) {
          lastSeen = value;
          setClipboardCandidate(value);
        }
      } catch { /* Clipboard support is optional. */ }
    }, 1600);
    return () => window.clearInterval(timer);
  }, [watchClipboard, url]);

  useEffect(() => {
    invoke<string>('default_download_dir').then(setDownloadDir).catch(() => {});
    invoke<{ytDlp:boolean; ffmpeg:boolean}>('dependency_status').then(setDeps).catch(() => {});
    const extensionUnlisten = listen<{url: string}>('extension-url', ({ payload }) => {
      setUrl(payload.url);
      setInfo(null);
      setError('Link received from browser extension. Analyze it when ready.');
    });
    const unlisten = listen<ProgressEvent>('download-progress', ({ payload }) => {
      setItems((current) =>
        current.map((item) =>
          item.id === payload.id
            ? {
                ...item,
                status: payload.status,
                progress: payload.percent ?? item.progress,
                speed: payload.speed ?? item.speed,
                eta: payload.eta ?? item.eta,
                filename: payload.filename ?? item.filename,
                message: payload.message ?? item.message,
              }
            : item,
        ),
      );
    });
    return () => {
      unlisten.then((fn) => fn());
      extensionUnlisten.then((fn) => fn());
    };
  }, []);

  const availableHeights = useMemo(() => {
    if (!info) return new Set<number>();
    return new Set(info.formats.map((f) => f.height).filter((h): h is number => Boolean(h)));
  }, [info]);

  async function analyze() {
    const trimmed = url.trim();
    if (!trimmed) return;
    setLoading(true);
    setError('');
    setInfo(null);
    try {
      const result = await invoke<VideoInfo>('analyze_url', { url: trimmed });
      setInfo(result);
      const heights = [...new Set(result.formats.map((f) => f.height).filter(Boolean))] as number[];
      if (heights.length && !heights.some((h) => h <= Number(quality))) {
        setQuality(String(Math.max(...heights)));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function chooseFolder() {
    const selected = await open({ directory: true, multiple: false, defaultPath: downloadDir || undefined });
    if (typeof selected === 'string') setDownloadDir(selected);
  }

  async function startDownload() {
    if (!info || !downloadDir) return;
    const id = crypto.randomUUID();
    const item: DownloadItem = {
      id,
      url: info.webpage_url || url.trim(),
      title: info.title,
      mode,
      quality,
      progress: 0,
      status: 'downloading',
    };
    setItems((current) => [item, ...current]);
    try {
      const request = { id, url: item.url, outputDir: downloadDir, mode, quality, subtitles, playlist, cookiesBrowser: cookiesBrowser === 'none' ? null : cookiesBrowser, embedMetadata, embedThumbnail, archivePath: `${downloadDir}/.linkforge-archive.txt` };
      const delay = scheduledAt ? Math.max(0, new Date(scheduledAt).getTime() - Date.now()) : 0;
      if (delay > 0) {
        setItems((current) => current.map((entry) => entry.id === id ? { ...entry, message: `Scheduled for ${new Date(scheduledAt).toLocaleString()}` } : entry));
        window.setTimeout(() => invoke('start_download', { request }).catch((e) => setError(String(e))), delay);
      } else {
        await invoke('start_download', { request });
      }
    } catch (e) {
      setItems((current) =>
        current.map((entry) =>
          entry.id === id ? { ...entry, status: 'error', message: String(e) } : entry,
        ),
      );
    }
  }

  async function startBatch() {
    if (!downloadDir) return;
    const urls = batchUrls.split(/\s+/).map((v) => v.trim()).filter(Boolean);
    for (const batchUrl of urls) {
      try {
        const meta = await invoke<VideoInfo>('analyze_url', { url: batchUrl });
        const id = crypto.randomUUID();
        setItems((current) => [{ id, url: batchUrl, title: meta.title, mode, quality, progress: 0, status: 'downloading' }, ...current]);
        await invoke('start_download', { request: { id, url: batchUrl, outputDir: downloadDir, mode, quality, subtitles, playlist, cookiesBrowser: cookiesBrowser === 'none' ? null : cookiesBrowser, embedMetadata, embedThumbnail, archivePath: `${downloadDir}/.linkforge-archive.txt` } });
      } catch (e) { setError(String(e)); }
    }
  }

  async function cancel(id: string) {
    await invoke('cancel_download', { id }).catch(() => {});
  }

  function clearFinished() {
    setItems((current) => current.filter((item) => !['finished', 'error', 'cancelled'].includes(item.status)));
  }

  return (
    <main className="shell">
      <section className="hero">
        <div>
          <div className="brand"><div className="mark">LF</div><span>LinkForge</span></div>
          <h1>Download the media you’re allowed to keep.</h1>
          <p>Paste a supported link, inspect it, choose the output, and save it locally.</p>
        </div>
        <div><div className="status-pill"><span className="status-dot" /> {deps ? `yt-dlp ${deps.ytDlp ? '✓' : '✗'} · FFmpeg ${deps.ffmpeg ? '✓' : '✗'}` : 'Checking tools…'}</div><button className="ghost" style={{marginTop:8}} onClick={async () => { try { setUpdateMessage(await invoke<string>('update_downloader')); } catch(e) { setUpdateMessage(String(e)); } }}>Update yt-dlp</button><button className="ghost" style={{marginTop:8, marginLeft:8}} onClick={async () => { try { setUpdateMessage(await invoke<string>('install_dependencies')); setDeps(await invoke('dependency_status')); } catch(e) { setUpdateMessage(String(e)); } }}>Install dependencies</button>{updateMessage && <div className="download-meta">{updateMessage}</div>}</div>
      </section>

      <section className="panel composer">
        <div className="url-row">
          <Link2 size={18} />
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && analyze()}
            placeholder="Paste a YouTube or supported video URL"
            aria-label="Video URL"
          />
          <button className="primary" onClick={analyze} disabled={loading || !url.trim()}>
            {loading ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
            Analyze
          </button>
        </div>
        {clipboardCandidate && <div className="error" style={{color:'#d8d8d8'}}><Link2 size={16} /> Clipboard link detected <button className="ghost" onClick={() => { setUrl(clipboardCandidate); setClipboardCandidate(''); }}>Use link</button><button className="ghost" onClick={() => setClipboardCandidate('')}>Dismiss</button></div>}
        {error && <div className="error"><XCircle size={16} /> {error}</div>}
        <label className="checkbox-row" style={{marginTop:10}}><input type="checkbox" checked={watchClipboard} onChange={(e) => setWatchClipboard(e.target.checked)} /><span>Detect copied media links</span></label>
        <div style={{marginTop: 12, display: 'flex', gap: 10}}><textarea value={batchUrls} onChange={(e) => setBatchUrls(e.target.value)} placeholder="Batch URLs — one per line" style={{flex: 1, minHeight: 70}} /><button className="ghost" onClick={startBatch} disabled={!batchUrls.trim() || !downloadDir}><Download size={15}/> Queue batch</button></div>
      </section>

      {info && (
        <section className="panel media-card">
          <div className="thumb-wrap">
            {info.thumbnail ? <img src={info.thumbnail} alt="" className="thumb" /> : <div className="thumb placeholder" />}
            <span className="duration">{formatDuration(info.duration)}</span>
          </div>
          <div className="media-content">
            <div className="media-copy">
              <p className="eyebrow">READY</p>
              <h2>{info.title}</h2>
              <p>{info.uploader || 'Unknown uploader'}</p>
            </div>

            <div className="controls-grid">
              <div className="control-group">
                <span>Output</span>
                <div className="segmented">
                  <button className={mode === 'video' ? 'active' : ''} onClick={() => setMode('video')}><Video size={15} /> Video</button>
                  <button className={mode === 'audio' ? 'active' : ''} onClick={() => setMode('audio')}><Music2 size={15} /> Audio</button>
                </div>
              </div>

              {mode === 'video' && (
                <label className="control-group">
                  <span>Quality</span>
                  <select value={quality} onChange={(e) => setQuality(e.target.value)}>
                    {qualityOptions.map((q) => (
                      <option key={q} value={q} disabled={q !== 'best' && info && !availableHeights.has(Number(q))}>
                        {q === 'best' ? 'Best available' : `${q}p`}
                      </option>
                    ))}
                  </select>
                </label>
              )}

              <label className="checkbox-row">
                <input type="checkbox" checked={subtitles} onChange={(e) => setSubtitles(e.target.checked)} />
                <span>Save subtitles when available</span>
              </label>
              <label className="checkbox-row"><input type="checkbox" checked={playlist} onChange={(e) => setPlaylist(e.target.checked)} /><span>Download playlist/channel links</span></label>
              <label className="checkbox-row"><input type="checkbox" checked={embedMetadata} onChange={(e) => setEmbedMetadata(e.target.checked)} /><span>Embed media metadata</span></label>
              <label className="checkbox-row"><input type="checkbox" checked={embedThumbnail} onChange={(e) => setEmbedThumbnail(e.target.checked)} /><span>Embed thumbnail</span></label>
              <label className="control-group"><span>Browser cookies</span><select value={cookiesBrowser} onChange={(e) => setCookiesBrowser(e.target.value)}><option value="none">None</option><option value="chrome">Chrome</option><option value="firefox">Firefox</option><option value="edge">Edge</option><option value="safari">Safari</option></select></label>
            </div>

            <div className="download-actions">
              <input type="datetime-local" value={scheduledAt} onChange={(e) => setScheduledAt(e.target.value)} title="Schedule download" />
              <button className="folder-button" onClick={chooseFolder}><FolderOpen size={16} /> {downloadDir || 'Choose folder'}</button>
              <button className="download-button" onClick={startDownload} disabled={!downloadDir}><Download size={17} /> Download</button>
            </div>
          </div>
        </section>
      )}

      <section className="panel queue">
        <div className="section-head">
          <div>
            <p className="eyebrow">QUEUE</p>
            <h3>Downloads</h3>
          </div>
          {items.length > 0 && <button className="ghost" onClick={clearFinished}><Trash2 size={15} /> Clear finished</button>}
        </div>

        {items.length === 0 ? (
          <div className="empty">Your downloads will appear here.</div>
        ) : (
          <div className="download-list">
            {items.map((item) => (
              <article className="download-item" key={item.id}>
                <div className="download-icon">
                  {item.status === 'finished' ? <CheckCircle2 size={18} /> : item.status === 'error' ? <XCircle size={18} /> : item.mode === 'audio' ? <Music2 size={18} /> : <Video size={18} />}
                </div>
                <div className="download-main">
                  <div className="download-title-row">
                    <strong>{item.title}</strong>
                    <span>{item.mode === 'audio' ? 'MP3' : item.quality === 'best' ? 'BEST' : `${item.quality}P`}</span>
                  </div>
                  <div className="progress-track"><div className="progress-fill" style={{ width: `${Math.max(0, Math.min(100, item.progress))}%` }} /></div>
                  <div className="download-meta">
                    <span>{item.status === 'processing' ? 'Processing' : item.status}</span>
                    {item.speed && <span>{item.speed}</span>}
                    {item.eta && <span>ETA {item.eta}</span>}
                    {item.message && <span className="message">{item.message}</span>}
                  </div>
                </div>
                {['downloading', 'processing'].includes(item.status) && (
                  <button className="icon-button" onClick={() => cancel(item.id)} title="Cancel"><Square size={15} fill="currentColor" /></button>
                )}
              </article>
            ))}
          </div>
        )}
      </section>

      <footer>
        LinkForge does not bypass DRM or access controls. Use it only for media you own or have permission to download.
      </footer>
    </main>
  );
}

export default App;
