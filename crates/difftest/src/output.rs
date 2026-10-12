//! Structured result of running one case on one side (C++ or Rust).

use std::collections::BTreeMap;

/// What one side produced for a case. Both sides must fill the *same* keys.
///
/// * `values`: floating-point arrays, compared with the 1e-9 relative rule.
/// * `ints`: integer arrays (indices, counts, flags), compared exactly.
/// * `topology`: compared exactly (same counts and order).
/// * `error`: a normalised error string; if either side errors, both must
///   report the same string (values are then not compared).
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
}

impl Output {
    /// Empty output.
    pub fn new() -> Self {
        Self::default()
    }

    /// Output representing a failed operation.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            error: Some(msg.into()),
            ..Self::default()
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
