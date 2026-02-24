import { searchFragments } from "../api/search.ts";
import { navigate } from "../lib/router.ts";
import { shortId, relativeTime } from "../lib/format.ts";
import { tokenize } from "../lib/dsl.ts";
import { getSuggestions } from "../lib/dsl.ts";
import type { SearchResultDto } from "../api/types.ts";
import "../components/type-badge.ts";

export class SearchView extends HTMLElement {
  private shadow: ShadowRoot;
  private results: SearchResultDto[] = [];
  private query = "";
  private searchTimeout: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  static get observedAttributes(): string[] {
    return ["initial-query"];
  }

  connectedCallback(): void {
    this.query = this.getAttribute("initial-query") || "";
    this.render();
    if (this.query) this.doSearch();
  }

  private render(): void {
    this.shadow.innerHTML = `
      <style>
        :host { display: block; }
        .search-container {
          position: relative;
          margin-bottom: 16px;
        }
        .search-input {
          width: 100%;
          padding: 10px 14px;
          border: 2px solid var(--border);
          border-radius: var(--radius-lg);
          background: var(--bg);
          color: var(--text);
          font-size: 15px;
          outline: none;
          font-family: "SF Mono", "Fira Code", monospace;
        }
        .search-input:focus { border-color: var(--accent); }
        .chips {
          display: flex;
          gap: 6px;
          flex-wrap: wrap;
          margin-bottom: 12px;
        }
        .chip {
          display: inline-flex;
          align-items: center;
          gap: 4px;
          padding: 3px 10px;
          border-radius: 12px;
          font-size: 12px;
          font-family: monospace;
        }
        .chip-filter { background: var(--accent-light); color: var(--accent); }
        .chip-hashtag { background: var(--bg-code); color: var(--text-secondary); }
        .chip-word { background: var(--bg-surface); color: var(--text); border: 1px solid var(--border); }
        .suggestions {
          position: absolute;
          top: 100%;
          left: 0;
          right: 0;
          background: var(--bg-surface);
          border: 1px solid var(--border);
          border-radius: var(--radius-md);
          box-shadow: var(--shadow-md);
          z-index: 10;
          display: none;
          max-height: 200px;
          overflow-y: auto;
        }
        .suggestion {
          padding: 6px 12px;
          cursor: pointer;
          font-size: 13px;
          font-family: monospace;
        }
        .suggestion:hover { background: var(--bg-hover); }
        .count { font-size: 13px; color: var(--text-muted); margin-bottom: 8px; }
        .result {
          display: flex;
          align-items: center;
          gap: 10px;
          padding: 10px 12px;
          border-bottom: 1px solid var(--border-light);
          cursor: pointer;
        }
        .result:hover { background: var(--bg-hover); }
        .result-title { font-size: 14px; color: var(--text); flex: 1; }
        .result-id { font-family: monospace; font-size: 11px; color: var(--text-muted); }
        .result-time { font-size: 11px; color: var(--text-muted); }
        .snippet { font-size: 12px; color: var(--text-secondary); margin-top: 2px; }
        .empty { text-align: center; padding: 40px; color: var(--text-muted); }
      </style>
      <div class="search-container">
        <input type="text" class="search-input" id="search-input" value="${this.esc(this.query)}" placeholder="Search with DSL: type:todo status:open #tag keyword..." />
        <div class="suggestions" id="suggestions"></div>
      </div>
      <div class="chips" id="chips"></div>
      <div class="count" id="count"></div>
      <div id="results"></div>
    `;

    const input = this.shadow.getElementById("search-input") as HTMLInputElement;
    input.addEventListener("input", () => {
      this.query = input.value;
      this.updateChips();
      this.updateSuggestions();
      if (this.searchTimeout) clearTimeout(this.searchTimeout);
      this.searchTimeout = setTimeout(() => this.doSearch(), 300);
    });

    input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        this.doSearch();
      }
      if (e.key === "Escape") {
        this.shadow.getElementById("suggestions")!.style.display = "none";
      }
    });

    input.addEventListener("focus", () => this.updateSuggestions());
    input.addEventListener("blur", () => {
      setTimeout(() => {
        this.shadow.getElementById("suggestions")!.style.display = "none";
      }, 200);
    });

    this.updateChips();
  }

  private updateChips(): void {
    const container = this.shadow.getElementById("chips")!;
    const tokens = tokenize(this.query);
    container.innerHTML = tokens
      .map((t) => {
        const cls =
          t.type === "filter"
            ? "chip-filter"
            : t.type === "hashtag"
              ? "chip-hashtag"
              : "chip-word";
        const label =
          t.type === "filter"
            ? `${t.field}:${t.value}`
            : t.type === "hashtag"
              ? `#${t.value}`
              : t.value;
        return `<span class="chip ${cls}">${label}</span>`;
      })
      .join("");
  }

  private updateSuggestions(): void {
    const input = this.shadow.getElementById("search-input") as HTMLInputElement;
    const suggestions = getSuggestions(input.value, input.selectionStart || 0);
    const container = this.shadow.getElementById("suggestions")!;

    if (suggestions.length === 0) {
      container.style.display = "none";
      return;
    }

    container.style.display = "block";
    container.innerHTML = suggestions
      .slice(0, 8)
      .map((s) => `<div class="suggestion">${s}</div>`)
      .join("");

    container.querySelectorAll(".suggestion").forEach((el) => {
      el.addEventListener("mousedown", (e) => {
        e.preventDefault();
        // Replace current token with suggestion
        const tokens = tokenize(this.query);
        const cursorPos = input.selectionStart || 0;
        const currentToken = tokens.find((t) => cursorPos >= t.start && cursorPos <= t.end);

        if (currentToken) {
          const before = this.query.slice(0, currentToken.start);
          const after = this.query.slice(currentToken.end);
          this.query = before + el.textContent + after;
        } else {
          this.query += (this.query ? " " : "") + el.textContent;
        }

        input.value = this.query;
        input.focus();
        container.style.display = "none";
        this.updateChips();
        this.doSearch();
      });
    });
  }

  private async doSearch(): Promise<void> {
    if (!this.query.trim()) {
      this.shadow.getElementById("count")!.textContent = "";
      this.shadow.getElementById("results")!.innerHTML = `<div class="empty">Enter a search query</div>`;
      return;
    }

    try {
      this.results = await searchFragments(this.query);
      this.shadow.getElementById("count")!.textContent = `${this.results.length} result${this.results.length !== 1 ? "s" : ""}`;

      const container = this.shadow.getElementById("results")!;
      if (this.results.length === 0) {
        container.innerHTML = `<div class="empty">No results found</div>`;
        return;
      }

      container.innerHTML = this.results
        .map(
          (r) => `
          <div class="result" data-id="${r.id}">
            <type-badge type="${r.type}"></type-badge>
            <div>
              <div class="result-title">${this.esc(r.title || "Untitled")}</div>
              ${r.snippet ? `<div class="snippet">${r.snippet}</div>` : ""}
            </div>
            <span class="result-id">${shortId(r.id)}</span>
            <span class="result-time">${relativeTime(r.updated_at)}</span>
          </div>
        `
        )
        .join("");

      container.querySelectorAll(".result").forEach((el) => {
        el.addEventListener("click", () => {
          navigate(`fragment/${(el as HTMLElement).dataset.id}`);
        });
      });
    } catch (err) {
      this.shadow.getElementById("results")!.innerHTML = `<div class="empty" style="color: var(--type-risk)">Search error: ${err}</div>`;
    }
  }

  private esc(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }
}

customElements.define("search-view", SearchView);
