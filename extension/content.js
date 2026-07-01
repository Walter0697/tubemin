// extension/content.js

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type === 'getTitle') {
    const og = document.querySelector('meta[property="og:title"]')?.content?.trim();

    // Collect subtitle/caption <track> elements from all <video> elements on the page
    const seen = new Set();
    const subtitleTracks = [];
    document.querySelectorAll('video track').forEach(track => {
      const kind = track.kind;
      if (kind !== 'subtitles' && kind !== 'captions') return;
      const lang = (track.srclang || 'und').toLowerCase();
      const src = track.src ? new URL(track.src, document.baseURI).href : null;
      if (!src) return;
      const key = `${lang}:${src}`;
      if (seen.has(key)) return;
      seen.add(key);
      subtitleTracks.push({ lang, src });
    });

    reply({
      title: og || document.title?.trim() || '',
      subtitleTracks: subtitleTracks.length > 0 ? subtitleTracks : undefined,
    });
  }
});
