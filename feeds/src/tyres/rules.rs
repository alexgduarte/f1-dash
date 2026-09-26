//! Tyre allocation and return rules of the FIA Formula 1 Sporting Regulations.
//!
//! 2025: Article 30 (30.2 allocation, 30.5 usage and returns).
//! 2026: Section B, Article B6 (B6.2.4 allocation, B6.3.8 standard events,
//! B6.3.9 sprint events). The dry allocation and return counts are the same in
//! both years.

use serde::Serialize;

use super::{Compound, SessionKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Standard,
    Sprint,
}

impl Format {
    pub fn from_sessions(kinds: &[SessionKind]) -> Self {
        if kinds
            .iter()
            .any(|k| matches!(k, SessionKind::Sprint | SessionKind::SprintQualifying))
        {
            Format::Sprint
        } else {
            Format::Standard
        }
    }

    /// Best guess when the weekend's session list is unknown.
    pub fn guess(current: SessionKind) -> Self {
        Self::from_sessions(&[current])
    }
}

pub struct Allocation {
    pub soft: u8,
    pub medium: u8,
    pub hard: u8,
    pub intermediate: u8,
    pub wet: u8,
}

impl Allocation {
    pub fn sets(&self, compound: Compound) -> u8 {
        match compound {
            Compound::Soft => self.soft,
            Compound::Medium => self.medium,
            Compound::Hard => self.hard,
            Compound::Intermediate => self.intermediate,
            Compound::Wet => self.wet,
        }
    }
}

/// Sets per driver. Soft, medium and hard stand for the softest, middle and
/// hardest of the three specifications nominated for the event.
pub fn allocation(format: Format) -> Allocation {
    match format {
        Format::Standard => Allocation {
            soft: 8,
            medium: 3,
            hard: 2,
            intermediate: 5,
            wet: 2,
        },
        Format::Sprint => Allocation {
            soft: 6,
            medium: 4,
            hard: 2,
            intermediate: 5,
            wet: 2,
        },
    }
}

/// How the sets handed back after a session are chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Return {
    /// Any slick sets, the team's choice.
    Any(u8),
    /// The slick set with the most laps completed in the session (sprint).
    MostLapsInSession,
    /// One set of the softest specification, only for cars that reached Q3.
    Q3Softest,
}

/// Slick returns after a session.
///
/// Standard: two sets within two hours of each of FP1, FP2 and FP3; cars that
/// reached Q3 return one set of the Q3 specification before the race.
/// Sprint: one set after FP1, the most used set of the sprint after the
/// sprint, and three sets after qualifying.
pub fn slick_returns_after(format: Format, kind: SessionKind) -> &'static [Return] {
    match (format, kind) {
        (Format::Standard, SessionKind::Practice) => &[Return::Any(2)],
        (Format::Standard, SessionKind::Qualifying) => &[Return::Q3Softest],
        (Format::Sprint, SessionKind::Practice) => &[Return::Any(1)],
        (Format::Sprint, SessionKind::Sprint) => &[Return::MostLapsInSession],
        (Format::Sprint, SessionKind::Qualifying) => &[Return::Any(3)],
        _ => &[],
    }
}

/// Sets that may not be handed back yet at a standard event: one set of the
/// Q3 specification until Q3, and one set of each harder specification (the
/// mandatory race specifications) until the race. Sprint events protect none.
pub fn protected_before(format: Format, kind: SessionKind) -> &'static [Compound] {
    match (format, kind) {
        (Format::Standard, SessionKind::Practice) => {
            &[Compound::Soft, Compound::Medium, Compound::Hard]
        }
        (Format::Standard, SessionKind::Qualifying) => &[Compound::Medium, Compound::Hard],
        _ => &[],
    }
}

/// At a standard event, one intermediate set is handed back after FP3 when a
/// practice session was declared wet.
pub fn intermediate_return_after_wet_practice(format: Format, kind: SessionKind) -> bool {
    format == Format::Standard && kind == SessionKind::Practice
}
