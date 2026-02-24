import { getFragment, deleteFragment, archiveFragment } from "../api/fragments.ts";
import { renderMarkdown } from "../api/markdown.ts";
import { getBacklinks } from "../api/links.ts";
import { listAttachments } from "../api/attachments.ts";
import { shortId, relativeTime, formatSize } from "../lib/format.ts";
import { navigate } from "../lib/router.ts";
import type { FragmentDto, BacklinkDto, AttachmentInfoDto } from "../api/types.ts";
import "../components/type-badge.ts";
import "../components/status-badge.ts";
import "../components/tag-chip.ts";

export class FragmentDetail extends HTMLElement {
  private shadow: ShadowRoot;
  private fragment: FragmentDto | null = null;
  private backlinks: BacklinkDto[] = [];
  private attachments: AttachmentInfoDto[] = [];

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  static get observedAttributes(): string[] {
    return ["fragment-id"];
  }

  attributeChangedCallback(): void {
    this.load();
  }

  connectedCallback(): void {
    this.load();
  }

  private async load(): Promise<void> {
    const id = this.getAttribute("fragment-id");
    if (!id) return;

    try {
      const [frag, bl, att] = await Promise.all([
        getFragment(id),
        getBacklinks(id).catch(() => []),
        listAttachments(id).catch(() => []),
      ]);
      this.fragment = frag;
      this.backlinks = bl;
      this.attachments = att;
      await this.render();
    } catch (err) {
      this.shadow.innerHTML = `<div style="color: var(--type-risk); padding: 20px;">Error: ${err}</div>`;
    }
  }

  private async render(): Promise<void> {
    const f = this.fragment;
    if (!f) return;

    let bodyHtml = "";
    if (f.body.trim()) {
      bodyHtml = await renderMarkdown(f.body);
    }

    const status = f.extra_fields.status as string | undefined;
    const priority = f.extra_fields.priority as string | undefined;
    const due = f.extra_fields.due as string | undefined;
    const assignee = f.extra_fields.assignee as string | undefined;

    const tagsHtml = f.tags.map((t) => `<tag-chip tag="${t}"></tag-chip>`).join("");

    const extraFields = Object.entries(f.extra_fields)
      .filter(([k]) => !["status", "priority", "due", "assignee", "archived"].includes(k))
      .map(([k, v]) => `<div class="field"><span class="label">${k}</span><span class="value">${v}</span></div>`)
      .join("");

    const backlinksHtml = this.backlinks.length
      ? this.backlinks
          .map(
            (bl) =>
              `<div class="backlink" data-id="${bl.id}">
                <type-badge type="${bl.type}"></type-badge>
                <span>${this.esc(bl.title)}</span>
                <span class="bl-id">${shortId(bl.id)}</span>
              </div>`
          )
          .join("")
      : `<div class="empty-section">No backlinks</div>`;

    const attachmentsHtml = this.attachments.length
      ? this.attachments
          .map(
            (a) =>
              `<div class="attachment">
                <span class="att-name">${this.esc(a.filename)}</span>
                <span class="att-size">${formatSize(a.size)}</span>
              </div>`
          )
          .join("")
      : "";

    this.shadow.innerHTML = `
      <style>
        :host { display: block; max-width: 900px; }
        .header { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
        h1 { font-size: 22px; font-weight: 600; color: var(--text); flex: 1; margin: 0; }
        .actions { display: flex; gap: 8px; }
        .btn {
          padding: 6px 12px;
          border: 1px solid var(--border);
          border-radius: var(--radius-md);
          background: var(--bg);
          color: var(--text-secondary);
          font-size: 12px;
          cursor: pointer;
        }
        .btn:hover { background: var(--bg-hover); }
        .btn-danger { color: var(--type-risk); }
        .btn-danger:hover { background: var(--diff-del-bg); }
        .meta-grid {
          display: grid;
          grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
          gap: 8px;
          padding: 12px 16px;
          background: var(--bg-surface);
          border-radius: var(--radius-lg);
          border: 1px solid var(--border-light);
          margin-bottom: 16px;
        }
        .field { display: flex; flex-direction: column; gap: 2px; }
        .label { font-size: 11px; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.3px; }
        .value { font-size: 13px; color: var(--text); }
        .tags-row { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 16px; }
        .body { margin-bottom: 24px; }
        .section-title { font-size: 14px; font-weight: 600; color: var(--text); margin: 20px 0 8px 0; }
        .backlink {
          display: flex;
          align-items: center;
          gap: 8px;
          padding: 6px 10px;
          border-radius: var(--radius-sm);
          cursor: pointer;
          font-size: 13px;
        }
        .backlink:hover { background: var(--bg-hover); }
        .bl-id { font-family: monospace; font-size: 11px; color: var(--text-muted); margin-left: auto; }
        .attachment {
          display: flex;
          align-items: center;
          gap: 8px;
          padding: 6px 10px;
          border-radius: var(--radius-sm);
          font-size: 13px;
        }
        .att-name { color: var(--accent); }
        .att-size { font-size: 11px; color: var(--text-muted); margin-left: auto; }
        .id-line { font-family: monospace; font-size: 11px; color: var(--text-muted); margin-bottom: 12px; }
        .empty-section { font-size: 12px; color: var(--text-muted); padding: 4px 0; }
      </style>
      <div class="header">
        <type-badge type="${f.type}"></type-badge>
        <h1>${this.esc(f.title || "Untitled")}</h1>
        ${status ? `<status-badge status="${status}"></status-badge>` : ""}
      </div>
      <div class="id-line">${f.id} &middot; ${relativeTime(f.updated_at)}</div>
      <div class="meta-grid">
        ${priority ? `<div class="field"><span class="label">Priority</span><span class="value">${priority}</span></div>` : ""}
        ${due ? `<div class="field"><span class="label">Due</span><span class="value">${due}</span></div>` : ""}
        ${assignee ? `<div class="field"><span class="label">Assignee</span><span class="value">${assignee}</span></div>` : ""}
        <div class="field"><span class="label">Created</span><span class="value">${relativeTime(f.created_at)}</span></div>
        ${f.created_by ? `<div class="field"><span class="label">By</span><span class="value">${f.created_by}</span></div>` : ""}
        ${extraFields}
      </div>
      ${f.tags.length ? `<div class="tags-row">${tagsHtml}</div>` : ""}
      <div class="body rendered-md">${bodyHtml}</div>
      ${attachmentsHtml ? `<div class="section-title">Attachments</div>${attachmentsHtml}` : ""}
      <div class="section-title">Backlinks</div>
      ${backlinksHtml}
      <div class="actions" style="margin-top: 24px; padding-top: 16px; border-top: 1px solid var(--border-light)">
        <button class="btn" id="btn-edit">Edit</button>
        <button class="btn" id="btn-history">History</button>
        <button class="btn" id="btn-archive">Archive</button>
        <button class="btn btn-danger" id="btn-delete">Delete</button>
        <button class="btn" id="btn-copy" style="margin-left: auto">Copy ID</button>
      </div>
    `;

    // Wire up buttons
    this.shadow.getElementById("btn-edit")?.addEventListener("click", () => {
      navigate(`edit/${f.id}`);
    });

    this.shadow.getElementById("btn-history")?.addEventListener("click", () => {
      navigate(`history/${f.id}`);
    });

    this.shadow.getElementById("btn-archive")?.addEventListener("click", async () => {
      await archiveFragment(f.id);
      navigate("all");
    });

    this.shadow.getElementById("btn-delete")?.addEventListener("click", async () => {
      if (confirm(`Delete "${f.title}"?`)) {
        await deleteFragment(f.id);
        navigate("all");
      }
    });

    this.shadow.getElementById("btn-copy")?.addEventListener("click", () => {
      navigator.clipboard.writeText(f.id);
    });

    // Backlink navigation
    this.shadow.querySelectorAll(".backlink").forEach((el) => {
      el.addEventListener("click", () => {
        const id = (el as HTMLElement).dataset.id;
        if (id) navigate(`fragment/${id}`);
      });
    });
  }

  private esc(s: string): string {
    const el = document.createElement("span");
    el.textContent = s;
    return el.innerHTML;
  }
}

customElements.define("fragment-detail", FragmentDetail);
