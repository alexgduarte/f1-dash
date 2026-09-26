use std::{collections::HashMap, env, sync::Arc};

use anyhow::Error;
use axum::{
    Json, Router,
    http::{HeaderValue, Method, StatusCode},
    routing::get,
};
use feeds::{FeedHub, Series};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::info;

mod connections;
mod current;
mod drivers;
mod health;
mod realtime;
mod replay;
mod series;

pub type Hubs = Arc<HashMap<Series, FeedHub>>;

#[derive(Debug, Deserialize)]
pub struct SeriesQuery {
    series: Option<String>,
}

pub type ApiError = (StatusCode, Json<serde_json::Value>);

impl SeriesQuery {
    /// Resolves `?series=` (default `f1`) to an enabled hub.
    pub fn hub(&self, hubs: &Hubs) -> Result<FeedHub, ApiError> {
        let series = match self.series.as_deref() {
            None | Some("") => Series::F1,
            Some(id) => id.parse().map_err(|err: Error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": err.to_string() })),
                )
            })?,
        };

        hubs.get(&series).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(
                    json!({ "error": format!("series '{series}' is not enabled on this server") }),
                ),
            )
        })
    }
}

pub async fn start(hubs: Hubs) -> Result<(), Error> {
    let addr = env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0:80".to_string());

    let app = Router::new()
        .route("/api/health", get(health::health_check))
        .route("/api/series", get(series::list))
        .route("/api/realtime", get(realtime::sse_stream))
        .route("/api/current", get(current::current_state))
        .route("/api/drivers", get(drivers::drivers))
        .route("/api/connections", get(connections::current_connections))
        .route("/api/replay", get(replay::stream))
        .route("/api/replay/sessions", get(replay::sessions))
        .with_state(hubs)
        .layer(cors_layer())
        .into_make_service();

    info!(addr, "starting realtime http server");

    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}

pub fn cors_layer() -> CorsLayer {
    let origin = env::var("ORIGIN").unwrap_or_else(|_| "https://f1-dash.com".to_string());

    let origins = origin
        .split(';')
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect::<Vec<HeaderValue>>();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::CONNECT])
}
