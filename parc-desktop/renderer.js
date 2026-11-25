import '@luntta/aihio';
import { markdownEditor, fold } from './editor.js';
import { dictation } from './speech.js';
import { icon, typeIcon, statusIcon, hydrateIcons } from './icons.js';
import { vaultGraph } from './graph.js';
import { palette } from './palette.js';
import { searchAssist } from './assist.js';
import { diffLines, fold as foldDiff, stat } from './diff.js';
import { LANGUAGES } from './languages.cjs';
import { vaultSettings } from './vaultsettings.js';
import { typeView, dragging } from './board.js';
import { relatedPanel } from './related.js';
import { workspace, FRAGMENT } from './tabs.js';
import { agentChat } from './agent.js';
import { sidebarList } from './sidebar.js';
import { renderTrashView, renderDoctorView, pulseDailySections } from './views.js';

const $ = id => document.getElementById(id), api = window.parc;
hydrateIcons();
const isMac = /Mac/.test(navigator.platform), mod = isMac ? '⌘' : 'Ctrl';
$('saveKey').textContent = isMac ? '⌘S' : 'Ctrl S';
$('save').title = `Save fragment (${mod}+S)`;

let vaults = [], suggested = [], scope = '', rows = [], phase = 'loading';
let speechPhase = 'idle', pendingWork = 0, savingDocs = 0, navSeq = 0, scopeSeq = 0;
let speech = { ready: true };              // dictation: { ready, problem?, language, languages, warning? }; checked at launch and after Settings
const schemas = new Map();                 // vault path → schema.list
const changedVaults = new Set();           // vaults whose fragments changed on disk, waiting for an idle moment
const changedSchemas = new Set();          // vaults whose schemas or config.yml changed on disk
const outside = new Map();                 // vault path → rows, for open tabs whose vault is outside the scope
const key = r => JSON.stringify([r.vault, r.id]);
const name = p => vaults.find(v => v.path === p)?.name || 'All vaults';
const labels = { search: 'Search', graph: 'Graph', agent: 'Agent', pulse: 'Pulse', history: 'History', trash: 'Trash', doctor: 'Vault health' };
// A type's view is named for what it holds: Todos, Risks, Ideas.
const plural = t => { const w = typeLabel(t); return /(s|x|z|ch|sh)$/i.test(w) ? `${w}es` : /[^aeiou]y$/i.test(w) ? `${w.slice(0, -1)}ies` : `${w}s`; };
const blocking = () => speechPhase !== 'idle';

// Per-viewer memory: last vault, the open tabs and their unsaved text, the sidebar's shape, filter, grouping, order
// and folded groups. The app works without it.
const WORKSPACE = 'parc.workspace', DRAFT = 'parc.draft', SCOPE = 'parc.scope', SIDE = 'parc.sidebar', OPEN_ONLY = 'parc.openOnly', LIST_VIEW = 'parc.listView', FOLDED = 'parc.folded', TYPE_VIEWS = 'parc.typeViews';
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
const monthYear = new Intl.DateTimeFormat(undefined, { month: 'short', year: 'numeric' });
function age(iso) { // compact, for lists: a few characters, so it fits beside a one-line title
  const t = new Date(iso), s = (Date.now() - t) / 1000, days = Math.round((midnight(Date.now()) - midnight(t)) / dayMs);
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (days === 0) return `${Math.floor(s / 3600)}h`;
  if (days < 7) return `${days}d`;
  return t.getFullYear() === new Date().getFullYear() ? shortDate.format(t) : monthYear.format(t);
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
// Status words differ per type; what each means for attention (open, active, done or closed) is in the type's schema.
const statusField = (vault, type) => schemaFor(vault, type)?.fields.find(f => f.name === 'status');
const tone = (vault, type, status) => (status && statusField(vault, type)?.categories?.[status]) || 'open';
const settled = r => ['done', 'closed'].includes(tone(r.vault, r.type, r.status));
function state(vault, type, status) { const s = el('span', 'state', capital(status)); s.dataset.tone = tone(vault, type, status); return s; }
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
// Types new captures can use (a vault can turn types off), and the one they start as.
const enabledTypes = vault => typesIn(vault).filter(s => s.enabled !== false);
const defaultType = vault => (enabledTypes(vault).find(s => s.default) || enabledTypes(vault)[0])?.name || 'note';
// The statuses a type (or, with none given, any type in view) counts as done or closed: search filters that leave them out.
const settledValues = (vault, type) => [...new Set(typesIn(vault).filter(s => !type || s.name === type).flatMap(s => {
  const f = s.fields.find(x => x.name === 'status');
  return (f?.values || []).filter(v => ['done', 'closed'].includes(f.categories?.[v]));
}))].map(v => `status:!${v}`).join(' ');
// Every value an enum field takes in the types in view, e.g. all statuses.
const fieldValues = field => [...new Set(typesIn(scope).flatMap(s => s.fields.filter(f => f.name === field && f.values).flatMap(f => f.values)))];
const allTags = () => [...new Set(rows.flatMap(r => r.tags || []))].sort();

// ── Tabs ──
// Each tab is a fragment in the editor or a view. A fragment tab (kind 'doc') has its own text, editor and Related
// panel: `opened` is the saved fragment, or null for a new capture, whose type, fields and tags wait in `fresh`
// until its first save (`dest`: the vault chosen for it when the scope is every vault); `dirty` while its text has
// unsaved changes. A view tab (kind 'view') shows `view`: pulse, search, graph, agent (its chat in `agent`), history
// (of `target`) or type (`viewType`'s board, matrix or cards). A fragment is open in at most one tab. The current
// group's shown tab (`tab`) is what the bar, the keys and Go to… act on; `recentDoc` is the fragment tab used last
// (what the Agent view offers to share with a question).
let tab = null, recentDoc = null, tabSeq = 0, keepNext = null;
const cur = () => tab?.kind === 'doc' ? tab : null;
const modeOf = t => !t ? 'empty' : t.kind === 'doc' ? 'capture' : t.view;
const docTabs = () => ws.tabs().filter(t => t.kind === 'doc');
const viewTabs = () => ws.tabs().filter(t => t.kind === 'view');
const tabOf = r => docTabs().find(d => d.opened && d.opened.vault === r.vault && d.opened.id === r.id) || null;
const viewLabel = t => t.view === 'type' ? plural(t.viewType) : t.view === 'history' ? `History of ${t.target?.title || 'Untitled'}`
  : t.view === 'agent' ? t.agent?.title() || labels.agent : labels[t.view];
const blankCapture = d => ({ type: defaultType(scope || d?.dest), fields: {}, tags: [] });
const templateFor = (vault, type) => schemaFor(vault, type)?.template || '';
const isEmpty = d => {
  if (d.ui.title.value.trim()) return false;
  const body = d.editor.text();
  if (!body.trim()) return true;
  return !d.opened && !!d.templateBody && body === d.templateBody;
};
const destination = d => d.opened?.vault || scope || d.dest;
const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif)$/i;
const vaultToken = p => vaults.find(v => v.path === p)?.token || '';

// Apply the type's Markdown template to an untouched new capture without marking it dirty.
function applyTemplate(d) {
  if (d.opened || d.dirty) return;
  const current = d.editor.text();
  if (current.trim() && current !== d.templateBody) return;
  const tpl = templateFor(destination(d), d.fresh?.type);
  if (tpl === current) { d.templateBody = tpl; return; }
  d.templateBody = tpl;
  d.editor.load(tpl);
}

// A tab's content from its template: each id gains the tab's number, and so do the attributes that name it.
function instance(template, n) {
  const root = document.importNode($(template).content, true).firstElementChild, ui = { scroller: root }, ids = new Set();
  for (const node of root.querySelectorAll('[id]')) { ids.add(node.id); ui[node.id] = node; node.id = `${node.id}-${n}`; }
  for (const node of root.querySelectorAll('[for], [aria-labelledby], [aria-describedby], [aria-controls]')) {
    for (const attr of ['for', 'aria-labelledby', 'aria-describedby', 'aria-controls']) {
      const v = node.getAttribute(attr);
      if (v) node.setAttribute(attr, v.split(/\s+/).map(x => ids.has(x) ? `${x}-${n}` : x).join(' '));
    }
  }
  hydrateIcons(root);
  return { root, ui };
}
function docTab(note = null, draftId = null) {
  const n = ++tabSeq, { root, ui } = instance('docTemplate', n);
  const d = { kind: 'doc', n, panel: root, ui, opened: null, fresh: null, dirty: false, dest: '', templateBody: '',
    attachments: null, draftId: draftId || crypto.randomUUID(), draftSaved: false, draftAt: null, saving: false, metaQueue: Promise.resolve() };
  d.links = linksFor(d);
  d.editor = markdownEditor(ui.draft, () => changed(d), d.links);
  // Dragging external files onto a saved fragment, or pasting an image into its editor, attaches it.
  const hasExternalFiles = e => {
    const types = [...(e.dataTransfer?.types || [])];
    return types.includes('Files') && !types.includes(FRAGMENT) && !types.includes('application/x-parc-tab');
  };
  ui.document.addEventListener('dragover', e => {
    if (!hasExternalFiles(e)) return;
    e.preventDefault(); e.dataTransfer.dropEffect = 'copy';
    ui.document.dataset.fileDrop = '';
  });
  ui.document.addEventListener('dragleave', e => { if (!ui.document.contains(e.relatedTarget)) delete ui.document.dataset.fileDrop; });
  ui.document.addEventListener('drop', e => {
    delete ui.document.dataset.fileDrop;
    if (!hasExternalFiles(e)) return;
    e.preventDefault(); e.stopPropagation();
    const files = [...(e.dataTransfer?.files || [])];
    if (files.length) attachDroppedFiles(d, files);
  });
  ui.draft.addEventListener('paste', e => {
    const items = [...(e.clipboardData?.items || [])];
    const image = items.find(i => i.kind === 'file' && i.type.startsWith('image/'));
    if (!image) return;
    const file = image.getAsFile();
    if (!file) return;
    e.preventDefault();
    const ext = ({ 'image/png': '.png', 'image/jpeg': '.jpg', 'image/gif': '.gif', 'image/webp': '.webp', 'image/avif': '.avif' })[file.type] || '.png';
    const stampName = `pasted-${new Date().toISOString().replace(/[-:T]/g, '').slice(0, 15)}${ext}`;
    attachDroppedFiles(d, [new File([file], file.name && file.name !== 'image.png' ? file.name : stampName, { type: file.type })], { insertImage: true });
  });
  // Fragments that bear on what is being written, from the open fragment's vault, or from every vault in view
  // for a new capture. Asked again as you type or dictate; see related.js.
  d.related = relatedPanel(ui.related, {
    seed: () => recallSeed(d), recall: api.recall, note: api.note,
    open: s => openRow({ vault: s.vault, id: s.id }),
    link: s => { d.editor.insert(`[[${d.links.shortId(s)}]]`); status(`Linked “${s.title || 'Untitled'}”.`); },
    date: iso => { const t = new Date(iso); return t.getFullYear() === new Date().getFullYear() ? shortDate.format(t) : monthYear.format(t); }
  });
  ui.title.oninput = () => { const t = ui.title; if (/[\r\n]/.test(t.value)) t.value = t.value.replace(/\s*[\r\n]+\s*/g, ' '); changed(d); };
  ui.title.onkeydown = e => { if (e.key === 'Enter' && !e.isComposing && !(e.metaKey || e.ctrlKey)) { e.preventDefault(); d.editor.focus(); } };
  load(d, note);
  return d;
}
function viewTab(view, { type = null, target = null, query = '', chat = null } = {}) {
  const n = ++tabSeq, { root, ui } = instance('viewTemplate', n);
  const v = { kind: 'view', n, view, viewType: type, target, panel: root, ui, stale: true, rendering: null, everything: false, chatId: chat };
  root.tabIndex = -1; // a view's page takes focus, so it scrolls from the keyboard
  ui.tool.dataset.view = view;
  ui.searchForm.hidden = view !== 'search';
  if (view === 'search') {
    ui.query.value = query;
    searchAssist(ui.query, { id: `queryAssist-${n}`, values: assistValues, describe: f => FILTER_HINTS[f] });
    // Results follow the box as you type; a search you typed keeps its tab.
    let timer = 0;
    ui.query.addEventListener('input', () => {
      clearTimeout(timer); ws.keep(v); ws.update(v); rememberWorkspace();
      timer = setTimeout(() => { if (ws.shown(v) && !halfTyped(ui.query.value.trim())) action(() => search(v)); }, 160);
    });
    ui.searchForm.onsubmit = e => { e.preventDefault(); clearTimeout(timer); action(() => search(v)); };
  }
  return v;
}
const VIEW_ICONS = { pulse: 'pulse', search: 'search', graph: 'graph', agent: 'agent', history: 'history', trash: 'trash', doctor: 'shield' };
function tabLabel(t) {
  if (t.kind === 'doc') {
    const type = t.opened?.type ?? t.fresh?.type, title = t.ui.title.value.trim();
    const text = title || (t.opened ? 'Untitled' : `New ${type || 'note'}`);
    return { text, glyph: typeIcon(type), dirty: t.dirty, title: `${text}\n${t.opened ? `${typeLabel(type)} in ${name(t.opened.vault)}` : 'Not saved yet'}` };
  }
  const q = t.view === 'search' ? t.ui.query.value.trim() : '';
  const text = q ? `Search: ${q}` : t.view === 'history' ? `History: ${t.target?.title || 'Untitled'}` : viewLabel(t);
  return { text, glyph: t.view === 'type' ? typeIcon(t.viewType) : icon(VIEW_ICONS[t.view]), title: t.view === 'history' ? viewLabel(t) : text };
}
// What an empty group shows: how to fill it.
function emptyGroup() {
  const box = el('div', 'watermark'), dl = el('dl');
  const keys = [['Open a fragment here', 'Click it in the list'], ['Open beside', `${mod}+click, or ${mod} ↵`], ['Go to…', `${mod} K`], ['New capture', `${mod} N`], ['Move a tab between groups', `${mod} Alt ← →`], ['Close a tab', `${mod} W`]];
  for (const [what, how] of keys) dl.append(el('dt', '', what), el('dd', '', how));
  box.append(el('p', 'watermark-title', 'Nothing open in this group'), dl, el('p', 'watermark-hint', 'Or drag a tab, or a fragment from the list, here.'));
  return box;
}
// Tab commands in a tab's context menu, beside the layout's own.
function tabMenu(t) {
  if (t.view === 'agent' && t.agent?.saved()) return [{ label: 'Delete chat', run: () => deleteChat(t.agent.id(), t.agent.title()) }];
  if (t.kind !== 'doc' || !t.opened) return [];
  return [
    { label: 'Copy link', run: () => copyLink(t) },
    { label: 'History', run: () => showHistory(t) }
  ];
}
const closedTabs = []; // what Reopen closed tab brings back, newest last
const ws = workspace($('groups'), {
  label: tabLabel,
  canClose: async t => !blocking() && (t.kind !== 'doc' || await canLeave(t)),
  closed(t) {
    if (recentDoc === t) recentDoc = null;
    const saved = saveTab(t);
    if (t.kind === 'view' || t.opened) { closedTabs.push({ ...saved, draft: null }); if (closedTabs.length > 20) closedTabs.shift(); }
    discard(t);
  },
  activate: (t, { focus }) => activated(t, focus),
  changed: () => rememberWorkspace(),
  drop: (r, { group, index }) => openRow(r, { group, index, focus: true }),
  menu: tabMenu,
  empty: emptyGroup,
  busy: blocking,
  lead: $('sidebarShow'),
  tools: $('docActions')
});
// The current group's shown tab changed. Whatever is on screen in any group is brought up to date: a view whose
// fragments changed while hidden redraws, and a fragment's Related panel asks again if its text or vault moved on.
function activated(t, focus) {
  tab = t;
  if (t?.kind === 'doc') recentDoc = t;
  for (const g of ws.groups) {
    const s = g.shown;
    if (s?.kind === 'view' && s.stale) draw(s);
    else if (s?.kind === 'doc') s.related.now();
    else if (s?.view === 'agent') s.agent?.shown();
  }
  syncPhase(); sidebar.markSelection({ scroll: true });
  if (focus) focusTab(t);
}
function focusTab(t) {
  if (!t) { ws.group?.empty.querySelector('.watermark')?.focus?.(); return; }
  if (t.kind === 'doc') { if (t.opened || t.ui.title.value) t.editor.focus(); else t.ui.title.focus(); }
  else if (t.view === 'search') t.ui.query.focus();
  else if (t.view === 'agent' && t.agent?.focus()) return;
  else t.ui.scroller.focus({ preventScroll: true });
}
// A tab that never joined the layout, or left it: its editor and Related panel stop, and any draft file goes.
function discard(t) {
  if (t.kind === 'doc') {
    t.related.reset(); t.editor.destroy();
    if (t.draftSaved) { t.draftSaved = false; api.deleteDraft(t.draftId).catch(() => {}); }
  } else t.agent?.destroy();
}
// Ctrl+W closes the current tab, or the current group when it is empty and not the only one.
function closeTab(t = tab) {
  if (blocking()) return;
  if (t) return ws.close(t);
  ws.closeEmpty();
}
function reopenTab() {
  const saved = closedTabs.pop();
  if (!saved) return;
  return action(async () => {
    const t = await restoreTab(saved);
    if (!t) { status('That tab’s fragment is gone; it may have been moved to the trash.'); return; }
    if (t.kind === 'doc' && t.opened && tabOf(t.opened)) { discard(t); ws.activate(tabOf(t.opened)); return; }
    ws.open(t, { focus: true });
    if (t.kind === 'doc') await Promise.all([loadNeighbors(t), loadAttachments(t)]);
  });
}

// ── Remembering the workspace ──
// Layout (groups, tabs, sizes) stays tiny in localStorage; each dirty tab's unsaved text is written atomically to
// userData/drafts/<id>.json on disk so big notes never stall or fill localStorage. A draft's `base` is the revision
// its text was written against, so restoring an old draft over a newer file still ends in the overwrite question.
function draftPayload(t) {
  if (!t.dirty || isEmpty(t)) return null;
  t.draftAt ||= new Date().toISOString();
  return { title: t.ui.title.value, body: t.editor.text(), base: t.opened?.revision ?? null, at: t.draftAt };
}
function saveTab(t) {
  if (t.kind === 'view') {
    const chat = t.view === 'agent' ? (t.agent ? (t.agent.saved() ? t.agent.id() : null) : t.chatId) : null;
    return { view: t.view, ...(t.viewType ? { type: t.viewType } : {}), ...(t.target ? { target: t.target } : {}), ...(t.view === 'search' ? { query: t.ui.query.value } : {}), ...(chat ? { chat } : {}) };
  }
  const hasDraft = t.dirty && !isEmpty(t);
  const draft = hasDraft ? { id: t.draftId, base: t.opened?.revision ?? null, at: t.draftAt || (t.draftAt = new Date().toISOString()) } : null;
  return t.opened ? { vault: t.opened.vault, id: t.opened.id, draft } : { vault: destination(t) || '', fresh: t.fresh, draft };
}
let saveTimer = 0, restoring = true;
function persistDrafts(sync = false) {
  const entries = [];
  for (const d of docTabs()) {
    const payload = draftPayload(d);
    if (payload) {
      d.draftSaved = true;
      entries.push({ id: d.draftId, draft: payload });
    } else if (d.draftSaved) {
      d.draftSaved = false;
      entries.push({ id: d.draftId, draft: null });
    }
  }
  if (!entries.length) return;
  if (sync) { api.flushDrafts(entries); return; }
  for (const e of entries) {
    if (e.draft) api.saveDraft(e.id, e.draft).catch(() => {});
    else api.deleteDraft(e.id).catch(() => {});
  }
}
function rememberWorkspace(now = false, { syncDisk = false } = {}) {
  clearTimeout(saveTimer);
  if (restoring) return;
  const write = () => {
    remember.set(WORKSPACE, {
      groups: ws.groups.map(g => ({ size: g.size, shown: g.tabs.indexOf(g.shown), tabs: g.tabs.map(t => ({ ...saveTab(t), ...(t.preview ? { preview: true } : {}) })) })),
      current: ws.groups.indexOf(ws.group)
    });
    persistDrafts(syncDisk);
  };
  if (now) write(); else saveTimer = setTimeout(write, 300);
}
// A tab as it was remembered, or null when it can't come back (its vault is gone, or its fragment without a draft).
async function restoreTab(e) {
  const known = p => vaults.some(v => v.path === p);
  if (e.view) {
    if (!labels[e.view] && e.view !== 'type') return null;
    if (e.view === 'type' && typeof e.type !== 'string') return null;
    if (e.view === 'history' && !(e.target?.id && known(e.target.vault))) return null;
    const chat = e.view === 'agent' && typeof e.chat === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(e.chat) ? e.chat : null;
    return viewTab(e.view, { type: e.type, target: e.target, query: typeof e.query === 'string' ? e.query : '', chat });
  }
  if (e.vault && !known(e.vault)) return null;
  let draft = null, draftId = null;
  if (e.draft && typeof e.draft.id === 'string') {
    draftId = e.draft.id;
    draft = await api.loadDraft(draftId).catch(() => null);
  } else if (e.draft && typeof e.draft.body === 'string') {
    // Legacy draft stored inline in localStorage before drafts moved to disk.
    draft = e.draft;
  }
  let full = null;
  if (e.id) { try { full = await api.note(e.vault, e.id); } catch { if (!draft) return null; } }
  const d = docTab(full ? { ...full, vault: e.vault } : null, draftId);
  if (!full) { if (!scope && e.vault) d.dest = e.vault; d.fresh = { ...blankCapture(d), ...(e.fresh || {}) }; if (!draft) applyTemplate(d); meta(d); }
  if (draft) {
    if (full) d.opened = { ...d.opened, revision: /^[0-9a-f]{16}$/.test(draft.base || '') ? draft.base : full.revision };
    d.ui.title.value = draft.title || ''; d.editor.load(draft.body); d.dirty = true;
    d.draftAt = draft.at || new Date().toISOString(); d.draftSaved = !!draftId;
    d.restored = { at: d.draftAt, orphan: !!e.id && !full };
  }
  return d;
}
async function restoreWorkspace(saved) {
  const shown = new Map(), groups = Array.isArray(saved?.groups) ? saved.groups : [];
  let wanted = null;
  for (const [i, sg] of groups.entries()) {
    const g = i < ws.groups.length ? ws.groups[i] : ws.addGroup();
    g.size = Number(sg?.size) > 0 ? Number(sg.size) : 1;
    if (i === saved.current) wanted = g;
    for (const [j, entry] of (Array.isArray(sg?.tabs) ? sg.tabs : []).entries()) {
      const t = await restoreTab(entry || {}).catch(() => null);
      if (!t) continue;
      if (t.kind === 'doc' && t.opened && tabOf(t.opened)) { discard(t); continue; }
      ws.open(t, { group: g, preview: !!entry.preview && !t.dirty, activate: false });
      if (j === sg.shown) shown.set(g, t);
    }
  }
  ws.prune(); // groups whose tabs couldn't come back go
  const current = ws.groups.includes(wanted) ? wanted : ws.groups[0];
  for (const g of ws.groups) if (g !== current) { const t = shown.get(g) ?? g.shown; if (t?.group) ws.activate(t); }
  const t = shown.get(current) ?? current.shown;
  if (t?.group) ws.activate(t); else ws.focusGroup(ws.groups.indexOf(current));
  for (const d of docTabs()) { loadNeighbors(d).catch(() => {}); loadAttachments(d).catch(() => {}); }
  api.pruneDrafts(docTabs().filter(d => d.dirty).map(d => d.draftId)).catch(() => {});
  const drafts = docTabs().filter(d => d.restored);
  if (drafts.length === 1) {
    const { at, orphan } = drafts[0].restored;
    status(orphan ? 'Restored your unsaved draft. Its fragment was deleted, so it will save as a new capture.' : `Restored your unsaved draft from ${stamp.format(new Date(at))}.`);
  } else if (drafts.length) status(`Restored ${drafts.length} unsaved drafts.`);
  return ws.tabs().length > 0;
}

// ── Editor links ──
// A prefix resolves only when it names exactly one fragment; ULIDs made within ~256ms share 8 characters.
// Links resolve inside one vault: the tab's fragment's, the scope's, or the chosen save destination.
const inScope = p => !scope || scope === p;
const rowsIn = p => inScope(p) ? rows.filter(r => r.vault === p) : outside.get(p) || [];
const linkVault = d => d.opened?.vault ?? (scope || d.dest);
const linkable = d => { const v = linkVault(d); return v ? rowsIn(v) : rows; };
const shortId = (id, pool) => { let n = 8; while (n < id.length && pool.some(r => r.id !== id && r.id.startsWith(id.slice(0, n)))) n++; return id.slice(0, n); };
function linksFor(d) {
  const links = {
    find(prefix) { const hits = linkable(d).filter(r => r.id.startsWith(prefix)); return hits.length === 1 ? hits[0] : null; },
    resolve(prefix) { const r = links.find(prefix); return r ? { title: r.title, type: r.type } : null; },
    open(prefix, e) { const r = links.find(prefix); if (r) openRow(r, clickHow(e)); },
    attachment(filename) {
      if (!d.opened) return null;
      const list = d.attachments != null ? d.attachments.map(a => a.filename) : (d.opened.attachments || []);
      const exists = list.includes(filename);
      if (!exists) return { exists: false };
      const token = vaultToken(d.opened.vault);
      const imageUrl = IMAGE_EXT.test(filename) && token
        ? `parc-attach://${token}/${d.opened.id}/${encodeURIComponent(filename)}`
        : null;
      return { exists: true, imageUrl };
    },
    openAttachment(filename) {
      if (d.opened) api.openAttachment(d.opened.vault, d.opened.id, filename).catch(status);
    },
    pool: () => linkable(d).filter(r => !(d.opened && key(d.opened) === key(r))),
    // Unique where it will be resolved (the link scope, which may span vaults) and in its own vault.
    shortId: r => shortId(r.id, [...linkable(d), ...rowsIn(r.vault)]),
    describe: r => [typeLabel(r.type), ...(linkVault(d) ? [] : [name(r.vault)]), age(r.updated_at)]
  };
  return links;
}
// Tabs on a vault outside the scope keep its rows, so their links still resolve.
async function syncOutside() {
  const chats = viewTabs().filter(v => v.view === 'agent').flatMap(v => v.agent?.vaults() || []);
  const need = new Set([...docTabs().map(d => d.opened?.vault), ...chats].filter(p => p && !inScope(p) && vaults.some(v => v.path === p)));
  for (const p of outside.keys()) if (!need.has(p)) outside.delete(p);
  const missing = [...need].filter(p => !outside.has(p));
  if (!missing.length) return;
  await Promise.all(missing.map(async p => { try { outside.set(p, await listVault(p)); } catch { /* asked again next time */ } }));
  for (const d of docTabs()) d.editor.refresh();
  for (const v of viewTabs()) v.agent?.relink();
}

// ── Related ──
function recallSeed(d) {
  if (!ws.shown(d) || !vaults.length) return null;
  return {
    vaults: d.opened ? [d.opened.vault] : scope ? [scope] : vaults.map(v => v.path),
    vault: d.opened?.vault ?? null, id: d.opened?.id ?? null,
    text: `${d.ui.title.value}\n${d.editor.text()}`, tags: (d.opened ? d.opened.tags : d.fresh?.tags) || []
  };
}
let statsFrame = 0;
function changed(d) {
  d.dirty = true; d.draftAt = new Date().toISOString(); ws.keep(d);
  if (d === tab) paint(); else ws.update(d);
  rememberWorkspace(); d.related.changed();
}
function stats() {
  cancelAnimationFrame(statsFrame);
  statsFrame = requestAnimationFrame(() => {
    const d = cur();
    if (!d) { $('stats').textContent = ''; return; }
    const n = words(`${d.ui.title.value} ${d.editor.text()}`);
    $('stats').replaceChildren(el('span', '', `${n.toLocaleString()} ${n === 1 ? 'word' : 'words'}`));
    if (d.opened && !d.dirty) {
      // The last edit opens the fragment's history.
      const t = el('button', 'edited', edited(d.opened.updated_at)); t.type = 'button';
      t.title = `${stamp.format(new Date(d.opened.updated_at))}. Show history`;
      t.onclick = () => showHistory(d);
      $('stats').append(t);
    }
  });
}

// ── Phase & Dictation ──
// Phase is derived from independent states so background loads, metadata edits and board moves never lock the editor.
// Dictation writes at the caret of the fragment tab it started in; tabs don't switch while it records or transcribes.
let recordStart = 0, recordTimer = 0;
function derivePhase() {
  if (speechPhase !== 'idle') return speechPhase;
  if (restoring) return 'loading';
  if (cur()?.saving) return 'saving';
  if (pendingWork > 0) return 'loading';
  return 'idle';
}
function syncPhase() {
  phase = derivePhase();
  paint();
  if (phase === 'idle' && (changedVaults.size || changedSchemas.size)) setTimeout(flushChanges);
}
function setSpeechPhase(value) {
  if (value === 'recording' && speechPhase !== 'recording') {
    recordStart = Date.now(); clearInterval(recordTimer);
    recordTimer = setInterval(() => { const s = Math.floor((Date.now() - recordStart) / 1000); $('recTime').textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; }, 250);
    $('recTime').textContent = '0:00';
  } else if (value !== 'recording') clearInterval(recordTimer);
  speechPhase = value; syncPhase();
}
const voice = dictation(api, setSpeechPhase, status, text => { const d = cur(); if (!d) return; d.editor.insert(text); d.related.now(); });
async function checkSpeech() {
  try { speech = await api.speechStatus(); } catch (err) { speech = { ready: false, problem: cleanError(err) }; }
  languageSwitch(); paint();
}

// ── Dictation languages: whisper's codes, named in English ──
const displayNames = (() => { try { return new Intl.DisplayNames(['en'], { type: 'language' }); } catch { return null; } })();
const languageName = code => code === 'auto' ? 'Detect automatically' : code === 'jw' ? 'Javanese' : displayNames?.of(code) || code;
// Next to Dictate, with more than one preferred language: which one the next recording is in.
function languageSwitch() {
  const slot = $('languageSlot'), languages = speech.languages || [];
  slot.replaceChildren();
  if (languages.length < 2) return;
  const current = speech.language || 'auto';
  const dd = menu([el('span', 'sr', 'Dictation language: '), el('span', 'language-code', current === 'auto' ? 'Auto' : current.toUpperCase())],
    [...languages.map(code => [code, languageName(code), code === current]), null, ['auto', languageName('auto'), current === 'auto']],
    code => api.setSpeechLanguage(code).then(checkSpeech).catch(status), 'language-menu');
  dd.querySelector('aihio-button').title = `Dictation language: ${languageName(current)}`;
  slot.append(dd);
}

// ── Paint: the bar and window from (phase, the current tab, scope) ──
function paint() {
  const busy = blocking(), recording = phase === 'recording', d = cur(), capture = !!d, mode = modeOf(tab);
  document.body.dataset.phase = phase;
  $('main').setAttribute('aria-busy', String(['loading', 'saving', 'transcribing'].includes(phase)));
  $('scope').toggleAttribute('disabled', busy || !vaults.length);
  $('destination').disabled = busy;
  $('record').hidden = !capture; $('save').hidden = !capture;
  $('record').toggleAttribute('disabled', phase === 'saving'); // its click handler ignores every other non-idle phase
  $('record').toggleAttribute('loading', phase === 'transcribing' || phase === 'starting');
  const setup = !speech.ready && phase === 'idle';
  $('recordLabel').textContent = recording ? 'Stop & transcribe' : phase === 'transcribing' ? 'Transcribing' : setup ? 'Set up dictation' : 'Dictate';
  $('record').title = setup ? speech.problem || '' : speech.warning || '';
  $('languageSlot').hidden = !capture;
  $('languageSlot').querySelector('aihio-button')?.toggleAttribute('disabled', phase !== 'idle');
  $('record').setAttribute('variant', setup ? 'ghost' : 'outline');
  const glyph = $('recordIcon');
  if (glyph && glyph.dataset.name !== (recording ? 'stop' : 'mic')) {
    const next = icon(recording ? 'stop' : 'mic'); next.id = 'recordIcon'; next.dataset.name = recording ? 'stop' : 'mic'; glyph.replaceWith(next);
  }
  $('cancel').hidden = !recording; $('recTime').hidden = !recording;
  const unsavable = busy || !d?.dirty || !destination(d) || isEmpty(d);
  $('save').toggleAttribute('disabled', unsavable);
  $('save').setAttribute('variant', unsavable && phase !== 'saving' ? 'outline' : 'default'); // a disabled primary reads as a grey slab
  $('save').toggleAttribute('loading', phase === 'saving');
  $('fragmentMenu').hidden = !d?.opened;
  $('archiveLabel').textContent = d?.opened?.archived ? 'Unarchive' : 'Archive';
  $('destinationLabel').hidden = !d || !!scope || !!d.opened;
  if (d && !d.opened) $('destination').value = d.dest;
  $('destination').toggleAttribute('data-empty', !$('destination').value);
  const rev = $('revision');
  rev.textContent = phase === 'saving' ? 'Saving…' : d?.dirty ? 'Unsaved' : d?.opened ? 'Saved' : '';
  rev.dataset.state = phase === 'saving' ? 'saving' : d?.dirty ? 'dirty' : 'saved';
  // Beside the word count. "Saved" is left to the last edit's time there; the recording state speaks for the draft.
  rev.hidden = rev.dataset.state === 'saved' || ['recording', 'transcribing', 'starting', 'stopping'].includes(phase);
  for (const b of $('sidebar').querySelectorAll('[data-view]')) {
    b.toggleAttribute('disabled', busy);
    if (b.dataset.view === mode && !d?.opened) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
  }
  // The window's title names what is open, with a dot while it has unsaved changes.
  const here = d ? d.ui.title.value.trim() || (d.opened ? 'Untitled' : `New ${d.fresh?.type || 'note'}`) : tab ? viewLabel(tab) : '';
  document.title = here ? `${d?.dirty ? '• ' : ''}${here} — parc` : 'parc';
  if (tab) ws.update(tab);
  stats();
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
  const removable = scope && vaults.find(v => v.path === scope)?.removable;
  menu.append(
    ...vaults.map(v => item(v.path, v.name, vaultMark(v.path), v.path === scope)),
    item('@all', 'All opened vaults', vaultMark(''), scope === ''),
    sep(), ...suggested.map(s => item(`@suggest:${s.path}`, `Open ${s.name} vault`, icon('folder'), false, s.hint)),
    item('@open', 'Open vault folder…', icon('folder'), false),
    item('@create', 'New vault…', icon('folderPlus'), false),
    ...(removable ? [item('@close', `Remove ${name(scope)}…`, icon('x'), false)] : []),
    sep(),
    item('@settings', 'Vault settings…', icon('settings'), false),
    item('@doctor', 'Vault health', icon('shield'), false),
    item('@trash', 'Trash', icon('trash'), false));
  $('vaultName').textContent = scope ? name(scope) : 'All vaults';
  $('vaultMark').replaceChildren(vaultMark(scope));
  $('scope').title = scope || 'All opened vaults';
  const chosen = $('destination').value;
  $('destination').replaceChildren(new Option('Choose vault…', ''), ...vaults.map(v => new Option(v.name, v.path)));
  $('destination').value = chosen;
}
// Remove a vault from the opened list (files on disk stay untouched) and switch scope if it was active.
function closeVault(p = scope) {
  if (!p) return;
  return action(async () => {
    const res = await api.closeVault(p);
    if (!res?.closed) return;
    const oldName = name(p);
    vaults = res.vaults;
    schemas.delete(p); outside.delete(p);
    for (const d of docTabs()) if (d.opened?.vault === p && !d.dirty) ws.remove(d);
    for (const v of viewTabs()) if (v.view === 'history' && v.target?.vault === p) ws.remove(v);
    suggested = await api.suggestedVaults().catch(() => []);
    if (scope === p) scope = vaults[0]?.path || '';
    remember.set(SCOPE, scope);
    rows = await collect();
    options(); await refilter(); await syncOutside();
    rowsChanged(true);
    status(`Removed “${oldName}” from parc Capture. Its files on disk were not changed.`);
    paint();
  });
}
// The scope is what the list and the views show. Open tabs stay: fragments keep their own vault, and views redraw for
// the new scope. A fragment from a vault the new scope leaves out gives way to the new vault's Pulse.
function switchScope(target) {
  const seq = ++scopeSeq;
  return action(async () => {
    let next = target === '@all' ? '' : target;
    if (target === '@open' || target.startsWith('@suggest:') || target === '@create') {
      const picked = target === '@create' ? await api.createVault()
        : await api.openVault(target === '@open' ? undefined : target.slice(9));
      if (!picked || seq !== scopeSeq) return;
      vaults = picked.vaults; next = picked.path;
      await loadSchemas([next]); suggested = await api.suggestedVaults().catch(() => []);
    }
    if (seq !== scopeSeq) return;
    const old = scope; scope = next;
    try { rows = await collect(); } catch (e) { if (seq === scopeSeq) scope = old; throw e; }
    if (seq !== scopeSeq) return;
    await loadSchemas((scope ? [scope] : vaults.map(v => v.path)).filter(p => !schemas.has(p)));
    if (seq !== scopeSeq) return;
    remember.set(SCOPE, scope);
    for (const d of docTabs()) {
      if (!d.opened && !d.dirty && !enabledTypes(destination(d)).some(s => s.name === d.fresh.type)) d.fresh = { ...blankCapture(d), tags: d.fresh.tags };
      meta(d);
    }
    options(); await refilter(); await syncOutside();
    rowsChanged(true);
    if (cur()?.opened && !inScope(cur().opened.vault)) await openView('pulse');
    status(); paint();
  });
}

// ── Fragment rows ──
const listVault = async (p, query = '') => (await api.list(p, query)).map(r => ({ ...r, vault: p }));
async function collect(query = '') {
  const paths = scope ? [scope] : vaults.map(v => v.path);
  const groups = await Promise.all(paths.map(p => listVault(p, query)));
  return groups.flat().sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.title.localeCompare(b.title));
}
// Refresh tag datalist and open editors' [[link]] decorations only when the vault rows actually changed.
function rowsSynced() {
  $('tagOptions').replaceChildren(...allTags().map(t => new Option(t)));
  // A new fragment can resolve a link, or lengthen the prefix that names another.
  for (const d of docTabs()) {
    d.editor.refresh();
    const fid = d.opened && d.ui.meta.querySelector('.fid'), short = fid && shortId(d.opened.id, linkable(d));
    if (fid && fid.textContent !== short) { fid.textContent = short; fid.title = `Link to this fragment with [[${short}]]`; }
  }
}
// The list changed: the sidebar, link decorations and the views that show it follow.
function rowsChanged(all = false) { rowsSynced(); list(); refreshViews(all); }
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
// How a click asks to open a fragment: Ctrl (⌘) opens it beside, in the next group; the middle button in a tab of
// its own; a keyboard click puts the caret in its text. Otherwise it opens as the group's preview.
function clickHow(e) {
  if (!e || typeof e !== 'object') return { focus: e === true };
  return { focus: e.detail === 0, side: !!(e.ctrlKey || e.metaKey), tab: e.button === 1 };
}
function openRow(r, how = {}) {
  return action(() => openFragment(r, how)).then(() => { const d = tabOf(r); if (how.focus && d && d === tab) d.editor.focus(); });
}
// A fragment row opens on click, stays open on a double-click (its tab stops being the preview), opens in a tab of
// its own on a middle click, and drags into a group (or beside one) to open there.
function rowEvents(b, r) {
  b.onclick = e => openRow(r, clickHow(e));
  b.ondblclick = () => { const d = tabOf(r); if (d) ws.keep(d); else keepNext = key(r); };
  b.onmousedown = e => { if (e.button === 1) e.preventDefault(); };
  b.onauxclick = e => { if (e.button === 1) { e.preventDefault(); openRow(r, { tab: true }); } };
  b.draggable = true;
  b.ondragstart = e => {
    e.dataTransfer.effectAllowed = 'copyMove';
    e.dataTransfer.setData(FRAGMENT, JSON.stringify({ vault: r.vault, id: r.id }));
    e.dataTransfer.setData('text/plain', r.title || 'Untitled');
  };
}
const isOpenHere = r => { const d = cur(); return !!d?.opened && key(d.opened) === key(r); };
// A fragment row in results: type glyph, title and age, then type, state and tags.
const createdAt = r => r.created_at || r.updated_at;
function timeOf(r, created) { // the last edit, or the creation when the list is in creation order
  const at = created ? createdAt(r) : r.updated_at, time = el('time', '', age(at));
  time.dateTime = at; time.title = `${created ? 'Created' : 'Edited'} ${stamp.format(new Date(at))}`;
  return time;
}
function row(r, { withSnippet = false } = {}) {
  const b = el('button', 'row'); b.type = 'button';
  const meta = el('span', 'row-meta'), facts = el('span', 'row-facts'), main = el('span', 'row-main');
  if (!scope) facts.append(el('span', 'where', name(r.vault)));
  facts.append(el('span', '', typeLabel(r.type)));
  if (r.status) facts.append(state(r.vault, r.type, r.status));
  if (r.archived) facts.append(el('span', 'archived', 'Archived'));
  if (r.tags?.length) facts.append(el('span', 'tags', r.tags.map(t => `#${t}`).join(' ')));
  meta.append(facts, timeOf(r));
  main.append(el('span', 'row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'), meta);
  b.append(typeIcon(r.type), main);
  if (withSnippet && r.snippet) b.append(snippet(r.snippet));
  if (r.archived) b.dataset.archived = '';
  if (isOpenHere(r)) b.setAttribute('aria-current', 'page');
  rowEvents(b, r);
  return b;
}
function rowList(items, options) {
  const ul = el('ul', 'results');
  ul.append(...items.map(r => { const li = el('li'); li.append(row(r, options)); return li; }));
  return ul;
}
// A sidebar row: one line, so a screen holds a screenful. Its leading mark says what the group header doesn't:
// grouped by type it is the status (its category: open, active, done or closed), otherwise the type's glyph.
// Archived rows recede. The words behind the marks are in its tooltip and accessible name.
function sideRow(r, { created = false, mark = 'type' } = {}) {
  const b = el('button', 'row side-row'); b.type = 'button';
  b.dataset.key = key(r);
  const category = r.status ? tone(r.vault, r.type, r.status) : '';
  const lead = mark === 'type' ? typeIcon(r.type, 'icon lead') : r.status ? statusIcon(category, 'icon lead status') : el('span', 'lead');
  const facts = [mark === 'type' && typeLabel(r.type), r.status && capital(r.status), r.archived && 'Archived', !scope && name(r.vault)].filter(Boolean);
  const end = el('span', 'row-end');
  if (facts.length) end.append(el('span', 'sr', `, ${facts.join(', ')}, `));
  if (r.archived) end.append(icon('archive', 'icon archived'));
  if (!scope) { const dot = el('span', 'vault-dot'); dot.style.setProperty('--vault-hue', hue(r.vault)); end.append(dot); }
  end.append(timeOf(r, created));
  b.append(lead, el('span', 'row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'), end);
  b.title = [r.title || 'Untitled', [...facts, ...(r.tags || []).map(t => `#${t}`)].join(' · ')].filter(Boolean).join('\n');
  if (r.archived) b.dataset.archived = '';
  if (isOpenHere(r)) b.setAttribute('aria-current', 'page');
  else if (tabOf(r)) b.dataset.open = ''; // open in another tab
  rowEvents(b, r);
  return b;
}
const sideItem = (r, options) => { const li = el('li'); li.append(sideRow(r, options)); return li; };
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

// ── Sidebar grouping and order ──
// Grouped by type (the default), by tag, or not at all. By tag, a fragment with several tags is listed under each,
// and untagged ones under "No tags". Groups run alphabetically, so they stay put while their fragments change; the
// order applies inside each. Dates sort newest first by default, titles A to Z. A folded group keeps its header and count.
const GROUPS = { none: 'No grouping', type: 'Group by type', tag: 'Group by tag' };
const SORTS = { updated: 'Sort by last edit', created: 'Sort by creation date', title: 'Sort by title' };
const DEFAULT_VIEW = { group: 'type', sort: 'updated', dir: 'desc' };
const naturalDir = sort => sort === 'title' ? 'asc' : 'desc';
const dirLabel = (sort, dir) => sort === 'title' ? (dir === 'asc' ? 'A to Z' : 'Z to A') : (dir === 'desc' ? 'Newest first' : 'Oldest first');
const listView = (() => {
  const saved = remember.get(LIST_VIEW) || {}, own = (map, k) => typeof k === 'string' && Object.hasOwn(map, k);
  const sort = own(SORTS, saved.sort) ? saved.sort : DEFAULT_VIEW.sort;
  return { group: own(GROUPS, saved.group) ? saved.group : DEFAULT_VIEW.group, sort, dir: ['asc', 'desc'].includes(saved.dir) ? saved.dir : naturalDir(sort) };
})();
const folded = new Set([remember.get(FOLDED)].flat().filter(k => typeof k === 'string'));
const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });
function setListView(change) {
  if (change.sort && change.sort !== listView.sort && !change.dir) change = { ...change, dir: naturalDir(change.sort) };
  Object.assign(listView, change);
  remember.set(LIST_VIEW, listView); viewMenu(); list();
}
function ordered(items) {
  const { sort, dir } = listView, sign = dir === 'asc' ? 1 : -1, when = sort === 'created' ? createdAt : r => r.updated_at;
  const keyed = items.map(r => ({ r, t: Date.parse(when(r)) || 0 }));
  // created_at is kept to the second; IDs (ULIDs) keep the millisecond, so captures made within a second stay in order.
  const byId = (a, b) => sort === 'created' ? sign * (a.r.id < b.r.id ? -1 : a.r.id > b.r.id ? 1 : 0) : 0;
  keyed.sort(sort === 'title'
    ? (a, b) => (!a.r.title - !b.r.title) || sign * collator.compare(a.r.title, b.r.title) || b.t - a.t // untitled last
    : (a, b) => sign * (a.t - b.t) || byId(a, b) || collator.compare(a.r.title || '', b.r.title || ''));
  return keyed.map(k => k.r);
}
function grouped(items) {
  const groups = new Map();
  const add = (id, label, glyph, r) => { if (!groups.has(id)) groups.set(id, { id, label, glyph, items: [] }); groups.get(id).items.push(r); };
  for (const r of items) {
    if (listView.group === 'type') { add(`type:${r.type}`, typeLabel(r.type), () => typeIcon(r.type, 'icon glyph'), r); continue; }
    const tags = new Map((r.tags || []).map(t => [t.toLowerCase(), t])); // tags are case-insensitive
    if (!tags.size) add('tag:', 'No tags', () => el('span', 'glyph'), r);
    for (const [low, t] of tags) add(`tag:${low}`, t, () => icon('hash', 'icon glyph'), r);
  }
  return [...groups.values()].sort((a, b) => (a.id === 'tag:') - (b.id === 'tag:') || collator.compare(a.label, b.label));
}
// A group's header: its glyph over the rows' marks, its name over their titles, and a fold mark at the end
// that shows while folded or pointed at. It sticks to the top while its rows scroll under it. A type's header
// opens that type's own view (its board, matrix or cards), so its fold mark is a button of its own; a tag's
// header folds. ← and → fold either from the keyboard.
function groupShell(g, open) {
  const li = el('li', 'group'), bar = el('div', 'group-bar'), head = el('button', 'group-head');
  const label = g.id.startsWith('tag:') && g.id !== 'tag:' ? `#${g.label}` : g.label, type = g.id.startsWith('type:') ? g.id.slice(5) : null;
  const fold = el('button', 'group-fold'); fold.type = 'button'; fold.tabIndex = -1;
  fold.setAttribute('aria-expanded', String(open)); fold.setAttribute('aria-label', `${open ? 'Fold' : 'Unfold'} ${label}`); fold.title = fold.getAttribute('aria-label');
  fold.append(icon('chevron', 'icon fold')); fold.onclick = () => foldGroup(g.id);
  li.dataset.group = g.id;
  head.type = 'button';
  head.append(g.glyph(), el('span', 'group-name', g.label), el('span', 'group-count', g.items.length.toLocaleString()));
  if (type) {
    head.dataset.type = type;
    head.title = `Open ${plural(type)}`;
    head.onclick = e => openTypeView(type, clickHow(e));
    if (tab?.kind === 'view' && tab.view === 'type' && tab.viewType === type) head.setAttribute('aria-current', 'page');
  } else { head.setAttribute('aria-expanded', String(open)); head.onclick = () => foldGroup(g.id); }
  bar.dataset.open = String(open);
  bar.append(head, fold); li.append(bar);
  let rowsEl = null;
  if (open) { rowsEl = el('ul', 'group-rows'); rowsEl.setAttribute('aria-label', label); li.append(rowsEl); }
  return { li, head, rowsEl };
}
function groupItem(g, options) {
  const { li, rowsEl } = groupShell(g, !folded.has(g.id));
  if (rowsEl) rowsEl.append(...g.items.map(r => sideItem(r, options)));
  return li;
}
function foldGroup(id, fold = !folded.has(id)) {
  if (fold) folded.add(id); else folded.delete(id);
  remember.set(FOLDED, [...folded]); list();
  $('recent').querySelector(`[data-group="${CSS.escape(id)}"] .group-head`)?.focus();
}
// The menu beside Open. Its button is marked while the list isn't in its default grouping and order.
function viewMenu() {
  const menu = $('listView'), { group, sort, dir } = listView;
  menu.querySelectorAll('aihio-dropdown-item, aihio-dropdown-separator').forEach(n => n.remove());
  const item = (value, label, current) => {
    const it = document.createElement('aihio-dropdown-item');
    it.setAttribute('value', value); it.append(el('span', 'item-name', label));
    if (current) it.append(icon('check', 'icon current'), el('span', 'sr', '(current)'));
    return it;
  };
  const sep = () => document.createElement('aihio-dropdown-separator');
  const dirs = sort === 'title' ? ['asc', 'desc'] : ['desc', 'asc'];
  menu.append(...Object.entries(GROUPS).map(([k, label]) => item(`group:${k}`, label, k === group)), sep(),
    ...Object.entries(SORTS).map(([k, label]) => item(`sort:${k}`, label, k === sort)), sep(),
    ...dirs.map(d => item(`dir:${d}`, dirLabel(sort, d), d === dir)));
  menu.toggleAttribute('data-active', Object.entries(DEFAULT_VIEW).some(([k, v]) => listView[k] !== v));
  const how = [group === 'none' ? 'not grouped' : `by ${group}`, SORTS[sort].replace(/^Sort /, ''), dirLabel(sort, dir).toLowerCase()].filter(Boolean).join(', ');
  menu.querySelector('aihio-button').title = `Group and sort the list (now ${how})`;
}

const sidebar = sidebarList($('recent'), {
  key, item: sideItem, groupItem, groupShell,
  isFolded: id => folded.has(id),
  currentKey: () => cur()?.opened ? key(cur().opened) : null,
  currentType: () => tab?.kind === 'view' && tab.view === 'type' ? tab.viewType : null,
  isOpenTab: k => docTabs().some(d => d.opened && key(d.opened) === k)
});
function list() {
  const shown = ordered(visible()), narrowed = !!filterText.trim() || openOnly;
  const opts = { created: listView.sort === 'created', mark: listView.group === 'type' ? 'status' : 'type' };
  const isGrouped = listView.group !== 'none';
  sidebar.render({ grouped: isGrouped, items: shown, groups: isGrouped ? grouped(shown) : [], opts });
  $('listHeading').textContent = { none: 'Fragments', type: 'Fragments by type', tag: 'Fragments by tag' }[listView.group];
  // The count sits at the end of the filter box; the box's text stops short of it.
  const count = !rows.length ? '' : narrowed ? `${shown.length.toLocaleString()} of ${rows.length.toLocaleString()}` : rows.length.toLocaleString();
  $('count').textContent = count;
  $('filterBox').style.setProperty('--count-ch', count.length);
  $('empty').textContent = !rows.length
    ? suggested.length && scope === vaults[0]?.path
      ? `No fragments yet. Press + to capture, or open your ${suggested[0].name} vault (${suggested[0].hint}) from the vault menu.`
      : 'No fragments yet. Press + to capture your first.'
    : filterError ? `This filter can’t be read: ${filterError}.`
      : openOnly && !filterText.trim() ? 'Nothing open: everything here is done, closed or archived.' : 'No fragments match.';
  $('empty').dataset.tone = filterError ? 'error' : '';
  $('empty').hidden = !!shown.length;
  $('openOnly').setAttribute('aria-pressed', String(openOnly));
}

// ── Sidebar shape: drag its edge to resize, Ctrl+\ to hide ──
const side = { width: null, collapsed: false, ...(remember.get(SIDE) || {}) };
function applySide() {
  document.body.toggleAttribute('data-collapsed', side.collapsed);
  if (side.width) document.documentElement.style.setProperty('--side-width', `${side.width}px`);
  else document.documentElement.style.removeProperty('--side-width');
  // Hide sits in the sidebar's head; Show takes its place at the start of the tabs while the sidebar is away.
  $('sidebarShow').hidden = !side.collapsed;
  for (const b of [$('sidebarHide'), $('sidebarShow')]) b.setAttribute('aria-expanded', String(!side.collapsed));
  $('resizer').setAttribute('aria-valuenow', String(Math.round(side.width || $('sidebar').getBoundingClientRect().width)));
}
function toggleSidebar() {
  side.collapsed = !side.collapsed;
  const from = document.activeElement;
  applySide(); remember.set(SIDE, side);
  if (side.collapsed && $('sidebar').contains(from)) $('sidebarShow').focus();
  else if (!side.collapsed && from?.closest?.('#sidebarShow')) $('sidebarHide').focus();
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
function describe(d) {
  if (d.opened) return { d, isNew: false, vault: d.opened.vault, type: d.opened.type, schema: schemaFor(d.opened.vault, d.opened.type), values: d.opened, tags: d.opened.tags || [] };
  const vault = scope || d.dest, schema = schemaFor(vault, d.fresh.type), defaults = {};
  for (const f of schema?.fields || []) if (f.default != null) defaults[f.name] = f.default;
  return { d, isNew: true, vault, type: d.fresh.type, schema, values: { ...defaults, ...d.fresh.fields }, tags: d.fresh.tags };
}
const editable = schema => (schema?.fields || []).filter(f => (f.kind === 'enum' && f.values?.length) || f.kind === 'date' || f.kind === 'string' || f.kind === 'list');
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
  const types = [...enabledTypes(c.vault)];
  if (!types.some(s => s.name === c.type)) types.unshift({ name: c.type });
  return menu([el('span', 'sr', 'Type: '), typeIcon(c.type), el('span', '', c.isNew ? `New ${c.type}` : typeLabel(c.type))],
    types.map(s => [s.name, typeLabel(s.name), s.name === c.type, typeIcon(s.name)]),
    type => c.isNew ? setType(c.d, type) : promoteFragment(c.d, type),
    c.isNew ? 'prop type-menu' : 'prop type-menu saved-type');
}
function setType(d, type) {
  if (d.opened || type === d.fresh.type) return;
  d.fresh = { type, fields: {}, tags: d.fresh.tags }; applyTemplate(d); meta(d); paint(); ws.update(d); rememberWorkspace();
}
function promoteFragment(d, newType) {
  const f = d?.opened;
  if (!f || f.type === newType) return;
  return applyChange(d, rev => api.promote(f.vault, f.id, newType, rev),
    `Converted to ${typeLabel(newType)}. Earlier version kept in History.`);
}
function textControl(field, c) {
  const raw = c.values[field.name], value = typeof raw === 'string' && raw.trim() ? raw.trim() : '';
  const wrap = el('span', 'prop-text'), btn = el('button', 'prop-button');
  const label = capital(field.name);
  wrap.dataset.field = field.name;
  btn.type = 'button'; btn.dataset.field = field.name;
  btn.title = value ? `${label}: ${value}. Click to edit` : `Set ${label.toLowerCase()}`;
  btn.append(icon('text'), el('span', value ? '' : 'unset', value ? `${label}: ${value}` : label));
  btn.onclick = () => {
    const input = el('input', 'prop-input'); let finished = false;
    Object.assign(input, { type: 'text', value, placeholder: label, autocomplete: 'off' });
    input.setAttribute('aria-label', label);
    const finish = keep => {
      if (finished) return; finished = true;
      const next = input.value.trim();
      if (keep && next !== value) setField(c.d, field.name, next || null);
      else meta(c.d);
    };
    input.onkeydown = e => {
      if (e.key === 'Enter') { e.preventDefault(); finish(true); }
      else if (e.key === 'Escape') { e.preventDefault(); finish(false); }
    };
    input.onblur = () => finish(true);
    wrap.replaceChildren(input); input.focus(); input.select();
  };
  wrap.append(btn);
  if (value && !field.required) {
    const clear = el('button', 'prop-clear'); clear.type = 'button';
    clear.title = `Clear ${label.toLowerCase()}`; clear.setAttribute('aria-label', clear.title);
    clear.append(icon('x')); clear.onclick = () => setField(c.d, field.name, null);
    wrap.append(clear);
  }
  return wrap;
}
function listControl(field, c) {
  const items = Array.isArray(c.values[field.name]) ? c.values[field.name].filter(s => typeof s === 'string' && s.trim()) : [];
  const label = capital(field.name), box = el('span', 'prop-list');
  box.dataset.field = field.name;
  for (const item of items) {
    const chip = el('span', 'tag list-chip'), remove = el('button', 'tag-x');
    remove.type = 'button'; remove.title = `Remove ${item} from ${label.toLowerCase()}`; remove.setAttribute('aria-label', remove.title);
    remove.append(icon('x'));
    remove.onclick = () => {
      const next = items.filter(x => x !== item);
      setField(c.d, field.name, next.length ? next : (field.required ? [] : null));
    };
    chip.append(item, remove);
    box.append(chip);
  }
  const add = el('button', 'prop-button add-list'); add.type = 'button'; add.title = `Add to ${label.toLowerCase()}`;
  add.append(icon('list'), el('span', items.length ? 'sr' : 'unset', items.length ? `Add ${label.toLowerCase()}` : label));
  add.onclick = () => {
    const input = el('input', 'tag-input'); let finished = false;
    Object.assign(input, { type: 'text', placeholder: label.toLowerCase(), autocomplete: 'off' });
    input.setAttribute('aria-label', `Add to ${label.toLowerCase()}`);
    const finish = keep => {
      if (finished) return; finished = true;
      const val = input.value.trim();
      if (keep && val && !items.some(x => x.toLowerCase() === val.toLowerCase())) setField(c.d, field.name, [...items, val]);
      else meta(c.d);
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
function enumControl(field, c) {
  const value = c.values[field.name] ?? null, toggle = c.schema?.toggle?.field === field.name ? c.schema.toggle : null;
  const shown = field.name === 'status' ? capital(value) : `${capital(value)} ${field.name.replace(/[-_]/g, ' ')}`;
  const trigger = [el('span', 'sr', `${capital(field.name)}: `)];
  if (field.name === 'status' && !toggle) { const dot = el('span', 'state'); dot.dataset.tone = tone(c.vault, c.type, value); trigger.push(dot); }
  if (field.name === 'priority') trigger.push(icon('flag'));
  trigger.push(el('span', value ? '' : 'unset', value ? shown : capital(field.name)), ...(value ? [] : [el('span', 'sr', ' not set')]));
  const items = field.values.map(v => [v, capital(v), v === value]);
  if (!field.required && value) items.push(null, ['', 'Clear']);
  const dd = menu(trigger, items, v => setField(c.d, field.name, v === '' ? null : v), 'prop');
  dd.dataset.field = field.name;
  if (field.name === 'priority' && ['high', 'critical'].includes(value)) dd.dataset.emphasis = '';
  if (!toggle) return dd;
  // A one-click toggle (the schema's `toggle:`, the TUI's t) on the status mark.
  const next = value === toggle.on ? toggle.off : toggle.on, mark = el('button', 'toggle-mark');
  mark.type = 'button'; mark.dataset.tone = tone(c.vault, c.type, value);
  mark.title = `Mark ${capital(next).toLowerCase()} (T)`; mark.setAttribute('aria-label', `Mark ${next}`);
  mark.onclick = () => setField(c.d, toggle.field, next);
  const group = el('span', 'prop-group'); group.append(mark, dd);
  return group;
}
function dateControl(field, c) {
  const value = c.values[field.name] ?? null, wrap = el('span', 'prop-date'), btn = el('button', 'prop-button');
  const late = field.name === 'due' && overdue({ vault: c.vault, type: c.type, status: c.values.status, due: value });
  btn.type = 'button'; btn.dataset.field = field.name;
  btn.append(icon('calendar'), el('span', value ? '' : 'unset', value ? `${capital(field.name)} ${dayLabel(value)}${late ? ', overdue' : ''}` : `${capital(field.name)} date`));
  if (late) btn.dataset.overdue = '';
  btn.onclick = () => {
    const picker = document.createElement('aihio-date-picker');
    picker.setAttribute('size', 'sm'); picker.setAttribute('aria-label', `${capital(field.name)} date`);
    if (value) picker.setAttribute('value', value);
    let settledPick = false;
    const done = next => { if (settledPick) return; settledPick = true; if (next !== undefined && next !== value) setField(c.d, field.name, next); else meta(c.d); };
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
    clear.append(icon('x')); clear.onclick = () => setField(c.d, field.name, null);
    wrap.append(clear);
  }
  return wrap;
}
function tagEditor(c) {
  const box = el('span', 'tag-edit');
  for (const t of c.tags) {
    const chip = el('span', 'tag'), remove = el('button', 'tag-x');
    remove.type = 'button'; remove.title = `Remove #${t}`; remove.setAttribute('aria-label', remove.title);
    remove.append(icon('x')); remove.onclick = () => setTags(c.d, c.tags.filter(x => x !== t));
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
      if (keep && tag && !c.tags.some(t => t.toLowerCase() === tag.toLowerCase())) setTags(c.d, [...c.tags, tag]); else meta(c.d);
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
function meta(d) {
  const box = d.ui.meta, c = describe(d);
  box.replaceChildren();
  box.append(typeMenu(c));
  if (!c.isNew && d.opened.archived) { const a = document.createElement('aihio-badge'); a.setAttribute('variant', 'secondary'); a.append(icon('archive'), 'Archived'); box.append(a); }
  for (const f of editable(c.schema)) {
    box.append(f.kind === 'date' ? dateControl(f, c) : f.kind === 'string' ? textControl(f, c) : f.kind === 'list' ? listControl(f, c) : enumControl(f, c));
  }
  if (!c.schema && c.values.status) box.append(state(c.vault, c.type, c.values.status)); // a type without a schema here: shown, not edited
  box.append(tagEditor(c));
  if (!c.isNew) {
    const short = shortId(d.opened.id, linkable(d)), id = el('span', 'fid', short); id.title = `Link to this fragment with [[${short}]]`;
    if (vaults.length > 1) {
      const where = el('span', 'meta-vault'), dot = el('span', 'vault-dot');
      dot.style.setProperty('--vault-hue', hue(d.opened.vault));
      where.append(dot, el('span', '', name(d.opened.vault)));
      where.title = `In the vault ${name(d.opened.vault)}`;
      box.append(where);
    }
    box.append(id);
  }
}
function setField(d, field, value) {
  if (!d.opened) { d.fresh.fields = { ...d.fresh.fields, [field]: value }; meta(d); rememberWorkspace(); return; }
  const f = d.opened, def = schemaFor(f.vault, f.type)?.fields.find(x => x.name === field);
  const shown = value == null ? ''
    : Array.isArray(value) ? value.join(', ')
    : /^\d{4}-\d{2}-\d{2}$/.test(value) ? dayLabel(value)
    : def?.kind === 'string' ? String(value).slice(0, 60)
    : capital(value).toLowerCase();
  return applyChange(d, rev => api.update(f.vault, f.id, { fields: { [field]: value } }, rev),
    value == null || (Array.isArray(value) && !value.length) ? `${capital(field)} cleared.` : `${capital(field)} set to ${shown}.`);
}
function setTags(d, tags) {
  if (!d.opened) { d.fresh.tags = tags; meta(d); rememberWorkspace(); d.related.changed(); return; }
  const f = d.opened;
  return applyChange(d, rev => api.update(f.vault, f.id, { tags }, rev), 'Tags saved.');
}
function toggleField(d = cur()) {
  if (!d) return;
  const c = describe(d), t = c.schema?.toggle;
  if (!t) return;
  setField(d, t.field, c.values[t.field] === t.on ? t.off : t.on);
}
function toggleArchive(d = cur()) {
  const f = d?.opened;
  if (!f) return;
  const archived = !f.archived;
  return applyChange(d, rev => api.archive(f.vault, f.id, archived, rev),
    archived ? 'Archived: it leaves default searches; find it with is:archived.' : 'Unarchived.',
    { label: 'Undo', run: () => { const t = tabOf(f); if (t) toggleArchive(t); } });
}
// A field-level save names the revision it was based on. If the file moved on and the text is clean, reload
// and apply the change to the newer file. If the text has edits, apply the change (it touches nothing else)
// but keep the text's base, so saving the text still asks before overwriting.
function applyChange(d, send, done, undo) {
  const run = () => action(async () => {
    const f = d.opened;
    if (!f) return;
    let note = await send(f.revision), base = null;
    if (note.conflict && !d.dirty) { await reload(d); note = await send(d.opened.revision); }
    else if (note.conflict) { base = f.revision; note = await send(null); }
    if (note.conflict) throw new Error('This fragment keeps changing on disk. Try again in a moment.');
    d.opened = { ...note, vault: f.vault, ...(base ? { revision: base } : {}) };
    meta(d); paint(); ws.update(d); d.related.changed();
    status(note.warnings?.join('; ') || (base ? `${done} It also changed outside parc: saving your text will ask before overwriting.` : done), undo);
    rows = await collect(); await refilter(); rowsChanged();
  });
  d.metaQueue = (d.metaQueue || Promise.resolve()).then(run, run);
  return d.metaQueue;
}

// ── Document ──
function load(d, note) { // puts a fragment, or a blank capture, in a tab's editor
  if (!note || note.id !== d.opened?.id) {
    d.ui.docGraph.hidden = true;
    d.ui.docAttachments.hidden = true;
    d.attachments = null;
  }
  d.ui.document.toggleAttribute('data-saved', !!note);
  d.opened = note; d.dirty = false; d.draftAt = null; d.dest = ''; d.templateBody = '';
  d.fresh = note ? null : blankCapture(d); d.ui.title.value = note?.title || '';
  d.editor.load((note?.body || '').replace(/^\n/, ''));
  if (!note) applyTemplate(d);
  meta(d); ws.update(d); rememberWorkspace();
  d.related.now();
}
async function reload(d) { const f = d.opened, full = await api.note(f.vault, f.id); load(d, { ...full, vault: f.vault }); paint(); list(); }
// Asks before dropping a tab's unsaved work, showing it first. Resolves true when it is fine to let it go.
async function canLeave(d) {
  if (!d.dirty) return true;
  if (d !== tab && d.group) ws.activate(d);
  const choice = await api.discard({ title: d.ui.title.value, canSave: !!destination(d) && !isEmpty(d) });
  if (choice === 'save') return commit(d);
  if (choice === 'discard') { d.dirty = false; d.draftAt = null; ws.update(d); rememberWorkspace(true); return true; }
  return false;
}
// Track async workspace work without locking editors or swallowing concurrent clicks.
// Only active dictation holds the tab until it finishes.
async function action(fn) {
  if (speechPhase !== 'idle') return;
  pendingWork++;
  syncPhase();
  try { return await fn(); } catch (err) { status(err); }
  finally { pendingWork--; syncPhase(); }
}
// Where a tab opens: beside (`side`: the next group, opened if there is none), where it was dropped (`group`,
// `index`), or in the current group, as its preview unless it should stay (`keep`, or a tab of its own: `tab`).
function placement(how = {}) {
  let group = how.group ?? ws.group ?? undefined;
  if (how.side && group?.tabs.length) {
    const i = ws.groups.indexOf(group);
    group = ws.groups[i + 1] ?? ws.split(group, 'right') ?? ws.groups[i - 1] ?? group;
  }
  return { group, index: how.index, preview: !(how.keep || how.tab || how.side || how.group), focus: !!how.focus };
}
// Opens a fragment in a tab, or shows the tab it is already open in (moving it where it was asked to go).
async function openFragment(r, how = {}) {
  const explicit = !!(how.group || how.side || how.tab);
  const seq = explicit ? navSeq : ++navSeq;
  const here = tabOf(r), k = key(r);
  if (here) {
    if (how.keep || how.tab) ws.keep(here);
    if (how.group && here.group !== how.group) ws.move(here, how.group, how.index);
    else if (how.side && here.group === ws.group && here.group.tabs.length > 1) ws.toNeighbour(here, 1);
    else ws.activate(here);
    return here;
  }
  const full = await api.note(r.vault, r.id);
  if (!explicit && seq !== navSeq) return null; // a newer click overtook this load
  const existing = tabOf(r);
  if (existing) { ws.activate(existing); return existing; }
  const d = docTab({ ...full, vault: r.vault });
  ws.open(d, placement(how));
  if (keepNext === k) ws.keep(d);
  keepNext = null;
  status();
  if (!inScope(r.vault)) await syncOutside();
  await Promise.all([loadNeighbors(d), loadAttachments(d)]);
  return d;
}
// Saves a tab's text; resolves true when it did. Locks only this tab while the write RPC itself runs,
// unlocking immediately once saved so typing never waits on the sidebar/graph refresh.
async function commit(d) {
  if (d.saving) return false;
  d.saving = true; savingDocs++;
  d.ui.title.readOnly = true; d.editor.lock(true);
  syncPhase();
  let savedNote = null;
  try {
    for (let force = false; ; force = true) {
      const p = destination(d), f = d.opened, title = d.ui.title.value, body = d.editor.text();
      const note = await api.save(p, f
        ? { id: f.id, title, body, expected: force ? null : f.revision }
        : { title, body, type: d.fresh.type, fields: d.fresh.fields, tags: d.fresh.tags });
      if (note.conflict) {
        const choice = await api.conflict({ title: title || f.title });
        if (choice === 'overwrite') continue;
        if (choice === 'reload') { await reload(d); await Promise.all([loadNeighbors(d), loadAttachments(d)]); status('Reloaded the version saved outside parc.'); }
        else status('Not saved: this fragment changed outside parc. Save again to choose.');
        return false;
      }
      savedNote = note;
      load(d, { ...note, vault: p }); d.ui.title.value = note.title; ws.update(d);
      status(note.warnings?.join('; ') || `Saved to ${name(p)}.`);
      break;
    }
  } finally {
    d.saving = false; savingDocs--;
    d.ui.title.readOnly = false; d.editor.lock(false);
    syncPhase();
  }
  if (!savedNote) return false;
  rows = await collect(); await refilter(); rowsChanged(); await Promise.all([loadNeighbors(d), loadAttachments(d)]);
  return true;
}
async function save() {
  const d = cur();
  if (!d || d.saving || $('save').hasAttribute('disabled')) return;
  await action(() => commit(d));
}
// A new capture opens as the current group's preview, or reuses a blank one there. `fields`: preset values,
// e.g. the status of the board column it was started from.
function newCapture(type, fields) {
  return action(async () => {
    const blank = t => t?.kind === 'doc' && !t.opened && !t.dirty && isEmpty(t);
    let d = blank(tab) ? tab : ws.group?.tabs.find(blank);
    if (d) { ws.activate(d); d.fresh = blankCapture(d); }
    else { d = docTab(null); ws.open(d, { preview: true }); }
    if (type) d.fresh.type = type;
    if (fields && Object.keys(fields).length) d.fresh.fields = { ...fields };
    applyTemplate(d); meta(d); paint(); status();
  }).then(() => { const d = cur(); if (d && !d.opened) d.ui.title.focus(); });
}
function copyLink(d = cur()) {
  if (!d?.opened) return;
  const link = `[[${shortId(d.opened.id, rowsIn(d.opened.vault))}]]`;
  api.copyText(link).then(() => status(`Copied ${link}. Paste it into another fragment to link the two.`), status);
}
// Moving to the trash asks first and closes the fragment's tab, then offers Undo until the next message.
function trashOpened(d = cur()) {
  const f = d?.opened;
  if (!f) return;
  return action(async () => {
    if (!await canLeave(d)) return;
    const { deleted } = await api.remove(f.vault, f.id, f.title);
    if (!deleted) return;
    for (const v of viewTabs()) if (v.view === 'history' && key(v.target) === key(f)) ws.remove(v); // History goes with it
    ws.remove(d);
    rows = await collect(); await refilter(); rowsChanged();
    status(`Moved “${f.title || 'Untitled'}” to the trash.`, { label: 'Undo', run: () => action(async () => {
      const back = await api.untrash(f.vault, f.id);
      rows = await collect(); await refilter(); rowsChanged();
      await openFragment({ vault: f.vault, id: back.id ?? f.id }, { keep: true });
      status(`Restored “${back.title || 'Untitled'}” from the trash.`);
    }) });
  });
}

// ── Changes made outside this window ──
// The main process watches each vault's fragments/, schemas/ and config.yml. Refreshing waits for an idle moment
// and never takes a tab's text away: a clean fragment reloads, an edited one only warns (its save will ask).
let refreshing = false;
async function flushChanges() {
  if (refreshing || phase !== 'idle' || (!changedVaults.size && !changedSchemas.size)) return;
  refreshing = true;
  try {
    for (const p of [...changedSchemas]) { changedSchemas.delete(p); changedVaults.delete(p); await reloadVault(p); }
    for (const p of [...changedVaults]) { changedVaults.delete(p); await refreshVault(p); }
  } catch (err) { status(err); }
  finally { refreshing = false; if (changedVaults.size || changedSchemas.size) setTimeout(flushChanges); }
}
async function refreshVault(p) {
  const view = scope;
  if (inScope(p)) {
    const next = await collect();
    if (scope === view) { rows = next; await refilter(); rowsChanged(); }
  } else if (outside.has(p)) { outside.set(p, await listVault(p)); for (const d of docTabs()) d.editor.refresh(); }
  for (const d of docTabs().filter(t => t.opened?.vault === p)) await refreshDoc(d);
}
async function refreshDoc(d) {
  const f = d.opened, which = d === tab ? 'This fragment' : `“${f.title || 'Untitled'}”`;
  let latest;
  try { latest = await api.note(f.vault, f.id); } catch {
    if (d.opened === f) status(`${which} was moved to the trash outside parc. Copy anything you need; saving it will fail.`);
    return;
  }
  if (d.opened !== f || latest.revision === f.revision) return;
  if (d.dirty || d.saving || speechPhase !== 'idle') { status(`${which} changed outside parc while you were editing. Saving will ask before overwriting.`); return; }
  load(d, { ...latest, vault: f.vault }); paint(); sidebar.markSelection({ scroll: false });
  await Promise.all([loadNeighbors(d), loadAttachments(d)]);
  status(`Reloaded: ${which === 'This fragment' ? 'this fragment' : which} changed outside parc.`);
}
api.onVaultChanged(event => {
  const p = typeof event === 'string' ? event : event?.vault;
  const kind = typeof event === 'object' ? event?.kind : 'fragments';
  if (!p) return;
  if (kind === 'schemas') changedSchemas.add(p); else changedVaults.add(p);
  flushChanges();
  for (const d of docTabs()) {
    const target = d.opened?.vault;
    if (target ? target === p : (!scope || scope === p)) d.related.refresh();
  }
});

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
    b.onclick = e => openRow(r, clickHow(e));
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
// ── Attachments ──
// Attached files live in <vault>/attachments/<id>/ and are listed in the fragment's frontmatter.
const bytesLabel = n => typeof n !== 'number' ? '' : n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
async function loadAttachments(d) {
  const f = d.opened, section = d.ui.docAttachments;
  if (!f) { d.attachments = null; section.hidden = true; return; }
  let items;
  try { items = await api.attachments(f.vault, f.id); } catch { items = []; }
  if (d.opened !== f) return;
  d.attachments = items;
  d.editor.refresh();
  if (!items.length) { section.hidden = true; d.ui.docAttachmentsBody.replaceChildren(); return; }
  section.hidden = false;
  d.ui.docAttachmentsNote.textContent = `${items.length} ${items.length === 1 ? 'file' : 'files'}`;
  const ul = el('ul', 'attach-list');
  for (const a of items) {
    const li = el('li', 'attach-chip');
    const open = el('button', 'attach-open'); open.type = 'button';
    open.title = `Open ${a.filename}`;
    open.append(icon(IMAGE_EXT.test(a.filename) ? 'image' : 'file'), el('span', 'attach-name', a.filename), el('span', 'attach-size', bytesLabel(a.size)));
    open.onclick = () => api.openAttachment(f.vault, f.id, a.filename).catch(status);
    const ins = el('button', 'attach-btn'); ins.type = 'button';
    ins.title = `Insert ![[attach:${a.filename}]] into text`; ins.setAttribute('aria-label', ins.title);
    ins.append(icon('link'));
    ins.onclick = () => { d.editor.insert(`![[attach:${a.filename}]]`); status(`Inserted ![[attach:${a.filename}]].`); };
    const del = el('button', 'attach-btn attach-remove'); del.type = 'button';
    del.title = `Remove ${a.filename}`; del.setAttribute('aria-label', del.title);
    del.append(icon('x'));
    del.onclick = () => detachFile(d, a.filename);
    li.append(open, ins, del);
    ul.append(li);
  }
  const add = el('button', 'attach-add'); add.type = 'button';
  add.append(icon('paperclip'), 'Attach…');
  add.onclick = () => attachFiles(d);
  d.ui.docAttachmentsBody.replaceChildren(ul, add);
}
function attachFiles(d = cur()) {
  const f = d?.opened;
  if (!f) { status('Save this fragment first before attaching files.'); return; }
  return action(async () => {
    const res = await api.pickAttachment(f.vault, f.id);
    if (!res) return;
    if (d.opened === f) {
      if (res.revision) d.opened.revision = res.revision;
      if (res.attachments) d.opened.attachments = res.attachments;
      await loadAttachments(d);
    }
    if (res.attached?.length) {
      rows = await collect(); await refilter(); rowsChanged();
      const msg = res.attached.length === 1 ? `Attached “${res.attached[0]}”.` : `Attached ${res.attached.length} files.`;
      status(res.errors?.length ? `${msg} (${res.errors.join('; ')})` : msg);
    } else if (res.errors?.length) {
      status(new Error(res.errors.join('; ')));
    }
  });
}
function attachDroppedFiles(d, files, { insertImage = false } = {}) {
  const f = d?.opened;
  if (!f) { status('Save this fragment first before attaching files.'); return; }
  return action(async () => {
    const attached = [], errors = [];
    for (const file of files) {
      try {
        const bytes = new Uint8Array(await file.arrayBuffer());
        const res = await api.attachBytes(f.vault, f.id, { name: file.name || 'attachment.bin', bytes });
        if (d.opened !== f) return;
        if (res.revision) d.opened.revision = res.revision;
        if (res.attachments) d.opened.attachments = res.attachments;
        attached.push(res.filename);
        if (insertImage || IMAGE_EXT.test(res.filename)) d.editor.insert(`![[attach:${res.filename}]]`);
      } catch (err) {
        errors.push(`${file.name || 'file'}: ${cleanError(err)}`);
      }
    }
    if (d.opened === f) await loadAttachments(d);
    if (attached.length) {
      rows = await collect(); await refilter(); rowsChanged();
      const msg = attached.length === 1 ? `Attached “${attached[0]}”.` : `Attached ${attached.length} files.`;
      status(errors.length ? `${msg} (${errors.join('; ')})` : msg);
    } else if (errors.length) {
      status(new Error(errors.join('; ')));
    }
  });
}
function detachFile(d, filename) {
  const f = d?.opened;
  if (!f) return;
  return action(async () => {
    const res = await api.detach(f.vault, f.id, filename);
    if (!res?.detached) return;
    if (d.opened === f) {
      if (res.revision) d.opened.revision = res.revision;
      if (res.attachments) d.opened.attachments = res.attachments;
      await loadAttachments(d);
    }
    rows = await collect(); await refilter(); rowsChanged();
    status(`Removed attachment “${filename}”.`);
  });
}

// A fragment tab's Graph section: what links to it and what it links to, as saved.
async function loadNeighbors(d) {
  const f = d.opened, section = d.ui.docGraph;
  if (!f) { section.hidden = true; return; }
  const items = (await api.list(f.vault, `is:all linked:${f.id}`)).map(r => ({ ...r, vault: f.vault }));
  if (d.opened !== f) return;
  section.hidden = false;
  d.ui.docGraphNote.textContent = items.length ? `${items.length} linked, as saved` : '';
  if (items.length) { d.ui.docGraphBody.replaceChildren(graph(f, items)); return; }
  const prefix = shortId(f.id, rowsIn(f.vault)), hint = el('p', 'graph-none');
  hint.append('No links yet. Type ', el('code', '', '[['), ' to link another fragment from here, or mention ', el('code', '', `[[${prefix}]]`), ' in one.');
  d.ui.docGraphBody.replaceChildren(hint);
}

// ── History ──
// A view of its own, about one fragment: versions newest first, each compared with the current one, i.e. what
// restoring it would bring back.
const SAME = new Set(['id', 'body', 'revision', 'created_at', 'updated_at', 'attachments', 'links', 'vault', 'warnings']);
const show = v => v == null || v === '' ? '—' : Array.isArray(v) ? v.join(', ') || '—' : String(v);
function differences(version, current) {
  const fields = [...new Set([...Object.keys(version), ...Object.keys(current)])].filter(k => !SAME.has(k))
    .filter(k => JSON.stringify(version[k] ?? null) !== JSON.stringify(current[k] ?? null))
    .sort((a, b) => (a === 'title' ? -1 : b === 'title' ? 1 : a.localeCompare(b)));
  const lines = diffLines((current.body || '').replace(/^\n/, ''), (version.body || '').replace(/^\n/, ''));
  return { fields, lines, ...stat(lines) };
}
function showHistory(d = cur()) {
  const f = d?.opened;
  if (f) action(() => openView('history', { target: { vault: f.vault, id: f.id, title: f.title } }));
}
async function historyView(v) {
  const f = v.target, out = v.ui.results, intro = v.ui.intro;
  let listing, current;
  try { [listing, current] = await Promise.all([api.history(f.vault, f.id), api.note(f.vault, f.id)]); } catch {
    intro.textContent = '';
    out.replaceChildren(emptyState(`“${f.title || 'Untitled'}” is gone`, 'It may have been moved to the trash. Its versions come back with it.'));
    return;
  }
  if (current.title !== f.title) { v.target = { ...f, title: current.title }; v.ui.toolTitle.textContent = viewLabel(v); ws.update(v); }
  intro.textContent = `Earlier versions of “${current.title || 'Untitled'}”, newest first. Every save keeps the version it replaced.`;
  const versions = listing.sort((a, b) => b.timestamp.localeCompare(a.timestamp)).slice(0, 60);
  if (!versions.length) { out.replaceChildren(emptyState('No earlier versions yet', 'Each save keeps the version it replaces, so they appear here after the next change.')); return; }
  // Lazy-load each version's full text only when selected, instead of fetching up to 60 versions up front.
  const cache = new Map();
  let pickSeq = 0;
  const wrap = el('div', 'history'), choices = el('ul', 'versions'), view = el('div', 'version-view');
  choices.setAttribute('role', 'listbox'); choices.setAttribute('aria-label', 'Versions');
  const buttons = versions.map((ver, i) => {
    const li = el('li'), b = el('button', 'version'); b.type = 'button';
    b.setAttribute('role', 'option');
    const when = el('span', 'version-when', stamp.format(new Date(ver.timestamp)));
    const what = el('span', 'version-what', bytesLabel(ver.size));
    b.append(when, what);
    b.onclick = () => pick(i);
    li.append(b); choices.append(li);
    return b;
  });
  choices.addEventListener('keydown', e => {
    const i = buttons.indexOf(document.activeElement), step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
    if (i >= 0 && step) { e.preventDefault(); const j = Math.max(0, Math.min(buttons.length - 1, i + step)); pick(j); buttons[j].focus(); }
  });
  async function pick(i) {
    const seq = ++pickSeq, ver = versions[i];
    buttons.forEach((b, j) => b.setAttribute('aria-selected', String(i === j)));
    if (cache.has(i)) { view.replaceChildren(compare(cache.get(i), current, f)); return; }
    view.replaceChildren(el('p', 'compare-same', 'Loading version…'));
    let frag;
    try { frag = await api.version(f.vault, f.id, ver.timestamp); }
    catch { if (seq === pickSeq) view.replaceChildren(el('p', 'compare-same', 'Couldn’t load this version.')); return; }
    const entry = { ...ver, frag };
    cache.set(i, entry);
    const diff = differences(frag, current);
    const summary = [diff.added || diff.removed ? `+${diff.added} −${diff.removed}` : '', ...diff.fields.map(capital)].filter(Boolean).join(', ') || 'Same as now';
    buttons[i].querySelector('.version-when').textContent = stamp.format(new Date(frag.updated_at));
    buttons[i].querySelector('.version-what').textContent = summary;
    if (seq === pickSeq) view.replaceChildren(compare(entry, current, f));
  }
  wrap.append(choices, view); out.replaceChildren(wrap);
  await pick(0);
}
// Restoring a version asks about the fragment's unsaved text first, then shows the fragment as restored.
function compare(ver, current, f) {
  const box = el('div', 'compare'), diff = differences(ver.frag, current), when = stamp.format(new Date(ver.frag.updated_at));
  const head = el('div', 'compare-head'), restore = document.createElement('aihio-button');
  restore.setAttribute('variant', 'outline'); restore.setAttribute('size', 'sm'); restore.append(icon('history'), 'Restore this version');
  restore.onclick = () => action(async () => {
    const d = tabOf(f);
    if (d && !await canLeave(d)) return;
    await api.restoreVersion(f.vault, f.id, ver.timestamp);
    const full = await api.note(f.vault, f.id);
    rows = await collect(); await refilter(); rowsChanged();
    for (const v of viewTabs()) if (v.view === 'history' && key(v.target) === key(f)) v.stale = true;
    if (d?.group) { load(d, { ...full, vault: f.vault }); ws.activate(d); await Promise.all([loadNeighbors(d), loadAttachments(d)]); }
    else await openFragment(f);
    status(`Restored the version saved ${when}. The one it replaced is in History.`);
  });
  head.append(el('h2', '', `Saved ${when}`), restore);
  box.append(head);
  if (!diff.fields.length && !diff.added && !diff.removed) { box.append(el('p', 'compare-same', 'This version matches the current one.')); return box; }
  if (diff.fields.length) {
    const table = el('table', 'compare-fields'), thead = el('thead'), tr = el('tr');
    tr.append(el('th', '', 'Field'), el('th', '', 'Now'), el('th', '', 'This version'));
    for (const th of tr.children) th.scope = 'col';
    thead.append(tr); table.append(thead);
    const tbody = el('tbody');
    for (const k of diff.fields) { const r = el('tr'), th = el('th', '', capital(k)); th.scope = 'row'; r.append(th, el('td', '', show(current[k])), el('td', '', show(ver.frag[k]))); tbody.append(r); }
    table.append(tbody); box.append(table);
  }
  if (diff.added || diff.removed) {
    const legend = el('p', 'diff-legend');
    legend.append(el('span', 'diff-key del', '− Only now'), el('span', 'diff-key ins', '+ Only in this version'));
    const pre = el('div', 'diff'); pre.setAttribute('role', 'region'); pre.setAttribute('aria-label', 'Text differences'); pre.tabIndex = 0;
    for (const line of foldDiff(diff.lines)) {
      if (line.op === '…') { pre.append(el('div', 'diff-fold', `${line.count} unchanged ${line.count === 1 ? 'line' : 'lines'}`)); continue; }
      const row = el('div', `diff-line ${line.op === '+' ? 'ins' : line.op === '-' ? 'del' : ''}`.trim());
      row.append(el('span', 'diff-op', line.op === ' ' ? '' : line.op === '-' ? '−' : '+'), el('span', 'diff-text', line.text || ' '));
      pre.append(row);
    }
    box.append(legend, pre);
  }
  return box;
}

// ── A type's own view ──
// Its board (a lane per status value), its risk matrix (a type with likelihood and impact fields) or, for a type
// without a status, its cards. Which lens a type shows, and whether its matrix includes settled risks, are per
// viewer, like the sidebar's grouping. Archived fragments stay out, as they do in default searches.
const typeViews = (() => { const saved = remember.get(TYPE_VIEWS); return saved && typeof saved === 'object' && !Array.isArray(saved) ? saved : {}; })();
const enumValues = (type, field) => schemaFor(scope, type)?.fields.find(f => f.name === field && f.kind === 'enum')?.values || [];
function typeContext(v) {
  const type = v.viewType;
  const status = schemaFor(scope, type)?.fields.find(f => f.name === 'status' && f.kind === 'enum' && f.values?.length) || null;
  const likelihood = enumValues(type, 'likelihood'), impact = enumValues(type, 'impact');
  const axes = likelihood.length && impact.length ? { likelihood, impact } : null;
  const lenses = [...(axes ? [['matrix', 'Matrix', 'matrix']] : []), ...(status ? [['board', 'Board', 'board']] : [])];
  const pref = typeViews[type] || {}, prefer = change => { typeViews[type] = { ...pref, ...change }; remember.set(TYPE_VIEWS, typeViews); refreshTypeViews(null, type); };
  return {
    type, plural: plural(type), items: rows.filter(r => r.type === type && !r.archived),
    status: status && { values: status.values, categories: status.categories || {} }, axes, lenses,
    lens: lenses.some(([x]) => x === pref.lens) ? pref.lens : lenses[0]?.[0] || 'cards', settled: pref.settled === true,
    many: !scope, movable: true, current: recentDoc?.opened ? key(recentDoc.opened) : null,
    key, order: ordered, tone: r => tone(r.vault, r.type, r.status), values: field => enumValues(type, field),
    overdue: r => overdue({ ...r, due: r.fields?.due }), dayLabel, time: r => timeOf(r, listView.sort === 'created'), hue, vaultName: name,
    open: (r, e) => openRow(r, clickHow(e)), move: moveFragment,
    create: enabledTypes(scope).some(s => s.name === type) ? fields => newCapture(type, fields) : null,
    setLens: lens => prefer({ lens }), setSettled: settled => prefer({ settled }), rerender: () => renderTypeView(v)
  };
}
function openTypeView(type, how = {}) { return action(() => openView('type', { type, how })); }
// Re-rendered in place when its fragments change, keeping focus on the same card (or control) and the board's scroll.
function renderTypeView(v, focus) {
  if (!v.group) return;
  if (dragging()) { setTimeout(() => renderTypeView(v, focus), 200); return; } // never pull the card out from under the pointer
  const ctx = typeContext(v), out = v.ui.results, where = scope ? name(scope) : 'your opened vaults', active = document.activeElement;
  focus ??= out.contains(active) ? active.closest('[data-key]') ? `[data-key="${CSS.escape(active.closest('[data-key]').dataset.key)}"]`
    : active.dataset.lens ? `[data-lens="${active.dataset.lens}"]` : active.closest('.view-bar .chip') ? '.view-bar .chip' : null : null;
  const x = out.querySelector('.board')?.scrollLeft || 0, kind = ctx.type, many = ctx.plural.toLowerCase();
  v.ui.toolTitle.textContent = ctx.plural;
  v.ui.intro.textContent = ctx.lens === 'matrix'
    ? `${ctx.settled ? 'All' : 'Open'} ${many} in ${where} by likelihood and impact; the most serious sit top right. Drag one to another cell (or press Shift with an arrow) to reassess it. Select one to open it.`
    : ctx.lens === 'board' ? `Every ${kind} in ${where} by status. Drag a card to another column (or press Shift with ← or →) to change its status. Select one to open it.`
      : `Every ${kind} in ${where}. Select one to open it.`;
  out.replaceChildren(typeView(ctx));
  const board = out.querySelector('.board');
  if (board) board.scrollLeft = x;
  if (focus) out.querySelector(focus)?.focus({ preventScroll: true });
}
// Every type view (of `type`, if given) redraws: those on screen at once, keeping focus on `focus` in the current
// tab; the others when next shown.
function refreshTypeViews(focus, type) {
  for (const v of viewTabs()) {
    if (v.view !== 'type' || (type && v.viewType !== type)) continue;
    if (ws.shown(v)) renderTypeView(v, v === tab ? focus : undefined); else v.stale = true;
  }
}

// ── Moving a fragment on a board or the matrix ──
// The card moves at once; the saves follow in order. Each changes only the fields it names, so it needs no
// revision (nothing else in the file is touched). Then the list reloads from the vault, and Undo moves it back.
const moves = [];
let moving = false;
function patchRow(row, fields) {
  for (const [k, v] of Object.entries(fields)) {
    if (k === 'status') { row.status = v; continue; }
    row.fields = { ...row.fields };
    if (v == null) delete row.fields[k]; else row.fields[k] = v;
  }
}
function moveFragment(r, fields) {
  const was = Object.fromEntries(Object.keys(fields).map(k => [k, (k === 'status' ? r.status : r.fields?.[k]) ?? null]));
  if (Object.keys(fields).every(k => was[k] === (fields[k] ?? null))) return;
  for (const row of [...rows, ...(filtered || [])]) if (key(row) === key(r)) patchRow(row, fields);
  list(); refreshTypeViews(`[data-key="${CSS.escape(key(r))}"]`);
  moves.push({ vault: r.vault, id: r.id, title: r.title, fields, was });
  drainMoves();
}
const movedMessage = m => {
  const t = `“${m.title || 'Untitled'}”`;
  if ('status' in m.fields) return m.fields.status == null ? `${t} has no status now.` : `Moved ${t} to ${capital(m.fields.status)}.`;
  return `${t} is now ${Object.entries(m.fields).map(([k, v]) => v == null ? `without ${k}` : `${v} ${k}`).join(', ')}.`;
};
async function drainMoves() {
  if (moving || !moves.length) return;
  if (speechPhase !== 'idle' || savingDocs > 0) { setTimeout(drainMoves, 50); return; }
  moving = true;
  pendingWork++;
  syncPhase();
  try {
    let last = null;
    try {
      while (moves.length) {
        const m = moves[0], note = await api.update(m.vault, m.id, { fields: m.fields }, null);
        moves.shift(); last = m;
        // A tab on the fragment follows the move, unless its text has edits: then saving them still asks first.
        const d = tabOf(m);
        if (d && !d.dirty) { d.opened = { ...note, vault: m.vault }; meta(d); ws.update(d); }
        if (note.warnings?.length) status(note.warnings.join('; '));
      }
    } catch (err) {
      status(err);
      last = null;
    } finally {
      moves.length = 0; // after a failed save, what the vault holds is shown again
      try { rows = await collect(); await refilter(); rowsSynced(); list(); refreshTypeViews(); }
      catch (err) { status(err); }
    }
    if (last) {
      const m = last;
      status(movedMessage(m), { label: 'Undo', run: () => { const r = rows.find(x => x.vault === m.vault && x.id === m.id); if (r) moveFragment(r, m.was); } });
    }
  } finally {
    moving = false;
    pendingWork--;
    syncPhase();
  }
  if (moves.length) drainMoves();
}

// ── Views ──
function section(text, aside) { const h = el('h2', 'section-label', text); if (aside) h.append(el('small', '', aside)); return h; }
function emptyState(title, body, code) {
  const p = el('div', 'empty'); p.append(el('strong', '', title), body);
  if (code) { p.append(' '); p.append(el('code', '', code)); }
  return p;
}
// Runs until the results match what is in the box, so typing fast never leaves stale results behind.
async function search(v) {
  if (v.searching) return;
  v.searching = true;
  const out = v.ui.results, input = v.ui.query;
  let q, found, failure = null;
  try {
    do {
      q = input.value.trim();
      try { found = await collect(q); failure = null; } catch (err) { failure = err; }
    } while (input.value.trim() !== q);
  } finally { v.searching = false; }
  ws.update(v);
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
// A search for `query`; with none, the list of every fragment.
function searchFor(query) { return action(() => openView('search', { query, everything: !query })); }
function hints(v) {
  const tagCounts = new Map();
  for (const r of rows) for (const t of r.tags || []) tagCounts.set(t, (tagCounts.get(t) || 0) + 1);
  const top = [...tagCounts].sort((a, b) => b[1] - a[1])[0]?.[0];
  const tokens = ['type:decision', 'status:open', 'due:this-week', 'updated:this-week', 'has:links', top && `#${top}`].filter(Boolean);
  const box = v.ui.hints;
  box.querySelectorAll('.hint').forEach(n => n.remove());
  box.append(...tokens.map(t => {
    const b = el('button', 'hint', t); b.type = 'button'; b.setAttribute('aria-describedby', v.ui.hintsLabel.id);
    b.onclick = () => {
      const q = v.ui.query.value.trim();
      if (!q.split(/\s+/).includes(t)) v.ui.query.value = q ? `${q} ${t}` : t;
      ws.keep(v); action(() => search(v));
    };
    return b;
  }));
}
// Each count opens the search that lists what it counts.
function pulse(out, overdueCount) {
  const openOf = type => rows.filter(r => r.type === type && !settled(r)).length;
  const known = type => typesIn(scope).some(s => s.name === type) || rows.some(r => r.type === type);
  const figures = el('div', 'figures');
  // A count of one type's open work opens that type's view; the others open the search that lists them.
  const figure = (label, value, query, alert, type) => {
    const b = el('button', 'figure'); b.type = 'button';
    if (alert) b.dataset.tone = 'alert';
    b.append(el('span', 'figure-label', label), el('span', 'figure-value', value.toLocaleString()));
    b.title = type ? `Open ${plural(type)}` : query ? `Search ${query}` : 'List every fragment';
    b.onclick = e => type ? openTypeView(type, clickHow(e)) : searchFor(query);
    return b;
  };
  figures.append(figure('Fragments', rows.length, ''));
  if (known('todo')) figures.append(figure('Open todos', openOf('todo'), '', false, 'todo'));
  figures.append(figure('Overdue', overdueCount, `due:overdue ${settledValues(scope)}`.trim(), overdueCount > 0));
  if (known('risk')) figures.append(figure('Open risks', openOf('risk'), '', false, 'risk'));

  const byType = new Map(); for (const r of rows) byType.set(r.type, (byType.get(r.type) || 0) + 1);
  const meters = el('dl', 'meters');
  for (const [type, count] of [...byType].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))) {
    const share = count / rows.length * 100, dt = el('dt'), link = el('button', 'meter-link'), bar = el('dd', 'meter'), fill = el('i');
    link.type = 'button'; link.title = `Open ${plural(type)}`; link.onclick = e => openTypeView(type, clickHow(e));
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
    ...pulseDailySections(rows, { scope, name, settled, overdue, dayLabel, age, section, openRow, clickHow, openTypeView, searchFor, settledValues }),
    section('By type', `${byType.size} ${byType.size === 1 ? 'type' : 'types'}`), meters,
    section('Last edited', 'Fragments by the week they were last edited, 12 weeks'), chart, axis,
    section('Recently edited'), rowList(rows.slice(0, 6)));
}
// Pulse is home: drawn on arrival, and again in place when the vault changes (keeping focus on the same control).
async function renderPulse(v) {
  const out = v.ui.results, where = name(scope), buttons = () => [...out.querySelectorAll('button')], at = buttons().indexOf(document.activeElement);
  v.ui.intro.textContent = `Where ${scope ? where : 'your opened vaults'} stand${scope ? 's' : ''} today. Select a count to see what it counts.`;
  const late = rows.length ? (await collect('due:overdue')).filter(r => !settled(r)).length : 0;
  if (!v.group) return;
  out.replaceChildren();
  if (!rows.length) { out.append(emptyState('Nothing to measure yet', 'Capture a few fragments and their counts will show here.')); return; }
  pulse(out, late);
  if (at >= 0) buttons()[at]?.focus({ preventScroll: true });
}
// Views follow the list. Those on screen redraw at once, the others when next shown: Pulse and the type views
// always; the rest (search, graph, agent) when the scope changed (`all`). History waits until it is shown again.
function refreshViews(all = false) {
  for (const v of viewTabs()) {
    if (v.view === 'type') continue;
    if (v.view === 'agent') v.agent?.relink(); // citations in answers resolve against the fragments in view
    const live = ['pulse', 'trash'].includes(v.view) || (all && v.view !== 'history');
    if (!live) { if (v.view === 'history' || v.view === 'doctor') v.stale = true; continue; }
    if (ws.shown(v)) draw(v); else v.stale = true;
  }
  refreshTypeViews();
}
const draw = v => { v.rendering = renderView(v).catch(status); return v.rendering; };
// Opens a view in the current group: its tab there if it has one (a type's view per type, History per fragment),
// redrawn with what is in the vault now; otherwise a new tab, the group's preview unless asked to stay.
async function openView(view, { type = null, target = null, query, everything = false, how = {} } = {}) {
  if (!(how.group || how.side || how.tab)) ++navSeq;
  if (view === 'type' && !type) view = 'pulse';
  const same = t => t.kind === 'view' && t.view === view && (view !== 'type' || t.viewType === type) && (view !== 'history' || key(t.target) === key(target));
  let v = how.side || how.group ? null : (ws.group?.tabs || []).find(same);
  if (v) {
    if (query != null) v.ui.query.value = query;
    v.everything = everything; v.stale = true;
    if (how.keep || how.tab) ws.keep(v);
    ws.activate(v, { focus: true });
  } else {
    v = viewTab(view, { type, target, query: query ?? '' });
    v.everything = everything;
    ws.open(v, { ...placement(how), focus: true });
  }
  status();
  await v.rendering;
}
async function renderView(v) {
  v.stale = false;
  const { ui } = v, out = ui.results, intro = ui.intro, where = name(scope), view = v.view;
  ui.toolTitle.textContent = viewLabel(v);
  ws.update(v);
  if (view === 'search') {
    intro.textContent = `Titles, bodies and fields in ${scope ? where : 'every opened vault'}. Combine words with filters; every term must match. Tab completes filters.`;
    hints(v);
    if (ui.query.value.trim() || v.everything) await search(v); else out.replaceChildren();
    return;
  }
  if (view === 'history') { await historyView(v); return; }
  if (view === 'type') { renderTypeView(v); return; }
  if (view === 'pulse') { await renderPulse(v); return; }
  if (view === 'agent') { await renderAgent(v); return; }
  if (view === 'trash') {
    const paths = scope ? [scope] : vaults.map(x => x.path);
    await renderTrashView(v, {
      api, paths, scope, name, typeLabel, timeOf, emptyState, status,
      onRestored: async (vault, id, title) => {
        rows = await collect(); await refilter(); rowsChanged();
        await openFragment({ vault, id }, { keep: true });
        status(`Restored “${title || 'Untitled'}” from the trash.`);
      },
      onChanged: async () => { await renderView(v); }
    });
    return;
  }
  if (view === 'doctor') {
    const paths = scope ? [scope] : vaults.map(x => x.path);
    await renderDoctorView(v, {
      api, paths, scope, name,
      openById: (vault, id, how) => openRow({ vault, id }, how),
      clickHow, section, status,
      onReindexed: async () => { rows = await collect(); await refilter(); rowsChanged(); },
      rerender: () => draw(v)
    });
    return;
  }
  out.replaceChildren();
  if (view === 'graph') {
    intro.textContent = `Every fragment in ${scope ? where : 'your opened vaults'} and the links between them. Select one to open it (${mod}+click: beside); scroll to zoom, drag to pan.`;
    if (!rows.length) { out.append(emptyState('Nothing to draw yet', 'Capture a few fragments and link them with [[ to see them here.')); return; }
    const paths = scope ? [scope] : vaults.map(x => x.path);
    const edges = (await Promise.all(paths.map(async p => (await api.links(p)).map(e => [key({ vault: p, id: e.source }), key({ vault: p, id: e.target })])))).flat();
    out.replaceChildren(vaultGraph(rows, edges, {
      key, current: recentDoc?.opened ? key(recentDoc.opened) : null, settled, open: (r, e) => openRow(r, clickHow(e)),
      label: r => r.title || 'Untitled', describe: r => [typeLabel(r.type), r.status && capital(r.status), !scope && name(r.vault)].filter(Boolean).join(', ')
    }));
    return;
  }
}

// ── Agent ──
// A chat per Agent tab (agent.js); the model and its tools run in the main process (agent.cjs). A new chat searches
// the vaults in view; once saved it keeps its own, so its citations resolve in those vaults wherever the scope is.
const agentScope = () => ({
  vaults: (scope ? vaults.filter(x => x.path === scope) : vaults).map(x => ({ path: x.path, name: x.name })),
  label: scope ? name(scope) : 'every opened vault'
});
const agentVaults = paths => paths.map(p => {
  const v = vaults.find(x => x.path === p);
  return { path: p, name: v?.name || p.split('/').slice(-2, -1)[0] || p, open: !!v };
});
function agentSubject() {
  const o = recentDoc?.opened;
  return o ? { vault: o.vault, id: o.id, title: o.title, type: o.type } : null;
}
// Citations resolve in the chat's vaults; a prefix names one fragment there or none.
function agentLinks(paths) {
  const find = prefix => { const hits = paths.flatMap(rowsIn).filter(r => r.id.startsWith(prefix)); return hits.length === 1 ? hits[0] : null; };
  return {
    resolve(prefix) { const r = find(prefix); return r ? { title: r.title, type: r.type } : null; },
    open(prefix, e) { const r = find(prefix); if (r) openRow(r, clickHow(e)); },
    pool: () => [], shortId: r => r.id, describe: () => []
  };
}
const chatTab = id => viewTabs().find(t => t.view === 'agent' && (t.agent ? t.agent.id() === id : t.chatId === id)) || null;
async function renderAgent(v) {
  v.ui.intro.textContent = 'Ask about your notes. The agent searches and reads your fragments to answer, and cites the ones it used. It can’t change anything.';
  v.agent ||= agentChat(v.ui.results, {
    api, chatId: v.chatId, scope: agentScope, vaultNames: agentVaults, subject: agentSubject, links: agentLinks, clickHow,
    notify: (message, act) => status(message, act),
    open: (id, how) => openChat(id, how, v), fresh: how => newChat(how), remove: deleteChat,
    changed: () => {
      // Asked or reopened: the tab is the chat's, named for it, and stays when another is opened.
      if (v.agent?.saved()) ws.keep(v);
      v.chatId = v.agent?.saved() ? v.agent.id() : null;
      v.ui.toolTitle.textContent = viewLabel(v); ws.update(v); rememberWorkspace();
      syncOutside().catch(() => {});
    }
  });
  await v.agent.render();
}
// A saved chat: its tab if it has one; else in place of the empty chat it was picked from; else a new tab.
function openChat(id, how = {}, from = null) {
  return action(async () => {
    const here = chatTab(id);
    if (here) {
      if (how.side && here.group === ws.group && here.group.tabs.length > 1) ws.toNeighbour(here, 1);
      else ws.activate(here, { focus: true });
      return;
    }
    if (from?.agent?.empty() && !how.side && !how.group) { await from.agent.load(id); return; }
    ws.open(viewTab('agent', { chat: id }), { ...placement({ ...how, tab: true }), focus: true });
  });
}
// An empty chat: the current tab's or the group's if there is one, else a new tab.
function newChat(how = {}) {
  return action(async () => {
    const blank = t => t?.view === 'agent' && (t.agent ? t.agent.empty() && !t.agent.saved() : !t.chatId);
    const here = !how.side && (blank(tab) ? tab : ws.group?.tabs.find(blank));
    if (here) { ws.activate(here, { focus: true }); here.agent?.focus(); return; }
    ws.open(viewTab('agent'), { ...placement({ ...how, tab: true }), focus: true });
  });
}
// Deleted until the app next starts: Undo brings it back (its tab doesn't come back with it).
async function deleteChat(id, title) {
  const t = chatTab(id);
  if (t) { await ws.close(t); if (chatTab(id)) return; }
  await api.agentDeleteChat(id);
  status(`Deleted the chat “${title || 'Untitled'}”.`, { label: 'Undo', run: () => api.agentRestoreChat(id).then(() => status(`Restored the chat “${title || 'Untitled'}”.`), status) });
}
// Saved chats for Go to…, most recent first.
let savedChats = [];
const refreshChats = () => api.agentChats().then(list => { savedChats = list; }, () => {});
api.onAgentChats(refreshChats);

// ── Settings ──
// Choices wait for Save (`pending`); downloading and deleting a model happen at once.
let pending = null, models = null, settingsView = null;
function renderSettings(view = settingsView) {
  settingsView = view;
  const custom = pending.whisperModel && !models?.models.some(m => m.path === pending.whisperModel);
  $('modelPath').textContent = custom ? pending.whisperModel : 'None'; $('modelPath').title = custom ? pending.whisperModel : '';
  $('binPath').textContent = pending.whisperBin || 'whisper-cli from PATH'; $('binPath').title = pending.whisperBin;
  $('clearBin').hidden = !pending.whisperBin;
  const where = speech.language && speech.language !== 'auto' ? ` in ${languageName(speech.language)}` : '';
  $('speechState').textContent = speech.ready ? speech.warning || `Ready to dictate${where}.` : speech.problem || 'Not set up.';
  $('speechState').dataset.tone = speech.ready && !speech.warning ? 'ok' : 'warn';
  const asked = `Asked the desktop for ${view.quickCaptureShortcut}. Wayland desktops don’t say whether they agreed: if it does nothing, bind the command below instead.`;
  $('shortcutState').textContent = view.shortcut?.error || (view.shortcut?.asked ? asked : view.shortcut?.active ? `${view.quickCaptureShortcut} opens quick capture.` : 'Leave empty for none.');
  $('shortcutState').dataset.tone = view.shortcut?.error ? 'error' : view.shortcut?.asked ? 'warn' : '';
  $('captureCommand').textContent = view.captureCommand;
  renderLanguages(); renderModels();
}
function renderLanguages() {
  const box = $('languageList'), chosen = pending.speechLanguages;
  box.replaceChildren();
  for (const code of chosen) {
    const chip = el('span', 'tag'), remove = el('button', 'tag-x');
    remove.type = 'button'; remove.title = `Remove ${languageName(code)}`; remove.setAttribute('aria-label', remove.title);
    remove.append(icon('x'));
    remove.onclick = () => { pending.speechLanguages = chosen.filter(c => c !== code); refreshModels().catch(modelMessage); };
    chip.append(languageName(code), remove);
    box.append(chip);
  }
  const add = el('select', 'language-add');
  add.setAttribute('aria-label', 'Add a language');
  add.append(new Option(chosen.length ? 'Add a language…' : 'Choose a language…', ''));
  for (const [code, label] of LANGUAGES.filter(c => !chosen.includes(c)).map(c => [c, languageName(c)]).sort((a, b) => a[1].localeCompare(b[1]))) {
    add.append(new Option(label, code));
  }
  add.disabled = chosen.length >= 12;
  add.onchange = () => {
    if (!add.value) return;
    pending.speechLanguages = [...chosen, add.value];
    refreshModels().then(() => $('languageList').querySelector('select')?.focus()).catch(modelMessage);
  };
  box.append(add);
}
const megabytes = bytes => `${Math.round(bytes / 1e6)} MB`;
function smallButton(label, variant, onClick) {
  const b = document.createElement('aihio-button');
  b.setAttribute('variant', variant); b.setAttribute('size', 'sm'); b.append(label); b.onclick = onClick;
  return b;
}
function renderModels() {
  const list = $('modelList');
  list.replaceChildren();
  if (!models) return;
  const busy = models.models.some(m => m.downloading);
  for (const m of models.models) {
    const li = el('li', 'model'), text = el('div', 'model-text'), actions = el('div', 'model-actions');
    li.dataset.id = m.id;
    const title = el('strong', '', m.label);
    if (m.id === models.recommended) {
      const badge = document.createElement('aihio-badge'); badge.setAttribute('variant', 'secondary'); badge.append('Recommended');
      title.append(' ', badge);
    }
    text.append(title, el('span', 'model-note', `${m.note} ${megabytes(m.size)}.`));
    if (m.downloading) {
      const bar = el('progress'); bar.max = m.size; bar.value = m.downloading.received;
      bar.setAttribute('aria-label', `Downloading ${m.label}`);
      actions.append(bar, el('span', 'model-percent', `${Math.floor(100 * m.downloading.received / m.size)}%`),
        smallButton('Cancel', 'ghost', () => api.cancelDownload().catch(modelMessage)));
    } else if (m.installed) {
      actions.append(pending.whisperModel === m.path ? el('span', 'model-in-use', 'In use')
        : smallButton('Use', 'outline', () => { pending.whisperModel = m.path; renderSettings(); }));
      const remove = smallButton(icon('trash'), 'ghost', () => deleteModel(m));
      remove.setAttribute('size', 'icon'); remove.setAttribute('aria-label', `Delete ${m.label}`); remove.title = remove.getAttribute('aria-label');
      actions.append(remove);
    } else {
      const get = smallButton('Download', 'outline', () => downloadModel(m));
      get.toggleAttribute('disabled', busy);
      actions.append(get);
    }
    li.append(text, actions);
    list.append(li);
  }
}
function modelMessage(message) { $('modelMessage').textContent = message ? cleanError(message) : ''; }
async function refreshModels() {
  models = await api.speechModels(pending.speechLanguages);
  renderSettings();
}
async function downloadModel(m) {
  modelMessage(`Downloading ${m.label}…`);
  const done = api.downloadModel(m.id);
  refreshModels().catch(() => {});
  try {
    const result = await done;
    if (result.cancelled) modelMessage('Download cancelled. Download again to resume where it stopped.');
    else {
      if (result.inUse || !pending.whisperModel) pending.whisperModel = result.path;
      modelMessage(`${m.label} downloaded${result.inUse ? ' and in use' : ''}.`);
      if (!$('settings').hasAttribute('open')) status(`Speech model downloaded${result.inUse ? '. Dictation is ready' : ''}.`);
    }
  } catch (err) { modelMessage(err); }
  await checkSpeech();
  if (pending) await refreshModels().catch(() => {});
}
async function deleteModel(m) {
  try {
    const result = await api.deleteModel(m.id);
    if (!result.deleted) return;
    if (pending.whisperModel === result.path) pending.whisperModel = '';
    modelMessage(`${m.label} deleted.`);
    await checkSpeech(); await refreshModels();
  } catch (err) { modelMessage(err); }
}
// Moves the open dialog's progress bar in place, so the Cancel button keeps focus.
api.onModelProgress(p => {
  const m = models?.models.find(x => x.id === p.id);
  if (!m) return;
  if (p.done) { m.downloading = null; return; }
  m.downloading = { received: p.received, size: p.size };
  const row = $('modelList').querySelector(`[data-id="${CSS.escape(p.id)}"]`), bar = row?.querySelector('progress');
  if (!bar) return renderModels();
  bar.value = p.received;
  row.querySelector('.model-percent').textContent = `${Math.floor(100 * p.received / p.size)}%`;
});
async function openSettings() {
  const view = await api.settings();
  pending = { whisperModel: view.whisperModel, whisperBin: view.whisperBin, speechLanguages: [...(view.speechLanguages || [])], theme: view.theme };
  for (const radio of $('themeChoice').querySelectorAll('input')) radio.checked = radio.value === view.theme;
  $('shortcutInput').value = view.quickCaptureShortcut;
  modelMessage('');
  [models] = await Promise.all([api.speechModels(pending.speechLanguages).catch(() => null), checkSpeech()]);
  renderSettings(view);
  $('settings').open();
}
async function pickFor(kind) {
  const path = await api.pickFile(kind);
  if (!path) return;
  pending[kind === 'model' ? 'whisperModel' : 'whisperBin'] = path;
  renderSettings(await api.settings());
}
// A theme shows at once; closing without Save puts the saved one back (a no-op after Save).
$('themeChoice').addEventListener('change', e => { pending.theme = e.target.value; api.previewTheme(pending.theme).catch(status); });
$('settings').addEventListener('aihio-close', () => api.previewTheme().catch(() => {}));
$('pickModel').addEventListener('click', () => pickFor('model').catch(status));
$('pickBin').addEventListener('click', () => pickFor('binary').catch(status));
$('clearBin').addEventListener('click', async () => { pending.whisperBin = ''; renderSettings(await api.settings()); });
$('copyCaptureCommand').addEventListener('click', () => {
  api.copyText(settingsView.captureCommand).then(() => status('Copied the quick capture command. Bind a key to it in your desktop’s shortcut settings.'), status);
});
$('settingsSave').addEventListener('click', async () => {
  try {
    const before = settingsView?.quickCaptureShortcut;
    const view = await api.saveSettings({ ...pending, quickCaptureShortcut: $('shortcutInput').value });
    await checkSpeech(); renderSettings(view);
    if (!view.shortcut?.error) {
      $('settings').close();
      // Wayland desktops don't confirm a shortcut, so say what to do if a new one doesn't work.
      status(view.shortcut?.asked && view.quickCaptureShortcut !== before
        ? `Settings saved. If ${view.quickCaptureShortcut} does nothing, bind the quick capture command in your desktop instead.`
        : speech.ready ? 'Settings saved. Dictation is ready.' : 'Settings saved.');
    }
  } catch (err) { $('shortcutState').textContent = cleanError(err); $('shortcutState').dataset.tone = 'error'; }
});

// ── Vault settings: types, their fields and statuses (vaultsettings.js) ──
// Kept in the vault itself. After a change, the vault's types and rows reload; a type change may have moved
// fragments to renamed statuses, so its open fragments reload too unless they have unsaved edits.
async function reloadVault(p) {
  await loadSchemas([p]);
  if (inScope(p)) { rows = await collect(); await refilter(); }
  for (const d of docTabs()) {
    if (d.opened?.vault === p && !d.dirty) { try { const full = await api.note(p, d.opened.id); load(d, { ...full, vault: p }); loadNeighbors(d).catch(() => {}); loadAttachments(d).catch(() => {}); } catch { /* moved away: left as it is */ } }
    if (!d.opened && d.fresh && !d.dirty) {
      if (!enabledTypes(destination(d)).some(s => s.name === d.fresh.type)) d.fresh = { ...blankCapture(d), tags: d.fresh.tags };
      applyTemplate(d);
    }
    meta(d); ws.update(d);
  }
  options(); paint(); rowsChanged();
}
const vaultPanel = vaultSettings({
  api,
  vaults: () => vaults,
  schemasOf: p => schemas.get(p),
  async counts(p) {
    const list = inScope(p) ? rows.filter(r => r.vault === p) : await api.list(p, '');
    const n = new Map(); for (const r of list) n.set(r.type, (n.get(r.type) || 0) + 1);
    return n;
  },
  reload: reloadVault,
  notify: message => status(message)
});
async function openVaultSettings() {
  const target = cur()?.opened?.vault || scope || vaults[0]?.path;
  if (!schemas.has(target)) await loadSchemas([target]);
  return vaultPanel.open(target).catch(status);
}
$('vaultSettingsLink').addEventListener('click', () => { $('settings').close(); openVaultSettings(); });

// ── Go to… (Ctrl+K) ──
function commands() {
  const out = [], add = (label, run, extra = {}) => out.push({ label, run, ...extra });
  const startAs = defaultType(scope), d = cur();
  add('New capture', () => newCapture(), { icon: 'plus', keys: `${mod} N`, keywords: `${startAs} create` });
  for (const s of enabledTypes(scope)) if (s.name !== startAs) add(`New ${s.name}`, () => newCapture(s.name), { glyph: typeIcon(s.name), keywords: 'create capture' });
  add('Search', () => action(() => openView('search')), { icon: 'search', keys: `${mod} F` });
  add('Search inside fragments', () => goTo.open('/'), { icon: 'search', keys: `${mod} Shift F`, keywords: 'grep text find lines body content' });
  add('Graph of the vault', () => action(() => openView('graph')), { icon: 'graph', keys: 'G', keywords: 'links' });
  add('Pulse', () => action(() => openView('pulse')), { icon: 'pulse', keys: 'P', keywords: 'stats counts home' });
  add('Vault health', () => action(() => openView('doctor')), { icon: 'shield', keywords: 'doctor broken links check reindex orphans' });
  add('Trash', () => action(() => openView('trash')), { icon: 'trash', keywords: 'deleted restore empty bin' });
  // Each type's own view, for the types in scope that are on or that fragments still use.
  for (const s of typesIn(scope)) {
    if (s.enabled === false && !rows.some(r => r.type === s.name)) continue;
    const lenses = typeContext({ viewType: s.name }).lenses.map(([, label]) => label.toLowerCase());
    add(plural(s.name), () => openTypeView(s.name), { glyph: typeIcon(s.name), keywords: `view type ${lenses.join(' ')} ${lenses.includes('board') ? 'kanban columns' : 'cards'}` });
  }
  add('Agent', () => action(() => openView('agent')), { icon: 'agent', keys: 'A', keywords: 'ask chatgpt question chat answer' });
  add('New chat', () => newChat(), { icon: 'compose', keywords: 'agent ask chatgpt question conversation' });
  for (const c of savedChats.slice(0, 20)) add(c.title || 'Untitled chat', () => openChat(c.id), { icon: 'agent', keywords: 'chat agent conversation' });
  if (d?.dirty) add('Save', save, { icon: 'check', keys: `${mod} S` });
  if (d?.opened) {
    const t = schemaFor(d.opened.vault, d.opened.type)?.toggle;
    if (t) { const next = d.opened[t.field] === t.on ? t.off : t.on; add(`Mark ${next}`, () => toggleField(d), { icon: 'check', keys: 'T', keywords: t.field }); }
    add('Attach file…', () => attachFiles(d), { icon: 'paperclip', keywords: 'file image upload drop' });
    for (const s of enabledTypes(d.opened.vault)) if (s.name !== d.opened.type) add(`Convert to ${typeLabel(s.name)}`, () => promoteFragment(d, s.name), { glyph: typeIcon(s.name), keywords: 'promote change type' });
    add('History of this fragment', () => showHistory(d), { icon: 'history', keywords: 'versions restore' });
    add('Copy link to this fragment', () => copyLink(d), { icon: 'copy', keywords: 'wiki id' });
    add(d.opened.archived ? 'Unarchive this fragment' : 'Archive this fragment', () => toggleArchive(d), { icon: 'archive' });
    add('Move this fragment to the trash', () => trashOpened(d), { icon: 'trash', keywords: 'delete remove' });
  }
  // Tabs and groups.
  const g = ws.group, tabWords = { keywords: 'tab tabs editor' };
  if (tab) {
    add('Close tab', () => closeTab(tab), { icon: 'x', keys: `${mod} W`, ...tabWords });
    if (tab.preview) add('Keep this tab open', () => ws.keep(tab), { icon: 'check', keywords: 'tab pin preview' });
    if (g.tabs.length > 1) add('Close other tabs', () => ws.closeAll(g.tabs.filter(x => x !== tab)), { icon: 'x', ...tabWords });
    const i = ws.groups.indexOf(g), alone = g.tabs.length === 1;
    if (ws.groups[i + 1] || !alone) add(ws.groups[i + 1] ? 'Move tab to the right group' : 'Move tab to a new group on the right', () => ws.toNeighbour(tab, 1), { icon: 'split', keys: `${mod} Alt →`, keywords: 'tab split side beside' });
    if (ws.groups[i - 1] || !alone) add(ws.groups[i - 1] ? 'Move tab to the left group' : 'Move tab to a new group on the left', () => ws.toNeighbour(tab, -1), { icon: 'split', keys: `${mod} Alt ←`, keywords: 'tab split side beside' });
  }
  if (g && ws.groups.length < 4) add('Split right', () => { const n = ws.split(g, 'right'); if (n) ws.focusGroup(ws.groups.indexOf(n)); }, { icon: 'split', keywords: 'group side beside columns tabs' });
  if (ws.groups.length > 1) add('Next group', () => ws.focusGroup((ws.groups.indexOf(g) + 1) % ws.groups.length), { icon: 'split', keys: `${mod} 1–4`, keywords: 'focus tabs' });
  if (closedTabs.length) add('Reopen closed tab', reopenTab, { icon: 'back', keys: `${mod} Shift T`, keywords: 'tab undo restore' });
  for (const v of vaults) if (v.path !== scope) add(`Switch to ${v.name}`, () => switchScope(v.path), { glyph: vaultMark(v.path), keywords: 'vault' });
  if (scope && vaults.length > 1) add('Switch to all opened vaults', () => switchScope('@all'), { icon: 'layers', keywords: 'vault' });
  for (const s of suggested) add(`Open ${s.name} vault (${s.hint})`, () => switchScope(`@suggest:${s.path}`), { icon: 'folder', keywords: 'vault' });
  add('Open vault folder…', () => switchScope('@open'), { icon: 'folder', keywords: 'vault' });
  add('New vault…', () => switchScope('@create'), { icon: 'folderPlus', keywords: 'vault create init' });
  for (const v of vaults) if (v.removable) add(`Remove ${v.name} vault…`, () => closeVault(v.path), { icon: 'x', keywords: 'vault close forget' });
  add(side.collapsed ? 'Show sidebar' : 'Hide sidebar', toggleSidebar, { icon: 'sidebar', keys: `${mod} \\` });
  add(openOnly ? 'Show every fragment in the list' : 'Show only open fragments in the list', toggleOpenOnly, { icon: 'check', keywords: 'filter done' });
  const sidebar = { icon: 'sort', keywords: 'sidebar list group sort order' };
  for (const [gr, label] of Object.entries({ type: 'Group the list by type', tag: 'Group the list by tag', none: 'Ungroup the list' })) {
    if (gr !== listView.group) add(label, () => setListView({ group: gr }), sidebar);
  }
  for (const [s, label] of Object.entries(SORTS)) if (s !== listView.sort) add(label.replace('Sort by', 'Sort the list by'), () => setListView({ sort: s }), sidebar);
  const flip = listView.dir === 'asc' ? 'desc' : 'asc';
  const flipped = dirLabel(listView.sort, flip);
  add(`List ${listView.sort === 'title' ? flipped : flipped.toLowerCase()}`, () => setListView({ dir: flip }), { ...sidebar, keywords: `${sidebar.keywords} reverse ascending descending` });
  add('Quick capture window', () => api.openCapture(), { icon: 'bolt' });
  if (!speech.ready) add('Set up dictation', openSettings, { icon: 'mic', keywords: 'whisper speech' });
  add('Vault settings', () => openVaultSettings(), { icon: 'settings', keywords: 'types statuses fields schema turn off' });
  add('Settings', openSettings, { icon: 'settings', keywords: 'preferences theme dark light appearance dictation shortcut' });
  return out;
}
// Its preview reads fragments as saved. A few stay cached, each until its row says it changed.
const previewCache = new Map();
function loadNote(r) {
  const k = key(r), cached = previewCache.get(k);
  if (cached?.updated_at === r.updated_at) return cached.note;
  const note = api.note(r.vault, r.id);
  previewCache.delete(k); previewCache.set(k, { updated_at: r.updated_at, note });
  note.catch(() => { if (previewCache.get(k)?.note === note) previewCache.delete(k); });
  if (previewCache.size > 40) previewCache.delete(previewCache.keys().next().value);
  return note;
}
// "/" in Go to…: every vault in view, merged, most recently edited first.
async function grepScope(text) {
  const paths = scope ? [scope] : vaults.map(v => v.path);
  const answers = await Promise.all(paths.map(async p => ({ p, found: await api.grep(p, text) })));
  const hits = answers.flatMap(({ p, found }) => found.matches.map(h => ({ ...h, vault: p })));
  return { hits: hits.sort((a, b) => b.updated_at.localeCompare(a.updated_at)), truncated: answers.some(a => a.found.truncated) };
}
// A link in the preview resolves inside the previewed fragment's vault; following it opens that fragment.
function previewLinks(r) {
  const find = prefix => { const hits = rows.filter(x => x.vault === r.vault && x.id.startsWith(prefix)); return hits.length === 1 ? hits[0] : null; };
  return {
    resolve(prefix) { const t = find(prefix); return t ? { title: t.title, type: t.type } : null; },
    open(prefix, e) { const t = find(prefix); if (t) { goTo.close(false); openRow(t, { ...clickHow(e), focus: true }); } },
    pool: () => [], shortId: x => x.id, describe: () => []
  };
}
const goTo = palette({
  fragments: () => rows, commands,
  describe: r => [typeLabel(r.type), !scope && name(r.vault), age(r.updated_at)].filter(Boolean).join(' · '),
  facts: r => [typeLabel(r.type), r.status && capital(r.status), r.archived && 'Archived', !scope && name(r.vault), edited(r.updated_at)].filter(Boolean),
  // A grep match opens with the match selected, the caret ready to edit there. Ctrl+Enter opens it beside.
  open: (r, at, how = {}) => openRow(r, { focus: true, side: !!how.side }).then(() => { const d = tabOf(r); if (at && d && d === tab) d.editor.reveal(at); }),
  load: loadNote, grep: grepScope, linksFor: previewLinks
});

// ── Wiring ──
const DATES = ['today', 'yesterday', 'tomorrow', 'this-week', 'last-week', 'next-week', 'this-month', 'last-month', 'overdue'];
const FILTER_HINTS = { type: 'fragment type', status: 'state', priority: 'low … critical', tag: 'a tag, or #tag', due: 'due date', created: 'date created', updated: 'date edited', has: 'links, due, attachments', is: 'archived, all', by: 'author', linked: 'links to an ID' };
const assistValues = field => ({ type: typesIn(scope).map(s => s.name), tag: allTags(), due: DATES, created: DATES, updated: DATES, has: ['links', 'due', 'attachments'], is: ['archived', 'all'] })[field] ?? fieldValues(field);
searchAssist($('filter'), { id: 'filterAssist', values: assistValues, describe: f => FILTER_HINTS[f] });
const halfTyped = q => /(^|\s)!?[a-z]+:!?$/i.test(q); // "status:" waits for its value
// A keyboard click on a row, carrying Ctrl (⌘) so it can open beside.
const keyClick = (b, e) => b.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: e.ctrlKey, metaKey: e.metaKey }));
$('filter').addEventListener('input', onFilter);
$('filter').addEventListener('keydown', e => {
  const first = sidebar.firstRow();
  if (e.key === 'ArrowDown' && first) { e.preventDefault(); sidebar.focusIndex(0); }
  else if (e.key === 'Enter' && first && $('filterAssist').hidden) { e.preventDefault(); e.stopPropagation(); keyClick(first, e); } // opens it, caret in the text
});
// The list moves with the arrow keys, through group headers too; Enter opens a fragment with the caret in its
// text (Ctrl+Enter: beside). On a header ← folds and → unfolds (or enters the group); ← from a row goes up to its header.
$('recent').addEventListener('keydown', e => {
  const active = document.activeElement, i = sidebar.indexOf(active);
  if (i < 0) return;
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && active.classList.contains('row')) { e.preventDefault(); e.stopPropagation(); keyClick(active, e); return; }
  const group = active.closest('.group'), onHead = active.classList.contains('group-head');
  if (group && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
    e.preventDefault();
    const id = group.dataset.group, open = !folded.has(id);
    if (e.key === 'ArrowLeft') { if (!onHead) group.querySelector('.group-head').focus(); else if (open) foldGroup(id, true); }
    else if (onHead) { if (open) sidebar.firstGroupRow(id)?.focus(); else foldGroup(id, false); }
    return;
  }
  const total = sidebar.totalEntries();
  const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: total - 1 }[e.key];
  if (to === undefined) return;
  e.preventDefault();
  if (to < 0) $('filter').focus(); else sidebar.focusIndex(Math.min(total - 1, to));
});
$('openOnly').onclick = toggleOpenOnly;
$('listView').addEventListener('aihio-select', e => {
  e.stopPropagation();
  const [what, value] = e.detail.value.split(':');
  if (['group', 'sort', 'dir'].includes(what)) setListView({ [what]: value });
});
$('sidebarHide').addEventListener('click', toggleSidebar);
$('sidebarShow').addEventListener('click', toggleSidebar);
$('goTo').onclick = () => goTo.open();
$('openSettings').onclick = () => openSettings().catch(status);
$('quickCapture').onclick = () => api.openCapture().catch(status);
$('fragmentMenu').addEventListener('aihio-select', e => {
  e.stopPropagation();
  ({ attach: attachFiles, history: showHistory, copy: copyLink, archive: toggleArchive, trash: trashOpened })[e.detail.value]?.(cur());
});
$('vaultMenu').addEventListener('aihio-select', e => {
  const v = e.detail.value;
  if (v === '@settings') openVaultSettings();
  else if (v === '@doctor') action(() => openView('doctor'));
  else if (v === '@trash') action(() => openView('trash'));
  else if (v === '@close') closeVault(scope);
  else switchScope(v);
});
$('destination').onchange = () => {
  const d = cur();
  if (!d || d.opened) return;
  d.dest = $('destination').value;
  // A new capture whose type is off in the chosen vault starts as that vault's default instead.
  if (d.fresh && !enabledTypes(d.dest).some(s => s.name === d.fresh.type)) d.fresh = { ...blankCapture(d), tags: d.fresh.tags };
  applyTemplate(d); meta(d); paint(); d.editor.refresh(); rememberWorkspace();
};
$('save').onclick = save;
$('record').onclick = async () => {
  if (speechPhase === 'recording') return voice.stop();
  if (speechPhase !== 'idle' || cur()?.saving) return;
  if (!speech.ready) await checkSpeech(); // whisper.cpp may have been installed since the app started
  if (speechPhase !== 'idle' || cur()?.saving) return;
  if (!speech.ready) return openSettings().catch(status);
  voice.start();
};
$('cancel').onclick = () => voice.stop(true);
// The nav opens its view in the current group; Ctrl+click opens it beside.
for (const b of $('sidebar').querySelectorAll('[data-view]')) {
  b.onclick = e => b.dataset.view === 'capture' ? newCapture() : action(() => openView(b.dataset.view, { how: clickHow(e) }));
}
document.addEventListener('keydown', e => {
  if (goTo.isOpen || $('settings').hasAttribute('open') || vaultPanel.isOpen || ws.menuOpen) return;
  if (e.key === 'Escape' && phase === 'recording') return voice.stop(true);
  const ctrl = (e.metaKey || e.ctrlKey) && !e.altKey, k = e.key.toLowerCase();
  // Tabs and groups, as in a code editor: Ctrl+Tab and Ctrl+PgUp/PgDn step through the group's tabs, Ctrl+Alt+←/→
  // moves the tab to the next group (opening one), Ctrl+1–4 goes to a group, Alt+1–9 to a tab (9: the last).
  if ((e.metaKey || e.ctrlKey) && e.altKey && !e.shiftKey && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) {
    e.preventDefault(); if (tab && !blocking()) ws.toNeighbour(tab, e.key === 'ArrowRight' ? 1 : -1); return;
  }
  if (e.ctrlKey && !e.altKey && !e.metaKey && (e.key === 'Tab' || ((e.key === 'PageDown' || e.key === 'PageUp') && !e.shiftKey))) {
    e.preventDefault(); if (!blocking()) ws.cycle(e.key === 'PageUp' || (e.key === 'Tab' && e.shiftKey) ? -1 : 1); return;
  }
  if (!isMac && e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && /^Digit[1-9]$/.test(e.code)) {
    const tabs = ws.group?.tabs || [], t = e.code === 'Digit9' ? tabs.at(-1) : tabs[+e.code.slice(5) - 1];
    if (t) { e.preventDefault(); if (!blocking()) ws.activate(t, { focus: true }); }
    return;
  }
  if (ctrl && e.shiftKey && k === 't') { e.preventDefault(); reopenTab(); return; }
  if (ctrl && !e.shiftKey && /^Digit[1-4]$/.test(e.code)) { e.preventDefault(); if (!blocking()) ws.focusGroup(+e.code.slice(5) - 1); return; }
  if (ctrl && !e.shiftKey) {
    const run = { enter: () => cur() && save(), s: () => cur() && save(), k: () => goTo.open(), n: () => newCapture(),
      f: () => action(() => openView('search')), w: () => closeTab(), '\\': toggleSidebar }[k];
    if (run) { e.preventDefault(); run(); }
    return;
  }
  if (ctrl && e.shiftKey && k === 'p') { e.preventDefault(); goTo.open('>'); return; }
  if (ctrl && e.shiftKey && k === 'f') { e.preventDefault(); goTo.open('/'); return; }
  if (e.ctrlKey || e.metaKey || e.altKey || e.isComposing || e.target.closest('input,textarea,select,[contenteditable=true],aihio-dropdown,aihio-date-picker,aihio-input')) return;
  const next = ({ '+': 'capture', '/': 'search', g: 'graph', a: 'agent', p: 'pulse' })[k];
  if (next === 'capture') { e.preventDefault(); newCapture(); }
  else if (next) { e.preventDefault(); action(() => openView(next)); }
  else if (k === 't' && cur()) { e.preventDefault(); toggleField(); }
});
// Drafts are flushed atomically to disk on unload, so only work that cannot resume holds the window.
window.addEventListener('beforeunload', e => { rememberWorkspace(true, { syncDisk: true }); if (speechPhase !== 'idle' || savingDocs > 0 || moving) e.returnValue = false; });
applySide(); viewMenu();
(async () => {
  try {
    api.agentReset().catch(() => {}); // chats open before a reload leave memory; saved ones come back with their tabs
    refreshChats();
    vaults = await api.vaults();
    const known = p => vaults.some(v => v.path === p), last = remember.get(SCOPE);
    scope = last === '' || known(last) ? last : vaults[0].path;
    [suggested] = await Promise.all([api.suggestedVaults().catch(() => []), loadSchemas(vaults.map(v => v.path)), checkSpeech()]);
    try { rows = await collect(); } catch (err) { if (scope === vaults[0].path) throw err; scope = vaults[0].path; rows = await collect(); }
    remember.set(SCOPE, scope); options(); rowsSynced(); list();
    // The tabs as they were left. An unsaved draft kept before there were tabs comes back as one.
    let saved = remember.get(WORKSPACE);
    const legacy = remember.get(DRAFT);
    if (!Array.isArray(saved?.groups) && legacy && typeof legacy.body === 'string') {
      saved = { groups: [{ tabs: [{ vault: legacy.vault || '', id: legacy.id || undefined, fresh: legacy.fresh, draft: legacy }], shown: 0 }], current: 0 };
    }
    ws.addGroup();
    const any = saved ? await restoreWorkspace(saved) : false;
    restoring = false; remember.set(DRAFT, null);
    await syncOutside();
    if (!any) await openView('pulse'); // Pulse is home
  } catch (err) { status(err); $('empty').textContent = 'Unable to load fragments.'; }
  finally { restoring = false; rememberWorkspace(true); syncPhase(); }
})();
