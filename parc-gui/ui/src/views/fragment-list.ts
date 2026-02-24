import { listFragments } from "../api/fragments.ts";
import { on } from "../lib/event-bus.ts";
import type { FragmentSummaryDto } from "../api/types.ts";
import "../components/fragment-card.ts";
import "../components/fragment-row.ts";
import "../components/filter-bar.ts";

export class FragmentList extends HTMLElement {
  private shadow: ShadowRoot;
  private fragments: FragmentSummaryDto[] = [];
  private viewMode: "card" | "table" = "card";
  private typeFilter: string | undefined;
  private statusFilter: string | undefined;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  static get observedAttributes(): string[] {
    return ["type"];
  }

  attributeChangedCallback(): void {
    this.typeFilter = this.getAttribute("type") || undefined;
    this.loadFragments();
  }

  connectedCallback(): void {
    this.typeFilter = this.getAttribute("type") || undefined;

    on("filter:change", (data: unknown) => {
      const d = data as { status: string };
      this.statusFilter = d.status || undefined;
      this.loadFragments();
    });

    on("view:mode", (data: unknown) => {
      const d = data as { mode: string };
      this.viewMode = d.mode as "card" | "table";
      this.renderList();
    });

    this.loadFragments();
  }

  private async loadFragments(): Promise<void> {
    try {
      this.fragments = await listFragments({
        type: this.typeFilter,
        status: this.statusFilter,
      });
      this.renderList();
    } catch (err) {
      this.shadow.innerHTML = `<div style="color: var(--text-muted); padding: 20px;">Error loading fragments: ${err}</div>`;
    }
  }

  private renderList(): void {
    const title = this.typeFilter
      ? `${this.typeFilter.charAt(0).toUpperCase() + this.typeFilter.slice(1)}s`
      : "All Fragments";

    this.shadow.innerHTML = `
      <style>
        :host { display: block; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0 0 4px 0; }
        .count { font-size: 13px; color: var(--text-muted); margin-bottom: 12px; }
        .grid {
          display: grid;
          grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
          gap: 12px;
        }
        .table-header {
          display: grid;
          grid-template-columns: 80px 70px 1fr 120px 100px;
          gap: 12px;
          padding: 6px 12px;
          font-size: 11px;
          font-weight: 600;
          color: var(--text-muted);
          text-transform: uppercase;
          letter-spacing: 0.5px;
          border-bottom: 2px solid var(--border);
        }
        .empty {
          text-align: center;
          padding: 40px 20px;
          color: var(--text-muted);
        }
      </style>
      <h2>${title}</h2>
      <div class="count">${this.fragments.length} fragment${this.fragments.length !== 1 ? "s" : ""}</div>
      <filter-bar></filter-bar>
      <div id="list-container"></div>
    `;

    const container = this.shadow.getElementById("list-container")!;

    if (this.fragments.length === 0) {
      container.innerHTML = `<div class="empty">No fragments found</div>`;
      return;
    }

    if (this.viewMode === "card") {
      container.className = "grid";
      this.fragments.forEach((f) => {
        const card = document.createElement("fragment-card") as import("../components/fragment-card.ts").FragmentCard;
        card.data = f;
        container.appendChild(card);
      });
    } else {
      container.innerHTML = `
        <div class="table-header">
          <span>ID</span><span>Type</span><span>Title</span><span>Tags</span><span style="text-align:right">Updated</span>
        </div>
      `;
      this.fragments.forEach((f) => {
        const row = document.createElement("fragment-row") as import("../components/fragment-row.ts").FragmentRow;
        row.data = f;
        container.appendChild(row);
      });
    }
  }
}

customElements.define("fragment-list", FragmentList);
