//! The golden rows that the retirement of `bspline_trim_preserves_uv_spans` (WR-441) moves.
//! `BSplineCurve::trim` now keeps the short end spans of a dimension-2 (UV) curve: the
//! result is the exact restriction `crv_trim(curve, lo, hi)`. Upstream's
//! `N_CrvRemoveDegenSegs` is no longer run on it with the 3D zone tolerance, a length in
//! model units applied in surface-parameter units. The exception is an end span that
//! collapses to a pole as check test #15 measures one; that span is still cleaned (#2129).
//! Model curves (dimension 3) are cleaned as before.
//!
//! The certificates:
//! - **UV curves with a 3D partner** (silhouette and intersection curves, the fillet
//!   rail). These are certified on their partners with
//!   `curve_surface_gap::trim_image_gap_bound`, a rounded-interval Hausdorff bound between
//!   `C` and `S(W)`, over each end span of the trim: the spans the change keeps. The
//!   bound must be within the operation's tolerance.
//! - **B-rep trims** (the fillet removal and the two offsets). These are certified on
//!   their edges within the edge's tolerance (`trims_on_edges::certify_edgeuse`).
//!
//! The rest of each moved curve is the restriction `crv_trim` that upstream also built,
//! before its cleanup.
//!
//! The certificates live next to each unit's runner:
//! - `surface_surface_p1::tests::bstuv_silhouette_kept_spans_lie_on_their_curves`
//! - `surface_surface_p2::tests::bstuv_intersection_kept_spans_lie_on_their_curves`
//! - `blend_fillet_solver_p1::bstuv::bstuv_trim_rail_keeps_its_rail_on_the_edge`
//! - `blend_fillet_executive_p1::topo::tests::bstuv_mouse_two_struts_trims_lie_on_their_edges`
//! - `offset_offset_geometry_creation::tests::bstuv_offset_moved_trims_lie_on_their_edges`
//!
//! This module checks that each moved row's replay reproduces its blessed row.
//!
//! Run (the certificates bisect long curves, so they are `#[ignore]`d; the goldens job
//! checks the rows themselves):
//! `cargo test --profile goldens -p goldens --lib bstuv_ -- --include-ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr
)]

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::kernel::Kernel;
use solidcore::query::curve_surface_gap::trim_image_gap_bound;
use solidcore::surface::surface::Surface;

use crate::runner::UnitDiff;
use crate::units::{
    blend_fillet_executive_p1, blend_fillet_solver_p1, offset_offset_geometry_creation,
    surface_surface_p1, surface_surface_p2,
};

/// The moved rows, as (unit, case).
const ROWS: [(&str, &str); 7] = [
    (
        "blend.fillet_executive_p1",
        "remove_topo_edges_mouse_two_struts",
    ),
    ("blend.fillet_solver_p1", "trim_rail_mouse_rebuild_edge7"),
    ("offset.offset_geometry_creation", "gen0074"),
    ("offset.offset_geometry_creation", "gen0400"),
    ("surface.surface_p1", "sil0048"),
    ("surface.surface_p1", "sil_adv_20_torus_f0"),
    ("surface.surface_p2", "ssi_adv_ssi_clasic_ruled_f0_f0"),
];

/// A UV curve's certificate on its 3D partner, as [`trim_end_spans`] encloses it.
#[derive(Debug)]
pub(crate) struct EndSpans {
    /// The UV curve's control point count.
    pub(crate) n: usize,
    /// Proved bounds on the Hausdorff distance between `C` and `S(W)` over each of them.
    pub(crate) bound: [f64; 2],
}

/// Certifies the end spans of the UV curve `w` on its 3D partner `c` on `surface`. It
/// encloses the Hausdorff distance between `C` and `S(W)` over the first and the last knot
/// span of `w`, through `trim_image_gap_bound`. The two curves share their parameters, as
/// the trace builds them. Panics when a curve is not a B-spline or a bound is not
/// enclosed.
pub(crate) fn trim_end_spans(k: &Kernel, surface: &Surface, c: &Curve, w: &Curve) -> EndSpans {
    let CurveKind::BSpline(bw) = w.kind() else {
        panic!("the UV curve is not a B-spline")
    };
    let nw = bw.gw_nurb().unwrap();
    let mut d: Vec<f64> = nw.knots().to_vec();
    d.dedup();
    assert!(d.len() >= 2, "an empty knot vector");
    let (lo, hi) = (d[0], d[d.len() - 1]);
    let (head, tail) = ([lo, d[1]], [d[d.len() - 2], hi]);
    let bound = |r: [f64; 2]| {
        trim_image_gap_bound(k, surface, c, w, r, 0.0)
            .unwrap()
            .expect("the bound is enclosed")
    };
    EndSpans {
        n: nw.n(),
        bound: [bound(head), bound(tail)],
    }
}

/// Every moved row's replay reproduces its blessed row, so the certificates in the
/// runners' modules measure the bodies and curves the rows record.
#[test]
#[ignore = "replays the fillet and offset rows (minutes); see the module docs"]
fn bstuv_moved_rows_replay_their_rows() {
    let dir = crate::golden::default_dir();
    for (unit, name) in ROWS {
        let runner: &dyn UnitDiff = match unit {
            "blend.fillet_executive_p1" => &blend_fillet_executive_p1::Runner,
            "blend.fillet_solver_p1" => &blend_fillet_solver_p1::Runner,
            "offset.offset_geometry_creation" => &offset_offset_geometry_creation::Runner,
            "surface.surface_p1" => &surface_surface_p1::Runner,
            _ => &surface_surface_p2::Runner,
        };
        let case = crate::case::regression_case(runner, name);
        let out = runner.run_rust(&case).unwrap();
        let rows = crate::golden::read(&dir, unit).unwrap();
        assert!(
            crate::golden::diff(
                &rows[name],
                &crate::golden::encode(name, &out),
                runner.abs_floors()
            )
            .is_none(),
            "{unit} {name}: the replay does not reproduce the row"
        );
    }
}
