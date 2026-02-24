import { invoke } from "@tauri-apps/api/core";
import type {
  FragmentDto,
  FragmentSummaryDto,
  CreateFragmentParams,
  UpdateFragmentParams,
  ListFragmentsParams,
} from "./types.ts";

export async function listFragments(
  params: ListFragmentsParams = {}
): Promise<FragmentSummaryDto[]> {
  return invoke("list_fragments", { params });
}

export async function getFragment(id: string): Promise<FragmentDto> {
  return invoke("get_fragment", { id });
}

export async function createFragment(
  params: CreateFragmentParams
): Promise<FragmentDto> {
  return invoke("create_fragment", { params });
}

export async function updateFragment(
  params: UpdateFragmentParams
): Promise<FragmentDto> {
  return invoke("update_fragment", { params });
}

export async function deleteFragment(id: string): Promise<string> {
  return invoke("delete_fragment", { id });
}

export async function archiveFragment(id: string): Promise<FragmentDto> {
  return invoke("archive_fragment", { id });
}
