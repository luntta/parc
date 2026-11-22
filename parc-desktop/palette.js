// Ctrl+K: one box for fragments and commands, ranked together like the TUI's launcher. A leading ">" limits
// it to commands. Fragments rank with the [[ link picker's ranking, so both find things the same way.
import { rank, fold } from './editor.js';
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

/**
 * fragments(): rows to search. commands(): [{ label, run, icon?, glyph?, keys?, keywords? }].
 * describe(row): short facts. open(row): open a fragment.
 */
export function palette({ fragments, commands, describe, open }) {
  const root = el('div', 'palette'), box = el('div', 'palette-box'), input = el('input', 'palette-input'), list = el('ul', 'palette-list');
  root.hidden = true;
  box.setAttribute('role', 'dialog'); box.setAttribute('aria-modal', 'true'); box.setAttribute('aria-label', 'Go to a fragment or command');
  Object.assign(input, { type: 'text', autocomplete: 'off', spellcheck: false, placeholder: 'Go to a fragment, or type > for commands' });
  input.setAttribute('role', 'combobox'); input.setAttribute('aria-expanded', 'true'); input.setAttribute('aria-controls', 'paletteList'); input.setAttribute('aria-autocomplete', 'list');
  list.id = 'paletteList'; list.setAttribute('role', 'listbox'); list.setAttribute('aria-label', 'Results');
  const foot = el('p', 'palette-foot');
  foot.append(el('kbd', '', '↑↓'), ' move ', el('kbd', '', '↵'), ' open ', el('kbd', '', 'Esc'), ' close ', el('kbd', '', '>'), ' commands only');
  box.append(input, list, foot);
  root.append(box);
  document.body.append(root);

  let items = [], active = 0, back = null;
  function refresh() {
    const raw = input.value, onlyCommands = raw.trimStart().startsWith('>'), query = (onlyCommands ? raw.trimStart().slice(1) : raw).trim();
    const cmds = commands().map(c => ({ kind: 'command', c, score: query ? scoreCommand(c, query) : 0 })).filter(x => !query || x.score > 0);
    const frags = onlyCommands ? [] : rank(fragments(), query).map(h => ({ kind: 'fragment', r: h.r, score: h.score || 0 }));
    // Nothing typed: recent fragments, then every command. Typed: one list, best first, commands winning ties.
    items = query ? [...cmds, ...frags].sort((a, b) => b.score - a.score || (a.kind === 'command' ? -1 : 1)).slice(0, 40) : [...frags.slice(0, 6), ...cmds];
    active = Math.min(active, Math.max(0, items.length - 1));
    render(!query);
  }
  function render(sections) {
    list.replaceChildren();
    let section = '';
    items.forEach((item, i) => {
      const heading = sections ? (item.kind === 'fragment' ? 'Recent' : 'Commands') : '';
      if (heading && heading !== section) { section = heading; const h = el('li', 'palette-section', heading); h.setAttribute('role', 'presentation'); list.append(h); }
      const li = el('li', 'palette-item'); li.id = `palette-${i}`; li.setAttribute('role', 'option');
      li.setAttribute('aria-selected', String(i === active));
      if (item.kind === 'command') {
        li.append(item.c.glyph || icon(item.c.icon || 'command'), el('span', 'palette-label', item.c.label));
        if (item.c.keys) li.append(el('kbd', '', item.c.keys));
      } else {
        li.append(typeIcon(item.r.type), el('span', 'palette-label' + (item.r.title ? '' : ' untitled'), item.r.title || 'Untitled'), el('span', 'palette-meta', describe(item.r)));
      }
      li.onpointermove = () => { if (active !== i) { active = i; mark(); } };
      li.onpointerdown = e => e.preventDefault(); // keep focus in the box
      li.onclick = () => choose(i);
      list.append(li);
    });
    if (!items.length) { const none = el('li', 'palette-none', 'Nothing matches'); none.setAttribute('role', 'presentation'); list.append(none); }
    mark();
  }
  function mark() {
    for (const li of list.querySelectorAll('[role=option]')) li.setAttribute('aria-selected', String(li.id === `palette-${active}`));
    const current = list.querySelector(`#palette-${active}`);
    if (current) { input.setAttribute('aria-activedescendant', current.id); current.scrollIntoView({ block: 'nearest' }); }
    else input.removeAttribute('aria-activedescendant');
  }
  function choose(i) {
    const item = items[i];
    if (!item) return;
    close(false);
    if (item.kind === 'command') item.c.run(); else open(item.r);
  }
  function close(restore = true) {
    if (root.hidden) return;
    root.hidden = true;
    if (restore && back?.isConnected) back.focus();
  }
  input.addEventListener('input', () => { active = 0; refresh(); });
  input.addEventListener('keydown', e => {
    const step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
    if (step && items.length) { e.preventDefault(); active = (active + step + items.length) % items.length; mark(); }
    else if (e.key === 'Enter') { e.preventDefault(); choose(active); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
  });
  root.addEventListener('pointerdown', e => { if (e.target === root) close(); });
  box.addEventListener('focusout', e => { if (!box.contains(e.relatedTarget)) setTimeout(() => { if (!box.contains(document.activeElement)) close(false); }); });
  return {
    open(prefix = '') {
      if (root.hidden) back = document.activeElement;
      root.hidden = false; input.value = prefix; active = 0; refresh(); input.focus();
    },
    close,
    get isOpen() { return !root.hidden; }
  };
}
