//! How hard each operation works (docs/design/rigor-dial.md): the design's default rigor and
//! the operations that override it, as the design saves them.
//!
//! The kernel owns what a rigor is and what the dial means (`solidcore_ops::rigor`); the design
//! only stores it and hands it back. Every record keeps what was asked (`set`: the dial and any
//! knob set on its own) and what that resolved to when it was asked (`built`): the operation
//! is built from `built` alone, so a later kernel that maps the dial differently never changes a
//! saved design. Only an edit resolves again.
//!
//! - **The design default** (`rigor.default`). None saved: [`ops::Rigor::LEGACY`], what every
//!   operation did before rigor existed.
//! - **An operation that inherits** has no record of its own: it is built with the default's
//!   `built`. Changing the default rebuilds these.
//! - **An operation that overrides** has a record (`rigor.ops`, by feature id): its own `set`
//!   and `built`. Changing the default leaves these as they are.
//!
//! A design saved before rigor existed has neither, so every operation builds with
//! [`ops::Rigor::LEGACY`]: the same calls, the same faces.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use solidcore_ops::rigor as ops;

use crate::{DocError, Document, Result};

/// What is tested of an operation's operands and required of its result
/// ([`ops::Certify`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certify {
    Off,
    Closed,
    Proved,
}

impl From<Certify> for ops::Certify {
    fn from(c: Certify) -> Self {
        match c {
            Certify::Off => ops::Certify::Off,
            Certify::Closed => ops::Certify::Closed,
            Certify::Proved => ops::Certify::Proved,
        }
    }
}

impl From<ops::Certify> for Certify {
    fn from(c: ops::Certify) -> Self {
        match c {
            ops::Certify::Off => Certify::Off,
            ops::Certify::Closed => Certify::Closed,
            ops::Certify::Proved => Certify::Proved,
        }
    }
}

/// A rigor as asked for: the dial, and the knobs set on their own over it. Nothing set: the
/// legacy rigor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RigorSetting {
    /// The dial, `0..=1` ([`ops::Rigor::dial`]); none: [`ops::Rigor::LEGACY`] under the knobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dial: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certify: Option<Certify>,
    /// Model tolerance, mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    /// Largest chord height of a fit's sampling, mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chord: Option<f64>,
    /// Fewest samples per span of a fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<u32>,
    /// Iteration and subdivision limits, percent of the kernel's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_percent: Option<u32>,
    /// Wall-clock budget, milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<u64>,
}

impl RigorSetting {
    /// The rigor this asks for: the dial's stop (legacy without one), each knob set on its own
    /// in its place.
    pub fn resolve(&self) -> Result<Built> {
        let bad = |what: &str| DocError::Invalid(format!("rigor: {what}"));
        let mut r = match self.dial {
            Some(d) => ops::Rigor::dial(d).map_err(|_| bad("the dial goes from 0 to 1"))?,
            None => ops::Rigor::LEGACY,
        };
        if let Some(c) = self.certify {
            r.certify = c.into();
        }
        if self.tolerance.is_some() {
            r.accuracy.tolerance = self.tolerance;
        }
        if self.chord.is_some() {
            r.accuracy.chord = self.chord;
        }
        if self.samples.is_some() {
            r.accuracy.samples = self.samples;
        }
        if self.effort_percent.is_some() {
            r.effort.percent = self.effort_percent;
        }
        if let Some(ms) = self.time_ms {
            r.time = Some(Duration::from_millis(ms));
        }
        r.check().map_err(|_| bad("a tolerance or chord must be above 0, samples and effort at least 1"))?;
        Ok(Built::from(r))
    }
}

/// A rigor as an operation was built with: every knob, as the kernel took it (`None`: the
/// kernel's own, as before rigor).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Built {
    pub certify: Certify,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chord: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_percent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<u64>,
}

impl From<ops::Rigor> for Built {
    fn from(r: ops::Rigor) -> Self {
        Built {
            certify: r.certify.into(),
            tolerance: r.accuracy.tolerance,
            chord: r.accuracy.chord,
            samples: r.accuracy.samples,
            effort_percent: r.effort.percent,
            time_ms: r.time.map(|t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX)),
        }
    }
}

impl From<Built> for ops::Rigor {
    fn from(b: Built) -> Self {
        ops::Rigor {
            certify: b.certify.into(),
            accuracy: ops::Accuracy { tolerance: b.tolerance, chord: b.chord, samples: b.samples },
            effort: ops::Effort { percent: b.effort_percent },
            time: b.time_ms.map(Duration::from_millis),
        }
    }
}

impl Built {
    /// What every operation was built with before rigor existed.
    pub fn legacy() -> Self {
        Built::from(ops::Rigor::LEGACY)
    }

    pub fn is_legacy(&self) -> bool {
        ops::Rigor::from(*self).is_legacy()
    }
}

/// A rigor record: what was asked, and what it resolved to then.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub set: RigorSetting,
    pub built: Built,
}

impl Record {
    fn of(set: RigorSetting) -> Result<Record> {
        let built = set.resolve()?;
        Ok(Record { set, built })
    }
}

/// The design's rigor: its default, and the operations that override it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rigors {
    /// The design default; none: legacy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Record>,
    /// The operations that override it, by feature id. One not here inherits it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ops: BTreeMap<u64, Record>,
}

impl Rigors {
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.ops.is_empty()
    }

    /// What the default resolved to (legacy with none).
    pub fn default_built(&self) -> Built {
        self.default.as_ref().map_or_else(Built::legacy, |d| d.built)
    }

    /// What feature `id` is built with: its override's, else the default's.
    pub fn built(&self, id: u64) -> Built {
        self.ops.get(&id).map_or_else(|| self.default_built(), |r| r.built)
    }

    /// Whether feature `id` inherits the default.
    pub fn inherits(&self, id: u64) -> bool {
        !self.ops.contains_key(&id)
    }

    /// What feature `id`'s fingerprint takes of its rigor: nothing at legacy, so a design that
    /// records none evaluates, and reuses its saved model, exactly as before.
    pub(crate) fn digest(&self, id: u64) -> Option<String> {
        let b = self.built(id);
        (!b.is_legacy()).then(|| serde_json::to_string(&b).unwrap_or_default())
    }
}

impl Document {
    /// Sets the design default (`None`: legacy) and gives the features it changes the rigor
    /// of: those that inherit it, when what it resolves to changed. The features that override
    /// it keep theirs, and evaluation builds again only from the first that changed.
    pub fn set_rigor_default(&mut self, set: Option<RigorSetting>) -> Result<Vec<u64>> {
        let before = self.rigor.default_built();
        self.rigor.default = set.map(Record::of).transpose()?;
        if self.rigor.default_built() == before {
            return Ok(Vec::new());
        }
        Ok(self.features.iter().map(|f| f.id).filter(|id| self.rigor.inherits(*id)).collect())
    }

    /// Sets feature `id`'s own rigor (`None`: it inherits the default again).
    pub fn set_rigor_override(&mut self, id: u64, set: Option<RigorSetting>) -> Result<()> {
        if self.feature(id).is_none() {
            return Err(DocError::Invalid(format!("no feature {id}")));
        }
        match set {
            Some(s) => {
                self.rigor.ops.insert(id, Record::of(s)?);
            }
            None => {
                self.rigor.ops.remove(&id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
