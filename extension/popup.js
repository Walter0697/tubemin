// extension/popup.js

const sendBtn = document.getElementById('send-btn');
const urlPreview = document.getElementById('url-preview');
const hint = document.getElementById('hint');
const statusEl = document.getElementById('status');
const settingsLink = document.getElementById('settings-link');

let currentUrl = '';
let serverUrl = '';
let apiKey = '';

settingsLink.addEventListener('click', () => chrome.runtime.openOptionsPage());

async function validateConnection() {
  try {
    const resp = await fetch(`${serverUrl}/api/validate`, {
      headers: { 'X-API-Key': apiKey },
    });
    if (resp.ok) {
      hint.textContent = '';
    } else if (resp.status === 401) {
      sendBtn.disabled = true;
      hint.textContent = 'Invalid API key — check Settings.';
    } else {
      hint.textContent = `Server error (${resp.status}).`;
    }
  } catch {
    hint.textContent = 'Cannot reach server — check Settings.';
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
      resolve();
    });
  }),
]).then(() => {
  if (serverUrl && apiKey) {
    sendBtn.disabled = false;  // enable immediately; validation runs in background
    validateConnection();
  } else {
    hint.textContent = 'Configure your server in Settings.';
  }
}).catch(() => {
  hint.textContent = 'Extension error — try reloading.';
});

sendBtn.addEventListener('click', async () => {
  sendBtn.disabled = true;
  statusEl.className = '';
  statusEl.textContent = 'Sending…';

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
      statusEl.className = 'success';
      statusEl.textContent = 'Queued!';
    } else if (resp.status === 401) {
      statusEl.className = 'error';
      statusEl.textContent = 'Invalid API key. Check Settings.';
    } else {
      statusEl.className = 'error';
      statusEl.textContent = `Error ${resp.status}. Check server logs.`;
    }
  } catch {
    statusEl.className = 'error';
    statusEl.textContent = 'Could not reach server. Check Settings.';
  } finally {
    sendBtn.disabled = false;
  }
});
