export class AppShell extends HTMLElement {
  private shadow: ShadowRoot;

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
        }
        .welcome {
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          height: 100%;
          color: var(--text-muted);
          font-size: 14px;
          gap: 8px;
        }
        .welcome h2 {
          color: var(--text);
          font-size: 20px;
          font-weight: 600;
          margin: 0;
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
        <div class="vault-info" id="vault-info">Loading vault...</div>
      </nav>
      <header class="topbar">
        <input type="text" class="search-input" placeholder="Search fragments... (Ctrl+K)" />
        <button class="btn-new">+ New</button>
      </header>
      <main class="main" id="main-content">
        <div class="welcome">
          <h2>parc</h2>
          <p>Personal Archive — select a category or create a new fragment</p>
        </div>
      </main>
    `;

    this.setupNavigation();
    this.loadVaultInfo();
  }

  private setupNavigation(): void {
    const items = this.shadow.querySelectorAll(".nav-item");
    items.forEach((item) => {
      item.addEventListener("click", () => {
        items.forEach((i) => i.classList.remove("active"));
        item.classList.add("active");
        const route = (item as HTMLElement).dataset.route;
        window.dispatchEvent(
          new CustomEvent("parc:navigate", { detail: { route } })
        );
      });
    });
  }

  private async loadVaultInfo(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const info = (await invoke("vault_info")) as {
        path: string;
        scope: string;
        fragment_count: number;
      };
      const el = this.shadow.getElementById("vault-info");
      if (el) {
        const short = info.path.replace(/^\/home\/[^/]+/, "~");
        el.textContent = `${short} (${info.fragment_count} fragments)`;
      }
    } catch {
      const el = this.shadow.getElementById("vault-info");
      if (el) el.textContent = "No vault connected";
    }
  }
}

customElements.define("app-shell", AppShell);
