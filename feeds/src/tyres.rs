//! Tyre sets each driver has left over a race weekend.
//!
//! The live feed only reports stints of the running session (`TimingAppData`):
//! the compound, whether the set was new when fitted, its age when fitted
//! (`StartLaps`) and its age now (`TotalLaps`). Tyre age carries over between
//! the sessions of a weekend, so stints from earlier sessions (read from the
//! static archive) can be chained into individual physical sets.
//!
//! The feed does not say which sets a team hands back to the tyre supplier
//! after each session, only the regulations say how many. Returned sets are
//! therefore estimated (see [`rules`]); a set that shows up again in a later
//! session is known not to have been returned, which corrects the estimate as
//! the weekend goes on.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use anyhow::{Context, Error};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

pub mod rules;

use rules::{Format, Return};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Compound {
    Soft,
    Medium,
    Hard,
    Intermediate,
    Wet,
}

impl Compound {
    pub const ALL: [Compound; 5] = [
        Compound::Soft,
        Compound::Medium,
        Compound::Hard,
        Compound::Intermediate,
        Compound::Wet,
    ];

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "SOFT" => Some(Compound::Soft),
            "MEDIUM" => Some(Compound::Medium),
            "HARD" => Some(Compound::Hard),
            "INTERMEDIATE" => Some(Compound::Intermediate),
            "WET" => Some(Compound::Wet),
            _ => None,
        }
    }

    pub fn is_slick(self) -> bool {
        matches!(self, Compound::Soft | Compound::Medium | Compound::Hard)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKind {
    Practice,
    SprintQualifying,
    Sprint,
    Qualifying,
    Race,
}

impl SessionKind {
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();

        if name.starts_with("practice") {
            Some(SessionKind::Practice)
        } else if name == "sprint qualifying" || name == "sprint shootout" {
            Some(SessionKind::SprintQualifying)
        } else if name == "sprint" {
            Some(SessionKind::Sprint)
        } else if name == "qualifying" {
            Some(SessionKind::Qualifying)
        } else if name == "race" {
            Some(SessionKind::Race)
        } else {
            None
        }
    }
}

/// One stint as reported in `TimingAppData.Lines[nr].Stints`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stint {
    pub compound: Option<Compound>,
    /// `New` in the feed: the set had never been used when it was fitted.
    pub new: Option<bool>,
    pub start_laps: u32,
    pub total_laps: u32,
    /// `TyresNotChanged`: the stint continues on the previous stint's set.
    pub tyres_not_changed: bool,
}

fn flag(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_u64().map(|n| n != 0),
        _ => None,
    }
}

fn laps(value: Option<&Value>) -> u32 {
    match value {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

impl Stint {
    fn from_value(value: &Value) -> Self {
        Stint {
            compound: value
                .get("Compound")
                .and_then(Value::as_str)
                .and_then(Compound::parse),
            new: flag(value.get("New")),
            start_laps: laps(value.get("StartLaps")),
            total_laps: laps(value.get("TotalLaps")),
            tyres_not_changed: flag(value.get("TyresNotChanged")).unwrap_or(false),
        }
    }
}

/// Stints may arrive as an array or, when patched, as an index keyed object.
fn stints_of(line: &Value) -> Vec<Stint> {
    match line.get("Stints") {
        Some(Value::Array(items)) => items.iter().map(Stint::from_value).collect(),
        Some(Value::Object(map)) => {
            let mut items: Vec<(usize, &Value)> = map
                .iter()
                .filter_map(|(k, v)| k.parse().ok().map(|i| (i, v)))
                .collect();
            items.sort_by_key(|(i, _)| *i);
            items
                .into_iter()
                .map(|(_, v)| Stint::from_value(v))
                .collect()
        }
        _ => vec![],
    }
}

pub type SessionStints = BTreeMap<String, Vec<Stint>>;

/// Parses the stints of every driver from a `TimingAppData` topic.
pub fn parse_timing_app_data(timing_app_data: &Value) -> SessionStints {
    timing_app_data
        .get("Lines")
        .and_then(Value::as_object)
        .map(|lines| {
            lines
                .iter()
                .map(|(nr, line)| (nr.clone(), stints_of(line)))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
pub struct Session {
    pub name: String,
    pub kind: SessionKind,
}

/// A session that ended earlier in the weekend.
#[derive(Clone, Debug, Default)]
pub struct PastSession {
    pub session: Option<Session>,
    /// `None` when the archive had no timing data for it.
    pub stints: Option<SessionStints>,
    /// Racing numbers of the cars that reached Q3 (qualifying only).
    pub q3: Option<BTreeSet<String>>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct TyreSet {
    pub compound: Compound,
    /// Never driven on.
    pub new: bool,
    /// Laps on the set, including earlier sessions.
    pub laps: u32,
    /// Sessions the set was used in, in order.
    pub sessions: Vec<String>,
    /// The session after which the set was handed back.
    pub returned_after: Option<String>,
    /// Whether the returned set is a guess (teams pick most returned sets
    /// themselves) rather than prescribed by the regulations.
    pub return_estimated: bool,
    /// Fitted to the car in the latest stint of the current session.
    pub fitted: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DriverTyreSets {
    pub sets: Vec<TyreSet>,
    /// Slick sets the regulations leave this driver at this point of the
    /// weekend (allocation minus required returns), when every session could
    /// be read. A different count of available slick sets means the timing
    /// data does not add up for this driver.
    pub expected_slick_sets: Option<u8>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct SessionCoverage {
    pub name: String,
    /// Whether timing data for the session was available.
    pub loaded: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct TyreSets {
    pub format: Format,
    pub allocation: BTreeMap<Compound, u8>,
    /// Sessions that went into the reconstruction, oldest first, the current
    /// one last.
    pub sessions: Vec<SessionCoverage>,
    /// False when earlier sessions of the weekend could not be read, in which
    /// case only the current session is accounted for.
    pub history_loaded: bool,
    pub lines: BTreeMap<String, DriverTyreSets>,
}

struct SetState {
    compound: Compound,
    laps: u32,
    used: bool,
    extra: bool,
    /// Laps completed on the set per session index, in order.
    session_laps: Vec<(usize, u32)>,
    returned_after: Option<usize>,
    return_estimated: bool,
}

impl SetState {
    fn unused(compound: Compound) -> Self {
        SetState {
            compound,
            laps: 0,
            used: false,
            extra: false,
            session_laps: vec![],
            returned_after: None,
            return_estimated: false,
        }
    }

    fn used_after(&self, session: usize) -> bool {
        self.session_laps.iter().any(|&(s, _)| s > session)
    }

    fn laps_in(&self, session: usize) -> u32 {
        self.session_laps
            .iter()
            .filter(|&&(s, _)| s == session)
            .map(|&(_, laps)| laps)
            .sum()
    }

    fn available(&self) -> bool {
        self.returned_after.is_none()
    }
}

fn take_new(sets: &mut Vec<SetState>, compound: Compound) -> usize {
    if let Some(i) = sets
        .iter()
        .position(|s| s.compound == compound && !s.used && s.session_laps.is_empty())
    {
        return i;
    }

    // more new sets than the allocation, e.g. an extra intermediate set
    sets.push(SetState {
        extra: true,
        ..SetState::unused(compound)
    });
    sets.len() - 1
}

/// Finds the set a used-tyre stint was fitted with. Tyre age only goes up, so
/// only sets with at most `start_laps` laps qualify; the oldest of those is
/// the best match (an exact match when every session was read).
fn match_used(sets: &mut Vec<SetState>, compound: Compound, start_laps: u32) -> usize {
    let best = sets
        .iter()
        .enumerate()
        .filter(|(_, s)| s.compound == compound && s.used && s.laps <= start_laps)
        .max_by_key(|(_, s)| s.laps)
        .map(|(i, _)| i);

    if let Some(i) = best {
        return i;
    }

    // no earlier record of this set (a session we could not read); account
    // for it by using up one of the allocated sets
    let i = take_new(sets, compound);
    sets[i].used = true;
    sets[i].laps = start_laps;
    i
}

/// One driver's view of a session.
struct DriverSession<'a> {
    kind: SessionKind,
    stints: Option<&'a Vec<Stint>>,
    reached_q3: bool,
}

/// Chains the stints of every session into sets. Returns the set fitted in
/// the latest stint of the current (last) session.
fn chain_stints(
    sets: &mut Vec<SetState>,
    sessions: &[DriverSession],
    current: bool,
) -> Option<usize> {
    let mut fitted = None;

    for (index, session) in sessions.iter().enumerate() {
        let mut previous: Option<usize> = None;

        for stint in session.stints.into_iter().flatten() {
            let Some(compound) = stint.compound else {
                previous = None;
                continue;
            };

            // `New` is authoritative: a set counts as used once it has left
            // the pit lane, even with zero laps recorded
            let set = match previous {
                Some(prev) if stint.tyres_not_changed && sets[prev].compound == compound => prev,
                _ if stint.new == Some(true) || (stint.new.is_none() && stint.start_laps == 0) => {
                    take_new(sets, compound)
                }
                _ => match_used(sets, compound, stint.start_laps),
            };

            let state = &mut sets[set];
            state.laps = state.laps.max(stint.total_laps);
            state.used |= state.laps > 0 || stint.new == Some(false);
            state
                .session_laps
                .push((index, stint.total_laps.saturating_sub(stint.start_laps)));

            previous = Some(set);
        }

        if current && index == sessions.len() - 1 {
            fitted = previous;
        }
    }

    fitted
}

/// Whether handing back set `i` after a session of `kind` would break a
/// protection: the last unused Q3-specification set before Q3, or the last set
/// of a mandatory race specification before the race.
fn protected(sets: &[SetState], i: usize, format: Format, kind: SessionKind) -> bool {
    let compound = sets[i].compound;

    if !rules::protected_before(format, kind).contains(&compound) {
        return false;
    }

    let same = sets
        .iter()
        .filter(|s| s.compound == compound && s.available());

    if compound == Compound::Soft {
        !sets[i].used && same.filter(|s| !s.used).count() <= 1
    } else {
        same.count() <= 1
    }
}

/// Hands back `count` sets matching `eligible` after session `index`: the most
/// worn first, then unused sets of the most plentiful specification. Never a
/// set that is seen again later.
fn return_most_worn(
    sets: &mut [SetState],
    index: usize,
    count: u8,
    format: Format,
    kind: SessionKind,
    eligible: impl Fn(&SetState) -> bool,
) {
    for _ in 0..count {
        let remaining_unused = |compound: Compound| {
            sets.iter()
                .filter(|s| s.compound == compound && !s.used && s.available())
                .count()
        };

        let candidate = (0..sets.len())
            .filter(|&i| {
                let s = &sets[i];
                eligible(s)
                    && s.available()
                    && !s.used_after(index)
                    && !protected(sets, i, format, kind)
            })
            .max_by_key(|&i| {
                let s = &sets[i];
                (
                    s.used,
                    s.laps,
                    remaining_unused(s.compound),
                    Reverse(s.compound),
                )
            });

        let Some(i) = candidate else { break };

        sets[i].returned_after = Some(index);
        sets[i].return_estimated = true;
    }
}

/// Applies the returns required after every finished session.
fn apply_returns(
    sets: &mut [SetState],
    sessions: &[DriverSession],
    finished: usize,
    format: Format,
    wet_practice: bool,
) {
    let last_practice = sessions[..finished]
        .iter()
        .rposition(|s| s.kind == SessionKind::Practice);

    for (index, session) in sessions[..finished].iter().enumerate() {
        for rule in rules::slick_returns_after(format, session.kind) {
            match *rule {
                Return::Any(count) => {
                    return_most_worn(sets, index, count, format, session.kind, |s| {
                        s.compound.is_slick()
                    })
                }
                Return::Q3Softest if session.reached_q3 => {
                    return_most_worn(sets, index, 1, format, session.kind, |s| {
                        s.compound == Compound::Soft
                    })
                }
                Return::Q3Softest => {}
                Return::MostLapsInSession => {
                    // prescribed by the regulations, so not an estimate
                    let most_used = (0..sets.len())
                        .filter(|&i| sets[i].compound.is_slick() && sets[i].available())
                        .filter(|&i| sets[i].laps_in(index) > 0)
                        .max_by_key(|&i| sets[i].laps_in(index));

                    match most_used {
                        Some(i) => sets[i].returned_after = Some(index),
                        None => return_most_worn(sets, index, 1, format, session.kind, |s| {
                            s.compound.is_slick()
                        }),
                    }
                }
            }
        }

        if wet_practice
            && Some(index) == last_practice
            && rules::intermediate_return_after_wet_practice(format, session.kind)
        {
            return_most_worn(sets, index, 1, format, session.kind, |s| {
                s.compound == Compound::Intermediate
            });
        }
    }
}

/// Allocation minus every return the regulations require after the finished
/// sessions.
fn expected_slick_sets(
    allocation: &rules::Allocation,
    format: Format,
    finished: &[DriverSession],
) -> u8 {
    let allocated = allocation.soft + allocation.medium + allocation.hard;

    let returned: u8 = finished
        .iter()
        .flat_map(|session| {
            rules::slick_returns_after(format, session.kind)
                .iter()
                .map(move |rule| match *rule {
                    Return::Any(count) => count,
                    Return::MostLapsInSession => 1,
                    Return::Q3Softest => u8::from(session.reached_q3),
                })
        })
        .sum();

    allocated.saturating_sub(returned)
}

fn summarise(sets: Vec<SetState>, names: &[&str], fitted: Option<usize>) -> DriverTyreSets {
    let mut out: Vec<TyreSet> = sets
        .into_iter()
        .enumerate()
        // placeholder sets beyond the allocation that were never used carry no information
        .filter(|(_, s)| !s.extra || s.used)
        .map(|(i, s)| {
            let mut sessions: Vec<String> = Vec::new();
            for &(index, _) in &s.session_laps {
                if sessions.last().map(String::as_str) != Some(names[index]) {
                    sessions.push(names[index].to_owned());
                }
            }

            TyreSet {
                compound: s.compound,
                new: !s.used,
                laps: s.laps,
                sessions,
                returned_after: s.returned_after.map(|i| names[i].to_owned()),
                return_estimated: s.return_estimated,
                fitted: Some(i) == fitted,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        (a.returned_after.is_some(), a.compound, !a.new, a.laps).cmp(&(
            b.returned_after.is_some(),
            b.compound,
            !b.new,
            b.laps,
        ))
    });

    DriverTyreSets {
        sets: out,
        expected_slick_sets: None,
    }
}

/// Computes every driver's tyre sets from the finished sessions of the
/// weekend and the stints of the current one.
pub fn compute(
    format: Format,
    past: &[PastSession],
    current: Option<(&Session, &SessionStints)>,
    history_loaded: bool,
) -> TyreSets {
    let allocation = rules::allocation(format);

    type SessionRef<'a> = (
        &'a Session,
        Option<&'a SessionStints>,
        Option<&'a BTreeSet<String>>,
    );

    let mut sessions: Vec<SessionRef> = past
        .iter()
        .filter_map(|p| Some((p.session.as_ref()?, p.stints.as_ref(), p.q3.as_ref())))
        .collect();
    let finished = sessions.len();
    if let Some((session, stints)) = current {
        sessions.push((session, Some(stints), None));
    }

    let names: Vec<&str> = sessions.iter().map(|(s, _, _)| s.name.as_str()).collect();

    // the expected count is only meaningful when no session is missing
    let complete = history_loaded && sessions.iter().all(|(_, stints, _)| stints.is_some());

    // a proxy for "declared wet": someone ran rain tyres in practice
    let wet_practice = sessions[..finished].iter().any(|(session, stints, _)| {
        session.kind == SessionKind::Practice
            && stints.is_some_and(|stints| {
                stints
                    .values()
                    .flatten()
                    .any(|stint| stint.compound.is_some_and(|c| !c.is_slick()))
            })
    });

    // the drivers in the current session; before it starts, everyone seen so far
    let drivers: Vec<String> = match current {
        Some((_, stints)) if !stints.is_empty() => stints.keys().cloned().collect(),
        _ => sessions
            .iter()
            .filter_map(|(_, stints, _)| *stints)
            .flat_map(|stints| stints.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    };

    let lines = drivers
        .into_iter()
        .map(|nr| {
            let driver_sessions: Vec<DriverSession> = sessions
                .iter()
                .map(|(session, stints, q3)| DriverSession {
                    kind: session.kind,
                    stints: stints.and_then(|s| s.get(&nr)),
                    reached_q3: q3.is_some_and(|q3| q3.contains(&nr)),
                })
                .collect();

            let mut sets: Vec<SetState> = Compound::ALL
                .into_iter()
                .flat_map(|c| (0..allocation.sets(c)).map(move |_| SetState::unused(c)))
                .collect();

            let fitted = chain_stints(&mut sets, &driver_sessions, current.is_some());
            apply_returns(&mut sets, &driver_sessions, finished, format, wet_practice);

            let mut summary = summarise(sets, &names, fitted);
            summary.expected_slick_sets = complete
                .then(|| expected_slick_sets(&allocation, format, &driver_sessions[..finished]));

            (nr, summary)
        })
        .collect();

    TyreSets {
        format,
        allocation: Compound::ALL
            .into_iter()
            .map(|c| (c, allocation.sets(c)))
            .collect(),
        sessions: sessions
            .iter()
            .map(|(session, stints, _)| SessionCoverage {
                name: session.name.clone(),
                loaded: stints.is_some(),
            })
            .collect(),
        history_loaded,
        lines,
    }
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone, Debug)]
struct CurrentSession {
    meeting_key: i64,
    key: i64,
    year: String,
    start: String,
    session: Session,
}

impl CurrentSession {
    fn from_info(info: &Value) -> Option<Self> {
        let name = info.get("Name")?.as_str()?;
        let meeting = info.get("Meeting")?;

        // pre-season testing has its own allocation and no returns
        let meeting_name = meeting.get("Name").and_then(Value::as_str).unwrap_or("");
        if meeting_name.to_ascii_lowercase().contains("testing") {
            return None;
        }

        let start = info.get("StartDate")?.as_str()?.to_owned();
        let year = info
            .get("Path")
            .and_then(Value::as_str)
            .and_then(|path| path.get(0..4))
            .or_else(|| start.get(0..4))?
            .to_owned();

        Some(CurrentSession {
            meeting_key: meeting.get("Key")?.as_i64()?,
            key: info.get("Key")?.as_i64()?,
            year,
            start,
            session: Session {
                name: name.to_owned(),
                kind: SessionKind::from_name(name)?,
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct History {
    format: Format,
    past: Vec<PastSession>,
}

enum HistoryState {
    /// No archive for this feed, or the session is not part of a race weekend.
    Unavailable,
    Loading(AbortOnDrop<Result<History, Error>>),
    Loaded(History),
    Failed,
}

/// Keeps the `TyreSets` topic of one live session up to date.
pub struct Tracker {
    current: Option<CurrentSession>,
    history: HistoryState,
    last: Option<TyreSets>,
}

impl Tracker {
    /// Starts fetching the earlier sessions of the weekend in the background.
    pub fn start(archive: Option<&'static str>, session_info: Option<Value>) -> Self {
        let current = session_info.as_ref().and_then(CurrentSession::from_info);

        let history = match (archive, &current) {
            (Some(archive), Some(current)) => HistoryState::Loading(AbortOnDrop(tokio::spawn(
                fetch_history(archive, current.clone()),
            ))),
            _ => HistoryState::Unavailable,
        };

        Tracker {
            current,
            history,
            last: None,
        }
    }

    /// Resolves once the earlier sessions have been fetched or failed to be,
    /// with whether that changes the result; never resolves afterwards.
    pub async fn history_loaded(&mut self) -> bool {
        let HistoryState::Loading(task) = &mut self.history else {
            return std::future::pending().await;
        };

        self.history = match (&mut task.0).await {
            Ok(Ok(history)) => HistoryState::Loaded(history),
            Ok(Err(err)) => {
                warn!(?err, "could not read the earlier sessions of the weekend");
                HistoryState::Failed
            }
            Err(err) => {
                warn!(?err, "tyre history task failed");
                HistoryState::Failed
            }
        };

        true
    }

    /// Recomputes the topic from the current `TimingAppData`. Returns an
    /// update `{ "TyreSets": ... }` when the result changed. Nothing is
    /// published while the weekend's history is still loading, so clients
    /// never see a result that only covers the current session by accident.
    pub fn recompute(&mut self, timing_app_data: Option<&Value>) -> Option<Value> {
        let current = self.current.as_ref()?;

        let (format, past, history_loaded) = match &self.history {
            HistoryState::Loading(_) => return None,
            HistoryState::Loaded(history) => (history.format, history.past.as_slice(), true),
            HistoryState::Unavailable | HistoryState::Failed => {
                (Format::guess(current.session.kind), &[][..], false)
            }
        };

        let stints = timing_app_data
            .map(parse_timing_app_data)
            .unwrap_or_default();

        let sets = compute(
            format,
            past,
            Some((&current.session, &stints)),
            history_loaded,
        );

        if self.last.as_ref() == Some(&sets) {
            return None;
        }

        let value = serde_json::to_value(&sets).ok()?;
        self.last = Some(sets);

        Some(json!({ "TyreSets": value }))
    }
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, Error> {
    let text = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // the archive serves UTF-8 with a byte order mark
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .with_context(|| format!("invalid json at {url}"))
}

/// Past sessions never change, so their stints are fetched once per process.
fn cached_stints() -> &'static std::sync::Mutex<BTreeMap<String, SessionStints>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<BTreeMap<String, SessionStints>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

async fn fetch_stints(client: &reqwest::Client, url: String) -> Option<SessionStints> {
    if let Some(stints) = cached_stints().lock().ok()?.get(&url) {
        return Some(stints.clone());
    }

    match get_json(client, &url).await {
        Ok(value) => {
            let stints = parse_timing_app_data(&value);
            if let Ok(mut cache) = cached_stints().lock() {
                cache.insert(url, stints.clone());
            }
            Some(stints)
        }
        Err(err) => {
            warn!(?err, url, "no timing data for earlier session");
            None
        }
    }
}

/// Q3 is contested by the ten fastest cars, so the final qualifying
/// classification tells who took part.
fn q3_cars(timing_data: &Value) -> BTreeSet<String> {
    timing_data
        .get("Lines")
        .and_then(Value::as_object)
        .map(|lines| {
            lines
                .iter()
                .filter(|(_, line)| {
                    line.get("Position")
                        .and_then(|p| {
                            p.as_str()
                                .and_then(|p| p.parse::<u32>().ok())
                                .or(p.as_u64().map(|p| p as u32))
                        })
                        .is_some_and(|p| (1..=10).contains(&p))
                })
                .map(|(nr, _)| nr.clone())
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_history(archive: &'static str, current: CurrentSession) -> Result<History, Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let index = get_json(&client, &format!("{archive}{}/Index.json", current.year)).await?;

    let meeting = index
        .get("Meetings")
        .and_then(Value::as_array)
        .and_then(|meetings| {
            meetings
                .iter()
                .find(|m| m.get("Key").and_then(Value::as_i64) == Some(current.meeting_key))
        })
        .context("meeting missing from archive index")?;

    let sessions = meeting
        .get("Sessions")
        .and_then(Value::as_array)
        .context("meeting has no sessions")?;

    let kinds: Vec<SessionKind> = sessions
        .iter()
        .filter_map(|s| s.get("Name").and_then(Value::as_str))
        .filter_map(SessionKind::from_name)
        .collect();
    let format = Format::from_sessions(&kinds);

    // earlier sessions of the same weekend; start dates share the event's
    // local time zone, so comparing the ISO strings orders them
    let mut earlier: Vec<(&str, Session, Option<&str>)> = sessions
        .iter()
        .filter(|s| s.get("Key").and_then(Value::as_i64) != Some(current.key))
        .filter_map(|s| {
            let name = s.get("Name")?.as_str()?;
            let start = s.get("StartDate")?.as_str()?;
            let session = Session {
                name: name.to_owned(),
                kind: SessionKind::from_name(name)?,
            };
            Some((start, session, s.get("Path").and_then(Value::as_str)))
        })
        .filter(|(start, _, _)| *start < current.start.as_str())
        .collect();
    earlier.sort_by(|a, b| a.0.cmp(b.0));

    let mut past = Vec::with_capacity(earlier.len());

    for (_, session, path) in earlier {
        let Some(path) = path else {
            past.push(PastSession {
                session: Some(session),
                ..Default::default()
            });
            continue;
        };

        let stints = fetch_stints(&client, format!("{archive}{path}TimingAppData.json")).await;

        // cars that reached Q3 hand back a set of the Q3 specification
        let q3 = if format == Format::Standard && session.kind == SessionKind::Qualifying {
            match get_json(&client, &format!("{archive}{path}TimingData.json")).await {
                Ok(timing) => Some(q3_cars(&timing)),
                Err(err) => {
                    warn!(?err, "no qualifying classification");
                    None
                }
            }
        } else {
            None
        };

        past.push(PastSession {
            session: Some(session),
            stints,
            q3,
        });
    }

    debug!(sessions = past.len(), ?format, "loaded tyre history");

    Ok(History { format, past })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stint(compound: &str, new: bool, start: u32, total: u32) -> Value {
        json!({
            "Compound": compound,
            "New": if new { "true" } else { "false" },
            "TyresNotChanged": "0",
            "StartLaps": start,
            "TotalLaps": total,
        })
    }

    fn session(name: &str) -> Session {
        Session {
            name: name.to_owned(),
            kind: SessionKind::from_name(name).unwrap(),
        }
    }

    fn past(name: &str, stints: Vec<Value>) -> PastSession {
        PastSession {
            session: Some(session(name)),
            stints: Some(parse_timing_app_data(
                &json!({"Lines": {"44": {"Stints": stints}}}),
            )),
            q3: None,
        }
    }

    /// (compound, new, laps) of the sets not handed back.
    fn available(sets: &TyreSets, nr: &str) -> Vec<(Compound, bool, u32)> {
        sets.lines[nr]
            .sets
            .iter()
            .filter(|s| s.returned_after.is_none())
            .map(|s| (s.compound, s.new, s.laps))
            .filter(|(c, _, _)| c.is_slick())
            .collect()
    }

    fn standard_weekend() -> Vec<PastSession> {
        let mut qualifying = past(
            "Qualifying",
            vec![
                stint("SOFT", true, 0, 3),
                stint("SOFT", true, 0, 3),
                stint("SOFT", true, 0, 3),
                stint("SOFT", true, 0, 3),
            ],
        );
        qualifying.q3 = Some(BTreeSet::from(["44".to_owned()]));

        vec![
            past(
                "Practice 1",
                vec![stint("MEDIUM", true, 0, 8), stint("HARD", true, 0, 10)],
            ),
            past(
                "Practice 2",
                vec![
                    stint("SOFT", true, 0, 3),
                    json!({"Compound": "SOFT", "New": "false", "TyresNotChanged": "1", "StartLaps": 3, "TotalLaps": 6}),
                    stint("MEDIUM", true, 0, 15),
                ],
            ),
            past(
                "Practice 3",
                vec![stint("SOFT", true, 0, 4), stint("SOFT", true, 0, 4)],
            ),
            qualifying,
        ]
    }

    #[test]
    fn standard_weekend_leaves_six_sets_for_a_q3_car() {
        let race = session("Race");
        let result = compute(
            Format::Standard,
            &standard_weekend(),
            Some((&race, &SessionStints::new())),
            true,
        );

        // 13 sets, minus 2 after each practice session, minus 1 soft for Q3
        assert_eq!(
            available(&result, "44"),
            vec![
                (Compound::Soft, true, 0),
                (Compound::Soft, false, 3),
                (Compound::Soft, false, 3),
                (Compound::Soft, false, 3),
                (Compound::Medium, true, 0),
                (Compound::Hard, true, 0),
            ]
        );

        let returned: Vec<(Compound, u32, &str)> = result.lines["44"]
            .sets
            .iter()
            .filter_map(|s| Some((s.compound, s.laps, s.returned_after.as_deref()?)))
            .collect();

        assert!(returned.contains(&(Compound::Hard, 10, "Practice 1")));
        assert!(returned.contains(&(Compound::Medium, 8, "Practice 1")));
        assert!(returned.contains(&(Compound::Medium, 15, "Practice 2")));
        assert!(returned.contains(&(Compound::Soft, 6, "Practice 2")));
        assert_eq!(
            returned.iter().filter(|r| r.2 == "Qualifying").count(),
            1,
            "a Q3 car hands back exactly one soft"
        );
        assert!(
            result.lines["44"]
                .sets
                .iter()
                .all(|s| s.returned_after.is_none() || s.return_estimated)
        );
        assert_eq!(result.lines["44"].expected_slick_sets, Some(6));
    }

    #[test]
    fn q1_exit_keeps_the_protected_soft_new() {
        // softs used in every practice session and three new ones in Q1
        let mut qualifying = past(
            "Qualifying",
            vec![
                stint("SOFT", true, 0, 3),
                stint("SOFT", true, 0, 3),
                stint("SOFT", true, 0, 3),
            ],
        );
        qualifying.q3 = Some(BTreeSet::new());

        let weekend = vec![
            past(
                "Practice 1",
                vec![stint("SOFT", true, 0, 6), stint("MEDIUM", true, 0, 12)],
            ),
            past(
                "Practice 2",
                vec![stint("SOFT", true, 0, 4), stint("HARD", true, 0, 15)],
            ),
            past(
                "Practice 3",
                vec![stint("SOFT", true, 0, 5), stint("SOFT", true, 0, 5)],
            ),
            qualifying,
        ];

        let race = session("Race");
        let result = compute(
            Format::Standard,
            &weekend,
            Some((&race, &SessionStints::new())),
            true,
        );
        let available = available(&result, "44");

        assert_eq!(available.len(), 7);
        assert_eq!(result.lines["44"].expected_slick_sets, Some(7));
        assert!(
            available
                .iter()
                .any(|(c, new, _)| *c == Compound::Soft && *new),
            "the set protected for Q3 was never used"
        );
    }

    #[test]
    fn used_race_stint_matches_qualifying_set_and_is_fitted() {
        let race = session("Race");
        let stints = parse_timing_app_data(&json!({"Lines": {"44": {"Stints": [
            {"Compound": "SOFT", "New": "false", "TyresNotChanged": "0", "StartLaps": 3, "TotalLaps": 12}
        ]}}}));

        let result = compute(
            Format::Standard,
            &standard_weekend(),
            Some((&race, &stints)),
            true,
        );
        let sets = &result.lines["44"].sets;

        let fitted: Vec<&TyreSet> = sets.iter().filter(|s| s.fitted).collect();
        assert_eq!(fitted.len(), 1);
        assert_eq!(fitted[0].compound, Compound::Soft);
        assert_eq!(fitted[0].laps, 12);
        assert_eq!(fitted[0].sessions, vec!["Qualifying", "Race"]);
        assert!(fitted[0].returned_after.is_none());

        // still six sets for the race, one of them now in use
        assert_eq!(available(&result, "44").len(), 6);
    }

    #[test]
    fn car_knocked_out_before_q3_keeps_seven_sets() {
        let mut weekend = standard_weekend();
        weekend[3].q3 = Some(BTreeSet::new());

        let race = session("Race");
        let result = compute(
            Format::Standard,
            &weekend,
            Some((&race, &SessionStints::new())),
            true,
        );

        assert_eq!(available(&result, "44").len(), 7);
    }

    #[test]
    fn protects_last_race_specification_sets() {
        // both hards and all mediums worn in practice; one of each must stay
        let weekend = vec![
            past(
                "Practice 1",
                vec![stint("HARD", true, 0, 20), stint("HARD", true, 0, 18)],
            ),
            past(
                "Practice 2",
                vec![
                    stint("MEDIUM", true, 0, 12),
                    stint("MEDIUM", true, 0, 11),
                    stint("MEDIUM", true, 0, 10),
                ],
            ),
        ];

        let fp3 = session("Practice 3");
        let result = compute(
            Format::Standard,
            &weekend,
            Some((&fp3, &SessionStints::new())),
            true,
        );
        let available = available(&result, "44");

        assert_eq!(
            available.iter().filter(|s| s.0 == Compound::Hard).count(),
            1
        );
        assert_eq!(
            available.iter().filter(|s| s.0 == Compound::Medium).count(),
            1
        );
        assert_eq!(available.len(), 13 - 4);
    }

    #[test]
    fn sprint_returns_the_set_with_most_sprint_laps() {
        let weekend = vec![
            past(
                "Practice 1",
                vec![stint("MEDIUM", true, 0, 5), stint("SOFT", true, 0, 3)],
            ),
            past(
                "Sprint Qualifying",
                vec![
                    stint("MEDIUM", true, 0, 3),
                    stint("MEDIUM", true, 0, 3),
                    stint("SOFT", true, 0, 4),
                ],
            ),
            past(
                "Sprint",
                vec![stint("MEDIUM", false, 3, 22), stint("SOFT", false, 3, 5)],
            ),
            past(
                "Qualifying",
                vec![
                    stint("SOFT", true, 0, 3),
                    stint("SOFT", true, 0, 3),
                    stint("SOFT", true, 0, 3),
                ],
            ),
        ];

        let race = session("Race");
        let result = compute(
            Format::Sprint,
            &weekend,
            Some((&race, &SessionStints::new())),
            true,
        );
        let sets = &result.lines["44"].sets;

        let sprint_return: Vec<&TyreSet> = sets
            .iter()
            .filter(|s| s.returned_after.as_deref() == Some("Sprint"))
            .collect();
        assert_eq!(sprint_return.len(), 1);
        assert_eq!(
            (sprint_return[0].compound, sprint_return[0].laps),
            (Compound::Medium, 22)
        );
        assert!(
            !sprint_return[0].return_estimated,
            "prescribed by the regulations"
        );

        // 12 sets minus 1 after FP1, 1 after the sprint and 3 after qualifying
        assert_eq!(available(&result, "44").len(), 7);
        assert_eq!(result.lines["44"].expected_slick_sets, Some(7));
        assert_eq!(
            sets.iter()
                .filter(|s| s.returned_after.as_deref() == Some("Qualifying"))
                .count(),
            3
        );
    }

    #[test]
    fn parses_list_and_patched_stints() {
        let data = json!({"Lines": {
            "1": {"Stints": [{"Compound": "HARD", "New": "true", "TotalLaps": 2, "StartLaps": 0}]},
            "4": {"Stints": {"1": {"Compound": "SOFT", "New": "false", "TotalLaps": "7", "StartLaps": "3"},
                             "0": {"Compound": "UNKNOWN", "New": "true"}}}
        }});

        let parsed = parse_timing_app_data(&data);

        assert_eq!(parsed["1"][0].compound, Some(Compound::Hard));
        assert_eq!(parsed["1"][0].new, Some(true));
        assert_eq!(parsed["4"][0].compound, None);
        assert_eq!(parsed["4"][1].compound, Some(Compound::Soft));
        assert_eq!(
            (parsed["4"][1].start_laps, parsed["4"][1].total_laps),
            (3, 7)
        );
    }

    #[test]
    fn used_set_from_unreadable_session_consumes_an_allocated_set() {
        let weekend = vec![PastSession {
            session: Some(session("Practice 1")),
            stints: None,
            q3: None,
        }];

        let fp2 = session("Practice 2");
        let stints = parse_timing_app_data(&json!({"Lines": {"44": {"Stints": [
            stint("SOFT", false, 5, 9)
        ]}}}));

        let result = compute(Format::Standard, &weekend, Some((&fp2, &stints)), true);
        let softs: Vec<&TyreSet> = result.lines["44"]
            .sets
            .iter()
            .filter(|s| s.compound == Compound::Soft)
            .collect();

        assert_eq!(softs.len(), 8, "no set is invented beyond the allocation");
        assert_eq!(
            result.lines["44"].expected_slick_sets, None,
            "a session is missing"
        );
        assert_eq!(softs.iter().filter(|s| !s.new).count(), 1);
        assert!(
            result
                .sessions
                .iter()
                .any(|s| s.name == "Practice 1" && !s.loaded)
        );
    }

    #[test]
    fn distinct_scrubbed_sets_are_not_merged() {
        // four different sets scrubbed for one lap in a practice session we
        // could not read, one of them run twice (2025 Mexico, car 14)
        let quali = session("Qualifying");
        let stints = parse_timing_app_data(&json!({"Lines": {"14": {"Stints": [
            stint("SOFT", false, 1, 4),
            stint("SOFT", false, 1, 4),
            stint("SOFT", false, 1, 4),
            stint("SOFT", false, 4, 7),
            stint("SOFT", false, 1, 4),
        ]}}}));

        let result = compute(Format::Standard, &[], Some((&quali, &stints)), true);
        let used: Vec<u32> = result.lines["14"]
            .sets
            .iter()
            .filter(|s| s.compound == Compound::Soft && !s.new)
            .map(|s| s.laps)
            .collect();

        assert_eq!(used, vec![4, 4, 4, 7]);
    }

    #[test]
    fn new_false_with_zero_laps_counts_as_used() {
        let fp1 = session("Practice 1");
        let stints = parse_timing_app_data(&json!({"Lines": {"12": {"Stints": [
            stint("MEDIUM", false, 0, 0)
        ]}}}));

        let result = compute(Format::Standard, &[], Some((&fp1, &stints)), true);
        let mediums: Vec<&TyreSet> = result.lines["12"]
            .sets
            .iter()
            .filter(|s| s.compound == Compound::Medium)
            .collect();

        assert_eq!(mediums.iter().filter(|s| !s.new).count(), 1);
        assert!(mediums.iter().any(|s| s.fitted));
    }

    #[test]
    fn wet_practice_returns_one_intermediate_after_last_practice() {
        let weekend = vec![
            past("Practice 1", vec![stint("INTERMEDIATE", true, 0, 9)]),
            past("Practice 2", vec![stint("SOFT", true, 0, 4)]),
            past("Practice 3", vec![stint("SOFT", true, 0, 4)]),
        ];

        let quali = session("Qualifying");
        let result = compute(
            Format::Standard,
            &weekend,
            Some((&quali, &SessionStints::new())),
            true,
        );

        let inters: Vec<&TyreSet> = result.lines["44"]
            .sets
            .iter()
            .filter(|s| s.compound == Compound::Intermediate)
            .collect();

        assert_eq!(inters.len(), 5);
        let returned: Vec<&&TyreSet> = inters
            .iter()
            .filter(|s| s.returned_after.is_some())
            .collect();
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].returned_after.as_deref(), Some("Practice 3"));
        assert_eq!(returned[0].laps, 9);
    }

    #[test]
    fn tracker_skips_testing_and_unknown_sessions() {
        let testing = json!({
            "Key": 1, "Name": "Day 1", "StartDate": "2026-02-26T10:00:00", "Path": "2026/x/",
            "Meeting": {"Key": 2, "Name": "Pre-Season Testing"}
        });
        assert!(CurrentSession::from_info(&testing).is_none());

        let race = json!({
            "Key": 9, "Name": "Race", "StartDate": "2026-07-05T15:00:00",
            "Path": "2026/2026-07-05_British_Grand_Prix/2026-07-05_Race/",
            "Meeting": {"Key": 1300, "Name": "British Grand Prix"}
        });
        let current = CurrentSession::from_info(&race).unwrap();
        assert_eq!(current.year, "2026");
        assert_eq!(current.meeting_key, 1300);
    }

    #[test]
    fn q3_cars_are_the_top_ten() {
        let mut lines = serde_json::Map::new();
        for p in 1..=20 {
            lines.insert(format!("{}", p + 30), json!({"Position": p.to_string()}));
        }
        let q3 = q3_cars(&json!({"Lines": lines}));
        assert_eq!(q3.len(), 10);
        assert!(q3.contains("31") && q3.contains("40") && !q3.contains("41"));
    }
}
