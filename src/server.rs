use crate::{
    error::{Error, ServerError},
    state::State,
};
use axum::{
    extract::{self, rejection::RawPathParamsRejection, MatchedPath, Query, RawPathParams},
    response::Response,
    routing, Router,
};
use axum_macros::debug_handler;
use handlers::{
    health::{audit, health, status},
    order::{force_withdrawal, investigate, order},
};
use std::{borrow::Cow, collections::HashMap, future::Future, net::SocketAddr};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

mod handlers;

pub mod definitions;

pub const MODULE: &str = module_path!();

pub async fn new(
    shutdown_notification: CancellationToken,
    host: SocketAddr,
    state: State,
) -> Result<impl Future<Output = Result<Cow<'static, str>, Error>>, ServerError> {
    let v2: Router<State> = Router::new()
        .route("/order/:order_id", routing::post(order))
        .route(
            "/order/:order_id/forceWithdrawal",
            routing::post(force_withdrawal),
        )
        .route("/status", routing::get(status))
        .route("/health", routing::get(health))
        .route("/audit", routing::get(audit))
        .route("/order/:order_id/investigate", routing::post(investigate));
    let app = Router::new()
        .route(
            "/public/v2/payment/:paymentAccount",
            routing::post(public_payment_account),
        )
        .nest("/v2", v2)
        .with_state(state);

    let listener = TcpListener::bind(host)
        .await
        .map_err(|_| ServerError::TcpListenerBind(host))?;

    Ok(async move {
        tracing::info!("The server is listening on {host}.");

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_notification.cancelled_owned())
            .await
            .map_err(|_| ServerError::ThreadError)?;

        Ok("The server module is shut down.".into())
    })
}

// TODO: Clarify what this is doing
#[debug_handler]
async fn public_payment_account(
    extract::State(state): extract::State<State>,
    matched_path: MatchedPath,
    path_result: Result<RawPathParams, RawPathParamsRejection>,
    query: Query<HashMap<String, String>>,
) -> Response {
    todo!()
}
