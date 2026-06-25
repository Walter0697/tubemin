// extension/settings.js

const serverUrlInput = document.getElementById('server-url');
const apiKeyInput = document.getElementById('api-key');
const keyDisplay = document.getElementById('key-display');
const keyInputDiv = document.getElementById('key-input');
const keyMasked = document.getElementById('key-masked');
const changeBtn = document.getElementById('change-btn');
const saveBtn = document.getElementById('save-btn');
const testBtn = document.getElementById('test-btn');
const status = document.getElementById('status');

function maskKey(key) {
  if (!key || key.length <= 4) return '••••';
  return '••••••••' + key.slice(-4);
}

function showMasked(key) {
  keyMasked.textContent = maskKey(key);
  keyDisplay.style.display = 'block';
  keyInputDiv.style.display = 'none';
}

function showInput() {
  keyDisplay.style.display = 'none';
  keyInputDiv.style.display = 'block';
  apiKeyInput.value = '';
  apiKeyInput.focus();
}

// Load saved values on open
chrome.storage.sync.get(['serverUrl', 'apiKey'], (data) => {
  if (data.serverUrl) serverUrlInput.value = data.serverUrl;
  if (data.apiKey) {
    showMasked(data.apiKey);
  }
});

changeBtn.addEventListener('click', showInput);

testBtn.addEventListener('click', async () => {
  const url = serverUrlInput.value.trim().replace(/\/$/, '');
  const key = apiKeyInput.style.display !== 'none' && apiKeyInput.value.trim()
    ? apiKeyInput.value.trim()
    : await new Promise((resolve) => chrome.storage.sync.get(['apiKey'], (d) => resolve(d.apiKey || '')));

  if (!url || !key) {
    status.className = 'err';
    status.textContent = 'Enter a server URL and API key first.';
    return;
  }

  testBtn.disabled = true;
  status.className = '';
  status.textContent = 'Testing…';

  try {
    const resp = await fetch(`${url}/api/validate`, { headers: { 'X-API-Key': key } });
    if (resp.ok) {
      status.className = 'ok';
      status.textContent = 'Connected successfully.';
    } else if (resp.status === 401) {
      status.className = 'err';
      status.textContent = 'Invalid API key.';
    } else {
      status.className = 'err';
      status.textContent = `Server error (${resp.status}).`;
    }
  } catch {
    status.className = 'err';
    status.textContent = 'Cannot reach server.';
  } finally {
    testBtn.disabled = false;
  }
});

saveBtn.addEventListener('click', () => {
  const serverUrl = serverUrlInput.value.trim().replace(/\/$/, '');
  const newKey = apiKeyInput.value.trim();

  if (!serverUrl) {
    status.style.color = '#c00';
    status.textContent = 'Server URL is required.';
    return;
  }

  const toSave = { serverUrl };

  // Only update key if a new one was entered
  if (newKey) {
    toSave.apiKey = newKey;
    chrome.storage.sync.set(toSave, () => {
      showMasked(newKey);
      status.style.color = '#2a2';
      status.textContent = 'Saved.';
    });
  } else {
    // Save URL only; keep existing key
    chrome.storage.sync.get(['apiKey'], (existing) => {
      if (!existing.apiKey) {
        status.style.color = '#c00';
        status.textContent = 'API key is required.';
        return;
      }
      chrome.storage.sync.set(toSave, () => {
        status.style.color = '#2a2';
        status.textContent = 'Saved.';
      });
    });
  }
});
