use std::collections::BTreeMap;

use axum::{Json, extract::State};
use serde::Serialize;

use crate::http_server::Hubs;

#[derive(Debug, Serialize)]
pub struct ConnectionsResponse {
    connections: usize,
    series: BTreeMap<&'static str, usize>,
}

pub async fn current_connections(State(hubs): State<Hubs>) -> Json<ConnectionsResponse> {
    let series: BTreeMap<&'static str, usize> = hubs
        .iter()
        .map(|(series, hub)| (series.id(), hub.receiver_count()))
        .collect();

    Json(ConnectionsResponse {
        connections: series.values().sum(),
        series,
    })
}
