//! Season schedules for every series, in one shape.
//!
//! - F1: the official calendar subscription (ICS).
//! - F2, F3, F1 Academy: the community maintained sportstimes calendars
//!   (MIT licensed JSON, the data behind f2calendar.com and friends).
//! - WEC: the GriiipLive session list behind the official timing page.

use std::{
    collections::HashMap,
    io::BufReader,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Error};
use chrono::{DateTime, Datelike, NaiveDateTime, TimeZone, Utc};
use ical::parser::ical::component::IcalEvent;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

use crate::Series;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub kind: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Round {
    pub name: String,
    pub country_name: String,
    pub country_key: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub sessions: Vec<Session>,
    pub over: bool,
}

const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

type Cache = Mutex<HashMap<(Series, i32), (Instant, Vec<Round>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The schedule of `series` for `year`, cached for half an hour.
pub async fn schedule(series: Series, year: i32) -> Result<Vec<Round>, Error> {
    if let Some((at, rounds)) = cache()
        .lock()
        .ok()
        .and_then(|c| c.get(&(series, year)).cloned())
        && at.elapsed() < CACHE_TTL
    {
        return Ok(with_over(rounds));
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("f1-dash/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()?;

    let rounds = match series {
        Series::F1 => f1(&client, year).await?,
        Series::F2 => sportstimes(&client, "f2", year).await?,
        Series::F3 => sportstimes(&client, "f3", year).await?,
        Series::F1Academy => sportstimes(&client, "f1-academy", year).await?,
        Series::Wec => griiip(&client, 10, year).await?,
    };

    if let Ok(mut cache) = cache().lock() {
        cache.insert((series, year), (Instant::now(), rounds.clone()));
    }

    Ok(with_over(rounds))
}

/// The first round that has not ended.
pub async fn next(series: Series, year: i32) -> Result<Option<Round>, Error> {
    Ok(schedule(series, year).await?.into_iter().find(|r| !r.over))
}

fn with_over(mut rounds: Vec<Round>) -> Vec<Round> {
    let now = Utc::now();
    for round in &mut rounds {
        round.over = round.end < now;
    }
    rounds
}

fn finish(mut rounds: Vec<Round>) -> Vec<Round> {
    for round in &mut rounds {
        round.sessions.sort_unstable_by_key(|s| s.start);
        if let (Some(first), Some(last)) = (
            round.sessions.iter().map(|s| s.start).min(),
            round.sessions.iter().map(|s| s.end).max(),
        ) {
            round.start = first;
            round.end = last;
        }
    }
    rounds.retain(|r| !r.sessions.is_empty());
    rounds.sort_unstable_by_key(|r| r.start);
    with_over(rounds)
}

// ---------------------------------------------------------------------------
// F1: official calendar subscription

fn parse_ical_utc(date_string: &str) -> Result<DateTime<Utc>, Error> {
    let date_string = date_string.trim();

    // date only: YYYYMMDD
    if date_string.len() == 8 && !date_string.contains('T') {
        let date = chrono::NaiveDate::parse_from_str(date_string, "%Y%m%d")?;
        return Ok(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).context("midnight")?));
    }

    // UTC: YYYYMMDDTHHMMSSZ
    if date_string.ends_with('Z') {
        let time = NaiveDateTime::parse_from_str(date_string, "%Y%m%dT%H%M%SZ")?;
        return Ok(Utc.from_utc_datetime(&time));
    }

    // floating time: YYYYMMDDTHHMMSS
    if date_string.contains('T') {
        let time = NaiveDateTime::parse_from_str(date_string, "%Y%m%dT%H%M%S")?;
        return Ok(Utc.from_utc_datetime(&time));
    }

    Err(anyhow::anyhow!("unrecognized date '{date_string}'"))
}

fn property(event: &IcalEvent, name: &str) -> Option<String> {
    event
        .properties
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.value.clone())
}

fn parse_f1_summary(full_name: &str) -> Option<(String, String)> {
    let regex = Regex::new(r"FORMULA 1 (?P<name>.+?\d{4})\s*(?:-|–)\s*(?P<kind>.+)").ok()?;
    let captures = regex.captures(full_name)?;
    Some((captures["name"].to_owned(), captures["kind"].to_owned()))
}

async fn f1(client: &reqwest::Client, year: i32) -> Result<Vec<Round>, Error> {
    // a subscription link generated on the F1 website with an email address
    let url = "https://ics.ecal.com/ecal-sub/660897ca63f9ca0008bcbea6/Formula%201.ics";
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    parse_f1_calendar(&bytes, year)
}

fn parse_f1_calendar(bytes: &[u8], year: i32) -> Result<Vec<Round>, Error> {
    let mut rounds: Vec<Round> = vec![];

    for calendar in ical::IcalParser::new(BufReader::new(bytes)) {
        for event in calendar?.events {
            let Some((name, kind)) = property(&event, "SUMMARY").and_then(|s| parse_f1_summary(&s))
            else {
                continue;
            };

            let (Some(start), Some(end)) = (property(&event, "DTSTART"), property(&event, "DTEND"))
            else {
                warn!(name, "event without start or end");
                continue;
            };
            let (Ok(start), Ok(end)) = (parse_ical_utc(&start), parse_ical_utc(&end)) else {
                warn!(name, "event with invalid dates");
                continue;
            };

            let session = Session { kind, start, end };

            match rounds.iter_mut().find(|r| r.name == name) {
                Some(round) => round.sessions.push(session),
                None => {
                    if start.year() != year {
                        debug!(name, "filtering round of another year");
                        continue;
                    }

                    rounds.push(Round {
                        name,
                        country_name: property(&event, "LOCATION").unwrap_or_default(),
                        country_key: None,
                        start,
                        end,
                        sessions: vec![session],
                        over: false,
                    });
                }
            }
        }
    }

    Ok(finish(rounds))
}

// ---------------------------------------------------------------------------
// F2, F3, F1 Academy: sportstimes calendars

const SPORTSTIMES: &str = "https://raw.githubusercontent.com/sportstimes/f1/main/_db";

/// "feature" -> "Feature Race", "qualifying1" -> "Qualifying 1", "fp2" -> "Practice 2".
fn session_kind(key: &str) -> String {
    let (word, number) = match key.find(|c: char| c.is_ascii_digit()) {
        Some(i) => (&key[..i], Some(&key[i..])),
        None => (key, None),
    };

    let word = match word {
        "fp" | "practice" => "Practice".to_owned(),
        "feature" => "Feature Race".to_owned(),
        "sprint" => "Sprint Race".to_owned(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    };

    match number {
        Some(n) => format!("{word} {n}"),
        None => word,
    }
}

fn parse_sportstimes(calendar: &Value, config: &Value) -> Vec<Round> {
    let lengths = config.get("sessionLengths").and_then(Value::as_object);

    let rounds = calendar
        .get("races")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|race| {
            let sessions: Vec<Session> = race
                .get("sessions")?
                .as_object()?
                .iter()
                .filter_map(|(key, start)| {
                    let start = DateTime::parse_from_rfc3339(start.as_str()?)
                        .ok()?
                        .with_timezone(&Utc);
                    let minutes = lengths
                        .and_then(|l| l.get(key))
                        .and_then(Value::as_i64)
                        .unwrap_or(60);
                    Some(Session {
                        kind: session_kind(key),
                        start,
                        end: start + chrono::Duration::minutes(minutes),
                    })
                })
                .collect();

            let first = sessions.iter().map(|s| s.start).min()?;
            let name = race.get("name").and_then(Value::as_str).unwrap_or_default();
            let location = race.get("location").and_then(Value::as_str).unwrap_or(name);

            Some(Round {
                name: format!("{} {}", location, first.year()),
                country_name: location.to_owned(),
                country_key: None,
                start: first,
                end: first,
                sessions,
                over: false,
            })
        })
        .collect();

    finish(rounds)
}

async fn sportstimes(client: &reqwest::Client, site: &str, year: i32) -> Result<Vec<Round>, Error> {
    let get = |path: String| async move {
        client
            .get(path)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
            .map_err(Error::from)
    };

    let calendar = get(format!("{SPORTSTIMES}/{site}/{year}.json")).await?;
    let config = get(format!("{SPORTSTIMES}/{site}/config.json"))
        .await
        .unwrap_or(Value::Null);

    Ok(parse_sportstimes(&calendar, &config))
}

// ---------------------------------------------------------------------------
// WEC: GriiipLive sessions

fn parse_griiip(sessions: &[Value]) -> Vec<Round> {
    let mut rounds: Vec<Round> = vec![];

    for item in sessions {
        if item.get("hideFromUsers").and_then(Value::as_bool) == Some(true) {
            continue;
        }

        let Some(start) = item
            .get("startTime")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|s| s.with_timezone(&Utc))
        else {
            continue;
        };

        // the session list has no scheduled end, so assume an hour unless given
        let end = item
            .get("endTime")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|s| s.with_timezone(&Utc))
            .filter(|end| *end > start)
            .unwrap_or(start + chrono::Duration::hours(1));

        let event = item
            .pointer("/event/name")
            .and_then(Value::as_str)
            .unwrap_or("WEC");
        let venue = item
            .pointer("/event/trackConfig/track/name")
            .and_then(Value::as_str)
            .unwrap_or(event);
        let session = Session {
            kind: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Session")
                .to_owned(),
            start,
            end,
        };

        match rounds.iter_mut().find(|r| r.name == event) {
            Some(round) => round.sessions.push(session),
            None => rounds.push(Round {
                name: event.to_owned(),
                country_name: venue.to_owned(),
                country_key: None,
                start,
                end,
                sessions: vec![session],
                over: false,
            }),
        }
    }

    finish(rounds)
}

async fn griiip(client: &reqwest::Client, series_id: u32, year: i32) -> Result<Vec<Round>, Error> {
    let url = format!(
        "https://insights.griiip.com/meta/sessions?dateTime={year}-01-01T00%3A00%3A00Z&forward=true&seriesIds={series_id}"
    );
    let sessions: Value = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(
        parse_griiip(sessions.as_array().map(Vec::as_slice).unwrap_or_default())
            .into_iter()
            .filter(|r| r.start.year() == year)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_sessions() {
        assert_eq!(session_kind("feature"), "Feature Race");
        assert_eq!(session_kind("sprint"), "Sprint Race");
        assert_eq!(session_kind("practice"), "Practice");
        assert_eq!(session_kind("fp2"), "Practice 2");
        assert_eq!(session_kind("qualifying1"), "Qualifying 1");
        assert_eq!(session_kind("race3"), "Race 3");
    }

    #[test]
    fn parses_sportstimes_calendar() {
        let calendar = json!({ "races": [
            { "name": "Monaco", "location": "Monte Carlo", "round": 4, "sessions": {
                "practice": "2026-06-04T13:00:00Z", "qualifying": "2026-06-05T13:10:00Z",
                "sprint": "2026-06-06T14:15:00Z", "feature": "2026-06-07T09:40:00Z" } },
            { "name": "Australian", "location": "Melbourne", "round": 1, "sessions": {
                "practice": "2026-03-05T23:00:00Z", "feature": "2026-03-08T00:25:00Z" } }
        ]});
        let config = json!({ "sessionLengths": { "practice": 45, "qualifying": 30, "sprint": 45, "feature": 60 } });

        let rounds = parse_sportstimes(&calendar, &config);

        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].country_name, "Melbourne", "sorted by start");
        let monaco = &rounds[1];
        assert_eq!(monaco.name, "Monte Carlo 2026");
        assert_eq!(monaco.sessions.first().unwrap().kind, "Practice");
        assert_eq!(monaco.sessions.last().unwrap().kind, "Feature Race");
        assert_eq!(
            monaco.end,
            DateTime::parse_from_rfc3339("2026-06-07T10:40:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn groups_griiip_sessions_by_event() {
        let sessions = vec![
            json!({ "name": "Race", "startTime": "2026-05-09T11:00:00Z", "endTime": "2026-05-09T17:00:00Z",
                    "event": { "name": "6 Hours of Spa", "trackConfig": { "track": { "name": "Spa-Francorchamps" } } } }),
            json!({ "name": "Free Practice 1", "startTime": "2026-05-07T10:00:00Z",
                    "event": { "name": "6 Hours of Spa" } }),
            json!({ "name": "Hidden", "startTime": "2026-05-07T08:00:00Z", "hideFromUsers": true,
                    "event": { "name": "6 Hours of Spa" } }),
        ];

        let rounds = parse_griiip(&sessions);

        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].sessions.len(), 2);
        assert_eq!(rounds[0].country_name, "Spa-Francorchamps");
        assert_eq!(rounds[0].sessions[0].kind, "Free Practice 1");
        assert_eq!(rounds[0].end.to_rfc3339(), "2026-05-09T17:00:00+00:00");
    }

    #[test]
    fn parses_f1_ics() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nSUMMARY:FORMULA 1 BRITISH GRAND PRIX 2026 - Race\r\nLOCATION:Great Britain\r\nDTSTART:20260705T140000Z\r\nDTEND:20260705T160000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nSUMMARY:FORMULA 1 BRITISH GRAND PRIX 2026 - Practice 1\r\nLOCATION:Great Britain\r\nDTSTART:20260703T113000Z\r\nDTEND:20260703T123000Z\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

        let rounds = parse_f1_calendar(ics.as_bytes(), 2026).unwrap();

        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].name, "BRITISH GRAND PRIX 2026");
        assert_eq!(rounds[0].country_name, "Great Britain");
        assert_eq!(rounds[0].sessions[0].kind, "Practice 1");
        assert_eq!(rounds[0].start.to_rfc3339(), "2026-07-03T11:30:00+00:00");
    }
}
