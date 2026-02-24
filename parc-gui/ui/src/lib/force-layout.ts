export interface GraphNode {
  id: string;
  label: string;
  type: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
  pinned?: boolean;
}

export interface GraphEdge {
  source: string;
  target: string;
}

export interface ForceLayoutOptions {
  width: number;
  height: number;
  repulsion: number;
  attraction: number;
  damping: number;
  centerForce: number;
}

const defaults: ForceLayoutOptions = {
  width: 800,
  height: 600,
  repulsion: 5000,
  attraction: 0.01,
  damping: 0.9,
  centerForce: 0.005,
};

export function initPositions(
  nodes: GraphNode[],
  width: number,
  height: number
): void {
  const cx = width / 2;
  const cy = height / 2;
  const radius = Math.min(width, height) * 0.3;
  nodes.forEach((node, i) => {
    const angle = (2 * Math.PI * i) / nodes.length;
    node.x = cx + radius * Math.cos(angle);
    node.y = cy + radius * Math.sin(angle);
    node.vx = 0;
    node.vy = 0;
  });
}

export function simulateStep(
  nodes: GraphNode[],
  edges: GraphEdge[],
  opts: Partial<ForceLayoutOptions> = {}
): void {
  const o = { ...defaults, ...opts };
  const cx = o.width / 2;
  const cy = o.height / 2;

  // Repulsion between all pairs
  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const a = nodes[i];
      const b = nodes[j];
      let dx = b.x - a.x;
      let dy = b.y - a.y;
      let dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < 1) dist = 1;

      const force = o.repulsion / (dist * dist);
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;

      if (!a.pinned) { a.vx -= fx; a.vy -= fy; }
      if (!b.pinned) { b.vx += fx; b.vy += fy; }
    }
  }

  // Attraction along edges
  const nodeMap = new Map(nodes.map((n) => [n.id, n]));
  for (const edge of edges) {
    const a = nodeMap.get(edge.source);
    const b = nodeMap.get(edge.target);
    if (!a || !b) continue;

    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (dist < 1) continue;

    const force = dist * o.attraction;
    const fx = (dx / dist) * force;
    const fy = (dy / dist) * force;

    if (!a.pinned) { a.vx += fx; a.vy += fy; }
    if (!b.pinned) { b.vx -= fx; b.vy -= fy; }
  }

  // Center gravity and velocity update
  for (const node of nodes) {
    if (node.pinned) continue;

    node.vx += (cx - node.x) * o.centerForce;
    node.vy += (cy - node.y) * o.centerForce;

    node.vx *= o.damping;
    node.vy *= o.damping;

    node.x += node.vx;
    node.y += node.vy;

    // Keep in bounds
    node.x = Math.max(20, Math.min(o.width - 20, node.x));
    node.y = Math.max(20, Math.min(o.height - 20, node.y));
  }
}
