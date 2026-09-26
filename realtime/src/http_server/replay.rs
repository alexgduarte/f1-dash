use std::{convert::Infallible, env};

use axum::{
    Json,
    extract::Query,
    http::StatusCode,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use chrono::Datelike;
use feeds::{
    FeedHub, Series,
    archive::{self, Archive, ArchiveMeeting},
    replay::{self, ReplayRequest},
    util::AbortOnDrop,
};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use crate::http_server::{ApiError, sse_event};

/// Replays can be turned off with `REPLAY=off`, e.g. on a busy public server
/// where every replay viewer holds a session in memory.
fn enabled() -> Result<(), ApiError> {
    match env::var("REPLAY").as_deref() {
        Ok("off" | "0" | "false") => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "replays are disabled on this server" })),
        )),
        _ => Ok(()),
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    year: Option<i32>,
}

/// Past F1 sessions that can be replayed.
pub async fn sessions(
    Query(query): Query<SessionsQuery>,
) -> Result<Json<Vec<ArchiveMeeting>>, ApiError> {
    enabled()?;

    let year = query.year.unwrap_or_else(|| chrono::Utc::now().year());

    archive::meetings(&Archive::f1(), year)
        .await
        .map(Json)
        .map_err(|err| {
            warn!(?err, year, "failed to list archived sessions");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "could not read the session archive" })),
            )
        })
}

/// Streams a replay like `/api/realtime` streams a live feed. Changing the
/// position, speed or pause state means opening a new stream.
pub async fn stream(
    Query(request): Query<ReplayRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    enabled()?;

    archive::check_session_path(&request.path).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
    })?;

    info!(
        path = request.path,
        from = request.from,
        "replay client connected"
    );

    let hub = FeedHub::new(Series::F1);
    let task = hub.spawn_with(move |sink| replay::run_logged(Archive::f1(), request, sink));
    let guard = AbortOnDrop(task);

    let stream = hub.stream().map(move |message| {
        let _replay = &guard;
        Ok(sse_event(message))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().text("keep-alive-text")))
}
