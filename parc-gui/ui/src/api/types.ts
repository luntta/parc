export interface FragmentDto {
  id: string;
  type: string;
  title: string;
  tags: string[];
  links: string[];
  attachments: string[];
  created_at: string;
  updated_at: string;
  created_by: string | null;
  extra_fields: Record<string, unknown>;
  body: string;
}

export interface FragmentSummaryDto {
  id: string;
  type: string;
  title: string;
  status: string | null;
  tags: string[];
  updated_at: string;
}

export interface SearchResultDto {
  id: string;
  type: string;
  title: string;
  status: string | null;
  tags: string[];
  updated_at: string;
  snippet: string | null;
}

export interface SchemaFieldDto {
  name: string;
  type: string;
  required: boolean;
  default: string | null;
  values: string[];
}

export interface SchemaDto {
  name: string;
  alias: string | null;
  editor_skip: boolean;
  fields: SchemaFieldDto[];
}

export interface TagCountDto {
  tag: string;
  count: number;
}

export interface VaultInfoDto {
  path: string;
  scope: string;
  fragment_count: number;
}

export interface VersionEntryDto {
  timestamp: string;
  size: number;
}

export interface AttachmentInfoDto {
  filename: string;
  size: number;
  path: string;
}

export interface DoctorFindingDto {
  type: string;
  details: Record<string, unknown>;
}

export interface DoctorReportDto {
  fragments_checked: number;
  healthy: boolean;
  findings: DoctorFindingDto[];
}

export interface BacklinkDto {
  id: string;
  type: string;
  title: string;
}

export interface DiffDto {
  diff: string;
}

export interface CreateFragmentParams {
  type: string;
  title?: string;
  tags?: string[];
  body?: string;
  links?: string[];
  due?: string;
  priority?: string;
  status?: string;
  assignee?: string;
}

export interface UpdateFragmentParams {
  id: string;
  title?: string;
  tags?: string[];
  body?: string;
  links?: string[];
  due?: string;
  priority?: string;
  status?: string;
  assignee?: string;
  extra_fields?: Record<string, unknown>;
}

export interface ListFragmentsParams {
  type?: string;
  status?: string;
  tag?: string;
  limit?: number;
}
