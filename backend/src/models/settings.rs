use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccountSettings {
    pub locale: String,
    pub timezone: String,
    pub date_format: String,
    pub accessibility_reduce_motion: bool,
    pub sync_appearance_across_devices: bool,
    pub default_share_expiry_days: i32,
    pub default_include_attachments_on_share: bool,
    pub default_map_layout: String,
    pub default_map_theme: String,
    pub default_export_format: String,
    pub default_node_style_preset: String,
    /// User label library stored as JSON string: [{"name":"...","color":"#..."}]
    pub user_labels_json: String,
    pub updated_at: DateTime<Utc>,
}

impl Default for UserAccountSettings {
    fn default() -> Self {
        Self {
            locale: "en".to_string(),
            timezone: "UTC".to_string(),
            date_format: "iso".to_string(),
            accessibility_reduce_motion: false,
            sync_appearance_across_devices: false,
            default_share_expiry_days: 7,
            default_include_attachments_on_share: false,
            default_map_layout: "mindmap".to_string(),
            default_map_theme: "system".to_string(),
            default_export_format: "cryptmind".to_string(),
            default_node_style_preset: "default".to_string(),
            user_labels_json: "[]".to_string(),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserAccountSettingsRequest {
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub date_format: Option<String>,
    pub accessibility_reduce_motion: Option<bool>,
    pub sync_appearance_across_devices: Option<bool>,
    pub default_share_expiry_days: Option<i32>,
    pub default_include_attachments_on_share: Option<bool>,
    pub default_map_layout: Option<String>,
    pub default_map_theme: Option<String>,
    pub default_export_format: Option<String>,
    pub default_node_style_preset: Option<String>,
    pub user_labels_json: Option<String>,
}