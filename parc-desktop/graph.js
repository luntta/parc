// The whole vault as a graph: a force layout computed off the UI thread for large vaults (no animation),
// drawn as SVG for accessible small/medium graphs (with O(1) CSS-variable counter-scaling on pan/zoom)
// and as a high-DPI Canvas above CANVAS_THRESHOLD nodes so 10,000-node vaults stay at 60 fps.
import { layout, ring } from './graph-layout.mjs';
export { layout };

const NS = 'http://www.w3.org/2000/svg';
const WORKER_THRESHOLD = 120;
const CANVAS_THRESHOLD = 600;

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

// One shared layout worker, started lazily on the first large graph.
let worker = null, workerSeq = 0;
const workerJobs = new Map();
function computeAsync(linkedKeys, looseKeys, links) {
  if (typeof Worker === 'undefined') return null;
  try {
    if (!worker) {
      worker = new Worker('dist/graph-worker.js');
      worker.onmessage = ({ data }) => {
        const job = workerJobs.get(data.id);
        if (!job) return;
        workerJobs.delete(data.id);
        job.resolve(data.coords);
      };
      worker.onerror = err => {
        for (const job of workerJobs.values()) job.reject(err);
        workerJobs.clear();
        worker = null;
      };
    }
    const id = ++workerSeq;
    return new Promise((resolve, reject) => {
      workerJobs.set(id, { resolve, reject });
      worker.postMessage({ id, linked: linkedKeys, loose: looseKeys, links });
    });
  } catch { return null; }
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

  const canvasWrap = html('div', 'vg-canvas');
  const drawing = svg('svg', { class: 'vg', role: 'group', 'aria-label': 'Fragment graph. Tab to a fragment and press Enter to open it.' });
  const edgeLayer = svg('g', { class: 'vg-edges', 'aria-hidden': 'true' }), nodeLayer = svg('g', { class: 'vg-nodes' });
  drawing.append(edgeLayer, nodeLayer);
  const paintCanvas = html('canvas', 'vg vg-bitmap');
  paintCanvas.hidden = true;
  paintCanvas.setAttribute('role', 'img');
  paintCanvas.setAttribute('aria-label', 'Fragment graph. Drag to pan, scroll to zoom, click a node to open it.');
  const busyNote = html('p', 'vg-busy', 'Laying out graph…');
  busyNote.hidden = true;
  canvasWrap.append(drawing, paintCanvas, busyNote);
  root.append(canvasWrap);

  let nodes = [], edgePairs = [], view = null, width = 0, height = 0, scale = 1, lastScale = 0, fitted = 1;
  let useCanvas = false, focusedKey = null, buildSeq = 0, raf = 0;
  const hub = Math.max(3, [...adjacent.values()].map(s => s.size).sort((a, b) => b - a)[Math.floor(adjacent.size * 0.12)] || 0);

  function mountNodes() {
    useCanvas = nodes.length > CANVAS_THRESHOLD;
    drawing.hidden = useCanvas;
    paintCanvas.hidden = !useCanvas;
    const at = new Map(nodes.map(d => [d.key, d]));
    edgePairs = links.map(l => [at.get(l.source), at.get(l.target)]).filter(([a, b]) => a && b);
    for (const d of nodes) {
      d.degree = adjacent.get(d.key)?.size || 0;
      d.r = 3.5 + Math.sqrt(d.degree) * 1.6;
      d.name = o.label(d.row);
      d.short = d.name.length > 48 ? `${d.name.slice(0, 47)}…` : d.name;
      d.isSettled = !!o.settled(d.row);
      d.isCurrent = d.key === o.current;
      d.isHub = d.degree >= hub;
    }
    if (useCanvas) {
      edgeLayer.replaceChildren();
      nodeLayer.replaceChildren();
      return;
    }
    edgeLayer.replaceChildren(...edgePairs.map(([a, b]) => {
      const line = svg('line', { x1: a.x.toFixed(1), y1: a.y.toFixed(1), x2: b.x.toFixed(1), y2: b.y.toFixed(1) });
      line.dataset.a = a.key; line.dataset.b = b.key;
      return line;
    }));
    nodeLayer.replaceChildren(...nodes.map(d => {
      const g = svg('g', {
        class: 'vg-node', tabindex: '0', role: 'button',
        transform: `translate(${d.x.toFixed(1)} ${d.y.toFixed(1)})`,
        'aria-label': `${d.name}. ${o.describe(d.row)}. ${d.degree} ${d.degree === 1 ? 'link' : 'links'}.`
      });
      g.dataset.key = d.key;
      if (d.isSettled) g.classList.add('settled');
      if (d.isCurrent) g.classList.add('current');
      if (d.isHub) g.classList.add('hub');
      const glyph = svg('g', { class: 'vg-glyph' });
      const label = svg('text', { x: (d.r + 5).toFixed(1), dy: '0.35em' });
      label.textContent = d.short;
      glyph.append(svg('circle', { r: d.r.toFixed(1) }), label);
      g.append(glyph);
      return g;
    }));
    lastScale = 0;
  }

  function build() {
    const seq = ++buildSeq;
    const base = linked.map(r => ({ key: o.key(r), row: r }));
    const extras = showLoose ? loose.map(r => ({ key: o.key(r), row: r })) : [];
    const job = base.length > WORKER_THRESHOLD ? computeAsync(base.map(d => d.key), extras.map(d => d.key), links) : null;
    if (!job) {
      layout(base, links);
      if (extras.length) ring(extras, base);
      nodes = base.concat(extras);
      busyNote.hidden = true;
      mountNodes();
      if (width && height) fit();
      return;
    }
    busyNote.hidden = false;
    job.then(coords => {
      if (seq !== buildSeq || !root.isConnected) return;
      const all = base.concat(extras);
      for (let i = 0; i < all.length; i++) { all[i].x = coords[i * 2]; all[i].y = coords[i * 2 + 1]; }
      nodes = all;
      busyNote.hidden = true;
      mountNodes();
      if (width && height) fit();
    }, () => {
      if (seq !== buildSeq || !root.isConnected) return;
      layout(base, links);
      if (extras.length) ring(extras, base);
      nodes = base.concat(extras);
      busyNote.hidden = true;
      mountNodes();
      if (width && height) fit();
    });
  }

  // Paint the high-DPI Canvas for graphs above CANVAS_THRESHOLD nodes.
  function drawCanvas() {
    if (!width || !height || !view) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const pw = Math.round(width * dpr), ph = Math.round(height * dpr);
    if (paintCanvas.width !== pw || paintCanvas.height !== ph) { paintCanvas.width = pw; paintCanvas.height = ph; }
    const ctx = paintCanvas.getContext('2d');
    if (!ctx) return;
    const cs = getComputedStyle(canvasWrap);
    const fg = cs.getPropertyValue('--aihio-color-page-fg').trim() || '#111';
    const bg = cs.getPropertyValue('--aihio-color-surface-bg').trim() || '#fff';
    const muted = cs.getPropertyValue('--muted').trim() || '#666';
    const ringColor = cs.getPropertyValue('--aihio-color-focus-ring').trim() || '#2563eb';
    const font = cs.fontFamily || 'Inter, sans-serif';
    const inv = 1 / scale, near = focusedKey ? adjacent.get(focusedKey) : null;
    const toX = x => (x - view.x) * inv, toY = y => (y - view.y) * inv;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    // Edges: quiet pass, then highlighted pass if a node is focused.
    ctx.lineWidth = 1;
    ctx.strokeStyle = fg;
    ctx.globalAlpha = focusedKey ? 0.08 : 0.22;
    ctx.beginPath();
    for (const [a, b] of edgePairs) {
      if (focusedKey && (a.key === focusedKey || b.key === focusedKey)) continue;
      const x1 = toX(a.x), y1 = toY(a.y), x2 = toX(b.x), y2 = toY(b.y);
      if ((x1 < -20 && x2 < -20) || (x1 > width + 20 && x2 > width + 20) || (y1 < -20 && y2 < -20) || (y1 > height + 20 && y2 > height + 20)) continue;
      ctx.moveTo(x1, y1); ctx.lineTo(x2, y2);
    }
    ctx.stroke();

    if (focusedKey) {
      ctx.globalAlpha = 0.7;
      ctx.beginPath();
      for (const [a, b] of edgePairs) {
        if (a.key !== focusedKey && b.key !== focusedKey) continue;
        ctx.moveTo(toX(a.x), toY(a.y)); ctx.lineTo(toX(b.x), toY(b.y));
      }
      ctx.stroke();
    }

    // Nodes
    const zoomNear = scale < fitted / 1.5;
    const labels = [];
    for (const d of nodes) {
      const x = toX(d.x), y = toY(d.y);
      if (x < -40 || x > width + 40 || y < -40 || y > height + 40) continue;
      const lit = !focusedKey || d.key === focusedKey || !!near?.has(d.key);
      ctx.globalAlpha = lit ? 1 : 0.25;
      ctx.beginPath();
      ctx.arc(x, y, d.r, 0, Math.PI * 2);
      ctx.fillStyle = d.isSettled ? bg : fg;
      ctx.fill();
      ctx.lineWidth = d.isCurrent ? 3 : 1.5;
      ctx.strokeStyle = d.isCurrent ? ringColor : d.isSettled ? muted : bg;
      ctx.stroke();
      if (d.isHub || d.isCurrent || (focusedKey && lit) || zoomNear) labels.push([d, x, y, lit]);
    }

    // Labels (capped to keep zoomed-out dense clusters readable)
    if (labels.length) {
      ctx.font = `12px ${font}`;
      ctx.textBaseline = 'middle';
      ctx.lineJoin = 'round';
      ctx.lineWidth = 3;
      ctx.strokeStyle = bg;
      const maxLabels = zoomNear ? 180 : 60;
      for (let i = 0; i < labels.length && i < maxLabels; i++) {
        const [d, x, y, lit] = labels[i];
        ctx.globalAlpha = lit ? 1 : 0.25;
        ctx.fillStyle = d.isSettled ? muted : fg;
        ctx.strokeText(d.short, x + d.r + 5, y);
        ctx.fillText(d.short, x + d.r + 5, y);
      }
    }
    ctx.globalAlpha = 1;
  }

  function applyNow() {
    raf = 0;
    if (!view || !width) return;
    scale = view.w / width;
    if (useCanvas) { drawCanvas(); return; }
    drawing.setAttribute('viewBox', `${view.x} ${view.y} ${view.w} ${view.h}`);
    // O(1) zoom scaling: a single CSS variable scales every .vg-glyph without looping over SVG nodes.
    if (scale !== lastScale) {
      lastScale = scale;
      nodeLayer.style.setProperty('--vg-scale', String(scale));
      root.dataset.zoom = nodes.length <= 40 || scale < fitted / 1.5 ? 'near' : 'far';
    }
  }
  function scheduleApply() { if (!raf) raf = requestAnimationFrame(applyNow); }

  function fit() {
    if (!width || !nodes.length) return;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const d of nodes) {
      if (d.x < minX) minX = d.x; if (d.x > maxX) maxX = d.x;
      if (d.y < minY) minY = d.y; if (d.y > maxY) maxY = d.y;
    }
    // Labels hang to the right of their nodes, so the right margin is wider than the left.
    const left = 32, right = 220, k = Math.min(1.6, (width - left - right) / Math.max(maxX - minX, 1), (height - 64) / Math.max(maxY - minY, 1));
    view = { w: width / k, h: height / k };
    view.x = minX - (left + (width - left - right - (maxX - minX) * k) / 2) / k; view.y = (minY + maxY) / 2 - view.h / 2;
    fitted = 1 / k;
    applyNow();
  }
  const ro = new ResizeObserver(([entry]) => {
    if (!root.isConnected) { ro.disconnect(); return; }
    const { width: w, height: h } = entry.contentRect;
    if (!w || !h) return;
    if (view) { view.w *= w / width; view.h *= h / height; width = w; height = h; applyNow(); }
    else { width = w; height = h; if (nodes.length) fit(); }
  });
  ro.observe(canvasWrap);

  // Hit-test a pointer position against Canvas nodes (nearest node within its screen radius + 4 px).
  function hitCanvas(clientX, clientY) {
    if (!useCanvas || !view) return null;
    const box = paintCanvas.getBoundingClientRect();
    const sx = clientX - box.left, sy = clientY - box.top, inv = 1 / scale;
    let best = null, bestD2 = Infinity;
    for (const d of nodes) {
      const dx = (d.x - view.x) * inv - sx, dy = (d.y - view.y) * inv - sy;
      const hitR = d.r + 4, d2 = dx * dx + dy * dy;
      if (d2 <= hitR * hitR && d2 < bestD2) { best = d; bestD2 = d2; }
    }
    return best;
  }

  // Highlight a node with its neighbours and the links between them.
  function focus(key) {
    if (focusedKey === key) return;
    focusedKey = key;
    root.toggleAttribute('data-focus', !!key);
    if (useCanvas) {
      paintCanvas.style.cursor = key ? 'pointer' : '';
      scheduleApply();
      return;
    }
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
  canvasWrap.addEventListener('pointerdown', e => {
    if (e.button !== 0 || !view) return;
    const hitKey = useCanvas ? hitCanvas(e.clientX, e.clientY)?.key ?? null : e.target.closest('.vg-node')?.dataset.key ?? null;
    drag = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y, key: hitKey, moved: false };
    canvasWrap.setPointerCapture(e.pointerId);
  });
  canvasWrap.addEventListener('pointermove', e => {
    if (!drag) {
      if (useCanvas) focus(hitCanvas(e.clientX, e.clientY)?.key ?? null);
      return;
    }
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 4) return;
    drag.moved = true; root.dataset.panning = '';
    view.x = drag.vx - dx * scale; view.y = drag.vy - dy * scale;
    scheduleApply();
  });
  const release = e => {
    if (!drag) return;
    const { key, moved } = drag;
    drag = null; delete root.dataset.panning;
    if (e.type === 'pointerup' && key && !moved) o.open(byKey.get(key), e);
  };
  canvasWrap.addEventListener('pointerup', release);
  canvasWrap.addEventListener('pointercancel', release);
  canvasWrap.addEventListener('pointerleave', () => { if (useCanvas && !drag) focus(null); });
  canvasWrap.addEventListener('wheel', e => {
    if (!view) return;
    e.preventDefault();
    const box = canvasWrap.getBoundingClientRect(), px = (e.clientX - box.left) / box.width, py = (e.clientY - box.top) / box.height;
    const k = Math.min(6 * width / view.w, Math.max(0.15 * width / view.w, Math.exp(e.deltaY * (e.deltaMode ? 0.05 : 0.0015))));
    const w = view.w * k, h = view.h * k;
    view.x += (view.w - w) * px; view.y += (view.h - h) * py; view.w = w; view.h = h;
    scheduleApply();
  }, { passive: false });

  // Redraw Canvas when system/app theme flips.
  const scheme = window.matchMedia?.('(prefers-color-scheme: dark)');
  const onScheme = () => { if (!root.isConnected) scheme?.removeEventListener?.('change', onScheme); else if (useCanvas) scheduleApply(); };
  scheme?.addEventListener?.('change', onScheme);

  fitButton.addEventListener('click', fit);
  looseToggle?.addEventListener('click', () => {
    showLoose = !showLoose;
    looseLabel.textContent = showLoose ? 'Hide unlinked' : `Show ${loose.length.toLocaleString()} unlinked`;
    looseToggle.setAttribute('aria-pressed', String(showLoose));
    build();
  });
  build();
  return root;
}
