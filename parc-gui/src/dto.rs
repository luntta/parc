use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentDto {
    pub id: String,
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub attachments: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
    pub extra_fields: BTreeMap<String, serde_json::Value>,
    pub body: String,
}

impl From<&parc_core::fragment::Fragment> for FragmentDto {
    fn from(f: &parc_core::fragment::Fragment) -> Self {
        Self {
            id: f.id.clone(),
            fragment_type: f.fragment_type.clone(),
            title: f.title.clone(),
            tags: f.tags.clone(),
            links: f.links.clone(),
            attachments: f.attachments.clone(),
            created_at: f.created_at.to_rfc3339(),
            updated_at: f.updated_at.to_rfc3339(),
            created_by: f.created_by.clone(),
            extra_fields: f.extra_fields.clone(),
            body: f.body.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentSummaryDto {
    pub id: String,
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: String,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub id: String,
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: String,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaFieldDto {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub required: bool,
    pub default: Option<String>,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDto {
    pub name: String,
    pub alias: Option<String>,
    pub editor_skip: bool,
    pub fields: Vec<SchemaFieldDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCountDto {
    pub tag: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultInfoDto {
    pub path: String,
    pub scope: String,
    pub fragment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntryDto {
    pub timestamp: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfoDto {
    pub filename: String,
    pub size: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorFindingDto {
    #[serde(rename = "type")]
    pub finding_type: String,
    pub details: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReportDto {
    pub fragments_checked: usize,
    pub healthy: bool,
    pub findings: Vec<DoctorFindingDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacklinkDto {
    pub id: String,
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffDto {
    pub diff: String,
}
