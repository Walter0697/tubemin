// extension/settings.js

const serverUrlInput = document.getElementById('server-url');
const apiKeyInput = document.getElementById('api-key');
const keyDisplay = document.getElementById('key-display');
const keyInputDiv = document.getElementById('key-input');
const keyMasked = document.getElementById('key-masked');
const changeBtn = document.getElementById('change-btn');
const saveBtn = document.getElementById('save-btn');
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
