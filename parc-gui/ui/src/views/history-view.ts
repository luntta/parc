import { listVersions, getVersion, restoreVersion, diffVersions } from "../api/history.ts";
import { getFragment } from "../api/fragments.ts";
import { shortId, relativeTime, formatSize } from "../lib/format.ts";
import { navigate } from "../lib/router.ts";
import type { VersionEntryDto, FragmentDto, DiffDto } from "../api/types.ts";

export class HistoryView extends HTMLElement {
  private shadow: ShadowRoot;
  private fragmentId = "";
  private fragment: FragmentDto | null = null;
  private versions: VersionEntryDto[] = [];
  private selectedVersion: string | null = null;
  private diffContent: string | null = null;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  static get observedAttributes(): string[] {
    return ["fragment-id"];
  }

  attributeChangedCallback(): void {
    this.fragmentId = this.getAttribute("fragment-id") || "";
    this.load();
  }

  connectedCallback(): void {
    this.fragmentId = this.getAttribute("fragment-id") || "";
    this.load();
  }

  private async load(): Promise<void> {
    if (!this.fragmentId) return;
    const [frag, vers] = await Promise.all([
      getFragment(this.fragmentId),
      listVersions(this.fragmentId),
    ]);
    this.fragment = frag;
    this.versions = vers;
    this.render();
  }

  private render(): void {
    const f = this.fragment;
    if (!f) return;

    this.shadow.innerHTML = `
      <style>
        :host { display: block; }
        .header { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0; }
        .back { color: var(--accent); cursor: pointer; font-size: 13px; background: none; border: none; }
        .layout { display: grid; grid-template-columns: 280px 1fr; gap: 20px; }
        .timeline { display: flex; flex-direction: column; gap: 0; }
        .version {
          display: flex; align-items: center; gap: 10px;
          padding: 10px 14px;
          border-left: 3px solid var(--border);
          cursor: pointer;
          transition: background 0.1s;
        }
        .version:hover { background: var(--bg-hover); }
        .version.active { border-left-color: var(--accent); background: var(--accent-light); }
        .version-dot {
          width: 8px; height: 8px; border-radius: 50%;
          background: var(--border);
          flex-shrink: 0;
        }
        .version.active .version-dot { background: var(--accent); }
        .version-info { flex: 1; }
        .version-time { font-size: 12px; color: var(--text); font-weight: 500; }
        .version-size { font-size: 11px; color: var(--text-muted); }
        .version-actions { display: flex; gap: 4px; }
        .btn-sm {
          padding: 2px 8px; border: 1px solid var(--border); border-radius: var(--radius-sm);
          background: var(--bg); color: var(--text-secondary); font-size: 11px; cursor: pointer;
        }
        .btn-sm:hover { background: var(--bg-hover); }
        .diff-view {
          background: var(--bg-surface);
          border: 1px solid var(--border);
          border-radius: var(--radius-md);
          padding: 16px;
          font-family: "SF Mono", "Fira Code", monospace;
          font-size: 12px;
          line-height: 1.6;
          overflow-x: auto;
          white-space: pre;
        }
        .diff-add { color: var(--diff-add-text); background: var(--diff-add-bg); }
        .diff-del { color: var(--diff-del-text); background: var(--diff-del-bg); }
        .diff-header { color: var(--accent); font-weight: 600; }
        .empty { text-align: center; padding: 40px; color: var(--text-muted); }
        .current-label {
          padding: 10px 14px;
          border-left: 3px solid var(--status-done);
          font-size: 12px; font-weight: 600; color: var(--status-done);
        }
      </style>
      <div class="header">
        <button class="back" id="btn-back">&larr; Back</button>
        <h2>History: ${this.esc(f.title)}</h2>
        <span style="font-size: 12px; color: var(--text-muted)">${shortId(f.id)}</span>
      </div>
      <div class="layout">
        <div>
          <div class="current-label">Current version</div>
          <div class="timeline" id="timeline">
            ${this.versions.length === 0
              ? `<div class="empty">No history versions</div>`
              : this.versions
                  .map(
                    (v) => `
                  <div class="version ${this.selectedVersion === v.timestamp ? "active" : ""}" data-ts="${v.timestamp}">
                    <span class="version-dot"></span>
                    <div class="version-info">
                      <div class="version-time">${this.formatTimestamp(v.timestamp)}</div>
                      <div class="version-size">${formatSize(v.size)}</div>
                    </div>
                  </div>
                `
                  )
                  .join("")}
          </div>
        </div>
        <div id="diff-container">
          <div class="empty">Select a version to see changes</div>
        </div>
      </div>
    `;

    this.shadow.getElementById("btn-back")?.addEventListener("click", () => {
      navigate(`fragment/${f.id}`);
    });

    this.shadow.querySelectorAll(".version").forEach((el) => {
      el.addEventListener("click", async () => {
        const ts = (el as HTMLElement).dataset.ts!;
        this.selectedVersion = ts;
        this.shadow.querySelectorAll(".version").forEach((v) => v.classList.remove("active"));
        el.classList.add("active");
        await this.showDiff(ts);
      });
    });
  }

  private async showDiff(timestamp: string): Promise<void> {
    const container = this.shadow.getElementById("diff-container")!;
    try {
      const result = await diffVersions(this.fragmentId, timestamp);
      const lines = result.diff.split("\n").map((line) => {
        if (line.startsWith("---") || line.startsWith("+++") || line.startsWith("@@")) {
          return `<span class="diff-header">${this.esc(line)}</span>`;
        }
        if (line.startsWith("+")) return `<span class="diff-add">${this.esc(line)}</span>`;
        if (line.startsWith("-")) return `<span class="diff-del">${this.esc(line)}</span>`;
        return this.esc(line);
      });

      container.innerHTML = `
        <div style="display: flex; gap: 8px; margin-bottom: 8px;">
          <button class="btn-sm" id="btn-restore">Restore this version</button>
        </div>
        <div class="diff-view">${lines.join("\n")}</div>
      `;

      container.querySelector("#btn-restore")?.addEventListener("click", async () => {
        if (confirm("Restore this version? Current changes will be saved as a snapshot.")) {
          await restoreVersion(this.fragmentId, timestamp);
          navigate(`fragment/${this.fragmentId}`);
        }
      });
    } catch (err) {
      container.innerHTML = `<div class="empty" style="color: var(--type-risk)">Error loading diff: ${err}</div>`;
    }
  }

  private formatTimestamp(ts: string): string {
    try {
      return new Date(ts).toLocaleString();
    } catch {
      return ts;
    }
  }

  private esc(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
}

customElements.define("history-view", HistoryView);
