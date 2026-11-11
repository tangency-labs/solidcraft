//! Contract and reply type for Interference (`inspect.interference`, #12874), whose handler is
//! `interference` in `cmd/interference.rs`: the pairs of solid bodies that share volume, and how
//! much, in world placement. It changes nothing.
//!
//! Its hits and its `unchecked` entries have the shapes of Inspect Clearance's `Overlap` and
//! `Unchecked` (`schema/clearance.rs`), which it reuses. The handler reads its parameters from
//! the raw JSON, so each key here reads as its handler reads it and fails with its message.
//! Whether each body exists stays the handler's to check.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::clearance::{Overlap, Unchecked};
use super::config::NameList;
use super::motion::{PartPlace, PartRef};

/// The longest `budget_s` a call takes: a day (`MAX_BUDGET_S` in `cmd/interference.rs`).
const MAX_BUDGET_S: f64 = 86_400.0;

/// Find the pairs of solid bodies that share volume, in world placement, and how much. Pairs are
/// ruled out by their boxes, then by their meshes; the pairs left are intersected exactly,
/// slowest first, within `budget_s`. Mesh bodies aren't intersected: each is listed in
/// `unchecked`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct InterferenceParams {
    /// The bodies to check, by name. Default (or `null`): every body. A name that is no body is
    /// refused.
    #[serde(default)]
    pub bodies: Option<NameList>,
    /// The seconds the exact checks may take, above 0, at most 86400 (a day). Pairs not finished
    /// by then are listed in `unchecked`, and the call returns. Default (or `null`): 240.
    #[serde(default, deserialize_with = "budget_s")]
    #[schemars(schema_with = "budget_s_schema")]
    pub budget_s: Option<f64>,
    /// Pairs left out, such as a pin in its hole: `[a, b]`, each side a body's world name or an
    /// occurrence's id or name, matching every body under it. Default: none.
    #[serde(default)]
    pub exclude_pairs: Option<Vec<[PartRef; 2]>>,
}

/// `budget_s`: seconds above 0, at most a day, or `null`, as the handler reads it (with
/// `params::num`, which takes the number finite and smaller than 1e9 in size), with its message
/// (the check names the key).
fn budget_s<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|s| s.is_finite() && *s > 0.0 && *s <= MAX_BUDGET_S)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("must be a number of seconds above 0, at most {MAX_BUDGET_S}"))),
    }
}

fn budget_s_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The seconds the exact checks may take, above 0, at most 86400 (a day). Pairs not finished by then are listed \
            in `unchecked`, and the call returns. Default (or `null`): 240.",
        "type": ["number", "null"],
        "exclusiveMinimum": 0,
        "maximum": MAX_BUDGET_S,
    })
}

/// The time one pair's exact check took, in ms.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PairTime {
    pub a: String,
    pub b: String,
    pub ms: f64,
    /// The kernel's boolean time within it.
    pub boolean_ms: f64,
    /// The kernel's tessellation time within it.
    pub tessellate_ms: f64,
}

/// A pair that shares volume, and where each body sits in the assembly.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacedOverlap {
    #[serde(flatten)]
    pub overlap: Overlap,
    pub a_part: PartPlace,
    pub b_part: PartPlace,
}

/// A pair or mesh body not checked, and where each body sits in the assembly.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacedUnchecked {
    #[serde(flatten)]
    pub unchecked: Unchecked,
    pub a_part: PartPlace,
    /// `null` for a mesh body.
    pub b_part: Option<PartPlace>,
}

/// What `inspect.interference` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InterferenceResult {
    /// The pairs that share volume, with the volume, its surface area, its box and its centre.
    pub interferences: Vec<PlacedOverlap>,
    /// How many: 0 isn't "no overlap" while `unchecked` lists pairs.
    pub count: usize,
    /// How many solid bodies were checked.
    pub checked: usize,
    /// The pairs that couldn't be checked, with the reason (a boolean that failed, or the time
    /// budget ran out), and each mesh body (`b` is then `null`).
    pub unchecked: Vec<PlacedUnchecked>,
    /// Whether more than 20000 pairs had overlapping boxes, so `skipped_pairs` were left out.
    pub truncated: bool,
    pub skipped_pairs: usize,
    /// The pairs whose boxes overlap.
    pub pairs_candidate: usize,
    /// Of those, the pairs cleared by their meshes, without a boolean.
    pub pairs_cleared: usize,
    /// The pairs intersected exactly.
    pub pairs_intersected: usize,
    /// The pairs not finished within the time budget.
    pub pairs_not_checked: usize,
    /// The pairs left out by `exclude_pairs`.
    pub pairs_excluded: usize,
    /// The time budget, in seconds.
    pub budget_s: f64,
    /// Whether it ran out.
    pub budget_exceeded: bool,
    /// The worker threads the exact checks ran on; 0: on the calling thread.
    pub threads: usize,
    /// The kernel's boolean and tessellation time, summed over the pairs, in ms.
    pub boolean_ms: f64,
    pub tessellate_ms: f64,
    /// The slowest pairs, at most 8, slowest first.
    pub slowest: Vec<PairTime>,
    /// The time the whole call took, in ms.
    pub ms: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "interference_tests.rs"]
mod tests;
