import { invoke } from "@tauri-apps/api/core";
import type { TagCountDto } from "./types.ts";

export async function listTags(): Promise<TagCountDto[]> {
  return invoke("list_tags");
}
