use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        FromRef, Path, Query, State,
    },
    response::Response,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;

use crate::{
    collaboration::PlaintextCollaborationHub,
    db::sql_store::{DynSqlStore, StoredPlainTextMap, StoredSharedUserGroup, StoredUser},
    error::AppError,
    middleware::auth::{AuthenticatedUser, JwtService},
    models::{
        access::{SubscriptionMode, UiSurface, UserAccessGrant},
        collaboration::{
            CollaborationClientMessage, CollaborationOpsResponse, CollaborationServerMessage,
            CollaborationSnapshotPayload, CollaborationSnapshotResponse,
        },
        plaintext_map::PlainTextAccessRole,
    },
};

#[derive(Clone)]
pub struct CollaborationSqlState {
    pub db: DynSqlStore,
    pub jwt: Arc<JwtService>,
    pub hub: PlaintextCollaborationHub,
}

impl FromRef<CollaborationSqlState> for Arc<JwtService> {
    fn from_ref(state: &CollaborationSqlState) -> Self {
        state.jwt.clone()
    }
}

pub fn router(state: CollaborationSqlState) -> Router {
    Router::new()
        .route("/maps/{id}/snapshot", get(get_snapshot))
        .route("/maps/{id}/ops", get(get_ops))
        .route("/maps/{id}/ws", get(connect_ws))
        .with_state(state)
}

#[derive(Debug, Default, Deserialize)]
struct OpsQuery {
    after_seq: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    access_token: String,
}

async fn get_snapshot(
    State(state): State<CollaborationSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<CollaborationSnapshotResponse>, AppError> {
    let current_user = require_collaboration_user(&state.db, &user.0).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let (map, role) = find_map_for_user(&state.db, &id, &current_user.id, &groups).await?;
    let room = state.hub.room(&id).await;
    let participants = room.participants().await;
    let seq = room.current_seq().await;

    Ok(Json(CollaborationSnapshotResponse {
        snapshot: snapshot_from_map(&map, seq, participants),
        access_role: role,
    }))
}

async fn get_ops(
    State(state): State<CollaborationSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(query): Query<OpsQuery>,
) -> Result<Json<CollaborationOpsResponse>, AppError> {
    let current_user = require_collaboration_user(&state.db, &user.0).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let _ = find_map_for_user(&state.db, &id, &current_user.id, &groups).await?;
    let room = state.hub.room(&id).await;
    let (current_seq, events) = room.events_after(query.after_seq.unwrap_or(0)).await;
    Ok(Json(CollaborationOpsResponse { current_seq, events }))
}

async fn connect_ws(
    State(state): State<CollaborationSqlState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let claims = state.jwt.validate_access_token(query.access_token.trim())?;
    let current_user = require_collaboration_user(&state.db, &claims.sub).await?;
    let groups = state
        .db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let (map, role) = find_map_for_user(&state.db, &id, &current_user.id, &groups).await?;
    let room = state.hub.room(&id).await;

    Ok(ws.on_upgrade(move |socket| async move {
        handle_socket(socket, room, state.db, map, current_user, role).await;
    }))
}

async fn handle_socket(
    mut socket: WebSocket,
    room: Arc<crate::collaboration::PlaintextCollaborationRoom>,
    db: DynSqlStore,
    map: StoredPlainTextMap,
    current_user: StoredUser,
    role: PlainTextAccessRole,
) {
    let mut receiver = room.subscribe();
    let (presence, seq) = room
        .join(current_user.id.clone(), current_user.username.clone(), role.clone())
        .await;
    let initial_participants = room.participants().await;

    if send_json(
        &mut socket,
        &CollaborationServerMessage::HelloAck {
            session_id: presence.session_id.clone(),
            role: role.clone(),
            seq,
        },
    )
    .await
    .is_err()
    {
        room.leave(&presence.session_id).await;
        return;
    }

    if send_json(
        &mut socket,
        &CollaborationServerMessage::Snapshot {
            snapshot: snapshot_from_map(&map, seq, initial_participants),
        },
    )
    .await
    .is_err()
    {
        room.leave(&presence.session_id).await;
        return;
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(payload))) => {
                        match serde_json::from_str::<CollaborationClientMessage>(&payload) {
                            Ok(CollaborationClientMessage::Hello { .. }) => {
                                let seq = room.current_seq().await;
                                let participants = room.participants().await;
                                if send_json(
                                    &mut socket,
                                    &CollaborationServerMessage::Snapshot {
                                        snapshot: snapshot_from_map(&map, seq, participants),
                                    },
                                ).await.is_err() {
                                    break;
                                }
                            }
                            Ok(CollaborationClientMessage::Ping) => {
                                if send_json(&mut socket, &CollaborationServerMessage::Pong).await.is_err() {
                                    break;
                                }
                            }
                            Ok(CollaborationClientMessage::PresenceUpdate { selected_node_id }) => {
                                let _ = room.update_presence(&presence.session_id, selected_node_id).await;
                            }
                            Ok(CollaborationClientMessage::DocumentUpdate { base_seq, title, summary, content_json }) => {
                                match apply_document_update_sql(
                                    &db,
                                    &room,
                                    &map.id,
                                    &current_user,
                                    base_seq,
                                    title,
                                    summary,
                                    content_json,
                                ).await {
                                    Ok(()) => {}
                                    Err(AppError::Conflict(current)) => {
                                        if send_json(&mut socket, &CollaborationServerMessage::ResyncRequired { seq: current.parse().unwrap_or_default(), reason: "stale_revision".to_string() }).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(error) => {
                                        if send_json(&mut socket, &CollaborationServerMessage::Error { message: error.to_string() }).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                if send_json(&mut socket, &CollaborationServerMessage::Error { message: format!("invalid collaboration message: {error}") }).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            broadcasted = receiver.recv() => {
                match broadcasted {
                    Ok(message) => {
                        if send_json(&mut socket, &message).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let seq = room.current_seq().await;
                        if send_json(&mut socket, &CollaborationServerMessage::ResyncRequired { seq, reason: "lagged".to_string() }).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    room.leave(&presence.session_id).await;
}

async fn apply_document_update_sql(
    db: &DynSqlStore,
    room: &Arc<crate::collaboration::PlaintextCollaborationRoom>,
    map_id: &str,
    current_user: &StoredUser,
    base_seq: u64,
    title: String,
    summary: Option<String>,
    content_json: serde_json::Value,
) -> Result<(), AppError> {
    validate_plaintext_map_payload(&title, &content_json)?;
    let groups = db
        .list_shared_user_groups_for_user(&current_user.id)
        .await?;
    let (map, role) = find_map_for_user(db, map_id, &current_user.id, &groups).await?;
    if !role.can_edit() {
        return Err(AppError::Unauthorized("edit access required".to_string()));
    }

    let now = Utc::now();
    let normalized_summary = normalize_optional_text(summary);
    let normalized_title = title.trim().to_string();
    room
        .apply_document(
            base_seq,
            normalized_title.clone(),
            normalized_summary.clone(),
            content_json.clone(),
            current_user.id.clone(),
            current_user.username.clone(),
            now,
            map.id.clone(),
            || async {
                db.update_plaintext_map(
                    &map.id,
                    crate::db::sql_store::PlainTextMapUpdate {
                        title: normalized_title.clone(),
                        summary: normalized_summary.clone(),
                        content_json: content_json.clone(),
                        direct_user_shares: map.direct_user_shares.clone(),
                        group_shares: map.group_shares.clone(),
                        updated_at: now,
                    },
                )
                .await
            },
        )
        .await?;

    Ok(())
}

fn snapshot_from_map(
    map: &StoredPlainTextMap,
    seq: u64,
    participants: Vec<crate::models::collaboration::CollaborationPresence>,
) -> CollaborationSnapshotPayload {
    CollaborationSnapshotPayload {
        map_id: map.id.clone(),
        seq,
        title: map.title.clone(),
        summary: map.summary.clone(),
        content_json: map.content_json.clone(),
        participants,
        updated_at: map.updated_at,
        actor_user_id: None,
        actor_username: None,
    }
}

async fn require_collaboration_user(db: &DynSqlStore, user_id: &str) -> Result<StoredUser, AppError> {
    let user = db
        .load_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    if !has_collaboration_access(&user.effective_access_grants(Utc::now())) {
        return Err(AppError::Unauthorized("plaintext collaboration access grant required".to_string()));
    }

    Ok(user)
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

fn has_collaboration_access(grants: &[UserAccessGrant]) -> bool {
    grants.iter().any(|grant| {
        ((matches!(grant.subscription_mode, SubscriptionMode::SharedPlaintext)
            && matches!(grant.ui_surface, UiSurface::SharedMapApp))
            || (matches!(grant.subscription_mode, SubscriptionMode::RealtimeCollaboration)
                && matches!(grant.ui_surface, UiSurface::CollaborationApp)))
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
        if groups.iter().any(|group| {
            group.id == share.group_id
                && (group.owner_user_id == user_id
                    || group.members.iter().any(|member| member.user_id == user_id))
        }) {
            resolved = Some(match resolved {
                Some(existing) if existing >= share.role => existing,
                _ => share.role.clone(),
            });
        }
    }

    resolved
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

async fn send_json(socket: &mut WebSocket, message: &CollaborationServerMessage) -> Result<(), AppError> {
    let payload = serde_json::to_string(message)
        .map_err(|error| AppError::Internal(format!("failed to serialize collaboration message: {error}")))?;
    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|error| AppError::ServiceUnavailable(format!("failed to send collaboration message: {error}")))
}