const { invoke } = window.__TAURI__.core;
const $ = id => document.getElementById(id);

async function refresh() {
  const ok = await invoke('accessibility_status');
  $('ax-dot').classList.toggle('ok', ok);
  $('ax-text').textContent = ok ? 'Allowed. HateClip can paste for you.' : 'Not allowed yet';
  $('grant').style.display = ok ? 'none' : '';
  $('skip').style.display = ok ? 'none' : '';
  $('done').style.display = ok ? '' : 'none';
}

$('grant').addEventListener('click', () => invoke('request_accessibility'));
$('skip').addEventListener('click', () => invoke('finish_onboarding'));
$('done').addEventListener('click', () => invoke('finish_onboarding'));

(async () => {
  const s = await invoke('get_settings');
  $('shortcut').textContent = s.shortcut
    .replace(/CmdOrCtrl|CommandOrControl|Command|Cmd|Super/g, 'Cmd')
    .replace(/Alt/g, 'Option')
    .split('+').join(' ');
  refresh();
  setInterval(refresh, 1000);
})();
