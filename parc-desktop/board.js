// A type's own view: a board with a lane per status value, a risk matrix (likelihood × impact) for types that
// have both fields, or a grid of cards for types without a status. Everything comes from the type's schema, so
// a renamed status, an added value or a new type with these fields gets the same views.
//
// Cards move by drag and drop or from the keyboard (Shift+arrows); `ctx.move` saves the change and re-renders.
import { icon, statusIcon } from './icons.js';

function el(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
const capital = s => s ? s[0].toUpperCase() + s.slice(1).replace(/[-_]/g, ' ') : '';
const SETTLED = ['done', 'closed'];
const SHOWN_SETTLED = 8;      // a done or closed lane shows its latest few until asked for the rest
const SHOWN_IN_CELL = 4;      // a matrix cell likewise
const expanded = new Set();   // lanes and cells showing everything, for this session

// ── Severity: where a likelihood × impact pair sits on the schema's own scales ──
// The product of both positions (1-based, as a share of each scale), so a 3×3 matrix reads
// 9 Severe, 6 Major, 4 and 3 Moderate, 2 and 1 Minor, and a longer scale keeps the same shape.
const BANDS = [[0.85, 'severe', 'Severe'], [0.55, 'major', 'Major'], [0.3, 'moderate', 'Moderate'], [0, 'minor', 'Minor']];
export function severity(axes, fields = {}) {
  const li = axes.likelihood.indexOf(fields.likelihood), ii = axes.impact.indexOf(fields.impact);
  if (li < 0 || ii < 0) return null;
  const score = (li + 1) / axes.likelihood.length * ((ii + 1) / axes.impact.length);
  const [, level, label] = BANDS.find(([min]) => score >= min);
  return { score, level, label, li, ii };
}
function severityPill(s, className = 'sev') {
  const pill = el('span', className, s.label);
  pill.dataset.level = s.level;
  return pill;
}

// ── Cards ──
// Title, then what the schema makes worth seeing at a glance: high priority, a due date (overdue in red),
// severity, a couple of tags, and the age. The words behind marks are in the tooltip and accessible name.
function card(r, ctx, { mark = false, compact = false } = {}) {
  const b = el('button', compact ? 'card chip-card' : 'card'); b.type = 'button';
  b.dataset.key = ctx.key(r);
  const category = r.status ? ctx.tone(r) : '';
  if (mark && r.status) b.append(statusIcon(category, 'icon card-mark status'));
  b.append(el('span', 'card-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
  const facts = [], f = r.fields || {};
  if (!compact) {
    const foot = el('span', 'card-foot');
    const sev = ctx.axes && severity(ctx.axes, f);
    if (sev) { foot.append(severityPill(sev)); facts.push(`${sev.label}: likelihood ${f.likelihood}, impact ${f.impact}`); }
    const priorities = ctx.values('priority');
    if (f.priority && priorities.indexOf(f.priority) >= Math.ceil(priorities.length / 2)) { // the upper half of the scale
      const p = el('span', 'card-fact priority'); p.append(icon('flag'), capital(f.priority));
      foot.append(p); facts.push(`${capital(f.priority)} priority`);
    }
    if (f.due) {
      const late = ctx.overdue(r), d = el('span', 'card-fact due');
      d.append(icon('calendar'), ctx.dayLabel(f.due));
      if (late) d.dataset.overdue = '';
      foot.append(d); facts.push(`Due ${ctx.dayLabel(f.due)}${late ? ', overdue' : ''}`);
    }
    for (const t of (r.tags || []).slice(0, 2)) foot.append(el('span', 'card-tag', `#${t}`));
    const end = el('span', 'card-end');
    if (ctx.many) { const dot = el('span', 'vault-dot'); dot.style.setProperty('--vault-hue', ctx.hue(r.vault)); end.append(dot); }
    end.append(ctx.time(r));
    foot.append(end);
    b.append(foot);
  }
  const words = [ctx.many && ctx.vaultName(r.vault), r.status && capital(r.status), ...facts].filter(Boolean);
  b.title = [r.title || 'Untitled', [...words, ...(r.tags || []).map(t => `#${t}`)].join(' · ')].filter(Boolean).join('\n');
  b.append(el('span', 'sr', `, ${words.join(', ')}`));
  if (ctx.current === b.dataset.key) b.setAttribute('aria-current', 'true');
  b.onclick = e => ctx.open(r, e.detail === 0);
  if (ctx.movable) {
    b.draggable = true;
    b.addEventListener('dragstart', e => {
      drag = r; e.dataTransfer.effectAllowed = 'move'; e.dataTransfer.setData('text/plain', r.title || 'Untitled');
      requestAnimationFrame(() => b.dataset.dragging = '');
      b.closest('.type-view')?.setAttribute('data-dragging', '');
    });
    b.addEventListener('dragend', () => {
      drag = null; delete b.dataset.dragging;
      const view = b.closest('.type-view');
      view?.removeAttribute('data-dragging');
      view?.querySelectorAll('[data-over]').forEach(n => delete n.dataset.over);
    });
  }
  return b;
}
let drag = null; // the row being dragged
export const dragging = () => !!drag;

// A drop zone takes a card from this view and hands its new fields to `ctx.move`.
function dropZone(zone, fieldsFor, ctx) {
  zone.addEventListener('dragover', e => {
    if (!drag) return;
    e.preventDefault(); e.dataTransfer.dropEffect = 'move';
    zone.dataset.over = '';
  });
  zone.addEventListener('dragleave', e => { if (!zone.contains(e.relatedTarget)) delete zone.dataset.over; });
  zone.addEventListener('drop', e => {
    if (!drag) return;
    e.preventDefault(); delete zone.dataset.over;
    const r = drag; drag = null;
    ctx.move(r, fieldsFor(r));
  });
}
function more(id, hidden, label) {
  const b = el('button', 'show-more', expanded.has(id) ? 'Show fewer' : `${label} ${hidden.toLocaleString()} more`); b.type = 'button';
  b.setAttribute('aria-expanded', String(expanded.has(id)));
  b.onclick = () => { if (expanded.has(id)) expanded.delete(id); else expanded.add(id); b.dispatchEvent(new CustomEvent('board-rerender', { bubbles: true })); };
  return b;
}

// ── Board: a lane per status value, in the schema's order ──
// A status no schema value names (an edit made elsewhere) still gets a lane, so no fragment is hidden.
function lanesOf(ctx) {
  const values = [...ctx.status.values];
  for (const r of ctx.items) if (r.status && !values.includes(r.status)) values.push(r.status);
  const lanes = values.map(value => ({ value, category: ctx.status.categories?.[value] || 'open', known: ctx.status.values.includes(value), items: [] }));
  const none = { value: null, category: 'open', known: false, items: [] };
  for (const r of ctx.order(ctx.items)) (lanes.find(l => l.value === r.status) || none).items.push(r);
  return none.items.length ? [...lanes, none] : lanes;
}
function board(ctx) {
  const wrap = el('div', 'board'), lanes = lanesOf(ctx);
  wrap.style.setProperty('--lanes', lanes.length);
  lanes.forEach((lane, i) => {
    const col = el('section', 'lane'), head = el('header', 'lane-head'), body = el('ul', 'lane-body');
    const label = lane.value == null ? 'No status' : capital(lane.value), id = `lane:${ctx.type}:${lane.value}`;
    col.dataset.status = lane.value ?? ''; col.dataset.tone = lane.category; col.dataset.lane = i;
    col.setAttribute('aria-label', `${label}, ${lane.items.length}`);
    head.append(statusIcon(lane.category, 'icon status'), el('h2', 'lane-name', label), el('span', 'lane-count', lane.items.length.toLocaleString()));
    if (lane.known && ctx.create) {
      const add = el('button', 'lane-add'); add.type = 'button';
      add.title = `New ${ctx.type} in ${label}`; add.setAttribute('aria-label', add.title);
      add.append(icon('plus')); add.onclick = () => ctx.create({ status: lane.value });
      head.append(add);
    }
    const capped = SETTLED.includes(lane.category) && lane.items.length > SHOWN_SETTLED && !expanded.has(id);
    const shown = capped ? lane.items.slice(0, SHOWN_SETTLED) : lane.items;
    body.append(...shown.map(r => { const li = el('li'); li.append(card(r, ctx)); return li; }));
    if (!lane.items.length) body.append(el('li', 'lane-empty', ctx.movable && lane.known ? 'Drop here' : 'None'));
    col.append(head, body);
    if (SETTLED.includes(lane.category) && lane.items.length > SHOWN_SETTLED) col.append(more(id, lane.items.length - SHOWN_SETTLED, 'Show'));
    if (ctx.movable && lane.known) dropZone(col, () => ({ status: lane.value }), ctx);
    wrap.append(col);
  });
  // Arrows move between cards (← → to the neighbouring lane); Shift+← → moves the card itself.
  wrap.addEventListener('keydown', e => {
    const b = e.target.closest?.('.card');
    if (!b || !['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key) || e.altKey || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    const cols = [...wrap.querySelectorAll('.lane')], col = b.closest('.lane'), at = cols.indexOf(col);
    const cards = c => [...c.querySelectorAll('.card')], index = cards(col).indexOf(b);
    if (e.shiftKey) {
      if (!ctx.movable || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight')) return;
      const step = e.key === 'ArrowLeft' ? -1 : 1;
      let to = at + step;
      while (cols[to] && !lanes[to].known) to += step;
      const r = ctx.items.find(x => ctx.key(x) === b.dataset.key);
      if (cols[to] && r) ctx.move(r, { status: lanes[to].value });
      return;
    }
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') { cards(col)[index + (e.key === 'ArrowUp' ? -1 : 1)]?.focus(); return; }
    const step = e.key === 'ArrowLeft' ? -1 : 1;
    for (let to = at + step; cols[to]; to += step) {
      const there = cards(cols[to]);
      if (there.length) { there[Math.min(index, there.length - 1)].focus(); return; }
    }
  });
  return wrap;
}

// ── Cards for a type without a status ──
function grid(ctx) {
  const ul = el('ul', 'card-grid');
  ul.append(...ctx.order(ctx.items).map(r => { const li = el('li'); li.append(card(r, ctx)); return li; }));
  ul.addEventListener('keydown', e => {
    const b = e.target.closest?.('.card'), step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!b || !step || e.altKey || e.ctrlKey || e.metaKey) return;
    const all = [...ul.querySelectorAll('.card')];
    e.preventDefault(); all[all.indexOf(b) + step]?.focus();
  });
  return ul;
}

// ── Risk matrix: likelihood up the side (highest at the top), impact along the bottom ──
// Most serious sits top right. Open and active risks by default; settled ones on request. Dragging a risk to a
// cell sets both fields; Shift+arrows move it a step (↑ ↓ likelihood, ← → impact). Beside it, the assessed
// risks ranked by severity, with their mitigation, and the ones not yet assessed.
function matrix(ctx) {
  const { likelihood, impact } = ctx.axes;
  const shown = ctx.items.filter(r => ctx.settled || !SETTLED.includes(ctx.tone(r)));
  const assessed = [], unassessed = [];
  for (const r of ctx.order(shown)) (severity(ctx.axes, r.fields) ? assessed : unassessed).push(r);
  const wrap = el('div', 'risk-layout'), plot = el('div', 'matrix-wrap');
  const grid = el('div', 'matrix');
  grid.style.setProperty('--cols', impact.length); grid.style.setProperty('--rows', likelihood.length);
  grid.setAttribute('role', 'group'); grid.setAttribute('aria-label', 'Risks by likelihood and impact');
  const ylabel = el('span', 'axis-title y', 'Likelihood'), xlabel = el('span', 'axis-title x', 'Impact');
  for (let li = likelihood.length - 1; li >= 0; li--) {
    grid.append(el('span', 'axis-tick y', capital(likelihood[li])));
    for (let ii = 0; ii < impact.length; ii++) {
      const s = severity(ctx.axes, { likelihood: likelihood[li], impact: impact[ii] });
      const here = assessed.filter(r => r.fields.likelihood === likelihood[li] && r.fields.impact === impact[ii]);
      const cell = el('div', 'cell'), list = el('ul', 'cell-list'), id = `cell:${ctx.type}:${li}:${ii}`;
      cell.dataset.level = s.level; cell.dataset.li = li; cell.dataset.ii = ii;
      cell.setAttribute('role', 'group');
      cell.setAttribute('aria-label', `Likelihood ${likelihood[li]}, impact ${impact[ii]}: ${s.label}, ${here.length} ${here.length === 1 ? 'risk' : 'risks'}`);
      if (here.length) cell.append(el('span', 'cell-count', here.length.toLocaleString()));
      const capped = here.length > SHOWN_IN_CELL && !expanded.has(id);
      list.append(...(capped ? here.slice(0, SHOWN_IN_CELL) : here).map(r => { const item = el('li'); item.append(card(r, ctx, { mark: true, compact: true })); return item; }));
      cell.append(list);
      if (here.length > SHOWN_IN_CELL) cell.append(more(id, here.length - SHOWN_IN_CELL, '+'));
      if (ctx.movable) dropZone(cell, () => ({ likelihood: likelihood[li], impact: impact[ii] }), ctx);
      grid.append(cell);
    }
  }
  grid.append(el('span', 'axis-corner'), ...impact.map(v => el('span', 'axis-tick x', capital(v))));
  plot.append(ylabel, grid, xlabel);
  const left = el('div', 'risk-main');

  // Not yet assessed: a tray under the matrix to drag from. Opening one sets its fields too.
  const tray = el('section', 'tray');
  tray.append(heading('Not assessed', unassessed.length ? `${unassessed.length} without likelihood and impact` : ''));
  if (unassessed.length) {
    const ul = el('ul', 'tray-list');
    ul.append(...unassessed.map(r => { const li = el('li'); li.append(card(r, ctx, { mark: true, compact: true })); return li; }));
    tray.append(ul);
  } else tray.append(el('p', 'quiet', 'Every risk shown has a likelihood and an impact.'));

  // Ranked: what to mitigate first.
  const ranked = assessed.map(r => ({ r, s: severity(ctx.axes, r.fields) }))
    .sort((a, b) => b.s.score - a.s.score || b.s.li - a.s.li); // the order within a score is the list's
  const rank = el('section', 'ranking');
  rank.append(heading('Most serious first', ranked.length ? `${ranked.length} assessed` : ''));
  if (!ranked.length) rank.append(el('p', 'quiet', shown.length ? 'Drag a risk into the matrix, or open it and set its likelihood and impact.' : ''));
  const ol = el('ol', 'rank-list');
  for (const { r, s } of ranked) {
    const li = el('li'), b = el('button', 'rank-row'); b.type = 'button';
    b.dataset.key = ctx.key(r);
    const f = r.fields, mitigation = typeof f.mitigation === 'string' && f.mitigation.trim();
    const top = el('span', 'rank-top'), sub = el('span', 'rank-sub');
    top.append(severityPill(s), el('span', 'rank-title' + (r.title ? '' : ' untitled'), r.title || 'Untitled'));
    const state = el('span', 'rank-state'); state.append(statusIcon(ctx.tone(r), 'icon status'), capital(r.status));
    sub.append(state, el('span', '', `${capital(f.likelihood)} likelihood · ${capital(f.impact)} impact`));
    if (ctx.many) sub.append(el('span', '', ctx.vaultName(r.vault)));
    b.append(top, sub, el('span', mitigation ? 'rank-mitigation' : 'rank-mitigation none', mitigation || 'No mitigation written'));
    b.title = r.title || 'Untitled';
    if (ctx.current === b.dataset.key) b.setAttribute('aria-current', 'true');
    b.onclick = e => ctx.open(r, e.detail === 0);
    li.append(b); ol.append(li);
  }
  if (ranked.length) rank.append(ol);

  left.append(plot, tray);
  wrap.append(left, rank);

  // Shift+arrows move a risk across the matrix: ↑ ↓ likelihood, ← → impact. One not yet assessed starts at the lowest.
  wrap.addEventListener('keydown', e => {
    const b = e.target.closest?.('.card');
    if (!b || !e.shiftKey || !ctx.movable || e.altKey || e.ctrlKey || e.metaKey) return;
    const dy = { ArrowUp: 1, ArrowDown: -1 }[e.key] ?? 0, dx = { ArrowRight: 1, ArrowLeft: -1 }[e.key] ?? 0;
    if (!dx && !dy) return;
    e.preventDefault();
    const r = ctx.items.find(x => ctx.key(x) === b.dataset.key), s = r && severity(ctx.axes, r.fields);
    if (!r) return;
    const li = s ? s.li + dy : 0, ii = s ? s.ii + dx : 0;
    if (li < 0 || ii < 0 || li >= likelihood.length || ii >= impact.length) return;
    ctx.move(r, { likelihood: likelihood[li], impact: impact[ii] });
  });
  return wrap;
}
function heading(text, aside) { const h = el('h2', 'section-label', text); if (aside) h.append(el('small', '', aside)); return h; }

// The whole view: a toolbar (lens, settled risks, new), then the board, matrix or cards.
export function typeView(ctx) {
  const view = el('div', 'type-view'), bar = el('div', 'view-bar');
  view.dataset.lens = ctx.lens;
  if (ctx.lenses.length > 1) {
    const group = el('div', 'lens'); group.setAttribute('role', 'group'); group.setAttribute('aria-label', 'View');
    for (const [value, label, glyph] of ctx.lenses) {
      const b = el('button', 'lens-option'); b.type = 'button'; b.dataset.lens = value;
      b.setAttribute('aria-pressed', String(value === ctx.lens));
      b.append(icon(glyph), label); b.onclick = () => ctx.setLens(value);
      group.append(b);
    }
    bar.append(group);
  }
  if (ctx.lens === 'matrix') {
    const settledCount = ctx.items.filter(r => SETTLED.includes(ctx.tone(r))).length;
    const b = el('button', 'chip'); b.type = 'button'; b.setAttribute('aria-pressed', String(ctx.settled));
    b.append(icon('check'), `Settled ${settledCount.toLocaleString()}`);
    b.title = ctx.settled ? 'Hide settled risks (done or closed)' : 'Show settled risks (done or closed) too';
    b.onclick = () => ctx.setSettled(!ctx.settled);
    bar.append(b);
  }
  bar.append(el('span', 'view-count', `${ctx.items.length.toLocaleString()} ${ctx.items.length === 1 ? ctx.type : ctx.plural.toLowerCase()}`));
  if (ctx.create) {
    const add = document.createElement('aihio-button');
    add.setAttribute('variant', 'outline'); add.setAttribute('size', 'sm');
    add.append(icon('plus'), `New ${ctx.type}`); add.onclick = () => ctx.create({});
    bar.append(add);
  }
  view.append(bar);
  if (!ctx.items.length) {
    const p = el('div', 'empty'); p.append(el('strong', '', `No ${ctx.plural.toLowerCase()} yet`), ctx.create ? `New ${ctx.plural.toLowerCase()} you capture show up here.` : '');
    view.append(p);
  } else view.append(ctx.lens === 'matrix' ? matrix(ctx) : ctx.status ? board(ctx) : grid(ctx));
  view.addEventListener('board-rerender', () => ctx.rerender());
  return view;
}
