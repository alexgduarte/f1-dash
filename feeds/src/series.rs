use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A championship with a live timing feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Series {
    #[serde(rename = "f1")]
    F1,
    #[serde(rename = "f2")]
    F2,
    #[serde(rename = "f3")]
    F3,
    #[serde(rename = "f1a")]
    F1Academy,
    #[serde(rename = "wec")]
    Wec,
}

impl Series {
    pub const ALL: [Series; 5] = [
        Series::F1,
        Series::F2,
        Series::F3,
        Series::F1Academy,
        Series::Wec,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Series::F1 => "f1",
            Series::F2 => "f2",
            Series::F3 => "f3",
            Series::F1Academy => "f1a",
            Series::Wec => "wec",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Series::F1 => "Formula 1",
            Series::F2 => "FIA Formula 2",
            Series::F3 => "FIA Formula 3",
            Series::F1Academy => "F1 Academy",
            Series::Wec => "FIA World Endurance Championship",
        }
    }
}

impl fmt::Display for Series {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Series {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "f1" | "formula1" => Ok(Series::F1),
            "f2" | "formula2" => Ok(Series::F2),
            "f3" | "formula3" => Ok(Series::F3),
            "f1a" | "f1academy" | "f1-academy" => Ok(Series::F1Academy),
            "wec" => Ok(Series::Wec),
            other => Err(anyhow::anyhow!("unknown series '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ids() {
        for series in Series::ALL {
            assert_eq!(series.id().parse::<Series>().unwrap(), series);
            assert_eq!(
                serde_json::to_string(&series).unwrap(),
                format!("\"{}\"", series.id())
            );
        }
    }

    #[test]
    fn rejects_unknown() {
        assert!("indycar".parse::<Series>().is_err());
    }
}
