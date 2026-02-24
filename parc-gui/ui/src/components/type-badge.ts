import { typeColor } from "../lib/format.ts";

export class TypeBadge extends HTMLElement {
  static get observedAttributes(): string[] {
    return ["type"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    this.render();
  }

  private render(): void {
    const type = this.getAttribute("type") || "note";
    const color = typeColor(type);
    this.style.cssText = `
      display: inline-flex;
      align-items: center;
      gap: 4px;
      padding: 2px 8px;
      border-radius: 4px;
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.3px;
      background: ${color}20;
      color: ${color};
    `;
    this.textContent = type;
  }
}

customElements.define("type-badge", TypeBadge);
