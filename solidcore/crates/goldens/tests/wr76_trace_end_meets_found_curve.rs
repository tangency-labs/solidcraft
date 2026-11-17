//! WR-76: a near-tangent trace that reaches the free end of a curve already found stops
//! there, and its shortened last step ends on that curve's end.
//!
//! In `offset.offset_geometry_creation` case `gen0696` two offset surfaces touch along a
//! circle that is traced in two curves. The second used to run on over about 31 degrees
//! of the first (a second trace of it); it now stops at the first's start. A first-order
//! shortened step left its end 7e-4 short of that start; the secant corrections bring it
//! within the tolerance (1.2e-8 here).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::float_cmp
)] // test

use goldens::case::repo_root;
use goldens::{registry, CaseContext};

/// The start and end control points of every cubic non-rational 3D B-spline the offset
/// record holds (header `14033 3 1 n 3 n+4`), in record order.
fn curve_ends(v: &[f64]) -> Vec<([f64; 3], [f64; 3])> {
    let mut out = Vec::new();
    for i in 0..v.len().saturating_sub(6) {
        let h = &v[i..i + 6];
        if h[0] == 14033.0 && h[1] == 3.0 && h[2] == 1.0 && h[4] == 3.0 && h[5] == h[3] + 4.0 {
            let n = h[3] as usize;
            let st = i + 6;
            let Some(last) = v.get(st + 4 * n..st + 4 * n + 3) else {
                continue;
            };
            out.push(([v[st], v[st + 1], v[st + 2]], [last[0], last[1], last[2]]));
        }
    }
    out
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn shortened_trace_ends_on_the_found_curve() {
    let unit = registry()
        .into_iter()
        .find(|u| u.id() == "offset.offset_geometry_creation")
        .unwrap();
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = unit.cases(&ctx).unwrap();
    let case = cases.iter().find(|c| c.name == "gen0696").unwrap();
    let out = unit.run_rust(case).unwrap();
    let ends = curve_ends(&out.values["state"]);
    let (start, end) = *ends.last().unwrap();
    // the last curve is open: it stops at another curve's start
    assert!(dist(start, end) > 1.0, "the last curve closed on itself");
    let gap = ends[..ends.len() - 1]
        .iter()
        .map(|(s, _)| dist(*s, end))
        .fold(f64::INFINITY, f64::min);
    assert!(
        gap <= 1e-6,
        "the last curve ends {gap:e} from every other curve's start"
    );
}
