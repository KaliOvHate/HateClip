const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = id => document.getElementById(id);
let settings = null;
let clearArmed = false;

function showError(msg) {
  $('error').textContent = msg || '';
}

function shortcutLabel(s) {
  return s
    .replace(/CmdOrCtrl|CommandOrControl|Command|Cmd|Super/g, 'Cmd')
    .replace(/Alt|Option/g, 'Option')
    .replace(/Control|Ctrl/g, 'Ctrl')
    .split('+').join(' + ');
}

function fill(s) {
  settings = s;
  $('shortcut').textContent = shortcutLabel(s.shortcut);
  $('launch').checked = s.launch_at_login;
  $('paused').checked = s.paused;
  $('images').checked = s.record_images;
  setSelect($('max-items'), s.max_items);
  setSelect($('max-age'), s.max_age_days);
  $('excluded').value = s.excluded_apps.join('\n');
}

function setSelect(sel, value) {
  const v = String(value);
  if (![...sel.options].some(o => o.value === v)) {
    sel.add(new Option(v, v));
  }
  sel.value = v;
}

async function save(patch) {
  showError('');
  try {
    fill(await invoke('save_settings', { settings: { ...settings, ...patch } }));
    refreshInfo();
  } catch (e) {
    showError(String(e));
    fill(settings);
  }
}

async function refreshInfo() {
  const info = await invoke('app_info');
  $('version').textContent = `${info.name} ${info.version}`;
  $('item-count').textContent = `${info.item_count.toLocaleString()} items`;
  $('data-dir').textContent = info.data_dir;
}

async function refreshAccessibility() {
  const ok = await invoke('accessibility_status');
  $('ax-dot').classList.toggle('ok', ok);
  $('ax-text').textContent = ok ? 'Allowed' : 'Not allowed';
  $('ax-btn').style.display = ok ? 'none' : '';
}

async function loadSources() {
  const sources = await invoke('list_sources');
  const sel = $('sources');
  sel.length = 1;
  for (const s of sources) sel.add(new Option(`${s.name} (${s.bundle_id})`, s.bundle_id));
}

// Shortcut recorder: builds a Tauri accelerator string from a key press.
const KEY_NAMES = { Space: 'Space', Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']',
  Backslash: '\\', Semicolon: ';', Quote: "'", Comma: ',', Period: '.', Slash: '/' };
function keyFromCode(code) {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|20)$/.test(code)) return code;
  return KEY_NAMES[code] || null;
}

let recording = false;
$('shortcut').addEventListener('click', () => {
  recording = true;
  $('shortcut').classList.add('recording');
  $('shortcut').textContent = 'Press keys...';
});
document.addEventListener('keydown', e => {
  if (!recording) return;
  e.preventDefault();
  if (e.key === 'Escape') {
    recording = false;
    $('shortcut').classList.remove('recording');
    fill(settings);
    return;
  }
  const key = keyFromCode(e.code);
  const mods = [];
  if (e.metaKey) mods.push('CmdOrCtrl');
  if (e.ctrlKey) mods.push('Control');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (!key) return;
  if (!mods.length || (mods.length === 1 && mods[0] === 'Shift')) {
    showError('Use at least one of Cmd, Control or Option in the shortcut.');
    return;
  }
  recording = false;
  $('shortcut').classList.remove('recording');
  save({ shortcut: [...mods, key].join('+') });
});

$('launch').addEventListener('change', e => save({ launch_at_login: e.target.checked }));
$('paused').addEventListener('change', e => save({ paused: e.target.checked }));
$('images').addEventListener('change', e => save({ record_images: e.target.checked }));
$('max-items').addEventListener('change', e => save({ max_items: Number(e.target.value) }));
$('max-age').addEventListener('change', e => save({ max_age_days: Number(e.target.value) }));
$('excluded').addEventListener('change', e => save({
  excluded_apps: e.target.value.split('\n').map(s => s.trim()).filter(Boolean),
}));
$('add-source').addEventListener('click', () => {
  const id = $('sources').value;
  if (!id || settings.excluded_apps.includes(id)) return;
  save({ excluded_apps: [...settings.excluded_apps, id] });
});
$('clear').addEventListener('click', async () => {
  if (!clearArmed) {
    clearArmed = true;
    $('clear').textContent = 'Click again to confirm';
    setTimeout(() => { clearArmed = false; $('clear').textContent = 'Clear History'; }, 4000);
    return;
  }
  clearArmed = false;
  $('clear').textContent = 'Clear History';
  await invoke('clear_history', { includePinned: false });
  refreshInfo();
});
$('ax-btn').addEventListener('click', () => invoke('request_accessibility'));

listen('settings-changed', e => fill(e.payload));
window.addEventListener('focus', () => { refreshAccessibility(); refreshInfo(); loadSources(); });

(async () => {
  fill(await invoke('get_settings'));
  refreshInfo();
  refreshAccessibility();
  loadSources();
  setInterval(refreshAccessibility, 2000);
})();
