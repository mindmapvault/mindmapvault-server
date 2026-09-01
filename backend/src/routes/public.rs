use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::models::instance_settings::InstanceSettingsHandle;

#[derive(Clone)]
pub struct PublicState {
    pub settings: InstanceSettingsHandle,
}

pub fn router(state: PublicState) -> Router {
    Router::new()
        .route("/instance", get(get_instance))
        .with_state(state)
}

/// What an unauthenticated client may know about this instance.
///
/// Deliberately just what the sign-up page has to render correctly. Storage
/// caps and throttle settings are operational detail that would only help
/// someone probing the server, and the app learns its own limits from
/// `/api/auth/capabilities` once signed in. How many invites exist is not said
/// either — only that a code is what gets you in.
#[derive(Serialize)]
struct InstanceInfoResponse {
    registration_enabled: bool,
    /// True when sign-ups are closed, so the page offers a code field instead
    /// of a dead end.
    invite_required: bool,
}

async fn get_instance(State(state): State<PublicState>) -> Json<InstanceInfoResponse> {
    let registration_enabled = state.settings.get().registration_enabled;
    Json(InstanceInfoResponse {
        registration_enabled,
        invite_required: !registration_enabled,
    })
}
