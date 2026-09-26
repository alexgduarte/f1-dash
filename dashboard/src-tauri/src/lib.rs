//! The native app: the dashboard in a webview, with the feed adapters running
//! in-process. Nothing goes through an f1-dash server; the app connects to the
//! timing feeds directly and streams them to the webview over IPC channels.

use std::{
    collections::HashMap,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicU32, Ordering},
    },
};

use chrono::Datelike;
use feeds::{
    FeedHub, HubMessage, Series,
    archive::{self, Archive, ArchiveMeeting},
    replay::{self, ReplayRequest},
    schedule::{self, Round},
};
use futures::StreamExt;
use tauri::{AppHandle, Manager, State, ipc::Channel};
use tokio::task::JoinHandle;
use tracing::info;

struct RunningFeed {
    hub: FeedHub,
    task: JoinHandle<()>,
    subscribers: usize,
}

/// A webview subscription: the task forwarding messages, and for replays the
/// replay itself.
struct Subscription {
    series: Option<Series>,
    tasks: Vec<JoinHandle<()>>,
}

/// Feeds run only while something subscribes to them, which keeps a phone
/// from holding connections open for series nobody is looking at.
#[derive(Default)]
struct Feeds {
    running: Mutex<HashMap<Series, RunningFeed>>,
    subscriptions: Mutex<HashMap<u32, Subscription>>,
    next_id: AtomicU32,
}

/// Forwards a hub to the webview. When the webview goes away (reload, closed
/// window) the subscription is dropped as if it had unsubscribed.
fn forward(app: AppHandle, id: u32, hub: FeedHub, channel: Channel<HubMessage>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream = std::pin::pin!(hub.stream());

        while let Some(message) = stream.next().await {
            if channel.send(message).is_err() {
                break;
            }
        }

        app.state::<Feeds>().unsubscribe(id);
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Feeds {
    fn unsubscribe(&self, id: u32) {
        let subscription = lock(&self.subscriptions).remove(&id);

        if let Some(subscription) = subscription {
            subscription.tasks.iter().for_each(JoinHandle::abort);
            if let Some(series) = subscription.series {
                self.release(series);
            }
        }
    }

    fn acquire(&self, series: Series) -> FeedHub {
        let mut running = lock(&self.running);

        let feed = running.entry(series).or_insert_with(|| {
            info!(%series, "starting feed");
            let hub = FeedHub::new(series);
            let task = hub.spawn();
            RunningFeed {
                hub,
                task,
                subscribers: 0,
            }
        });

        feed.subscribers += 1;
        feed.hub.clone()
    }

    fn release(&self, series: Series) {
        let mut running = lock(&self.running);

        if let Some(feed) = running.get_mut(&series) {
            feed.subscribers = feed.subscribers.saturating_sub(1);

            if feed.subscribers == 0 {
                info!(%series, "stopping feed");
                if let Some(feed) = running.remove(&series) {
                    feed.task.abort();
                }
            }
        }
    }
}

/// Streams a series to the webview: the current state first, then updates.
/// Returns an id for [`feed_unsubscribe`].
#[tauri::command]
async fn feed_subscribe(
    app: AppHandle,
    feeds: State<'_, Feeds>,
    series: Series,
    channel: Channel<HubMessage>,
) -> Result<u32, String> {
    let hub = feeds.acquire(series);
    let id = feeds.next_id.fetch_add(1, Ordering::Relaxed);

    // registered before forwarding starts, so an early disconnect finds it
    let mut subscriptions = lock(&feeds.subscriptions);
    subscriptions.insert(
        id,
        Subscription {
            series: Some(series),
            tasks: vec![],
        },
    );
    if let Some(subscription) = subscriptions.get_mut(&id) {
        subscription.tasks.push(forward(app, id, hub, channel));
    }

    Ok(id)
}

/// Streams a replay of a past session. Pausing, seeking or changing speed
/// means unsubscribing and subscribing again at the new position.
#[tauri::command]
async fn replay_subscribe(
    app: AppHandle,
    feeds: State<'_, Feeds>,
    request: ReplayRequest,
    channel: Channel<HubMessage>,
) -> Result<u32, String> {
    archive::check_session_path(&request.path).map_err(|err| err.to_string())?;

    let hub = FeedHub::new(Series::F1);
    let replay = hub.spawn_with(move |sink| replay::run_logged(Archive::f1(), request, sink));
    let id = feeds.next_id.fetch_add(1, Ordering::Relaxed);

    let mut subscriptions = lock(&feeds.subscriptions);
    subscriptions.insert(
        id,
        Subscription {
            series: None,
            tasks: vec![replay],
        },
    );
    if let Some(subscription) = subscriptions.get_mut(&id) {
        subscription.tasks.push(forward(app, id, hub, channel));
    }

    Ok(id)
}

#[tauri::command]
async fn replay_sessions(year: i32) -> Result<Vec<ArchiveMeeting>, String> {
    archive::meetings(&Archive::f1(), year)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn feed_unsubscribe(feeds: State<'_, Feeds>, id: u32) {
    feeds.unsubscribe(id);
}

#[tauri::command]
async fn schedule(series: Series) -> Result<Vec<Round>, String> {
    schedule::schedule(series, chrono::Utc::now().year())
        .await
        .map_err(|err| err.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,feeds=info".into()),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Feeds::default())
        .invoke_handler(tauri::generate_handler![
            feed_subscribe,
            feed_unsubscribe,
            replay_subscribe,
            replay_sessions,
            schedule
        ])
        .run(tauri::generate_context!())
        .expect("error while running f1-dash");
}
