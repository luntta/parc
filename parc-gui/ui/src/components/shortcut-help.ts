import { getShortcuts } from "../lib/keyboard.ts";

export class ShortcutHelp extends HTMLElement {
  private shadow: ShadowRoot;

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
          align-items: center;
        }
        .modal {
          width: 480px;
          max-height: 500px;
          background: var(--bg-surface);
          border: 1px solid var(--border);
          border-radius: var(--radius-xl);
          box-shadow: var(--shadow-lg);
          overflow: hidden;
        }
        .header {
          display: flex;
          align-items: center;
          justify-content: space-between;
          padding: 16px 20px;
          border-bottom: 1px solid var(--border-light);
        }
        h3 { font-size: 16px; font-weight: 600; color: var(--text); margin: 0; }
        .close {
          background: none; border: none; color: var(--text-muted);
          font-size: 18px; cursor: pointer;
        }
        .shortcuts { padding: 12px 20px; overflow-y: auto; max-height: 400px; }
        .shortcut {
          display: flex;
          align-items: center;
          justify-content: space-between;
          padding: 6px 0;
          border-bottom: 1px solid var(--border-light);
        }
        .shortcut:last-child { border-bottom: none; }
        .desc { font-size: 13px; color: var(--text); }
        .keys { display: flex; gap: 4px; }
        kbd {
          padding: 2px 8px;
          background: var(--bg-code);
          border: 1px solid var(--border);
          border-radius: 4px;
          font-size: 12px;
          font-family: monospace;
          color: var(--text);
        }
      </style>
      <div class="overlay" id="overlay">
        <div class="modal">
          <div class="header">
            <h3>Keyboard Shortcuts</h3>
            <button class="close" id="close">&times;</button>
          </div>
          <div class="shortcuts" id="shortcuts"></div>
        </div>
      </div>
    `;

    this.shadow.getElementById("overlay")?.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).id === "overlay") this.hide();
    });
    this.shadow.getElementById("close")?.addEventListener("click", () => this.hide());
  }

  show(): void {
    this.classList.add("visible");
    const container = this.shadow.getElementById("shortcuts")!;
    const shortcuts = getShortcuts();

    container.innerHTML = shortcuts
      .map((s) => {
        const keys: string[] = [];
        if (s.ctrl) keys.push("Ctrl");
        if (s.shift) keys.push("Shift");
        if (s.alt) keys.push("Alt");
        keys.push(s.key.length === 1 ? s.key.toUpperCase() : s.key);

        return `
          <div class="shortcut">
            <span class="desc">${s.description}</span>
            <span class="keys">${keys.map((k) => `<kbd>${k}</kbd>`).join("")}</span>
          </div>
        `;
      })
      .join("");
  }

  hide(): void {
    this.classList.remove("visible");
  }

  toggle(): void {
    if (this.classList.contains("visible")) this.hide();
    else this.show();
  }
}

customElements.define("shortcut-help", ShortcutHelp);
