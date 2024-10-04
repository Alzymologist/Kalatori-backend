use crate::{
    error::{ForceWithdrawalError, OrderError},
    server::definitions::api_v2::{
        InvalidParameter, OrderQuery, OrderResponse, OrderStatus, AMOUNT, CURRENCY,
    },
    state::State,
};
use axum::{
    extract::{Path, State as ExtractState},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OrderPayload {
    pub amount: f64,
    pub currency: String,
    pub callback: Option<String>,
}

pub async fn process_order(
    state: State,
    order_id: String,
    payload: OrderPayload,
) -> Result<OrderResponse, OrderError> {
    if payload.amount < 0.07 {
        return Err(OrderError::ExistentialDeposit(0.07));
    }

    state
        .create_order(OrderQuery {
            order: order_id,
            amount: payload.amount,
            callback: payload.callback.unwrap_or_default(),
            currency: payload.currency,
        })
        .await
        .map_err(|_| OrderError::InternalError)
}

pub async fn order(
    ExtractState(state): ExtractState<State>,
    Path(order_id): Path<String>,
    Json(payload): Json<OrderPayload>,
) -> Response {
    match process_order(state, order_id, payload).await {
        Ok(order) => match order {
            OrderResponse::NewOrder(order_status) => (StatusCode::CREATED, Json(order_status)).into_response(),
            OrderResponse::FoundOrder(order_status) => (StatusCode::OK, Json(order_status)).into_response(),
            OrderResponse::ModifiedOrder(order_status) => (StatusCode::OK, Json(order_status)).into_response(),
            OrderResponse::CollidedOrder(order_status) => (StatusCode::CONFLICT, Json(order_status)).into_response(),
            OrderResponse::NotFound => (StatusCode::NOT_FOUND, "").into_response(),
        },
        Err(error) => match error {
            OrderError::ExistentialDeposit(existential_deposit) => (
                StatusCode::BAD_REQUEST,
                Json([InvalidParameter {
                    parameter: AMOUNT.into(),
                    message: format!("provided amount is less than the currency's existential deposit ({existential_deposit})"),
                }]),
            )
                .into_response(),
            OrderError::UnknownCurrency => (
                StatusCode::BAD_REQUEST,
                Json([InvalidParameter {
                    parameter: CURRENCY.into(),
                    message: "provided currency isn't supported".into(),
                }]),
            )
                .into_response(),
            _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
    }
}

pub async fn process_force_withdrawal(
    state: State,
    order_id: String,
) -> Result<OrderStatus, ForceWithdrawalError> {
    state
        .force_withdrawal(order_id)
        .await
        .map_err(|e| ForceWithdrawalError::WithdrawalError(e.into()))
}

pub async fn force_withdrawal(
    ExtractState(state): ExtractState<State>,
    Path(order_id): Path<String>,
) -> Response {
    match process_force_withdrawal(state, order_id).await {
        Ok(a) => (StatusCode::CREATED, Json(a)).into_response(),
        Err(ForceWithdrawalError::WithdrawalError(a)) => {
            (StatusCode::BAD_REQUEST, Json(a)).into_response()
        }
        Err(ForceWithdrawalError::MissingParameter(parameter)) => (
            StatusCode::BAD_REQUEST,
            Json([InvalidParameter {
                parameter,
                message: "parameter wasn't found".into(),
            }]),
        )
            .into_response(),
        Err(ForceWithdrawalError::InvalidParameter(parameter)) => (
            StatusCode::BAD_REQUEST,
            Json([InvalidParameter {
                parameter,
                message: "parameter's format is invalid".into(),
            }]),
        )
            .into_response(),
    }
}

pub async fn investigate(
    ExtractState(_state): ExtractState<State>,
    Path(_order_id): Path<String>,
) -> Response {
    // Investigation logic will be implemented here as needed
    StatusCode::NOT_IMPLEMENTED.into_response()
}
