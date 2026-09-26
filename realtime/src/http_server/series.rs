use axum::{Json, extract::State};
use feeds::Series;
use serde::Serialize;

use crate::http_server::Hubs;

#[derive(Debug, Serialize)]
pub struct SeriesInfo {
    id: Series,
    name: &'static str,
    enabled: bool,
}

/// Lists every known series and whether this server ingests it.
pub async fn list(State(hubs): State<Hubs>) -> Json<Vec<SeriesInfo>> {
    Json(
        Series::ALL
            .into_iter()
            .map(|series| SeriesInfo {
                id: series,
                name: series.name(),
                enabled: hubs.contains_key(&series),
            })
            .collect(),
    )
}
