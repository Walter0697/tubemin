# Tubemin Chrome Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Manifest V3 Chrome extension that sends the active tab's URL to a Tubemin server via API key authentication, with a settings page to configure and manage the server URL and API key.

**Architecture:** Pure HTML/CSS/JS, no build step. Popup reads `chrome.storage.sync` for config; if unset, disables the submit button. Settings page stores server URL and API key, masking the key after save.

**Tech Stack:** Chrome Extension Manifest V3, Vanilla JS, `chrome.storage.sync`, `chrome.tabs`, `fetch` API

## Global Constraints

- No build step — plain HTML/JS files loadable directly in Chrome via "Load unpacked"
- `chrome.storage.sync` for all persistent values (roams across Chrome profile)
- API key displayed masked after save (`••••••••` + last 4 chars), with a "Change" button to re-enable
- Submit button disabled + shows hint text if server URL or API key is not configured
- Request: `POST <serverUrl>/api/submit` with header `X-API-Key: <key>` and body `{"url": "<tab url>"}`
- Naming in copy: "Send to Tubemin" — no platform names

---

### Task 1: Scaffold and Manifest

**Files:**
- Create: `extension/manifest.json`
- Create: `extension/icons/icon16.png` (placeholder)
- Create: `extension/icons/icon48.png` (placeholder)
- Create: `extension/icons/icon128.png` (placeholder)

**Interfaces:**
- Produces: loadable Chrome extension (no functionality yet, just valid manifest)

- [ ] **Step 1: Create extension directory**

```bash
mkdir -p /Users/walter/Documents/git/tubemin/extension/icons
```

- [ ] **Step 2: Write manifest.json**

```json
{
  "manifest_version": 3,
  "name": "Tubemin",
  "version": "1.0.0",
  "description": "Send MeTube-supported URLs to your Tubemin server.",
  "permissions": ["activeTab", "storage"],
  "icons": {
    "16": "icons/icon16.png",
    "48": "icons/icon48.png",
    "128": "icons/icon128.png"
  },
  "action": {
    "default_popup": "popup.html",
    "default_icon": {
      "16": "icons/icon16.png",
      "48": "icons/icon48.png"
    }
  },
  "options_page": "settings.html"
}
```

- [ ] **Step 3: Generate placeholder icons**

Create 3 placeholder PNG files (1×1 pixel green PNG, base64 encoded):

```bash
# Minimal valid 1x1 green PNG (base64)
GREEN_PNG="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
echo "$GREEN_PNG" | base64 -d > /Users/walter/Documents/git/tubemin/extension/icons/icon16.png
cp /Users/walter/Documents/git/tubemin/extension/icons/icon16.png /Users/walter/Documents/git/tubemin/extension/icons/icon48.png
cp /Users/walter/Documents/git/tubemin/extension/icons/icon16.png /Users/walter/Documents/git/tubemin/extension/icons/icon128.png
```

- [ ] **Step 4: Load in Chrome to verify**

1. Open `chrome://extensions`
2. Enable "Developer mode" (top right)
3. Click "Load unpacked"
4. Select `/Users/walter/Documents/git/tubemin/extension`

Expected: Tubemin appears in extension list with no errors

- [ ] **Step 5: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add extension/
git commit -m "feat: scaffold Chrome extension with Manifest V3"
```

---

### Task 2: Settings Page

**Files:**
- Create: `extension/settings.html`
- Create: `extension/settings.js`

**Interfaces:**
- Produces: settings page with server URL and API key fields
- Saves to: `chrome.storage.sync` keys `serverUrl` and `apiKey`
- After save: API key field replaced by masked display + "Change" button

- [ ] **Step 1: Write settings.html**

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Tubemin Settings</title>
  <style>
    body {
      font-family: sans-serif;
      max-width: 480px;
      margin: 32px auto;
      padding: 0 20px;
      color: #222;
    }
    h1 { font-size: 1.25rem; margin-bottom: 1.5rem; }
    label { display: block; font-size: 0.875rem; margin-bottom: 4px; color: #555; }
    input[type="text"], input[type="password"] {
      width: 100%;
      box-sizing: border-box;
      padding: 8px 10px;
      border: 1px solid #ccc;
      border-radius: 4px;
      font-size: 0.9rem;
      margin-bottom: 16px;
    }
    .field-row { display: flex; gap: 8px; align-items: center; margin-bottom: 16px; }
    .field-row input { margin-bottom: 0; flex: 1; }
    .masked { font-family: monospace; font-size: 0.9rem; color: #444; }
    button {
      padding: 8px 16px;
      border: none;
      border-radius: 4px;
      cursor: pointer;
      font-size: 0.875rem;
    }
    #save-btn { background: #2563eb; color: white; width: 100%; padding: 10px; }
    #save-btn:hover { background: #1d4ed8; }
    #change-btn { background: #f0f0f0; color: #333; }
    #status { font-size: 0.875rem; margin-top: 8px; color: #2a2; min-height: 1.2em; }
  </style>
</head>
<body>
  <h1>Tubemin Settings</h1>

  <label for="server-url">Server URL</label>
  <input type="text" id="server-url" placeholder="https://tubemin.yourdomain.com" />

  <label>API Key</label>
  <div id="key-display" style="display:none">
    <div class="field-row">
      <span class="masked" id="key-masked"></span>
      <button id="change-btn" type="button">Change</button>
    </div>
  </div>
  <div id="key-input">
    <input type="password" id="api-key" placeholder="Paste API key from Tubemin settings" />
  </div>

  <button id="save-btn" type="button">Save</button>
  <div id="status"></div>

  <script src="settings.js"></script>
</body>
</html>
```

- [ ] **Step 2: Write settings.js**

```javascript
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
```

- [ ] **Step 3: Manual test — settings page**

1. Go to `chrome://extensions`, click "Tubemin" → "Details" → "Extension options"
2. Verify: both fields empty, Save works, API key shows masked after save, Change button re-shows input

- [ ] **Step 4: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add extension/settings.html extension/settings.js
git commit -m "feat: add settings page with server URL and masked API key storage"
```

---

### Task 3: Popup

**Files:**
- Create: `extension/popup.html`
- Create: `extension/popup.js`

**Interfaces:**
- Consumes: `chrome.storage.sync` → `serverUrl`, `apiKey`
- Consumes: `chrome.tabs.query` → active tab URL
- Produces: "Send to Tubemin" button — disabled with hint if config missing, enabled otherwise
- On click: `POST <serverUrl>/api/submit` with `X-API-Key` header → shows success or error inline

- [ ] **Step 1: Write popup.html**

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Tubemin</title>
  <style>
    body {
      font-family: sans-serif;
      width: 280px;
      padding: 16px;
      margin: 0;
      color: #222;
    }
    h1 { font-size: 1rem; margin: 0 0 12px; }
    #url-preview {
      font-size: 0.75rem;
      color: #666;
      margin-bottom: 12px;
      word-break: break-all;
      min-height: 1em;
    }
    #send-btn {
      width: 100%;
      padding: 10px;
      background: #2563eb;
      color: white;
      border: none;
      border-radius: 4px;
      font-size: 0.9rem;
      cursor: pointer;
    }
    #send-btn:hover:not(:disabled) { background: #1d4ed8; }
    #send-btn:disabled {
      background: #ccc;
      cursor: not-allowed;
    }
    #hint {
      font-size: 0.75rem;
      color: #888;
      margin-top: 8px;
      min-height: 1em;
    }
    #hint a { color: #2563eb; cursor: pointer; text-decoration: underline; }
    #status {
      font-size: 0.8rem;
      margin-top: 10px;
      min-height: 1.2em;
    }
    .success { color: #2a2; }
    .error { color: #c00; }
  </style>
</head>
<body>
  <h1>Tubemin</h1>
  <div id="url-preview"></div>
  <button id="send-btn" type="button" disabled>Send to Tubemin</button>
  <div id="hint"></div>
  <div id="status"></div>

  <script src="popup.js"></script>
</body>
</html>
```

- [ ] **Step 2: Write popup.js**

```javascript
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
```

- [ ] **Step 3: Manual test — popup disabled state**

1. Clear storage: open extension settings, clear both fields, save with empty key (use DevTools → Application → Storage → chrome.storage.sync → clear)
2. Click Tubemin toolbar icon
3. Expected: button disabled, hint text with settings link visible

- [ ] **Step 4: Manual test — popup enabled state**

1. Open extension settings, set server URL + API key, save
2. Navigate to any URL in Chrome
3. Click Tubemin toolbar icon
4. Expected: URL preview shows current tab URL, button enabled
5. Click "Send to Tubemin"
6. Expected: "Queued!" on success, or clear error message on failure

- [ ] **Step 5: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add extension/popup.html extension/popup.js
git commit -m "feat: add popup with Send to Tubemin button and disabled state for missing config"
```
