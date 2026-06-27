// extension/popup.js

const sendBtn = document.getElementById('send-btn');
const dashboardBtn = document.getElementById('dashboard-btn');
const urlSite = document.getElementById('url-site');
const urlPreview = document.getElementById('url-preview');
const hint = document.getElementById('hint');
const statusEl = document.getElementById('status');
const settingsLink = document.getElementById('settings-link');

let currentUrl = '';
let serverUrl = '';
let apiKey = '';

settingsLink.addEventListener('click', () => chrome.runtime.openOptionsPage());

dashboardBtn.addEventListener('click', () => {
  if (serverUrl) chrome.tabs.create({ url: `${serverUrl}/dashboard` });
});

function setHint(text, warn = false) {
  hint.textContent = text;
  hint.className = warn ? 'warn' : '';
}

async function validateConnection() {
  try {
    const resp = await fetch(`${serverUrl}/api/validate`, {
      headers: { 'X-API-Key': apiKey },
    });
    if (resp.ok) {
      // intentionally blank — don't clear hints set by other checks
    } else if (resp.status === 401) {
      sendBtn.disabled = true;
      setHint('Invalid API key — check Settings.');
    } else {
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
      setHint("This site isn't supported by yt-dlp.", true);
    }
  } catch {
    // server unreachable — validateConnection will surface that
  }
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
  } catch {
    // non-fatal
  }
}

Promise.all([
  new Promise((resolve) => {
    chrome.storage.sync.get(['serverUrl', 'apiKey'], (data) => {
      serverUrl = (data.serverUrl || '').trim().replace(/\/$/, '');
      apiKey = (data.apiKey || '').trim();
      resolve();
    });
  }),
  new Promise((resolve) => {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      currentUrl = tabs[0]?.url || '';
      urlPreview.textContent = currentUrl || 'No URL detected';
      try {
        urlSite.textContent = new URL(currentUrl).hostname.replace(/^www\./, '');
      } catch {
        urlSite.textContent = '';
      }
      resolve();
    });
  }),
]).then(() => {
  if (serverUrl && apiKey) {
    sendBtn.disabled = false;  // enable immediately; checks run in background
    dashboardBtn.disabled = false;
    validateConnection();
    checkUrlSupported();
    checkExistingSubmission();
  } else {
    dashboardBtn.disabled = !serverUrl;
    setHint('Configure your server in Settings.');
  }
}).catch(() => {
  setHint('Extension error — try reloading.');
});

sendBtn.addEventListener('click', async () => {
  sendBtn.disabled = true;
  statusEl.className = '';
  statusEl.textContent = 'Sending…';

  let succeeded = false;
  try {
    const resp = await fetch(`${serverUrl}/api/submit`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-API-Key': apiKey,
      },
      body: JSON.stringify({ url: currentUrl }),
    });

    if (resp.ok) {
      succeeded = true;
      sendBtn.textContent = 'Queued ✓';
      setHint('');
      statusEl.className = '';
      statusEl.textContent = '';
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
});
