function escapeHtml(value) {
  return String(value ?? '').replace(/[&<>'"]/g, character => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;'
  }[character]));
}

const archiveToolbar = document.getElementById('archive-toolbar');
const archiveSummary = document.getElementById('archive-summary');
const archiveModal = document.getElementById('archive-confirm-modal');
const archiveConfirmCounts = document.getElementById('archive-confirm-counts');
const archiveConfirmButton = document.getElementById('archive-confirm-btn');

function updateArchiveControls(counts) {
  const deletedTransfers = Number(counts?.deletedTransfers || 0);
  const handedOffSubmissions = Number(counts?.handedOffSubmissions || 0);
  const total = deletedTransfers + handedOffSubmissions;
  archiveToolbar.hidden = total === 0;
  if (total === 0) return;
  archiveSummary.textContent = `${deletedTransfers} deleted transfer${deletedTransfers === 1 ? '' : 's'} · ${handedOffSubmissions} legacy handed-off submission${handedOffSubmissions === 1 ? '' : 's'}`;
  archiveConfirmCounts.textContent = `This will remove ${total} archived record${total === 1 ? '' : 's'} (${deletedTransfers} deleted transfer${deletedTransfers === 1 ? '' : 's'} and ${handedOffSubmissions} legacy handed-off submission${handedOffSubmissions === 1 ? '' : 's'}).`;
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

function loadTransfers() {
  return fetch('/api/transfers')
  .then(response => response.ok ? response.json() : Promise.reject(new Error('Could not load transfer history')))
  .then(body => {
    renderTransfers(body.transfers || []);
    updateArchiveControls(body.archiveCounts);
  })
  .catch(error => {
    document.getElementById('transfer-history').innerHTML =
      '<p class="error">' + escapeHtml(error.message) + '</p>';
  });
}

document.getElementById('clear-archived-btn').addEventListener('click', () => {
  archiveModal.hidden = false;
});
document.getElementById('archive-cancel-btn').addEventListener('click', () => {
  archiveModal.hidden = true;
});
archiveModal.addEventListener('click', event => {
  if (event.target === archiveModal) archiveModal.hidden = true;
});
archiveConfirmButton.addEventListener('click', () => {
  archiveConfirmButton.disabled = true;
  fetch('/api/transfers/cleanup-archived', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: '{}',
  })
    .then(response => response.ok ? response.json() : response.json().then(body => Promise.reject(new Error(body.error || 'Could not clear archived records'))))
    .then(body => {
      archiveModal.hidden = true;
      archiveConfirmButton.disabled = false;
      const removed = Number(body.deletedTransfers || 0) + Number(body.handedOffSubmissions || 0);
      window.alert(`Removed ${removed} archived record${removed === 1 ? '' : 's'}.`);
      return loadTransfers();
    })
    .catch(error => {
      archiveConfirmButton.disabled = false;
      window.alert(error.message);
    });
});

loadTransfers();
