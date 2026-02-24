import { invoke } from "@tauri-apps/api/core";

export async function renderMarkdown(content: string): Promise<string> {
  return invoke("render_markdown", { content });
}
