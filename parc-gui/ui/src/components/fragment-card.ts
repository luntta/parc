import { relativeTime, shortId } from "../lib/format.ts";
import { navigate } from "../lib/router.ts";
import type { FragmentSummaryDto } from "../api/types.ts";
import "./type-badge.ts";
import "./status-badge.ts";
import "./tag-chip.ts";

export class FragmentCard extends HTMLElement {
  private shadow: ShadowRoot;
  private _data: FragmentSummaryDto | null = null;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  set data(val: FragmentSummaryDto) {
    this._data = val;
    this.render();
  }

  private render(): void {
    const d = this._data;
    if (!d) return;

    const tagsHtml = d.tags
      .slice(0, 3)
      .map((t) => `<tag-chip tag="${t}"></tag-chip>`)
      .join("");
    const moreTag = d.tags.length > 3 ? `<span class="more">+${d.tags.length - 3}</span>` : "";

    this.shadow.innerHTML = `
      <style>
        :host {
          display: block;
        }
        .card {
          padding: 14px 16px;
          border: 1px solid var(--border);
          border-radius: var(--radius-lg);
          background: var(--bg-surface);
          cursor: pointer;
          transition: box-shadow 0.15s, border-color 0.15s;
        }
        .card:hover {
          border-color: var(--accent);
          box-shadow: var(--shadow-sm);
        }
        .header {
          display: flex;
          align-items: center;
          gap: 8px;
          margin-bottom: 6px;
        }
        .title {
          font-size: 14px;
          font-weight: 600;
          color: var(--text);
          flex: 1;
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
        }
        .meta {
          display: flex;
          align-items: center;
          gap: 6px;
          margin-top: 8px;
        }
        .id {
          font-size: 11px;
          color: var(--text-muted);
          font-family: monospace;
        }
        .time {
          font-size: 11px;
          color: var(--text-muted);
          margin-left: auto;
        }
        .tags {
          display: flex;
          gap: 4px;
          flex-wrap: wrap;
          margin-top: 6px;
        }
        .more {
          font-size: 11px;
          color: var(--text-muted);
          padding: 1px 6px;
        }
      </style>
      <div class="card">
        <div class="header">
          <type-badge type="${d.type}"></type-badge>
          <span class="title">${this.esc(d.title || "Untitled")}</span>
          ${d.status ? `<status-badge status="${d.status}"></status-badge>` : ""}
        </div>
        ${d.tags.length ? `<div class="tags">${tagsHtml}${moreTag}</div>` : ""}
        <div class="meta">
          <span class="id">${shortId(d.id)}</span>
          <span class="time">${relativeTime(d.updated_at)}</span>
        </div>
      </div>
    `;

    this.shadow.querySelector(".card")?.addEventListener("click", () => {
      navigate(`fragment/${d.id}`);
    });
  }

  private esc(s: string): string {
    const el = document.createElement("span");
    el.textContent = s;
    return el.innerHTML;
  }
}

customElements.define("fragment-card", FragmentCard);
