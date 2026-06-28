// extension/background.js

const CACHE_TTL_MS = 4 * 60 * 60 * 1000; // 4 hours

// ── Storage helpers (keyed by domain hostname) ────────────────────────────

function domainKey(hostname) {
  return `domain_${hostname}`;
}

async function getDomainList(hostname) {
  const data = await chrome.storage.session.get(domainKey(hostname));
  const list = data[domainKey(hostname)] || [];
  // Filter expired entries inline
  const cutoff = Date.now() - CACHE_TTL_MS;
  return list.filter(v => v.capturedAt > cutoff);
}

async function setDomainList(hostname, list) {
  await chrome.storage.session.set({ [domainKey(hostname)]: list });
}

async function tabHostname(tabId) {
  try {
    const tab = await chrome.tabs.get(tabId);
    return new URL(tab.url).hostname;
  } catch { return null; }
}

// ── m3u8 manifest analyser ────────────────────────────────────────────────
// Returns { duration: seconds|null, isMaster: bool|null }
// isMaster=true  → contains #EXT-X-STREAM-INF (master/multivariant playlist)
// isMaster=false → contains #EXTINF (media/variant playlist)
// isMaster=null  → fetch failed, can't tell

async function analyzeM3u8(url, cookies, referer) {
  try {
    const headers = { 'User-Agent': 'Mozilla/5.0' };
    if (referer) headers['Referer'] = referer;
    if (cookies) headers['Cookie'] = cookies;
    const res = await fetch(url, { headers });
    if (!res.ok) return { duration: null, isMaster: null };
    const text = await res.text();

    if (text.includes('#EXT-X-STREAM-INF')) {
      // Master playlist — follow first variant URL to get actual duration
      let variantUrl = null;
      for (const line of text.split('\n')) {
        const t = line.trim();
        if (t && !t.startsWith('#')) {
          variantUrl = t.startsWith('http') ? t : new URL(t, url).href;
          break;
        }
      }
      let duration = null;
      if (variantUrl) {
        try {
          const vr = await fetch(variantUrl, { headers });
          if (vr.ok) {
            const vt = await vr.text();
            let total = 0;
            for (const m of vt.matchAll(/#EXTINF:([\d.]+)/g)) total += parseFloat(m[1]);
            if (total > 0) duration = Math.round(total);
          }
        } catch {}
      }
      return { duration, isMaster: true };
    } else {
      // Subtitle-only playlist (WebVTT segments) — not a video stream, discard
      const hasVtt = text.split('\n').some(l => {
        const t = l.trim();
        return t && !t.startsWith('#') && t.split('?')[0].endsWith('.vtt');
      });
      if (hasVtt) return { duration: null, isMaster: null, isSubtitle: true };

      // Media/variant playlist
      let total = 0;
      for (const m of text.matchAll(/#EXTINF:([\d.]+)/g)) total += parseFloat(m[1]);
      return { duration: total > 0 ? Math.round(total) : null, isMaster: false };
    }
  } catch {
    return { duration: null, isMaster: null };
  }
}

// ── URL helpers ───────────────────────────────────────────────────────────

// Strip query params — used as dedup key so expiry tokens don't create duplicates.
function pathKey(url) { return url.split('?')[0]; }

// ── Segment filter ────────────────────────────────────────────────────────

function isSegmentUrl(url) {
  const path = url.split('?')[0].toLowerCase();
  const seg = path.split('/').pop();
  if (seg === 'init.mp4' || seg === 'header.mp4') return true;
  if (/^(?:seg|chunk|fragment|part|frag)-?\d+\.mp4$/.test(seg)) return true;
  if (/^\d+\.mp4$/.test(seg)) return true;
  if (/\/audio[_\/]/.test(path) && path.endsWith('.mp4')) return true;
  return false;
}

// ── Badge ─────────────────────────────────────────────────────────────────

async function updateBadge(tabId, hostname) {
  if (tabId < 0) return;
  const list = await getDomainList(hostname);
  const count = list.length;
  chrome.action.setBadgeText({ text: count > 0 ? String(count) : '', tabId });
  if (count > 0) chrome.action.setBadgeBackgroundColor({ color: '#ef4444', tabId });
}

// Clear badge when the user navigates away from a page
chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === 'loading' && changeInfo.url) {
    chrome.action.setBadgeText({ text: '', tabId });
  }
});

// ── Intercept handler ─────────────────────────────────────────────────────

chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (details.tabId < 0) return;
    if (!/\.(m3u8|mpd|mp4)(\?|$)/i.test(details.url)) return;
    if (details.initiator?.startsWith('chrome-extension://')) return;
    if (isSegmentUrl(details.url)) return;
    handleIntercept(details);
  },
  { urls: ['<all_urls>'] }
);

async function handleIntercept(details) {
  // Always key by the main tab's hostname, not the request's documentUrl/initiator.
  // Video requests often fire from iframes on a different origin, so
  // details.documentUrl != the page the user is actually looking at.
  let hostname = null;
  let title = null;
  let tabUrl = '';
  try {
    const tab = await chrome.tabs.get(details.tabId);
    hostname = new URL(tab.url).hostname;
    tabUrl = tab.url;
    title = tab.title?.trim() || null;
  } catch { return; }

  // Prefer og:title from content script if available
  try {
    const res = await chrome.tabs.sendMessage(details.tabId, { type: 'getTitle' });
    if (res?.title) title = res.title;
  } catch {}

  let cookies = null;
  try {
    const jar = await chrome.cookies.getAll({ url: details.url });
    if (jar.length > 0) cookies = jar.map(c => `${c.name}=${c.value}`).join('; ');
  } catch {}

  const pageUrl = details.documentUrl || details.initiator || '';
  const isHls = /\.m3u8(\?|$)/i.test(details.url);

  const entry = {
    videoUrl: details.url,
    pageUrl,
    sourceUrl: tabUrl,
    title,
    cookies,
    capturedAt: Date.now(),
    duration: null,  // filled async below for m3u8
    isMaster: null,  // filled async below for m3u8
  };

  // Deduplicate by URL path — expiry tokens in query params change on each page
  // load but the path uniquely identifies the same stream.
  const list = await getDomainList(hostname);
  const deduped = list.filter(v => pathKey(v.videoUrl) !== pathKey(entry.videoUrl));
  deduped.push(entry);
  await setDomainList(hostname, deduped);
  updateBadge(details.tabId, hostname);

  // Async: analyse the m3u8 manifest for duration and master/variant status,
  // then prune: keep master playlists, drop variants when a master exists.
  if (isHls) {
    const { duration, isMaster, isSubtitle } = await analyzeM3u8(details.url, cookies, pageUrl);

    // Subtitle-only tracks have no video — remove from capture list
    if (isSubtitle) {
      const current = await getDomainList(hostname);
      await setDomainList(hostname, current.filter(v => pathKey(v.videoUrl) !== pathKey(entry.videoUrl)));
      updateBadge(details.tabId, hostname);
      return;
    }

    const current = await getDomainList(hostname);
    const myIdx = current.findIndex(v => pathKey(v.videoUrl) === pathKey(entry.videoUrl));
    if (myIdx === -1) return;

    current[myIdx].duration = duration;
    current[myIdx].isMaster = isMaster;

    let final;
    if (isMaster === true) {
      // We're the master — remove any variant playlists
      final = current.filter((v, i) => i === myIdx || v.isMaster !== false);
    } else if (isMaster === false) {
      // We're a variant — remove ourselves if a master already exists
      const hasMaster = current.some((v, i) => i !== myIdx && v.isMaster === true);
      final = hasMaster ? current.filter((_, i) => i !== myIdx) : current;
    } else {
      final = current;
    }
    await setDomainList(hostname, final);
    updateBadge(details.tabId, hostname);
  }
}

// ── Tab close cleanup ─────────────────────────────────────────────────────
// Remove a domain's cache only when no remaining tab is on that domain.

chrome.tabs.onRemoved.addListener(async (tabId, removeInfo) => {
  // Get the hostname of the closed tab from existing tabs
  const allTabs = await chrome.tabs.query({});
  // We can't know the closed tab's URL after removal, so we do a full sweep:
  // collect all active domains, then remove any stored domain not in that set.
  const activeDomains = new Set();
  for (const tab of allTabs) {
    try { activeDomains.add(new URL(tab.url).hostname); } catch {}
  }
  const all = await chrome.storage.session.get(null);
  const toRemove = Object.keys(all).filter(k => {
    if (!k.startsWith('domain_')) return false;
    const host = k.slice('domain_'.length);
    return !activeDomains.has(host);
  });
  if (toRemove.length) await chrome.storage.session.remove(toRemove);
});

// ── Message handler ───────────────────────────────────────────────────────

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type === 'getVideos') {
    getDomainList(msg.hostname).then(list => reply({ list }));
    return true;
  }
  if (msg.type === 'clearBadge') {
    chrome.action.setBadgeText({ text: '', tabId: msg.tabId });
  }
});
