//! FIA Formula 2, FIA Formula 3 and F1 Academy timing.
//!
//! The three series share one classic ASP.NET SignalR service, selected by a
//! series code. `GetData2` returns a snapshot of every feed, `JoinFeeds`
//! subscribes to pushes (`datafeed`, `statsfeed`, `trackfeed`, ...). The feed
//! has timing, weather, track status and commentary, but no tyre, team,
//! mini-sector or position data.

use std::collections::BTreeMap;

use anyhow::{Error, anyhow};
use futures::StreamExt;
use serde_json::{Map, Value, json};
use signalr::classic::{self, ClassicMessage, ClassicOptions};
use tracing::{debug, info, warn};

use super::util::{
    Publisher, fallback_colour, int, lap_time_millis, now_utc, split_name, stable_key, text, truthy,
};
use crate::Sink;

pub struct FiaSource {
    /// Series code the service expects.
    pub code: &'static str,
    /// Service endpoints, tried in order.
    pub endpoints: &'static [&'static str],
    pub origin: &'static str,
}

const LTSS: &str = "https://ltss.fiaformula2.com/streaming";

pub const F2: FiaSource = FiaSource {
    code: "F2",
    endpoints: &[LTSS],
    origin: "https://www.fiaformula2.com",
};

// F3 moved onto the F2 host in 2026; its own host is kept as a fallback
pub const F3: FiaSource = FiaSource {
    code: "F3",
    endpoints: &[LTSS, "https://ltss.fiaformula3.com/streaming"],
    origin: "https://www.fiaformula3.com",
};

// the F1 Academy site loads its hub from this host
pub const F1_ACADEMY: FiaSource = FiaSource {
    code: "F1 Academy",
    endpoints: &[
        "https://f2f3-prod-livetiming.azurewebsites.net/streaming",
        LTSS,
    ],
    origin: "https://www.f1academy.com",
};

const HUB: &str = "streaming";
const USER_AGENT: &str = concat!("f1-dash/", env!("CARGO_PKG_VERSION"));

const JOIN_FEEDS: [&str; 7] = [
    "data",
    "stats",
    "weather",
    "status",
    "time",
    "commentary",
    "racedetails",
];

const INITIAL_FEEDS: [&str; 8] = [
    "data",
    "statsfeed",
    "weatherfeed",
    "sessionfeed",
    "trackfeed",
    "commentaryfeed",
    "timefeed",
    "racedetailsfeed",
];

const MAX_MESSAGES: usize = 200;

/// The raw feed state, kept close to the wire format and translated into F1
/// topics on every change.
#[derive(Default)]
pub struct FiaState {
    session_type: Option<String>,
    lines: BTreeMap<String, Value>,
    best_laps: BTreeMap<String, String>,
    weather: Option<Value>,
    track: Option<Value>,
    session_status: Option<String>,
    clock: Option<(String, bool, String)>,
    details: Map<String, Value>,
    messages: Vec<Value>,
}

impl FiaState {
    /// Applies a `GetData2` result.
    pub fn apply_initial(&mut self, result: &Value) {
        if let Some(data) = result.get("data").and_then(Value::as_array) {
            if let Some(meta) = data.get(1) {
                self.apply_meta(meta);
            }
            if let Some(lines) = data.get(2) {
                self.apply_lines(lines);
            }
        }

        let second = |feed: &str| result.get(feed).and_then(|v| v.get(1)).cloned();

        if let Some(stats) = second("statsfeed") {
            self.apply_stats(&stats);
        }
        if let Some(weather) = second("weatherfeed") {
            self.weather = Some(weather);
        }
        if let Some(status) = second("sessionfeed") {
            self.session_status = text(status.get("Value"));
        }
        if let Some(track) = second("trackfeed") {
            self.track = Some(track);
        }
        if let Some(details) = second("racedetailsfeed") {
            self.apply_details(&details);
        }
        if let Some(time) = result.get("timefeed").and_then(Value::as_array) {
            self.apply_time(time);
        }
        if let Some(commentary) = second("commentaryfeed") {
            self.apply_commentary(&commentary);
        }
    }

    /// Applies a pushed hub call. Returns true when a new session started.
    pub fn apply_push(&mut self, method: &str, args: &[Value]) -> bool {
        let payload = args.get(1);

        match method.to_ascii_lowercase().as_str() {
            "datafeed" => {
                let Some(feed) = args.get(2).or(args.last()) else {
                    return false;
                };

                let new_session = feed
                    .get("UpdateType")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.eq_ignore_ascii_case("new"));

                if new_session {
                    self.lines.clear();
                    self.best_laps.clear();
                    self.messages.clear();
                }

                self.apply_meta(feed);
                if let Some(lines) = feed.get("lines") {
                    self.apply_lines(lines);
                }

                return new_session;
            }
            "statsfeed" => {
                if let Some(stats) = payload {
                    self.apply_stats(stats);
                }
            }
            "weatherfeed" => self.weather = payload.cloned(),
            "trackfeed" => self.track = payload.cloned(),
            "sessionfeed" => self.session_status = payload.and_then(|p| text(p.get("Value"))),
            "racedetailsfeed" => {
                if let Some(details) = payload {
                    self.apply_details(details);
                }
            }
            "timefeed" => self.apply_time(args),
            "commentaryfeed" => {
                if let Some(commentary) = payload {
                    self.apply_commentary(commentary);
                }
            }
            "comment" => {
                if let Some(commentary) = args.first() {
                    self.apply_commentary(commentary);
                }
            }
            other => debug!(method = other, "unhandled feed"),
        }

        false
    }

    fn apply_meta(&mut self, meta: &Value) {
        if let Some(session) = text(meta.get("Session")) {
            self.session_type = Some(session);
        }
    }

    /// Rows arrive keyed by car number (older captures used a list).
    fn apply_lines(&mut self, lines: &Value) {
        let rows: Vec<(String, &Value)> = match lines {
            Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v)).collect(),
            Value::Array(items) => items
                .iter()
                .filter_map(|row| Some((text(row.pointer("/driver/RacingNumber"))?, row)))
                .collect(),
            _ => return,
        };

        for (car, update) in rows {
            let line = self.lines.entry(car).or_insert(Value::Null);
            crate::state::merge(line, update.clone());
        }
    }

    fn apply_stats(&mut self, stats: &Value) {
        let lines = stats.get("lines").unwrap_or(stats);
        let items: Vec<&Value> = match lines {
            Value::Array(items) => items.iter().collect(),
            Value::Object(map) => map.values().collect(),
            _ => return,
        };

        for item in items {
            let Some(car) = text(item.pointer("/driver/RacingNumber")) else {
                continue;
            };
            if let Some(best) =
                text(item.pointer("/PersonalBestLapTime/Value")).filter(|b| !b.is_empty())
            {
                self.best_laps.insert(car, best);
            }
        }
    }

    fn apply_details(&mut self, details: &Value) {
        if let Some(map) = details.as_object() {
            for (k, v) in map {
                self.details.insert(k.clone(), v.clone());
            }
        }
    }

    /// `[timestamp, running, "HH:MM:SS" remaining]`
    fn apply_time(&mut self, args: &[Value]) {
        if let Some(remaining) = text(args.get(2)) {
            // stamped with the time it was received, so extrapolating on the
            // client does not depend on the feed's clock or time zone
            self.clock = Some((now_utc(), truthy(args.get(1)), remaining));
        }
    }

    fn apply_commentary(&mut self, value: &Value) {
        let items: Vec<&Value> = match value {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        };

        for item in items {
            let message = match item {
                Value::String(s) => Some(s.clone()),
                other => text(other.get("Text"))
                    .or_else(|| text(other.get("Message")))
                    .or_else(|| text(other.get("Value"))),
            };

            let Some(message) = message
                .map(|m| m.trim().to_owned())
                .filter(|m| !m.is_empty())
            else {
                continue;
            };

            // the snapshot and the first push can overlap by a message, but
            // race control does repeat itself (a second double yellow)
            if self
                .messages
                .last()
                .is_some_and(|m| m["Message"] == message.as_str())
            {
                continue;
            }

            self.messages.push(json!({
                "Utc": now_utc(),
                "Lap": 0,
                "Message": message,
                "Category": "Other",
            }));

            if self.messages.len() > MAX_MESSAGES {
                self.messages.remove(0);
            }
        }
    }

    fn is_race(&self) -> bool {
        self.session_type
            .as_deref()
            .is_some_and(|s| s.to_ascii_lowercase().contains("race"))
    }

    /// Everything translated into F1 topics.
    pub fn topics(&self, series: &str) -> Map<String, Value> {
        let mut topics = Map::new();
        let race = self.is_race();

        let mut driver_list = Map::new();
        let mut timing_lines = Map::new();
        let mut stats_lines = Map::new();

        // best lap per car, from the stats feed or the timing rows
        let best: BTreeMap<&str, String> =
            self.lines
                .iter()
                .filter_map(|(car, row)| {
                    let best =
                        self.best_laps.get(car).cloned().or_else(|| {
                            text(row.pointer("/best/Value")).filter(|b| !b.is_empty())
                        })?;
                    Some((car.as_str(), best))
                })
                .collect();

        let mut ranked: Vec<(&str, u64)> = best
            .iter()
            .filter_map(|(car, time)| Some((*car, lap_time_millis(time)?)))
            .collect();
        ranked.sort_by_key(|(_, millis)| *millis);

        for (car, row) in &self.lines {
            let full_name = text(row.pointer("/driver/FullName")).unwrap_or_default();
            let (first, last) = split_name(&full_name);
            let line = int(row.get("Number"))
                .or_else(|| int(row.pointer("/position/Value")))
                .unwrap_or(99);

            driver_list.insert(
                car.clone(),
                json!({
                    "RacingNumber": car,
                    "BroadcastName": text(row.pointer("/driver/BroadcastName")).unwrap_or_default(),
                    "FullName": full_name,
                    "FirstName": first,
                    "LastName": last,
                    "Tla": text(row.pointer("/driver/TLA")).unwrap_or_else(|| car.clone()),
                    "TeamName": text(row.pointer("/driver/TeamName")).unwrap_or_default(),
                    "TeamColour": fallback_colour(car),
                    "Line": line,
                    "Reference": "",
                    "HeadshotUrl": "",
                    "CountryCode": "",
                }),
            );

            let (gap_key, interval_key) = if race {
                ("gap", "interval")
            } else {
                ("gapP", "intervalP")
            };
            let value_of = |key: &str| text(row.pointer(&format!("/{key}/Value")));
            let gap = value_of(gap_key).or_else(|| value_of(if race { "gapP" } else { "gap" }));
            let interval = value_of(interval_key)
                .or_else(|| value_of(if race { "intervalP" } else { "interval" }));

            let lap_time = |key: &str| {
                json!({
                    "Value": text(row.pointer(&format!("/{key}/Value"))).unwrap_or_default(),
                    "Status": 0,
                    "OverallFastest": truthy(row.pointer(&format!("/{key}/OverallFastest"))),
                    "PersonalFastest": truthy(row.pointer(&format!("/{key}/PersonalFastest"))),
                })
            };

            let sectors: Vec<Value> = (0..3)
                .map(|i| {
                    let sector = row.pointer(&format!("/sectors/{i}"));
                    json!({
                        "Value": sector.and_then(|s| text(s.get("Value"))).unwrap_or_default(),
                        "Status": 0,
                        "Stopped": false,
                        "OverallFastest": truthy(sector.and_then(|s| s.get("OverallFastest"))),
                        "PersonalFastest": truthy(sector.and_then(|s| s.get("PersonalFastest"))),
                        "Segments": [],
                    })
                })
                .collect();

            timing_lines.insert(
                car.clone(),
                json!({
                    "RacingNumber": car,
                    "Line": line,
                    "Position": text(row.pointer("/position/Value")).unwrap_or_else(|| line.to_string()),
                    "ShowPosition": truthy(row.pointer("/position/Show")),
                    "GapToLeader": gap.unwrap_or_default(),
                    "IntervalToPositionAhead": { "Value": interval.unwrap_or_default(), "Catching": false },
                    "NumberOfLaps": int(row.pointer("/laps/Value")).unwrap_or(0),
                    "NumberOfPitStops": int(row.pointer("/pits/Value")).unwrap_or(0),
                    "InPit": truthy(row.pointer("/status/InPit")),
                    "PitOut": truthy(row.pointer("/status/PitOut")),
                    "Stopped": truthy(row.pointer("/status/Stopped")),
                    "Retired": truthy(row.pointer("/status/Retired")),
                    "KnockedOut": truthy(row.pointer("/qual/KnockOut")),
                    "Cutoff": truthy(row.pointer("/qual/Cutoff")),
                    "Status": 0,
                    "LastLapTime": lap_time("last"),
                    "BestLapTime": {
                        "Value": best.get(car.as_str()).cloned().unwrap_or_default(),
                        "Lap": int(row.pointer("/best/Lap")).unwrap_or(0),
                    },
                    "Sectors": sectors,
                }),
            );

            let position = ranked
                .iter()
                .position(|(c, _)| *c == car)
                .map_or(0, |p| p + 1);
            stats_lines.insert(
                car.clone(),
                json!({
                    "Line": line,
                    "RacingNumber": car,
                    "PersonalBestLapTime": {
                        "Value": best.get(car.as_str()).cloned().unwrap_or_default(),
                        "Position": position,
                    },
                    "BestSectors": [
                        { "Value": "", "Position": 0 },
                        { "Value": "", "Position": 0 },
                        { "Value": "", "Position": 0 },
                    ],
                }),
            );
        }

        topics.insert("DriverList".into(), Value::Object(driver_list));
        topics.insert(
            "TimingData".into(),
            json!({ "Lines": timing_lines, "Withheld": false }),
        );
        topics.insert(
            "TimingStats".into(),
            json!({ "Lines": stats_lines, "Withheld": false, "SessionType": self.session_type }),
        );

        let detail = |key: &str| text(self.details.get(key)).unwrap_or_default();
        let session_name = self
            .details
            .get("Session")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.session_type.clone())
            .unwrap_or_else(|| "Session".into());
        let meeting_name = match detail("Race") {
            name if name.is_empty() => series.to_owned(),
            name => name,
        };

        topics.insert(
            "SessionInfo".into(),
            json!({
                "Meeting": {
                    "Key": stable_key(&[series, &detail("Season"), &detail("Round"), &meeting_name]),
                    "Name": meeting_name,
                    "OfficialName": meeting_name,
                    "Location": detail("Circuit"),
                    "Country": { "Key": 0, "Code": detail("CountryCode"), "Name": detail("Country") },
                    "Circuit": { "Key": 0, "ShortName": detail("Circuit") },
                },
                "ArchiveStatus": { "Status": "Generating" },
                "Key": stable_key(&[series, &detail("Season"), &detail("Round"), &session_name]),
                "Type": session_type_name(self.session_type.as_deref()),
                "Name": session_name,
                "StartDate": "",
                "EndDate": "",
                "GmtOffset": "00:00:00",
                "Path": "",
            }),
        );

        if let Some(track) = &self.track {
            topics.insert(
                "TrackStatus".into(),
                json!({
                    "Status": text(track.get("Value")).unwrap_or_default(),
                    "Message": text(track.get("Message")).unwrap_or_default(),
                }),
            );
        }

        if let Some(status) = &self.session_status {
            topics.insert("SessionStatus".into(), json!({ "Status": status }));
        }

        if let Some((utc, running, remaining)) = &self.clock {
            topics.insert(
                "ExtrapolatedClock".into(),
                json!({ "Utc": utc, "Remaining": remaining, "Extrapolating": running }),
            );
        }

        if let Some(weather) = &self.weather {
            let field = |key: &str| text(weather.get(key)).unwrap_or_default();
            topics.insert(
                "WeatherData".into(),
                json!({
                    "AirTemp": field("airtemp"),
                    "TrackTemp": field("tracktemp"),
                    "Humidity": field("humidity"),
                    "Pressure": field("pressure"),
                    "WindSpeed": field("windspeed"),
                    "WindDirection": field("winddir"),
                    "Rainfall": field("rainfall"),
                }),
            );
        }

        topics.insert(
            "RaceControlMessages".into(),
            json!({ "Messages": self.messages }),
        );

        if race {
            let lap = self
                .lines
                .values()
                .filter_map(|row| int(row.pointer("/laps/Value")))
                .max()
                .unwrap_or(0);
            topics.insert("LapCount".into(), json!({ "CurrentLap": lap }));
        }

        topics
    }
}

fn session_type_name(feed: Option<&str>) -> &'static str {
    match feed.map(str::to_ascii_lowercase).as_deref() {
        Some(s) if s.contains("race") => "Race",
        Some(s) if s.contains("qual") => "Qualifying",
        _ => "Practice",
    }
}

async fn connect(source: &FiaSource) -> Result<classic::ClassicClient, Error> {
    let mut last_error = anyhow!("no endpoints configured");

    for endpoint in source.endpoints {
        match classic::connect(ClassicOptions {
            base_url: endpoint,
            hub: HUB,
            origin: Some(source.origin),
            user_agent: USER_AGENT,
        })
        .await
        {
            Ok(client) => {
                info!(series = source.code, endpoint, "connected");
                return Ok(client);
            }
            Err(err) => {
                warn!(series = source.code, endpoint, ?err, "connect failed");
                last_error = err;
            }
        }
    }

    Err(last_error)
}

pub async fn run(source: &FiaSource, sink: &Sink) -> Result<(), Error> {
    let mut client = connect(source).await?;

    client
        .invoke("JoinFeeds", vec![json!(source.code), json!(JOIN_FEEDS)])
        .await?;
    let snapshot_id = client
        .invoke("GetData2", vec![json!(source.code), json!(INITIAL_FEEDS)])
        .await?;

    let mut state = FiaState::default();
    let mut publisher = Publisher::default();
    let mut stream = std::pin::pin!(client.into_stream());

    // pushes can arrive before the snapshot; publishing waits for it so the
    // first state clients see is complete
    let mut ready = false;

    while let Some(message) = stream.next().await {
        match message {
            ClassicMessage::Result { id, result, error } if id == snapshot_id => {
                if let Some(error) = error {
                    return Err(anyhow!("{} snapshot rejected: {error}", source.code));
                }
                if let Some(result) = result {
                    state.apply_initial(&result);
                }
                ready = true;
            }
            ClassicMessage::Result {
                error: Some(error), ..
            } => {
                warn!(series = source.code, error, "invocation failed");
                continue;
            }
            ClassicMessage::Result { .. } => continue,
            ClassicMessage::Invocation { method, args } => {
                if state.apply_push(&method, &args) {
                    info!(series = source.code, "new session");
                    publisher.restart();
                }
            }
        }

        if ready {
            publisher.publish(sink, state.topics(source.code)).await;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // the snapshot shape as captured from the live service in June 2026
    fn snapshot() -> Value {
        json!({
            "data": [
                "2026-06-05T14:04:30",
                { "cutOffTime": { "Percentage": "107", "Value": "1:26.726" }, "Series": "F2", "Session": "Qualifying", "DataWithheld": 0 },
                {
                    "2": {
                        "Number": "5",
                        "position": { "Show": 1, "Value": "5" },
                        "status": { "Retired": 0, "InPit": 1, "PitOut": 0, "Stopped": 0 },
                        "driver": { "RacingNumber": "2", "FullName": "Joshua DURKSEN", "BroadcastName": "J DURKSEN", "TLA": "DUR" },
                        "gapP": { "Value": "+0.5" },
                        "intervalP": { "Value": "+0.1" },
                        "qual": { "KnockOut": 0, "Cutoff": 0 },
                        "laps": { "Value": "11" },
                        "pits": { "Value": "1" },
                        "sectors": [
                            { "OverallFastest": 0, "PersonalFastest": 1, "Value": "22.1" },
                            { "OverallFastest": 0, "PersonalFastest": 0, "Value": "33.2" },
                            { "OverallFastest": 0, "PersonalFastest": 0, "Value": "26.2" }
                        ],
                        "last": { "OverallFastest": 0, "PersonalFastest": 1, "Value": "1:21.557" },
                        "best": { "Lap": "10", "Value": "1:21.557" }
                    },
                    "7": {
                        "Number": "1",
                        "position": { "Show": 1, "Value": "1" },
                        "status": { "Retired": 0, "InPit": 0, "PitOut": 0, "Stopped": 0 },
                        "driver": { "RacingNumber": "7", "FullName": "Test DRIVER", "BroadcastName": "T DRIVER", "TLA": "TES" },
                        "laps": { "Value": "12" },
                        "best": { "Lap": "9", "Value": "1:21.057" }
                    }
                }
            ],
            "trackfeed": ["ts", { "Value": "1", "Message": "AllClear" }],
            "sessionfeed": ["ts", { "Value": "Started" }],
            "timefeed": ["ts", true, "00:12:31"],
            "weatherfeed": ["ts", { "airtemp": "24.2", "tracktemp": "43.0", "humidity": "59.9", "pressure": "1011.4", "windspeed": "0.0", "winddir": "0", "rainfall": "0" }],
            "racedetailsfeed": ["ts", { "Season": "2026", "Round": "6", "Race": "Monaco", "Country": "Monaco", "CountryCode": "MON", "Circuit": "Monte Carlo", "Session": "Qualifying" }]
        })
    }

    #[test]
    fn translates_snapshot_into_f1_topics() {
        let mut state = FiaState::default();
        state.apply_initial(&snapshot());
        let topics = state.topics("F2");

        let line = &topics["TimingData"]["Lines"]["2"];
        assert_eq!(line["Position"], "5");
        assert_eq!(line["InPit"], true);
        assert_eq!(line["GapToLeader"], "+0.5");
        assert_eq!(line["IntervalToPositionAhead"]["Value"], "+0.1");
        assert_eq!(line["NumberOfLaps"], 11);
        assert_eq!(line["NumberOfPitStops"], 1);
        assert_eq!(line["LastLapTime"]["PersonalFastest"], true);
        assert_eq!(line["Sectors"][0]["Value"], "22.1");
        assert!(
            line["Sectors"][0]["Segments"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let driver = &topics["DriverList"]["2"];
        assert_eq!(driver["Tla"], "DUR");
        assert_eq!(driver["LastName"], "DURKSEN");

        // car 7 has the faster best lap
        assert_eq!(
            topics["TimingStats"]["Lines"]["7"]["PersonalBestLapTime"]["Position"],
            1
        );
        assert_eq!(
            topics["TimingStats"]["Lines"]["2"]["PersonalBestLapTime"]["Position"],
            2
        );

        assert_eq!(topics["SessionInfo"]["Name"], "Qualifying");
        assert_eq!(topics["SessionInfo"]["Type"], "Qualifying");
        assert_eq!(topics["SessionInfo"]["Meeting"]["Location"], "Monte Carlo");
        assert_eq!(topics["TrackStatus"]["Status"], "1");
        assert_eq!(topics["SessionStatus"]["Status"], "Started");
        assert_eq!(topics["ExtrapolatedClock"]["Remaining"], "00:12:31");
        assert_eq!(topics["ExtrapolatedClock"]["Extrapolating"], true);
        assert_eq!(topics["WeatherData"]["TrackTemp"], "43.0");
        assert!(
            !topics.contains_key("LapCount"),
            "no lap count outside races"
        );
    }

    #[test]
    fn merges_pushed_deltas_and_detects_new_sessions() {
        let mut state = FiaState::default();
        state.apply_initial(&snapshot());

        let new = state.apply_push(
            "datafeed",
            &[json!("ts"), json!(null), json!({ "lines": { "2": { "position": { "Value": "3" }, "status": { "InPit": 0 } } } })],
        );
        assert!(!new);

        let topics = state.topics("F2");
        assert_eq!(topics["TimingData"]["Lines"]["2"]["Position"], "3");
        assert_eq!(topics["TimingData"]["Lines"]["2"]["InPit"], false);
        assert_eq!(
            topics["DriverList"]["2"]["Tla"], "DUR",
            "untouched fields survive"
        );

        let new = state.apply_push(
            "datafeed",
            &[
                json!("ts"),
                json!(null),
                json!({ "UpdateType": "new", "Session": "Race", "lines": {} }),
            ],
        );
        assert!(new);
        let topics = state.topics("F2");
        assert!(
            topics["TimingData"]["Lines"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert_eq!(topics["LapCount"]["CurrentLap"], 0);
    }

    #[test]
    fn uses_race_gaps_in_races() {
        let mut state = FiaState::default();
        state.apply_push(
            "datafeed",
            &[json!("ts"), json!(null), json!({
                "Session": "Race",
                "lines": { "4": { "driver": { "RacingNumber": "4" }, "gap": { "Value": "+3.2" }, "gapP": { "Value": "old" }, "laps": { "Value": "7" } } }
            })],
        );

        let topics = state.topics("F2");
        assert_eq!(topics["TimingData"]["Lines"]["4"]["GapToLeader"], "+3.2");
        assert_eq!(topics["LapCount"]["CurrentLap"], 7);
    }

    #[test]
    fn collects_commentary_without_duplicates() {
        let mut state = FiaState::default();
        state.apply_push(
            "commentaryfeed",
            &[json!("ts"), json!("DOUBLE YELLOW SECTOR 2")],
        );
        state.apply_push(
            "commentaryfeed",
            &[json!("ts"), json!({ "Text": "DOUBLE YELLOW SECTOR 2" })],
        );
        state.apply_push(
            "commentaryfeed",
            &[json!("ts"), json!([{ "Text": "GREEN FLAG" }])],
        );
        state.apply_push(
            "commentaryfeed",
            &[json!("ts"), json!("DOUBLE YELLOW SECTOR 2")],
        );

        let topics = state.topics("F2");
        let messages: Vec<&str> = topics["RaceControlMessages"]["Messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["Message"].as_str().unwrap())
            .collect();
        assert_eq!(
            messages,
            vec![
                "DOUBLE YELLOW SECTOR 2",
                "GREEN FLAG",
                "DOUBLE YELLOW SECTOR 2"
            ],
            "an overlapping repeat is dropped, a later repeat is kept"
        );
    }
}
