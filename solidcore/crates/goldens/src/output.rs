//! Structured result of running one kernel case.

use std::collections::BTreeMap;

use solidcore::SolidError;

/// What the runner produced for a case.
///
/// * `values`: floating-point arrays, compared with the 1e-9 relative rule.
/// * `ints`: integer arrays (indices, counts, flags), compared exactly.
/// * `topology`: compared exactly (same counts and order).
/// * `error`: the operation's status: `None` is success, otherwise a normalised
///   error string (upstream `SmStatus` on the C++ side, `SolidError` mapped the
///   same way on the Rust side). It is always compared: if either side errors,
///   both must report the same string (values are then not compared). A runner
///   must never turn a Rust `Err` into an empty or default output (see
///   [`Output::from_result`]); a pass that compared nothing but this status is
///   counted as *vacuous* in the summary line.
/// * `deferred`: set only by the Rust side (via [`Output::deferred`] or
///   [`Output::unported`]) when the port reached code that is not ported yet
///   ([`SolidError::Unported`]). The case is then *deferred*: neither pass nor
///   fail, and nothing is compared (docs/porting.md §4a).
/// * `rejected`: the runner refused fixture construction or an input precondition
///   before the operation under test ran. The golden stores the exact reason and
///   compares it exactly: a rejection is never an empty success, and an empty
///   reason is still a rejection. Some legacy reason strings describe upstream
///   guards; they do not mean the C++ was run.
/// * `diagnostics`: named counts the Rust side reports for the record (e.g. arena
///   records no walk reaches); written to the report, never compared.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    /// Named float arrays.
    pub values: BTreeMap<String, Vec<f64>>,
    /// Named integer arrays.
    pub ints: BTreeMap<String, Vec<i64>>,
    /// Optional topology record.
    pub topology: Option<Topology>,
    /// Normalised error (e.g. `"nl_error"`), if the operation failed.
    pub error: Option<String>,
    /// The Rust side hit an unported callee; the case is deferred.
    pub deferred: Option<Deferred>,
    /// The exact reason fixture construction was refused before the operation ran;
    /// recorded in the golden and compared exactly. `None` means no rejection was reported.
    pub rejected: Option<String>,
    /// Rust-side diagnostics for the report (never compared); boxed, as most outputs
    /// have none.
    pub diagnostics: Option<Box<BTreeMap<String, i64>>>,
}

/// The exact bits of a computed `x` for an `ints` record: a NaN is recorded as the one quiet
/// NaN `f64::NAN`, because IEEE 754 leaves the sign and payload of a NaN an operation
/// produces to the hardware (x86-64 gives a negative NaN where aarch64 gives a positive one).
/// Every other value, signed zeros and infinities included, keeps its bits. Values that are
/// only copied (read, written, byte-swapped) keep their raw bits, payloads included.
#[must_use]
pub fn float_bits(x: f64) -> i64 {
    let x = if x.is_nan() { f64::NAN } else { x };
    x.to_bits() as i64
}

/// Why a case is deferred: the unported upstream callee and the unit that will port it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deferred {
    /// Plan unit id that blocks the case (e.g. `geom.curve`).
    pub unit: String,
    /// Upstream C++ symbol that is not ported yet.
    pub symbol: String,
}

impl Output {
    /// Adds a Rust-side diagnostic count (written to the report, never compared).
    #[must_use]
    pub fn diagnostic(mut self, key: &str, v: i64) -> Self {
        self.diagnostics
            .get_or_insert_with(Box::default)
            .insert(key.to_string(), v);
        self
    }

    /// The diagnostic `key`, if the Rust side reported one.
    pub fn diagnostic_value(&self, key: &str) -> Option<i64> {
        self.diagnostics.as_ref().and_then(|d| d.get(key).copied())
    }

    /// Empty output.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fixture construction or an input precondition was refused before the operation ran.
    /// The reason is recorded in the golden, separately from an operation error.
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            rejected: Some(reason.into()),
            ..Self::default()
        }
    }

    /// Output representing a failed operation.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            error: Some(msg.into()),
            ..Self::default()
        }
    }

    /// Output of a Rust side that could not finish because `symbol` (owned by plan
    /// unit `unit`) is not ported yet: the case is counted as deferred.
    pub fn deferred(unit: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self {
            deferred: Some(Deferred {
                unit: unit.into(),
                symbol: symbol.into(),
            }),
            ..Self::default()
        }
    }

    /// `Some(Output::deferred(..))` if `e` is [`SolidError::Unported`], else `None`.
    /// Runners call this first when mapping a Rust error:
    /// `Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))`.
    pub fn unported(e: &SolidError) -> Option<Self> {
        match e {
            SolidError::Unported { upstream, unit } => Some(Self::deferred(*unit, *upstream)),
            _ => None,
        }
    }

    /// Maps a Rust-side result to an output so that the operation's status is always
    /// compared: `Ok(v)` becomes `ok(v)`; `Unported` defers the case; any other error
    /// becomes [`Output::err`]`(normalise(&e))`, which the C++ side must match with the
    /// same normalised status (a C++ success against a Rust error fails, even when both
    /// outputs would otherwise be empty). Never map an `Err` to an empty or default
    /// output instead: that turns a Rust failure into a vacuous pass.
    pub fn from_result<T>(
        r: solidcore::Result<T>,
        normalise: impl FnOnce(&SolidError) -> String,
        ok: impl FnOnce(T) -> Self,
    ) -> Self {
        match r {
            Ok(v) => ok(v),
            Err(e) => Self::unported(&e).unwrap_or_else(|| Self::err(normalise(&e))),
        }
    }

    /// Adds a float array.
    #[must_use]
    pub fn values(mut self, key: &str, v: impl Into<Vec<f64>>) -> Self {
        self.values.insert(key.to_string(), v.into());
        self
    }

    /// Adds a single float.
    #[must_use]
    pub fn scalar(self, key: &str, v: f64) -> Self {
        self.values(key, vec![v])
    }

    /// Adds a list of points / vectors, flattened (`[x0,y0,z0,x1,...]`).
    #[must_use]
    pub fn points<const N: usize>(self, key: &str, pts: &[[f64; N]]) -> Self {
        let flat: Vec<f64> = pts.iter().flat_map(|p| p.iter().copied()).collect();
        self.values(key, flat)
    }

    /// Adds an integer array.
    #[must_use]
    pub fn ints(mut self, key: &str, v: impl Into<Vec<i64>>) -> Self {
        self.ints.insert(key.to_string(), v.into());
        self
    }

    /// Adds a single integer.
    #[must_use]
    pub fn int(self, key: &str, v: i64) -> Self {
        self.ints(key, vec![v])
    }

    /// Sets the topology record.
    #[must_use]
    pub fn topology(mut self, t: Topology) -> Self {
        self.topology = Some(t);
        self
    }
}

/// Topology record, compared exactly.
///
/// Ids must be *traversal-order indices* (0, 1, 2, ... in the order upstream
/// iterates its lists), never pointers or arena ids, so both sides agree.
///
/// Conventional entity keys: `shells`, `faces`, `loops`, `coedges`, `edges`,
/// `vertices`. Conventional adjacency keys: `face_loops`, `loop_coedges`,
/// `coedge_edge`, `edge_vertices`, `edge_faces`, ...
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topology {
    /// Ordered entity id lists (the length is the count).
    pub entities: BTreeMap<String, Vec<i64>>,
    /// Ordered adjacency lists: `adjacency[key][i]` = neighbours of entity `i`.
    pub adjacency: BTreeMap<String, Vec<Vec<i64>>>,
}

impl Topology {
    /// Empty record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entity list.
    #[must_use]
    pub fn entities(mut self, key: &str, ids: impl Into<Vec<i64>>) -> Self {
        self.entities.insert(key.to_string(), ids.into());
        self
    }

    /// Adds `0..n` as the entity list (when only the count matters).
    #[must_use]
    pub fn count(self, key: &str, n: usize) -> Self {
        let ids: Vec<i64> = (0..n as i64).collect();
        self.entities(key, ids)
    }

    /// Adds an adjacency list.
    #[must_use]
    pub fn adjacency(mut self, key: &str, adj: Vec<Vec<i64>>) -> Self {
        self.adjacency.insert(key.to_string(), adj);
        self
    }
}

/// The status of an [`SolidError::Upstream`], or of a typed blend failure that replaced one
/// (solidcore's `BlendFailure`, #13790), as the blend runners read a status: other errors,
/// including other areas' typed failures, give `None`, as they did before the blend sites
/// were converted, so a runner's rows stay as they were.
#[must_use]
pub fn blend_status(e: &SolidError) -> Option<i32> {
    match e {
        SolidError::Upstream { code, .. } => Some(*code),
        e if e.blend_failure().is_some() => solidcore::error::upstream_status(e),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::float_bits;

    #[test]
    fn float_bits_records_one_nan_and_keeps_every_other_value() {
        let negative_nan = f64::from_bits(0xfff8_0000_0000_0000);
        let payload_nan = f64::from_bits(0x7ff0_0000_0000_0001);
        assert_eq!(float_bits(negative_nan), float_bits(f64::NAN));
        assert_eq!(float_bits(payload_nan), float_bits(f64::NAN));
        assert_ne!(float_bits(0.0), float_bits(-0.0));
        assert_eq!(
            float_bits(f64::NEG_INFINITY),
            f64::NEG_INFINITY.to_bits() as i64
        );
        assert_eq!(float_bits(1.5), 1.5_f64.to_bits() as i64);
    }
}
