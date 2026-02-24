import { invoke } from "@tauri-apps/api/core";
import type { VaultInfoDto, DoctorReportDto } from "./types.ts";

export async function getVaultInfo(): Promise<VaultInfoDto> {
  return invoke("vault_info");
}

export async function reindex(): Promise<number> {
  return invoke("reindex");
}

export async function runDoctor(): Promise<DoctorReportDto> {
  return invoke("doctor");
}

export async function switchVault(path: string): Promise<VaultInfoDto> {
  return invoke("switch_vault", { params: { path } });
}

export async function listVaults(): Promise<VaultInfoDto[]> {
  return invoke("list_vaults");
}

export async function initVault(path: string): Promise<VaultInfoDto> {
  return invoke("init_vault", { params: { path } });
}
