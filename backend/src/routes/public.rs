use axum::Router;

#[derive(Clone)]
pub struct PublicState {}

pub fn router(state: PublicState) -> Router {
    Router::new().with_state(state)
}
