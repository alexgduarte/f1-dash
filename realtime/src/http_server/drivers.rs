use axum::{
    Json,
    extract::{Query, State},
};
use serde_json::Value;

use crate::http_server::{ApiError, Hubs, SeriesQuery};

fn map_to_vec(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Object(map)) => map.values().filter(|v| v.is_object()).cloned().collect(),
        _ => vec![],
    }
}

pub async fn drivers(
    State(hubs): State<Hubs>,
    Query(query): Query<SeriesQuery>,
) -> Result<Json<Vec<Value>>, ApiError> {
    let state = query.hub(&hubs)?.state().await;
    Ok(Json(map_to_vec(state.get("DriverList"))))
}
