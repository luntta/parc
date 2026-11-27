// The Inbox view: triage `source` fragments dropped into `<vault>/inbox/`.
// Supports keyboard navigation (↑/↓, Enter open, r reading, p processed, i inbox,
// a Attach to…, l Link to…, d discard) as well as direct row actions.
import { icon, statusIcon, typeIcon } from './icons.js';
import reports from './inbox-report.cjs';
import { rank } from './editor.js';

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

const bytesLabel = n => typeof n !== 'number' ? '' : n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;

function kindBadge(filename) {
  if (!filename) return '';
  const m = /\.([A-Za-z0-9]{1,8})$/.exec(filename);
  if (!m) return 'FILE';
  const ext = m[1].toUpperCase();
  if (ext === 'HTM') return 'HTML';
  if (ext === 'MARKDOWN') return 'MD';
  return ext;
}

function progressLabel(att, pos) {
  const unit = pos?.unit || att?.unit || 'page';
  const total = pos?.total || att?.parts || 0;
  const cur = pos?.part || 0;
  if (cur && total > 1) {
    if (unit === 'slide') return `slide ${cur} of ${total}`;
    if (unit === 'section') return `§ ${cur} of ${total}`;
    return `p. ${cur} of ${total}`;
  }
  if (total > 0) {
    if (unit === 'slide') return `${total} ${total === 1 ? 'slide' : 'slides'}`;
    if (unit === 'section') return `${total} ${total === 1 ? 'section' : 'sections'}`;
    return `${total} ${total === 1 ? 'page' : 'pages'}`;
  }
  if (att?.status === 'pending') return 'extracting…';
  if (att?.status === 'error') return att.error ? `unreadable (${att.error})` : 'unreadable';
  return '';
}

/**
 * Lightweight modal fragment picker for "Attach to…" and "Link to…" inside the same vault.
 */
function openFragmentPicker({ title, placeholder, items, onPick }) {
  const backdrop = el('div', 'inbox-picker');
  backdrop.setAttribute('role', 'dialog');
  backdrop.setAttribute('aria-modal', 'true');
  backdrop.setAttribute('aria-label', title);

  const box = el('div', 'inbox-picker-box');
  const head = el('div', 'inbox-picker-head');
  const input = el('input', 'inbox-picker-input');
  Object.assign(input, { type: 'search', placeholder, autocomplete: 'off', spellcheck: false });
  input.setAttribute('aria-label', title);
  const closeBtn = el('button', 'inbox-picker-close');
  closeBtn.type = 'button'; closeBtn.setAttribute('aria-label', 'Close');
  closeBtn.append(icon('x'));
  head.append(el('span', 'inbox-picker-title', title), input, closeBtn);

  const list = el('ul', 'inbox-picker-list');
  list.setAttribute('role', 'listbox');

  let shown = [], sel = 0;

  function close() {
    backdrop.remove();
  }

  function render() {
    shown = rank(items, input.value).map(h => h.r);
    sel = Math.min(sel, Math.max(0, shown.length - 1));
    list.replaceChildren();
    if (!shown.length) {
      list.append(el('li', 'inbox-picker-empty', 'No matching fragments in this vault.'));
      return;
    }
    shown.forEach((r, idx) => {
      const li = el('li');
      const b = el('button', 'inbox-picker-item');
      b.type = 'button';
      b.setAttribute('role', 'option');
      b.setAttribute('aria-selected', String(idx === sel));
      b.append(typeIcon(r.type), el('span', 'inbox-picker-name', r.title || 'Untitled'), el('span', 'inbox-picker-type', r.type));
      b.onmouseenter = () => { sel = idx; paintSel(); };
      b.onclick = () => { close(); onPick(r); };
      li.append(b);
      list.append(li);
    });
  }

  function paintSel() {
    list.querySelectorAll('.inbox-picker-item').forEach((b, i) => b.setAttribute('aria-selected', String(i === sel)));
    list.querySelectorAll('.inbox-picker-item')[sel]?.scrollIntoView({ block: 'nearest' });
  }

  input.oninput = () => { sel = 0; render(); };
  input.onkeydown = e => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
    else if (e.key === 'ArrowDown' && shown.length) { e.preventDefault(); sel = (sel + 1) % shown.length; paintSel(); }
    else if (e.key === 'ArrowUp' && shown.length) { e.preventDefault(); sel = (sel - 1 + shown.length) % shown.length; paintSel(); }
    else if (e.key === 'Enter' && shown[sel]) { e.preventDefault(); const chosen = shown[sel]; close(); onPick(chosen); }
  };
  closeBtn.onclick = close;
  backdrop.onclick = e => { if (e.target === backdrop) close(); };

  box.append(head, list);
  backdrop.append(box);
  document.body.append(backdrop);
  render();
  input.focus();
}

/**
 * Renders the Inbox view into `v.ui.results`.
 */
export async function renderInboxView(v, deps) {
  const {
    api, rows, allRows, scope, vaults, name, age, hue, tone, key, clickHow,
    openRow, moveFragment, status, emptyState, onRefresh
  } = deps;

  const generation = (v.inboxGeneration || 0) + 1;
  v.inboxGeneration = generation;
  const current = () => v.group && v.inboxGeneration === generation;
  const out = v.ui.results;
  const filter = v.inboxFilter || 'open';
  const where = scope ? name(scope) : 'your opened vaults';
  v.ui.intro.textContent = `Documents waiting in ${where}. Enter opens · R reading · P processed · A attach to… · L link to… · D discard.`;

  const positions = await api.readingPositions().catch(() => ({}));
  if (!current()) return;

  const sources = rows.filter(r => r.type === 'source' && !r.archived);
  const openSources = sources.filter(r => !['processed'].includes(r.status || ''));
  const processedSources = sources.filter(r => r.status === 'processed');
  const shown = filter === 'processed' ? processedSources : filter === 'all' ? sources : openSources;

  // Preserve keyboard focus by row key across refreshes.
  const activeKey = out.contains(document.activeElement)
    ? document.activeElement.closest('[data-key]')?.dataset.key || null
    : null;

  const wrap = el('div', 'inbox-view');

  // Top bar: filter switch + Sweep + Open folder
  const bar = el('div', 'view-bar inbox-bar');
  const seg = el('div', 'lens');
  seg.setAttribute('role', 'group');
  seg.setAttribute('aria-label', 'Filter inbox');
  const tabs = [
    ['open', `Open (${openSources.length})`],
    ['processed', `Processed (${processedSources.length})`],
    ['all', `All (${sources.length})`]
  ];
  for (const [val, label] of tabs) {
    const b = el('button', 'lens-option', label);
    b.type = 'button';
    b.setAttribute('aria-pressed', String(filter === val));
    b.onclick = () => { v.inboxFilter = val; renderInboxView(v, deps); };
    seg.append(b);
  }
  bar.append(seg);

  const actions = el('div', 'inbox-bar-actions');
  const sweepBtn = document.createElement('aihio-button');
  sweepBtn.setAttribute('variant', 'outline'); sweepBtn.setAttribute('size', 'sm');
  sweepBtn.append(icon('inbox'), 'Sweep folder');
  sweepBtn.onclick = async () => {
    sweepBtn.setAttribute('loading', '');
    try {
      const target = scope || null;
      const rep = await api.inboxSweep(target);
      v.lastSweep = { scope, report: rep };
      await onRefresh();
      status(reports.sweepMessage(rep));
    } catch (err) { status(err); }
    finally { sweepBtn.removeAttribute('loading'); }
  };
  actions.append(sweepBtn);

  const folderVault = scope || vaults[0]?.path;
  if (folderVault) {
    const openDirBtn = document.createElement('aihio-button');
    openDirBtn.setAttribute('variant', 'ghost'); openDirBtn.setAttribute('size', 'sm');
    openDirBtn.append(icon('folder'), 'Open folder');
    openDirBtn.title = `Open ${name(folderVault)} inbox folder`;
    openDirBtn.onclick = () => api.openInboxFolder(folderVault).catch(status);
    actions.append(openDirBtn);
  }
  bar.append(actions);
  wrap.append(bar);

  const last = v.lastSweep && v.lastSweep.scope === scope ? v.lastSweep.report : null;
  if (last && (last.failed?.length || last.warnings?.length || last.skipped?.length)) {
    const details = el('details', 'inbox-sweep-details');
    details.append(el('summary', '', 'Last sweep: skipped files and problems'));
    const list = el('ul');
    for (const f of last.failed || []) list.append(el('li', '', [f.vault, f.filename, f.error].filter(Boolean).join(': ')));
    for (const w of last.warnings || []) list.append(el('li', '', w));
    for (const file of last.skipped || []) list.append(el('li', '', 'Skipped: ' + file));
    details.append(list);
    wrap.append(details);
  }

  if (!shown.length) {
    const inboxPath = folderVault ? await api.inboxPath(folderVault).catch(() => '') : '';
    if (!current()) return;
    const hint = filter === 'processed'
      ? 'Documents you mark processed appear here.'
      : 'Drop PDFs, Word docs, presentations, HTML or text files into your vault’s inbox folder and they arrive here automatically.';
    wrap.append(emptyState(filter === 'processed' ? 'No processed documents yet' : 'No open documents', hint, inboxPath || undefined));
    out.replaceChildren(wrap);
    return;
  }

  // One snapshot per vault. Do not cache pending/error states by fragment
  // timestamp: background extraction does not edit the fragment.
  const attMap = new Map();
  const errors = [];
  await Promise.all([...new Set(shown.map(r => r.vault))].map(async vault => {
    try { attMap.set(vault, await api.inboxAttachments(vault)); }
    catch (err) { errors.push(name(vault) + ': ' + String(err.message ?? err)); }
  }));
  if (!current()) return;
  if (errors.length) wrap.append(el('p', 'quiet', 'Attachment details unavailable. ' + errors.join('; ')));

  async function discardSource(r) {
    try {
      const res = await api.removeQuiet(r.vault, r.id, r.title);
      if (!res?.deleted) return;
      await onRefresh();
      status(`Discarded “${r.title || 'Untitled'}”.`, {
        label: 'Undo',
        run: async () => {
          try {
            await api.untrash(r.vault, r.id);
            await onRefresh();
            status(`Restored “${r.title || 'Untitled'}” to Inbox.`);
          } catch (err) { status(err); }
        }
      });
    } catch (err) { status(err); }
  }

  function attachToTarget(r) {
    if (!attMap.has(r.vault)) { status('Attachment details unavailable. Refresh the inbox and try again.'); return; }
    const atts = attMap.get(r.vault)?.[r.id] || [];
    if (!atts.length) { status('This source has no attached files to move.'); return; }
    const candidates = allRows.filter(x => x.vault === r.vault && x.id !== r.id && !x.archived);
    openFragmentPicker({
      title: `Attach ${atts.length === 1 ? `“${atts[0].filename}”` : `${atts.length} files`} to…`,
      placeholder: 'Search fragments in this vault…',
      items: candidates,
      onPick: async target => {
        let moved = 0;
        try {
          for (const a of atts) {
            await api.moveAttachment(r.vault, r.id, target.id, a.filename);
            moved++;
          }
          // Remove the now-empty source wrapper (quietly if it has no notes, otherwise confirm).
          await api.removeQuiet(r.vault, r.id, r.title, { onlyIfEmpty: true });
          await onRefresh();
          status(`Moved ${atts.length === 1 ? `“${atts[0].filename}”` : `${atts.length} files`} to “${target.title || 'Untitled'}”.`, {
            label: 'Open target',
            run: () => openRow(target, { focus: true })
          });
        } catch (err) {
          if (moved) await onRefresh();
          status(moved ? `Moved ${moved} of ${atts.length} files. ${err.message ?? err}` : err);
        }
      }
    });
  }

  function linkToTarget(r) {
    const candidates = allRows.filter(x => x.vault === r.vault && x.id !== r.id && !x.archived);
    openFragmentPicker({
      title: `Link “${r.title || 'Untitled'}” to…`,
      placeholder: 'Search fragments in this vault…',
      items: candidates,
      onPick: async target => {
        try {
          await api.linkFragments(r.vault, r.id, target.id);
          await onRefresh();
          status(`Linked “${r.title || 'Untitled'}” and “${target.title || 'Untitled'}”.`);
        } catch (err) { status(err); }
      }
    });
  }

  function setStatus(r, next) {
    if (r.status === next) return;
    moveFragment(r, { status: next });
  }

  const ul = el('ul', 'inbox-list');
  ul.setAttribute('role', 'listbox');
  ul.setAttribute('aria-label', 'Inbox documents');

  const rowButtons = [];
  for (const r of shown) {
    const rk = key(r);
    const atts = attMap.get(r.vault)?.[r.id] || [];
    const primary = atts[0] || null;
    const pos = positions[JSON.stringify([r.vault, r.id])] || null;

    const li = el('li', 'inbox-item');
    li.dataset.key = rk;

    const mainBtn = el('button', 'row inbox-row');
    mainBtn.type = 'button';
    mainBtn.dataset.key = rk;

    const st = r.status || 'inbox';
    const lead = statusIcon(tone(r.vault, r.type, st), 'icon lead status');
    const info = el('span', 'inbox-info');
    const topLine = el('span', 'inbox-top');
    topLine.append(el('span', 'row-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
    if (primary) {
      const kind = kindBadge(primary.filename);
      if (kind) topLine.append(el('span', 'inbox-kind', kind));
      if (atts.length > 1) topLine.append(el('span', 'inbox-kind', `+${atts.length - 1}`));
    }

    const sub = el('span', 'inbox-sub');
    const pieces = [];
    if (!scope) {
      const w = el('span', 'inbox-where');
      const dot = el('span', 'vault-dot');
      dot.style.setProperty('--vault-hue', hue(r.vault));
      w.append(dot, name(r.vault));
      pieces.push(w);
    }
    pieces.push(el('span', 'inbox-status', st === 'inbox' ? 'Inbox' : st === 'reading' ? 'Reading' : 'Processed'));
    if (primary) {
      const sz = bytesLabel(primary.size);
      if (sz) pieces.push(el('span', '', sz));
    }
    const prog = progressLabel(primary, pos);
    if (prog) pieces.push(el('span', 'inbox-progress', prog));
    if (r.tags?.length) pieces.push(el('span', 'inbox-tags', r.tags.map(t => `#${t}`).join(' ')));
    pieces.forEach((p, idx) => {
      if (idx > 0) sub.append(el('span', 'inbox-dot', '·'));
      sub.append(p);
    });
    info.append(topLine, sub);

    const when = el('time', 'inbox-age', age(r.created_at || r.updated_at));
    when.dateTime = r.created_at || r.updated_at;

    mainBtn.append(lead, info, when);
    mainBtn.onclick = e => openRow(r, clickHow(e));
    mainBtn.onkeydown = e => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const idx = rowButtons.indexOf(mainBtn);
      if (e.key === 'ArrowDown') { e.preventDefault(); rowButtons[Math.min(rowButtons.length - 1, idx + 1)]?.focus(); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); rowButtons[Math.max(0, idx - 1)]?.focus(); }
      else if (e.key === 'd') { e.preventDefault(); discardSource(r); }
      else if (e.key === 'a') { e.preventDefault(); attachToTarget(r); }
      else if (e.key === 'l') { e.preventDefault(); linkToTarget(r); }
      else if (e.key === 'r') { e.preventDefault(); setStatus(r, 'reading'); }
      else if (e.key === 'p' || e.key === 'e') { e.preventDefault(); setStatus(r, st === 'processed' ? 'inbox' : 'processed'); }
      else if (e.key === 'i') { e.preventDefault(); setStatus(r, 'inbox'); }
    };

    const rowTools = el('div', 'inbox-row-actions');
    const toolBtn = (glyph, title, fn, danger = false) => {
      const b = el('button', 'inbox-tool' + (danger ? ' danger' : ''));
      b.type = 'button'; b.title = title; b.setAttribute('aria-label', title);
      b.append(icon(glyph));
      b.onclick = e => { e.stopPropagation(); fn(); };
      return b;
    };
    rowTools.append(
      toolBtn('paperclip', 'Attach to… (A)', () => attachToTarget(r)),
      toolBtn('link', 'Link to… (L)', () => linkToTarget(r)),
      toolBtn('check', st === 'processed' ? 'Move back to Inbox (I)' : 'Mark processed (P)', () => setStatus(r, st === 'processed' ? 'inbox' : 'processed')),
      toolBtn('trash', 'Discard (D)', () => discardSource(r), true)
    );

    li.append(mainBtn, rowTools);
    ul.append(li);
    rowButtons.push(mainBtn);
  }

  wrap.append(ul);
  out.replaceChildren(wrap);

  if (activeKey) {
    out.querySelector(`.inbox-row[data-key="${CSS.escape(activeKey)}"]`)?.focus({ preventScroll: true });
  }
}
