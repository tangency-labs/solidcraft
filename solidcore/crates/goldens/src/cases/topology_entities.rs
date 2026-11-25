//! A reader for the topology entity runners' input contracts.
//!
//! Several topology runners encode a case into the flat `f64` record the reference shims
//! read (curve, surface and B-rep blocks, scripts of ops), and their generators keep only
//! cases whose record keeps the contract the reference wrappers enforced on it before any
//! shim call. [`Reader`] walks such a record the way those wrappers' readers did; each
//! runner keeps its own block checks. A check returns `None` where the wrapper refused the
//! record.

/// Sequential reader over a flat input record.
pub struct Reader<'a> {
    input: &'a [f64],
    /// Index of the next value to read.
    pub at: usize,
    max_coord: f64,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `input`; [`Reader::finite`] accepts magnitudes up to
    /// `max_coord`.
    #[must_use]
    pub fn new(input: &'a [f64], max_coord: f64) -> Self {
        Self {
            input,
            at: 0,
            max_coord,
        }
    }

    /// The value at `at + k` without reading it.
    #[must_use]
    pub fn peek(&self, k: usize) -> Option<f64> {
        self.input.get(self.at.checked_add(k)?).copied()
    }

    /// The next value.
    pub fn num(&mut self) -> Option<f64> {
        let v = self.input.get(self.at).copied()?;
        self.at += 1;
        Some(v)
    }

    /// The next value, finite and of magnitude at most the reader's bound.
    pub fn finite(&mut self) -> Option<f64> {
        let v = self.num()?;
        (v.is_finite() && v.abs() <= self.max_coord).then_some(v)
    }

    /// The next `k` values, each as [`Reader::finite`].
    pub fn finites(&mut self, k: usize) -> Option<Vec<f64>> {
        (0..k).map(|_| self.finite()).collect()
    }

    /// The next three values, each as [`Reader::finite`].
    pub fn vec3(&mut self) -> Option<[f64; 3]> {
        Some([self.finite()?, self.finite()?, self.finite()?])
    }

    /// [`Reader::vec3`] with squared length at least `1e-12`.
    pub fn nonzero3(&mut self) -> Option<[f64; 3]> {
        let v = self.vec3()?;
        (v.iter().map(|c| c * c).sum::<f64>() >= 1e-12).then_some(v)
    }

    /// The next value as an integer in `0..=max`.
    pub fn index(&mut self, max: usize) -> Option<usize> {
        let v = self.num()?;
        (v.fract() == 0.0 && v >= 0.0 && v <= max as f64).then_some(v as usize)
    }

    /// The next value as an index below `count`.
    pub fn below(&mut self, count: usize) -> Option<usize> {
        let i = self.index(usize::MAX >> 12)?;
        (i < count).then_some(i)
    }

    /// The next value as a flag: `0` or `1`.
    pub fn flag(&mut self) -> Option<bool> {
        Some(self.index(1)? == 1)
    }

    /// Whether the whole record was read.
    #[must_use]
    pub fn done(&self) -> bool {
        self.at == self.input.len()
    }
}

/// The largest magnitude a B-rep block value may have (HANG-29): building a face on a
/// surface with a coordinate above it does not return upstream, and the port spins in the
/// same builds for tens of seconds (nightly fuzz cases).
pub const MAX_BLOCK_MAGNITUDE: f64 = 1.0e150;

/// Whether every value of the B-rep block `block` is at most [`MAX_BLOCK_MAGNITUDE`] in
/// magnitude (`f64::MAX`, the `NOZ` / `NOW` markers, and non-finite values are left to the
/// block's own checks).
#[must_use]
pub fn block_magnitudes_ok(block: &[f64]) -> bool {
    !block.iter().any(|v| {
        let a = v.abs();
        a > MAX_BLOCK_MAGNITUDE && a < f64::MAX
    })
}

/// [`block_magnitudes_ok`] over every number of a case's B-rep source as JSON (the
/// numbers its B-rep block is made of, besides small counts and flags).
#[must_use]
pub fn source_magnitudes_ok(source: &serde_json::Value) -> bool {
    match source {
        serde_json::Value::Number(n) => n.as_f64().is_none_or(|v| block_magnitudes_ok(&[v])),
        serde_json::Value::Array(a) => a.iter().all(source_magnitudes_ok),
        serde_json::Value::Object(o) => o.values().all(source_magnitudes_ok),
        _ => true,
    }
}
