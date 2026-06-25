// extension/popup.js

const sendBtn = document.getElementById('send-btn');
const urlPreview = document.getElementById('url-preview');
const hint = document.getElementById('hint');
const statusEl = document.getElementById('status');

let currentUrl = '';
let serverUrl = '';
let apiKey = '';

function setReady() {
  sendBtn.disabled = false;
  hint.innerHTML = '';
}

function setUnconfigured() {
  sendBtn.disabled = true;
  hint.innerHTML = 'Configure your server in <a id="settings-link">Settings</a>.';
  document.getElementById('settings-link')?.addEventListener('click', () => {
    chrome.runtime.openOptionsPage();
  });
}

// Load config and current tab URL in parallel
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
    setReady();
  } else {
    setUnconfigured();
  }
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
  } catch (e) {
    statusEl.className = 'error';
    statusEl.textContent = 'Could not reach server. Check Settings.';
  } finally {
    sendBtn.disabled = false;
  }
});
