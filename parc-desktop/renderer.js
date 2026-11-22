import '@luntta/aihio';
import { markdownEditor, fold } from './editor.js';
import { dictation } from './speech.js';
import { icon, typeIcon, hydrateIcons } from './icons.js';
import { vaultGraph } from './graph.js';
import { palette } from './palette.js';
import { searchAssist } from './assist.js';
import { diffLines, fold as foldDiff, stat } from './diff.js';

const $ = id => document.getElementById(id), api = window.parc;
hydrateIcons();
const isMac = /Mac/.test(navigator.platform), mod = isMac ? '⌘' : 'Ctrl';
$('saveKey').textContent = isMac ? '⌘S' : 'Ctrl S';
$('save').title = `Save fragment (${mod}+S)`;

let vaults = [], suggested = [], scope = '', rows = [], opened = null, mode = 'capture', phase = 'loading', dirty = false;
let fresh = null;                          // a new capture's type, schema fields and tags until its first save
let speech = { ready: true };              // whether dictation is set up; checked at launch and after Settings
const schemas = new Map();                 // vault path → schema.list
const changedVaults = new Set();           // vaults whose fragments changed on disk, waiting for an idle moment
const key = r => JSON.stringify([r.vault, r.id]);
const name = p => vaults.find(v => v.path === p)?.name || 'All vaults';
const labels = { capture: 'New capture', search: 'Search', graph: 'Graph', agent: 'Agent', pulse: 'Pulse', history: 'History' };
const blankCapture = () => ({ type: 'note', fields: {}, tags: [] });

// Per-viewer memory: last vault, an unsaved draft, the sidebar's shape and filter. The app works without it.
const DRAFT = 'parc.draft', SCOPE = 'parc.scope', SIDE = 'parc.sidebar', OPEN_ONLY = 'parc.openOnly';
const remember = {
  get(k) { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } },
  set(k, v) { try { if (v == null) localStorage.removeItem(k); else localStorage.setItem(k, JSON.stringify(v)); } catch { /* not persisted */ } }
};

// ── Small DOM and text helpers ──
function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
const capital = s => s ? s[0].toUpperCase() + s.slice(1).replace(/[-_]/g, ' ') : '';
const typeLabel = t => capital(t || 'fragment');
const words = text => (text.match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) || []).length;
const dayMs = 864e5;
const shortDate = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });
const longDate = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
const stamp = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const rel = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
const midnight = d => { const x = new Date(d); x.setHours(0, 0, 0, 0); return x; };
const cleanError = err => String(err?.message ?? err).replace(/^Error invoking remote method '[^']+': Error: /, '');
function age(iso) { // compact, for lists
  const t = new Date(iso), s = (Date.now() - t) / 1000, days = Math.round((midnight(Date.now()) - midnight(t)) / dayMs);
  if (s < 60) return 'Now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (days === 0) return `${Math.floor(s / 3600)}h`;
  if (days === 1) return 'Yesterday';
  return t.getFullYear() === new Date().getFullYear() ? shortDate.format(t) : longDate.format(t);
}
function edited(iso) { // prose, for the status bar
  const t = new Date(iso), s = (Date.now() - t) / 1000, days = Math.round((midnight(Date.now()) - midnight(t)) / dayMs);
  if (s < 60) return 'Edited just now';
  if (s < 3600) return `Edited ${rel.format(-Math.floor(s / 60), 'minute')}`;
  if (days === 0) return `Edited ${rel.format(-Math.floor(s / 3600), 'hour')}`;
  if (days < 7) return `Edited ${rel.format(-days, 'day')}`;
  return `Edited ${longDate.format(t)}`;
}
const dayLabel = day => (day.slice(0, 4) === String(new Date().getFullYear()) ? shortDate : longDate).format(new Date(`${day}T12:00`));
// Status words differ per type; tone is what they mean for attention.
const TONES = { open: 'open', proposed: 'open', raw: 'open', identified: 'attention', 'in-progress': 'active', mitigating: 'active', exploring: 'active',
  done: 'done', accepted: 'done', resolved: 'done', promoted: 'done', cancelled: 'closed', superseded: 'closed', deprecated: 'closed', parked: 'closed', discarded: 'closed' };
const tone = (type, status) => type === 'risk' && status === 'accepted' ? 'closed' : TONES[status] || 'open';
const settled = r => ['done', 'closed'].includes(tone(r.type, r.status));
function state(type, status) { const s = el('span', 'state', capital(status)); s.dataset.tone = tone(type, status); return s; }
const overdue = f => !!f.due && !settled(f) && f.due < new Date().toLocaleDateString('sv');

// ── Status line ── An optional action (Undo) sits after the message until the next one.
function status(message = '', action) {
  const error = message instanceof Error || (typeof message === 'object' && message && 'message' in message);
  const text = cleanError(message), line = el('span', 'status-text', text);
  line.title = text;
  $('status').replaceChildren(line);
  $('status').dataset.tone = error ? 'error' : '';
  if (action) {
    const b = el('button', 'status-action', action.label); b.type = 'button';
    b.onclick = () => { status(); action.run(); };
    $('status').append(b);
  }
}

// ── Schemas ──
async function loadSchemas(paths) {
  // A vault that cannot answer yet keeps no entry, so it is asked again when next chosen.
  await Promise.all(paths.map(async p => { try { schemas.set(p, await api.schemas(p)); } catch { schemas.delete(p); } }));
}
// One vault's types, or every opened vault's (first definition wins) when no vault is chosen yet.
function typesIn(vault) {
  if (vault && schemas.has(vault)) return schemas.get(vault);
  const byName = new Map();
  for (const list of schemas.values()) for (const s of list) if (!byName.has(s.name)) byName.set(s.name, s);
  return [...byName.values()];
}
const schemaFor = (vault, type) => typesIn(vault).find(s => s.name === type) || null;
// Every value an enum field takes in the types in view, e.g. all statuses.
const fieldValues = field => [...new Set(typesIn(scope).flatMap(s => s.fields.filter(f => f.name === field && f.values).flatMap(f => f.values)))];
const allTags = () => [...new Set(rows.flatMap(r => r.tags || []))].sort();

// ── Editor ──
// A prefix resolves only when it names exactly one fragment; ULIDs made within ~256ms share 8 characters.
// Links resolve inside one vault: the open fragment's, the scope's, or the chosen save destination.
const linkVault = () => opened?.vault ?? (scope || $('destination').value);
const linkable = () => { const v = linkVault(); return rows.filter(r => !v || r.vault === v); };
const shortId = (id, pool = linkable()) => { let n = 8; while (n < id.length && pool.some(r => r.id !== id && r.id.startsWith(id.slice(0, n)))) n++; return id.slice(0, n); };
const links = {
  find(prefix) { const hits = linkable().filter(r => r.id.startsWith(prefix)); return hits.length === 1 ? hits[0] : null; },
  resolve(prefix) { const r = links.find(prefix); return r ? { title: r.title, type: r.type } : null; },
  open(prefix) { const r = links.find(prefix); if (r) action(() => openFragment(r)); },
  pool: () => linkable().filter(r => !(opened && key(opened) === key(r))),
  // Unique where it will be resolved (the link scope, which may span vaults) and in its own vault.
  shortId: r => shortId(r.id, [...linkable(), ...rows.filter(x => x.vault === r.vault)]),
  describe: r => [typeLabel(r.type), ...(linkVault() ? [] : [name(r.vault)]), age(r.updated_at)]
};
const editor = markdownEditor($('draft'), changed, links);
let statsFrame = 0;
function changed() { dirty = true; paint(); rememberDraft(); }
function stats() {
  cancelAnimationFrame(statsFrame);
  statsFrame = requestAnimationFrame(() => {
    if (mode !== 'capture') { $('stats').textContent = ''; return; }
    const n = words(`${$('title').value} ${editor.text()}`);
    $('stats').replaceChildren(el('span', '', `${n.toLocaleString()} ${n === 1 ? 'word' : 'words'}`));
    if (opened && !dirty) {
      // The last edit opens the fragment's history.
      const t = el('button', 'edited', edited(opened.updated_at)); t.type = 'button';
      t.title = `${stamp.format(new Date(opened.updated_at))}. Show history`;
      t.onclick = showHistory;
      $('stats').append(t);
    }
  });
}
const isEmpty = () => !$('title').value.trim() && !editor.text().trim();
const destination = () => opened?.vault || scope || $('destination').value;

// ── Drafts ──
// The editor's unsaved state is written locally as it changes, so quitting or a crash keeps it. One draft:
// leaving a dirty fragment still asks to save or discard. `base` is the revision the text was written
// against, so restoring an old draft over a newer file still ends in the overwrite question.
let draftTimer = 0;
function rememberDraft(now = false) {
  clearTimeout(draftTimer);
  const write = () => remember.set(DRAFT, dirty && !isEmpty() ? {
    vault: destination() || '', id: opened?.id ?? null, base: opened?.revision ?? null, fresh: opened ? null : fresh,
    title: $('title').value, body: editor.text(), at: new Date().toISOString()
  } : null);
  if (now) write(); else draftTimer = setTimeout(write, 300);
}
async function restoreDraft(draft) {
  let full = null;
  if (draft.id) { try { full = await api.note(draft.vault, draft.id); } catch { /* deleted since: keep the words as a new capture */ } }
  const base = /^[0-9a-f]{16}$/.test(draft.base || '') ? draft.base : full?.revision;
  if (full) reset({ ...full, vault: draft.vault, revision: base });
  else { reset(); fresh = { ...blankCapture(), ...(draft.fresh || {}) }; if (!scope && draft.vault) $('destination').value = draft.vault; meta(); }
  $('title').value = draft.title; editor.load(draft.body); dirty = true; paint(); rememberDraft(true);
  if (full) await loadNeighbors();
  status(draft.id && !full ? 'Restored your unsaved draft. Its fragment was deleted, so it will save as a new capture.' : `Restored your unsaved draft from ${stamp.format(new Date(draft.at))}.`);
}

// ── Dictation ──
let recordStart = 0, recordTimer = 0;
function setPhase(value) {
  if (value === 'recording' && phase !== 'recording') {
    recordStart = Date.now(); clearInterval(recordTimer);
    recordTimer = setInterval(() => { const s = Math.floor((Date.now() - recordStart) / 1000); $('recTime').textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; }, 250);
    $('recTime').textContent = '0:00';
  } else if (value !== 'recording') clearInterval(recordTimer);
  phase = value; paint();
  if (value === 'idle' && changedVaults.size) setTimeout(flushChanges);
}
const voice = dictation(api, setPhase, status, text => editor.append(text));
async function checkSpeech() {
  try { speech = await api.speechStatus(); } catch (err) { speech = { ready: false, problem: cleanError(err) }; }
  paint();
}

// ── Paint: every control's state from (phase, mode, dirty, opened, scope) ──
function paint() {
  const blocking = !['idle', 'loading'].includes(phase), recording = phase === 'recording', capture = mode === 'capture';
  document.body.dataset.phase = phase;
  $('main').setAttribute('aria-busy', String(['loading', 'saving', 'transcribing'].includes(phase)));
  $('scope').toggleAttribute('disabled', blocking || !vaults.length);
  $('destination').disabled = blocking;
  $('record').hidden = !capture; $('save').hidden = !capture;
  $('record').toggleAttribute('disabled', phase === 'saving'); // its click handler ignores every other non-idle phase
  $('record').toggleAttribute('loading', phase === 'transcribing' || phase === 'starting');
  const setup = !speech.ready && phase === 'idle';
  $('recordLabel').textContent = recording ? 'Stop & transcribe' : phase === 'transcribing' ? 'Transcribing' : setup ? 'Set up dictation' : 'Dictate';
  $('record').title = setup ? speech.problem || '' : '';
  $('record').setAttribute('variant', setup ? 'ghost' : 'outline');
  const glyph = $('recordIcon');
  if (glyph && glyph.dataset.name !== (recording ? 'stop' : 'mic')) {
    const next = icon(recording ? 'stop' : 'mic'); next.id = 'recordIcon'; next.dataset.name = recording ? 'stop' : 'mic'; glyph.replaceWith(next);
  }
  $('cancel').hidden = !recording; $('recTime').hidden = !recording;
  const unsavable = blocking || !dirty || !destination() || isEmpty();
  $('save').toggleAttribute('disabled', unsavable);
  $('save').setAttribute('variant', unsavable && phase !== 'saving' ? 'outline' : 'default'); // a disabled primary reads as a grey slab
  $('save').toggleAttribute('loading', phase === 'saving');
  $('fragmentMenu').hidden = !(capture && opened);
  $('archiveLabel').textContent = opened?.archived ? 'Unarchive' : 'Archive';
  $('document').hidden = !capture; $('tool').hidden = capture;
  $('destinationLabel').hidden = !capture || !!scope || !!opened;
  $('destination').toggleAttribute('data-empty', !$('destination').value);
  $('resume').hidden = !dirty || capture;
  const rev = $('revision');
  rev.textContent = phase === 'saving' ? 'Saving…' : dirty ? 'Unsaved' : capture && opened ? 'Saved' : '';
  rev.dataset.state = phase === 'saving' ? 'saving' : dirty ? 'dirty' : 'saved';
  rev.hidden = !rev.textContent || ['recording', 'transcribing', 'starting', 'stopping'].includes(phase); // the recording state speaks for the draft
  crumb(capture);
  for (const b of document.querySelectorAll('[data-view]')) {
    b.toggleAttribute('disabled', blocking);
    const current = b.dataset.view === mode && !(opened && capture);
    if (current) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
  }
  // The window's title names what is open, with a dot while it has unsaved changes.
  const here = capture ? $('title').value.trim() || (opened ? 'Untitled' : `New ${fresh?.type || 'note'}`) : labels[mode];
  document.title = `${dirty ? '• ' : ''}${here} — parc`;
  stats();
}
function crumb(capture) {
  const where = capture && opened ? opened.vault : scope;
  const here = capture ? opened ? typeLabel(opened.type) : `New ${fresh?.type || 'note'}` : labels[mode];
  $('crumb').replaceChildren(el('span', 'vault', name(where)), el('span', 'sep', '/'), el('span', 'here', here));
}

// ── Vault switcher ──
// Two vaults can share an initial; a hue from the path tells their marks apart.
const hue = path => { let h = 7; for (const ch of path) h = (h * 31 + ch.codePointAt(0)) % 360; return h; };
function vaultMark(path) {
  if (!path) { const all = el('span', 'all-mark'); all.append(icon('layers')); return all; }
  const avatar = document.createElement('aihio-avatar');
  avatar.setAttribute('size', 'sm'); avatar.setAttribute('alt', ''); avatar.setAttribute('fallback', [...name(path)][0]?.toUpperCase() || 'V');
  avatar.style.setProperty('--vault-hue', hue(path));
  return avatar;
}
function options() {
  const menu = $('vaultMenu');
  menu.querySelectorAll('aihio-dropdown-item, aihio-dropdown-separator').forEach(n => n.remove());
  const item = (value, label, mark, current, hint) => {
    const it = document.createElement('aihio-dropdown-item');
    it.setAttribute('value', value); it.title = value.startsWith('@suggest:') ? value.slice(9) : value.startsWith('@') ? '' : value;
    it.append(mark, el('span', 'item-name', label));
    if (hint) it.append(el('span', 'item-hint', hint));
    if (current) { it.append(icon('check', 'icon current'), el('span', 'sr', '(current)')); }
    return it;
  };
  const sep = () => document.createElement('aihio-dropdown-separator');
  menu.append(
    ...vaults.map(v => item(v.path, v.name, vaultMark(v.path), v.path === scope)),
    item('@all', 'All opened vaults', vaultMark(''), scope === ''),
    sep(), ...suggested.map(s => item(`@suggest:${s.path}`, `Open ${s.name} vault`, icon('folder'), false, s.hint)),
    item('@open', 'Open vault folder…', icon('folder'), false));
  $('vaultName').textContent = scope ? name(scope) : 'All vaults';
  $('vaultMark').replaceChildren(vaultMark(scope));
  $('scope').title = scope || 'All opened vaults';
  const chosen = $('destination').value;
  $('destination').replaceChildren(new Option('Choose vault…', ''), ...vaults.map(v => new Option(v.name, v.path)));
  $('destination').value = chosen;
}
function switchScope(target) {
  return action(async () => {
    if (!await canLeave()) return;
    let next = target === '@all' ? '' : target;
    if (target === '@open' || target.startsWith('@suggest:')) {
      const picked = await api.openVault(target === '@open' ? undefined : target.slice(9));
      if (!picked) return;
      vaults = picked.vaults; next = picked.path;
      await loadSchemas([next]); suggested = await api.suggestedVaults().catch(() => []);
    }
    const old = scope; scope = next;
    try { rows = await collect(); } catch (e) { scope = old; throw e; }
    await loadSchemas((scope ? [scope] : vaults.map(v => v.path)).filter(p => !schemas.has(p)));
    remember.set(SCOPE, scope);
    options(); $('query').value = ''; await refilter(); reset();
  });
}

// ── Fragment rows ──
async function collect(query = '') {
  const paths = scope ? [scope] : vaults.map(v => v.path);
  const groups = await Promise.all(paths.map(async p => (await api.list(p, query)).map(r => ({ ...r, vault: p }))));
  return groups.flat().sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.title.localeCompare(b.title));
}
// FTS snippets are raw Markdown with »matches« marked; show them as prose.
const prose = text => text.split('\n').map(l => l.replace(/^\s*(?:#{1,6}\s+|>\s?|[-*+]\s+(?:\[[ xX]\]\s+)?|\d+[.)]\s+)/, ''))
  .join(' ').replace(/\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/\*\*|__|~~|`/g, '').replace(/\s+/g, ' ').trim();
function snippet(text) {
  const p = el('span', 'row-snippet');
  prose(text).split(/(»[^«]*«)/).forEach(part => {
    if (part.startsWith('»') && part.endsWith('«')) p.append(el('mark', '', part.slice(1, -1))); else if (part) p.append(part);
  });
  return p;
}
// Opening from the keyboard puts the caret in the text; from the pointer it leaves focus where it was.
function openRow(r, focus = false) {
  return action(() => openFragment(r)).then(() => { if (focus && opened && key(opened) === key(r) && mode === 'capture') editor.focus(); });
}
// A fragment row. In the sidebar (compact) the glyph says the type, so the row keeps its room for the title.
function row(r, { withSnippet = false, compact = false } = {}) {
  const b = el('button', 'row'); b.type = 'button';
  const meta = el('span', 'row-meta'), facts = el('span', 'row-facts'), main = el('span', 'row-main');
  const time = el('time', '', age(r.updated_at)); time.dateTime = r.updated_at; time.title = stamp.format(new Date(r.updated_at));
  if (!scope) facts.append(el('span', 'where', name(r.vault)));
  if (!compact) facts.append(el('span', '', typeLabel(r.type)));
  if (r.status) facts.append(state(r.type, r.status));
  if (r.archived) facts.append(el('span', 'archived', 'Archived'));
  if (!compact && r.tags?.length) facts.append(el('span', 'tags', r.tags.map(t => `#${t}`).join(' ')));
  meta.append(facts, time);
  main.append(el('span', 'row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'), meta);
  b.append(typeIcon(r.type), main);
  if (withSnippet && r.snippet) b.append(snippet(r.snippet));
  if (r.archived) b.dataset.archived = '';
  if (opened && key(opened) === key(r) && mode === 'capture') b.setAttribute('aria-current', 'page');
  b.onclick = e => openRow(r, e.detail === 0);
  return b;
}
function rowList(items, options) {
  const ul = el('ul', 'results');
  ul.append(...items.map(r => { const li = el('li'); li.append(row(r, options)); return li; }));
  return ul;
}

// ── Sidebar filter ──
// Plain words filter the list as you type, by title, type, status, tags and vault. Anything in the search
// language (type:todo, #tag, due:overdue) asks the vault instead. "Open" hides settled and archived work.
let filterText = '', filtered = null, filterError = '', openOnly = remember.get(OPEN_ONLY) === true, filterRun = 0;
const isDsl = q => /(^|\s)(!?[a-z]+:\S*|#\S+)/i.test(q);
function visible() {
  let list = filtered ?? rows;
  const q = filterText.trim();
  if (q && !isDsl(q)) {
    const terms = fold(q).split(/\s+/);
    list = list.filter(r => {
      const hay = fold([r.title, r.type, r.status, ...(r.tags || []).map(t => `#${t}`), scope ? '' : name(r.vault)].join(' '));
      return terms.every(t => hay.includes(t));
    });
  }
  return openOnly ? list.filter(r => !settled(r) && !r.archived) : list;
}
async function refilter() {
  const q = filterText.trim(), run = ++filterRun;
  if (!isDsl(q)) { filtered = null; filterError = ''; return; }
  try { const found = await collect(q); if (run === filterRun) { filtered = found; filterError = ''; } }
  catch (err) { if (run === filterRun) { filtered = []; filterError = cleanError(err).replace(/^parse error:\s*/i, ''); } }
}
let filterTimer = 0;
function onFilter() {
  filterText = $('filter').value;
  clearTimeout(filterTimer);
  if (!isDsl(filterText)) { filtered = null; filterError = ''; list(); return; }
  filterTimer = setTimeout(async () => { await refilter(); list(); }, 150);
}
function toggleOpenOnly() { openOnly = !openOnly; remember.set(OPEN_ONLY, openOnly); list(); }
function list() {
  const shown = visible(), narrowed = !!filterText.trim() || openOnly;
  $('recent').replaceChildren(...shown.map(r => { const li = el('li'); li.append(row(r, { compact: true })); return li; }));
  $('count').textContent = rows.length ? narrowed ? `${shown.length} of ${rows.length}` : rows.length : '';
  $('empty').textContent = !rows.length
    ? suggested.length && scope === vaults[0]?.path
      ? `No fragments yet. Press + to capture, or open your ${suggested[0].name} vault (${suggested[0].hint}) from the vault menu.`
      : 'No fragments yet. Press + to capture your first.'
    : filterError ? `This filter can’t be read: ${filterError}.`
      : openOnly && !filterText.trim() ? 'Nothing open: everything here is done, closed or archived.' : 'No fragments match.';
  $('empty').dataset.tone = filterError ? 'error' : '';
  $('empty').hidden = !!shown.length;
  $('openOnly').setAttribute('aria-pressed', String(openOnly));
  $('tagOptions').replaceChildren(...allTags().map(t => new Option(t)));
  editor.refresh();
  if (opened) { const fid = $('meta').querySelector('.fid'), short = shortId(opened.id); if (fid && fid.textContent !== short) { fid.textContent = short; fid.title = `Link to this fragment with [[${short}]]`; } }
}

// ── Sidebar shape: drag its edge to resize, Ctrl+\ to hide ──
const side = { width: null, collapsed: false, ...(remember.get(SIDE) || {}) };
function applySide() {
  document.body.toggleAttribute('data-collapsed', side.collapsed);
  if (side.width) document.documentElement.style.setProperty('--side-width', `${side.width}px`);
  else document.documentElement.style.removeProperty('--side-width');
  const label = side.collapsed ? 'Show sidebar' : 'Hide sidebar';
  $('sidebarToggle').setAttribute('aria-label', label); $('sidebarToggle').title = `${label} (${mod}+\\)`;
  $('sidebarToggle').setAttribute('aria-expanded', String(!side.collapsed));
  $('resizer').setAttribute('aria-valuenow', String(Math.round(side.width || $('sidebar').getBoundingClientRect().width)));
}
function toggleSidebar() {
  side.collapsed = !side.collapsed;
  if (side.collapsed && $('sidebar').contains(document.activeElement)) $('sidebarToggle').focus();
  applySide(); remember.set(SIDE, side);
}
function resizeTo(width) {
  side.width = Math.round(Math.max(200, Math.min(520, window.innerWidth * 0.5, width)));
  applySide();
}
{
  const handle = $('resizer');
  handle.setAttribute('aria-valuemin', '200'); handle.setAttribute('aria-valuemax', '520');
  handle.addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    handle.setPointerCapture(e.pointerId); document.body.dataset.resizing = '';
    const move = ev => resizeTo(ev.clientX);
    const end = () => { handle.removeEventListener('pointermove', move); delete document.body.dataset.resizing; remember.set(SIDE, side); };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end, { once: true }); handle.addEventListener('pointercancel', end, { once: true });
  });
  handle.addEventListener('keydown', e => {
    const step = { ArrowLeft: -16, ArrowRight: 16 }[e.key];
    if (step) { e.preventDefault(); resizeTo($('sidebar').getBoundingClientRect().width + step); remember.set(SIDE, side); }
  });
  handle.addEventListener('dblclick', () => { side.width = null; applySide(); remember.set(SIDE, side); });
}

// ── Document metadata ──
// The type's enum and date fields are controls. On a saved fragment each change is its own save, like
// the TUI's: the text keeps its separate Save. On a new capture they wait for the first save.
function describe() {
  if (opened) return { isNew: false, vault: opened.vault, type: opened.type, schema: schemaFor(opened.vault, opened.type), values: opened, tags: opened.tags || [] };
  const vault = scope || $('destination').value, schema = schemaFor(vault, fresh.type), defaults = {};
  for (const f of schema?.fields || []) if (f.default != null) defaults[f.name] = f.default;
  return { isNew: true, vault, type: fresh.type, schema, values: { ...defaults, ...fresh.fields }, tags: fresh.tags };
}
const editable = schema => (schema?.fields || []).filter(f => (f.kind === 'enum' && f.values?.length) || f.kind === 'date');
function menu(trigger, items, onPick, className) {
  const dd = document.createElement('aihio-dropdown'), btn = document.createElement('aihio-button');
  dd.className = className;
  btn.slot = 'trigger'; btn.setAttribute('variant', 'ghost'); btn.setAttribute('size', 'sm');
  btn.append(...trigger, icon('chevron', 'icon caret'));
  dd.append(btn);
  for (const entry of items) {
    if (!entry) { dd.append(document.createElement('aihio-dropdown-separator')); continue; }
    const [value, label, current, glyph] = entry, it = document.createElement('aihio-dropdown-item');
    it.setAttribute('value', value);
    if (glyph) it.append(glyph);
    it.append(el('span', 'item-name', label));
    if (current) it.append(icon('check', 'icon current'), el('span', 'sr', '(current)'));
    dd.append(it);
  }
  dd.addEventListener('aihio-select', e => { e.stopPropagation(); onPick(e.detail.value); });
  return dd;
}
function typeMenu(c) {
  const types = [...typesIn(c.vault)];
  if (!types.some(s => s.name === c.type)) types.unshift({ name: c.type });
  return menu([el('span', 'sr', 'Type: '), typeIcon(c.type), el('span', '', `New ${c.type}`)],
    types.map(s => [s.name, typeLabel(s.name), s.name === c.type, typeIcon(s.name)]),
    type => setType(type), 'prop type-menu');
}
function setType(type) {
  if (opened || type === fresh.type) return;
  fresh = { type, fields: {}, tags: fresh.tags }; meta(); paint(); rememberDraft();
}
function enumControl(field, c) {
  const value = c.values[field.name] ?? null, toggle = c.schema?.toggle?.field === field.name ? c.schema.toggle : null;
  const shown = field.name === 'status' ? capital(value) : `${capital(value)} ${field.name.replace(/[-_]/g, ' ')}`;
  const trigger = [el('span', 'sr', `${capital(field.name)}: `)];
  if (field.name === 'status' && !toggle) { const dot = el('span', 'state'); dot.dataset.tone = tone(c.type, value); trigger.push(dot); }
  if (field.name === 'priority') trigger.push(icon('flag'));
  trigger.push(el('span', value ? '' : 'unset', value ? shown : capital(field.name)), ...(value ? [] : [el('span', 'sr', ' not set')]));
  const items = field.values.map(v => [v, capital(v), v === value]);
  if (!field.required && value) items.push(null, ['', 'Clear']);
  const dd = menu(trigger, items, v => setField(field.name, v === '' ? null : v), 'prop');
  dd.dataset.field = field.name;
  if (field.name === 'priority' && ['high', 'critical'].includes(value)) dd.dataset.emphasis = '';
  if (!toggle) return dd;
  // A one-click toggle (the schema's `toggle:`, the TUI's t) on the status mark.
  const next = value === toggle.on ? toggle.off : toggle.on, mark = el('button', 'toggle-mark');
  mark.type = 'button'; mark.dataset.tone = tone(c.type, value);
  mark.title = `Mark ${capital(next).toLowerCase()} (T)`; mark.setAttribute('aria-label', `Mark ${next}`);
  mark.onclick = () => setField(toggle.field, next);
  const group = el('span', 'prop-group'); group.append(mark, dd);
  return group;
}
function dateControl(field, c) {
  const value = c.values[field.name] ?? null, wrap = el('span', 'prop-date'), btn = el('button', 'prop-button');
  const late = field.name === 'due' && overdue({ type: c.type, status: c.values.status, due: value });
  btn.type = 'button'; btn.dataset.field = field.name;
  btn.append(icon('calendar'), el('span', value ? '' : 'unset', value ? `${capital(field.name)} ${dayLabel(value)}${late ? ', overdue' : ''}` : `${capital(field.name)} date`));
  if (late) btn.dataset.overdue = '';
  btn.onclick = () => {
    const picker = document.createElement('aihio-date-picker');
    picker.setAttribute('size', 'sm'); picker.setAttribute('aria-label', `${capital(field.name)} date`);
    if (value) picker.setAttribute('value', value);
    let settledPick = false;
    const done = next => { if (settledPick) return; settledPick = true; if (next !== undefined && next !== value) setField(field.name, next); else meta(); };
    picker.addEventListener('aihio-change', e => done(e.detail.value || null));
    wrap.addEventListener('keydown', e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); done(); } });
    wrap.addEventListener('focusout', e => { if (!wrap.contains(e.relatedTarget)) setTimeout(() => { if (!wrap.contains(document.activeElement)) done(); }); });
    wrap.replaceChildren(picker);
    requestAnimationFrame(() => { picker.focus(); picker.open?.(); });
  };
  wrap.append(btn);
  if (value && !field.required) {
    const clear = el('button', 'prop-clear'); clear.type = 'button';
    clear.title = `Remove ${field.name.replace(/[-_]/g, ' ')} date`; clear.setAttribute('aria-label', clear.title);
    clear.append(icon('x')); clear.onclick = () => setField(field.name, null);
    wrap.append(clear);
  }
  return wrap;
}
function tagEditor(c) {
  const box = el('span', 'tag-edit');
  for (const t of c.tags) {
    const chip = el('span', 'tag'), remove = el('button', 'tag-x');
    remove.type = 'button'; remove.title = `Remove #${t}`; remove.setAttribute('aria-label', remove.title);
    remove.append(icon('x')); remove.onclick = () => setTags(c.tags.filter(x => x !== t));
    chip.append(`#${t}`, remove);
    box.append(chip);
  }
  const add = el('button', 'prop-button add-tag'); add.type = 'button'; add.title = 'Add a tag';
  add.append(icon('tag'), el('span', c.tags.length ? 'sr' : 'unset', 'Add tag'));
  add.onclick = () => {
    const input = el('input', 'tag-input'); let finished = false;
    Object.assign(input, { type: 'text', placeholder: 'tag', autocomplete: 'off', spellcheck: false });
    input.setAttribute('list', 'tagOptions'); input.setAttribute('aria-label', 'New tag');
    const finish = keep => {
      if (finished) return; finished = true;
      const tag = input.value.trim().replace(/^#+/, '').replace(/\s+/g, '-');
      if (keep && tag && !c.tags.some(t => t.toLowerCase() === tag.toLowerCase())) setTags([...c.tags, tag]); else meta();
    };
    input.onkeydown = e => {
      if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); finish(true); }
      else if (e.key === 'Escape') { e.preventDefault(); finish(false); }
    };
    input.onblur = () => finish(true);
    add.replaceWith(input); input.focus();
  };
  box.append(add);
  return box;
}
function meta() {
  const box = $('meta'), c = describe();
  box.replaceChildren();
  if (c.isNew) box.append(typeMenu(c));
  else {
    const b = document.createElement('aihio-badge'); b.setAttribute('variant', 'outline'); b.append(typeIcon(c.type), typeLabel(c.type));
    box.append(b);
    if (opened.archived) { const a = document.createElement('aihio-badge'); a.setAttribute('variant', 'secondary'); a.append(icon('archive'), 'Archived'); box.append(a); }
  }
  for (const f of editable(c.schema)) box.append(f.kind === 'date' ? dateControl(f, c) : enumControl(f, c));
  if (!c.schema && c.values.status) box.append(state(c.type, c.values.status)); // a type without a schema here: shown, not edited
  box.append(tagEditor(c));
  if (!c.isNew) {
    const short = shortId(opened.id), id = el('span', 'fid', short); id.title = `Link to this fragment with [[${short}]]`;
    box.append(id);
  }
}
function setField(field, value) {
  if (!opened) { fresh.fields = { ...fresh.fields, [field]: value }; meta(); rememberDraft(); return; }
  const shown = /^\d{4}-\d{2}-\d{2}$/.test(value) ? dayLabel(value) : capital(value).toLowerCase(), f = opened;
  return applyChange(rev => api.update(f.vault, f.id, { fields: { [field]: value } }, rev),
    value == null ? `${capital(field)} cleared.` : `${capital(field)} set to ${shown}.`);
}
function setTags(tags) {
  if (!opened) { fresh.tags = tags; meta(); rememberDraft(); return; }
  const f = opened;
  return applyChange(rev => api.update(f.vault, f.id, { tags }, rev), 'Tags saved.');
}
function toggleField() {
  const c = describe(), t = c.schema?.toggle;
  if (!t) return;
  setField(t.field, c.values[t.field] === t.on ? t.off : t.on);
}
function toggleArchive() {
  const f = opened;
  if (!f) return;
  const archived = !f.archived;
  return applyChange(rev => api.archive(f.vault, f.id, archived, rev),
    archived ? 'Archived: it leaves default searches; find it with is:archived.' : 'Unarchived.',
    { label: 'Undo', run: () => { if (opened && key(opened) === key(f)) toggleArchive(); } });
}
// A field-level save names the revision it was based on. If the file moved on and the text is clean, reload
// and apply the change to the newer file. If the text has edits, apply the change (it touches nothing else)
// but keep the text's base, so saving the text still asks before overwriting.
function applyChange(send, done, undo) {
  return action(async () => {
    const f = opened;
    let note = await send(f.revision), base = null;
    if (note.conflict && !dirty) { await reload(f); note = await send(opened.revision); }
    else if (note.conflict) { base = f.revision; note = await send(null); }
    if (note.conflict) throw new Error('This fragment keeps changing on disk. Try again in a moment.');
    opened = { ...note, vault: f.vault, ...(base ? { revision: base } : {}) };
    meta(); paint();
    status(note.warnings?.join('; ') || (base ? `${done} It also changed outside parc: saving your text will ask before overwriting.` : done), undo);
    rows = await collect(); await refilter(); list();
  });
}

// ── Document ──
function load(note) { // puts a fragment, or a blank capture, in the editor without changing the view
  if (!note || note.id !== opened?.id) $('docGraph').hidden = true;
  $('document').toggleAttribute('data-saved', !!note);
  opened = note; dirty = false; fresh = note ? null : blankCapture(); $('title').value = note?.title || '';
  editor.load((note?.body || '').replace(/^\n/, '')); $('destination').value = ''; meta(); rememberDraft(true);
}
function reset(note = null) { load(note); mode = 'capture'; status(); paint(); list(); }
async function reload(f = opened) { const full = await api.note(f.vault, f.id); load({ ...full, vault: f.vault }); paint(); list(); }
// Asks before dropping unsaved work. Resolves true when it is fine to move on.
async function canLeave() {
  if (!dirty) return true;
  const choice = await api.discard({ title: $('title').value, canSave: !!destination() && !isEmpty() });
  if (choice === 'save') return commit();
  if (choice === 'discard') { dirty = false; rememberDraft(true); return true; }
  return false;
}
// Serialize navigation, RPC reads and saves. No late response may overwrite a newer draft.
async function action(fn) {
  if (phase !== 'idle') return;
  setPhase('loading'); $('title').readOnly = true; editor.lock(true);
  try { await fn(); } catch (err) { status(err); }
  finally { $('title').readOnly = false; editor.lock(false); setPhase('idle'); }
}
async function openFragment(r) {
  if (opened && key(opened) === key(r)) { mode = 'capture'; status(); paint(); list(); return; }
  if (!await canLeave()) return;
  const full = await api.note(r.vault, r.id); reset({ ...full, vault: r.vault });
  $('scroller').scrollTop = 0;
  await loadNeighbors();
}
// Saves the editor; resolves true when it did. A save based on an older revision than the file asks first.
async function commit(force = false) {
  const before = phase; setPhase('saving');
  try {
    const p = destination(), f = opened, title = $('title').value, body = editor.text();
    const note = await api.save(p, f
      ? { id: f.id, title, body, expected: force ? null : f.revision }
      : { title, body, type: fresh.type, fields: fresh.fields, tags: fresh.tags });
    if (note.conflict) {
      const choice = await api.conflict({ title: title || f.title });
      if (choice === 'overwrite') return await commit(true);
      if (choice === 'reload') { await reload(f); await loadNeighbors(); status('Reloaded the version saved outside parc.'); }
      else status('Not saved: this fragment changed outside parc. Save again to choose.');
      return false;
    }
    load({ ...note, vault: p }); $('title').value = note.title;
    status(note.warnings?.join('; ') || `Saved to ${name(p)}.`); rows = await collect(); await refilter(); list(); await loadNeighbors();
    return true;
  } finally { setPhase(before); }
}
async function save() {
  if ($('save').hasAttribute('disabled') || mode !== 'capture') return;
  await action(() => commit());
}
function newCapture(type) {
  return action(async () => {
    if (!await canLeave()) return;
    reset();
    if (type) setType(type);
  }).then(() => { if (mode === 'capture' && !opened) $('title').focus(); });
}
function copyLink() {
  if (!opened) return;
  const link = `[[${shortId(opened.id, rows.filter(r => r.vault === opened.vault))}]]`;
  api.copyText(link).then(() => status(`Copied ${link}. Paste it into another fragment to link the two.`), status);
}
// Moving to the trash asks first, then offers Undo until the next message.
function trashOpened() {
  const f = opened;
  if (!f) return;
  return action(async () => {
    if (!await canLeave()) return;
    const { deleted } = await api.remove(f.vault, f.id, f.title);
    if (!deleted) return;
    rows = await collect(); await refilter(); reset();
    status(`Moved “${f.title || 'Untitled'}” to the trash.`, { label: 'Undo', run: () => action(async () => {
      if (!await canLeave()) return;
      const back = await api.untrash(f.vault, f.id);
      rows = await collect(); await refilter(); reset({ ...back, vault: f.vault }); await loadNeighbors();
      status(`Restored “${back.title || 'Untitled'}” from the trash.`);
    }) });
  });
}

// ── Changes made outside this window ──
// The main process watches each vault's fragments/. Refreshing waits for an idle moment and never takes the
// editor away: a clean open fragment reloads, an edited one only warns (its save will ask).
let refreshing = false;
async function flushChanges() {
  if (refreshing || phase !== 'idle' || !changedVaults.size) return;
  refreshing = true;
  try { for (const p of [...changedVaults]) { changedVaults.delete(p); await refreshVault(p); } }
  catch (err) { status(err); }
  finally { refreshing = false; if (changedVaults.size) setTimeout(flushChanges); }
}
async function refreshVault(p) {
  const view = scope;
  if (!view || view === p) { const next = await collect(); if (scope === view) { rows = next; await refilter(); list(); } }
  const f = opened;
  if (!f || f.vault !== p) return;
  let latest;
  try { latest = await api.note(f.vault, f.id); } catch {
    if (opened === f) status('This fragment was moved to the trash outside parc. Copy anything you need; saving it will fail.');
    return;
  }
  if (opened !== f || latest.revision === f.revision) return;
  if (dirty || phase !== 'idle') { status('This fragment changed outside parc while you were editing. Saving will ask before overwriting.'); return; }
  load({ ...latest, vault: f.vault }); paint(); list();
  await loadNeighbors();
  status('Reloaded: this fragment changed outside parc.');
}
api.onVaultChanged(p => { changedVaults.add(p); flushChanges(); });

// ── Graphs ──
const TYPE_ORDER = ['decision', 'risk', 'todo', 'idea', 'note'];
const typeRank = t => { const i = TYPE_ORDER.indexOf(t); return i < 0 ? TYPE_ORDER.length : i; };
// One fragment's link neighbourhood, drawn around it.
function graph(seed, items) {
  // Neighbours grouped by type, so a side of the fan or an arc of the ring reads as one kind of thing.
  const neighbors = [...items].sort((a, b) => typeRank(a.type) - typeRank(b.type) || (a.title || '').localeCompare(b.title || ''));
  const n = neighbors.length, fan = n > 6, rows = Math.ceil(n / 2), box = el('div', 'graph');
  box.dataset.size = n <= 2 ? 'small' : fan ? 'fan' : 'medium';
  if (fan) box.style.setProperty('--rows', rows);
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('class', 'edges'); svg.setAttribute('viewBox', '0 0 100 100'); svg.setAttribute('preserveAspectRatio', 'none'); svg.setAttribute('aria-hidden', 'true');
  const node = (r, x, y, center = false, anchor = '') => {
    const b = el('button', center ? 'node center' : `node ${anchor}`.trim()); b.type = 'button';
    b.style.setProperty('--x', x); b.style.setProperty('--y', y);
    b.append(typeIcon(r.type), el('span', '', r.title || 'Untitled')); b.title = r.title || 'Untitled';
    b.onclick = e => openRow(r, e.detail === 0);
    return b;
  };
  // One neighbour sits beside the centre, which shifts left to balance it. Up to six go round an
  // ellipse from the top. More fan out in two columns, which cannot collide however many there are.
  const cx = n === 1 ? 34 : 50, start = n <= 2 ? 0 : -90, step = 300 / (rows * 3 + 3); // 3rem rows, 1.5rem padding (see .graph[data-size="fan"])
  const placed = neighbors.map((r, i) => {
    let x, y, anchor = '';
    if (n === 1) { x = 70; y = 50; }
    else if (fan) { // each column hangs from its inner edge, where its edges arrive
      const left = i < rows, j = left ? i : i - rows, count = left ? rows : n - rows;
      x = left ? 31 : 69; anchor = left ? 'end' : 'start'; y = 50 + (j - (count - 1) / 2) * step;
    }
    else { const a = (start + (360 / n) * i) * Math.PI / 180; x = 50 + Math.cos(a) * 34; y = 50 + Math.sin(a) * 36; }
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
    line.setAttribute('x1', cx); line.setAttribute('y1', 50); line.setAttribute('x2', x.toFixed(2)); line.setAttribute('y2', y.toFixed(2));
    svg.append(line);
    return node(r, x.toFixed(2), y.toFixed(2), false, anchor);
  });
  box.append(svg, node(seed, cx, 50, true), ...placed);
  return box;
}
// The open fragment's Graph section: what links to it and what it links to, as saved.
async function loadNeighbors() {
  const f = opened, section = $('docGraph');
  if (!f) { section.hidden = true; return; }
  const items = (await api.list(f.vault, `is:all linked:${f.id}`)).map(r => ({ ...r, vault: f.vault }));
  if (opened !== f) return;
  section.hidden = false;
  $('docGraphNote').textContent = items.length ? `${items.length} linked, as saved` : '';
  if (items.length) { $('docGraphBody').replaceChildren(graph(f, items)); return; }
  const prefix = shortId(f.id, rows.filter(r => r.vault === f.vault)), hint = el('p', 'graph-none');
  hint.append('No links yet. Type ', el('code', '', '[['), ' to link another fragment from here, or mention ', el('code', '', `[[${prefix}]]`), ' in one.');
  $('docGraphBody').replaceChildren(hint);
}

// ── History ──
// Versions newest first, each compared with the current one: what restoring it would bring back.
const SAME = new Set(['id', 'body', 'revision', 'created_at', 'updated_at', 'attachments', 'links', 'vault', 'warnings']);
const show = v => v == null || v === '' ? '—' : Array.isArray(v) ? v.join(', ') || '—' : String(v);
function differences(version, current) {
  const fields = [...new Set([...Object.keys(version), ...Object.keys(current)])].filter(k => !SAME.has(k))
    .filter(k => JSON.stringify(version[k] ?? null) !== JSON.stringify(current[k] ?? null))
    .sort((a, b) => (a === 'title' ? -1 : b === 'title' ? 1 : a.localeCompare(b)));
  const lines = diffLines((current.body || '').replace(/^\n/, ''), (version.body || '').replace(/^\n/, ''));
  return { fields, lines, ...stat(lines) };
}
function showHistory() { if (opened) action(() => navigate('history')); }
async function historyView() {
  const f = opened, out = $('results'), intro = $('intro');
  if (!f) { intro.textContent = ''; out.append(emptyState('Open a fragment first', 'History belongs to a saved fragment.')); return; }
  intro.textContent = `Earlier versions of “${f.title || 'Untitled'}”, newest first. Every save keeps the version it replaced.`;
  const [listing, current] = await Promise.all([api.history(f.vault, f.id), api.note(f.vault, f.id)]);
  const versions = listing.sort((a, b) => b.timestamp.localeCompare(a.timestamp)).slice(0, 60);
  if (!versions.length) { out.append(emptyState('No earlier versions yet', 'Each save keeps the version it replaces, so they appear here after the next change.')); return; }
  const loaded = (await Promise.all(versions.map(v => api.version(f.vault, f.id, v.timestamp).then(frag => ({ ...v, frag }), () => null)))).filter(Boolean);
  const wrap = el('div', 'history'), choices = el('ul', 'versions'), view = el('div', 'version-view');
  choices.setAttribute('role', 'listbox'); choices.setAttribute('aria-label', 'Versions');
  const buttons = loaded.map((v, i) => {
    const d = differences(v.frag, current), li = el('li'), b = el('button', 'version'); b.type = 'button';
    b.setAttribute('role', 'option');
    const when = new Date(v.frag.updated_at);
    const summary = [d.added || d.removed ? `+${d.added} −${d.removed}` : '', ...d.fields.map(capital)].filter(Boolean).join(', ') || 'Same as now';
    b.append(el('span', 'version-when', stamp.format(when)), el('span', 'version-what', summary));
    b.onclick = () => pick(i);
    li.append(b); choices.append(li);
    return b;
  });
  choices.addEventListener('keydown', e => {
    const i = buttons.indexOf(document.activeElement), step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
    if (i >= 0 && step) { e.preventDefault(); const j = Math.max(0, Math.min(buttons.length - 1, i + step)); pick(j); buttons[j].focus(); }
  });
  function pick(i) {
    buttons.forEach((b, j) => b.setAttribute('aria-selected', String(i === j)));
    view.replaceChildren(compare(loaded[i], current, f));
  }
  wrap.append(choices, view); out.append(wrap);
  pick(0);
}
function compare(v, current, f) {
  const box = el('div', 'compare'), d = differences(v.frag, current), when = stamp.format(new Date(v.frag.updated_at));
  const head = el('div', 'compare-head'), restore = document.createElement('aihio-button');
  restore.setAttribute('variant', 'outline'); restore.setAttribute('size', 'sm'); restore.append(icon('history'), 'Restore this version');
  restore.onclick = () => action(async () => {
    if (!await canLeave()) return;
    await api.restoreVersion(f.vault, f.id, v.timestamp);
    const full = await api.note(f.vault, f.id);
    rows = await collect(); await refilter(); reset({ ...full, vault: f.vault }); await loadNeighbors();
    status(`Restored the version saved ${when}. The one it replaced is in History.`);
  });
  head.append(el('h2', '', `Saved ${when}`), restore);
  box.append(head);
  if (!d.fields.length && !d.added && !d.removed) { box.append(el('p', 'compare-same', 'This version matches the current one.')); return box; }
  if (d.fields.length) {
    const table = el('table', 'compare-fields'), thead = el('thead'), tr = el('tr');
    tr.append(el('th', '', 'Field'), el('th', '', 'Now'), el('th', '', 'This version'));
    for (const th of tr.children) th.scope = 'col';
    thead.append(tr); table.append(thead);
    const tbody = el('tbody');
    for (const k of d.fields) { const r = el('tr'), th = el('th', '', capital(k)); th.scope = 'row'; r.append(th, el('td', '', show(current[k])), el('td', '', show(v.frag[k]))); tbody.append(r); }
    table.append(tbody); box.append(table);
  }
  if (d.added || d.removed) {
    const legend = el('p', 'diff-legend');
    legend.append(el('span', 'diff-key del', '− Only now'), el('span', 'diff-key ins', '+ Only in this version'));
    const pre = el('div', 'diff'); pre.setAttribute('role', 'region'); pre.setAttribute('aria-label', 'Text differences'); pre.tabIndex = 0;
    for (const line of foldDiff(d.lines)) {
      if (line.op === '…') { pre.append(el('div', 'diff-fold', `${line.count} unchanged ${line.count === 1 ? 'line' : 'lines'}`)); continue; }
      const row = el('div', `diff-line ${line.op === '+' ? 'ins' : line.op === '-' ? 'del' : ''}`.trim());
      row.append(el('span', 'diff-op', line.op === ' ' ? '' : line.op === '-' ? '−' : '+'), el('span', 'diff-text', line.text || ' '));
      pre.append(row);
    }
    box.append(legend, pre);
  }
  return box;
}

// ── Tool views ──
function section(text, aside) { const h = el('h2', 'section-label', text); if (aside) h.append(el('small', '', aside)); return h; }
function emptyState(title, body, code) {
  const p = el('div', 'empty'); p.append(el('strong', '', title), body);
  if (code) { p.append(' '); p.append(el('code', '', code)); }
  return p;
}
// Runs until the results match what is in the box, so typing fast never leaves stale results behind.
async function search() {
  const out = $('results');
  let q, found, failure = null;
  do {
    q = $('query').value.trim();
    try { found = await collect(q); failure = null; } catch (err) { failure = err; }
  } while ($('query').value.trim() !== q);
  if (failure) {
    // A query the DSL cannot parse is answered where the results would be, not only in the status bar.
    const alert = document.createElement('aihio-alert'), text = cleanError(failure).replace(/^parse error:\s*/i, '');
    alert.setAttribute('variant', 'destructive');
    alert.append(el('div', '', 'This search can’t be read'), el('div', '', `${capital(text)}.${/date/i.test(text) ? ' Dates take today, this-week, overdue or YYYY-MM-DD.' : ''}`));
    alert.children[0].slot = 'title'; alert.children[1].slot = 'description';
    out.replaceChildren(alert); return;
  }
  out.replaceChildren();
  if (!found.length) { out.append(q ? emptyState(`Nothing matches “${q}”`, 'Try fewer words, or drop a filter.') : emptyState('No fragments yet', 'Captures you save will be searchable here.')); return; }
  out.append(section(q ? `${found.length} ${found.length === 1 ? 'match' : 'matches'}` : 'Every fragment', q ? '' : 'Newest first'), rowList(found, { withSnippet: true }));
}
function searchFor(query) {
  return action(async () => { $('query').value = query; await navigate('search'); if (!query) await search(); });
}
function hints() {
  const tagCounts = new Map();
  for (const r of rows) for (const t of r.tags || []) tagCounts.set(t, (tagCounts.get(t) || 0) + 1);
  const top = [...tagCounts].sort((a, b) => b[1] - a[1])[0]?.[0];
  const tokens = ['type:decision', 'status:open', 'due:this-week', 'updated:this-week', 'has:links', top && `#${top}`].filter(Boolean);
  const box = $('hints');
  box.querySelectorAll('.hint').forEach(n => n.remove());
  box.append(...tokens.map(t => {
    const b = el('button', 'hint', t); b.type = 'button'; b.setAttribute('aria-describedby', 'hintsLabel');
    b.onclick = () => {
      const q = $('query').value.trim();
      if (!q.split(/\s+/).includes(t)) $('query').value = q ? `${q} ${t}` : t;
      action(search);
    };
    return b;
  }));
}
function highlight(json) {
  const pre = el('pre', 'context');
  let last = 0;
  for (const m of json.matchAll(/("(?:\\.|[^"\\])*")(\s*:)?|([{}[\],])|(\btrue\b|\bfalse\b|\bnull\b|-?\d+(?:\.\d+)?)/g)) {
    if (m.index > last) pre.append(json.slice(last, m.index));
    if (m[1]) { if (m[2]) { pre.append(el('span', 'k', m[1]), el('span', 'p', m[2])); } else pre.append(m[1]); }
    else pre.append(el('span', m[3] ? 'p' : 'n', m[0]));
    last = m.index + m[0].length;
  }
  pre.append(json.slice(last));
  return pre;
}
// Each count opens the search that lists what it counts.
function pulse(overdueCount) {
  const out = $('results');
  const openTodos = rows.filter(r => r.type === 'todo' && !settled(r)).length;
  const openRisks = rows.filter(r => r.type === 'risk' && !settled(r)).length;
  const figures = el('div', 'figures');
  const figure = (label, value, query, alert) => {
    const b = el('button', 'figure'); b.type = 'button';
    if (alert) b.dataset.tone = 'alert';
    b.append(el('span', 'figure-label', label), el('span', 'figure-value', value.toLocaleString()));
    b.title = query ? `Search ${query}` : 'List every fragment';
    b.onclick = () => searchFor(query);
    return b;
  };
  figures.append(figure('Fragments', rows.length, ''), figure('Open todos', openTodos, 'type:todo status:!done status:!cancelled'),
    figure('Overdue', overdueCount, 'due:overdue status:!done status:!cancelled', overdueCount > 0), figure('Open risks', openRisks, 'type:risk status:!resolved status:!accepted'));

  const byType = new Map(); for (const r of rows) byType.set(r.type, (byType.get(r.type) || 0) + 1);
  const meters = el('dl', 'meters');
  for (const [type, count] of [...byType].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))) {
    const share = count / rows.length * 100, dt = el('dt'), link = el('button', 'meter-link'), bar = el('dd', 'meter'), fill = el('i');
    link.type = 'button'; link.title = `Search type:${type}`; link.onclick = () => searchFor(`type:${type}`);
    link.append(typeIcon(type), typeLabel(type)); dt.append(link);
    fill.style.setProperty('--v', share.toFixed(1)); bar.append(fill);
    bar.setAttribute('aria-hidden', 'true');
    meters.append(dt, bar, el('dd', 'n', count.toLocaleString()), el('dd', 'pct', `${Math.round(share)}%`));
  }

  // Weeks start on Monday; bucket each fragment by the week it was last edited.
  const WEEKS = 12, weekStart = d => { const x = midnight(d); x.setDate(x.getDate() - ((x.getDay() + 6) % 7)); return x; };
  const monday = weekStart(Date.now()), counts = Array(WEEKS).fill(0);
  for (const r of rows) { const i = WEEKS - 1 - Math.round((monday - weekStart(r.updated_at)) / (7 * dayMs)); if (i >= 0 && i < WEEKS) counts[i]++; }
  const max = Math.max(...counts), peak = counts.lastIndexOf(max);
  const weekOf = i => { const d = new Date(monday); d.setDate(d.getDate() - (WEEKS - 1 - i) * 7); return d; };
  const chart = el('div', 'columns'); chart.style.setProperty('--count', WEEKS);
  chart.setAttribute('role', 'img');
  chart.setAttribute('aria-label', `Fragments by the week they were last edited: ${counts.map((c, i) => `${i === WEEKS - 1 ? 'this week' : `week of ${shortDate.format(weekOf(i))}`} ${c}`).join(', ')}.`);
  counts.forEach((c, i) => {
    const col = el('div', 'col'), v = max ? c / max * 82 : 0; // headroom for the peak's label
    col.style.setProperty('--v', v.toFixed(1)); if (i === WEEKS - 1) col.dataset.now = '';
    if (c) col.append(el('i'));
    if (c && i === peak) col.append(el('b', '', c));
    col.append(el('span', 'tip', `${c} last edited ${i === WEEKS - 1 ? 'this week' : `the week of ${shortDate.format(weekOf(i))}`}`));
    chart.append(col);
  });
  const axis = el('div', 'axis'); axis.append(el('span', '', shortDate.format(weekOf(0))), el('span', '', 'This week'));
  axis.setAttribute('aria-hidden', 'true');

  out.append(figures,
    section('By type', `${byType.size} ${byType.size === 1 ? 'type' : 'types'}`), meters,
    section('Last edited', 'Fragments by the week they were last edited, 12 weeks'), chart, axis,
    section('Recently edited'), rowList(rows.slice(0, 6)));
}
async function navigate(view) {
  if (view === 'capture') { if (await canLeave()) { reset(); $('title').focus(); } return; }
  if (view === 'history' && !opened) return;
  mode = view; queryAssist.hide(); paint(); list(); status();
  const where = name(scope), out = $('results');
  $('toolTitle').textContent = view === 'history' ? `History of ${opened.title || 'Untitled'}` : labels[view];
  $('searchForm').hidden = view !== 'search'; out.replaceChildren();
  const intro = $('intro');
  if (view === 'search') {
    intro.textContent = `Titles, bodies and fields in ${scope ? where : 'every opened vault'}. Combine words with filters; every term must match. Tab completes filters.`;
    hints(); $('query').focus(); if ($('query').value.trim()) await search(); return;
  }
  if (view === 'history') { await historyView(); return; }
  if (view === 'pulse') {
    intro.textContent = `Where ${scope ? where : 'your opened vaults'} stand${scope ? 's' : ''} today. Select a count to list what it counts.`;
    if (!rows.length) { out.append(emptyState('Nothing to measure yet', 'Capture a few fragments and their counts will show here.')); return; }
    const late = (await collect('due:overdue')).filter(r => !settled(r)).length;
    pulse(late); return;
  }
  if (view === 'graph') {
    intro.textContent = `Every fragment in ${scope ? where : 'your opened vaults'} and the links between them. Select one to open it; scroll to zoom, drag to pan.`;
    if (!rows.length) { out.append(emptyState('Nothing to draw yet', 'Capture a few fragments and link them with [[ to see them here.')); return; }
    const paths = scope ? [scope] : vaults.map(v => v.path);
    const edges = (await Promise.all(paths.map(async p => (await api.links(p)).map(e => [key({ vault: p, id: e.source }), key({ vault: p, id: e.target })])))).flat();
    out.append(vaultGraph(rows, edges, {
      key, current: opened ? key(opened) : null, settled, open: r => openRow(r, true),
      label: r => r.title || 'Untitled', describe: r => [typeLabel(r.type), r.status && capital(r.status), !scope && name(r.vault)].filter(Boolean).join(', ')
    }));
    return;
  }
  const seed = opened || rows[0];
  if (!seed) { intro.textContent = 'The context an agent can read.'; out.append(emptyState('Capture a fragment first', 'This view works from the open fragment, or the most recent one.')); return; }
  const note = await api.note(seed.vault, seed.id);
  intro.textContent = 'Exactly what an agent would read from this fragment. No model is connected, and nothing leaves this device.';
  const card = document.createElement('aihio-card'); card.className = 'context-card';
  const header = document.createElement('aihio-card-header'), title = document.createElement('aihio-card-title'), desc = document.createElement('aihio-card-description'), content = document.createElement('aihio-card-content');
  title.append(typeIcon(note.type), note.title || 'Untitled');
  desc.textContent = `Saved ${typeLabel(note.type).toLowerCase()} in ${name(seed.vault)}, ${stamp.format(new Date(note.updated_at))}. Unsaved edits are not included.`;
  header.append(title, desc);
  content.append(highlight(JSON.stringify({ vault: name(seed.vault), id: note.id, type: note.type, title: note.title, body: (note.body || '').replace(/^\n/, ''), links: note.links }, null, 2)));
  card.append(header, content); out.append(card);
}

// ── Settings ──
let pending = null;
function renderSettings(view) {
  $('modelPath').textContent = pending.whisperModel || 'Not chosen'; $('modelPath').title = pending.whisperModel;
  $('binPath').textContent = pending.whisperBin || 'whisper-cli from PATH'; $('binPath').title = pending.whisperBin;
  $('clearBin').hidden = !pending.whisperBin;
  $('speechState').textContent = speech.ready ? 'Ready to dictate.' : speech.problem || 'Not set up.';
  $('speechState').dataset.tone = speech.ready ? 'ok' : 'warn';
  $('shortcutState').textContent = view.shortcut?.error || (view.shortcut?.active ? `${view.quickCaptureShortcut} opens quick capture.` : 'Leave empty for none.');
  $('shortcutState').dataset.tone = view.shortcut?.error ? 'error' : '';
  $('captureCommand').textContent = view.captureCommand;
}
async function openSettings() {
  const view = await api.settings();
  pending = { whisperModel: view.whisperModel, whisperBin: view.whisperBin };
  $('shortcutInput').value = view.quickCaptureShortcut;
  renderSettings(view);
  $('settings').open();
}
async function pickFor(kind) {
  const path = await api.pickFile(kind);
  if (!path) return;
  pending[kind === 'model' ? 'whisperModel' : 'whisperBin'] = path;
  renderSettings(await api.settings());
}
$('pickModel').addEventListener('click', () => pickFor('model').catch(status));
$('pickBin').addEventListener('click', () => pickFor('binary').catch(status));
$('clearBin').addEventListener('click', async () => { pending.whisperBin = ''; renderSettings(await api.settings()); });
$('settingsSave').addEventListener('click', async () => {
  try {
    const view = await api.saveSettings({ ...pending, quickCaptureShortcut: $('shortcutInput').value });
    await checkSpeech(); renderSettings(view);
    if (!view.shortcut?.error) { $('settings').close(); status(speech.ready ? 'Settings saved. Dictation is ready.' : 'Settings saved.'); }
  } catch (err) { $('shortcutState').textContent = cleanError(err); $('shortcutState').dataset.tone = 'error'; }
});

// ── Go to… (Ctrl+K) ──
function commands() {
  const out = [], add = (label, run, extra = {}) => out.push({ label, run, ...extra });
  add('New capture', () => newCapture(), { icon: 'plus', keys: `${mod} N`, keywords: 'note create' });
  for (const s of typesIn(scope)) if (s.name !== 'note') add(`New ${s.name}`, () => newCapture(s.name), { glyph: typeIcon(s.name), keywords: 'create capture' });
  add('Search', () => action(() => navigate('search')), { icon: 'search', keys: `${mod} F` });
  add('Graph of the vault', () => action(() => navigate('graph')), { icon: 'graph', keys: 'G', keywords: 'links' });
  add('Pulse', () => action(() => navigate('pulse')), { icon: 'pulse', keys: 'P', keywords: 'stats counts' });
  add('Agent context', () => action(() => navigate('agent')), { icon: 'agent', keys: 'A' });
  if (dirty && mode === 'capture') add('Save', save, { icon: 'check', keys: `${mod} S` });
  if (opened) {
    const t = schemaFor(opened.vault, opened.type)?.toggle;
    if (t) { const next = opened[t.field] === t.on ? t.off : t.on; add(`Mark ${next}`, toggleField, { icon: 'check', keys: 'T', keywords: t.field }); }
    add('History of this fragment', showHistory, { icon: 'history', keywords: 'versions restore' });
    add('Copy link to this fragment', copyLink, { icon: 'copy', keywords: 'wiki id' });
    add(opened.archived ? 'Unarchive this fragment' : 'Archive this fragment', toggleArchive, { icon: 'archive' });
    add('Move this fragment to the trash', trashOpened, { icon: 'trash', keywords: 'delete remove' });
  }
  for (const v of vaults) if (v.path !== scope) add(`Switch to ${v.name}`, () => switchScope(v.path), { glyph: vaultMark(v.path), keywords: 'vault' });
  if (scope && vaults.length > 1) add('Switch to all opened vaults', () => switchScope('@all'), { icon: 'layers', keywords: 'vault' });
  for (const s of suggested) add(`Open ${s.name} vault (${s.hint})`, () => switchScope(`@suggest:${s.path}`), { icon: 'folder', keywords: 'vault' });
  add('Open vault folder…', () => switchScope('@open'), { icon: 'folder', keywords: 'vault' });
  add(side.collapsed ? 'Show sidebar' : 'Hide sidebar', toggleSidebar, { icon: 'sidebar', keys: `${mod} \\` });
  add(openOnly ? 'Show every fragment in the list' : 'Show only open fragments in the list', toggleOpenOnly, { icon: 'check', keywords: 'filter done' });
  add('Quick capture window', () => api.openCapture(), { icon: 'bolt' });
  if (!speech.ready) add('Set up dictation', openSettings, { icon: 'mic', keywords: 'whisper speech' });
  add('Settings', openSettings, { icon: 'settings', keywords: 'preferences dictation shortcut' });
  return out;
}
const goTo = palette({
  fragments: () => rows, commands,
  describe: r => [typeLabel(r.type), !scope && name(r.vault), age(r.updated_at)].filter(Boolean).join(' · '),
  open: r => openRow(r, true)
});

// ── Wiring ──
const DATES = ['today', 'yesterday', 'tomorrow', 'this-week', 'last-week', 'next-week', 'this-month', 'last-month', 'overdue'];
const FILTER_HINTS = { type: 'fragment type', status: 'state', priority: 'low … critical', tag: 'a tag, or #tag', due: 'due date', created: 'date created', updated: 'date edited', has: 'links, due, attachments', is: 'archived, all', by: 'author', linked: 'links to an ID' };
const assistValues = field => ({ type: typesIn(scope).map(s => s.name), tag: allTags(), due: DATES, created: DATES, updated: DATES, has: ['links', 'due', 'attachments'], is: ['archived', 'all'] })[field] ?? fieldValues(field);
const queryAssist = searchAssist($('query'), { id: 'queryAssist', values: assistValues, describe: f => FILTER_HINTS[f] });
searchAssist($('filter'), { id: 'filterAssist', values: assistValues, describe: f => FILTER_HINTS[f] });
let searchTimer = 0;
const halfTyped = q => /(^|\s)!?[a-z]+:!?$/i.test(q); // "status:" waits for its value
$('query').addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { if (mode === 'search' && !halfTyped($('query').value.trim())) action(search); }, 160);
});
$('filter').addEventListener('input', onFilter);
$('filter').addEventListener('keydown', e => {
  const first = $('recent').querySelector('.row');
  if (e.key === 'ArrowDown' && first) { e.preventDefault(); first.focus(); }
  else if (e.key === 'Enter' && first && $('filterAssist').hidden) { e.preventDefault(); first.click(); } // a keyboard click: opens it, caret in the text
});
// The list moves with the arrow keys; Enter opens a fragment with the caret in its text.
$('recent').addEventListener('keydown', e => {
  const all = [...$('recent').querySelectorAll('.row')], i = all.indexOf(document.activeElement);
  if (i < 0) return;
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: all.length - 1 }[e.key];
  if (to === undefined) return;
  e.preventDefault();
  if (to < 0) $('filter').focus(); else all[Math.min(all.length - 1, to)].focus();
});
$('openOnly').onclick = toggleOpenOnly;
$('sidebarToggle').addEventListener('click', toggleSidebar);
$('goTo').onclick = () => goTo.open();
$('openSettings').onclick = () => openSettings().catch(status);
$('quickCapture').onclick = () => api.openCapture().catch(status);
$('fragmentMenu').addEventListener('aihio-select', e => {
  e.stopPropagation();
  ({ history: showHistory, copy: copyLink, archive: toggleArchive, trash: trashOpened })[e.detail.value]?.();
});
$('vaultMenu').addEventListener('aihio-select', e => switchScope(e.detail.value));
$('resume').onclick = () => action(() => { mode = 'capture'; paint(); list(); editor.focus(); });
$('destination').onchange = () => { meta(); paint(); editor.refresh(); rememberDraft(); }; $('save').onclick = save;
$('title').oninput = () => { const t = $('title'); if (/[\r\n]/.test(t.value)) t.value = t.value.replace(/\s*[\r\n]+\s*/g, ' '); changed(); };
$('title').onkeydown = e => { if (e.key === 'Enter' && !e.isComposing && !(e.metaKey || e.ctrlKey)) { e.preventDefault(); editor.focus(); } };
$('record').onclick = () => {
  if (phase === 'recording') return voice.stop();
  if (phase !== 'idle') return;
  if (!speech.ready) return openSettings().catch(status);
  voice.start();
};
$('cancel').onclick = () => voice.stop(true);
$('searchForm').onsubmit = e => { e.preventDefault(); clearTimeout(searchTimer); action(search); };
for (const b of document.querySelectorAll('[data-view]')) b.onclick = () => b.dataset.view === 'capture' ? newCapture() : action(() => navigate(b.dataset.view));
document.addEventListener('keydown', e => {
  if (goTo.isOpen || $('settings').hasAttribute('open')) return;
  if (e.key === 'Escape' && phase === 'recording') return voice.stop(true);
  const ctrl = (e.metaKey || e.ctrlKey) && !e.altKey, k = e.key.toLowerCase();
  if (ctrl && !e.shiftKey) {
    const run = { enter: () => mode === 'capture' && save(), s: () => mode === 'capture' && save(), k: () => goTo.open(), n: () => newCapture(),
      f: () => action(() => navigate('search')), '\\': toggleSidebar }[k];
    if (run) { e.preventDefault(); run(); }
    return;
  }
  if (ctrl && e.shiftKey && k === 'p') { e.preventDefault(); goTo.open('>'); return; }
  if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing || e.target.closest('input,textarea,select,[contenteditable=true],aihio-dropdown,aihio-date-picker,aihio-input')) return;
  const next = ({ '+': 'capture', '/': 'search', g: 'graph', a: 'agent', p: 'pulse' })[k];
  if (next === 'capture') { e.preventDefault(); newCapture(); }
  else if (next) { e.preventDefault(); action(() => navigate(next)); }
  else if (k === 't' && mode === 'capture') { e.preventDefault(); toggleField(); }
});
// Drafts are kept on disk, so only work that cannot resume holds the window.
window.addEventListener('beforeunload', e => { rememberDraft(true); if (['recording', 'transcribing', 'saving'].includes(phase)) e.returnValue = false; });
applySide();
(async () => {
  try {
    vaults = await api.vaults();
    const known = p => vaults.some(v => v.path === p), last = remember.get(SCOPE), draft = remember.get(DRAFT);
    const usable = draft && typeof draft.body === 'string' && (!draft.vault || known(draft.vault)) ? draft : null;
    scope = last === '' || known(last) ? last : vaults[0].path;
    if (usable?.vault && scope && scope !== usable.vault) scope = usable.vault;
    [suggested] = await Promise.all([api.suggestedVaults().catch(() => []), loadSchemas(vaults.map(v => v.path)), checkSpeech()]);
    try { rows = await collect(); } catch (err) { if (scope === vaults[0].path) throw err; scope = vaults[0].path; rows = await collect(); }
    remember.set(SCOPE, scope); options();
    if (usable) await restoreDraft(usable); else reset();
  } catch (err) { status(err); $('empty').textContent = 'Unable to load fragments.'; }
  finally { setPhase('idle'); }
})();
