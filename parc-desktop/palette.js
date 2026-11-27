// Ctrl+K: one box, two panes. The left lists what matches; the right shows the selected fragment as it reads (or
// what a command does), so you can look before you open. The first character typed picks the mode:
//   anything   fragments and commands ranked together, like the TUI's launcher (the [[ picker's ranking);
//   >          commands only;
//   /          grep, as in Vim: the lines of fragment bodies that contain the text, under their fragment with
//              line:column, then the lines of its attached documents' text under the file's name, with the page
//              (slide, section). The preview marks every match and centres the selected one; Enter opens the
//              fragment with that match selected, or the document at that page with the text found.
import { rank, fold, markdownPreview } from './editor.js';
import { icon, typeIcon } from './icons.js';

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
// Word starts beat substrings beat loose subsequences; every word typed must land somewhere.
function scoreCommand(command, query) {
  const words = fold(query).split(/\s+/).filter(Boolean), hay = fold(`${command.label} ${command.keywords || ''}`);
  let score = 0;
  for (const w of words) {
    const at = hay.indexOf(w);
    if (at === 0) score += 130;
    else if (at > 0 && /[\s/:-]/.test(hay[at - 1])) score += 110;
    else if (at > 0) score += 45;
    else {
      let k = 0; for (const ch of hay) if (ch === w[k]) k++;
      if (k < w.length || w.length < 2) return 0;
      score += 10;
    }
  }
  return score;
}
const fragmentKey = r => `${r.vault}\u0000${r.id}`;
const message = err => String(err?.message ?? err).replace(/^Error invoking remote method '[^']+': Error: /, '');
// A matched line, from a little before its first match, with every match marked.
function lineText({ text, ranges }) {
  const chars = [...text], out = el('span', 'palette-text');
  let at = Math.max(0, ranges[0][0] - 24);
  if (at > 0) out.append('…');
  else while (at < ranges[0][0] && /\s/.test(chars[at])) at++; // indentation says nothing here
  for (const [a, b] of ranges) {
    if (b <= at) continue;
    const from = Math.max(a, at);
    out.append(chars.slice(at, from).join(''), el('mark', '', chars.slice(from, b).join('')));
    at = b;
  }
  out.append(chars.slice(at).join(''));
  return out;
}
// Where an attachment line is, short enough for the list: `p. 12`, `slide 3`, `§ 4` (the heading is in the preview).
const partRef = l => l.label?.startsWith('§') ? `§ ${l.part}` : l.label || `${l.part}`;

/**
 * fragments(): rows to rank. commands(): [{ label, run, icon?, glyph?, keys?, keywords? }].
 * describe(row): short facts for a list row; facts(row): the preview's facts, as strings.
 * open(row, at?, how?): open a fragment, at = { line, column, pattern, attachment?, part? } to show a grep match
 * (in the body, or in that attachment's part); how.side opens it beside.
 * load(row): the saved fragment, with its body. loadPart(row, filename, part): the text of an attachment's part.
 * grep(text): { hits: rows with `lines`, truncated }; a line with `attachment` is in that file's `part` (`label`).
 * linksFor(row): how the preview resolves and opens that fragment's [[links]].
 */
export function palette({ fragments, commands, describe, facts = r => [describe(r)], open, load, loadPart, grep, linksFor }) {
  const root = el('div', 'palette'), box = el('div', 'palette-box'), bar = el('div', 'palette-bar');
  const chip = el('span', 'palette-mode'), input = el('input', 'palette-input');
  const panes = el('div', 'palette-panes'), list = el('ul', 'palette-list'), pane = el('section', 'palette-preview');
  const head = el('header', 'pv-head'), body = el('div', 'pv-body'), blank = el('div', 'pv-blank');
  root.hidden = true; chip.hidden = true;
  box.setAttribute('role', 'dialog'); box.setAttribute('aria-modal', 'true'); box.setAttribute('aria-label', 'Go to a fragment or command');
  Object.assign(input, { type: 'text', autocomplete: 'off', spellcheck: false, placeholder: 'Go to a fragment, or type > for commands, / to search inside fragments' });
  input.setAttribute('role', 'combobox'); input.setAttribute('aria-expanded', 'true'); input.setAttribute('aria-controls', 'paletteList'); input.setAttribute('aria-autocomplete', 'list');
  list.id = 'paletteList'; list.setAttribute('role', 'listbox'); list.setAttribute('aria-label', 'Results');
  pane.setAttribute('aria-label', 'Preview');
  const foot = el('p', 'palette-foot');
  foot.append(el('kbd', '', '↑↓'), ' move ', el('kbd', '', '↵'), ' open ', el('kbd', '', `${/Mac/.test(navigator.platform) ? '⌘' : 'Ctrl'} ↵`), ' open beside ', el('kbd', '', 'PgUp PgDn'), ' scroll preview ', el('kbd', '', 'Esc'), ' close ',
    el('kbd', '', '>'), ' commands ', el('kbd', '', '/'), ' search inside fragments');
  bar.append(chip, input);
  pane.append(head, body, blank);
  panes.append(list, pane);
  box.append(bar, panes, foot);
  root.append(box);
  document.body.append(root);
  const viewer = markdownPreview(body);

  let items = [], active = 0, back = null, mode = 'go', notice = '', pattern = '';
  function refresh() {
    const raw = input.value.trimStart();
    mode = raw.startsWith('/') ? 'grep' : raw.startsWith('>') ? 'commands' : 'go';
    box.dataset.mode = mode;
    chip.hidden = mode === 'go'; chip.textContent = mode === 'grep' ? 'Grep' : 'Commands';
    if (mode === 'grep') { searchText(raw.slice(1)); return; }
    clearTimeout(grepTimer); grepRun++; notice = '';
    const query = (mode === 'commands' ? raw.slice(1) : raw).trim();
    const cmds = commands().map(c => ({ kind: 'command', c, score: query ? scoreCommand(c, query) : 0 })).filter(x => !query || x.score > 0);
    const frags = mode === 'commands' ? [] : rank(fragments(), query).map(h => ({ kind: 'fragment', r: h.r, score: h.score || 0 }));
    // Nothing typed: recent fragments, then every command. Typed: one list, best first, commands winning ties.
    items = query ? [...cmds, ...frags].sort((a, b) => b.score - a.score || (a.kind === 'command' ? -1 : 1)).slice(0, 40) : [...frags.slice(0, 6), ...cmds];
    active = Math.min(active, Math.max(0, items.length - 1));
    render(!query);
  }

  // ── Grep: asks the vaults a moment after typing stops; a late answer to older text is dropped ──
  let grepTimer = 0, grepRun = 0;
  function searchText(text) {
    clearTimeout(grepTimer);
    const run = ++grepRun;
    pattern = text;
    if (!text.trim()) { items = []; notice = ''; render(); return; }
    if (items[0]?.kind !== 'line') { items = []; notice = 'Searching…'; render(); }
    grepTimer = setTimeout(async () => {
      let found = null, failure = null;
      try { found = await grep(text); } catch (err) { failure = err; }
      if (run !== grepRun) return;
      items = failure ? [] : found.hits.flatMap(r => r.lines.map(l => ({ kind: 'line', r, l, pattern: text, count: r.lines.length })));
      notice = failure ? `Can’t search: ${message(failure)}`
        : found.truncated ? `Showing the first ${items.length} matching lines. Type more to narrow it down.` : '';
      active = 0; render();
    }, 120);
  }

  function render(sections = false) {
    list.replaceChildren();
    let section = '', file = '';
    const heading = node => { node.setAttribute('role', 'presentation'); list.append(node); };
    items.forEach((item, i) => {
      if (item.kind === 'line') {
        const k = fragmentKey(item.r);
        if (k !== section) { // the fragment, once, above its lines
          section = k; file = '';
          const h = el('li', 'palette-file');
          h.append(typeIcon(item.r.type), el('span', 'palette-label' + (item.r.title ? '' : ' untitled'), item.r.title || 'Untitled'),
            el('span', 'palette-meta', `${item.count} ${item.count === 1 ? 'line' : 'lines'}`));
          heading(h);
        }
        const where = item.l.attachment || '';
        if (where !== file) { // an attached file, once, above the lines of its text
          file = where;
          const h = el('li', 'palette-attachment');
          h.append(icon('paperclip'), el('span', 'palette-label', where));
          heading(h);
        }
      } else if (sections) {
        const name = item.kind === 'fragment' ? 'Recent' : 'Commands';
        if (name !== section) { section = name; heading(el('li', 'palette-section', name)); }
      }
      const li = el('li', 'palette-item'); li.id = `palette-${i}`; li.setAttribute('role', 'option');
      if (item.kind === 'command') {
        li.append(item.c.glyph || icon(item.c.icon || 'command'), el('span', 'palette-label', item.c.label));
        if (item.c.keys) li.append(el('kbd', '', item.c.keys));
      } else if (item.kind === 'fragment') {
        li.append(typeIcon(item.r.type), el('span', 'palette-label' + (item.r.title ? '' : ' untitled'), item.r.title || 'Untitled'), el('span', 'palette-meta', describe(item.r)));
      } else {
        const l = item.l, pos = el('span', 'palette-pos', l.attachment ? partRef(l) : `${l.line}:${l.column}`);
        li.classList.add('palette-hit');
        li.setAttribute('aria-label', `${item.r.title || 'Untitled'}, ${l.attachment ? `${l.attachment}, ${l.label}, line ${l.line}` : `line ${l.line}, column ${l.column}`}: ${l.text.trim()}`);
        if (l.attachment) pos.title = l.label;
        li.append(pos, lineText(l));
      }
      li.onpointermove = () => { if (active !== i) { active = i; mark(); } };
      li.onclick = e => choose(i, e.ctrlKey || e.metaKey);
      list.append(li);
    });
    const none = mode === 'grep' ? (pattern.trim() && !notice ? `No fragment contains “${pattern}”` : '') : 'Nothing matches';
    if (notice || (!items.length && none)) heading(el('li', 'palette-none', notice || none));
    mark();
  }
  function mark() {
    for (const li of list.querySelectorAll('[role=option]')) li.setAttribute('aria-selected', String(li.id === `palette-${active}`));
    const current = list.querySelector(`#palette-${active}`);
    if (current) {
      input.setAttribute('aria-activedescendant', current.id);
      // The first row under a heading (a fragment's and its file's, or Recent) brings the headings into view with it.
      let above = current.previousElementSibling, top = null;
      while (above?.getAttribute('role') === 'presentation') { top = above; above = above.previousElementSibling; }
      top?.scrollIntoView({ block: 'nearest' });
      current.scrollIntoView({ block: 'nearest' });
    } else input.removeAttribute('aria-activedescendant');
    preview();
  }

  // ── Preview ── The header follows the selection at once; the body loads a moment later, so holding an
  // arrow key doesn't read every fragment it passes. Lines of the fragment already shown only move the mark.
  let shown = '', loaded = '', previewRun = 0, previewTimer = 0;
  function showBlank(...content) { head.replaceChildren(); body.hidden = true; blank.hidden = false; blank.replaceChildren(...content); }
  function preview() {
    const item = items[active];
    const id = !item ? `none:${mode}:${!!pattern.trim()}` : item.kind === 'command' ? `command:${item.c.label}`
      : `${fragmentKey(item.r)}:${item.kind === 'line' ? `${item.l.attachment ?? ''}:${item.l.part ?? ''}:${item.l.line}:${item.l.column}:${item.pattern}` : ''}`;
    if (id === shown) return;
    shown = id;
    const run = ++previewRun;
    clearTimeout(previewTimer);
    if (!item) {
      if (mode === 'grep' && !pattern.trim()) {
        showBlank(el('strong', '', 'Search inside fragments'),
          el('p', '', 'Type after / to list every line that contains the text, in fragments and in the documents attached to them, with where it is: line and column, or the page. Matching is exact text, ignoring case unless you type an uppercase letter.'));
      } else showBlank();
      return;
    }
    if (item.kind === 'command') {
      const c = item.c, title = el('p', 'pv-command');
      title.append(c.glyph ? c.glyph.cloneNode(true) : icon(c.icon || 'command'), el('strong', '', c.label));
      const how = el('p', '', 'Press Enter to run this command');
      if (c.keys) how.append(', or ', el('kbd', '', c.keys), ' from anywhere');
      how.append('.');
      showBlank(title, how);
      return;
    }
    const r = item.r, at = item.kind === 'line' ? item.l : null, fact = el('p', 'pv-facts');
    fact.append(typeIcon(r.type), ...facts(r).flatMap((f, i) => i ? [el('span', 'pv-dot', '·'), f] : [f]));
    if (at) fact.append(el('span', 'pv-dot', '·'), el('span', 'pv-pos', at.attachment ? `${at.attachment}, ${at.label}, line ${at.line}` : `Line ${at.line}, column ${at.column}`));
    head.replaceChildren(fact, el('h2', 'pv-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
    blank.hidden = true; body.hidden = false;
    // An attachment line shows the text of its part (page, slide, section), not the fragment's body.
    const source = at?.attachment ? `${fragmentKey(r)}\u0000${at.attachment}\u0000${at.part}` : fragmentKey(r);
    const same = loaded === source;
    if (!same) body.dataset.loading = '';
    previewTimer = setTimeout(async () => {
      let text;
      try { text = at?.attachment ? await loadPart(r, at.attachment, at.part) : (await load(r)).body; } catch (err) {
        if (run === previewRun) { loaded = ''; showBlank(el('p', '', `Can’t show this ${at?.attachment ? 'part of the file' : 'fragment'}: ${message(err)}`)); }
        return;
      }
      if (run !== previewRun) return;
      const current = viewer.show((text || '').replace(/^\n/, ''), { links: linksFor(r), pattern: at ? item.pattern : '', line: at?.line, column: at?.column });
      loaded = source; delete body.dataset.loading;
      if (current) viewer.scrollTo(current.from); else if (!same) pane.scrollTop = 0;
    }, same ? 0 : 70);
  }

  function choose(i, side = false) {
    const item = items[i];
    if (!item) return;
    close(false);
    if (item.kind === 'command') item.c.run();
    else open(item.r, item.kind === 'line' ? { line: item.l.line, column: item.l.column, pattern: item.pattern, attachment: item.l.attachment, part: item.l.part } : undefined, { side });
  }
  function close(restore = true) {
    if (root.hidden) return;
    root.hidden = true; clearTimeout(grepTimer); grepRun++; clearTimeout(previewTimer); previewRun++;
    if (restore && back?.isConnected) back.focus();
  }
  input.addEventListener('input', () => { active = 0; refresh(); });
  input.addEventListener('keydown', e => {
    const step = { ArrowDown: 1, ArrowUp: -1 }[e.key], page = { PageDown: 1, PageUp: -1 }[e.key];
    if (step && items.length) { e.preventDefault(); active = (active + step + items.length) % items.length; mark(); }
    else if (page) { e.preventDefault(); pane.scrollBy({ top: page * pane.clientHeight * 0.85 }); }
    else if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); choose(active, e.ctrlKey || e.metaKey); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
  });
  // Focus stays in the box: pressing on a row, the preview or the scrollbars doesn't take it from the input.
  // A link in the preview handles its own press (it opens that fragment).
  box.addEventListener('pointerdown', e => { if (e.target !== input && !e.target.closest('.md-wiki')) e.preventDefault(); });
  root.addEventListener('pointerdown', e => { if (e.target === root) close(); });
  box.addEventListener('focusout', e => { if (!box.contains(e.relatedTarget)) setTimeout(() => { if (!box.contains(document.activeElement)) close(false); }); });
  return {
    open(prefix = '') {
      if (root.hidden) back = document.activeElement;
      root.hidden = false; input.value = prefix; active = 0; shown = ''; loaded = ''; refresh(); input.focus();
    },
    close,
    get isOpen() { return !root.hidden; }
  };
}
