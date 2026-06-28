// extension/popup.js

const sendBtn          = document.getElementById('send-btn');
const dashboardBtn     = document.getElementById('dashboard-btn');
const urlSite          = document.getElementById('url-site');
const urlPreview       = document.getElementById('url-preview');
const hint             = document.getElementById('hint');
const statusEl         = document.getElementById('status');
const settingsLink     = document.getElementById('settings-link');
const urlSection       = document.getElementById('url-section');
const videoListSection = document.getElementById('video-list-section');
const videoListHeader  = document.getElementById('video-list-header');
const videoList        = document.getElementById('video-list');
const selectAllBtn     = document.getElementById('select-all-btn');
const unselectAllBtn   = document.getElementById('unselect-all-btn');

let currentUrl      = '';
let serverUrl       = '';
let apiKey          = '';
let currentTabId    = null;
let currentHostname = '';
let minDurationSec  = 0;
let mode            = 'url'; // 'url' | 'list'
let capturedVideos  = [];

const STALE_MS = 20 * 60 * 1000;

settingsLink.addEventListener('click', () => chrome.runtime.openOptionsPage());

selectAllBtn.addEventListener('click', () => {
  videoList.querySelectorAll('input[type="checkbox"]').forEach(cb => { cb.checked = true; });
  updateQueueBtn();
});
unselectAllBtn.addEventListener('click', () => {
  videoList.querySelectorAll('input[type="checkbox"]').forEach(cb => { cb.checked = false; });
  updateQueueBtn();
});
dashboardBtn.addEventListener('click', () => {
  if (serverUrl) chrome.tabs.create({ url: `${serverUrl}/dashboard` });
});

function setHint(text, warn = false) {
  hint.textContent = text;
  hint.className = warn ? 'warn' : '';
}

function formatVideoUrl(url) {
  try {
    const u = new URL(url);
    const host = u.hostname.replace(/^www\./, '');
    const seg = u.pathname.split('/').filter(Boolean).pop() || '';
    return seg ? `${host} · ${seg}` : host;
  } catch {
    return url;
  }
}

function formatAge(ms) {
  const m = Math.floor(ms / 60000);
  if (m < 1) return 'just now';
  if (m === 1) return '1m ago';
  return `${m}m ago`;
}

function getVideos(hostname) {
  return new Promise(resolve => {
    chrome.runtime.sendMessage({ type: 'getVideos', hostname }, res => {
      resolve(res?.list || []);
    });
  });
}

// ── List mode ──────────────────────────────────────────────────────────────

function updateQueueBtn() {
  const checked = videoList.querySelectorAll('input:checked').length;
  sendBtn.disabled = checked === 0 || !serverUrl || !apiKey;
  sendBtn.textContent = checked > 0 ? `Queue Selected (${checked})` : 'Queue Selected';
}

function showListMode(videos) {
  mode = 'list';
  capturedVideos = videos;
  urlSection.hidden = true;
  videoListSection.hidden = false;

  const now = Date.now();
  let hostname = '';
  try { hostname = new URL(currentUrl).hostname.replace(/^www\./, ''); } catch {}

  videoListHeader.textContent =
    (hostname ? hostname + ' · ' : '') +
    `${videos.length} video${videos.length !== 1 ? 's' : ''} captured`;

  videoList.innerHTML = '';

  // Newest first
  [...videos].reverse().forEach((item, revIdx) => {
    const origIdx = videos.length - 1 - revIdx;
    const stale = (now - item.capturedAt) > STALE_MS;
    const displayTitle = item.title || 'Unknown';

    const label = document.createElement('label');
    label.className = 'video-item' + (stale ? ' stale' : '');

    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = !stale;
    cb.dataset.idx = String(origIdx);
    cb.addEventListener('change', updateQueueBtn);

    // Right column: title + URL hint stacked
    const col = document.createElement('div');
    col.className = 'video-item-col';

    const titleInput = document.createElement('input');
    titleInput.type = 'text';
    titleInput.className = 'video-item-title';
    titleInput.value = displayTitle;
    titleInput.title = 'Click to rename before queuing';
    titleInput.addEventListener('click', e => e.stopPropagation());

    const urlHint = document.createElement('span');
    urlHint.className = 'video-item-url';
    urlHint.textContent = formatVideoUrl(item.videoUrl);
    urlHint.title = item.videoUrl;

    col.appendChild(titleInput);
    col.appendChild(urlHint);

    const ageSpan = document.createElement('span');
    ageSpan.className = 'video-item-age';
    ageSpan.textContent = formatAge(now - item.capturedAt);

    label.appendChild(cb);
    label.appendChild(col);
    label.appendChild(ageSpan);
    videoList.appendChild(label);
  });

  if (!serverUrl || !apiKey) {
    setHint('Configure your server in Settings.');
    sendBtn.disabled = true;
    sendBtn.textContent = 'Queue Selected';
  } else {
    updateQueueBtn();
  }
}

// ── URL mode ───────────────────────────────────────────────────────────────

async function validateConnection() {
  try {
    const resp = await fetch(`${serverUrl}/api/validate`, {
      headers: { 'X-API-Key': apiKey },
    });
    if (resp.status === 401) {
      sendBtn.disabled = true;
      setHint('Invalid API key — check Settings.');
    } else if (!resp.ok) {
      setHint(`Server error (${resp.status}).`);
    }
  } catch {
    setHint('Cannot reach server — check Settings.');
  }
}

async function checkUrlSupported() {
  if (!serverUrl || !currentUrl) return;
  try {
    const resp = await fetch(
      `${serverUrl}/api/check-url?url=${encodeURIComponent(currentUrl)}`
    );
    if (!resp.ok) {
      sendBtn.disabled = true;
      setHint('Not supported — play the video first.', true);
    }
  } catch {}
}

async function checkExistingSubmission() {
  if (!serverUrl || !currentUrl) return;
  try {
    const resp = await fetch(
      `${serverUrl}/api/check-submission?url=${encodeURIComponent(currentUrl)}`
    );
    if (!resp.ok) return;
    const data = await resp.json();
    if (data.status === 'pending' || data.status === 'downloading') {
      sendBtn.disabled = true;
      setHint('Already in queue.');
    } else if (data.status === 'imported') {
      sendBtn.disabled = true;
      setHint('Already downloaded.');
    } else if (data.status === 'error') {
      setHint('Last attempt failed — retry?', true);
    }
  } catch {}
}

function showUrlMode() {
  mode = 'url';
  urlSection.hidden = false;
  videoListSection.hidden = true;
  sendBtn.textContent = 'Queue Video';

  if (serverUrl && apiKey) {
    sendBtn.disabled = false;
    validateConnection();
    checkUrlSupported();
    checkExistingSubmission();
  } else {
    setHint('Configure your server in Settings.');
  }
}

// ── Init ───────────────────────────────────────────────────────────────────

Promise.all([
  new Promise(resolve => {
    chrome.storage.sync.get(['serverUrl', 'apiKey', 'minDuration'], data => {
      serverUrl      = (data.serverUrl || '').trim().replace(/\/$/, '');
      apiKey         = (data.apiKey || '').trim();
      minDurationSec = parseInt(data.minDuration || '0', 10) * 60;
      resolve();
    });
  }),
  new Promise(resolve => {
    chrome.tabs.query({ active: true, currentWindow: true }, tabs => {
      const tab = tabs[0];
      currentTabId = tab?.id ?? null;
      currentUrl   = tab?.url || '';
      urlPreview.textContent = currentUrl || 'No URL detected';
      try {
        const u = new URL(currentUrl);
        currentHostname = u.hostname;
        urlSite.textContent = u.hostname.replace(/^www\./, '');
      } catch {
        urlSite.textContent = '';
      }
      resolve();
    });
  }),
]).then(async () => {
  if (currentHostname) {
    const videos = await getVideos(currentHostname);
    const filtered = minDurationSec > 0
      ? videos.filter(v => v.duration === null || v.duration >= minDurationSec)
      : videos;
    if (filtered.length > 0) {
      showListMode(filtered);
      return;
    }
  }
  showUrlMode();
}).catch(() => {
  setHint('Extension error — try reloading.');
});

// ── Send handlers ──────────────────────────────────────────────────────────

sendBtn.addEventListener('click', () => {
  if (mode === 'list') handleListQueue();
  else handleUrlQueue();
});

async function handleListQueue() {
  const checkboxes = videoList.querySelectorAll('input:checked');
  const selected = Array.from(checkboxes).map(cb => {
    const item = capturedVideos[parseInt(cb.dataset.idx)];
    const titleInput = cb.parentElement.querySelector('.video-item-col .video-item-title');
    return { ...item, title: titleInput?.value?.trim() || item.title || null };
  });

  sendBtn.disabled = true;
  statusEl.className = '';
  statusEl.textContent = `Queuing ${selected.length}…`;

  let succeeded = 0;
  let failed = 0;

  for (const item of selected) {
    try {
      const resp = await fetch(`${serverUrl}/api/submit`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-API-Key': apiKey },
        body: JSON.stringify({
          url: item.videoUrl,
          referer: item.pageUrl || null,
          source_url: item.sourceUrl || null,
          title: item.title || null,
          cookies: item.cookies || null,
        }),
      });
      if (resp.ok) succeeded++;
      else failed++;
    } catch {
      failed++;
    }
  }

  if (failed === 0) {
    sendBtn.textContent = `Queued ${succeeded} ✓`;
    statusEl.className = '';
    statusEl.textContent = '';
    setHint('');
    if (currentTabId) chrome.runtime.sendMessage({ type: 'clearBadge', tabId: currentTabId });
  } else {
    statusEl.className = 'error';
    statusEl.textContent = `${succeeded} queued, ${failed} failed.`;
    updateQueueBtn();
  }
}

async function handleUrlQueue() {
  sendBtn.disabled = true;
  statusEl.className = '';
  statusEl.textContent = 'Sending…';

  let succeeded = false;
  try {
    const resp = await fetch(`${serverUrl}/api/submit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-API-Key': apiKey },
      body: JSON.stringify({ url: currentUrl }),
    });

    if (resp.ok) {
      succeeded = true;
      sendBtn.textContent = 'Queued ✓';
      setHint('');
      statusEl.className = '';
      statusEl.textContent = '';
      if (currentTabId) chrome.runtime.sendMessage({ type: 'clearBadge', tabId: currentTabId });
    } else if (resp.status === 401) {
      statusEl.className = 'error';
      statusEl.textContent = 'Invalid API key. Check Settings.';
    } else if (resp.status === 422) {
      const data = await resp.json().catch(() => ({}));
      statusEl.className = 'error';
      statusEl.textContent = data.error || 'URL not supported by yt-dlp.';
    } else {
      statusEl.className = 'error';
      statusEl.textContent = `Error ${resp.status}. Check server logs.`;
    }
  } catch {
    statusEl.className = 'error';
    statusEl.textContent = 'Could not reach server. Check Settings.';
  } finally {
    if (!succeeded) sendBtn.disabled = false;
  }
}
