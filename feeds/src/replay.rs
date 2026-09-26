//! Replays past F1 sessions from the live timing archive.
//!
//! Every topic's `.jsonStream` is loaded into one timeline sorted by time.
//! Seeking folds the timeline up to the requested moment into a state, then
//! playback feeds the following updates at the requested speed, so replays
//! go through the same hub, stream and dashboard as live sessions.
//!
//! A replay is stateless on purpose: pausing, seeking or changing the speed
//! starts a new replay at the new position (timelines are cached), which
//! works the same over server-sent events and native IPC.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use anyhow::{Context, Error, bail};
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::{
    Sink,
    adapters::util::now_utc,
    archive::{self, Archive},
    state::merge,
    tyres,
};

/// Topics replayed. The car telemetry and position topics are left out: they
/// are large and, since 2025, often missing from the archive.
pub const TOPICS: [&str; 15] = [
    "SessionInfo",
    "DriverList",
    "TimingData",
    "TimingAppData",
    "TimingStats",
    "TopThree",
    "LapCount",
    "TrackStatus",
    "SessionStatus",
    "SessionData",
    "ExtrapolatedClock",
    "WeatherData",
    "RaceControlMessages",
    "TeamRadio",
    "ChampionshipPrediction",
];

/// Replays start this long before the session goes green.
const PRE_ROLL_MS: u64 = 60_000;
const TICK: Duration = Duration::from_millis(100);
const STATUS_EVERY: Duration = Duration::from_secs(1);
const CACHED_TIMELINES: usize = 2;

struct Message {
    at: u64,
    topic: &'static str,
    data: Value,
}

/// A session's updates, oldest first.
pub struct Timeline {
    path: String,
    messages: Vec<Message>,
    /// The clock after each clock update, for cheap lookups during playback.
    clocks: Vec<(u64, Value)>,
    /// When the session went green (or the stream start, if it never did).
    pub start: u64,
    pub end: u64,
}

impl Timeline {
    /// Builds a timeline from `(topic, stream text)` pairs.
    pub fn from_streams(path: &str, streams: Vec<(&'static str, String)>) -> Result<Self, Error> {
        let mut messages: Vec<Message> = streams
            .into_iter()
            .flat_map(|(topic, text)| {
                archive::parse_stream(&text)
                    .into_iter()
                    .map(move |(at, data)| Message { at, topic, data })
            })
            .collect();

        if !messages.iter().any(|m| m.topic == "TimingData") {
            bail!("no timing data for {path}");
        }

        // stable, so updates with the same timestamp keep their topic order
        messages.sort_by_key(|m| m.at);

        let start = messages
            .iter()
            .find(|m| {
                m.topic == "SessionStatus"
                    && m.data.get("Status").and_then(Value::as_str) == Some("Started")
            })
            .map_or(0, |m| m.at);
        let end = messages.last().map_or(0, |m| m.at);

        let mut clock = Value::Null;
        let clocks = messages
            .iter()
            .filter(|m| m.topic == "ExtrapolatedClock")
            .map(|m| {
                merge(&mut clock, m.data.clone());
                (m.at, clock.clone())
            })
            .collect();

        Ok(Timeline {
            path: path.to_owned(),
            messages,
            clocks,
            start,
            end,
        })
    }

    pub async fn load(archive: &Archive, path: &str) -> Result<Self, Error> {
        archive::check_session_path(path)?;

        let loads = TOPICS.map(|topic| async move {
            let file = format!("{path}{topic}.jsonStream");
            match archive.text(&file).await {
                Ok(text) => Some((topic, text)),
                Err(err) => {
                    debug!(?err, file, "topic not in archive");
                    None
                }
            }
        });

        let streams = futures::future::join_all(loads)
            .await
            .into_iter()
            .flatten()
            .collect();

        Timeline::from_streams(path, streams).with_context(|| format!("loading replay of {path}"))
    }

    /// The first update after `at`.
    fn index_after(&self, at: u64) -> usize {
        self.messages.partition_point(|m| m.at <= at)
    }

    /// Every topic as it stood at `at`.
    pub fn state_at(&self, at: u64) -> Value {
        let mut state = Value::Object(Map::new());
        for message in &self.messages[..self.index_after(at)] {
            merge(&mut state, json!({ message.topic: message.data }));
        }
        state
    }

    /// The session clock at `at`, counted down from the last clock update
    /// when it was running. Sent as not extrapolating, so the dashboard shows
    /// it as is whatever the playback speed.
    fn clock_at(&self, at: u64) -> Option<Value> {
        let index = self
            .clocks
            .partition_point(|(t, _)| *t <= at)
            .checked_sub(1)?;
        let (since, clock) = &self.clocks[index];

        let remaining = archive::parse_offset(clock.get("Remaining")?.as_str()?)?;
        let running = clock
            .get("Extrapolating")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let elapsed = if running {
            at.saturating_sub(*since)
        } else {
            0
        };

        Some(json!({
            "Utc": now_utc(),
            "Remaining": format_hms(remaining.saturating_sub(elapsed)),
            "Extrapolating": false,
        }))
    }
}

fn format_hms(millis: u64) -> String {
    let seconds = millis / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

type Cache = Mutex<Vec<Arc<Timeline>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

type Load = Shared<BoxFuture<'static, Result<Arc<Timeline>, String>>>;

/// Downloads in progress, shared by every replay of the same session.
fn loads() -> &'static Mutex<HashMap<String, Load>> {
    static LOADS: OnceLock<Mutex<HashMap<String, Load>>> = OnceLock::new();
    LOADS.get_or_init(Default::default)
}

/// Loads a session's timeline, keeping the last few in memory so seeking or
/// changing speed does not download the session again. The download runs as
/// its own task shared by everyone waiting for it, so a replay restarted
/// while loading (a pause, a speed change) picks up the same download.
pub async fn timeline(archive: &Archive, path: &str) -> Result<Arc<Timeline>, Error> {
    if let Some(timeline) = cache()
        .lock()
        .ok()
        .and_then(|c| c.iter().find(|t| t.path == path).cloned())
    {
        return Ok(timeline);
    }

    let load = {
        let mut loads = loads()
            .lock()
            .map_err(|_| anyhow::anyhow!("replay loads poisoned"))?;

        loads
            .entry(path.to_owned())
            .or_insert_with(|| {
                info!(path, "loading replay");
                let archive = archive.clone();
                let owned = path.to_owned();
                let task = tokio::spawn(async move {
                    Timeline::load(&archive, &owned)
                        .await
                        .map(Arc::new)
                        .map_err(|err| format!("{err:#}"))
                });
                async move { task.await.map_err(|err| err.to_string())? }
                    .boxed()
                    .shared()
            })
            .clone()
    };

    let result = load.await;

    if let Ok(mut loads) = loads().lock() {
        loads.remove(path);
    }

    let timeline = result.map_err(anyhow::Error::msg)?;

    if let Ok(mut cache) = cache().lock()
        && !cache.iter().any(|t| t.path == path)
    {
        cache.insert(0, timeline.clone());
        cache.truncate(CACHED_TIMELINES);
    }

    Ok(timeline)
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRequest {
    /// Session path in the archive.
    pub path: String,
    /// Stream time to start at in milliseconds; defaults to just before the
    /// session starts.
    pub from: Option<u64>,
    pub speed: Option<f64>,
    pub paused: Option<bool>,
}

/// Where the replay is, published as the `Replay` topic.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ReplayStatus {
    pub path: String,
    pub position: u64,
    pub start: u64,
    pub end: u64,
    pub speed: f64,
    pub paused: bool,
    pub ended: bool,
}

pub fn clamp_speed(speed: Option<f64>) -> f64 {
    speed
        .filter(|s| s.is_finite())
        .unwrap_or(1.0)
        .clamp(0.25, 64.0)
}

/// Plays a session into `sink` until the task is dropped.
pub async fn run(archive: Archive, request: ReplayRequest, sink: Sink) -> Result<(), Error> {
    let timeline = timeline(&archive, &request.path).await?;

    let speed = clamp_speed(request.speed);
    let paused = request.paused.unwrap_or(false);
    let from = request
        .from
        .unwrap_or_else(|| timeline.start.saturating_sub(PRE_ROLL_MS))
        .min(timeline.end);

    let status = |position: u64| ReplayStatus {
        path: timeline.path.clone(),
        position,
        start: timeline.start,
        end: timeline.end,
        speed,
        paused,
        ended: position >= timeline.end,
    };

    let mut state = timeline.state_at(from);
    if let Some(clock) = timeline.clock_at(from) {
        state["ExtrapolatedClock"] = clock;
    }
    state["Replay"] = serde_json::to_value(status(from))?;
    sink.reset(state).await;

    let mut tyre_sets =
        tyres::Tracker::start(Some(archive.clone()), sink.topic("SessionInfo").await);

    let started = Instant::now();
    let mut next = timeline.index_after(from);
    let mut last_clock = None;
    let mut last_status = Instant::now();
    // once at the end the final state stays up, and the loop keeps running so
    // a tyre history that loads late is still published
    let mut finished = from >= timeline.end;

    loop {
        tokio::select! {
            loaded = tyre_sets.history_loaded() => {
                if loaded && let Some(update) = tyre_sets.recompute(sink.topic("TimingAppData").await.as_ref()) {
                    sink.update(update).await;
                }
                continue;
            }
            _ = tokio::time::sleep(TICK) => {}
        }

        if paused || finished {
            continue;
        }

        let position =
            (from + (started.elapsed().as_millis() as f64 * speed) as u64).min(timeline.end);
        let mut timing_app_data_changed = false;

        while let Some(message) = timeline.messages.get(next).filter(|m| m.at <= position) {
            next += 1;

            // replaced by the synthetic clock below
            if message.topic == "ExtrapolatedClock" {
                continue;
            }

            timing_app_data_changed |= message.topic == "TimingAppData";
            sink.update(json!({ message.topic: message.data })).await;
        }

        if timing_app_data_changed
            && let Some(update) = tyre_sets.recompute(sink.topic("TimingAppData").await.as_ref())
        {
            sink.update(update).await;
        }

        if let Some(clock) = timeline.clock_at(position) {
            let remaining = clock["Remaining"].clone();
            if last_clock.as_ref() != Some(&remaining) {
                last_clock = Some(remaining);
                sink.update(json!({ "ExtrapolatedClock": clock })).await;
            }
        }

        let ended = position >= timeline.end;

        if last_status.elapsed() >= STATUS_EVERY || ended {
            last_status = Instant::now();
            sink.update(json!({ "Replay": status(position) })).await;
        }

        if ended {
            info!(path = timeline.path, "replay finished");
            finished = true;
        }
    }
}

/// Runs a replay, retrying with backoff when the session cannot be loaded
/// (archive unreachable, timeout), until the task is dropped.
pub async fn run_logged(archive: Archive, request: ReplayRequest, sink: Sink) {
    let mut backoff = Duration::from_secs(2);

    loop {
        match run(archive.clone(), request.clone(), sink.clone()).await {
            Ok(()) => return,
            Err(err) => warn!(?err, path = request.path, "replay failed, retrying"),
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline() -> Timeline {
        Timeline::from_streams(
            "2025/x/y/",
            vec![
                (
                    "SessionStatus",
                    "00:00:00.000{\"Status\":\"Inactive\"}\n00:01:00.000{\"Status\":\"Started\"}".into(),
                ),
                (
                    "TimingData",
                    "00:00:00.000{\"Lines\":{\"1\":{\"Position\":\"1\"}}}\n00:02:00.000{\"Lines\":{\"1\":{\"NumberOfLaps\":1}}}".into(),
                ),
                (
                    "ExtrapolatedClock",
                    "00:00:00.000{\"Remaining\":\"01:00:00\",\"Extrapolating\":false}\n00:01:00.000{\"Extrapolating\":true}".into(),
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn finds_session_start_and_end() {
        let timeline = timeline();
        assert_eq!(timeline.start, 60_000);
        assert_eq!(timeline.end, 120_000);
    }

    #[test]
    fn folds_state_up_to_a_moment() {
        let timeline = timeline();

        assert_eq!(
            timeline.state_at(59_000)["SessionStatus"]["Status"],
            "Inactive"
        );
        assert!(
            timeline.state_at(59_000)["TimingData"]["Lines"]["1"]
                .get("NumberOfLaps")
                .is_none()
        );

        let later = timeline.state_at(120_000);
        assert_eq!(later["SessionStatus"]["Status"], "Started");
        assert_eq!(later["TimingData"]["Lines"]["1"]["NumberOfLaps"], 1);
        assert_eq!(later["TimingData"]["Lines"]["1"]["Position"], "1");
    }

    #[test]
    fn counts_the_clock_down_while_running() {
        let timeline = timeline();

        assert_eq!(timeline.clock_at(30_000).unwrap()["Remaining"], "01:00:00");
        // running since 1:00, so a minute later a minute is gone
        assert_eq!(timeline.clock_at(120_000).unwrap()["Remaining"], "00:59:00");
        assert_eq!(timeline.clock_at(120_000).unwrap()["Extrapolating"], false);
    }

    #[test]
    fn needs_timing_data() {
        assert!(
            Timeline::from_streams("2025/x/y/", vec![("LapCount", "00:00:00.000{}".into())])
                .is_err()
        );
    }

    #[test]
    fn clamps_speed() {
        assert_eq!(clamp_speed(None), 1.0);
        assert_eq!(clamp_speed(Some(1000.0)), 64.0);
        assert_eq!(clamp_speed(Some(0.0)), 0.25);
        assert_eq!(clamp_speed(Some(f64::NAN)), 1.0);
    }

    #[tokio::test]
    async fn replays_into_a_hub() {
        use crate::{FeedHub, HubMessage, Series};
        use futures::StreamExt;

        let dir = std::env::temp_dir().join(format!("f1dash-replay-{}", std::process::id()));
        let session = dir.join("2025/2025-01-01_Test/2025-01-01_Race");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(
            session.join("TimingData.jsonStream"),
            "\u{feff}00:00:00.000{\"Lines\":{\"1\":{\"Position\":\"1\"}}}\r\n00:00:00.300{\"Lines\":{\"1\":{\"NumberOfLaps\":2}}}\r\n",
        )
        .unwrap();
        std::fs::write(
            session.join("SessionInfo.jsonStream"),
            "00:00:00.000{\"Name\":\"Race\"}\r\n",
        )
        .unwrap();

        let hub = FeedHub::new(Series::F1);
        let mut stream = Box::pin(hub.stream());
        // nothing published yet, so no initial state
        assert!(matches!(
            stream.next().await,
            Some(HubMessage::Status(false))
        ));

        let request = ReplayRequest {
            path: "2025/2025-01-01_Test/2025-01-01_Race/".into(),
            from: Some(0),
            speed: Some(4.0),
            paused: None,
        };
        let task = hub.spawn_with(move |sink| run_logged(Archive::Dir(dir.clone()), request, sink));

        let HubMessage::Initial(initial) = stream.next().await.unwrap() else {
            panic!("expected the state at the start");
        };
        let initial: Value = serde_json::from_str(&initial).unwrap();
        assert_eq!(initial["TimingData"]["Lines"]["1"]["Position"], "1");
        assert!(
            initial["TimingData"]["Lines"]["1"]
                .get("NumberOfLaps")
                .is_none()
        );
        assert_eq!(initial["Replay"]["Position"], 0);

        let laps = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(message) = stream.next().await {
                if let HubMessage::Update(update) = message
                    && update.contains("NumberOfLaps")
                {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap();

        assert!(laps, "the later update is played back");
        task.abort();
    }
}
