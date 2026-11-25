// Deterministic force layout for the vault graph. Uses exact pairwise repulsion for small graphs and a
// Barnes-Hut quadtree (O(n log n)) above 200 nodes so vaults with thousands of links lay out cleanly and fast.
const BH_THRESHOLD = 200;
const THETA2 = 0.85 * 0.85;

// Build a Barnes-Hut quadtree over nodes in-place. Leaf nodes hold one body (or stacked bodies at the same point).
function buildTree(nodes) {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const d of nodes) {
    if (d.x < minX) minX = d.x; if (d.x > maxX) maxX = d.x;
    if (d.y < minY) minY = d.y; if (d.y > maxY) maxY = d.y;
  }
  const size = Math.max(maxX - minX, maxY - minY, 1) * 1.02;
  const cx = (minX + maxX) / 2, cy = (minY + maxY) / 2;
  const root = { cx, cy, half: size / 2, mass: 0, mx: 0, my: 0, body: null, kids: null };

  function quadrant(q, x, y) {
    const right = x >= q.cx ? 1 : 0, bottom = y >= q.cy ? 2 : 0;
    const idx = right | bottom;
    if (!q.kids) q.kids = [null, null, null, null];
    let child = q.kids[idx];
    if (!child) {
      const h = q.half / 2;
      child = { cx: q.cx + (right ? h : -h), cy: q.cy + (bottom ? h : -h), half: h, mass: 0, mx: 0, my: 0, body: null, kids: null };
      q.kids[idx] = child;
    }
    return child;
  }

  for (const d of nodes) {
    let q = root, depth = 0;
    while (true) {
      q.mass += 1; q.mx += d.x; q.my += d.y;
      if (q.mass === 1) { q.body = d; break; }
      if (depth >= 24) { q.body = null; break; } // coincident bodies: treat as aggregated cell
      if (q.body) {
        const prev = q.body; q.body = null;
        const cPrev = quadrant(q, prev.x, prev.y);
        cPrev.mass += 1; cPrev.mx += prev.x; cPrev.my += prev.y; cPrev.body = prev;
      }
      q = quadrant(q, d.x, d.y);
      depth++;
    }
  }
  return root;
}

function applyRepulsionBH(node, root, kAlpha, maxDist2) {
  const stack = [root];
  while (stack.length) {
    const q = stack.pop();
    if (!q || !q.mass || q.body === node) continue;
    const dx = q.mx / q.mass - node.x, dy = q.my / q.mass - node.y;
    const d2 = dx * dx + dy * dy;
    const width = q.half * 2;
    if (!q.kids || (width * width < THETA2 * d2)) {
      if (d2 > maxDist2) continue;
      if (d2 < 1e-9) {
        node.vx += (kAlpha * q.mass) * 1e-3;
        node.vy += (kAlpha * q.mass) * 1e-3;
        continue;
      }
      const w = (kAlpha * q.mass) / Math.max(d2, 1);
      node.vx += dx * w;
      node.vy += dy * w;
    } else {
      for (let i = 0; i < 4; i++) if (q.kids[i]) stack.push(q.kids[i]);
    }
  }
}

// Links pull to `distance`, bodies repel, and a weak pull to the origin keeps components together.
// Starts on a sunflower spiral, so the same vault always lays out the same way.
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
  const useBH = n > BH_THRESHOLD;
  const ticks = useBH ? Math.max(120, Math.min(220, Math.round(6e5 / n))) : Math.max(120, Math.min(300, Math.round(3e7 / (n * n))));
  const decay = 1 - Math.pow(0.001, 1 / ticks);
  const maxDist2 = 250000;
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
    if (useBH) {
      const tree = buildTree(nodes), kAlpha = charge * alpha;
      for (let i = 0; i < n; i++) applyRepulsionBH(nodes[i], tree, kAlpha, maxDist2);
    } else {
      for (let i = 0; i < n; i++) {
        const a = nodes[i];
        for (let j = i + 1; j < n; j++) {
          const b = nodes[j];
          let dx = b.x - a.x, dy = b.y - a.y, d2 = dx * dx + dy * dy;
          if (d2 > maxDist2) continue;
          if (d2 < 1e-9) { dx = (i - j) * 1e-3; dy = 1e-3; d2 = dx * dx + dy * dy; }
          const w = charge * alpha / Math.max(d2, 1);
          a.vx += dx * w; a.vy += dy * w; b.vx -= dx * w; b.vy -= dy * w;
        }
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
export function ring(nodes, around) {
  let maxR = 60;
  for (const d of around) { const r = Math.hypot(d.x, d.y); if (r > maxR) maxR = r; }
  let r = maxR + 70, i = 0;
  while (i < nodes.length) {
    const room = Math.max(8, Math.floor(2 * Math.PI * r / 26)), count = Math.min(room, nodes.length - i);
    for (let j = 0; j < count; j++, i++) {
      const a = -Math.PI / 2 + j / count * 2 * Math.PI;
      nodes[i].x = Math.cos(a) * r; nodes[i].y = Math.sin(a) * r;
    }
    r += 30;
  }
}
