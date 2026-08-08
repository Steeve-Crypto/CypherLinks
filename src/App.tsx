import { useEffect, useMemo, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import {
  AlertCircle,
  CheckCircle2,
  Clipboard,
  Clock,
  Download,
  Eye,
  FolderOpen,
  Globe,
  Link2,
  List,
  Loader2,
  Music2,
  Package,
  Play,
  RefreshCw,
  Save,
  Settings,
  SlidersHorizontal,
  Square,
  Trash2,
  Video,
  X,
  XCircle,
} from 'lucide-react';

type DownloadStatus = 'scheduled' | 'queued' | 'downloading' | 'processing' | 'finished' | 'error' | 'cancelled';
type OutputMode = 'video' | 'audio';
type DuplicatePolicy = 'skip' | 'keep' | 'overwrite';
type Tab = 'download' | 'queue' | 'settings';

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
  status: DownloadStatus;
  percent?: number;
  speed?: string;
  eta?: string;
  filename?: string;
  message?: string;
};

type SitePreset = {
  mode: OutputMode;
  quality: string;
  subtitles: boolean;
  playlist: boolean;
  embedMetadata: boolean;
  embedThumbnail: boolean;
  filenameTemplate: string;
  bandwidthLimit: string;
  splitChapters: boolean;
  transcodePreset: string;
  duplicatePolicy: DuplicatePolicy;
  priority: number;
  postAction: string;
  cookiesBrowser: string;
};

type DownloadItem = {
  id: string;
  url: string;
  title: string;
  sourceId?: string;
  mode: OutputMode;
  quality: string;
  priority: number;
  progress: number;
  status: DownloadStatus;
  speed?: string;
  eta?: string;
  filename?: string;
  message?: string;
  scheduledAtMs?: number;
};

type DependencyStatus = { ytDlp: boolean; ffmpeg: boolean };

const qualityOptions = ['best', '2160', '1440', '1080', '720', '480', '360'];
const defaultTemplate = '%(title).180B [%(id)s].%(ext)s';

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function formatDuration(seconds?: number) {
  if (!seconds || Number.isNaN(seconds)) return '—';
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`
    : `${minutes}:${String(secs).padStart(2, '0')}`;
}

function hostnameFor(value: string) {
  try {
    return new URL(value).hostname.replace(/^www\./, '');
  } catch {
    return '';
  }
}

function statusLabel(status: DownloadStatus) {
  return status === 'processing' ? 'Processing' : status.charAt(0).toUpperCase() + status.slice(1);
}

function priorityLabel(priority: number) {
  return priority > 0 ? 'High' : priority < 0 ? 'Low' : 'Normal';
}

function App() {
  const [activeTab, setActiveTab] = useState<Tab>('download');
  const [url, setUrl] = useState('');
  const [batchUrls, setBatchUrls] = useState('');
  const [info, setInfo] = useState<VideoInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  const [mode, setMode] = useState<OutputMode>('video');
  const [quality, setQuality] = useState('1080');
  const [subtitles, setSubtitles] = useState(false);
  const [playlist, setPlaylist] = useState(false);
  const [cookiesBrowser, setCookiesBrowser] = useState('none');
  const [embedMetadata, setEmbedMetadata] = useState(true);
  const [embedThumbnail, setEmbedThumbnail] = useState(true);
  const [splitChapters, setSplitChapters] = useState(false);
  const [transcodePreset, setTranscodePreset] = useState('source');
  const [duplicatePolicy, setDuplicatePolicy] = useState<DuplicatePolicy>('skip');
  const [priority, setPriority] = useState(0);
  const [bandwidthLimit, setBandwidthLimit] = useState('');
  const [proxy, setProxy] = useState('');
  const [postAction, setPostAction] = useState('none');
  const [scheduledAt, setScheduledAt] = useState('');
  const [filenameTemplate, setFilenameTemplate] = useState(() => localStorage.getItem('linkforge-filename-template') || defaultTemplate);
  const [downloadDir, setDownloadDir] = useState('');

  const [items, setItems] = useState<DownloadItem[]>(() => readJson('linkforge-history', []));
  const [previewItem, setPreviewItem] = useState<DownloadItem | null>(null);
  const [sitePresets, setSitePresets] = useState<Record<string, SitePreset>>(() => readJson('linkforge-site-presets', {}));

  const [watchClipboard, setWatchClipboard] = useState(() => localStorage.getItem('linkforge-watch-clipboard') === 'true');
  const [clipboardCandidate, setClipboardCandidate] = useState('');
  const [maxConcurrent, setMaxConcurrent] = useState(() => Math.max(1, Math.min(6, Number(localStorage.getItem('linkforge-concurrency') || 2))));
  const [deps, setDeps] = useState<DependencyStatus | null>(null);
  const [toolMessage, setToolMessage] = useState('');

  useEffect(() => {
    localStorage.setItem('linkforge-history', JSON.stringify(items.slice(0, 150)));
  }, [items]);

  useEffect(() => {
    localStorage.setItem('linkforge-site-presets', JSON.stringify(sitePresets));
  }, [sitePresets]);

  useEffect(() => {
    localStorage.setItem('linkforge-filename-template', filenameTemplate);
  }, [filenameTemplate]);

  useEffect(() => {
    localStorage.setItem('linkforge-watch-clipboard', String(watchClipboard));
  }, [watchClipboard]);

  useEffect(() => {
    localStorage.setItem('linkforge-concurrency', String(maxConcurrent));
    invoke('set_max_concurrent', { value: maxConcurrent }).catch(() => {});
  }, [maxConcurrent]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(''), 4500);
    return () => window.clearTimeout(timer);
  }, [notice]);

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
      } catch {
        // Clipboard integration is optional and platform-dependent.
      }
    }, 1600);
    return () => window.clearInterval(timer);
  }, [watchClipboard, url]);

  useEffect(() => {
    invoke<string>('default_download_dir').then(setDownloadDir).catch(() => {});
    invoke<DependencyStatus>('dependency_status').then(setDeps).catch(() => {});

    const progressUnlisten = listen<ProgressEvent>('download-progress', ({ payload }) => {
      setItems((current) => current.map((item) => item.id === payload.id
        ? {
            ...item,
            status: payload.status,
            progress: payload.percent ?? item.progress,
            speed: payload.speed ?? item.speed,
            eta: payload.eta ?? item.eta,
            filename: payload.filename ?? item.filename,
            message: payload.message ?? item.message,
          }
        : item));
    });

    const extensionUnlisten = listen<{ url: string }>('extension-url', ({ payload }) => {
      setUrl(payload.url);
      setInfo(null);
      setError('');
      setNotice('Link received from the browser extension.');
      setActiveTab('download');
    });

    return () => {
      progressUnlisten.then((unlisten) => unlisten());
      extensionUnlisten.then((unlisten) => unlisten());
    };
  }, []);

  const availableHeights = useMemo(() => {
    if (!info) return new Set<number>();
    return new Set(info.formats.map((format) => format.height).filter((height): height is number => Boolean(height)));
  }, [info]);

  const currentHost = hostnameFor(url);
  const currentPreset = currentHost ? sitePresets[currentHost] : undefined;
  const duplicateCount = info ? items.filter((item) => item.sourceId === info.id && item.status === 'finished').length : 0;
  const activeCount = items.filter((item) => ['downloading', 'processing'].includes(item.status)).length;
  const waitingCount = items.filter((item) => ['queued', 'scheduled'].includes(item.status)).length;
  const finishedCount = items.filter((item) => item.status === 'finished').length;

  function applyPreset(preset: SitePreset, host?: string) {
    setMode(preset.mode);
    setQuality(preset.quality);
    setSubtitles(preset.subtitles);
    setPlaylist(preset.playlist);
    setEmbedMetadata(preset.embedMetadata);
    setEmbedThumbnail(preset.embedThumbnail);
    setFilenameTemplate(preset.filenameTemplate || defaultTemplate);
    setBandwidthLimit(preset.bandwidthLimit || '');
    setSplitChapters(Boolean(preset.splitChapters));
    setTranscodePreset(preset.transcodePreset || 'source');
    setDuplicatePolicy(preset.duplicatePolicy || 'skip');
    setPriority(preset.priority ?? 0);
    setPostAction(preset.postAction || 'none');
    setCookiesBrowser(preset.cookiesBrowser || 'none');
    if (host) setNotice(`Applied preset for ${host}.`);
  }

  function saveCurrentPreset() {
    if (!currentHost) {
      setError('Enter a valid site URL before saving a site preset.');
      return;
    }
    const preset: SitePreset = {
      mode,
      quality,
      subtitles,
      playlist,
      embedMetadata,
      embedThumbnail,
      filenameTemplate,
      bandwidthLimit,
      splitChapters,
      transcodePreset,
      duplicatePolicy,
      priority,
      postAction,
      cookiesBrowser,
    };
    setSitePresets((current) => ({ ...current, [currentHost]: preset }));
    setNotice(`Saved preset for ${currentHost}.`);
  }

  function removePreset(host: string) {
    setSitePresets((current) => {
      const next = { ...current };
      delete next[host];
      return next;
    });
    setNotice(`Removed preset for ${host}.`);
  }

  async function analyze(value = url) {
    const trimmed = value.trim();
    if (!trimmed) return;
    setUrl(trimmed);
    setLoading(true);
    setError('');
    setInfo(null);

    const host = hostnameFor(trimmed);
    const preset = host ? sitePresets[host] : undefined;
    const effectivePlaylist = preset?.playlist ?? playlist;
    const effectiveQuality = preset?.quality ?? quality;
    if (preset) applyPreset(preset, host);

    try {
      const result = await invoke<VideoInfo>('analyze_url', { url: trimmed, playlist: effectivePlaylist });
      setInfo(result);
      const heights = [...new Set(result.formats.map((format) => format.height).filter((height): height is number => Boolean(height)))];
      if (heights.length && effectiveQuality !== 'best' && !heights.includes(Number(effectiveQuality))) {
        const below = heights.filter((height) => height <= Number(effectiveQuality));
        setQuality(String(below.length ? Math.max(...below) : Math.max(...heights)));
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }

  async function chooseFolder() {
    const selected = await open({ directory: true, multiple: false, defaultPath: downloadDir || undefined });
    if (typeof selected === 'string') setDownloadDir(selected);
  }

  function scheduledEpoch() {
    if (!scheduledAt) return undefined;
    const value = new Date(scheduledAt).getTime();
    return Number.isFinite(value) && value > Date.now() ? value : undefined;
  }

  function requestFor(id: string, mediaUrl: string) {
    return {
      id,
      url: mediaUrl,
      outputDir: downloadDir,
      mode,
      quality,
      subtitles,
      playlist,
      cookiesBrowser: cookiesBrowser === 'none' ? null : cookiesBrowser,
      embedMetadata,
      embedThumbnail,
      archivePath: `${downloadDir}/.linkforge-archive.txt`,
      filenameTemplate,
      priority,
      limitRate: bandwidthLimit.trim() || null,
      proxy: proxy.trim() || null,
      duplicatePolicy,
      splitChapters,
      transcodePreset,
      postAction,
      scheduledAtMs: scheduledEpoch() ?? null,
    };
  }

  async function startDownload() {
    if (!info || !downloadDir) return;
    const id = crypto.randomUUID();
    const mediaUrl = info.webpage_url || url.trim();
    const scheduledAtMs = scheduledEpoch();
    const item: DownloadItem = {
      id,
      url: mediaUrl,
      title: info.title,
      sourceId: info.id,
      mode,
      quality,
      priority,
      progress: 0,
      status: scheduledAtMs ? 'scheduled' : 'queued',
      scheduledAtMs,
    };
    setItems((current) => [item, ...current]);
    try {
      await invoke('start_download', { request: requestFor(id, mediaUrl) });
      setActiveTab('queue');
    } catch (reason) {
      setItems((current) => current.map((entry) => entry.id === id ? { ...entry, status: 'error', message: String(reason) } : entry));
    }
  }

  async function startBatch() {
    if (!downloadDir) return;
    const urls = batchUrls.split(/\r?\n/).map((value) => value.trim()).filter(Boolean);
    if (!urls.length) return;
    setError('');
    setActiveTab('queue');

    for (const batchUrl of urls) {
      try {
        const metadata = await invoke<VideoInfo>('analyze_url', { url: batchUrl, playlist: false });
        const id = crypto.randomUUID();
        const scheduledAtMs = scheduledEpoch();
        const item: DownloadItem = {
          id,
          url: batchUrl,
          title: metadata.title,
          sourceId: metadata.id,
          mode,
          quality,
          priority,
          progress: 0,
          status: scheduledAtMs ? 'scheduled' : 'queued',
          scheduledAtMs,
        };
        setItems((current) => [item, ...current]);
        await invoke('start_download', { request: requestFor(id, batchUrl) });
      } catch (reason) {
        setError(String(reason));
      }
    }
  }

  async function cancel(id: string) {
    await invoke('cancel_download', { id }).catch((reason) => setError(String(reason)));
  }

  async function changeQueuedPriority(id: string, nextPriority: number) {
    try {
      await invoke('update_queued_priority', { id, priority: nextPriority });
      setItems((current) => current.map((item) => item.id === id ? { ...item, priority: nextPriority } : item));
    } catch (reason) {
      setError(String(reason));
    }
  }

  function clearFinished() {
    setItems((current) => current.filter((item) => !['finished', 'error', 'cancelled'].includes(item.status)));
  }

  async function refreshDependencies() {
    setDeps(await invoke<DependencyStatus>('dependency_status'));
  }

  async function updateDownloader() {
    setToolMessage('Updating yt-dlp…');
    try {
      setToolMessage(await invoke<string>('update_downloader'));
      await refreshDependencies();
    } catch (reason) {
      setToolMessage(String(reason));
    }
  }

  async function installDependencies() {
    setToolMessage('Installing dependencies…');
    try {
      setToolMessage(await invoke<string>('install_dependencies'));
      await refreshDependencies();
    } catch (reason) {
      setToolMessage(String(reason));
    }
  }

  async function openExtensionFolder() {
    try {
      const path = await invoke<string>('open_extension_folder');
      setToolMessage(`Opened extension folder: ${path}`);
    } catch (reason) {
      setToolMessage(String(reason));
    }
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <div className="brand-mark">LF</div>
          <div>
            <strong>LinkForge</strong>
            <span>Local media workspace</span>
          </div>
        </div>

        <nav className="tabs" aria-label="Primary navigation">
          <button className={activeTab === 'download' ? 'active' : ''} onClick={() => setActiveTab('download')}><Download size={15} /> Download</button>
          <button className={activeTab === 'queue' ? 'active' : ''} onClick={() => setActiveTab('queue')}><List size={15} /> Queue {waitingCount + activeCount > 0 && <b>{waitingCount + activeCount}</b>}</button>
          <button className={activeTab === 'settings' ? 'active' : ''} onClick={() => setActiveTab('settings')}><Settings size={15} /> Settings</button>
        </nav>

        <div className={`tool-health ${deps?.ytDlp && deps?.ffmpeg ? 'healthy' : ''}`}>
          <span className="health-dot" />
          {deps ? `${deps.ytDlp ? 'yt-dlp' : 'yt-dlp missing'} · ${deps.ffmpeg ? 'FFmpeg' : 'FFmpeg missing'}` : 'Checking tools'}
        </div>
      </header>

      {notice && <div className="toast"><CheckCircle2 size={15} /> {notice}</div>}

      {activeTab === 'download' && (
        <div className="page-grid">
          <section className="primary-column">
            <div className="panel source-panel">
              <div className="panel-heading compact-heading">
                <div>
                  <span className="kicker">SOURCE</span>
                  <h1>Bring in a link.</h1>
                </div>
                {currentPreset && <span className="preset-badge">Preset · {currentHost}</span>}
              </div>

              <div className="source-input-row">
                <Link2 size={18} />
                <input
                  value={url}
                  onChange={(event) => setUrl(event.target.value)}
                  onKeyDown={(event) => event.key === 'Enter' && analyze()}
                  placeholder="Paste a YouTube or supported media URL"
                  aria-label="Media URL"
                />
                <button className="primary-button" onClick={() => analyze()} disabled={loading || !url.trim()}>
                  {loading ? <Loader2 className="spin" size={16} /> : <RefreshCw size={16} />}
                  Analyze
                </button>
              </div>

              {clipboardCandidate && (
                <div className="inline-notice">
                  <Clipboard size={15} />
                  <span>Copied link detected</span>
                  <code>{clipboardCandidate}</code>
                  <button onClick={() => { setUrl(clipboardCandidate); setClipboardCandidate(''); }}>Use</button>
                  <button className="quiet" onClick={() => setClipboardCandidate('')}>Dismiss</button>
                </div>
              )}

              {error && <div className="error-banner"><AlertCircle size={16} /> {error}</div>}
            </div>

            {info ? (
              <div className="panel media-panel">
                <div className="media-hero">
                  <div className="thumbnail-wrap">
                    {info.thumbnail ? <img src={info.thumbnail} alt="" /> : <div className="thumbnail-placeholder"><Video size={28} /></div>}
                    <span>{formatDuration(info.duration)}</span>
                  </div>
                  <div className="media-details">
                    <span className="kicker">READY</span>
                    <h2>{info.title}</h2>
                    <p>{info.uploader || 'Unknown uploader'}</p>
                    <div className="media-tags">
                      {playlist && <span>Playlist / channel mode</span>}
                      {duplicateCount > 0 && <span className="warning-tag">Downloaded {duplicateCount}× before</span>}
                      {currentPreset && <span>Site preset applied</span>}
                    </div>
                  </div>
                </div>
              </div>
            ) : (
              <div className="panel empty-media">
                <div className="empty-icon"><Play size={22} /></div>
                <h2>Analyze a media page</h2>
                <p>LinkForge will inspect available formats before anything is downloaded.</p>
              </div>
            )}

            <div className="panel batch-panel">
              <div className="panel-heading compact-heading">
                <div><span className="kicker">BATCH</span><h3>Queue multiple URLs</h3></div>
                <span className="muted">One URL per line</span>
              </div>
              <textarea value={batchUrls} onChange={(event) => setBatchUrls(event.target.value)} placeholder={'https://…\nhttps://…\nhttps://…'} />
              <div className="batch-actions">
                <span>{batchUrls.split(/\r?\n/).filter((value) => value.trim()).length} links ready</span>
                <button className="secondary-button" onClick={startBatch} disabled={!batchUrls.trim() || !downloadDir}><Download size={15} /> Queue batch</button>
              </div>
            </div>
          </section>

          <aside className="panel profile-panel">
            <div className="panel-heading">
              <div><span className="kicker">DOWNLOAD PROFILE</span><h2>Output controls</h2></div>
              <SlidersHorizontal size={18} />
            </div>

            <div className="form-section">
              <label>Output</label>
              <div className="segmented-control">
                <button className={mode === 'video' ? 'active' : ''} onClick={() => { setMode('video'); if (transcodePreset === 'audio-library') setTranscodePreset('source'); }}><Video size={15} /> Video</button>
                <button className={mode === 'audio' ? 'active' : ''} onClick={() => { setMode('audio'); setTranscodePreset(transcodePreset === 'source' ? 'source' : 'audio-library'); }}><Music2 size={15} /> Audio</button>
              </div>
            </div>

            {mode === 'video' && (
              <div className="form-row two-column">
                <label><span>Quality</span><select value={quality} onChange={(event) => setQuality(event.target.value)}>{qualityOptions.map((option) => <option key={option} value={option} disabled={Boolean(info && info.formats.length && option !== 'best' && !availableHeights.has(Number(option)))}>{option === 'best' ? 'Best available' : `${option}p`}</option>)}</select></label>
                <label><span>Transcode</span><select value={transcodePreset} onChange={(event) => setTranscodePreset(event.target.value)}><option value="source">Keep source</option><option value="phone">Phone · 720p H.264</option><option value="desktop">Desktop · H.264 HQ</option><option value="archive">Archive · MKV HQ</option><option value="audio-library">Audio library · AAC</option></select></label>
              </div>
            )}

            {mode === 'audio' && (
              <div className="form-section"><label>Transcode</label><select value={transcodePreset} onChange={(event) => setTranscodePreset(event.target.value)}><option value="source">MP3 source workflow</option><option value="audio-library">Audio library · AAC 256k</option></select></div>
            )}

            <div className="form-section">
              <label>Destination</label>
              <button className="path-button" onClick={chooseFolder}><FolderOpen size={15} /><span>{downloadDir || 'Choose a download folder'}</span></button>
            </div>

            <div className="form-section">
              <label>Filename template</label>
              <input value={filenameTemplate} onChange={(event) => setFilenameTemplate(event.target.value)} placeholder={defaultTemplate} />
              <small>Supports yt-dlp fields such as %(title)s, %(uploader)s, and %(id)s.</small>
            </div>

            <div className="form-row two-column">
              <label><span>Queue priority</span><select value={priority} onChange={(event) => setPriority(Number(event.target.value))}><option value={10}>High</option><option value={0}>Normal</option><option value={-10}>Low</option></select></label>
              <label><span>Duplicates</span><select value={duplicatePolicy} onChange={(event) => setDuplicatePolicy(event.target.value as DuplicatePolicy)}><option value="skip">Skip known media</option><option value="keep">Keep both</option><option value="overwrite">Overwrite</option></select></label>
            </div>

            <div className="form-row two-column">
              <label><span>Bandwidth limit</span><input value={bandwidthLimit} onChange={(event) => setBandwidthLimit(event.target.value)} placeholder="Unlimited / 5M" /></label>
              <label><span>Browser cookies</span><select value={cookiesBrowser} onChange={(event) => setCookiesBrowser(event.target.value)}><option value="none">None</option><option value="chrome">Chrome</option><option value="firefox">Firefox</option><option value="edge">Edge</option><option value="safari">Safari</option></select></label>
            </div>

            <div className="form-section">
              <label>Proxy</label>
              <input value={proxy} onChange={(event) => setProxy(event.target.value)} placeholder="socks5://127.0.0.1:1080 (optional)" />
            </div>

            <div className="form-row two-column">
              <label><span>Schedule</span><input type="datetime-local" value={scheduledAt} onChange={(event) => setScheduledAt(event.target.value)} /></label>
              <label><span>After download</span><select value={postAction} onChange={(event) => setPostAction(event.target.value)}><option value="none">Do nothing</option><option value="open-folder">Open folder</option><option value="open-file">Open media</option></select></label>
            </div>

            <div className="switch-list">
              <label><input type="checkbox" checked={playlist} onChange={(event) => setPlaylist(event.target.checked)} /><span><strong>Playlist / channel mode</strong><small>Download all entries from collection URLs.</small></span></label>
              <label><input type="checkbox" checked={subtitles} onChange={(event) => setSubtitles(event.target.checked)} /><span><strong>Subtitles</strong><small>Save English subtitles and auto-captions when available.</small></span></label>
              <label><input type="checkbox" checked={splitChapters} onChange={(event) => setSplitChapters(event.target.checked)} /><span><strong>Split chapters</strong><small>Create separate files from chapter markers.</small></span></label>
              <label><input type="checkbox" checked={embedMetadata} onChange={(event) => setEmbedMetadata(event.target.checked)} /><span><strong>Embed metadata</strong><small>Keep title, artist, and source information.</small></span></label>
              <label><input type="checkbox" checked={embedThumbnail} onChange={(event) => setEmbedThumbnail(event.target.checked)} /><span><strong>Embed thumbnail</strong><small>Attach artwork to supported output formats.</small></span></label>
            </div>

            <div className="profile-actions">
              <button className="secondary-button" onClick={saveCurrentPreset} disabled={!currentHost}><Save size={15} /> {currentPreset ? 'Update site preset' : 'Save site preset'}</button>
              <button className="download-button" onClick={startDownload} disabled={!info || !downloadDir}><Download size={17} /> {scheduledEpoch() ? 'Schedule download' : 'Start download'}</button>
            </div>
          </aside>
        </div>
      )}

      {activeTab === 'queue' && (
        <section className="queue-page">
          <div className="stats-row">
            <div className="stat-card"><span>Active</span><strong>{activeCount}</strong><small>Downloading or processing</small></div>
            <div className="stat-card"><span>Waiting</span><strong>{waitingCount}</strong><small>Queued or scheduled</small></div>
            <div className="stat-card"><span>Completed</span><strong>{finishedCount}</strong><small>Available in local history</small></div>
          </div>

          <div className="panel queue-panel">
            <div className="panel-heading">
              <div><span className="kicker">QUEUE</span><h1>Download activity</h1></div>
              {items.some((item) => ['finished', 'error', 'cancelled'].includes(item.status)) && <button className="secondary-button" onClick={clearFinished}><Trash2 size={15} /> Clear completed</button>}
            </div>

            {items.length === 0 ? (
              <div className="queue-empty"><Package size={24} /><h3>Nothing queued yet</h3><p>Analyze a link or send one from the browser extension.</p><button className="secondary-button" onClick={() => setActiveTab('download')}>Go to downloader</button></div>
            ) : (
              <div className="queue-list">
                {items.map((item) => (
                  <article className="queue-item" key={item.id}>
                    <div className={`queue-status-icon ${item.status}`}>
                      {item.status === 'finished' ? <CheckCircle2 size={18} /> : item.status === 'error' ? <XCircle size={18} /> : item.mode === 'audio' ? <Music2 size={18} /> : <Video size={18} />}
                    </div>
                    <div className="queue-main">
                      <div className="queue-title-row">
                        <div><strong>{item.title}</strong><span>{item.mode === 'audio' ? 'Audio' : item.quality === 'best' ? 'Best quality' : `${item.quality}p`} · {priorityLabel(item.priority ?? 0)}</span></div>
                        <span className={`status-chip ${item.status}`}>{statusLabel(item.status)}</span>
                      </div>
                      <div className="progress-track"><div className="progress-fill" style={{ width: `${Math.max(0, Math.min(100, item.progress || 0))}%` }} /></div>
                      <div className="queue-meta">
                        {item.speed && <span>{item.speed}</span>}
                        {item.eta && <span>ETA {item.eta}</span>}
                        {item.scheduledAtMs && item.status === 'scheduled' && <span><Clock size={12} /> {new Date(item.scheduledAtMs).toLocaleString()}</span>}
                        {item.message && <span>{item.message}</span>}
                        {item.filename && item.status === 'finished' && <span className="file-path">{item.filename}</span>}
                      </div>
                    </div>
                    <div className="queue-actions">
                      {['queued', 'scheduled'].includes(item.status) && (
                        <select value={item.priority ?? 0} onChange={(event) => changeQueuedPriority(item.id, Number(event.target.value))} aria-label="Queue priority">
                          <option value={10}>High</option><option value={0}>Normal</option><option value={-10}>Low</option>
                        </select>
                      )}
                      {item.status === 'finished' && item.filename && <button className="icon-button" onClick={() => setPreviewItem(item)} title="Preview media"><Eye size={15} /></button>}
                      {['queued', 'scheduled', 'downloading', 'processing'].includes(item.status) && <button className="icon-button danger" onClick={() => cancel(item.id)} title="Cancel"><Square size={14} fill="currentColor" /></button>}
                    </div>
                  </article>
                ))}
              </div>
            )}
          </div>
        </section>
      )}

      {activeTab === 'settings' && (
        <section className="settings-page">
          <div className="settings-heading"><span className="kicker">SETTINGS</span><h1>Integrations & defaults</h1><p>Everything runs locally; these controls change how LinkForge interacts with your machine.</p></div>

          <div className="settings-grid">
            <div className="panel settings-card">
              <div className="card-icon"><Clipboard size={18} /></div>
              <h2>Clipboard detection</h2>
              <p>Watch your local clipboard for copied HTTP links and offer them in the downloader.</p>
              <label className="toggle-row"><span>Detect copied links</span><input type="checkbox" checked={watchClipboard} onChange={(event) => setWatchClipboard(event.target.checked)} /></label>
            </div>

            <div className="panel settings-card">
              <div className="card-icon"><Globe size={18} /></div>
              <h2>Browser extension</h2>
              <p>The included Chromium extension sends the current tab or context-menu link to LinkForge over localhost only.</p>
              <button className="secondary-button full" onClick={openExtensionFolder}><FolderOpen size={15} /> Open extension folder</button>
              <small>Chrome / Edge: Extensions → Developer mode → Load unpacked.</small>
            </div>

            <div className="panel settings-card">
              <div className="card-icon"><SlidersHorizontal size={18} /></div>
              <h2>Queue engine</h2>
              <p>Choose how many downloads may run simultaneously. Priority controls determine which waiting job starts next.</p>
              <label><span>Concurrent downloads</span><select value={maxConcurrent} onChange={(event) => setMaxConcurrent(Number(event.target.value))}>{[1, 2, 3, 4, 5, 6].map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
            </div>

            <div className="panel settings-card tool-card">
              <div className="card-icon"><Settings size={18} /></div>
              <h2>Download tools</h2>
              <div className="dependency-row"><span>yt-dlp</span><b className={deps?.ytDlp ? 'ok' : 'missing'}>{deps?.ytDlp ? 'Ready' : 'Missing'}</b></div>
              <div className="dependency-row"><span>FFmpeg</span><b className={deps?.ffmpeg ? 'ok' : 'missing'}>{deps?.ffmpeg ? 'Ready' : 'Missing'}</b></div>
              <div className="tool-actions"><button className="secondary-button" onClick={updateDownloader}><RefreshCw size={15} /> Update yt-dlp</button><button className="secondary-button" onClick={installDependencies}><Download size={15} /> Install tools</button></div>
              {toolMessage && <pre className="tool-message">{toolMessage}</pre>}
            </div>
          </div>

          <div className="panel presets-panel">
            <div className="panel-heading"><div><span className="kicker">SITE PRESETS</span><h2>Saved download profiles</h2></div><span className="muted">{Object.keys(sitePresets).length} saved</span></div>
            {Object.keys(sitePresets).length === 0 ? (
              <div className="preset-empty">Save a profile from the Download tab to automatically reuse settings for that site.</div>
            ) : (
              <div className="preset-list">
                {Object.entries(sitePresets).sort(([a], [b]) => a.localeCompare(b)).map(([host, preset]) => (
                  <div className="preset-row" key={host}>
                    <div className="preset-site"><Globe size={16} /><div><strong>{host}</strong><span>{preset.mode === 'audio' ? 'Audio' : preset.quality === 'best' ? 'Best video' : `${preset.quality}p video`} · {preset.transcodePreset || 'source'} · {priorityLabel(preset.priority ?? 0)} priority</span></div></div>
                    <div className="preset-actions"><button className="secondary-button" onClick={() => { applyPreset(preset, host); setActiveTab('download'); }}>Apply</button><button className="icon-button danger" onClick={() => removePreset(host)} title="Delete preset"><Trash2 size={15} /></button></div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>
      )}

      {previewItem?.filename && (
        <div className="preview-backdrop" onClick={() => setPreviewItem(null)}>
          <section className="preview-modal" onClick={(event) => event.stopPropagation()}>
            <div className="preview-heading"><div><span className="kicker">LOCAL PREVIEW</span><h2>{previewItem.title}</h2></div><button className="icon-button" onClick={() => setPreviewItem(null)}><X size={16} /></button></div>
            {previewItem.mode === 'audio' || /\.(mp3|m4a|aac|flac|wav|ogg)$/i.test(previewItem.filename)
              ? <audio src={convertFileSrc(previewItem.filename)} controls autoPlay className="media-player audio-player" />
              : <video src={convertFileSrc(previewItem.filename)} controls autoPlay className="media-player" />}
            <div className="preview-path">{previewItem.filename}</div>
          </section>
        </div>
      )}

      <footer className="app-footer">LinkForge does not bypass DRM or access controls. Use it only for media you own or have permission to download.</footer>
    </main>
  );
}

export default App;
