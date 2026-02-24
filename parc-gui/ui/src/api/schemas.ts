import { invoke } from "@tauri-apps/api/core";
import type { SchemaDto } from "./types.ts";

export async function listSchemas(): Promise<SchemaDto[]> {
  return invoke("list_schemas");
}

export async function getSchema(typeName: string): Promise<SchemaDto> {
  return invoke("get_schema", { params: { type: typeName } });
}
