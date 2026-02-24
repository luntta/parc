import { invoke } from "@tauri-apps/api/core";
import type { BacklinkDto } from "./types.ts";

export async function linkFragments(
  idA: string,
  idB: string
): Promise<string[]> {
  return invoke("link_fragments", { params: { id_a: idA, id_b: idB } });
}

export async function unlinkFragments(
  idA: string,
  idB: string
): Promise<string[]> {
  return invoke("unlink_fragments", { params: { id_a: idA, id_b: idB } });
}

export async function getBacklinks(id: string): Promise<BacklinkDto[]> {
  return invoke("get_backlinks", { id });
}
