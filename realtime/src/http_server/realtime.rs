use std::convert::Infallible;

use axum::{
    extract::{Query, State},
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use feeds::HubMessage;
use futures::{Stream, StreamExt};
use tracing::info;

use crate::http_server::{ApiError, Hubs, SeriesQuery};

/// Server-sent events: `initial` with the full state (again after a session
/// change or when the client lagged behind), then `update` with partials.
pub async fn sse_stream(
    State(hubs): State<Hubs>,
    Query(query): Query<SeriesQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let hub = query.hub(&hubs)?;

    info!(series = %hub.series(), connections = hub.receiver_count(), "sse client connected");

    let stream = hub.stream().map(|message| {
        Ok(match message {
            HubMessage::Initial(state) => Event::default().event("initial").data(&*state),
            HubMessage::Update(update) => Event::default().event("update").data(&*update),
            HubMessage::Status(connected) => {
                Event::default().event("status").data(connected.to_string())
            }
        })
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().text("keep-alive-text")))
}
