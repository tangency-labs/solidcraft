//! A memo for `SmBSplineCurve::AssertValid`'s two global property analyses (not
//! upstream, tangency-labs/solidcore#2568): the zero-speed search (tests 11/12,
//! `MINIMIZE_FIRST_DERIVATIVE`) and the kink search (tests 13/14,
//! `MINIMIZE_RADIUS_OF_CURVATURE`). It never changes a result.
//!
//! - [`AnalysisMemo`]: the analyses' results kept in the kernel by the curve's exact
//!   content, so an unchanged curve checked again is not solved again.
//!
//! It is off when `Kernel::memoize_curve_caches` is off (the `curve-memo-off` feature),
//! which runs every analysis in full, to check that it changes nothing.
//!
//! The control-point bounds that used to skip an analysis whose tolerance they cleared
//! are gone: the analyses' own statuses are passed on (WR-663, tangency-labs/smlib
//! 8843967), so a search that fails must run to report its failure, which no bound on
//! the curve's speed or radius predicts.
//!
//! Not upstream (tangency-labs/solidcraft#11574): one closed-form exception,
//! `span_bounds` in `p3.rs`. On a curve of degree 1 or 2 made of Bezier spans (straight
//! segments and conic arcs), it bounds the speed and the radius of curvature from below,
//! exactly up to rounding. A search whose bound clears its tolerance is not run: no
//! solution could fail its test, but a numeric failure of that search is then not
//! reported either, which is the WR-663 trade-off this exception makes for such curves.
//! It is off with `Kernel::memoize_curve_caches`, as the memo is.
//!
//! Deviations: not upstream. No recursion.

use std::collections::{BTreeMap, VecDeque};

use crate::geom::solution_array::SolutionEnd;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;

/// The most results an [`AnalysisMemo`] keeps; the oldest goes first.
const MEMO_CAPACITY: usize = 4096;

/// The largest curve (control points) an [`AnalysisMemo`] keeps a result for.
const MEMO_MAX_POINTS: usize = 4096;

/// A global property analysis's result: whether it returned `SM_SUCCESS`, and its
/// solutions' start ends.
pub(crate) type AnalysisResult = (bool, Vec<SolutionEnd>);

/// The results of `AssertValid`'s global property analyses, by the exact content of
/// what they depend on ([`analysis_key`]): an unchanged curve checked again gets the
/// result it got before. The key is the content bit for bit, never an address, so a
/// hit gives exactly what the analysis would compute; at most [`MEMO_CAPACITY`]
/// results are kept, the oldest dropped first.
#[derive(Debug, Default)]
pub struct AnalysisMemo {
    results: BTreeMap<Vec<u64>, AnalysisResult>,
    order: VecDeque<Vec<u64>>,
}

impl AnalysisMemo {
    /// The result kept for `key`.
    pub(crate) fn get(&self, key: &[u64]) -> Option<AnalysisResult> {
        self.results.get(key).cloned()
    }

    /// Keeps `result` for `key`, dropping the oldest result past [`MEMO_CAPACITY`].
    pub(crate) fn insert(&mut self, key: Vec<u64>, result: AnalysisResult) {
        if self.results.contains_key(&key) {
            return;
        }
        while self.order.len() >= MEMO_CAPACITY {
            match self.order.pop_front() {
                Some(old) => {
                    self.results.remove(&old);
                }
                None => break,
            }
        }
        self.order.push_back(key.clone());
        self.results.insert(key, result);
    }
}

/// The key of an analysis of `curve` over `[lo, hi]` for `property` with minimum
/// separation `min_sep`, under `settings` (the kernel's tolerances and fixes, as the
/// caller encodes them): every input of the analysis, bit for bit. `None` for a curve
/// past [`MEMO_MAX_POINTS`] or without a NURB.
pub(crate) fn analysis_key(
    curve: &BSplineCurve,
    lo: f64,
    hi: f64,
    property: u64,
    min_sep: f64,
    settings: &[u64],
) -> Option<Vec<u64>> {
    let cur = curve.nurb.as_ref()?;
    if cur.cpts().len() > MEMO_MAX_POINTS {
        return None;
    }
    let mut key = vec![
        property,
        lo.to_bits(),
        hi.to_bits(),
        min_sep.to_bits(),
        curve.dim,
        u64::from(curve.out_of_bounds_enabled),
        u64::try_from(cur.degree()).ok()?,
        u64::try_from(cur.cpts().len()).ok()?,
        u64::try_from(cur.knots().len()).ok()?,
    ];
    for c in cur.cpts() {
        key.extend([c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()]);
    }
    key.extend(cur.knots().iter().map(|v| v.to_bits()));
    // (form and knot type by their debug names, as bytes)
    for b in format!("{:?}/{:?}", curve.form, curve.knot_type).bytes() {
        key.push(u64::from(b));
    }
    key.extend_from_slice(settings);
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memo_keeps_the_newest() {
        let mut memo = AnalysisMemo::default();
        for i in 0..(MEMO_CAPACITY as u64 + 2) {
            memo.insert(vec![i], (true, Vec::new()));
        }
        assert!(memo.get(&[0]).is_none() && memo.get(&[1]).is_none());
        assert!(memo.get(&[2]).is_some());
        assert_eq!(memo.results.len(), MEMO_CAPACITY);
    }
}
