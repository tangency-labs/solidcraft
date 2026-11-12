//! Analysis studies the design keeps: what was asked of a solver, saved so it can be asked
//! again.
//!
//! A study is a question about the part -- how hot does it get, where does the air go, does it
//! bend -- and the answer is only worth as much as the question is repeatable. Before this, a
//! study lived for the length of one command: the parameters went in, a field came out, and
//! nothing was left to change one number and run again. This is the setup, kept with the design
//! and surviving save and reopen.
//!
//! **The setup is the command's own JSON, deliberately.** It would be tidier to spell every
//! boundary condition out as Rust types here, and it would be wrong: `solidcraft_sim`'s command
//! layer already parses and bounds every one of them (`sim.thermal`, `sim.flow`, `sim.stress`),
//! hostile numbers included, and a second set of types would be a second set of rules to keep in
//! step with the first. One parser, one set of rules, and a saved study that cannot drift from
//! the command it runs. What this module guards is the envelope -- how many studies, how long a
//! name, how big and how deep a setup -- because those are the numbers a design file could
//! carry to make opening it expensive.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{DocError, Result};

/// Studies one design may hold. More than this is a design built to be slow to open.
pub const MAX_STUDIES: usize = 64;
/// A study name is a label, not a document.
pub const MAX_STUDY_NAME_BYTES: usize = 128;
/// A setup is a page of conditions, not a mesh: a hundred thousand characters is already far
/// more than the largest sensible one, and small enough that reading it can never be the slow
/// part of opening a design.
pub const MAX_SETUP_BYTES: usize = 100_000;
/// How deeply a setup may nest. The deepest the commands themselves read is a condition's
/// `convection` object inside a `conditions` array inside the setup, which is three.
pub const MAX_SETUP_DEPTH: usize = 8;

/// Which solver a study asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyKind {
    /// Heat conduction through the solid (`sim.thermal`).
    Thermal,
    /// Air or water moving around it (`sim.flow`).
    Flow,
    /// What it does under load (`sim.stress`).
    Stress,
}

impl StudyKind {
    /// The command this study runs. A study and its command are the same thing said twice, so
    /// this is the only place that pairing is written down.
    pub fn command(self) -> &'static str {
        match self {
            StudyKind::Thermal => "sim.thermal",
            StudyKind::Flow => "sim.flow",
            StudyKind::Stress => "sim.stress",
        }
    }

    /// The name a caller gives this kind.
    pub fn id(self) -> &'static str {
        match self {
            StudyKind::Thermal => "thermal",
            StudyKind::Flow => "flow",
            StudyKind::Stress => "stress",
        }
    }

    /// Every kind, in the order a list of them reads best: the two that answer about heat and
    /// air first, since those were built first, then strength.
    pub const ALL: &'static [StudyKind] = &[StudyKind::Thermal, StudyKind::Flow, StudyKind::Stress];

    pub fn from_id(id: &str) -> Option<StudyKind> {
        StudyKind::ALL.iter().copied().find(|k| k.id() == id)
    }
}

/// One saved study: what to ask, and what to ask it of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Study {
    pub kind: StudyKind,
    /// The parameters its command takes, exactly as that command reads them. Empty is a study
    /// that has been started and not yet set up, which is an ordinary state, not an error.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub setup: serde_json::Map<String, Value>,
}

impl Study {
    pub fn new(kind: StudyKind) -> Study {
        Study { kind, setup: serde_json::Map::new() }
    }
}

/// The design's studies, by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Studies {
    pub studies: BTreeMap<String, Study>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UncheckedStudies {
    studies: BTreeMap<String, Study>,
}

impl<'de> Deserialize<'de> for Studies {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Studies, D::Error> {
        let unchecked = UncheckedStudies::deserialize(d)?;
        Studies { studies: unchecked.studies }.checked().map_err(serde::de::Error::custom)
    }
}

impl Studies {
    pub fn is_empty(&self) -> bool {
        self.studies.is_empty()
    }

    /// A name stands for a study in the browser and in every command that takes one, so it has
    /// to be a line of text and nothing else.
    pub fn validate_name(name: &str) -> Result<()> {
        if name.trim().is_empty() || name != name.trim() || name.len() > MAX_STUDY_NAME_BYTES || name.chars().any(char::is_control) {
            return Err(DocError::Invalid(format!(
                "a study's name must be trimmed, not empty, at most {MAX_STUDY_NAME_BYTES} bytes and free of control characters"
            )));
        }
        Ok(())
    }

    /// How deeply a setup nests. Counted rather than recursed into blindly: a design file is
    /// input, and input is hostile until it has been measured.
    fn depth(v: &Value) -> usize {
        match v {
            Value::Array(items) => 1 + items.iter().map(Studies::depth).max().unwrap_or(0),
            Value::Object(fields) => 1 + fields.values().map(Studies::depth).max().unwrap_or(0),
            _ => 0,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.studies.len() > MAX_STUDIES {
            return Err(DocError::Invalid(format!("a design holds at most {MAX_STUDIES} studies")));
        }
        for (name, study) in &self.studies {
            Studies::validate_name(name)?;
            let setup = Value::Object(study.setup.clone());
            let depth = Studies::depth(&setup);
            if depth > MAX_SETUP_DEPTH {
                return Err(DocError::Invalid(format!("study `{name}` nests {depth} deep, past the {MAX_SETUP_DEPTH} a setup may")));
            }
            let size = serde_json::to_string(&setup).map(|s| s.len()).unwrap_or(usize::MAX);
            if size > MAX_SETUP_BYTES {
                return Err(DocError::Invalid(format!("study `{name}` is {size} bytes of setup, past the {MAX_SETUP_BYTES} one may hold")));
            }
        }
        Ok(())
    }

    fn checked(self) -> Result<Studies> {
        self.validate()?;
        Ok(self)
    }
}

#[cfg(test)]
#[path = "analysis_tests.rs"]
mod tests;
