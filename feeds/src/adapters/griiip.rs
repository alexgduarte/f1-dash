//! FIA World Endurance Championship timing from GriiipLive, the platform behind
//! the official live timing page (livetiming.fiawec.com) since 2026.
//!
//! The public insights API has no push channel without extra negotiation, so
//! this polls the session `bootstrap`, which holds the whole live state, and
//! translates it into F1 topics. WEC specifics (classes, crews, class
//! positions) ride along as extra fields.

use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use anyhow::{Context, Error};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use tracing::{debug, info};

use super::util::{Publisher, format_lap_time, int, now_utc, stable_key, text, tla_from, truthy};
use crate::Sink;

pub struct GriiipSource {
    pub series_id: u32,
    pub name: &'static str,
    /// Classes of the championship, to leave out support races sharing a session.
    pub classes: &'static [&'static str],
}

pub const WEC: GriiipSource = GriiipSource {
    series_id: 10,
    name: "WEC",
    classes: &["HYPERCAR", "LMP2", "LMGT3"],
};

const BASE: &str = "https://insights.griiip.com";
const USER_AGENT: &str = concat!("f1-dash/", env!("CARGO_PKG_VERSION"));

const LIVE_POLL: Duration = Duration::from_secs(3);
const IDLE_POLL: Duration = Duration::from_secs(20);
const RESOLVE_EVERY: Duration = Duration::from_secs(30);
const MAX_MESSAGES: usize = 100;

async fn get_json(client: &reqwest::Client, path: &str) -> Result<Value, Error> {
    client
        .get(format!("{BASE}{path}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .with_context(|| format!("invalid json from {path}"))
}

fn classes_of(session: &Value) -> Vec<String> {
    session
        .get("sessionClasses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| text(c.get("classId")))
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn is_series_session(source: &GriiipSource, session: &Value) -> bool {
    let name = format!(
        "{} {}",
        text(session.get("seriesName")).unwrap_or_default(),
        text(session.get("eventName")).unwrap_or_default()
    )
    .to_ascii_uppercase();

    name.contains("WORLD ENDURANCE")
        || name.split_whitespace().any(|word| word == source.name)
        || classes_of(session)
            .iter()
            .any(|c| source.classes.contains(&c.as_str()))
}

fn timestamp(value: Option<&Value>) -> i64 {
    text(value)
        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
        .map_or(0, |t| t.timestamp_millis())
}

/// The session to show: a live one, else one that started and has not
/// closed, else the most recently published.
fn pick_live_session(source: &GriiipSource, sessions: &[Value]) -> Option<i64> {
    let rank = |s: &Value| {
        let availability = text(s.get("sessionAvailability")).unwrap_or_default();
        match availability.as_str() {
            "Live" => 3,
            "Closed" => 1,
            _ if truthy(s.get("hasSeenChequered")) => 1,
            _ if truthy(s.get("isStarted")) => 2,
            _ => 0,
        }
    };

    sessions
        .iter()
        .filter(|s| is_series_session(source, s))
        .filter(|s| int(s.get("sid")).is_some())
        .max_by_key(|s| {
            let published = s
                .pointer("/connectionStatus/lastPublished")
                .or_else(|| s.get("ts"));
            (rank(s), timestamp(published))
        })
        .and_then(|s| int(s.get("sid")))
}

/// Fallback from the calendar: the running session, else the latest started.
fn pick_scheduled_session(sessions: &[Value], now: i64) -> Option<i64> {
    let visible = sessions.iter().filter(|s| !truthy(s.get("hideFromUsers")));

    let running = visible
        .clone()
        .filter(|s| truthy(s.get("isRunning")))
        .max_by_key(|s| timestamp(s.get("startTime")));

    running
        .or_else(|| {
            visible
                .filter(|s| {
                    let start = timestamp(s.get("startTime"));
                    start > 0 && start <= now
                })
                .max_by_key(|s| timestamp(s.get("startTime")))
        })
        .and_then(|s| int(s.get("id")))
}

async fn resolve_session(
    client: &reqwest::Client,
    source: &GriiipSource,
) -> Result<Option<i64>, Error> {
    if let Ok(Value::Array(sessions)) =
        get_json(client, "/live/session-info/?includeViewers=false").await
        && let Some(sid) = pick_live_session(source, &sessions)
    {
        return Ok(Some(sid));
    }

    let since =
        (Utc::now() - chrono::Duration::days(7)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let path = format!(
        "/meta/sessions?dateTime={}&forward=true&seriesIds={}",
        urlencode(&since),
        source.series_id
    );

    match get_json(client, &path).await? {
        Value::Array(sessions) => Ok(pick_scheduled_session(
            &sessions,
            Utc::now().timestamp_millis(),
        )),
        _ => Ok(None),
    }
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Items of a per-car array, keyed by participant id.
fn by_pid(items: Option<&Value>) -> HashMap<i64, &Value> {
    items
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| Some((int(item.get("pid"))?, item)))
        .collect()
}

/// The newest item per participant, by lap number then timestamp.
fn latest_by_pid(items: Option<&Value>) -> HashMap<i64, &Value> {
    let mut out: HashMap<i64, &Value> = HashMap::new();

    for item in items.and_then(Value::as_array).into_iter().flatten() {
        let Some(pid) = int(item.get("pid")) else {
            continue;
        };
        let key = |v: &Value| (int(v.get("lapNumber")).unwrap_or(0), timestamp(v.get("ts")));

        if out.get(&pid).is_none_or(|prev| key(item) >= key(prev)) {
            out.insert(pid, item);
        }
    }

    out
}

fn gap_text(millis: Option<i64>, laps: Option<i64>) -> String {
    match (laps, millis) {
        (Some(laps), _) if laps > 0 => format!("+{laps} LAP{}", if laps == 1 { "" } else { "S" }),
        (_, Some(ms)) if ms > 0 => format!("+{:.3}", ms as f64 / 1000.0),
        _ => String::new(),
    }
}

fn lap_text(millis: Option<i64>) -> String {
    millis
        .filter(|ms| *ms > 0)
        .map(|ms| format_lap_time(ms as u64))
        .unwrap_or_default()
}

fn colour_flags(item: Option<&Value>) -> (bool, bool) {
    let colour = item
        .and_then(|i| text(i.get("color")))
        .unwrap_or_default()
        .to_ascii_uppercase();
    (colour == "PURPLE", colour == "GREEN")
}

fn class_colour(class: &str) -> &'static str {
    match class {
        "HYPERCAR" => "DC2626",
        "LMP2" => "2563EB",
        "LMGT3" => "16A34A",
        _ => "71717A",
    }
}

/// F1 track status codes, plus 8 (full course yellow) and 9 (code 60) which
/// only endurance racing uses.
fn track_status(flag: &str) -> (&'static str, &'static str) {
    match flag.to_ascii_uppercase().replace(['_', '-'], " ").as_str() {
        "GREEN" => ("1", "AllClear"),
        "YELLOW" | "DOUBLE YELLOW" => ("2", "Yellow"),
        "SAFETY CAR" | "SAFETYCAR" | "SC" => ("4", "SCDeployed"),
        "RED" | "RED FLAG" | "REDFLAG" => ("5", "Red"),
        "FCY" | "FULL COURSE YELLOW" | "FULLCOURSEYELLOW" => ("8", "FullCourseYellow"),
        "CODE60" | "CODE 60" => ("9", "Code60"),
        _ => ("1", "AllClear"),
    }
}

fn rc_flag(flag: &str) -> Option<&'static str> {
    match flag.to_ascii_uppercase().as_str() {
        "GREEN" => Some("GREEN"),
        "YELLOW" => Some("YELLOW"),
        "DOUBLE YELLOW" => Some("DOUBLE YELLOW"),
        "RED" => Some("RED"),
        "CHEQUERED" | "CHECKERED" => Some("CHEQUERED"),
        _ => None,
    }
}

fn hms(total_seconds: i64) -> String {
    let s = total_seconds.max(0);
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Whether the session is still running, which sets the poll rate.
pub fn is_live(bootstrap: &Value) -> bool {
    let info = bootstrap.get("sessionInfo");
    let availability = info.and_then(|i| text(i.get("sessionAvailability")));
    let chequered = info.is_some_and(|i| truthy(i.get("hasSeenChequered")));

    !chequered
        && (availability.as_deref() == Some("Live")
            || info.is_some_and(|i| {
                truthy(i.get("isStarted")) && availability.as_deref() != Some("Closed")
            }))
}

/// Translates a bootstrap payload into F1 topics.
pub fn topics(source: &GriiipSource, sid: i64, bootstrap: &Value) -> Map<String, Value> {
    let info = bootstrap.get("sessionInfo").cloned().unwrap_or(Value::Null);

    let participants = by_pid(bootstrap.get("participants"));
    let gaps = by_pid(bootstrap.get("gaps"));
    let statuses = by_pid(bootstrap.get("runningStatuses"));
    let locations = by_pid(bootstrap.get("carLocations"));
    let tyres = by_pid(bootstrap.get("tires"));
    let best_laps = by_pid(bootstrap.get("bestLaps"));
    let last_laps = latest_by_pid(bootstrap.get("laps"));

    let mut pit_stops: HashMap<i64, i64> = HashMap::new();
    for item in bootstrap
        .get("pitIns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(pid) = int(item.get("pid")) {
            *pit_stops.entry(pid).or_default() += 1;
        }
    }

    // latest time per sector and participant
    let mut sectors: HashMap<i64, BTreeMap<i64, &Value>> = HashMap::new();
    for sector in bootstrap
        .get("sectors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(pid), Some(number)) = (int(sector.get("pid")), int(sector.get("sectorNumber")))
        else {
            continue;
        };
        let slot = sectors
            .entry(pid)
            .or_default()
            .entry(number)
            .or_insert(sector);
        let key = |v: &Value| (int(v.get("lapNumber")).unwrap_or(0), timestamp(v.get("ts")));
        if key(sector) >= key(slot) {
            *slot = sector;
        }
    }
    if let Some(current) = bootstrap
        .get("currentLapSectors")
        .and_then(Value::as_object)
    {
        for (pid, list) in current {
            let Ok(pid) = pid.parse::<i64>() else {
                continue;
            };
            for sector in list.as_array().into_iter().flatten() {
                if let Some(number) = int(sector.get("sectorNumber")) {
                    sectors.entry(pid).or_default().insert(number, sector);
                }
            }
        }
    }
    let sector_count = bootstrap
        .pointer("/trackDetails/sectorsCount")
        .and_then(|v| int(Some(v)))
        .unwrap_or(3)
        .clamp(1, 6);

    let ranks: Vec<&Value> = bootstrap
        .get("ranks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| !truthy(r.get("isDeleted")) && int(r.get("pid")).is_some())
        .collect();

    // support races can share a session; keep the championship's classes when present
    let has_series_class = ranks.iter().any(|r| {
        text(r.get("classId"))
            .is_some_and(|c| source.classes.contains(&c.to_ascii_uppercase().as_str()))
    });

    let mut driver_list = Map::new();
    let mut timing_lines = Map::new();
    let mut app_lines = Map::new();
    let mut leader_laps = 0;

    for rank in ranks {
        let pid = int(rank.get("pid")).unwrap_or_default();
        let class = text(rank.get("classId"))
            .unwrap_or_default()
            .to_ascii_uppercase();
        if has_series_class && !source.classes.contains(&class.as_str()) {
            continue;
        }

        let participant = participants.get(&pid).copied();
        let car = text(participant.and_then(|p| p.get("carNumber")))
            .or_else(|| text(rank.get("carNumber")))
            .unwrap_or_else(|| pid.to_string());

        let drivers: Vec<&Value> = participant
            .and_then(|p| p.get("drivers"))
            .and_then(Value::as_array)
            .map(|d| d.iter().collect())
            .unwrap_or_default();
        let current_id = participant.and_then(|p| text(p.get("currentDriverId")));
        let driver = drivers
            .iter()
            .find(|d| text(d.get("externalDriverID")) == current_id && current_id.is_some())
            .or(drivers.first())
            .copied();

        let name = driver
            .and_then(|d| text(d.get("displayName")))
            .or_else(|| participant.and_then(|p| text(p.get("displayName"))))
            .unwrap_or_else(|| format!("Car {car}"));
        let tla = driver
            .and_then(|d| text(d.get("threeLettersName")))
            .or_else(|| participant.and_then(|p| text(p.get("threeLettersName"))))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| tla_from(name.rsplit(' ').next().unwrap_or(&name), &car));
        let crew: Vec<String> = drivers
            .iter()
            .filter_map(|d| text(d.get("displayName")))
            .collect();

        let overall = int(rank.get("overallPosition"))
            .or_else(|| int(rank.get("position")))
            .unwrap_or(0);
        let class_position = int(rank.get("position"));

        let last = last_laps.get(&pid).copied();
        let best = best_laps.get(&pid).copied();
        let laps = last
            .and_then(|l| int(l.get("lapNumber")))
            .max(best.and_then(|b| int(b.get("lapNumber"))))
            .unwrap_or(0);
        leader_laps = leader_laps.max(laps);

        let status = statuses
            .get(&pid)
            .and_then(|s| text(s.get("status")))
            .unwrap_or_default()
            .to_ascii_uppercase();
        let in_pit = locations
            .get(&pid)
            .and_then(|l| text(l.get("carLocation")))
            .map(|l| l.eq_ignore_ascii_case("pit"))
            .unwrap_or_else(|| status.contains("PIT") && !status.contains("OUT"));

        let gap = gaps.get(&pid).copied();
        let (last_overall, last_personal) = colour_flags(last);

        let sector_values: Vec<Value> = (1..=sector_count)
            .map(|n| {
                let sector = sectors.get(&pid).and_then(|s| s.get(&n)).copied();
                let (overall, personal) = colour_flags(sector);
                json!({
                    "Value": lap_text(sector.and_then(|s| int(s.get("sectorTimeMillis")))),
                    "Status": 0,
                    "Stopped": false,
                    "OverallFastest": overall,
                    "PersonalFastest": personal,
                    "Segments": [],
                })
            })
            .collect();

        driver_list.insert(
            car.clone(),
            json!({
                "RacingNumber": car,
                "BroadcastName": name,
                "FullName": name,
                "FirstName": "",
                "LastName": name,
                "Tla": tla,
                "TeamName": participant.and_then(|p| text(p.get("teamName"))).unwrap_or_default(),
                "TeamColour": class_colour(&class),
                "Line": overall,
                "Reference": "",
                "HeadshotUrl": "",
                "CountryCode": "",
                "Class": class,
                "Vehicle": participant.and_then(|p| text(p.get("manufacturer"))).unwrap_or_default(),
                "Crew": crew,
            }),
        );

        timing_lines.insert(
            car.clone(),
            json!({
                "RacingNumber": car,
                "Line": overall,
                "Position": overall.to_string(),
                "ClassPosition": class_position,
                "ShowPosition": true,
                "GapToLeader": if overall == 1 { String::new() } else {
                    gap_text(gap.and_then(|g| int(g.get("gapToFirstMillis"))), gap.and_then(|g| int(g.get("gapToFirstLaps"))))
                },
                "IntervalToPositionAhead": {
                    "Value": if overall == 1 { String::new() } else {
                        gap_text(gap.and_then(|g| int(g.get("gapToAheadMillis"))), gap.and_then(|g| int(g.get("gapToAheadLaps"))))
                    },
                    "Catching": false,
                },
                "NumberOfLaps": laps,
                "NumberOfPitStops": pit_stops.get(&pid).copied().unwrap_or(0),
                "InPit": in_pit,
                "PitOut": status.contains("OUT LAP") || status.contains("OUTLAP"),
                "Stopped": status.contains("STOPPED"),
                "Retired": status.contains("RETIRED") || status.contains("DNF") || status.contains("DIDNOTFINISH"),
                "KnockedOut": false,
                "Cutoff": false,
                "Status": 0,
                "LastLapTime": {
                    "Value": lap_text(last.and_then(|l| int(l.get("lapTimeMillis")))),
                    "Status": 0,
                    "OverallFastest": last_overall,
                    "PersonalFastest": last_personal,
                },
                "BestLapTime": {
                    "Value": lap_text(best.and_then(|b| int(b.get("lapTimeMillis")))),
                    "Lap": best.and_then(|b| int(b.get("lapNumber"))).unwrap_or(0),
                },
                "Sectors": sector_values,
            }),
        );

        if let Some(tyre) = tyres
            .get(&pid)
            .and_then(|t| t.get("tires"))
            .and_then(Value::as_array)
            .and_then(|t| t.first())
        {
            let age = int(tyre.get("ageInLaps")).unwrap_or(0);
            app_lines.insert(
                car.clone(),
                json!({
                    "RacingNumber": car,
                    "Line": overall,
                    "Stints": [{
                        "Compound": text(tyre.get("compound")).unwrap_or_default().to_ascii_uppercase(),
                        "TotalLaps": age,
                        "New": if age == 0 { "true" } else { "false" },
                    }],
                }),
            );
        }
    }

    // fastest lap holder gets PersonalBestLapTime position 1, as in F1
    let mut ranked_best: Vec<(String, i64)> = timing_lines
        .iter()
        .filter_map(|(car, line)| {
            let millis =
                super::util::lap_time_millis(line.pointer("/BestLapTime/Value")?.as_str()?)?;
            Some((car.clone(), millis as i64))
        })
        .collect();
    ranked_best.sort_by_key(|(_, ms)| *ms);

    let stats_lines: Map<String, Value> = timing_lines
        .iter()
        .map(|(car, line)| {
            let position = ranked_best.iter().position(|(c, _)| c == car).map_or(0, |p| p + 1);
            (
                car.clone(),
                json!({
                    "Line": line["Line"],
                    "RacingNumber": car,
                    "PersonalBestLapTime": { "Value": line["BestLapTime"]["Value"], "Position": position },
                    "BestSectors": [
                        { "Value": "", "Position": 0 },
                        { "Value": "", "Position": 0 },
                        { "Value": "", "Position": 0 },
                    ],
                }),
            )
        })
        .collect();

    let mut topics = Map::new();

    let event = text(info.get("eventName")).unwrap_or_else(|| source.name.to_owned());
    let session_name = text(info.get("sessionName")).unwrap_or_else(|| "Session".into());
    let track = text(info.get("trackName"))
        .or_else(|| text(bootstrap.pointer("/trackDetails/trackName")))
        .unwrap_or_default();
    let session_type = text(info.get("sessionType")).unwrap_or_default();

    topics.insert(
        "SessionInfo".into(),
        json!({
            "Meeting": {
                "Key": stable_key(&[source.name, &event]),
                "Name": event,
                "OfficialName": event,
                "Location": track,
                "Country": { "Key": 0, "Code": "", "Name": "" },
                "Circuit": { "Key": 0, "ShortName": track },
            },
            "ArchiveStatus": { "Status": "Generating" },
            "Key": sid,
            "Type": if truthy(info.get("isRaceSession")) || session_type.to_ascii_lowercase().contains("race") { "Race" }
                    else if session_type.to_ascii_lowercase().contains("qual") { "Qualifying" } else { "Practice" },
            "Name": session_name,
            "StartDate": text(info.get("startTime")).unwrap_or_default(),
            "EndDate": "",
            "GmtOffset": "00:00:00",
            "Path": "",
        }),
    );

    topics.insert("DriverList".into(), Value::Object(driver_list));
    topics.insert(
        "TimingData".into(),
        json!({ "Lines": timing_lines, "Withheld": false }),
    );
    topics.insert(
        "TimingStats".into(),
        json!({ "Lines": stats_lines, "Withheld": false }),
    );
    if !app_lines.is_empty() {
        topics.insert("TimingAppData".into(), json!({ "Lines": app_lines }));
    }

    let latest_flag = bootstrap
        .get("raceFlags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .max_by_key(|f| timestamp(f.get("ts")))
        .and_then(|f| text(f.get("flag")));
    let (status, message) = track_status(latest_flag.as_deref().unwrap_or("GREEN"));
    topics.insert(
        "TrackStatus".into(),
        json!({ "Status": status, "Message": message }),
    );

    let live = is_live(bootstrap);
    let finished = truthy(info.get("hasSeenChequered"))
        || text(info.get("sessionAvailability")).as_deref() == Some("Closed");
    topics.insert(
        "SessionStatus".into(),
        json!({ "Status": if finished { "Finished" } else if live { "Started" } else { "Inactive" } }),
    );

    let limit = bootstrap
        .pointer("/sessionLengthLimit/timeLimitSeconds")
        .and_then(|v| int(Some(v)));
    let clock = bootstrap.get("sessionClock");
    let elapsed_ms = clock
        .and_then(|c| int(c.get("elapsedTimeMillis")))
        .or_else(|| {
            let start = timestamp(clock.and_then(|c| c.get("startTime")));
            let now = timestamp(clock.and_then(|c| c.get("tsNow")));
            (start > 0 && now >= start).then_some(now - start)
        });
    if let (Some(limit), Some(elapsed)) = (limit.filter(|l| *l > 0), elapsed_ms) {
        topics.insert(
            "ExtrapolatedClock".into(),
            json!({
                "Utc": clock.and_then(|c| text(c.get("tsNow"))).unwrap_or_else(now_utc),
                "Remaining": hms(limit - elapsed / 1000),
                "Extrapolating": live,
            }),
        );
    }

    let laps_limit = bootstrap
        .pointer("/sessionLengthLimit/lapsLimit")
        .and_then(|v| int(Some(v)));
    if leader_laps > 0 {
        let mut lap_count = json!({ "CurrentLap": leader_laps });
        if let Some(total) = laps_limit.filter(|l| *l > 0) {
            lap_count["TotalLaps"] = json!(total);
        }
        topics.insert("LapCount".into(), lap_count);
    }

    if let Some(weather) = bootstrap.get("weather").filter(|w| w.is_object()) {
        let number = |key: &str| weather.get(key).and_then(Value::as_f64);
        topics.insert(
            "WeatherData".into(),
            json!({
                "AirTemp": number("temperature").map(|v| v.to_string()).unwrap_or_default(),
                "TrackTemp": number("trackTemperature").map(|v| v.to_string()).unwrap_or_default(),
                "Humidity": number("humidity").map(|v| v.to_string()).unwrap_or_default(),
                "Pressure": number("pressure").map(|v| v.to_string()).unwrap_or_default(),
                // the dashboard shows m/s, as the F1 feed reports
                "WindSpeed": number("windSpeedKph").map(|v| format!("{:.1}", v / 3.6)).unwrap_or_default(),
                "WindDirection": "",
                "Rainfall": "0",
            }),
        );
    }

    let mut messages: Vec<Value> = bootstrap
        .pointer("/raceLogFirstPage/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let message = text(item.get("text")).or_else(|| text(item.get("flag")))?;
            let mut out = json!({
                "Utc": text(item.get("ts")).unwrap_or_else(now_utc),
                "Lap": int(item.get("lapNumber")).unwrap_or(0),
                "Message": message,
                "Category": text(item.get("type")).unwrap_or_else(|| "Other".into()),
            });
            if let Some(flag) = item.get("flag").and_then(Value::as_str).and_then(rc_flag) {
                out["Flag"] = json!(flag);
                out["Category"] = json!("Flag");
            }
            Some(out)
        })
        .collect();
    messages.sort_by_key(|m| timestamp(m.get("Utc")));
    if messages.len() > MAX_MESSAGES {
        messages.drain(..messages.len() - MAX_MESSAGES);
    }
    topics.insert(
        "RaceControlMessages".into(),
        json!({ "Messages": messages }),
    );

    topics
}

pub async fn run(source: &GriiipSource, sink: &Sink) -> Result<(), Error> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()?;

    let mut publisher = Publisher::default();
    let mut current: Option<i64> = None;
    let mut resolved_at: Option<Instant> = None;

    loop {
        if resolved_at.is_none_or(|at| at.elapsed() > RESOLVE_EVERY) {
            let sid = resolve_session(&client, source).await?;
            resolved_at = Some(Instant::now());

            if sid != current {
                info!(series = source.name, ?sid, "following session");
                current = sid;
                publisher.restart();
            }
        }

        let Some(sid) = current else {
            debug!(series = source.name, "no session");
            tokio::time::sleep(IDLE_POLL).await;
            continue;
        };

        let bootstrap = get_json(
            &client,
            &format!("/api/v2/public/live/session/{sid}/bootstrap?includeViewers=false&includeUnclassifiedRanks=false"),
        )
        .await?;

        publisher
            .publish(sink, topics(source, sid, &bootstrap))
            .await;

        tokio::time::sleep(if is_live(&bootstrap) {
            LIVE_POLL
        } else {
            IDLE_POLL
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bootstrap() -> Value {
        json!({
            "sessionInfo": {
                "eventName": "6 Hours of Spa-Francorchamps", "sessionName": "Race", "sessionType": "Race",
                "trackName": "Circuit de Spa-Francorchamps", "isStarted": true, "isRaceSession": true,
                "sessionAvailability": "Live", "hasSeenChequered": false
            },
            "sessionClock": { "startTime": "2026-05-09T11:00:00Z", "elapsedTimeMillis": 3_600_000, "tsNow": "2026-05-09T12:00:00Z" },
            "sessionLengthLimit": { "timeLimitSeconds": 21_600 },
            "trackDetails": { "trackName": "Spa", "sectorsCount": 3 },
            "participants": [
                { "pid": 1, "carNumber": "7", "classId": "HYPERCAR", "teamName": "Toyota Gazoo Racing", "manufacturer": "Toyota",
                  "currentDriverId": "b", "drivers": [
                    { "displayName": "Driver A", "threeLettersName": "DRA", "externalDriverID": "a" },
                    { "displayName": "Driver B", "threeLettersName": "DRB", "externalDriverID": "b" }
                  ] },
                { "pid": 2, "carNumber": "92", "classId": "LMGT3", "teamName": "Manthey", "drivers": [] },
                { "pid": 3, "carNumber": "70", "classId": "SUPPORT", "teamName": "Other series" }
            ],
            "ranks": [
                { "pid": 1, "carNumber": "7", "classId": "HYPERCAR", "overallPosition": 1, "position": 1 },
                { "pid": 2, "carNumber": "92", "classId": "LMGT3", "overallPosition": 2, "position": 1 },
                { "pid": 3, "carNumber": "70", "classId": "SUPPORT", "overallPosition": 3, "position": 1 }
            ],
            "gaps": [ { "pid": 2, "gapToFirstLaps": 3, "gapToAheadLaps": 3 } ],
            "laps": [
                { "pid": 1, "lapNumber": 30, "lapTimeMillis": 125_400, "color": "Yellow" },
                { "pid": 1, "lapNumber": 31, "lapTimeMillis": 124_100, "color": "Purple" },
                { "pid": 2, "lapNumber": 28, "lapTimeMillis": 138_000, "color": "Green" }
            ],
            "bestLaps": [ { "pid": 1, "lapNumber": 31, "lapTimeMillis": 124_100 }, { "pid": 2, "lapNumber": 12, "lapTimeMillis": 137_500 } ],
            "sectors": [
                { "pid": 1, "sectorNumber": 1, "lapNumber": 31, "sectorTimeMillis": 40_100, "color": "Green" },
                { "pid": 1, "sectorNumber": 1, "lapNumber": 30, "sectorTimeMillis": 41_000 }
            ],
            "runningStatuses": [ { "pid": 2, "status": "OutLap" } ],
            "carLocations": [ { "pid": 1, "carLocation": "Track" }, { "pid": 2, "carLocation": "Pit" } ],
            "pitIns": [ { "pid": 1 }, { "pid": 1 } ],
            "tires": [ { "pid": 1, "tires": [ { "compound": "Medium", "ageInLaps": 12 } ] } ],
            "raceFlags": [ { "flag": "GREEN", "ts": "2026-05-09T11:00:00Z" }, { "flag": "FCY", "ts": "2026-05-09T11:40:00Z" } ],
            "raceLogFirstPage": { "items": [
                { "type": "RaceControl", "text": "FULL COURSE YELLOW", "ts": "2026-05-09T11:40:00Z", "lapNumber": 20 },
                { "type": "Flag", "flag": "GREEN", "ts": "2026-05-09T11:00:00Z" }
            ] },
            "weather": { "temperature": 18.5, "trackTemperature": 27.0, "humidity": 60, "pressure": 980, "windSpeedKph": 18 }
        })
    }

    #[test]
    fn translates_bootstrap_into_f1_topics() {
        let topics = topics(&WEC, 42, &bootstrap());

        let drivers = topics["DriverList"].as_object().unwrap();
        assert_eq!(drivers.len(), 2, "support class left out");
        assert_eq!(drivers["7"]["Tla"], "DRB", "current driver of the crew");
        assert_eq!(drivers["7"]["Class"], "HYPERCAR");
        assert_eq!(drivers["7"]["Crew"], json!(["Driver A", "Driver B"]));

        let car7 = &topics["TimingData"]["Lines"]["7"];
        assert_eq!(car7["Position"], "1");
        assert_eq!(car7["NumberOfLaps"], 31);
        assert_eq!(car7["LastLapTime"]["Value"], "2:04.100");
        assert_eq!(car7["LastLapTime"]["OverallFastest"], true);
        assert_eq!(car7["NumberOfPitStops"], 2);
        assert_eq!(car7["InPit"], false);
        assert_eq!(car7["Sectors"][0]["Value"], "40.100", "latest lap's sector");
        assert_eq!(car7["Sectors"][0]["PersonalFastest"], true);
        assert_eq!(car7["Sectors"].as_array().unwrap().len(), 3);

        let car92 = &topics["TimingData"]["Lines"]["92"];
        assert_eq!(car92["GapToLeader"], "+3 LAPS");
        assert_eq!(car92["ClassPosition"], 1);
        assert_eq!(car92["InPit"], true);
        assert_eq!(car92["PitOut"], true);
        assert_eq!(car92["Retired"], false, "an out lap is not a retirement");

        assert_eq!(
            topics["TimingAppData"]["Lines"]["7"]["Stints"][0]["Compound"],
            "MEDIUM"
        );
        assert_eq!(
            topics["TimingAppData"]["Lines"]["7"]["Stints"][0]["TotalLaps"],
            12
        );

        assert_eq!(topics["TrackStatus"]["Status"], "8", "latest flag wins");
        assert_eq!(topics["SessionStatus"]["Status"], "Started");
        assert_eq!(topics["ExtrapolatedClock"]["Remaining"], "05:00:00");
        assert_eq!(topics["SessionInfo"]["Type"], "Race");
        assert_eq!(topics["SessionInfo"]["Key"], 42);
        assert_eq!(topics["LapCount"]["CurrentLap"], 31);
        assert!(topics["LapCount"].get("TotalLaps").is_none(), "timed race");
        assert_eq!(topics["WeatherData"]["WindSpeed"], "5.0");

        let messages = topics["RaceControlMessages"]["Messages"]
            .as_array()
            .unwrap();
        assert_eq!(messages[0]["Flag"], "GREEN", "oldest first");
        assert_eq!(messages[1]["Message"], "FULL COURSE YELLOW");

        assert_eq!(
            topics["TimingStats"]["Lines"]["7"]["PersonalBestLapTime"]["Position"],
            1
        );
    }

    #[test]
    fn prefers_live_sessions() {
        let sessions = vec![
            json!({ "sid": 1, "seriesName": "FIA World Endurance Championship", "sessionAvailability": "Closed", "connectionStatus": { "lastPublished": "2026-05-09T10:00:00Z" } }),
            json!({ "sid": 2, "seriesName": "FIA World Endurance Championship", "sessionAvailability": "Live", "connectionStatus": { "lastPublished": "2026-05-09T09:00:00Z" } }),
            json!({ "sid": 3, "seriesName": "DTM", "sessionAvailability": "Live" }),
        ];
        assert_eq!(pick_live_session(&WEC, &sessions), Some(2));

        let sessions = vec![
            json!({ "sid": 9, "seriesName": "Other", "sessionClasses": [{ "classId": "LMP2" }], "sessionAvailability": "Closed" }),
        ];
        assert_eq!(
            pick_live_session(&WEC, &sessions),
            Some(9),
            "recognised by class"
        );
    }

    #[test]
    fn falls_back_to_latest_started_session() {
        let now = timestamp(Some(&json!("2026-05-10T12:00:00Z")));
        let sessions = vec![
            json!({ "id": 1, "startTime": "2026-05-09T11:00:00Z" }),
            json!({ "id": 2, "startTime": "2026-05-10T09:00:00Z" }),
            json!({ "id": 3, "startTime": "2026-05-11T09:00:00Z" }),
            json!({ "id": 4, "startTime": "2026-05-10T10:00:00Z", "hideFromUsers": true }),
        ];
        assert_eq!(pick_scheduled_session(&sessions, now), Some(2));

        let mut with_running = sessions.clone();
        with_running
            .push(json!({ "id": 5, "startTime": "2026-05-08T09:00:00Z", "isRunning": true }));
        assert_eq!(pick_scheduled_session(&with_running, now), Some(5));
    }
}
