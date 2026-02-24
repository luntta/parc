import { emit } from "../lib/event-bus.ts";

export class FilterBar extends HTMLElement {
  private shadow: ShadowRoot;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  connectedCallback(): void {
    this.shadow.innerHTML = `
      <style>
        :host {
          display: flex;
          align-items: center;
          gap: 8px;
          padding: 8px 0;
          flex-wrap: wrap;
        }
        select, input {
          padding: 4px 8px;
          border: 1px solid var(--border);
          border-radius: var(--radius-sm);
          background: var(--bg);
          color: var(--text);
          font-size: 12px;
        }
        .sort-btn {
          padding: 4px 8px;
          border: 1px solid var(--border);
          border-radius: var(--radius-sm);
          background: var(--bg);
          color: var(--text-secondary);
          font-size: 12px;
          cursor: pointer;
        }
        .sort-btn:hover {
          background: var(--bg-hover);
        }
        .view-toggle {
          display: flex;
          gap: 2px;
          margin-left: auto;
        }
        .view-btn {
          padding: 4px 8px;
          border: 1px solid var(--border);
          background: var(--bg);
          color: var(--text-muted);
          font-size: 12px;
          cursor: pointer;
        }
        .view-btn:first-child { border-radius: var(--radius-sm) 0 0 var(--radius-sm); }
        .view-btn:last-child { border-radius: 0 var(--radius-sm) var(--radius-sm) 0; }
        .view-btn.active {
          background: var(--accent);
          color: var(--accent-text);
          border-color: var(--accent);
        }
      </style>
      <select id="status-filter">
        <option value="">All statuses</option>
        <option value="open">Open</option>
        <option value="active">Active</option>
        <option value="done">Done</option>
        <option value="cancelled">Cancelled</option>
      </select>
      <select id="sort-select">
        <option value="updated">Last updated</option>
        <option value="created">Created</option>
        <option value="title">Title</option>
      </select>
      <div class="view-toggle">
        <button class="view-btn active" data-view="card">Cards</button>
        <button class="view-btn" data-view="table">Table</button>
      </div>
    `;

    this.shadow.getElementById("status-filter")?.addEventListener("change", (e) => {
      emit("filter:change", { status: (e.target as HTMLSelectElement).value });
    });

    this.shadow.getElementById("sort-select")?.addEventListener("change", (e) => {
      emit("filter:sort", { sort: (e.target as HTMLSelectElement).value });
    });

    this.shadow.querySelectorAll(".view-btn").forEach((btn) => {
      btn.addEventListener("click", () => {
        this.shadow.querySelectorAll(".view-btn").forEach((b) => b.classList.remove("active"));
        btn.classList.add("active");
        emit("view:mode", { mode: (btn as HTMLElement).dataset.view });
      });
    });
  }
}

customElements.define("filter-bar", FilterBar);
