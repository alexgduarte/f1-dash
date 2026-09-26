//! Helpers for adapters that build F1-shaped topics from another feed.

use serde_json::{Map, Value, json};

use crate::Sink;

/// Publishes whole topics, sending only the ones that changed since the last
/// call. The first call (and the first after [`Publisher::restart`]) replaces
/// the state, which clients treat as a new session.
#[derive(Default)]
pub struct Publisher {
    last: Option<Map<String, Value>>,
}

impl Publisher {
    pub async fn publish(&mut self, sink: &Sink, topics: Map<String, Value>) {
        let Some(last) = &mut self.last else {
            sink.reset(Value::Object(topics.clone())).await;
            self.last = Some(topics);
            return;
        };

        for (topic, value) in topics {
            if last.get(&topic) != Some(&value) {
                sink.update(json!({ topic.clone(): value.clone() })).await;
                last.insert(topic, value);
            }
        }
    }

    /// The next publish starts a fresh state.
    pub fn restart(&mut self) {
        self.last = None;
    }
}

pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Reads a string or number as text.
pub fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Feeds encode flags as booleans, 0/1 or "0"/"1".
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => matches!(s.to_ascii_lowercase().as_str(), "1" | "true"),
        _ => false,
    }
}

pub fn int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Parses "1:21.557" or "33.2" into milliseconds.
pub fn lap_time_millis(value: &str) -> Option<u64> {
    let value = value.trim();
    let (minutes, seconds) = match value.rsplit_once(':') {
        Some((minutes, seconds)) => (minutes.parse::<u64>().ok()?, seconds),
        None => (0, value),
    };
    let seconds: f64 = seconds.parse().ok()?;
    Some(minutes * 60_000 + (seconds * 1000.0).round() as u64)
}

/// Formats milliseconds the way the F1 feed does ("1:21.557", "33.200").
pub fn format_lap_time(millis: u64) -> String {
    let minutes = millis / 60_000;
    let seconds = (millis % 60_000) as f64 / 1000.0;
    if minutes > 0 {
        format!("{minutes}:{seconds:06.3}")
    } else {
        format!("{seconds:.3}")
    }
}

/// A stable positive key for identifiers that have no numeric id.
pub fn stable_key(parts: &[&str]) -> i64 {
    // FNV-1a
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in parts.join("|").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash >> 1) as i64
}

/// Colours for feeds without team colours, picked by car number so a car keeps
/// its colour for the whole session.
pub fn fallback_colour(car: &str) -> &'static str {
    const PALETTE: [&str; 12] = [
        "3B82F6", "EF4444", "10B981", "F59E0B", "8B5CF6", "EC4899", "14B8A6", "F97316", "6366F1",
        "84CC16", "06B6D4", "A855F7",
    ];
    let index = car.bytes().fold(0usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    });
    PALETTE[index % PALETTE.len()]
}

/// Splits "Joshua DURKSEN" into ("Joshua", "DURKSEN").
pub fn split_name(full: &str) -> (String, String) {
    match full.trim().rsplit_once(' ') {
        Some((first, last)) => (first.to_owned(), last.to_owned()),
        None => (String::new(), full.trim().to_owned()),
    }
}

/// Three letter code from a surname when the feed has none.
pub fn tla_from(name: &str, fallback: &str) -> String {
    let code: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(3)
        .collect::<String>()
        .to_uppercase();
    if code.is_empty() {
        fallback.to_owned()
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_lap_times() {
        assert_eq!(lap_time_millis("1:21.557"), Some(81_557));
        assert_eq!(lap_time_millis("33.2"), Some(33_200));
        assert_eq!(lap_time_millis(""), None);
        assert_eq!(format_lap_time(81_557), "1:21.557");
        assert_eq!(format_lap_time(33_200), "33.200");
        assert_eq!(format_lap_time(3_725_001), "62:05.001");
    }

    #[test]
    fn reads_flags() {
        assert!(truthy(Some(&json!(1))));
        assert!(truthy(Some(&json!("1"))));
        assert!(truthy(Some(&json!(true))));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(None));
    }

    #[test]
    fn stable_keys_are_stable_and_positive() {
        assert_eq!(stable_key(&["a", "b"]), stable_key(&["a", "b"]));
        assert_ne!(stable_key(&["a", "b"]), stable_key(&["a", "c"]));
        assert!(stable_key(&["x"]) >= 0);
    }

    #[test]
    fn splits_names() {
        assert_eq!(
            split_name("Joshua DURKSEN"),
            ("Joshua".into(), "DURKSEN".into())
        );
        assert_eq!(split_name("Mono"), ("".into(), "Mono".into()));
        assert_eq!(tla_from("DURKSEN", "2"), "DUR");
    }
}
