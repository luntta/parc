import { listTags } from "../api/tags.ts";
import { navigate } from "../lib/router.ts";
import type { TagCountDto } from "../api/types.ts";

export class TagBrowser extends HTMLElement {
  private shadow: ShadowRoot;
  private tags: TagCountDto[] = [];
  private viewMode: "cloud" | "list" = "cloud";

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  async connectedCallback(): Promise<void> {
    this.tags = await listTags();
    this.render();
  }

  private render(): void {
    const maxCount = Math.max(...this.tags.map((t) => t.count), 1);

    this.shadow.innerHTML = `
      <style>
        :host { display: block; }
        .header { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0; flex: 1; }
        .toggle { display: flex; gap: 2px; }
        .toggle-btn {
          padding: 4px 10px;
          border: 1px solid var(--border);
          background: var(--bg);
          color: var(--text-muted);
          font-size: 12px;
          cursor: pointer;
        }
        .toggle-btn:first-child { border-radius: var(--radius-sm) 0 0 var(--radius-sm); }
        .toggle-btn:last-child { border-radius: 0 var(--radius-sm) var(--radius-sm) 0; }
        .toggle-btn.active { background: var(--accent); color: var(--accent-text); border-color: var(--accent); }
        .cloud {
          display: flex;
          flex-wrap: wrap;
          gap: 8px;
          align-items: center;
          padding: 20px;
        }
        .cloud-tag {
          cursor: pointer;
          color: var(--accent);
          transition: opacity 0.15s;
          padding: 4px 8px;
          border-radius: var(--radius-sm);
        }
        .cloud-tag:hover { background: var(--accent-light); }
        .list-row {
          display: grid;
          grid-template-columns: 1fr 60px;
          gap: 12px;
          padding: 8px 12px;
          border-bottom: 1px solid var(--border-light);
          cursor: pointer;
          font-size: 13px;
        }
        .list-row:hover { background: var(--bg-hover); }
        .list-header {
          display: grid;
          grid-template-columns: 1fr 60px;
          gap: 12px;
          padding: 6px 12px;
          font-size: 11px;
          font-weight: 600;
          color: var(--text-muted);
          text-transform: uppercase;
          border-bottom: 2px solid var(--border);
        }
        .count { text-align: right; color: var(--text-muted); font-size: 12px; }
        .tag-name { color: var(--text); }
        .bar { height: 4px; background: var(--accent); border-radius: 2px; margin-top: 2px; }
        .empty { text-align: center; padding: 40px; color: var(--text-muted); }
      </style>
      <div class="header">
        <h2>Tags</h2>
        <span style="font-size: 13px; color: var(--text-muted)">${this.tags.length} tags</span>
        <div class="toggle">
          <button class="toggle-btn ${this.viewMode === "cloud" ? "active" : ""}" data-mode="cloud">Cloud</button>
          <button class="toggle-btn ${this.viewMode === "list" ? "active" : ""}" data-mode="list">List</button>
        </div>
      </div>
      <div id="content"></div>
    `;

    this.shadow.querySelectorAll(".toggle-btn").forEach((btn) => {
      btn.addEventListener("click", () => {
        this.viewMode = (btn as HTMLElement).dataset.mode as "cloud" | "list";
        this.render();
      });
    });

    const content = this.shadow.getElementById("content")!;

    if (this.tags.length === 0) {
      content.innerHTML = `<div class="empty">No tags yet</div>`;
      return;
    }

    if (this.viewMode === "cloud") {
      content.className = "cloud";
      content.innerHTML = this.tags
        .map((t) => {
          const size = 12 + (t.count / maxCount) * 20;
          return `<span class="cloud-tag" style="font-size: ${size}px" data-tag="${t.tag}">#${t.tag} <sup style="font-size:10px; color: var(--text-muted)">${t.count}</sup></span>`;
        })
        .join("");
    } else {
      content.innerHTML = `
        <div class="list-header"><span>Tag</span><span style="text-align:right">Count</span></div>
        ${this.tags
          .map(
            (t) => `
          <div class="list-row" data-tag="${t.tag}">
            <div>
              <span class="tag-name">#${t.tag}</span>
              <div class="bar" style="width: ${(t.count / maxCount) * 100}%"></div>
            </div>
            <span class="count">${t.count}</span>
          </div>
        `
          )
          .join("")}
      `;
    }

    content.querySelectorAll("[data-tag]").forEach((el) => {
      el.addEventListener("click", () => {
        navigate("search", { q: `tag:${(el as HTMLElement).dataset.tag}` });
      });
    });
  }
}

customElements.define("tag-browser", TagBrowser);
