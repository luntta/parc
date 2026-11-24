// Tabs and groups, as in a code editor. The work area holds one or more groups side by side; each has a strip of
// tabs and shows one of them. A tab is the app's own object: the layout gives it `group`, `node` (its tab in the
// strip) and `preview`, and shows its `panel`. A group holds at most one preview tab: opening another preview
// there replaces it, and keep() (editing, a double-click, moving it) makes it stay. One group is current: the
// app's commands act on its shown tab.
//
// Tabs move by drag and drop: along a strip, onto another group's strip or body, or onto a body's left or right
// edge to open a new group on that side. A fragment dragged from the list (FRAGMENT data) opens the same way.
import { icon } from './icons.js';

export const FRAGMENT = 'application/x-parc-fragment';
const TAB = 'application/x-parc-tab', MAX_GROUPS = 4, MIN_WIDTH = 260;
const mod = /Mac/.test(navigator.platform) ? '⌘' : 'Ctrl';

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

/**
 * host: the element that holds the groups. hooks:
 *   label(tab) → { text, glyph (Node), title, dirty }: what its tab says.
 *   canClose(tab) → Promise<boolean>: asked before a tab closes (unsaved work).
 *   closed(tab): the tab left the layout for good.
 *   activate(tab | null, { focus }): the current group, or the tab it shows, changed (null: that group is empty).
 *   changed(): the layout changed (to remember it).
 *   drop(data, { group, index }): a fragment from the list was dropped, to open there.
 *   menu(tab) → [{ label, run, keys?, disabled? } | null]: the app's entries in a tab's context menu.
 *   empty(group) → Node: what an empty group shows.
 *   busy() → boolean: tabs don't switch from the pointer or keyboard while true.
 */
export function workspace(host, hooks) {
  const groups = [];
  let current = null, seq = 0, dragged = null, menu = null;
  const busy = () => !!hooks.busy?.();
  const changed = () => hooks.changed?.();

  // ── Groups ──
  function addGroup(at = groups.length, size = 1) {
    const g = { id: ++seq, tabs: [], shown: null, mru: [], size };
    g.node = el('section', 'pane');
    g.strip = el('div', 'tabstrip'); g.list = el('div', 'tabs'); g.body = el('div', 'pane-body');
    g.empty = el('div', 'pane-empty'); g.overlay = el('div', 'drop-overlay');
    g.list.setAttribute('role', 'tablist');
    const split = el('button', 'tab-action'); split.type = 'button';
    split.setAttribute('aria-label', 'Split right'); split.title = `Split right: a new group beside this one (${mod}+Alt+→ moves the tab there)`;
    split.append(icon('split'));
    split.onclick = () => { if (!busy()) { const n = splitGroup(g, 'right'); if (n) focusGroup(groups.indexOf(n)); } };
    g.strip.append(g.list, split);
    g.body.append(g.empty, g.overlay);
    g.node.append(g.strip, g.body);
    wireGroup(g);
    groups.splice(at, 0, g);
    host.insertBefore(g.node, groups[at + 1]?.node ?? null);
    sync();
    return g;
  }
  // Its space goes to the group on its left (or right). The caller decides what becomes current.
  function dropGroup(g) {
    const i = groups.indexOf(g);
    if (i < 0 || groups.length < 2) return;
    groups.splice(i, 1); g.node.remove();
    (groups[i - 1] ?? groups[i]).size += g.size;
    if (current === g) current = null;
    sync();
  }
  function setCurrent(g) {
    if (current === g) return;
    current = g;
    sync();
  }
  // Clicking or focusing anything in a group makes it current.
  function wireGroup(g) {
    const enter = () => {
      if (current === g || busy() || !groups.includes(g)) return;
      setCurrent(g); hooks.activate(g.shown, {}); changed();
    };
    g.node.addEventListener('pointerdown', enter);
    g.node.addEventListener('focusin', enter);
    g.list.addEventListener('keydown', e => keys(g, e));
    g.list.addEventListener('wheel', e => {
      if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
      g.list.scrollLeft += e.deltaY; e.preventDefault();
    }, { passive: false });
    g.list.addEventListener('dragover', e => {
      const kind = carries(e);
      if (!kind) return;
      e.preventDefault(); e.dataTransfer.dropEffect = kind === 'tab' ? 'move' : 'copy';
      mark(g, stripIndex(g, e.clientX));
    });
    g.list.addEventListener('dragleave', e => { if (!g.list.contains(e.relatedTarget)) unmark(); });
    g.list.addEventListener('drop', e => {
      const kind = carries(e);
      if (!kind) return;
      e.preventDefault();
      const at = stripIndex(g, e.clientX), t = dragged;
      endDrag();
      if (kind === 'tab') { if (t) move(t, g, at); } else dropFragment(e, { group: g, index: at });
    });
    g.overlay.addEventListener('dragover', e => {
      const kind = carries(e), zone = kind && zoneAt(g, e, kind);
      if (!zone) { delete g.overlay.dataset.zone; return; }
      e.preventDefault(); e.dataTransfer.dropEffect = kind === 'tab' ? 'move' : 'copy';
      g.overlay.dataset.zone = zone;
    });
    g.overlay.addEventListener('dragleave', e => { if (!g.overlay.contains(e.relatedTarget)) delete g.overlay.dataset.zone; });
    g.overlay.addEventListener('drop', e => {
      const kind = carries(e), zone = kind && zoneAt(g, e, kind), t = dragged;
      endDrag();
      if (!zone) return;
      e.preventDefault();
      if (kind === 'tab') { if (zone === 'center') move(t, g); else splitGroup(g, zone, t); return; }
      const target = zone === 'center' ? g : splitGroup(g, zone);
      if (target) dropFragment(e, { group: target });
    });
  }

  // Groups share the width by their sizes; the line between two drags (or arrow keys) to resize them, and a
  // double-click shares it equally again.
  function sash(left, right) {
    const s = el('div', 'sash');
    s.setAttribute('role', 'separator'); s.setAttribute('aria-orientation', 'vertical'); s.setAttribute('aria-label', 'Resize groups'); s.tabIndex = 0;
    const resize = x => {
      const a = left.node.getBoundingClientRect(), b = right.node.getBoundingClientRect(), total = a.width + b.width;
      const min = Math.min(MIN_WIDTH, total / 2), w = Math.max(min, Math.min(total - min, x - a.left)), sum = left.size + right.size;
      left.size = sum * w / total; right.size = sum - left.size;
      sizes();
    };
    s.addEventListener('pointerdown', e => {
      if (e.button !== 0) return;
      s.setPointerCapture(e.pointerId); host.dataset.resizing = '';
      const moveTo = ev => resize(ev.clientX);
      const end = () => { s.removeEventListener('pointermove', moveTo); delete host.dataset.resizing; changed(); };
      s.addEventListener('pointermove', moveTo);
      s.addEventListener('pointerup', end, { once: true }); s.addEventListener('pointercancel', end, { once: true });
    });
    s.addEventListener('keydown', e => {
      const step = { ArrowLeft: -32, ArrowRight: 32 }[e.key];
      if (!step) return;
      e.preventDefault(); resize(left.node.getBoundingClientRect().right + step); changed();
    });
    s.addEventListener('dblclick', () => { for (const g of groups) g.size = 1; sizes(); changed(); });
    return s;
  }
  function sizes() {
    const total = groups.reduce((n, g) => n + g.size, 0) || 1;
    groups.forEach((g, i) => {
      g.node.style.flex = `${g.size} 1 0px`;
      const s = g.node.previousElementSibling;
      if (i && s?.classList.contains('sash')) s.setAttribute('aria-valuenow', String(Math.round(100 * groups.slice(0, i).reduce((n, x) => n + x.size, 0) / total)));
    });
  }

  // ── Tabs ──
  function tabNode(t) {
    const n = el('div', 'tab'), close = el('button', 'tab-close');
    n.id = `tab-${t.key}`; n.setAttribute('role', 'tab'); n.draggable = true; n.tabIndex = -1;
    t.panel.id ||= `panel-${t.key}`;
    t.panel.setAttribute('role', 'tabpanel'); t.panel.setAttribute('aria-labelledby', n.id);
    n.setAttribute('aria-controls', t.panel.id);
    close.type = 'button'; close.tabIndex = -1; close.append(icon('x'));
    n.append(el('span', 'tab-glyph'), el('span', 'tab-label'), close);
    n.addEventListener('mousedown', e => { if (e.button === 1) e.preventDefault(); }); // no autoscroll
    n.addEventListener('click', e => { if (!e.target.closest('.tab-close') && !busy()) activate(t, { focus: true }); });
    n.addEventListener('dblclick', e => { if (!e.target.closest('.tab-close')) keep(t); });
    n.addEventListener('auxclick', e => { if (e.button === 1) { e.preventDefault(); closeTab(t); } });
    close.addEventListener('click', e => { e.stopPropagation(); closeTab(t); });
    n.addEventListener('contextmenu', e => { e.preventDefault(); openMenu(t, e.clientX, e.clientY); });
    n.addEventListener('dragstart', e => {
      if (busy()) { e.preventDefault(); return; }
      dragged = t; e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData(TAB, String(t.key));
      host.dataset.dragging = '';
    });
    n.addEventListener('dragend', endDrag);
    return n;
  }
  function render(t) {
    const n = t.node;
    if (!n) return;
    const { text, glyph, title, dirty } = hooks.label(t), shown = t.group?.shown === t;
    n.querySelector('.tab-glyph').replaceChildren(...(glyph ? [glyph] : []));
    n.querySelector('.tab-label').textContent = text;
    n.title = title || text;
    n.setAttribute('aria-label', `${text}${dirty ? ', unsaved' : ''}${t.preview ? ', preview' : ''}`);
    n.toggleAttribute('data-dirty', !!dirty); n.toggleAttribute('data-preview', !!t.preview);
    n.setAttribute('aria-selected', String(shown)); n.tabIndex = shown ? 0 : -1;
    const close = n.querySelector('.tab-close');
    close.title = `${dirty ? 'Unsaved changes. ' : ''}Close (${mod}+W)`; close.setAttribute('aria-label', `Close ${text}`);
  }
  // Every group's tabs, panels, empty state and the lines between them, from the model.
  function sync() {
    if (!groups.length) return;
    if (!groups.includes(current)) current = null;
    host.querySelectorAll(':scope > .sash').forEach(s => s.remove());
    groups.forEach((g, i) => {
      if (i) host.insertBefore(sash(groups[i - 1], g), g.node);
      g.node.toggleAttribute('data-active', g === current);
      g.node.setAttribute('aria-label', groups.length > 1 ? `Group ${i + 1} of ${groups.length}` : 'Tabs');
      g.list.setAttribute('aria-label', groups.length > 1 ? `Open tabs, group ${i + 1}` : 'Open tabs');
      for (const t of g.tabs) {
        t.panel.hidden = g.shown !== t;
        t.panel.toggleAttribute('data-current', g === current && g.shown === t);
        render(t);
      }
      g.empty.hidden = g.tabs.length > 0;
      if (!g.tabs.length && !g.empty.childElementCount) g.empty.append(hooks.empty(g));
    });
    sizes();
  }
  function attach(t, g, at) {
    t.group = g; g.tabs.splice(at, 0, t);
    t.node ??= tabNode(t);
    g.list.insertBefore(t.node, g.tabs[at + 1]?.node ?? null);
    t.panel.hidden = true; g.body.insertBefore(t.panel, g.empty);
  }
  function detach(t) {
    const g = t.group;
    if (!g) return;
    g.tabs.splice(g.tabs.indexOf(t), 1); g.mru = g.mru.filter(x => x !== t);
    if (g.shown === t) g.shown = null;
    t.node.remove(); t.panel.remove(); t.group = null;
  }
  const holdsFocus = t => !!(t.panel?.contains(document.activeElement) || t.node?.contains(document.activeElement));
  // A group with nothing shown shows its most recently used tab.
  function backfill(g) { if (!g.shown && g.tabs.length) { g.shown = g.mru[0] ?? g.tabs[0]; g.mru = [g.shown, ...g.mru.filter(x => x !== g.shown)]; } }

  /** Puts a tab in a group (the current one by default) after its shown tab, or at `index`. */
  function open(t, { group, preview = false, index, activate: show = true, focus = false } = {}) {
    const g = group ?? current ?? groups[0] ?? addGroup();
    t.key ??= ++seq; t.preview = preview;
    let at = index ?? (g.shown ? g.tabs.indexOf(g.shown) + 1 : g.tabs.length);
    const old = preview && g.tabs.find(x => x.preview && x !== t);
    if (old) { at = g.tabs.indexOf(old); detach(old); hooks.closed(old); }
    attach(t, g, Math.min(at, g.tabs.length));
    if (show) activate(t, { focus });
    else { backfill(g); sync(); changed(); }
    return t;
  }
  /** Shows a tab in its group and makes that group current. */
  function activate(t, { focus = false } = {}) {
    const g = t?.group;
    if (!g) return false;
    g.shown = t; g.mru = [t, ...g.mru.filter(x => x !== t)];
    current = g;
    sync();
    t.node.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    hooks.activate(t, { focus });
    changed();
    return true;
  }
  /** Removes a tab without asking. Its group shows the tab used before it, or goes if it was its last. */
  function remove(t) {
    const g = t.group;
    if (!g) return;
    const wasShown = g.shown === t, focus = holdsFocus(t), at = g.tabs.indexOf(t);
    detach(t); hooks.closed(t);
    if (!g.tabs.length && groups.length > 1) {
      const wasCurrent = current === g, i = groups.indexOf(g);
      dropGroup(g);
      if (wasCurrent) { const next = groups[Math.max(0, i - 1)]; current = next; sync(); hooks.activate(next.shown, { focus }); }
    } else if (wasShown) {
      g.shown = g.mru[0] ?? g.tabs[Math.min(at, g.tabs.length - 1)] ?? null;
      if (g.shown) g.mru = [g.shown, ...g.mru.filter(x => x !== g.shown)];
      sync();
      if (current === g) hooks.activate(g.shown, { focus });
    } else sync();
    changed();
  }
  /** Closes a tab once the app agrees (it asks about unsaved work). Resolves true when it closed. */
  async function close(t) {
    if (!t?.group) return true;
    if (!(await hooks.canClose(t))) return false;
    remove(t);
    return true;
  }
  const closeTab = t => { if (!busy()) close(t); };
  async function closeAll(list) { for (const t of list) if (!(await close(t))) return false; return true; }
  function keep(t) { if (!t.preview) return; t.preview = false; render(t); changed(); }
  /** Moves a tab to `g` at `index` (the end by default), keeps it, and shows it there. */
  function move(t, g, index = g.tabs.length) {
    const from = t.group;
    if (!from || !groups.includes(g)) return;
    if (from === g) { const i = g.tabs.indexOf(t); if (index > i) index--; if (index === i) { activate(t, { focus: true }); return; } }
    detach(t); t.preview = false;
    attach(t, g, Math.min(index, g.tabs.length));
    if (from !== g) { if (!from.tabs.length && groups.length > 1) dropGroup(from); else backfill(from); }
    activate(t, { focus: true });
  }
  /** A new group beside `from`, holding `t` if given (moved there), otherwise empty and current. */
  function splitGroup(from = current, side = 'right', t = null) {
    if (!from || groups.length >= MAX_GROUPS || (t && t.group === from && from.tabs.length === 1)) return null;
    const i = groups.indexOf(from);
    from.size /= 2;
    const g = addGroup(side === 'right' ? i + 1 : i, from.size);
    if (t) move(t, g, 0);
    else { current = g; sync(); hooks.activate(null, {}); changed(); }
    return g;
  }
  /** The tab goes to the group on that side (step ±1), or to a new group there when it has company here. */
  function toNeighbour(t, step) {
    const next = groups[groups.indexOf(t.group) + step];
    if (next) { move(t, next); return next; }
    return splitGroup(t.group, step > 0 ? 'right' : 'left', t);
  }
  /** Makes group i current and puts focus in it; one past the last group opens a new one there. */
  function focusGroup(i) {
    if (i === groups.length && current) { const g = splitGroup(groups.at(-1), 'right'); if (!g) return; i = groups.indexOf(g); }
    const g = groups[i];
    if (!g) return;
    if (g.shown) { activate(g.shown, { focus: true }); return; }
    current = g; sync(); hooks.activate(null, { focus: true }); changed();
  }
  /** The next (step 1) or previous (-1) tab of the current group. */
  function cycle(step) {
    const g = current;
    if (!g?.tabs.length) return;
    const i = g.tabs.indexOf(g.shown);
    activate(g.tabs[(i + step + g.tabs.length) % g.tabs.length], { focus: true });
  }

  // ── Keys in a tab strip: arrows and Home/End move between tabs, Delete closes, the menu key opens the menu ──
  function keys(g, e) {
    const i = g.tabs.findIndex(t => t.node === document.activeElement), n = g.tabs.length;
    if (i < 0) return;
    const to = { ArrowLeft: i - 1, ArrowRight: i + 1, Home: 0, End: n - 1 }[e.key];
    if (to !== undefined) {
      e.preventDefault();
      if (busy()) return;
      const t = g.tabs[(to + n) % n];
      activate(t); t.node.focus();
    } else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); if (!busy()) activate(g.tabs[i], { focus: true }); }
    else if (e.key === 'Delete') { e.preventDefault(); closeTab(g.tabs[i]); }
    else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      e.preventDefault();
      const r = g.tabs[i].node.getBoundingClientRect();
      openMenu(g.tabs[i], r.left, r.bottom);
    }
  }

  // ── Drag and drop ──
  const carries = e => { const types = e.dataTransfer?.types || []; return types.includes(TAB) ? (dragged ? 'tab' : null) : types.includes(FRAGMENT) ? 'fragment' : null; };
  host.addEventListener('dragenter', e => { if (carries(e)) host.dataset.dragging = ''; });
  document.addEventListener('dragend', endDrag);
  function endDrag() {
    dragged = null; delete host.dataset.dragging; unmark();
    for (const g of groups) delete g.overlay.dataset.zone;
  }
  function stripIndex(g, x) {
    const i = g.tabs.findIndex(t => { const r = t.node.getBoundingClientRect(); return x < r.left + r.width / 2; });
    return i < 0 ? g.tabs.length : i;
  }
  function mark(g, at) {
    unmark();
    const n = g.tabs[at]?.node;
    if (n) n.dataset.dropBefore = ''; else g.list.dataset.dropEnd = '';
  }
  function unmark() {
    host.querySelectorAll('[data-drop-before]').forEach(n => delete n.dataset.dropBefore);
    host.querySelectorAll('[data-drop-end]').forEach(n => delete n.dataset.dropEnd);
  }
  // The left or right third of a body opens a new group on that side; the middle drops into the group.
  function zoneAt(g, e, kind) {
    const r = g.body.getBoundingClientRect(), x = (e.clientX - r.left) / r.width;
    const edge = groups.length < MAX_GROUPS ? (x < 0.3 ? 'left' : x > 0.7 ? 'right' : null) : null;
    if (kind !== 'tab') return edge || 'center';
    if (!dragged) return null;
    if (edge && !(dragged.group === g && g.tabs.length === 1)) return edge;
    return dragged.group === g ? null : 'center';
  }
  function dropFragment(e, where) {
    let data = null;
    try { data = JSON.parse(e.dataTransfer.getData(FRAGMENT)); } catch { return; }
    if (data?.vault && data?.id) hooks.drop(data, where);
  }

  // ── A tab's context menu ──
  function openMenu(t, x, y) {
    closeMenu();
    if (!t.group) return;
    const g = t.group, i = g.tabs.indexOf(t), others = g.tabs.filter(o => o !== t), right = g.tabs.slice(i + 1);
    const at = groups.indexOf(g), full = groups.length >= MAX_GROUPS, alone = g.tabs.length === 1;
    const entries = [
      { label: 'Close', keys: `${mod}+W`, run: () => close(t) },
      { label: 'Close others', run: () => closeAll(others), disabled: !others.length },
      { label: 'Close to the right', run: () => closeAll(right), disabled: !right.length },
      { label: 'Close saved', run: () => closeAll(g.tabs.filter(o => !hooks.label(o).dirty)) },
      { label: 'Close all', run: () => closeAll([...g.tabs]) },
      null,
      ...(t.preview ? [{ label: 'Keep open', run: () => keep(t) }] : []),
      { label: groups[at + 1] ? 'Move to the right group' : 'Move to a new group on the right', keys: `${mod}+Alt+→`, run: () => toNeighbour(t, 1), disabled: !groups[at + 1] && (full || alone) },
      { label: groups[at - 1] ? 'Move to the left group' : 'Move to a new group on the left', keys: `${mod}+Alt+←`, run: () => toNeighbour(t, -1), disabled: !groups[at - 1] && (full || alone) },
      ...(() => { const extra = hooks.menu?.(t) || []; return extra.length ? [null, ...extra] : []; })()
    ];
    const box = el('ul', 'tab-menu'), buttons = [];
    box.setAttribute('role', 'menu'); box.setAttribute('aria-label', `${hooks.label(t).text} tab`);
    for (const entry of entries) {
      if (!entry) { const sep = el('li', 'tab-menu-sep'); sep.setAttribute('role', 'separator'); box.append(sep); continue; }
      const li = el('li'), b = el('button', 'tab-menu-item'); li.setAttribute('role', 'none');
      b.type = 'button'; b.setAttribute('role', 'menuitem'); b.tabIndex = -1; b.disabled = !!entry.disabled;
      b.append(el('span', '', entry.label));
      if (entry.keys) b.append(el('kbd', '', entry.keys));
      b.onclick = () => { closeMenu(); if (!busy()) entry.run(); };
      li.append(b); box.append(li);
      if (!b.disabled) buttons.push(b);
    }
    box.addEventListener('keydown', e => {
      const k = buttons.indexOf(document.activeElement), step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
      if (step) { e.preventDefault(); buttons[(k + step + buttons.length) % buttons.length]?.focus(); }
      else if (e.key === 'Home' || e.key === 'End') { e.preventDefault(); buttons.at(e.key === 'Home' ? 0 : -1)?.focus(); }
      else if (e.key === 'Escape' || e.key === 'Tab') { e.preventDefault(); e.stopPropagation(); closeMenu(); t.node?.focus(); }
    });
    box.addEventListener('focusout', e => { if (!box.contains(e.relatedTarget)) setTimeout(() => { if (!box.contains(document.activeElement)) closeMenu(); }); });
    document.body.append(box);
    const r = box.getBoundingClientRect();
    box.style.left = `${Math.max(4, Math.min(x, innerWidth - r.width - 4))}px`;
    box.style.top = `${Math.max(4, Math.min(y, innerHeight - r.height - 4))}px`;
    menu = box;
    const away = e => { if (!box.contains(e.target)) closeMenu(); };
    setTimeout(() => document.addEventListener('pointerdown', away, true));
    box.cleanup = () => document.removeEventListener('pointerdown', away, true);
    buttons[0]?.focus();
  }
  function closeMenu() { if (!menu) return; menu.cleanup(); menu.remove(); menu = null; }
  window.addEventListener('blur', closeMenu);
  window.addEventListener('resize', closeMenu);

  return {
    open, activate, close, remove, closeAll, keep, move, toNeighbour, focusGroup, cycle, addGroup,
    split: splitGroup, update: render, refresh: sync,
    /** Closes the current group when it is empty and not the only one; the group on its left becomes current. */
    closeEmpty() {
      const g = current;
      if (!g || g.tabs.length || groups.length < 2) return;
      const i = groups.indexOf(g);
      dropGroup(g); current = groups[Math.max(0, i - 1)]; sync();
      hooks.activate(current.shown, { focus: true }); changed();
    },
    /** Drops empty groups beyond the first (after restoring a layout whose tabs couldn't all come back). */
    prune() { for (const g of [...groups]) if (!g.tabs.length && groups.length > 1) dropGroup(g); for (const g of groups) backfill(g); current ??= groups[0]; sync(); },
    get groups() { return groups; },
    get group() { return current; },
    get tab() { return current?.shown ?? null; },
    tabs: () => groups.flatMap(g => g.tabs),
    shown: t => !!t?.group && t.group.shown === t,
    get menuOpen() { return !!menu; }
  };
}
