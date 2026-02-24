import { getFragment, createFragment, updateFragment } from "../api/fragments.ts";
import { listSchemas, getSchema } from "../api/schemas.ts";
import { renderMarkdown } from "../api/markdown.ts";
import { navigate } from "../lib/router.ts";
import type { FragmentDto, SchemaDto, SchemaFieldDto } from "../api/types.ts";

export class FragmentEditor extends HTMLElement {
  private shadow: ShadowRoot;
  private fragment: FragmentDto | null = null;
  private schema: SchemaDto | null = null;
  private isCreate = false;
  private previewTimeout: ReturnType<typeof setTimeout> | null = null;
  private draftKey = "";

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  static get observedAttributes(): string[] {
    return ["fragment-id", "create-type"];
  }

  attributeChangedCallback(): void {
    this.load();
  }

  connectedCallback(): void {
    this.load();
  }

  private async load(): Promise<void> {
    const id = this.getAttribute("fragment-id");
    const createType = this.getAttribute("create-type");

    if (id) {
      this.isCreate = false;
      this.fragment = await getFragment(id);
      this.schema = await getSchema(this.fragment.type);
      this.draftKey = `parc-draft-${id}`;
    } else if (createType) {
      this.isCreate = true;
      this.schema = await getSchema(createType);
      this.fragment = {
        id: "",
        type: createType,
        title: "",
        tags: [],
        links: [],
        attachments: [],
        created_at: "",
        updated_at: "",
        created_by: null,
        extra_fields: {},
        body: "",
      };
      // Apply defaults from schema
      for (const field of this.schema.fields) {
        if (field.default) {
          this.fragment.extra_fields[field.name] = field.default;
        }
      }
      this.draftKey = `parc-draft-new-${createType}`;
    }

    // Restore draft
    const draft = localStorage.getItem(this.draftKey);
    if (draft && this.fragment) {
      try {
        const d = JSON.parse(draft);
        this.fragment.title = d.title ?? this.fragment.title;
        this.fragment.body = d.body ?? this.fragment.body;
        this.fragment.tags = d.tags ?? this.fragment.tags;
      } catch { /* ignore */ }
    }

    this.render();
  }

  private render(): void {
    if (!this.fragment || !this.schema) return;
    const f = this.fragment;
    const s = this.schema;

    const fieldsHtml = s.fields
      .map((field) => this.renderField(field, f.extra_fields[field.name]))
      .join("");

    this.shadow.innerHTML = `
      <style>
        :host { display: block; max-width: 1000px; }
        .editor { display: grid; grid-template-columns: 350px 1fr; gap: 20px; }
        .form-section { display: flex; flex-direction: column; gap: 12px; }
        .body-section { display: flex; flex-direction: column; gap: 8px; }
        .field-group { display: flex; flex-direction: column; gap: 4px; }
        label { font-size: 12px; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.3px; }
        input, select, textarea {
          padding: 8px 10px;
          border: 1px solid var(--border);
          border-radius: var(--radius-md);
          background: var(--bg);
          color: var(--text);
          font-size: 14px;
          font-family: inherit;
        }
        input:focus, select:focus, textarea:focus { outline: none; border-color: var(--accent); }
        textarea {
          resize: vertical;
          min-height: 300px;
          font-family: "SF Mono", "Fira Code", monospace;
          font-size: 13px;
          line-height: 1.6;
        }
        .header { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0; flex: 1; }
        .actions { display: flex; gap: 8px; }
        .btn {
          padding: 6px 14px;
          border: 1px solid var(--border);
          border-radius: var(--radius-md);
          background: var(--bg);
          color: var(--text-secondary);
          font-size: 13px;
          cursor: pointer;
        }
        .btn:hover { background: var(--bg-hover); }
        .btn-primary { background: var(--accent); color: var(--accent-text); border-color: var(--accent); }
        .btn-primary:hover { opacity: 0.9; }
        .preview-toggle {
          font-size: 12px;
          color: var(--accent);
          cursor: pointer;
          background: none;
          border: none;
          padding: 4px 0;
        }
        .preview {
          border: 1px solid var(--border-light);
          border-radius: var(--radius-md);
          padding: 16px;
          min-height: 200px;
          background: var(--bg-surface);
        }
        .tags-input { display: flex; gap: 4px; flex-wrap: wrap; align-items: center; }
        .tag-item {
          display: inline-flex;
          align-items: center;
          gap: 4px;
          padding: 2px 8px;
          background: var(--bg-code);
          border-radius: 10px;
          font-size: 12px;
          color: var(--text-secondary);
        }
        .tag-remove { cursor: pointer; color: var(--text-muted); font-size: 14px; }
        .tag-add {
          border: none;
          background: none;
          padding: 2px 4px;
          font-size: 12px;
          color: var(--text-muted);
          min-width: 80px;
        }
        .unsaved { font-size: 11px; color: var(--type-todo); margin-left: 8px; }
      </style>
      <div class="header">
        <h2>${this.isCreate ? "New" : "Edit"} ${s.name}</h2>
        <span class="unsaved" id="unsaved" style="display:none">Unsaved changes</span>
        <div class="actions">
          <button class="btn" id="btn-cancel">Cancel</button>
          <button class="btn btn-primary" id="btn-save">${this.isCreate ? "Create" : "Save"}</button>
        </div>
      </div>
      <div class="editor">
        <div class="form-section">
          <div class="field-group">
            <label>Title</label>
            <input type="text" id="field-title" value="${this.esc(f.title)}" placeholder="Fragment title..." />
          </div>
          ${fieldsHtml}
          <div class="field-group">
            <label>Tags</label>
            <div class="tags-input" id="tags-input">
              ${f.tags.map((t) => `<span class="tag-item">#${t} <span class="tag-remove" data-tag="${t}">&times;</span></span>`).join("")}
              <input type="text" class="tag-add" id="tag-add" placeholder="Add tag..." />
            </div>
          </div>
        </div>
        <div class="body-section">
          <div style="display: flex; align-items: center; gap: 8px;">
            <label>Body</label>
            <button class="preview-toggle" id="preview-toggle">Preview</button>
          </div>
          <textarea id="field-body" placeholder="Write markdown...">${this.escAttr(f.body)}</textarea>
          <div class="preview rendered-md" id="preview" style="display:none"></div>
        </div>
      </div>
    `;

    this.wireEvents();
  }

  private renderField(field: SchemaFieldDto, value: unknown): string {
    const val = (value as string) ?? field.default ?? "";

    if (field.type === "enum") {
      const options = field.values
        .map((v) => `<option value="${v}" ${v === val ? "selected" : ""}>${v}</option>`)
        .join("");
      return `
        <div class="field-group">
          <label>${field.name}</label>
          <select id="extra-${field.name}">${options}</select>
        </div>
      `;
    }

    if (field.type === "date") {
      return `
        <div class="field-group">
          <label>${field.name}</label>
          <input type="date" id="extra-${field.name}" value="${val}" />
        </div>
      `;
    }

    return `
      <div class="field-group">
        <label>${field.name}</label>
        <input type="text" id="extra-${field.name}" value="${this.esc(val)}" />
      </div>
    `;
  }

  private wireEvents(): void {
    const markDirty = () => {
      this.shadow.getElementById("unsaved")!.style.display = "";
      this.saveDraft();
    };

    this.shadow.getElementById("field-title")?.addEventListener("input", markDirty);
    this.shadow.getElementById("field-body")?.addEventListener("input", markDirty);
    this.shadow.querySelectorAll("[id^='extra-']").forEach((el) =>
      el.addEventListener("change", markDirty)
    );

    // Preview toggle
    this.shadow.getElementById("preview-toggle")?.addEventListener("click", async () => {
      const preview = this.shadow.getElementById("preview")!;
      const textarea = this.shadow.getElementById("field-body") as HTMLTextAreaElement;
      if (preview.style.display === "none") {
        preview.innerHTML = await renderMarkdown(textarea.value);
        preview.style.display = "";
        textarea.style.display = "none";
      } else {
        preview.style.display = "none";
        textarea.style.display = "";
      }
    });

    // Tag add
    const tagAdd = this.shadow.getElementById("tag-add") as HTMLInputElement;
    tagAdd?.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" && tagAdd.value.trim()) {
        e.preventDefault();
        const tag = tagAdd.value.trim().toLowerCase();
        if (!this.fragment!.tags.includes(tag)) {
          this.fragment!.tags.push(tag);
          this.render();
          markDirty();
        }
      }
    });

    // Tag remove
    this.shadow.querySelectorAll(".tag-remove").forEach((el) => {
      el.addEventListener("click", () => {
        const tag = (el as HTMLElement).dataset.tag!;
        this.fragment!.tags = this.fragment!.tags.filter((t) => t !== tag);
        this.render();
        markDirty();
      });
    });

    // Cancel
    this.shadow.getElementById("btn-cancel")?.addEventListener("click", () => {
      localStorage.removeItem(this.draftKey);
      window.history.back();
    });

    // Save
    this.shadow.getElementById("btn-save")?.addEventListener("click", async () => {
      await this.save();
    });
  }

  private async save(): Promise<void> {
    const f = this.fragment!;
    const title = (this.shadow.getElementById("field-title") as HTMLInputElement).value;
    const body = (this.shadow.getElementById("field-body") as HTMLTextAreaElement).value;

    const extra: Record<string, unknown> = {};
    this.schema!.fields.forEach((field) => {
      const el = this.shadow.getElementById(`extra-${field.name}`) as HTMLInputElement | HTMLSelectElement;
      if (el?.value) extra[field.name] = el.value;
    });

    try {
      if (this.isCreate) {
        const result = await createFragment({
          type: f.type,
          title,
          body,
          tags: f.tags,
          status: extra.status as string | undefined,
          priority: extra.priority as string | undefined,
          due: extra.due as string | undefined,
          assignee: extra.assignee as string | undefined,
        });
        localStorage.removeItem(this.draftKey);
        navigate(`fragment/${result.id}`);
      } else {
        await updateFragment({
          id: f.id,
          title,
          body,
          tags: f.tags,
          extra_fields: extra,
        });
        localStorage.removeItem(this.draftKey);
        navigate(`fragment/${f.id}`);
      }
    } catch (err) {
      alert(`Save failed: ${err}`);
    }
  }

  private saveDraft(): void {
    const title = (this.shadow.getElementById("field-title") as HTMLInputElement)?.value;
    const body = (this.shadow.getElementById("field-body") as HTMLTextAreaElement)?.value;
    localStorage.setItem(this.draftKey, JSON.stringify({
      title,
      body,
      tags: this.fragment?.tags,
    }));
  }

  private esc(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  private escAttr(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
}

customElements.define("fragment-editor", FragmentEditor);
