use std::sync::Arc;

use axum::{
    extract::{FromRef, Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    db::sql_store::{
        DynSqlStore, NewPlainTextMap, NewSharedUserGroup, PlainTextMapUpdate, SharedUserGroupUpdate,
        StoredPlainTextMap, StoredSharedUserGroup,
    },
    error::AppError,
    middleware::auth::{AuthenticatedUser, JwtService},
    models::{
        access::{SubscriptionMode, UiSurface, UserAccessGrant},
        admin_audit::AdminAuditEvent,
        plaintext_map::{
            AddGroupMemberRequest, CreatePlainTextMapRequest, CreateSharedUserGroupRequest,
            DirectUserShare, GroupMember, GroupShare, PlainTextAccessRole, PlainTextMap,
            PlainTextMapDetail, PlainTextMapListItem, ShareMapWithGroupRequest,
            ShareMapWithUserRequest, SharedUserGroup, SharedUserGroupDetail,
            SharedUserGroupListItem, UpdatePlainTextMapRequest, UpdateSharedUserGroupRequest,
        },
    },
};

#[derive(Clone)]
pub struct PlainTextSqlState {
    pub db: DynSqlStore,
    pub jwt: Arc<JwtService>,
}

impl FromRef<PlainTextSqlState> for Arc<JwtService> {
    fn from_ref(state: &PlainTextSqlState) -> Self {
        state.jwt.clone()
    }
}

pub fn router(state: PlainTextSqlState) -> Router {
    Router::new()
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/{id}", get(get_group).put(update_group).delete(delete_group))
        .route("/groups/{id}/members", post(add_group_member))
        .route("/groups/{id}/members/{user_id}", delete(remove_group_member))
        .route("/maps", get(list_maps).post(create_map))
        .route("/maps/{id}", get(get_map).put(update_map).delete(delete_map))
        .route("/maps/{id}/shares/users", post(share_map_with_user))
        .route("/maps/{id}/shares/users/{user_id}", delete(remove_map_user_share))
        .route("/maps/{id}/shares/groups", post(share_map_with_group))
        .route("/maps/{id}/shares/groups/{group_id}", delete(remove_map_group_share))
        .with_state(state)
}

async fn list_groups(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<SharedUserGroupListItem>>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;

    Ok(Json(groups.into_iter().map(|group| to_group(group).to_list_item()).collect()))
}

async fn create_group(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Json(body): Json<CreateSharedUserGroupRequest>,
) -> Result<Json<SharedUserGroupDetail>, AppError> {
    validate_group_name(&body.name)?;

    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let now = Utc::now();
    let id = Uuid::new_v4().to_string();
    let description = normalize_optional_text(body.description);
    let name = body.name.trim().to_string();

    state
        .db
        .create_shared_user_group(NewSharedUserGroup {
            id: id.clone(),
            owner_user_id: current_user.id.clone(),
            owner_username: current_user.username.clone(),
            name: name.clone(),
            description: description.clone(),
            members: Vec::new(),
            created_at: now,
            updated_at: now,
        })
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_group",
            &id,
            "plaintext_group_created",
            format!("{} created plaintext group {}", current_user.username, name),
            description.clone(),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(SharedUserGroup {
        id,
        owner_user_id: current_user.id,
        owner_username: current_user.username,
        name,
        description,
        members: Vec::new(),
        created_at: now,
        updated_at: now,
    }
    .to_detail()))
}

async fn get_group(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<SharedUserGroupDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let group = find_group_for_user(&state.db, &id, &current_user.id).await?;
    Ok(Json(to_group(group).to_detail()))
}

async fn update_group(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateSharedUserGroupRequest>,
) -> Result<Json<SharedUserGroupDetail>, AppError> {
    validate_group_name(&body.name)?;
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let group = find_group_owned(&state.db, &id, &current_user.id).await?;
    let now = Utc::now();
    let description = normalize_optional_text(body.description);
    let name = body.name.trim().to_string();

    state
        .db
        .update_shared_user_group(
            &group.id,
            &current_user.id,
            SharedUserGroupUpdate {
                name: name.clone(),
                description: description.clone(),
                members: group.members.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_group",
            &group.id,
            "plaintext_group_updated",
            format!("{} updated plaintext group {}", current_user.username, name),
            description.clone(),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(SharedUserGroup {
        id: group.id,
        owner_user_id: group.owner_user_id,
        owner_username: group.owner_username,
        name,
        description,
        members: group.members,
        created_at: group.created_at,
        updated_at: now,
    }
    .to_detail()))
}

async fn delete_group(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let group = find_group_owned(&state.db, &id, &current_user.id).await?;
    state.db.delete_shared_user_group(&group.id, &current_user.id).await?;
    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_group",
            &group.id,
            "plaintext_group_deleted",
            format!("{} deleted plaintext group {}", current_user.username, group.name),
            group.description.clone(),
            Some(current_user.username.clone()),
        ),
    )
    .await?;
    Ok(Json(serde_json::json!({ "message": "deleted" })))
}

async fn add_group_member(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<AddGroupMemberRequest>,
) -> Result<Json<SharedUserGroupDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let group = find_group_owned(&state.db, &id, &current_user.id).await?;
    let target_user = state
        .db
        .load_user_by_username(body.username.trim())
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    if target_user.id == current_user.id {
        return Err(AppError::BadRequest("group owner is already part of the group".to_string()));
    }

    if group.members.iter().any(|member| member.user_id == target_user.id) {
        return Err(AppError::Conflict("user is already a member of the group".to_string()));
    }

    let now = Utc::now();
    let mut members = group.members.clone();
    members.push(GroupMember {
        user_id: target_user.id,
        username: target_user.username,
        added_at: now,
    });

    state
        .db
        .update_shared_user_group(
            &group.id,
            &current_user.id,
            SharedUserGroupUpdate {
                name: group.name.clone(),
                description: group.description.clone(),
                members: members.clone(),
                updated_at: now,
            },
        )
        .await?;

    let added_member = members.last().cloned();
    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_group",
            &group.id,
            "plaintext_group_member_added",
            format!("{} added {} to group {}", current_user.username, body.username.trim(), group.name),
            added_member.map(|member| format!("role=member user_id={}", member.user_id)),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(SharedUserGroup {
        id: group.id,
        owner_user_id: group.owner_user_id,
        owner_username: group.owner_username,
        name: group.name,
        description: group.description,
        members,
        created_at: group.created_at,
        updated_at: now,
    }
    .to_detail()))
}

async fn remove_group_member(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<Json<SharedUserGroupDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let group = find_group_owned(&state.db, &id, &current_user.id).await?;
    let removed_member = group.members.iter().find(|member| member.user_id == user_id).cloned();

    let members: Vec<GroupMember> = group
        .members
        .iter()
        .filter(|member| member.user_id != user_id)
        .cloned()
        .collect();

    let now = Utc::now();
    state
        .db
        .update_shared_user_group(
            &group.id,
            &current_user.id,
            SharedUserGroupUpdate {
                name: group.name.clone(),
                description: group.description.clone(),
                members: members.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_group",
            &group.id,
            "plaintext_group_member_removed",
            format!("{} removed a member from group {}", current_user.username, group.name),
            removed_member.map(|member| format!("username={} user_id={}", member.username, member.user_id)),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(SharedUserGroup {
        id: group.id,
        owner_user_id: group.owner_user_id,
        owner_username: group.owner_username,
        name: group.name,
        description: group.description,
        members,
        created_at: group.created_at,
        updated_at: now,
    }
    .to_detail()))
}

async fn list_maps(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<PlainTextMapListItem>>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let maps = state.db.list_plaintext_maps_for_user(&current_user.id).await?;

    let items = maps
        .into_iter()
        .filter_map(|map| resolve_plaintext_role(&map, &current_user.id, &groups).map(|role| to_map(map).to_list_item(role)))
        .collect();

    Ok(Json(items))
}

async fn create_map(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Json(body): Json<CreatePlainTextMapRequest>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    validate_plaintext_map_payload(&body.title, &body.content_json)?;
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let now = Utc::now();
    let id = Uuid::new_v4().to_string();
    let title = body.title.trim().to_string();
    let summary = normalize_optional_text(body.summary);

    state
        .db
        .create_plaintext_map(NewPlainTextMap {
            id: id.clone(),
            owner_user_id: current_user.id.clone(),
            owner_username: current_user.username.clone(),
            title: title.clone(),
            summary: summary.clone(),
            content_json: body.content_json.clone(),
            direct_user_shares: Vec::new(),
            group_shares: Vec::new(),
            created_at: now,
            updated_at: now,
        })
        .await?;

    Ok(Json(PlainTextMap {
        id: Some(id),
        owner_user_id: current_user.id,
        owner_username: current_user.username,
        title,
        summary,
        content_json: body.content_json,
        direct_user_shares: Vec::new(),
        group_shares: Vec::new(),
        created_at: now,
        updated_at: now,
    }
    .to_detail(PlainTextAccessRole::Owner)))
}

async fn get_map(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let (map, role) = find_map_for_user(&state.db, &id, &current_user.id, &groups).await?;
    Ok(Json(to_map(map).to_detail(role)))
}

async fn update_map(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<UpdatePlainTextMapRequest>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    validate_plaintext_map_payload(&body.title, &body.content_json)?;
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let (map, role) = find_map_for_user(&state.db, &id, &current_user.id, &groups).await?;

    if !role.can_edit() {
        return Err(AppError::Unauthorized("edit access required".to_string()));
    }

    let now = Utc::now();
    let title = body.title.trim().to_string();
    let summary = normalize_optional_text(body.summary);

    state
        .db
        .update_plaintext_map(
            &map.id,
            PlainTextMapUpdate {
                title: title.clone(),
                summary: summary.clone(),
                content_json: body.content_json.clone(),
                direct_user_shares: map.direct_user_shares.clone(),
                group_shares: map.group_shares.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_map",
            &map.id,
            "plaintext_map_updated",
            format!("{} updated plaintext map {}", current_user.username, map.title),
            Some(format!("title={}", title)),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(PlainTextMap {
        id: Some(map.id),
        owner_user_id: map.owner_user_id,
        owner_username: map.owner_username,
        title,
        summary,
        content_json: body.content_json,
        direct_user_shares: map.direct_user_shares,
        group_shares: map.group_shares,
        created_at: map.created_at,
        updated_at: now,
    }
    .to_detail(role)))
}

async fn delete_map(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let map = find_map_owned(&state.db, &id, &current_user.id).await?;
    state.db.delete_plaintext_map(&map.id, &current_user.id).await?;
    Ok(Json(serde_json::json!({ "message": "deleted" })))
}

async fn share_map_with_user(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<ShareMapWithUserRequest>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let map = find_map_owned(&state.db, &id, &current_user.id).await?;
    let target_user = state
        .db
        .load_user_by_username(body.username.trim())
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    if target_user.id == current_user.id {
        return Err(AppError::BadRequest("owner already has access".to_string()));
    }

    let now = Utc::now();
    let target_user_id = target_user.id.clone();
    let target_username = target_user.username.clone();
    let mut direct_user_shares: Vec<DirectUserShare> = map
        .direct_user_shares
        .iter()
        .filter(|share| share.user_id != target_user_id)
        .cloned()
        .collect();
    direct_user_shares.push(DirectUserShare {
        user_id: target_user_id.clone(),
        username: target_username.clone(),
        role: body.role.clone(),
        shared_at: now,
    });

    state
        .db
        .update_plaintext_map(
            &map.id,
            PlainTextMapUpdate {
                title: map.title.clone(),
                summary: map.summary.clone(),
                content_json: map.content_json.clone(),
                direct_user_shares: direct_user_shares.clone(),
                group_shares: map.group_shares.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_map",
            &map.id,
            "plaintext_map_user_shared",
            format!("{} shared plaintext map {} with {}", current_user.username, map.title, target_username),
            Some(format!("role={:?} user_id={}", body.role, target_user_id).to_lowercase()),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(PlainTextMap {
        id: Some(map.id),
        owner_user_id: map.owner_user_id,
        owner_username: map.owner_username,
        title: map.title,
        summary: map.summary,
        content_json: map.content_json,
        direct_user_shares,
        group_shares: map.group_shares,
        created_at: map.created_at,
        updated_at: now,
    }
    .to_detail(PlainTextAccessRole::Owner)))
}

async fn remove_map_user_share(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let map = find_map_owned(&state.db, &id, &current_user.id).await?;
    let direct_user_shares: Vec<DirectUserShare> = map
        .direct_user_shares
        .iter()
        .filter(|share| share.user_id != user_id)
        .cloned()
        .collect();
    let now = Utc::now();

    state
        .db
        .update_plaintext_map(
            &map.id,
            PlainTextMapUpdate {
                title: map.title.clone(),
                summary: map.summary.clone(),
                content_json: map.content_json.clone(),
                direct_user_shares: direct_user_shares.clone(),
                group_shares: map.group_shares.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_map",
            &map.id,
            "plaintext_map_user_share_removed",
            format!("{} removed a direct share from plaintext map {}", current_user.username, map.title),
            Some(format!("user_id={user_id}")),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(PlainTextMap {
        id: Some(map.id),
        owner_user_id: map.owner_user_id,
        owner_username: map.owner_username,
        title: map.title,
        summary: map.summary,
        content_json: map.content_json,
        direct_user_shares,
        group_shares: map.group_shares,
        created_at: map.created_at,
        updated_at: now,
    }
    .to_detail(PlainTextAccessRole::Owner)))
}

async fn share_map_with_group(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<ShareMapWithGroupRequest>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let map = find_map_owned(&state.db, &id, &current_user.id).await?;
    let group = find_group_for_user(&state.db, &body.group_id, &current_user.id).await?;
    let now = Utc::now();

    let mut group_shares: Vec<GroupShare> = map
        .group_shares
        .iter()
        .filter(|share| share.group_id != group.id)
        .cloned()
        .collect();
    group_shares.push(GroupShare {
        group_id: group.id.clone(),
        group_name: group.name.clone(),
        role: body.role.clone(),
        shared_at: now,
    });

    state
        .db
        .update_plaintext_map(
            &map.id,
            PlainTextMapUpdate {
                title: map.title.clone(),
                summary: map.summary.clone(),
                content_json: map.content_json.clone(),
                direct_user_shares: map.direct_user_shares.clone(),
                group_shares: group_shares.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_map",
            &map.id,
            "plaintext_map_group_shared",
            format!("{} shared plaintext map {} with group {}", current_user.username, map.title, group.name),
            Some(format!("role={:?} group_id={}", body.role, group.id).to_lowercase()),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(PlainTextMap {
        id: Some(map.id),
        owner_user_id: map.owner_user_id,
        owner_username: map.owner_username,
        title: map.title,
        summary: map.summary,
        content_json: map.content_json,
        direct_user_shares: map.direct_user_shares,
        group_shares,
        created_at: map.created_at,
        updated_at: now,
    }
    .to_detail(PlainTextAccessRole::Owner)))
}

async fn remove_map_group_share(
    State(state): State<PlainTextSqlState>,
    user: AuthenticatedUser,
    Path((id, group_id)): Path<(String, String)>,
) -> Result<Json<PlainTextMapDetail>, AppError> {
    let current_user = require_plaintext_user(&state.db, &user.0).await?;
    let map = find_map_owned(&state.db, &id, &current_user.id).await?;
    let group_shares: Vec<GroupShare> = map
        .group_shares
        .iter()
        .filter(|share| share.group_id != group_id)
        .cloned()
        .collect();
    let now = Utc::now();

    state
        .db
        .update_plaintext_map(
            &map.id,
            PlainTextMapUpdate {
                title: map.title.clone(),
                summary: map.summary.clone(),
                content_json: map.content_json.clone(),
                direct_user_shares: map.direct_user_shares.clone(),
                group_shares: group_shares.clone(),
                updated_at: now,
            },
        )
        .await?;

    write_audit_event(
        &state.db,
        make_audit_event(
            "plaintext_map",
            &map.id,
            "plaintext_map_group_share_removed",
            format!("{} removed a group share from plaintext map {}", current_user.username, map.title),
            Some(format!("group_id={group_id}")),
            Some(current_user.username.clone()),
        ),
    )
    .await?;

    Ok(Json(PlainTextMap {
        id: Some(map.id),
        owner_user_id: map.owner_user_id,
        owner_username: map.owner_username,
        title: map.title,
        summary: map.summary,
        content_json: map.content_json,
        direct_user_shares: map.direct_user_shares,
        group_shares,
        created_at: map.created_at,
        updated_at: now,
    }
    .to_detail(PlainTextAccessRole::Owner)))
}

async fn require_plaintext_user(
    db: &DynSqlStore,
    user_id: &str,
) -> Result<crate::db::sql_store::StoredUser, AppError> {
    let user = db
        .load_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    if !has_plaintext_access(&user.effective_access_grants(Utc::now())) {
        return Err(AppError::Unauthorized("plaintext shared map access grant required".to_string()));
    }

    Ok(user)
}

async fn find_group_for_user(
    db: &DynSqlStore,
    id: &str,
    user_id: &str,
) -> Result<StoredSharedUserGroup, AppError> {
    let group = db
        .get_shared_user_group(id)
        .await?
        .ok_or_else(|| AppError::NotFound("group not found".to_string()))?;

    if group.owner_user_id != user_id && !group.members.iter().any(|member| member.user_id == user_id) {
        return Err(AppError::NotFound("group not found".to_string()));
    }

    Ok(group)
}

async fn find_group_owned(
    db: &DynSqlStore,
    id: &str,
    user_id: &str,
) -> Result<StoredSharedUserGroup, AppError> {
    let group = find_group_for_user(db, id, user_id).await?;
    if group.owner_user_id != user_id {
        return Err(AppError::Unauthorized("group owner access required".to_string()));
    }
    Ok(group)
}

async fn find_map_for_user(
    db: &DynSqlStore,
    id: &str,
    user_id: &str,
    groups: &[StoredSharedUserGroup],
) -> Result<(StoredPlainTextMap, PlainTextAccessRole), AppError> {
    let map = db
        .get_plaintext_map(id)
        .await?
        .ok_or_else(|| AppError::NotFound("plaintext map not found".to_string()))?;
    let role = resolve_plaintext_role(&map, user_id, groups)
        .ok_or_else(|| AppError::NotFound("plaintext map not found".to_string()))?;
    Ok((map, role))
}

async fn find_map_owned(
    db: &DynSqlStore,
    id: &str,
    user_id: &str,
) -> Result<StoredPlainTextMap, AppError> {
    let map = db
        .get_plaintext_map(id)
        .await?
        .ok_or_else(|| AppError::NotFound("plaintext map not found".to_string()))?;

    if map.owner_user_id != user_id {
        return Err(AppError::Unauthorized("map owner access required".to_string()));
    }

    Ok(map)
}

fn has_plaintext_access(grants: &[UserAccessGrant]) -> bool {
    grants.iter().any(|grant| {
        matches!(grant.subscription_mode, SubscriptionMode::SharedPlaintext)
            && matches!(grant.ui_surface, UiSurface::SharedMapApp)
            && grant.is_active(Utc::now())
    })
}

fn resolve_plaintext_role(
    map: &StoredPlainTextMap,
    user_id: &str,
    groups: &[StoredSharedUserGroup],
) -> Option<PlainTextAccessRole> {
    if map.owner_user_id == user_id {
        return Some(PlainTextAccessRole::Owner);
    }

    let mut resolved = map
        .direct_user_shares
        .iter()
        .filter(|share| share.user_id == user_id)
        .map(|share| share.role.clone())
        .max();

    for share in &map.group_shares {
        if groups.iter().any(|group| group.id == share.group_id && (group.owner_user_id == user_id || group.members.iter().any(|member| member.user_id == user_id))) {
            resolved = Some(match resolved {
                Some(existing) if existing >= share.role => existing,
                _ => share.role.clone(),
            });
        }
    }

    resolved
}

fn to_group(group: StoredSharedUserGroup) -> SharedUserGroup {
    SharedUserGroup {
        id: group.id,
        owner_user_id: group.owner_user_id,
        owner_username: group.owner_username,
        name: group.name,
        description: group.description,
        members: group.members,
        created_at: group.created_at,
        updated_at: group.updated_at,
    }
}

fn to_map(map: StoredPlainTextMap) -> PlainTextMap {
    PlainTextMap {
        id: Some(map.id),
        owner_user_id: map.owner_user_id,
        owner_username: map.owner_username,
        title: map.title,
        summary: map.summary,
        content_json: map.content_json,
        direct_user_shares: map.direct_user_shares,
        group_shares: map.group_shares,
        created_at: map.created_at,
        updated_at: map.updated_at,
    }
}

fn validate_group_name(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("group name is required".to_string()));
    }
    Ok(())
}

fn validate_plaintext_map_payload(title: &str, content_json: &serde_json::Value) -> Result<(), AppError> {
    if title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".to_string()));
    }

    if !(content_json.is_object() || content_json.is_array()) {
        return Err(AppError::BadRequest("content_json must be a JSON object or array".to_string()));
    }

    Ok(())
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

async fn write_audit_event(db: &DynSqlStore, event: AdminAuditEvent) -> Result<(), AppError> {
    db.create_admin_audit_event(event).await
}

fn make_audit_event(
    entity_type: &str,
    entity_id: &str,
    action_type: &str,
    summary: String,
    detail: Option<String>,
    actor: Option<String>,
) -> AdminAuditEvent {
    AdminAuditEvent {
        id: None,
        public_id: Uuid::new_v4().to_string(),
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        action_type: action_type.to_string(),
        summary,
        detail,
        actor,
        created_at: Utc::now(),
    }
}