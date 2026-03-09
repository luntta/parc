import { onRouteChange, navigate, type Route } from "../lib/router.ts";
import { getVaultInfo } from "../api/vault.ts";
import "../views/fragment-list.ts";
import "../views/fragment-detail.ts";
import "../views/fragment-editor.ts";
import "../views/fragment-create.ts";
import "../views/search-view.ts";
import "../views/tag-browser.ts";
import "../views/vault-switcher.ts";
import "../views/history-view.ts";
import "../views/graph-view.ts";
import "../views/settings-view.ts";

export class AppShell extends HTMLElement {
  private shadow: ShadowRoot;
  private cleanup: (() => void) | null = null;

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  connectedCallback(): void {
    this.shadow.innerHTML = `
      <style>
        :host {
          display: grid;
          grid-template-columns: 240px 1fr;
          grid-template-rows: 48px 1fr;
          grid-template-areas:
            "sidebar topbar"
            "sidebar main";
          height: 100vh;
          width: 100vw;
          overflow: hidden;
          color: var(--text);
          background: var(--bg);
        }
        .topbar {
          grid-area: topbar;
          display: flex;
          align-items: center;
          gap: 12px;
          padding: 0 16px;
          border-bottom: 1px solid var(--border);
          background: var(--bg-surface);
        }
        .sidebar {
          grid-area: sidebar;
          display: flex;
          flex-direction: column;
          border-right: 1px solid var(--border);
          background: var(--bg-sidebar);
          overflow-y: auto;
          padding: 12px 0;
        }
        .main {
          grid-area: main;
          overflow-y: auto;
          padding: 24px;
          background: var(--bg);
        }
        .search-input {
          flex: 1;
          padding: 6px 12px;
          border: 1px solid var(--border);
          border-radius: 6px;
          background: var(--bg);
          color: var(--text);
          font-size: 14px;
          outline: none;
        }
        .search-input:focus {
          border-color: var(--accent);
        }
        .search-input::placeholder {
          color: var(--text-muted);
        }
        .btn-new {
          padding: 6px 14px;
          border: none;
          border-radius: 6px;
          background: var(--accent);
          color: var(--accent-text);
          font-size: 13px;
          font-weight: 600;
          cursor: pointer;
          white-space: nowrap;
        }
        .btn-new:hover {
          opacity: 0.9;
        }
        .nav-section {
          padding: 4px 16px;
          font-size: 11px;
          font-weight: 600;
          text-transform: uppercase;
          color: var(--text-muted);
          letter-spacing: 0.5px;
          margin-top: 12px;
        }
        .nav-section:first-child {
          margin-top: 0;
        }
        .nav-item {
          display: flex;
          align-items: center;
          gap: 8px;
          padding: 6px 16px;
          font-size: 13px;
          color: var(--text-secondary);
          cursor: pointer;
          text-decoration: none;
          border: none;
          background: none;
          width: 100%;
          text-align: left;
        }
        .nav-item:hover {
          background: var(--bg-hover);
          color: var(--text);
        }
        .nav-item.active {
          background: var(--bg-active);
          color: var(--accent);
          font-weight: 600;
        }
        .nav-dot {
          width: 8px;
          height: 8px;
          border-radius: 50%;
          flex-shrink: 0;
        }
        .vault-info {
          padding: 12px 16px;
          font-size: 12px;
          color: var(--text-muted);
          border-top: 1px solid var(--border);
          margin-top: auto;
          cursor: pointer;
        }
        .vault-info:hover {
          color: var(--text);
        }
      </style>
      <nav class="sidebar">
        <div class="nav-section">Fragments</div>
        <button class="nav-item active" data-route="all">All</button>
        <button class="nav-item" data-route="note">
          <span class="nav-dot" style="background: var(--type-note)"></span>
          Notes
        </button>
        <button class="nav-item" data-route="todo">
          <span class="nav-dot" style="background: var(--type-todo)"></span>
          Todos
        </button>
        <button class="nav-item" data-route="decision">
          <span class="nav-dot" style="background: var(--type-decision)"></span>
          Decisions
        </button>
        <button class="nav-item" data-route="risk">
          <span class="nav-dot" style="background: var(--type-risk)"></span>
          Risks
        </button>
        <button class="nav-item" data-route="idea">
          <span class="nav-dot" style="background: var(--type-idea)"></span>
          Ideas
        </button>
        <div class="nav-section">Browse</div>
        <button class="nav-item" data-route="tags">Tags</button>
        <button class="nav-item" data-route="graph">Graph</button>
        <button class="nav-item" data-route="trash">Trash</button>
        <div class="nav-section">System</div>
        <button class="nav-item" data-route="vault">Vault</button>
        <button class="nav-item" data-route="settings">Settings</button>
        <div class="vault-info" id="vault-info">Loading vault...</div>
      </nav>
      <header class="topbar">
        <input type="text" class="search-input" id="search-input" placeholder="Search fragments... (Ctrl+K)" />
        <button class="btn-new" id="btn-new">+ New</button>
      </header>
      <main class="main" id="main-content"></main>
    `;

    this.setupNavigation();
    this.setupTopbar();
    this.loadVaultInfo();

    this.cleanup = onRouteChange((route) => this.renderRoute(route));
  }

  disconnectedCallback(): void {
    this.cleanup?.();
  }

  private setupNavigation(): void {
    const items = this.shadow.querySelectorAll(".nav-item");
    items.forEach((item) => {
      item.addEventListener("click", () => {
        const route = (item as HTMLElement).dataset.route!;
        navigate(route);
      });
    });
  }

  private setupTopbar(): void {
    const searchInput = this.shadow.getElementById("search-input") as HTMLInputElement;
    searchInput.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" && searchInput.value.trim()) {
        navigate("search", { q: searchInput.value.trim() });
        searchInput.value = "";
        searchInput.blur();
      }
    });

    this.shadow.getElementById("btn-new")?.addEventListener("click", () => {
      navigate("new");
    });
  }

  private renderRoute(route: Route): void {
    const main = this.shadow.getElementById("main-content")!;
    const path = route.path;

    // Update active nav item
    this.shadow.querySelectorAll(".nav-item").forEach((item) => {
      const itemRoute = (item as HTMLElement).dataset.route!;
      item.classList.toggle("active", this.isNavActive(itemRoute, path));
    });

    // Determine which view to render
    const parts = path.split("/");
    const root = parts[0];

    // Clear previous view
    main.innerHTML = "";

    switch (root) {
      case "all":
      case "note":
      case "todo":
      case "decision":
      case "risk":
      case "idea":
      case "trash": {
        const list = document.createElement("fragment-list");
        if (root !== "all") list.setAttribute("type", root);
        if (root === "trash") list.setAttribute("show-trash", "true");
        main.appendChild(list);
        break;
      }
      case "fragment": {
        const id = parts[1] || "";
        if (route.params.edit === "true") {
          const editor = document.createElement("fragment-editor");
          editor.setAttribute("fragment-id", id);
          main.appendChild(editor);
        } else {
          const detail = document.createElement("fragment-detail");
          detail.setAttribute("fragment-id", id);
          main.appendChild(detail);
        }
        break;
      }
      case "edit": {
        const id = parts[1] || "";
        const editor = document.createElement("fragment-editor");
        editor.setAttribute("fragment-id", id);
        main.appendChild(editor);
        break;
      }
      case "new": {
        const create = document.createElement("fragment-create");
        if (route.params.type) create.setAttribute("initial-type", route.params.type);
        main.appendChild(create);
        break;
      }
      case "search": {
        const search = document.createElement("search-view");
        if (route.params.q) search.setAttribute("query", route.params.q);
        main.appendChild(search);
        break;
      }
      case "tags": {
        main.appendChild(document.createElement("tag-browser"));
        break;
      }
      case "graph": {
        main.appendChild(document.createElement("graph-view"));
        break;
      }
      case "vault": {
        main.appendChild(document.createElement("vault-switcher"));
        break;
      }
      case "history": {
        const id = parts[1] || "";
        const history = document.createElement("history-view");
        history.setAttribute("fragment-id", id);
        main.appendChild(history);
        break;
      }
      case "settings": {
        main.appendChild(document.createElement("settings-view"));
        break;
      }
      default: {
        const list = document.createElement("fragment-list");
        main.appendChild(list);
        break;
      }
    }
  }

  private isNavActive(navRoute: string, currentPath: string): boolean {
    const root = currentPath.split("/")[0];
    if (navRoute === root) return true;
    if (navRoute === "all" && ["fragment", "edit", "new", "search", ""].includes(root)) return false;
    if (navRoute === "all" && !["note", "todo", "decision", "risk", "idea", "tags", "graph", "trash", "vault", "settings", "search", "new", "fragment", "edit", "history"].includes(root)) return true;
    return false;
  }

  private async loadVaultInfo(): Promise<void> {
    const el = this.shadow.getElementById("vault-info");
    if (!el) return;
    try {
      const info = await getVaultInfo();
      const short = info.path.replace(/^\/home\/[^/]+/, "~");
      el.textContent = `${short} (${info.fragment_count} fragments)`;
    } catch {
      el.textContent = "No vault connected";
    }
    el.addEventListener("click", () => navigate("vault"));
  }
}

customElements.define("app-shell", AppShell);
