use std::convert::Infallible;

use axum::{
    extract::{Query, State},
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use futures::{Stream, StreamExt};
use tracing::info;

use crate::http_server::{ApiError, Hubs, SeriesQuery, sse_event};

/// Server-sent events: `initial` with the full state (again after a session
/// change or when the client lagged behind), then `update` with partials.
pub async fn sse_stream(
    State(hubs): State<Hubs>,
    Query(query): Query<SeriesQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let hub = query.hub(&hubs)?;

    info!(series = %hub.series(), connections = hub.receiver_count(), "sse client connected");

    let stream = hub.stream().map(|message| Ok(sse_event(message)));

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().text("keep-alive-text")))
}
