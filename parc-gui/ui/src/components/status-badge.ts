import { statusColor } from "../lib/format.ts";

export class StatusBadge extends HTMLElement {
  static get observedAttributes(): string[] {
    return ["status"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    this.render();
  }

  private render(): void {
    const status = this.getAttribute("status") || "";
    if (!status) {
      this.style.display = "none";
      return;
    }
    const color = statusColor(status);
    this.style.cssText = `
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 2px 8px;
      border-radius: 4px;
      font-size: 11px;
      font-weight: 500;
      background: ${color}18;
      color: ${color};
    `;
    this.textContent = status;
  }
}

customElements.define("status-badge", StatusBadge);
