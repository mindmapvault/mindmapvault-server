use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PlainTextAccessRole {
    Viewer,
    Editor,
    Owner,
}

impl PlainTextAccessRole {
    pub fn can_edit(&self) -> bool {
        matches!(self, Self::Editor | Self::Owner)
    }

    pub fn can_manage_shares(&self) -> bool {
        matches!(self, Self::Owner)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupMember {
    pub user_id: String,
    pub username: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedUserGroup {
    #[serde(rename = "_id")]
    pub id: String,
    pub owner_user_id: String,
    pub owner_username: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub members: Vec<GroupMember>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectUserShare {
    pub user_id: String,
    pub username: String,
    pub role: PlainTextAccessRole,
    pub shared_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupShare {
    pub group_id: String,
    pub group_name: String,
    pub role: PlainTextAccessRole,
    pub shared_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlainTextMap {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub owner_user_id: String,
    pub owner_username: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub content_json: Value,
    #[serde(default)]
    pub direct_user_shares: Vec<DirectUserShare>,
    #[serde(default)]
    pub group_shares: Vec<GroupShare>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSharedUserGroupRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSharedUserGroupRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddGroupMemberRequest {
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct SharedUserGroupListItem {
    pub id: String,
    pub owner_user_id: String,
    pub owner_username: String,
    pub name: String,
    pub description: Option<String>,
    pub member_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SharedUserGroupDetail {
    pub id: String,
    pub owner_user_id: String,
    pub owner_username: String,
    pub name: String,
    pub description: Option<String>,
    pub members: Vec<GroupMember>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlainTextMapRequest {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub content_json: Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlainTextMapRequest {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub content_json: Value,
}

#[derive(Debug, Deserialize)]
pub struct ShareMapWithUserRequest {
    pub username: String,
    pub role: PlainTextAccessRole,
}

#[derive(Debug, Deserialize)]
pub struct ShareMapWithGroupRequest {
    pub group_id: String,
    pub role: PlainTextAccessRole,
}

#[derive(Debug, Serialize)]
pub struct PlainTextMapListItem {
    pub id: String,
    pub owner_user_id: String,
    pub owner_username: String,
    pub title: String,
    pub summary: Option<String>,
    pub access_role: PlainTextAccessRole,
    pub direct_user_share_count: usize,
    pub group_share_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PlainTextMapDetail {
    pub id: String,
    pub owner_user_id: String,
    pub owner_username: String,
    pub title: String,
    pub summary: Option<String>,
    pub content_json: Value,
    pub access_role: PlainTextAccessRole,
    pub direct_user_shares: Vec<DirectUserShare>,
    pub group_shares: Vec<GroupShare>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SharedUserGroup {
    pub fn to_list_item(&self) -> SharedUserGroupListItem {
        SharedUserGroupListItem {
            id: self.id.clone(),
            owner_user_id: self.owner_user_id.clone(),
            owner_username: self.owner_username.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            member_count: self.members.len(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn to_detail(&self) -> SharedUserGroupDetail {
        SharedUserGroupDetail {
            id: self.id.clone(),
            owner_user_id: self.owner_user_id.clone(),
            owner_username: self.owner_username.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            members: self.members.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

impl PlainTextMap {
    pub fn to_list_item(&self, access_role: PlainTextAccessRole) -> PlainTextMapListItem {
        PlainTextMapListItem {
            id: self.id.clone().unwrap_or_default(),
            owner_user_id: self.owner_user_id.clone(),
            owner_username: self.owner_username.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            access_role,
            direct_user_share_count: self.direct_user_shares.len(),
            group_share_count: self.group_shares.len(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn to_detail(&self, access_role: PlainTextAccessRole) -> PlainTextMapDetail {
        PlainTextMapDetail {
            id: self.id.clone().unwrap_or_default(),
            owner_user_id: self.owner_user_id.clone(),
            owner_username: self.owner_username.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            content_json: self.content_json.clone(),
            access_role,
            direct_user_shares: self.direct_user_shares.clone(),
            group_shares: self.group_shares.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}