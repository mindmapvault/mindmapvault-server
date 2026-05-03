use std::{collections::HashMap, future::Future, sync::Arc};

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex, RwLock};
use uuid::Uuid;

use crate::models::{
    collaboration::{
        CollaborationEvent, CollaborationPresence, CollaborationServerMessage,
        CollaborationSnapshotPayload,
    },
    plaintext_map::PlainTextAccessRole,
};

#[derive(Clone, Default)]
pub struct PlaintextCollaborationHub {
    rooms: Arc<RwLock<HashMap<String, Arc<PlaintextCollaborationRoom>>>>,
}

impl PlaintextCollaborationHub {
    pub async fn room(&self, map_id: &str) -> Arc<PlaintextCollaborationRoom> {
        if let Some(room) = self.rooms.read().await.get(map_id).cloned() {
            return room;
        }

        let mut rooms = self.rooms.write().await;
        rooms
            .entry(map_id.to_string())
            .or_insert_with(|| Arc::new(PlaintextCollaborationRoom::new(map_id.to_string())))
            .clone()
    }
}

pub struct PlaintextCollaborationRoom {
    tx: broadcast::Sender<CollaborationServerMessage>,
    state: Mutex<RoomState>,
}

struct RoomState {
    current_seq: u64,
    history: Vec<CollaborationEvent>,
    participants: HashMap<String, CollaborationPresence>,
}

impl PlaintextCollaborationRoom {
    fn new(_map_id: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tx,
            state: Mutex::new(RoomState {
                current_seq: 0,
                history: Vec::new(),
                participants: HashMap::new(),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CollaborationServerMessage> {
        self.tx.subscribe()
    }

    pub async fn join(
        &self,
        user_id: String,
        username: String,
        role: PlainTextAccessRole,
    ) -> (CollaborationPresence, u64) {
        let presence = CollaborationPresence {
            session_id: Uuid::new_v4().to_string(),
            user_id,
            username,
            role,
            selected_node_id: None,
            updated_at: Utc::now(),
        };

        let mut state = self.state.lock().await;
        let seq = state.current_seq;
        state
            .participants
            .insert(presence.session_id.clone(), presence.clone());
        drop(state);

        let _ = self.tx.send(CollaborationServerMessage::PresenceJoined {
            presence: presence.clone(),
        });

        (presence, seq)
    }

    pub async fn leave(&self, session_id: &str) {
        let mut state = self.state.lock().await;
        let removed = state.participants.remove(session_id).is_some();
        drop(state);

        if removed {
            let _ = self.tx.send(CollaborationServerMessage::PresenceLeft {
                session_id: session_id.to_string(),
            });
        }
    }

    pub async fn current_seq(&self) -> u64 {
        self.state.lock().await.current_seq
    }

    pub async fn participants(&self) -> Vec<CollaborationPresence> {
        self.state
            .lock()
            .await
            .participants
            .values()
            .cloned()
            .collect()
    }

    pub async fn update_presence(
        &self,
        session_id: &str,
        selected_node_id: Option<String>,
    ) -> Option<CollaborationPresence> {
        let mut state = self.state.lock().await;
        let presence = state.participants.get_mut(session_id)?;
        presence.selected_node_id = selected_node_id;
        presence.updated_at = Utc::now();
        let updated = presence.clone();
        drop(state);

        let _ = self.tx.send(CollaborationServerMessage::PresenceUpdated {
            presence: updated.clone(),
        });
        Some(updated)
    }

    pub async fn events_after(&self, after_seq: u64) -> (u64, Vec<CollaborationEvent>) {
        let state = self.state.lock().await;
        (
            state.current_seq,
            state
                .history
                .iter()
                .filter(|event| event.seq > after_seq)
                .cloned()
                .collect(),
        )
    }

    pub async fn apply_document<F, Fut>(
        &self,
        base_seq: u64,
        title: String,
        summary: Option<String>,
        content_json: Value,
        actor_user_id: String,
        actor_username: String,
        updated_at: DateTime<Utc>,
        map_id: String,
        persist: F,
    ) -> Result<CollaborationSnapshotPayload, crate::error::AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), crate::error::AppError>>,
    {
        let mut state = self.state.lock().await;
        if base_seq != state.current_seq {
            return Err(crate::error::AppError::Conflict(state.current_seq.to_string()));
        }

        persist().await?;

        state.current_seq += 1;
        let seq = state.current_seq;
        let event = CollaborationEvent {
            seq,
            map_id: map_id.clone(),
            title: title.clone(),
            summary: summary.clone(),
            content_json: content_json.clone(),
            actor_user_id: actor_user_id.clone(),
            actor_username: actor_username.clone(),
            created_at: updated_at,
        };
        state.history.push(event);
        if state.history.len() > 200 {
            let drop_count = state.history.len().saturating_sub(200);
            state.history.drain(0..drop_count);
        }

        let participants = state.participants.values().cloned().collect();
        drop(state);

        let snapshot = CollaborationSnapshotPayload {
            map_id,
            seq,
            title,
            summary,
            content_json,
            participants,
            updated_at,
            actor_user_id: Some(actor_user_id),
            actor_username: Some(actor_username),
        };

        let _ = self.tx.send(CollaborationServerMessage::Snapshot {
            snapshot: snapshot.clone(),
        });

        Ok(snapshot)
    }
}