//! The F1 live timing archive of past sessions.
//!
//! Layout: `{year}/Index.json` lists the meetings and their sessions, each
//! session has a `Path` (`2025/2025-07-06_British_Grand_Prix/2025-07-06_Race/`)
//! under which every topic is stored twice: `{Topic}.json` holds its final
//! state and `{Topic}.jsonStream` every update, one per line, prefixed with
//! the time since the stream started (`00:12:34.567{...}`).

use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Error, bail};
use serde::Serialize;
use serde_json::Value;

pub const F1_ARCHIVE: &str = "https://livetiming.formula1.com/static/";

#[derive(Clone, Debug)]
pub enum Archive {
    Http {
        base: String,
        client: reqwest::Client,
    },
    /// A local copy with the same layout, for development and tests.
    Dir(PathBuf),
}

impl Archive {
    pub fn http(base: &str) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("f1-dash/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();

        Archive::Http {
            base: format!("{}/", base.trim_end_matches('/')),
            client,
        }
    }

    /// The official archive, or the local copy `ARCHIVE_DIR` points at.
    pub fn f1() -> Self {
        match env::var_os("ARCHIVE_DIR") {
            Some(dir) => Archive::Dir(PathBuf::from(dir)),
            None => Archive::http(F1_ARCHIVE),
        }
    }

    /// Reads a file of the archive as text, without the byte order mark the
    /// archive puts in front of every file.
    pub async fn text(&self, relative: &str) -> Result<String, Error> {
        check_path(relative)?;

        let text = match self {
            Archive::Http { base, client } => {
                let url = format!("{base}{relative}");
                client
                    .get(&url)
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await
                    .with_context(|| format!("reading {url}"))?
            }
            Archive::Dir(dir) => {
                let path = dir.join(relative);
                tokio::fs::read_to_string(&path)
                    .await
                    .with_context(|| format!("reading {}", path.display()))?
            }
        };

        Ok(text.trim_start_matches('\u{feff}').to_owned())
    }

    pub async fn json(&self, relative: &str) -> Result<Value, Error> {
        let text = self.text(relative).await?;
        serde_json::from_str(&text).with_context(|| format!("invalid json in {relative}"))
    }
}

/// Paths come from clients (a replay request) and must stay inside the archive.
pub fn check_path(relative: &str) -> Result<(), Error> {
    let valid = !relative.is_empty()
        && !relative.starts_with('/')
        && !relative.contains('\\')
        && !relative.contains("://")
        && relative.split('/').all(|part| part != ".." && part != ".")
        && relative
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_-.%+".contains(c));

    if !valid {
        bail!("invalid archive path '{relative}'");
    }

    Ok(())
}

/// Validates a session path such as `2025/2025-07-06_British_Grand_Prix/2025-07-06_Race/`.
pub fn check_session_path(path: &str) -> Result<(), Error> {
    check_path(path)?;

    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    if !path.ends_with('/')
        || parts.len() != 3
        || parts[0].len() != 4
        || !parts[0].chars().all(|c| c.is_ascii_digit())
    {
        bail!("invalid session path '{path}'");
    }

    Ok(())
}

/// Parses a `.jsonStream`: one `HH:MM:SS.mmm{json}` update per line, into
/// (milliseconds since the stream started, update).
pub fn parse_stream(text: &str) -> Vec<(u64, Value)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches('\u{feff}').trim_end();
            let brace = line.find(['{', '['])?;
            let at = parse_offset(&line[..brace])?;
            let data = serde_json::from_str(&line[brace..]).ok()?;
            Some((at, data))
        })
        .collect()
}

fn parse_offset(stamp: &str) -> Option<u64> {
    let mut parts = stamp.trim().split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(hours * 3_600_000 + minutes * 60_000 + (seconds * 1000.0).round() as u64)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSession {
    pub key: i64,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub start: String,
    pub gmt_offset: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveMeeting {
    pub key: i64,
    pub name: String,
    pub location: String,
    pub country: String,
    pub sessions: Vec<ArchiveSession>,
}

/// The meetings of a season with their finished (archived) sessions.
pub fn parse_index(index: &Value) -> Vec<ArchiveMeeting> {
    let text = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };

    index
        .get("Meetings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|meeting| {
            let sessions: Vec<ArchiveSession> = meeting
                .get("Sessions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                // sessions get a path once they are over
                .filter(|s| {
                    s.get("Path")
                        .and_then(Value::as_str)
                        .is_some_and(|p| check_session_path(p).is_ok())
                })
                .map(|s| ArchiveSession {
                    key: s.get("Key").and_then(Value::as_i64).unwrap_or_default(),
                    name: text(s, "Name"),
                    kind: text(s, "Type"),
                    path: text(s, "Path"),
                    start: text(s, "StartDate"),
                    gmt_offset: text(s, "GmtOffset"),
                })
                .collect();

            (!sessions.is_empty()).then(|| ArchiveMeeting {
                key: meeting
                    .get("Key")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                name: text(meeting, "Name"),
                location: text(meeting, "Location"),
                country: meeting
                    .pointer("/Country/Name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                sessions,
            })
        })
        .collect()
}

pub async fn meetings(archive: &Archive, year: i32) -> Result<Vec<ArchiveMeeting>, Error> {
    let index = archive.json(&format!("{year}/Index.json")).await?;
    Ok(parse_index(&index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_streams() {
        let text = "\u{feff}00:00:01.500{\"Status\":\"Started\"}\r\n01:02:03.004{\"Lines\":{}}\r\nnot a line\r\n";

        assert_eq!(
            parse_stream(text),
            vec![
                (1_500, json!({"Status": "Started"})),
                (3_723_004, json!({"Lines": {}}))
            ]
        );
    }

    #[test]
    fn rejects_paths_outside_the_archive() {
        assert!(check_path("2025/Index.json").is_ok());
        assert!(
            check_path("2025/2025-07-06_British_Grand_Prix/2025-07-06_Race/TimingData.jsonStream")
                .is_ok()
        );
        assert!(check_path("../etc/passwd").is_err());
        assert!(check_path("2025/../../x").is_err());
        assert!(check_path("/2025/Index.json").is_err());
        assert!(check_path("http://evil/").is_err());
        assert!(check_path("2025/a b").is_err());

        assert!(check_session_path("2025/2025-07-06_British_Grand_Prix/2025-07-06_Race/").is_ok());
        assert!(check_session_path("2025/2025-07-06_British_Grand_Prix/").is_err());
        assert!(check_session_path("20x5/a/b/").is_err());
    }

    #[test]
    fn lists_archived_sessions() {
        let index = json!({"Year": 2025, "Meetings": [
            {"Key": 1270, "Name": "Mexico City Grand Prix", "Location": "Mexico City", "Country": {"Name": "Mexico"},
             "Sessions": [
                {"Key": 1, "Type": "Qualifying", "Name": "Qualifying", "StartDate": "2025-10-25T15:00:00", "GmtOffset": "-06:00:00",
                 "Path": "2025/2025-10-26_Mexico_City_Grand_Prix/2025-10-25_Qualifying/"},
                {"Key": 2, "Type": "Race", "Name": "Race", "StartDate": "2025-10-26T14:00:00"}
             ]},
            {"Key": 1280, "Name": "Future Grand Prix", "Sessions": [{"Key": 3, "Name": "Practice 1"}]}
        ]});

        let meetings = parse_index(&index);

        assert_eq!(
            meetings.len(),
            1,
            "meetings without archived sessions are left out"
        );
        assert_eq!(meetings[0].country, "Mexico");
        assert_eq!(meetings[0].sessions.len(), 1);
        assert_eq!(meetings[0].sessions[0].kind, "Qualifying");
    }
}
