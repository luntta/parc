// The quick capture window: pick a vault and type, write, save, and it closes. Text in progress is kept
// locally, so closing the window by accident loses nothing; Esc asks before dropping it.
import '@luntta/aihio';

const $ = id => document.getElementById(id), api = window.parc;
const DRAFT = 'parc.quickDraft', SCOPE = 'parc.scope';
const remember = {
  get(k) { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } },
  set(k, v) { try { if (v == null) localStorage.removeItem(k); else localStorage.setItem(k, JSON.stringify(v)); } catch { /* not persisted */ } }
};
const capital = s => s ? s[0].toUpperCase() + s.slice(1).replace(/[-_]/g, ' ') : '';
const isMac = /Mac/.test(navigator.platform);
$('qSaveKey').textContent = isMac ? '⌘S' : 'Ctrl S';
let busy = false;

const empty = () => !$('qTitle').value.trim() && !$('qBody').value.trim();
function status(message, error = false) {
  $('qStatus').textContent = String(message?.message ?? message ?? '').replace(/^Error invoking remote method '[^']+': Error: /, '');
  $('qStatus').dataset.tone = error ? 'error' : '';
}
function keep() {
  remember.set(DRAFT, empty() ? null : { vault: $('qVault').value, type: $('qType').value, title: $('qTitle').value, body: $('qBody').value });
}
// The vault's types that are on; a capture starts as the vault's default unless a draft chose another.
async function types(vault, chosen) {
  let list = [];
  try { list = (await api.schemas(vault)).filter(s => s.enabled !== false); } catch { /* an unreadable vault still takes notes */ }
  const names = list.map(s => s.name), start = list.find(s => s.default)?.name || names[0] || 'note';
  if (!names.length) names.push('note');
  $('qType').replaceChildren(...names.map(n => new Option(capital(n), n)));
  $('qType').value = names.includes(chosen) ? chosen : start;
}
async function save() {
  if (busy || empty()) { if (empty()) status('Write something first.', true); return; }
  busy = true; $('qSave').setAttribute('loading', '');
  try {
    const note = await api.save($('qVault').value, { title: $('qTitle').value, body: $('qBody').value, type: $('qType').value, fields: {}, tags: [] });
    remember.set(DRAFT, null);
    status(`Saved “${note.title}”.`);
    setTimeout(() => window.close(), 250);
  } catch (err) { status(err, true); }
  finally { busy = false; $('qSave').removeAttribute('loading'); }
}
async function close() {
  if (empty()) { remember.set(DRAFT, null); return window.close(); }
  const choice = await api.discard({ title: $('qTitle').value || $('qBody').value.split('\n')[0], canSave: true });
  if (choice === 'save') return save();
  if (choice === 'discard') { remember.set(DRAFT, null); window.close(); }
}

$('quick').addEventListener('submit', e => { e.preventDefault(); save(); });
$('qVault').addEventListener('change', () => { types($('qVault').value, $('qType').value); keep(); });
for (const id of ['qType', 'qTitle', 'qBody']) $(id).addEventListener('input', keep);
$('qTitle').addEventListener('keydown', e => { if (e.key === 'Enter' && !e.isComposing && !(e.ctrlKey || e.metaKey)) { e.preventDefault(); $('qBody').focus(); } });
document.addEventListener('keydown', e => {
  const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey;
  if (ctrl && (e.key === 'Enter' || e.key.toLowerCase() === 's')) { e.preventDefault(); save(); }
  else if (e.key === 'Escape' && !e.target.closest('select')) { e.preventDefault(); close(); }
});
window.addEventListener('beforeunload', keep);

(async () => {
  try {
    const vaults = await api.vaults(), draft = remember.get(DRAFT), last = remember.get(SCOPE);
    $('qVault').replaceChildren(...vaults.map(v => new Option(v.name, v.path)));
    const known = p => vaults.some(v => v.path === p);
    $('qVault').value = known(draft?.vault) ? draft.vault : known(last) ? last : vaults[0].path;
    await types($('qVault').value, draft?.type);
    if (draft) { $('qTitle').value = draft.title || ''; $('qBody').value = draft.body || ''; status('Restored what you were writing.'); }
  } catch (err) { status(err, true); }
  $('qBody').focus();
})();
