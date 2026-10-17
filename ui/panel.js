const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const PAGE = 100;
const ICONS = {
  text: '<path d="M3 4h12M3 8h12M3 12h8"/>',
  richtext: '<path d="M3 4h12M3 8h12M3 12h8"/><path d="M13 11l2 2-2 2"/>',
  image: '<rect x="2.5" y="3.5" width="13" height="11" rx="2"/><circle cx="6.5" cy="7.5" r="1.3"/><path d="M3 13l4-4 3 3 2-2 3 3"/>',
  files: '<path d="M4 2.5h6l4 4v9H4z"/><path d="M10 2.5v4h4"/>',
};
const PIN = '<svg class="pin" viewBox="0 0 12 12" fill="currentColor"><path d="M4 1h4l-.5 3.5L10 7H6.6L6 11l-.6-4H2l2.5-2.5z"/></svg>';

const els = {
  search: document.getElementById('search'),
  list: document.getElementById('list'),
  preview: document.getElementById('preview'),
  previewMeta: document.getElementById('preview-meta'),
  filters: document.getElementById('filters'),
  source: document.getElementById('source'),
  count: document.getElementById('count'),
  toast: document.getElementById('toast'),
};

const state = { items: [], selected: 0, filter: 'all', exhausted: false, loading: false, detailToken: 0 };

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function relativeTime(ms) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 86400 * 7) return `${Math.floor(s / 86400)} d ago`;
  return new Date(ms).toLocaleDateString();
}

function formatBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function query(offset) {
  const f = state.filter;
  return {
    text: els.search.value,
    kind: ['text', 'image', 'files'].includes(f) ? f : null,
    pinned_only: f === 'pinned',
    source_bundle_id: els.source.value || null,
    limit: PAGE,
    offset,
  };
}

async function load({ keepSelection = false } = {}) {
  const selectedId = keepSelection ? state.items[state.selected]?.id : null;
  const items = await invoke('list_items', { query: query(0) });
  state.items = items;
  state.exhausted = items.length < PAGE;
  const idx = selectedId != null ? items.findIndex(i => i.id === selectedId) : -1;
  state.selected = idx >= 0 ? idx : 0;
  render();
}

async function loadMore() {
  if (state.exhausted || state.loading) return;
  state.loading = true;
  try {
    const more = await invoke('list_items', { query: query(state.items.length) });
    state.items = state.items.concat(more);
    state.exhausted = more.length < PAGE;
    renderList();
  } finally {
    state.loading = false;
  }
}

function render() {
  renderList();
  renderPreview();
}

function renderList() {
  if (state.items.length === 0) {
    const msg = els.search.value || state.filter !== 'all' || els.source.value
      ? 'No matching items.'
      : 'Nothing here yet. Copy something and it will appear in this list.';
    els.list.innerHTML = `<div class="empty">${msg}</div>`;
    els.count.textContent = '';
    return;
  }
  els.list.innerHTML = state.items.map((item, i) => {
    const lead = item.kind === 'image' && item.thumb_path
      ? `<img class="thumb" src="${convertFileSrc(item.thumb_path)}" alt="">`
      : `<svg class="kind" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4">${ICONS[item.kind]}</svg>`;
    const meta = [item.source_name, relativeTime(item.last_used_at)].filter(Boolean).join(' \u00b7 ');
    return `<div class="row${i === state.selected ? ' selected' : ''}" data-index="${i}" role="option" aria-selected="${i === state.selected}">
      <span class="key">${i < 9 ? i + 1 : ''}</span>${lead}
      <div class="body"><div class="title">${escapeHtml(item.preview || '(empty)')}</div><div class="meta">${escapeHtml(meta)}</div></div>
      ${item.pinned ? PIN : ''}
    </div>`;
  }).join('');
  els.count.textContent = `${state.items.length}${state.exhausted ? '' : '+'} items`;
  els.list.querySelector('.row.selected')?.scrollIntoView({ block: 'nearest' });
}

async function renderPreview() {
  const item = state.items[state.selected];
  const token = ++state.detailToken;
  if (!item) {
    els.preview.innerHTML = '';
    els.previewMeta.innerHTML = '';
    return;
  }
  const detail = await invoke('item_detail', { id: item.id });
  if (token !== state.detailToken || !detail) return;
  if (detail.image_path) {
    els.preview.innerHTML = `<img src="${convertFileSrc(detail.image_path)}" alt="Copied image">`;
  } else if (detail.files.length) {
    els.preview.innerHTML = `<ul>${detail.files.map(f => `<li>${escapeHtml(f)}</li>`).join('')}</ul>`;
  } else {
    const more = detail.text_truncated ? '\n\n(Preview shortened. The full text is pasted.)' : '';
    els.preview.innerHTML = `<pre>${escapeHtml((detail.text || '') + more)}</pre>`;
  }
  const meta = [];
  const kindLabel = { text: 'Text', richtext: 'Formatted text', image: 'Image', files: item.file_count === 1 ? '1 file' : `${item.file_count} files` }[item.kind];
  meta.push(kindLabel);
  if (item.image_width) meta.push(`${item.image_width} x ${item.image_height}`);
  if (item.kind === 'text' || item.kind === 'richtext') {
    const chars = (detail.text || '').length;
    meta.push(`${chars.toLocaleString()} characters`);
  }
  meta.push(formatBytes(item.byte_size));
  if (item.source_name) meta.push(`From ${item.source_name}`);
  meta.push(new Date(item.created_at).toLocaleString());
  els.previewMeta.innerHTML = meta.map(m => `<span>${escapeHtml(m)}</span>`).join('');
}

function select(i) {
  if (!state.items.length) return;
  state.selected = Math.max(0, Math.min(state.items.length - 1, i));
  renderList();
  renderPreview();
  if (state.selected > state.items.length - 10) loadMore();
}

function toast(msg) {
  els.toast.textContent = msg;
  els.toast.classList.add('show');
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => els.toast.classList.remove('show'), 1600);
}

async function paste(index, mode) {
  const item = state.items[index];
  if (!item) return;
  try {
    await invoke('paste_item', { id: item.id, mode });
  } catch (e) {
    toast(String(e));
  }
}

async function togglePin() {
  const item = state.items[state.selected];
  if (!item) return;
  await invoke('set_pinned', { id: item.id, pinned: !item.pinned });
  toast(item.pinned ? 'Unpinned' : 'Pinned');
}

async function removeSelected() {
  const item = state.items[state.selected];
  if (!item) return;
  await invoke('delete_item', { id: item.id });
}

function setFilter(f) {
  state.filter = f;
  for (const chip of els.filters.querySelectorAll('.chip')) {
    chip.classList.toggle('active', chip.dataset.filter === f);
  }
  load();
}

const FILTER_ORDER = ['all', 'text', 'image', 'files', 'pinned'];

document.addEventListener('keydown', e => {
  const mod = e.metaKey;
  if (e.key === 'Escape') {
    e.preventDefault();
    if (els.search.value) { els.search.value = ''; load(); } else invoke('hide_panel');
  } else if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
    e.preventDefault(); select(state.selected + 1);
  } else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
    e.preventDefault(); select(state.selected - 1);
  } else if (e.key === 'PageDown') {
    e.preventDefault(); select(state.selected + 8);
  } else if (e.key === 'PageUp') {
    e.preventDefault(); select(state.selected - 8);
  } else if (e.key === 'Enter') {
    e.preventDefault();
    paste(state.selected, e.altKey ? 'copy' : e.shiftKey ? 'plain' : 'normal');
  } else if (mod && /^[1-9]$/.test(e.key)) {
    e.preventDefault(); paste(Number(e.key) - 1, e.shiftKey ? 'plain' : 'normal');
  } else if (mod && e.key.toLowerCase() === 'p') {
    e.preventDefault(); togglePin();
  } else if (mod && e.key === 'Backspace') {
    e.preventDefault(); removeSelected();
  } else if (mod && e.key === ',') {
    e.preventDefault(); invoke('open_settings');
  } else if (e.key === 'Tab') {
    e.preventDefault();
    const i = FILTER_ORDER.indexOf(state.filter);
    setFilter(FILTER_ORDER[(i + (e.shiftKey ? FILTER_ORDER.length - 1 : 1)) % FILTER_ORDER.length]);
  } else if (document.activeElement !== els.search && e.key.length === 1 && !mod && !e.ctrlKey) {
    els.search.focus();
  }
});

let searchTimer;
els.search.addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(load, 60);
});

els.filters.addEventListener('click', e => {
  const chip = e.target.closest('.chip');
  if (chip) setFilter(chip.dataset.filter);
  els.search.focus();
});

els.list.addEventListener('click', e => {
  const row = e.target.closest('.row');
  if (row) select(Number(row.dataset.index));
  els.search.focus();
});
els.list.addEventListener('dblclick', e => {
  const row = e.target.closest('.row');
  if (row) paste(Number(row.dataset.index), 'normal');
});
els.list.addEventListener('scroll', () => {
  if (els.list.scrollTop + els.list.clientHeight > els.list.scrollHeight - 200) loadMore();
});

document.getElementById('settings-btn').addEventListener('click', () => invoke('open_settings'));

async function loadSources() {
  const sources = await invoke('list_sources');
  els.source.length = 1;
  for (const s of sources) els.source.add(new Option(s.name, s.bundle_id));
}
els.source.addEventListener('change', () => { load(); els.search.focus(); });

listen('panel-shown', async () => {
  els.search.value = '';
  els.source.value = '';
  state.filter = 'all';
  setFilter('all');
  els.search.focus();
  await loadSources();
});
listen('history-changed', () => load({ keepSelection: true }));

load();
