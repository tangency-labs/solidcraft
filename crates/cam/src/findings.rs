//! What a simulation finds, two ways: every event at its move and place (for markers in a view
//! and a list to step through), and the events merged by kind (a stable kebab-case code), each
//! with its count and the first lines it happened at (for a summary).
//!
//! Ported from the author's own MIT-licensed G-code simulator: the merging by code, its
//! severities and the cap on the lines kept.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

/// Lines kept per merged finding.
pub const MAX_LINES: usize = 50;
/// Events kept in a log; past it they are counted, not kept.
pub const MAX_EVENTS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A crash, a broken tool or a ruined part.
    Error,
    /// Runs, but probably not as meant.
    Warning,
    /// Worth knowing.
    Info,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// One thing found, at its place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FindingEvent {
    /// Stable kebab-case kind: `rapid-in-stock`, `holder-hits-stock`, `z-plate-hits-vise`, ...
    pub code: String,
    pub severity: Severity,
    /// The move (simulation step) it happened in; `None` for the setup.
    pub at_move: Option<usize>,
    /// The G-code line, from 1 (0: none).
    pub line: usize,
    /// The operation's index, when known.
    pub op: Option<usize>,
    /// Where, work coordinates.
    pub at: Option<Vec3>,
    /// How deep (mm), when it has a depth.
    pub depth: f64,
    pub message: String,
}

/// Every event of one kind, merged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    /// The first event's message.
    pub message: String,
    /// The first lines it happened at (up to [`MAX_LINES`], in the order found, each once).
    pub lines: Vec<usize>,
    pub count: usize,
    /// The deepest of them (mm).
    pub worst: f64,
}

/// Findings merged by code, in the order first found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Findings {
    list: Vec<Finding>,
}

impl Findings {
    pub fn add(&mut self, e: &FindingEvent) {
        if let Some(f) = self.list.iter_mut().find(|f| f.code == e.code) {
            f.count += 1;
            f.worst = f.worst.max(e.depth);
            if e.line > 0 && f.lines.len() < MAX_LINES && !f.lines.contains(&e.line) {
                f.lines.push(e.line);
            }
            return;
        }
        self.list.push(Finding {
            code: e.code.clone(),
            severity: e.severity,
            message: e.message.clone(),
            lines: if e.line > 0 { vec![e.line] } else { Vec::new() },
            count: 1,
            worst: e.depth,
        });
    }

    pub fn merge(&mut self, other: &Findings) {
        for o in &other.list {
            if let Some(f) = self.list.iter_mut().find(|f| f.code == o.code) {
                f.count += o.count;
                f.worst = f.worst.max(o.worst);
                for l in &o.lines {
                    if f.lines.len() < MAX_LINES && !f.lines.contains(l) {
                        f.lines.push(*l);
                    }
                }
            } else {
                self.list.push(o.clone());
            }
        }
    }

    pub fn has(&self, code: &str) -> bool {
        self.list.iter().any(|f| f.code == code)
    }

    /// Errors first, then warnings, then infos; in the order found within each.
    pub fn list(&self) -> Vec<Finding> {
        let mut v = self.list.clone();
        v.sort_by_key(|f| f.severity);
        v
    }

    pub fn count(&self, s: Severity) -> usize {
        self.list.iter().filter(|f| f.severity == s).map(|f| f.count).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// Events in the order found (moves in order), at most [`MAX_EVENTS`]; past it only counted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindingLog {
    events: Vec<FindingEvent>,
    /// Events past the cap, merged.
    dropped: Findings,
}

impl FindingLog {
    pub fn push(&mut self, e: FindingEvent) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(e);
        } else {
            self.dropped.add(&e);
        }
    }

    pub fn events(&self) -> &[FindingEvent] {
        &self.events
    }

    /// Events past the cap, merged.
    pub fn dropped(&self) -> &Findings {
        &self.dropped
    }

    /// The setup's events and those of moves before `end`.
    pub fn until(&self, end: usize) -> impl Iterator<Item = &FindingEvent> {
        self.events.iter().filter(move |e| e.at_move.is_none_or(|m| m < end))
    }

    /// The events of [`FindingLog::until`] merged by code.
    pub fn merged_until(&self, end: usize) -> Findings {
        let mut f = Findings::default();
        for e in self.until(end) {
            f.add(e);
        }
        f
    }

    /// Every event merged by code, those past the cap included.
    pub fn merged(&self) -> Findings {
        let mut f = self.merged_until(usize::MAX);
        f.merge(&self.dropped);
        f
    }

    /// Sorts the events by move (the setup's first), keeping the order of each move's own.
    pub fn sort(&mut self) {
        self.events.sort_by_key(|e| e.at_move.map_or(0, |m| m + 1));
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
