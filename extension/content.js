// extension/content.js

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type === 'getTitle') {
    const og = document.querySelector('meta[property="og:title"]')?.content?.trim();
    reply({ title: og || document.title?.trim() || '' });
  }
});
