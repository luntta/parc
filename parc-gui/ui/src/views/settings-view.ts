import { getTheme, setTheme, type Theme } from "../lib/theme.ts";
import { getPrefs, setPrefs } from "../lib/state.ts";

export class SettingsView extends HTMLElement {
  private shadow: ShadowRoot;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  connectedCallback(): void {
    this.render();
  }

  private render(): void {
    const theme = getTheme();
    const prefs = getPrefs();

    this.shadow.innerHTML = `
      <style>
        :host { display: block; max-width: 600px; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0 0 20px 0; }
        .section { margin-bottom: 24px; }
        .section-title { font-size: 14px; font-weight: 600; color: var(--text); margin-bottom: 12px; }
        .setting {
          display: flex;
          align-items: center;
          justify-content: space-between;
          padding: 10px 0;
          border-bottom: 1px solid var(--border-light);
        }
        .setting-label { font-size: 13px; color: var(--text); }
        .setting-desc { font-size: 11px; color: var(--text-muted); margin-top: 2px; }
        select, input[type="number"] {
          padding: 4px 8px;
          border: 1px solid var(--border);
          border-radius: var(--radius-sm);
          background: var(--bg);
          color: var(--text);
          font-size: 13px;
        }
        .toggle {
          position: relative;
          width: 40px;
          height: 22px;
          background: var(--border);
          border-radius: 11px;
          cursor: pointer;
          transition: background 0.2s;
        }
        .toggle.on { background: var(--accent); }
        .toggle::after {
          content: '';
          position: absolute;
          width: 18px;
          height: 18px;
          border-radius: 50%;
          background: white;
          top: 2px;
          left: 2px;
          transition: transform 0.2s;
        }
        .toggle.on::after { transform: translateX(18px); }
      </style>
      <h2>Settings</h2>

      <div class="section">
        <div class="section-title">Appearance</div>
        <div class="setting">
          <div>
            <div class="setting-label">Theme</div>
            <div class="setting-desc">Choose light, dark, or follow system preference</div>
          </div>
          <select id="theme">
            <option value="system" ${theme === "system" ? "selected" : ""}>System</option>
            <option value="light" ${theme === "light" ? "selected" : ""}>Light</option>
            <option value="dark" ${theme === "dark" ? "selected" : ""}>Dark</option>
          </select>
        </div>
      </div>

      <div class="section">
        <div class="section-title">Editor</div>
        <div class="setting">
          <div>
            <div class="setting-label">Font Size</div>
            <div class="setting-desc">Editor font size in pixels</div>
          </div>
          <input type="number" id="font-size" value="${prefs.editorFontSize}" min="10" max="24" style="width: 60px" />
        </div>
        <div class="setting">
          <div>
            <div class="setting-label">Show Preview</div>
            <div class="setting-desc">Show markdown preview by default</div>
          </div>
          <div class="toggle ${prefs.showPreview ? "on" : ""}" id="preview-toggle"></div>
        </div>
      </div>
    `;

    this.shadow.getElementById("theme")?.addEventListener("change", (e) => {
      const value = (e.target as HTMLSelectElement).value as Theme;
      setTheme(value);
      setPrefs({ theme: value });
    });

    this.shadow.getElementById("font-size")?.addEventListener("change", (e) => {
      setPrefs({ editorFontSize: parseInt((e.target as HTMLInputElement).value) });
    });

    this.shadow.getElementById("preview-toggle")?.addEventListener("click", (e) => {
      const el = e.currentTarget as HTMLElement;
      const isOn = el.classList.toggle("on");
      setPrefs({ showPreview: isOn });
    });
  }
}

customElements.define("settings-view", SettingsView);
