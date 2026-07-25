use axum::{
    Router,
    routing::{get, post},
};

use crate::{handlers, state::AppState};

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/api/auth/register", post(handlers::register))
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/me", get(handlers::me))
        .with_state(state)
}
