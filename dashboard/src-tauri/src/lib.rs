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
    schedule::{self, Round},
};
use futures::StreamExt;
use tauri::{State, ipc::Channel};
use tokio::task::JoinHandle;
use tracing::info;

struct RunningFeed {
    hub: FeedHub,
    task: JoinHandle<()>,
    subscribers: usize,
}

/// Feeds run only while something subscribes to them, which keeps a phone
/// from holding connections open for series nobody is looking at.
#[derive(Default)]
struct Feeds {
    running: Mutex<HashMap<Series, RunningFeed>>,
    subscriptions: Mutex<HashMap<u32, (Series, JoinHandle<()>)>>,
    next_id: AtomicU32,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Feeds {
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
    feeds: State<'_, Feeds>,
    series: Series,
    channel: Channel<HubMessage>,
) -> Result<u32, String> {
    let hub = feeds.acquire(series);
    let id = feeds.next_id.fetch_add(1, Ordering::Relaxed);

    let forward = tokio::spawn(async move {
        let mut stream = std::pin::pin!(hub.stream());

        while let Some(message) = stream.next().await {
            // the webview went away (reload, closed window)
            if channel.send(message).is_err() {
                break;
            }
        }
    });

    lock(&feeds.subscriptions).insert(id, (series, forward));

    Ok(id)
}

#[tauri::command]
fn feed_unsubscribe(feeds: State<'_, Feeds>, id: u32) {
    let subscription = lock(&feeds.subscriptions).remove(&id);

    if let Some((series, forward)) = subscription {
        forward.abort();
        feeds.release(series);
    }
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
            schedule
        ])
        .run(tauri::generate_context!())
        .expect("error while running f1-dash");
}
