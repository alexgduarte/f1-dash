//! Reconstructs tyre sets from archived live timing files.
//!
//! ```sh
//! cargo run -p feeds --example tyre_sets -- standard \
//!     "Qualifying=q/TimingAppData.jsonStream,q/TimingData.jsonStream" \
//!     "Race=r/TimingAppData.jsonStream"
//! ```
//!
//! Each argument after the format is `<session name>=<TimingAppData file>`,
//! optionally followed by `,<TimingData file>` for qualifying (to find the Q3
//! cars). Files may be keyframes (`.json`) or streams (`.jsonStream`). The last
//! session is treated as the one in progress.

use std::collections::BTreeSet;

use anyhow::{Context, Error};
use feeds::{
    state::merge,
    tyres::{self, PastSession, Session, SessionKind, rules::Format},
};
use serde_json::Value;

/// Reads a keyframe, or folds a `.jsonStream` (`HH:MM:SS.mmm{json}` per line)
/// into its final state.
fn read_state(path: &str) -> Result<Value, Error> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let text = text.trim_start_matches('\u{feff}');

    if !path.ends_with(".jsonStream") {
        return Ok(serde_json::from_str(text)?);
    }

    let mut state = Value::Null;
    for line in text.lines() {
        if let Some(start) = line.find('{') {
            merge(&mut state, serde_json::from_str(&line[start..])?);
        }
    }
    Ok(state)
}

fn q3_from(timing_data: &Value) -> BTreeSet<String> {
    timing_data["Lines"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, line)| {
            line["Position"]
                .as_str()
                .and_then(|p| p.parse::<u32>().ok())
                .is_some_and(|p| p <= 10)
        })
        .map(|(nr, _)| nr.clone())
        .collect()
}

fn main() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);

    let format = match args.next().as_deref() {
        Some("sprint") => Format::Sprint,
        Some("standard") => Format::Standard,
        _ => anyhow::bail!("usage: tyre_sets <standard|sprint> <Name=file[,timing]>..."),
    };

    let mut sessions: Vec<PastSession> = Vec::new();

    for arg in args {
        let (name, files) = arg.split_once('=').context("expected Name=file")?;
        let mut files = files.split(',');

        let stints =
            tyres::parse_timing_app_data(&read_state(files.next().context("missing file")?)?);
        let q3 = files
            .next()
            .map(read_state)
            .transpose()?
            .map(|t| q3_from(&t));

        sessions.push(PastSession {
            session: Some(Session {
                name: name.to_owned(),
                kind: SessionKind::from_name(name).context("unknown session name")?,
            }),
            stints: Some(stints),
            q3,
        });
    }

    let current = sessions.pop().context("need at least one session")?;
    let current_session = current.session.context("session")?;
    let current_stints = current.stints.unwrap_or_default();

    let result = tyres::compute(
        format,
        &sessions,
        Some((&current_session, &current_stints)),
        true,
    );

    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}
