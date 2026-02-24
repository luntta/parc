import { invoke } from "@tauri-apps/api/core";
import type { AttachmentInfoDto } from "./types.ts";

export async function attachFile(
  id: string,
  path: string
): Promise<AttachmentInfoDto> {
  return invoke("attach_file", { params: { id, path } });
}

export async function detachFile(
  id: string,
  filename: string
): Promise<boolean> {
  return invoke("detach_file", { params: { id, filename } });
}

export async function listAttachments(
  id: string
): Promise<AttachmentInfoDto[]> {
  return invoke("list_attachments", { id });
}

export async function getAttachmentPath(
  id: string,
  filename: string
): Promise<string> {
  return invoke("get_attachment_path", { id, filename });
}
