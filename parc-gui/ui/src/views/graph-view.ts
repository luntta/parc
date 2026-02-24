import { listFragments, getFragment } from "../api/fragments.ts";
import { navigate } from "../lib/router.ts";
import { typeColor } from "../lib/format.ts";
import {
  initPositions,
  simulateStep,
  type GraphNode,
  type GraphEdge,
} from "../lib/force-layout.ts";

export class GraphView extends HTMLElement {
  private shadow: ShadowRoot;
  private canvas: HTMLCanvasElement | null = null;
  private ctx: CanvasRenderingContext2D | null = null;
  private nodes: GraphNode[] = [];
  private edges: GraphEdge[] = [];
  private animFrame = 0;
  private dragging: GraphNode | null = null;
  private offsetX = 0;
  private offsetY = 0;
  private zoom = 1;
  private panX = 0;
  private panY = 0;
  private hoveredNode: GraphNode | null = null;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  async connectedCallback(): Promise<void> {
    this.shadow.innerHTML = `
      <style>
        :host { display: block; height: 100%; }
        .container { position: relative; height: 100%; }
        canvas { width: 100%; height: 100%; display: block; cursor: grab; }
        canvas.dragging { cursor: grabbing; }
        .controls {
          position: absolute;
          top: 12px;
          right: 12px;
          display: flex;
          gap: 4px;
        }
        .ctrl-btn {
          padding: 4px 10px;
          border: 1px solid var(--border);
          border-radius: var(--radius-sm);
          background: var(--bg-surface);
          color: var(--text);
          font-size: 14px;
          cursor: pointer;
        }
        .ctrl-btn:hover { background: var(--bg-hover); }
        .legend {
          position: absolute;
          bottom: 12px;
          left: 12px;
          display: flex;
          gap: 12px;
          font-size: 11px;
          color: var(--text-muted);
        }
        .legend-item { display: flex; align-items: center; gap: 4px; }
        .legend-dot { width: 8px; height: 8px; border-radius: 50%; }
      </style>
      <div class="container">
        <canvas id="graph-canvas"></canvas>
        <div class="controls">
          <button class="ctrl-btn" id="zoom-in">+</button>
          <button class="ctrl-btn" id="zoom-out">-</button>
          <button class="ctrl-btn" id="zoom-reset">Reset</button>
        </div>
        <div class="legend">
          <span class="legend-item"><span class="legend-dot" style="background: var(--type-note)"></span> Note</span>
          <span class="legend-item"><span class="legend-dot" style="background: var(--type-todo)"></span> Todo</span>
          <span class="legend-item"><span class="legend-dot" style="background: var(--type-decision)"></span> Decision</span>
          <span class="legend-item"><span class="legend-dot" style="background: var(--type-risk)"></span> Risk</span>
          <span class="legend-item"><span class="legend-dot" style="background: var(--type-idea)"></span> Idea</span>
        </div>
      </div>
    `;

    this.canvas = this.shadow.getElementById("graph-canvas") as HTMLCanvasElement;
    this.ctx = this.canvas.getContext("2d");

    // Controls
    this.shadow.getElementById("zoom-in")?.addEventListener("click", () => { this.zoom *= 1.2; });
    this.shadow.getElementById("zoom-out")?.addEventListener("click", () => { this.zoom /= 1.2; });
    this.shadow.getElementById("zoom-reset")?.addEventListener("click", () => { this.zoom = 1; this.panX = 0; this.panY = 0; });

    await this.loadData();
    this.setupInteraction();
    this.startSimulation();
  }

  disconnectedCallback(): void {
    cancelAnimationFrame(this.animFrame);
  }

  private async loadData(): Promise<void> {
    const fragments = await listFragments({});
    const nodeMap = new Map<string, boolean>();

    this.nodes = fragments.map((f) => {
      nodeMap.set(f.id, true);
      return {
        id: f.id,
        label: f.title || f.id.slice(0, 8),
        type: f.type,
        x: 0, y: 0, vx: 0, vy: 0,
      };
    });

    // Build edges from links (we need full fragment data)
    this.edges = [];
    for (const f of fragments) {
      // Tags already available but links need full fragment read
      // For now, use index-based link data if available
    }

    // Load links by reading each fragment
    for (const f of fragments) {
      try {
        const full = await getFragment(f.id);
        for (const linkId of full.links) {
          if (nodeMap.has(linkId)) {
            // Avoid duplicates
            const exists = this.edges.some(
              (e) =>
                (e.source === f.id && e.target === linkId) ||
                (e.source === linkId && e.target === f.id)
            );
            if (!exists) {
              this.edges.push({ source: f.id, target: linkId });
            }
          }
        }
      } catch { /* ignore */ }
    }

    this.resizeCanvas();
    initPositions(this.nodes, this.canvas!.width, this.canvas!.height);
  }

  private resizeCanvas(): void {
    const rect = this.canvas!.parentElement!.getBoundingClientRect();
    this.canvas!.width = rect.width;
    this.canvas!.height = rect.height;
  }

  private setupInteraction(): void {
    const canvas = this.canvas!;

    canvas.addEventListener("mousedown", (e) => {
      const node = this.hitTest(e.offsetX, e.offsetY);
      if (node) {
        this.dragging = node;
        node.pinned = true;
        canvas.classList.add("dragging");
        this.offsetX = e.offsetX / this.zoom - this.panX - node.x;
        this.offsetY = e.offsetY / this.zoom - this.panY - node.y;
      }
    });

    canvas.addEventListener("mousemove", (e) => {
      if (this.dragging) {
        this.dragging.x = e.offsetX / this.zoom - this.panX - this.offsetX;
        this.dragging.y = e.offsetY / this.zoom - this.panY - this.offsetY;
      } else {
        this.hoveredNode = this.hitTest(e.offsetX, e.offsetY);
        canvas.style.cursor = this.hoveredNode ? "pointer" : "grab";
      }
    });

    canvas.addEventListener("mouseup", () => {
      if (this.dragging) {
        this.dragging.pinned = false;
        this.dragging = null;
        canvas.classList.remove("dragging");
      }
    });

    canvas.addEventListener("dblclick", (e) => {
      const node = this.hitTest(e.offsetX, e.offsetY);
      if (node) navigate(`fragment/${node.id}`);
    });

    canvas.addEventListener("wheel", (e) => {
      e.preventDefault();
      const factor = e.deltaY > 0 ? 0.95 : 1.05;
      this.zoom *= factor;
      this.zoom = Math.max(0.2, Math.min(5, this.zoom));
    });
  }

  private hitTest(mx: number, my: number): GraphNode | null {
    const x = mx / this.zoom - this.panX;
    const y = my / this.zoom - this.panY;
    for (const node of this.nodes) {
      const dx = node.x - x;
      const dy = node.y - y;
      if (dx * dx + dy * dy < 15 * 15) return node;
    }
    return null;
  }

  private startSimulation(): void {
    const tick = () => {
      simulateStep(this.nodes, this.edges, {
        width: this.canvas!.width / this.zoom,
        height: this.canvas!.height / this.zoom,
      });
      this.draw();
      this.animFrame = requestAnimationFrame(tick);
    };
    tick();
  }

  private draw(): void {
    const ctx = this.ctx!;
    const w = this.canvas!.width;
    const h = this.canvas!.height;

    ctx.clearRect(0, 0, w, h);
    ctx.save();
    ctx.scale(this.zoom, this.zoom);
    ctx.translate(this.panX, this.panY);

    // Draw edges
    ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--border").trim();
    ctx.lineWidth = 1;
    const nodeMap = new Map(this.nodes.map((n) => [n.id, n]));
    for (const edge of this.edges) {
      const a = nodeMap.get(edge.source);
      const b = nodeMap.get(edge.target);
      if (!a || !b) continue;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    }

    // Draw nodes
    for (const node of this.nodes) {
      const color = this.getCssVar(typeColor(node.type));
      const radius = node === this.hoveredNode ? 10 : 7;

      ctx.beginPath();
      ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.fill();

      // Label
      ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--text").trim();
      ctx.font = "10px sans-serif";
      ctx.textAlign = "center";
      const label = node.label.length > 20 ? node.label.slice(0, 20) + "..." : node.label;
      ctx.fillText(label, node.x, node.y + radius + 12);
    }

    ctx.restore();
  }

  private getCssVar(value: string): string {
    if (value.startsWith("var(")) {
      const name = value.slice(4, -1);
      return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";
    }
    return value;
  }
}

customElements.define("graph-view", GraphView);
