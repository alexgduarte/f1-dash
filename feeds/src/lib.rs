//! Live timing feed adapters.
//!
//! Each adapter connects to one championship's official timing feed and
//! normalises it into the F1 live timing topic layout, so the dashboard renders
//! every series with the same components. The realtime server runs these for
//! web clients; the native app runs them in-process and talks to the feeds
//! directly.

pub mod adapters;
pub mod hub;
pub mod schedule;
pub mod series;
pub mod state;
pub mod tyres;

pub use hub::{FeedHub, HubMessage, Sink};
pub use series::Series;
