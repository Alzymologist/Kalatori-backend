use crate::{server::definitions::api_v2::ServerStatus, state::State};
use axum::{extract::State as ExtractState, http::StatusCode, Json};

pub async fn status(
    ExtractState(state): ExtractState<State>,
) -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    Json<ServerStatus>,
) {
    match state.server_status().await {
        Ok(status) => (
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(status),
        ),
        Err(_) => panic!("db connection is down, state is lost"), // You can modify this as needed
    }
}

pub async fn health(
    ExtractState(_state): ExtractState<State>,
) -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    Json<ServerStatus>,
) {
    todo!();
}

pub async fn audit(ExtractState(_state): ExtractState<State>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
