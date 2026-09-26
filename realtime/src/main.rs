use std::{collections::HashMap, env, sync::Arc};

use anyhow::Error;
use feeds::{FeedHub, Series};
use shared::tracing_subscriber;
use tracing::info;

mod http_server;

/// Series to ingest, from `SERIES` (comma separated ids, default: all).
fn enabled_series() -> Result<Vec<Series>, Error> {
    match env::var("SERIES") {
        Ok(list) if !list.trim().is_empty() => list
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(str::parse)
            .collect(),
        _ => Ok(Series::ALL.to_vec()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber();

    let hubs: HashMap<Series, FeedHub> = enabled_series()?
        .into_iter()
        .map(|series| (series, FeedHub::new(series)))
        .collect();

    for hub in hubs.values() {
        info!(series = %hub.series(), "starting feed");
        hub.spawn();
    }

    http_server::start(Arc::new(hubs)).await?;

    Ok(())
}
