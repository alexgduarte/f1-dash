use axum::{
    Json,
    extract::{Query, State},
};
use serde_json::Value;

use crate::http_server::{ApiError, Hubs, SeriesQuery};

pub async fn current_state(
    State(hubs): State<Hubs>,
    Query(query): Query<SeriesQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(query.hub(&hubs)?.state().await))
}
