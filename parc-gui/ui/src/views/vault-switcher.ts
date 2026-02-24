import { getVaultInfo, listVaults, reindex, runDoctor, switchVault } from "../api/vault.ts";
import type { VaultInfoDto, DoctorReportDto } from "../api/types.ts";

export class VaultSwitcher extends HTMLElement {
  private shadow: ShadowRoot;
  private currentVault: VaultInfoDto | null = null;
  private vaults: VaultInfoDto[] = [];

  constructor() {
    super();
    this.shadow = this.attachShadow({ mode: "open" });
  }

  async connectedCallback(): Promise<void> {
    const [current, all] = await Promise.all([
      getVaultInfo(),
      listVaults(),
    ]);
    this.currentVault = current;
    this.vaults = all;
    this.render();
  }

  private render(): void {
    const v = this.currentVault;
    if (!v) return;

    const vaultOptions = this.vaults
      .map(
        (vault) => `
        <div class="vault-option ${vault.path === v.path ? "active" : ""}" data-path="${vault.path}">
          <span class="vault-path">${vault.path.replace(/^\/home\/[^/]+/, "~")}</span>
          <span class="vault-meta">${vault.scope} &middot; ${vault.fragment_count} fragments</span>
        </div>
      `
      )
      .join("");

    this.shadow.innerHTML = `
      <style>
        :host { display: block; max-width: 600px; }
        h2 { font-size: 18px; font-weight: 600; color: var(--text); margin: 0 0 16px 0; }
        .current {
          padding: 16px;
          background: var(--bg-surface);
          border: 1px solid var(--border);
          border-radius: var(--radius-lg);
          margin-bottom: 20px;
        }
        .current-path { font-size: 14px; font-weight: 600; color: var(--text); font-family: monospace; }
        .current-meta { font-size: 12px; color: var(--text-muted); margin-top: 4px; }
        .actions { display: flex; gap: 8px; margin-top: 12px; }
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
        .section-title { font-size: 14px; font-weight: 600; color: var(--text); margin: 20px 0 8px 0; }
        .vault-option {
          padding: 10px 14px;
          border: 1px solid var(--border-light);
          border-radius: var(--radius-md);
          margin-bottom: 8px;
          cursor: pointer;
        }
        .vault-option:hover { border-color: var(--accent); }
        .vault-option.active { border-color: var(--accent); background: var(--accent-light); }
        .vault-path { font-size: 13px; font-family: monospace; color: var(--text); }
        .vault-meta { font-size: 11px; color: var(--text-muted); display: block; margin-top: 2px; }
        .doctor-result { margin-top: 16px; padding: 12px; background: var(--bg-surface); border-radius: var(--radius-md); border: 1px solid var(--border-light); }
        .healthy { color: var(--status-done); }
        .unhealthy { color: var(--type-risk); }
        .finding { font-size: 12px; padding: 4px 0; color: var(--text-secondary); }
        .status-msg { font-size: 12px; color: var(--text-muted); margin-top: 8px; }
      </style>
      <h2>Vault</h2>
      <div class="current">
        <div class="current-path">${v.path}</div>
        <div class="current-meta">${v.scope} &middot; ${v.fragment_count} fragments</div>
        <div class="actions">
          <button class="btn" id="btn-reindex">Reindex</button>
          <button class="btn" id="btn-doctor">Doctor</button>
        </div>
        <div id="status"></div>
      </div>
      <div class="section-title">Available Vaults</div>
      ${vaultOptions}
    `;

    this.shadow.getElementById("btn-reindex")?.addEventListener("click", async () => {
      const status = this.shadow.getElementById("status")!;
      status.innerHTML = `<div class="status-msg">Reindexing...</div>`;
      const count = await reindex();
      status.innerHTML = `<div class="status-msg">Reindexed ${count} fragments</div>`;
    });

    this.shadow.getElementById("btn-doctor")?.addEventListener("click", async () => {
      const status = this.shadow.getElementById("status")!;
      status.innerHTML = `<div class="status-msg">Running doctor...</div>`;
      const report = await runDoctor();
      const cls = report.healthy ? "healthy" : "unhealthy";
      const findings = report.findings
        .map((f) => `<div class="finding">${f.type}: ${JSON.stringify(f.details)}</div>`)
        .join("");
      status.innerHTML = `
        <div class="doctor-result">
          <div class="${cls}">${report.healthy ? "Healthy" : "Issues found"} (${report.fragments_checked} checked)</div>
          ${findings}
        </div>
      `;
    });

    this.shadow.querySelectorAll(".vault-option").forEach((el) => {
      el.addEventListener("click", async () => {
        const path = (el as HTMLElement).dataset.path!;
        if (path !== this.currentVault?.path) {
          this.currentVault = await switchVault(path);
          this.render();
          window.dispatchEvent(new CustomEvent("parc:vault-changed"));
        }
      });
    });
  }
}

customElements.define("vault-switcher", VaultSwitcher);
