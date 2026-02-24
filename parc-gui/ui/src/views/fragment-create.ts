import { listSchemas } from "../api/schemas.ts";
import { navigate } from "../lib/router.ts";
import { typeColor } from "../lib/format.ts";
import type { SchemaDto } from "../api/types.ts";

export class FragmentCreate extends HTMLElement {
  private shadow: ShadowRoot;
  private schemas: SchemaDto[] = [];

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  async connectedCallback(): Promise<void> {
    this.schemas = await listSchemas();
    this.render();
  }

  private render(): void {
    const cards = this.schemas
      .map((s) => {
        const color = typeColor(s.name);
        return `
          <button class="type-card" data-type="${s.name}">
            <div class="dot" style="background: ${color}"></div>
            <div class="name">${s.name}</div>
            <div class="fields">${s.fields.length} fields</div>
          </button>
        `;
      })
      .join("");

    this.shadow.innerHTML = `
      <style>
        :host { display: block; max-width: 700px; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0 0 16px 0; }
        .grid {
          display: grid;
          grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
          gap: 12px;
        }
        .type-card {
          display: flex;
          flex-direction: column;
          align-items: center;
          gap: 8px;
          padding: 24px 16px;
          border: 1px solid var(--border);
          border-radius: var(--radius-lg);
          background: var(--bg-surface);
          cursor: pointer;
          transition: border-color 0.15s, box-shadow 0.15s;
        }
        .type-card:hover {
          border-color: var(--accent);
          box-shadow: var(--shadow-sm);
        }
        .dot { width: 12px; height: 12px; border-radius: 50%; }
        .name { font-size: 16px; font-weight: 600; color: var(--text); text-transform: capitalize; }
        .fields { font-size: 12px; color: var(--text-muted); }
      </style>
      <h2>New Fragment</h2>
      <p style="color: var(--text-secondary); margin-bottom: 20px; font-size: 14px;">Choose a type to get started</p>
      <div class="grid">${cards}</div>
    `;

    this.shadow.querySelectorAll(".type-card").forEach((el) => {
      el.addEventListener("click", () => {
        const type = (el as HTMLElement).dataset.type!;
        navigate(`create/${type}`);
      });
    });
  }
}

customElements.define("fragment-create", FragmentCreate);
