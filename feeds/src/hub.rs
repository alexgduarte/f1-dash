use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::Stream;
use serde::Serialize;
use serde_json::Value;
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::JoinHandle,
    time::Instant,
};
use tracing::{info, warn};

use crate::{Series, adapters, state::StateService};

const CHANNEL_CAPACITY: usize = 256;

const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// A message delivered to feed subscribers. Payloads are pre-serialised JSON so
/// a single update is encoded once no matter how many clients listen.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "lowercase")]
pub enum HubMessage {
    /// The complete state. Sent first, again after a session change, and again
    /// to a subscriber that fell too far behind to catch up with updates.
    Initial(Arc<str>),
    /// A partial update in the shape `{ "<Topic>": <partial data> }`.
    Update(Arc<str>),
    /// Whether the adapter is connected to the upstream feed. Sent after every
    /// initial state and whenever it changes.
    Status(bool),
}

/// Where adapters write what they receive.
#[derive(Clone)]
pub struct Sink {
    state: StateService,
    tx: broadcast::Sender<HubMessage>,
    connected: Arc<AtomicBool>,
}

impl Sink {
    /// Replaces the whole state, e.g. after (re)connecting to a feed, which
    /// also marks the feed as connected.
    pub async fn reset(&self, initial: Value) {
        self.state.set_state(initial).await;
        let serialized: Arc<str> = self.state.get_state_string().await.into();
        let _ = self.tx.send(HubMessage::Initial(serialized));
        self.set_connected(true);
    }

    fn set_connected(&self, connected: bool) {
        if self.connected.swap(connected, Ordering::SeqCst) != connected {
            let _ = self.tx.send(HubMessage::Status(connected));
        }
    }

    /// Applies and broadcasts a partial update shaped `{ "<Topic>": data }`.
    ///
    /// The state is merged before broadcasting, so a subscriber that snapshots
    /// in between sees the update at worst twice, and merges are idempotent.
    pub async fn update(&self, update: Value) {
        let serialized: Arc<str> = update.to_string().into();
        self.state.update_state(update).await;
        let _ = self.tx.send(HubMessage::Update(serialized));
    }

    pub async fn topic(&self, topic: &str) -> Option<Value> {
        self.state.get_topic(topic).await
    }
}

/// Owns the state and fan-out channel of one series' live feed.
#[derive(Clone)]
pub struct FeedHub {
    series: Series,
    sink: Sink,
}

impl FeedHub {
    pub fn new(series: Series) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);

        Self {
            series,
            sink: Sink {
                state: StateService::new(),
                tx,
                connected: Arc::new(AtomicBool::new(false)),
            },
        }
    }

    pub fn series(&self) -> Series {
        self.series
    }

    pub fn receiver_count(&self) -> usize {
        self.sink.tx.receiver_count()
    }

    pub async fn state(&self) -> Value {
        self.sink.state.get_state().await
    }

    /// Whether the adapter currently has a working upstream connection.
    pub fn connected(&self) -> bool {
        self.sink.connected.load(Ordering::SeqCst)
    }

    /// Subscribes first and snapshots second, so no update can fall between
    /// the snapshot and the first received message.
    pub async fn snapshot(&self) -> (Arc<str>, broadcast::Receiver<HubMessage>) {
        let rx = self.sink.tx.subscribe();
        let state: Arc<str> = self.sink.state.get_state_string().await.into();
        (state, rx)
    }

    /// A subscriber stream: the current state and connection status, then
    /// live updates. A subscriber that lags behind the channel gets a fresh
    /// snapshot instead of silently missing updates.
    pub fn stream(&self) -> impl Stream<Item = HubMessage> + Send + use<> {
        enum Phase {
            Start,
            Status(broadcast::Receiver<HubMessage>),
            Live(broadcast::Receiver<HubMessage>),
        }

        futures::stream::unfold((self.clone(), Phase::Start), |(hub, phase)| async move {
            match phase {
                Phase::Start => {
                    let (initial, rx) = hub.snapshot().await;

                    // nothing published yet: the adapter's first reset
                    // arrives as the initial state instead, so clients never
                    // mistake an empty snapshot for a new session
                    if &*initial == "{}" {
                        let status = HubMessage::Status(hub.connected());
                        return Some((status, (hub, Phase::Live(rx))));
                    }

                    Some((HubMessage::Initial(initial), (hub, Phase::Status(rx))))
                }
                Phase::Status(rx) => {
                    let status = HubMessage::Status(hub.connected());
                    Some((status, (hub, Phase::Live(rx))))
                }
                Phase::Live(mut rx) => match rx.recv().await {
                    Ok(msg) => Some((msg, (hub, Phase::Live(rx)))),
                    Err(RecvError::Lagged(skipped)) => {
                        warn!(series = %hub.series, skipped, "subscriber lagged, resending snapshot");
                        let (initial, rx) = hub.snapshot().await;
                        Some((HubMessage::Initial(initial), (hub, Phase::Status(rx))))
                    }
                    Err(RecvError::Closed) => None,
                },
            }
        })
    }

    /// Runs something other than the series adapter into this hub, such as a
    /// replay. The hub counts as connected while it has published a state.
    pub fn spawn_with<F, Fut>(&self, task: F) -> JoinHandle<()>
    where
        F: FnOnce(Sink) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let sink = self.sink.clone();
        let run = task(sink.clone());

        tokio::spawn(async move {
            run.await;
            sink.set_connected(false);
        })
    }

    /// Runs the series adapter forever, reconnecting with exponential backoff.
    /// Adapters return when the session changes, which triggers a fresh
    /// subscription and a new initial state.
    pub fn spawn(&self) -> JoinHandle<()> {
        let series = self.series;
        let sink = self.sink.clone();

        tokio::spawn(async move {
            let mut backoff = MIN_BACKOFF;

            loop {
                let started = Instant::now();

                match adapters::run(series, &sink).await {
                    Ok(()) => info!(%series, "feed ended, possible session change, reconnecting"),
                    Err(err) => warn!(%series, ?err, "feed failed"),
                }

                sink.set_connected(false);

                if started.elapsed() > MAX_BACKOFF {
                    backoff = MIN_BACKOFF;
                }

                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn stream_starts_with_snapshot_then_updates() {
        let hub = FeedHub::new(Series::F1);
        hub.sink.reset(json!({"LapCount": {"CurrentLap": 1}})).await;

        let mut stream = Box::pin(hub.stream());

        let HubMessage::Initial(initial) = stream.next().await.unwrap() else {
            panic!("expected initial");
        };
        assert_eq!(&*initial, r#"{"LapCount":{"CurrentLap":1}}"#);
        assert!(
            matches!(stream.next().await, Some(HubMessage::Status(true))),
            "a reset means the feed is connected"
        );

        hub.sink
            .update(json!({"LapCount": {"CurrentLap": 2}}))
            .await;

        let HubMessage::Update(update) = stream.next().await.unwrap() else {
            panic!("expected update");
        };
        assert_eq!(&*update, r#"{"LapCount":{"CurrentLap":2}}"#);
        assert_eq!(hub.state().await, json!({"LapCount": {"CurrentLap": 2}}));
    }

    #[tokio::test]
    async fn lagging_subscriber_gets_fresh_snapshot() {
        let hub = FeedHub::new(Series::F1);
        let mut stream = Box::pin(hub.stream());
        // nothing published yet, so no initial state
        assert!(matches!(
            stream.next().await,
            Some(HubMessage::Status(false))
        ));

        for lap in 0..(CHANNEL_CAPACITY as u64 + 10) {
            hub.sink
                .update(json!({"LapCount": {"CurrentLap": lap}}))
                .await;
        }

        let HubMessage::Initial(snapshot) = stream.next().await.unwrap() else {
            panic!("expected snapshot after lag");
        };
        assert!(matches!(
            stream.next().await,
            Some(HubMessage::Status(false))
        ));
        let last = CHANNEL_CAPACITY as u64 + 9;
        assert_eq!(
            serde_json::from_str::<Value>(&snapshot).unwrap(),
            json!({"LapCount": {"CurrentLap": last}})
        );
    }
}
