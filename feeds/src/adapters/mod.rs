use anyhow::Error;

use crate::{Series, Sink};

pub mod fia;
pub mod griiip;
pub mod livetiming;
pub mod util;

/// Runs the adapter for `series` until its connection ends or fails.
pub async fn run(series: Series, sink: &Sink) -> Result<(), Error> {
    match series {
        Series::F1 => livetiming::run(&livetiming::F1, sink).await,
        Series::F2 => fia::run(&fia::F2, sink).await,
        Series::F3 => fia::run(&fia::F3, sink).await,
        Series::F1Academy => fia::run(&fia::F1_ACADEMY, sink).await,
        Series::Wec => griiip::run(&griiip::WEC, sink).await,
    }
}
