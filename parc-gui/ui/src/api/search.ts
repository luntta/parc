import { invoke } from "@tauri-apps/api/core";
import type { SearchResultDto } from "./types.ts";

export async function searchFragments(
  query: string,
  limit?: number
): Promise<SearchResultDto[]> {
  return invoke("search_fragments", { params: { query, limit } });
}
