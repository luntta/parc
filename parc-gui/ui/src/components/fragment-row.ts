import { relativeTime, shortId } from "../lib/format.ts";
import type { FragmentSummaryDto } from "../api/types.ts";

export class FragmentRow extends HTMLElement {
  private _data: FragmentSummaryDto | null = null;

  set data(val: FragmentSummaryDto) {
    this._data = val;
    this.render();
  }

  private render(): void {
    const d = this._data;
    if (!d) return;

    this.style.cssText = `
      display: grid;
      grid-template-columns: 80px 70px 1fr 120px 100px;
      gap: 12px;
      align-items: center;
      padding: 8px 12px;
      border-bottom: 1px solid var(--border-light);
      font-size: 13px;
      cursor: pointer;
    `;

    this.innerHTML = `
      <span style="font-family: monospace; font-size: 11px; color: var(--text-muted)">${shortId(d.id)}</span>
      <type-badge type="${d.type}"></type-badge>
      <span style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text)">${this.esc(d.title || "Untitled")}</span>
      <span style="font-size: 12px; color: var(--text-muted)">${d.tags.slice(0, 2).map((t) => `#${t}`).join(" ")}</span>
      <span style="font-size: 11px; color: var(--text-muted); text-align: right">${relativeTime(d.updated_at)}</span>
    `;

    this.onmouseenter = () => (this.style.background = "var(--bg-hover)");
    this.onmouseleave = () => (this.style.background = "");
    this.onclick = () => {
      window.dispatchEvent(
        new CustomEvent("parc:navigate", { detail: { route: `fragment/${d.id}` } })
      );
    };
  }

  private esc(s: string): string {
    const el = document.createElement("span");
    el.textContent = s;
    return el.innerHTML;
  }
}

customElements.define("fragment-row", FragmentRow);
