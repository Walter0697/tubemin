function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>'"]/g, character => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;'
  }[character]));
}

function renderTransfers(items) {
  const root = document.getElementById('transfer-history');
  if (!items.length) {
    root.innerHTML = '<p class="empty-msg">No transfers recorded yet.</p>';
    return;
  }
  root.innerHTML = '<table class="transfer-history-table"><thead><tr>' +
    '<th>Title</th><th>Consumer</th><th>Destination</th><th>State</th><th>Updated</th><th>Error</th>' +
    '</tr></thead><tbody>' + items.map(item => '<tr>' +
    '<td>' + escapeHtml(item.source_title || item.peertube_uuid) + '</td>' +
    '<td>' + escapeHtml(item.consumer) + '</td>' +
    '<td>' + escapeHtml(item.destination) + '</td>' +
    '<td><span class="transfer-state"><span class="status-dot status-' + escapeHtml(item.state) + '"></span>' + escapeHtml(item.state) + '</span></td>' +
    '<td>' + escapeHtml(item.updated_at) + '</td>' +
    '<td>' + escapeHtml(item.error || '') + '</td>' +
    '</tr>').join('') + '</tbody></table>';
}

fetch('/api/transfers')
  .then(response => response.ok ? response.json() : Promise.reject(new Error('Could not load transfer history')))
  .then(body => renderTransfers(body.transfers || []))
  .catch(error => {
    document.getElementById('transfer-history').innerHTML =
      '<p class="error">' + escapeHtml(error.message) + '</p>';
  });
