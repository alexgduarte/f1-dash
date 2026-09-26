//! The Formula One Management live timing feed (SignalR Core).

use anyhow::Error;
use serde_json::{Map, Value, json};
use tokio_stream::StreamExt;
use tracing::{debug, info, trace};

use crate::{Sink, archive::Archive, tyres};

pub struct LiveTimingSource {
    /// Host and path of the SignalR Core endpoint, without scheme.
    pub host: &'static str,
    pub hub: &'static str,
    pub topics: &'static [&'static str],
    /// Whether past sessions are in the F1 archive, used to reconstruct the
    /// tyre sets a driver has left over the weekend.
    pub archive: bool,
}

pub const F1: LiveTimingSource = LiveTimingSource {
    host: "livetiming.formula1.com/signalrcore",
    hub: "Streaming",
    topics: &[
        "Heartbeat",
        "CarData.z",
        "Position.z",
        "ExtrapolatedClock",
        "TopThree",
        "TimingStats",
        "TimingAppData",
        "WeatherData",
        "TrackStatus",
        "SessionStatus",
        "DriverList",
        "RaceControlMessages",
        "SessionInfo",
        "SessionData",
        "LapCount",
        "TimingData",
        "TeamRadio",
        "ChampionshipPrediction",
    ],
    archive: true,
};

/// Topic names that are not valid JS identifiers are renamed so the front end
/// can destructure them (`CarData.z` -> `CarDataZ`).
pub fn normalize_topic(topic: &str) -> &str {
    match topic {
        "CarData.z" => "CarDataZ",
        "Position.z" => "PositionZ",
        other => other,
    }
}

fn normalize_initial(initial: Value) -> Value {
    match initial {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(topic, data)| (normalize_topic(&topic).to_owned(), data))
                .collect::<Map<String, Value>>(),
        ),
        other => other,
    }
}

/// A `SessionInfo` update that carries a session name means a new session
/// started on the same connection; resubscribing yields a clean state.
fn is_session_change(topic: &str, data: &Value) -> bool {
    topic == "SessionInfo" && data.pointer("/Name").is_some()
}

pub async fn run(source: &LiveTimingSource, sink: &Sink) -> Result<(), Error> {
    let mut client = signalr::create_client(source.host, source.hub).await?;

    let initial = signalr::subscribe(&mut client, source.topics).await?;
    sink.reset(normalize_initial(initial)).await;

    let session_info = sink.topic("SessionInfo").await;
    let mut tyre_sets = tyres::Tracker::start(source.archive.then(Archive::f1), session_info);

    if let Some(update) = tyre_sets.recompute(sink.topic("TimingAppData").await.as_ref()) {
        sink.update(update).await;
    }

    let mut stream = std::pin::pin!(signalr::listen(client));

    loop {
        tokio::select! {
            items = stream.next() => {
                let Some(items) = items else {
                    debug!("signalr stream ended");
                    return Ok(());
                };

                let mut timing_app_data_changed = false;

                for update in items {
                    trace!(topic = update.topic, "received update");

                    if is_session_change(&update.topic, &update.data) {
                        info!("session changed, resubscribing");
                        return Ok(());
                    }

                    let topic = normalize_topic(&update.topic);
                    timing_app_data_changed |= topic == "TimingAppData";

                    sink.update(json!({ topic: update.data })).await;
                }

                if timing_app_data_changed
                    && let Some(update) = tyre_sets.recompute(sink.topic("TimingAppData").await.as_ref())
                {
                    sink.update(update).await;
                }
            }

            loaded = tyre_sets.history_loaded() => {
                if loaded
                    && let Some(update) = tyre_sets.recompute(sink.topic("TimingAppData").await.as_ref())
                {
                    sink.update(update).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_compressed_topics() {
        let initial = normalize_initial(json!({
            "CarData.z": "abc",
            "Position.z": "def",
            "LapCount": {"CurrentLap": 1}
        }));

        assert_eq!(
            initial,
            json!({"CarDataZ": "abc", "PositionZ": "def", "LapCount": {"CurrentLap": 1}})
        );
    }

    #[test]
    fn detects_session_change_only_with_name() {
        assert!(is_session_change("SessionInfo", &json!({"Name": "Race"})));
        assert!(!is_session_change(
            "SessionInfo",
            &json!({"ArchiveStatus": {"Status": "Complete"}})
        ));
        assert!(!is_session_change("TimingData", &json!({"Name": "x"})));
    }
}
