import { navigate } from "../lib/router.ts";

export class TagChip extends HTMLElement {
  static get observedAttributes(): string[] {
    return ["tag"];
  }

  connectedCallback(): void {
    this.render();
  }

  attributeChangedCallback(): void {
    this.render();
  }

  private render(): void {
    const tag = this.getAttribute("tag") || "";
    this.style.cssText = `
      display: inline-flex;
      align-items: center;
      padding: 1px 8px;
      border-radius: 10px;
      font-size: 11px;
      background: var(--bg-code);
      color: var(--text-secondary);
      cursor: pointer;
    `;
    this.textContent = `#${tag}`;
    this.onclick = (e) => {
      e.stopPropagation();
      navigate("search", { q: `tag:${tag}` });
    };
  }
}

customElements.define("tag-chip", TagChip);
