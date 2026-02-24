import { searchFragments } from "../api/search.ts";
import { navigate } from "../lib/router.ts";
import { shortId } from "../lib/format.ts";

export class CommandPalette extends HTMLElement {
  private shadow: ShadowRoot;
  private visible = false;
  private searchTimeout: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  connectedCallback(): void {
    this.shadow.innerHTML = `
      <style>
        :host { display: none; }
        :host(.visible) { display: block; }
        .overlay {
          position: fixed;
          inset: 0;
          background: rgba(0,0,0,0.4);
          z-index: 100;
          display: flex;
          justify-content: center;
          padding-top: 120px;
        }
        .palette {
          width: 560px;
          max-height: 400px;
          background: var(--bg-surface);
          border: 1px solid var(--border);
          border-radius: var(--radius-xl);
          box-shadow: var(--shadow-lg);
          display: flex;
          flex-direction: column;
          overflow: hidden;
        }
        .search-input {
          padding: 14px 18px;
          border: none;
          border-bottom: 1px solid var(--border-light);
          background: transparent;
          color: var(--text);
          font-size: 16px;
          outline: none;
        }
        .search-input::placeholder { color: var(--text-muted); }
        .results {
          overflow-y: auto;
          flex: 1;
        }
        .result {
          display: flex;
          align-items: center;
          gap: 10px;
          padding: 10px 18px;
          cursor: pointer;
          font-size: 13px;
        }
        .result:hover, .result.selected { background: var(--bg-hover); }
        .result-icon {
          width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0;
        }
        .result-title { flex: 1; color: var(--text); }
        .result-id { font-family: monospace; font-size: 11px; color: var(--text-muted); }
        .result-type { font-size: 11px; color: var(--text-muted); text-transform: uppercase; }
        .actions {
          display: flex;
          gap: 2px;
          padding: 8px 18px;
          border-top: 1px solid var(--border-light);
        }
        .action {
          padding: 6px 14px;
          font-size: 12px;
          color: var(--text-secondary);
          cursor: pointer;
          border-radius: var(--radius-sm);
        }
        .action:hover { background: var(--bg-hover); }
        .hint { font-size: 11px; color: var(--text-muted); padding: 12px 18px; }
      </style>
      <div class="overlay" id="overlay">
        <div class="palette">
          <input type="text" class="search-input" id="palette-input" placeholder="Search fragments, navigate..." />
          <div class="results" id="results">
            <div class="hint">Type to search fragments or use commands: &gt;new, &gt;settings, &gt;graph, &gt;tags</div>
          </div>
          <div class="actions">
            <span class="action" data-action="new">New Fragment</span>
            <span class="action" data-action="settings">Settings</span>
            <span class="action" data-action="tags">Tags</span>
            <span class="action" data-action="graph">Graph</span>
          </div>
        </div>
      </div>
    `;

    this.shadow.getElementById("overlay")?.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).id === "overlay") this.hide();
    });

    const input = this.shadow.getElementById("palette-input") as HTMLInputElement;
    input.addEventListener("input", () => {
      if (this.searchTimeout) clearTimeout(this.searchTimeout);
      this.searchTimeout = setTimeout(() => this.doSearch(input.value), 200);
    });

    input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Escape") this.hide();
      if (e.key === "Enter") {
        const selected = this.shadow.querySelector(".result.selected, .result:first-child") as HTMLElement;
        selected?.click();
      }
    });

    this.shadow.querySelectorAll(".action").forEach((el) => {
      el.addEventListener("click", () => {
        const action = (el as HTMLElement).dataset.action!;
        this.hide();
        navigate(action);
      });
    });
  }

  show(): void {
    this.visible = true;
    this.classList.add("visible");
    const input = this.shadow.getElementById("palette-input") as HTMLInputElement;
    input.value = "";
    input.focus();
  }

  hide(): void {
    this.visible = false;
    this.classList.remove("visible");
  }

  toggle(): void {
    if (this.visible) this.hide();
    else this.show();
  }

  private async doSearch(query: string): Promise<void> {
    const container = this.shadow.getElementById("results")!;

    // Command mode
    if (query.startsWith(">")) {
      const cmd = query.slice(1).trim().toLowerCase();
      const commands = [
        { label: "New Fragment", route: "new" },
        { label: "Settings", route: "settings" },
        { label: "Tags", route: "tags" },
        { label: "Graph", route: "graph" },
        { label: "All Fragments", route: "all" },
        { label: "Vault", route: "vault" },
      ].filter((c) => c.label.toLowerCase().includes(cmd));

      container.innerHTML = commands
        .map(
          (c) => `<div class="result" data-route="${c.route}">
            <span class="result-title">${c.label}</span>
          </div>`
        )
        .join("");

      container.querySelectorAll(".result").forEach((el) => {
        el.addEventListener("click", () => {
          this.hide();
          navigate((el as HTMLElement).dataset.route!);
        });
      });
      return;
    }

    if (!query.trim()) {
      container.innerHTML = `<div class="hint">Type to search or use &gt; for commands</div>`;
      return;
    }

    try {
      const results = await searchFragments(query, 10);
      if (results.length === 0) {
        container.innerHTML = `<div class="hint">No results</div>`;
        return;
      }

      container.innerHTML = results
        .map(
          (r) => `
          <div class="result" data-id="${r.id}">
            <span class="result-icon" style="background: var(--type-${r.type})"></span>
            <span class="result-title">${this.esc(r.title || "Untitled")}</span>
            <span class="result-type">${r.type}</span>
            <span class="result-id">${shortId(r.id)}</span>
          </div>
        `
        )
        .join("");

      container.querySelectorAll(".result").forEach((el) => {
        el.addEventListener("click", () => {
          this.hide();
          navigate(`fragment/${(el as HTMLElement).dataset.id}`);
        });
      });
    } catch {
      container.innerHTML = `<div class="hint">Search error</div>`;
    }
  }

  private esc(s: string): string {
    const el = document.createElement("span");
    el.textContent = s;
    return el.innerHTML;
  }
}

customElements.define("command-palette", CommandPalette);
