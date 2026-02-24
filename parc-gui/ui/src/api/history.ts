import { invoke } from "@tauri-apps/api/core";
import type {
  VersionEntryDto,
  FragmentDto,
  DiffDto,
} from "./types.ts";

export async function listVersions(id: string): Promise<VersionEntryDto[]> {
  return invoke("list_versions", { id });
}

export async function getVersion(
  id: string,
  timestamp: string
): Promise<FragmentDto> {
  return invoke("get_version", { params: { id, timestamp } });
}

export async function restoreVersion(
  id: string,
  timestamp: string
): Promise<FragmentDto> {
  return invoke("restore_version", { params: { id, timestamp } });
}

export async function diffVersions(
  id: string,
  timestamp?: string
): Promise<DiffDto> {
  return invoke("diff_versions", { params: { id, timestamp } });
}
