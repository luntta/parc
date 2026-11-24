// The whole vault as a graph: a force layout computed once (no animation), drawn as SVG, then panned and
// zoomed by moving the viewBox. Nodes and labels counter-scale, so they keep their size at any zoom.
const NS = 'http://www.w3.org/2000/svg';
function svg(tag, attrs = {}) {
  const node = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  return node;
}
function html(tag, className = '', text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

// Links pull to `distance`, every pair repels, and a weak pull to the origin keeps components together.
// Starts on a sunflower spiral, so the same vault always lays out the same way. O(n²) per tick: ticks
// shrink as the graph grows to keep a large vault under a second.
export function layout(nodes, links, { distance = 64, charge = -150, gravity = 0.05 } = {}) {
  const n = nodes.length;
  nodes.forEach((d, i) => {
    const r = 12 * Math.sqrt(i + 0.5), a = i * Math.PI * (3 - Math.sqrt(5));
    d.x = r * Math.cos(a); d.y = r * Math.sin(a); d.vx = 0; d.vy = 0;
  });
  if (n < 2) return nodes;
  const at = new Map(nodes.map(d => [d.key, d]));
  const degree = new Map();
  for (const l of links) for (const k of [l.source, l.target]) degree.set(k, (degree.get(k) || 0) + 1);
  const pairs = links.map(l => [at.get(l.source), at.get(l.target)]).filter(([s, t]) => s && t);
  const ticks = Math.max(80, Math.min(300, Math.round(3e7 / (n * n))));
  const decay = 1 - Math.pow(0.001, 1 / ticks);
  let alpha = 1;
  for (let tick = 0; tick < ticks; tick++) {
    alpha -= alpha * decay;
    for (const [s, t] of pairs) {
      const ds = degree.get(s.key), dt = degree.get(t.key);
      let dx = t.x + t.vx - s.x - s.vx, dy = t.y + t.vy - s.y - s.vy;
      const d = Math.hypot(dx, dy) || 1e-6, k = (d - distance) / d * alpha / Math.min(ds, dt);
      dx *= k; dy *= k;
      const bias = ds / (ds + dt);
      t.vx -= dx * bias; t.vy -= dy * bias; s.vx += dx * (1 - bias); s.vy += dy * (1 - bias);
    }
    for (let i = 0; i < n; i++) {
      const a = nodes[i];
      for (let j = i + 1; j < n; j++) {
        const b = nodes[j];
        let dx = b.x - a.x, dy = b.y - a.y, d2 = dx * dx + dy * dy;
        if (d2 > 250000) continue;
        if (d2 < 1e-9) { dx = (i - j) * 1e-3; dy = 1e-3; d2 = dx * dx + dy * dy; }
        const w = charge * alpha / Math.max(d2, 1);
        a.vx += dx * w; a.vy += dy * w; b.vx -= dx * w; b.vy -= dy * w;
      }
    }
    for (const d of nodes) {
      d.vx -= d.x * gravity * alpha; d.vy -= d.y * gravity * alpha;
      d.x += (d.vx *= 0.6); d.y += (d.vy *= 0.6);
    }
  }
  return nodes;
}

// Fragments without links go round the outside in rings, in list order, instead of into the simulation.
function ring(nodes, around) {
  let r = Math.max(60, ...around.map(d => Math.hypot(d.x, d.y))) + 70, i = 0;
  while (i < nodes.length) {
    const room = Math.max(8, Math.floor(2 * Math.PI * r / 26)), count = Math.min(room, nodes.length - i);
    for (let j = 0; j < count; j++, i++) {
      const a = -Math.PI / 2 + j / count * 2 * Math.PI;
      nodes[i].x = Math.cos(a) * r; nodes[i].y = Math.sin(a) * r;
    }
    r += 30;
  }
}

/**
 * rows: fragments in view. edges: [keyA, keyB] pairs (direction ignored).
 * options: key(row), current (key of the open fragment), settled(row), open(row), label(row), describe(row).
 */
export function vaultGraph(rows, edges, o) {
  const byKey = new Map(rows.map(r => [o.key(r), r]));
  const seen = new Set(), links = [], adjacent = new Map();
  for (const [a, b] of edges) {
    if (a === b || !byKey.has(a) || !byKey.has(b)) continue;
    const id = a < b ? `${a}\n${b}` : `${b}\n${a}`;
    if (seen.has(id)) continue;
    seen.add(id); links.push({ source: a, target: b });
    for (const [x, y] of [[a, b], [b, a]]) { if (!adjacent.has(x)) adjacent.set(x, new Set()); adjacent.get(x).add(y); }
  }
  const root = html('div', 'vault-graph');
  const linked = rows.filter(r => adjacent.has(o.key(r))), loose = rows.filter(r => !adjacent.has(o.key(r)));

  const bar = html('div', 'vg-bar'), caption = html('p', 'vg-caption');
  caption.append(`${linked.length.toLocaleString()} linked ${linked.length === 1 ? 'fragment' : 'fragments'}, ${links.length.toLocaleString()} ${links.length === 1 ? 'link' : 'links'}`);
  const legend = html('span', 'vg-legend');
  legend.append(html('span', 'vg-key open', 'Open'), html('span', 'vg-key settled', 'Settled'));
  caption.append(legend);
  bar.append(caption);
  root.append(bar);
  if (!links.length) {
    const empty = html('div', 'empty');
    empty.append(html('strong', '', 'No links yet'), 'Type ', html('code', '', '[['), ' in a fragment to link it to another; links show up here once saved.');
    root.append(empty);
    return root;
  }

  const tools = html('div', 'vg-tools');
  const button = text => {
    const b = document.createElement('aihio-button'), label = html('span', '', text);
    b.setAttribute('variant', 'ghost'); b.setAttribute('size', 'sm');
    b.append(label);
    return [b, label];
  };
  let showLoose = false;
  const [looseToggle, looseLabel] = loose.length ? button(`Show ${loose.length.toLocaleString()} unlinked`) : [];
  const [fitButton] = button('Fit');
  fitButton.title = 'Fit the graph to the view';
  if (looseToggle) { looseToggle.setAttribute('aria-pressed', 'false'); tools.append(looseToggle); }
  tools.append(fitButton);
  bar.append(tools);

  const canvas = html('div', 'vg-canvas'), drawing = svg('svg', { class: 'vg', role: 'group', 'aria-label': 'Fragment graph. Tab to a fragment and press Enter to open it.' });
  const edgeLayer = svg('g', { class: 'vg-edges', 'aria-hidden': 'true' }), nodeLayer = svg('g', { class: 'vg-nodes' });
  drawing.append(edgeLayer, nodeLayer);
  canvas.append(drawing);
  root.append(canvas);

  let nodes = [], view = null, width = 0, height = 0, scale = 1, fitted = 1;
  const hub = Math.max(3, [...adjacent.values()].map(s => s.size).sort((a, b) => b - a)[Math.floor(adjacent.size * 0.12)] || 0);
  function build() {
    nodes = linked.map(r => ({ key: o.key(r), row: r }));
    layout(nodes, links);
    if (showLoose) { const outer = loose.map(r => ({ key: o.key(r), row: r })); ring(outer, nodes); nodes = nodes.concat(outer); }
    const at = new Map(nodes.map(d => [d.key, d]));
    edgeLayer.replaceChildren(...links.map(l => {
      const a = at.get(l.source), b = at.get(l.target);
      const line = svg('line', { x1: a.x.toFixed(1), y1: a.y.toFixed(1), x2: b.x.toFixed(1), y2: b.y.toFixed(1) });
      line.dataset.a = l.source; line.dataset.b = l.target;
      return line;
    }));
    nodeLayer.replaceChildren(...nodes.map(d => {
      const degree = adjacent.get(d.key)?.size || 0, r = 3.5 + Math.sqrt(degree) * 1.6, name = o.label(d.row);
      const g = svg('g', { class: 'vg-node', tabindex: '0', role: 'button', 'aria-label': `${name}. ${o.describe(d.row)}. ${degree} ${degree === 1 ? 'link' : 'links'}.` });
      g.dataset.key = d.key; g.dataset.x = d.x.toFixed(1); g.dataset.y = d.y.toFixed(1);
      if (o.settled(d.row)) g.classList.add('settled');
      if (d.key === o.current) g.classList.add('current');
      if (degree >= hub) g.classList.add('hub');
      const label = svg('text', { x: (r + 5).toFixed(1), dy: '0.35em' });
      label.textContent = name.length > 48 ? `${name.slice(0, 47)}…` : name;
      g.append(svg('circle', { r: r.toFixed(1) }), label);
      return g;
    }));
  }
  function apply() {
    drawing.setAttribute('viewBox', `${view.x} ${view.y} ${view.w} ${view.h}`);
    scale = view.w / width;
    for (const g of nodeLayer.children) g.setAttribute('transform', `translate(${g.dataset.x} ${g.dataset.y}) scale(${scale})`);
    // A small graph is labelled throughout; a large one labels its hubs until you zoom in past the fit.
    root.dataset.zoom = nodes.length <= 40 || scale < fitted / 1.5 ? 'near' : 'far';
  }
  function fit() {
    if (!width || !nodes.length) return;
    const xs = nodes.map(d => d.x), ys = nodes.map(d => d.y);
    const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
    // Labels hang to the right of their nodes, so the right margin is wider than the left.
    const left = 32, right = 220, k = Math.min(1.6, (width - left - right) / Math.max(maxX - minX, 1), (height - 64) / Math.max(maxY - minY, 1));
    view = { w: width / k, h: height / k };
    view.x = minX - (left + (width - left - right - (maxX - minX) * k) / 2) / k; view.y = (minY + maxY) / 2 - view.h / 2;
    fitted = 1 / k;
    apply();
  }
  new ResizeObserver(([entry]) => {
    const { width: w, height: h } = entry.contentRect;
    if (!w || !h) return;
    if (view) { view.w *= w / width; view.h *= h / height; width = w; height = h; apply(); }
    else { width = w; height = h; fit(); }
  }).observe(canvas);

  // Highlight a node with its neighbours and the links between them.
  function focus(key) {
    root.toggleAttribute('data-focus', !!key);
    const near = key ? adjacent.get(key) : null;
    for (const g of nodeLayer.children) g.classList.toggle('lit', !!key && (g.dataset.key === key || !!near?.has(g.dataset.key)));
    for (const l of edgeLayer.children) l.classList.toggle('lit', !!key && (l.dataset.a === key || l.dataset.b === key));
  }
  nodeLayer.addEventListener('pointerover', e => { const g = e.target.closest('.vg-node'); if (g) focus(g.dataset.key); });
  nodeLayer.addEventListener('pointerout', e => { if (!e.relatedTarget?.closest?.('.vg-node')) focus(null); });
  nodeLayer.addEventListener('focusin', e => focus(e.target.closest('.vg-node')?.dataset.key));
  nodeLayer.addEventListener('focusout', () => focus(null));
  nodeLayer.addEventListener('keydown', e => {
    const g = e.target.closest('.vg-node');
    if (g && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); o.open(byKey.get(g.dataset.key), e); }
  });

  // Drag to pan, wheel to zoom about the pointer, click a node to open it.
  let drag = null;
  canvas.addEventListener('pointerdown', e => {
    if (e.button !== 0 || !view) return;
    drag = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y, node: e.target.closest('.vg-node'), moved: false };
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener('pointermove', e => {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 4) return;
    drag.moved = true; root.dataset.panning = '';
    view.x = drag.vx - dx * scale; view.y = drag.vy - dy * scale; apply();
  });
  const release = e => {
    if (!drag) return;
    const { node, moved } = drag;
    drag = null; delete root.dataset.panning;
    if (e.type === 'pointerup' && node && !moved) o.open(byKey.get(node.dataset.key), e);
  };
  canvas.addEventListener('pointerup', release);
  canvas.addEventListener('pointercancel', release);
  canvas.addEventListener('wheel', e => {
    if (!view) return;
    e.preventDefault();
    const box = drawing.getBoundingClientRect(), px = (e.clientX - box.left) / box.width, py = (e.clientY - box.top) / box.height;
    const k = Math.min(6 * width / view.w, Math.max(0.15 * width / view.w, Math.exp(e.deltaY * (e.deltaMode ? 0.05 : 0.0015))));
    const w = view.w * k, h = view.h * k;
    view.x += (view.w - w) * px; view.y += (view.h - h) * py; view.w = w; view.h = h;
    apply();
  }, { passive: false });

  fitButton.addEventListener('click', fit);
  looseToggle?.addEventListener('click', () => {
    showLoose = !showLoose;
    looseLabel.textContent = showLoose ? 'Hide unlinked' : `Show ${loose.length.toLocaleString()} unlinked`;
    looseToggle.setAttribute('aria-pressed', String(showLoose));
    build(); fit();
  });
  build();
  return root;
}
