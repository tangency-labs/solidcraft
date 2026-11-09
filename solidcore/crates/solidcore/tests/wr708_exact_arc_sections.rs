//! WR-708 (`Kernel::fixes.fillet_exact_arc_sections`, the fork's tangency-labs/smlib@523bc25):
//! the executive's circular rounds take the ball's exact rational circle as their section.
//!
//! The part is the fork's tangent peg (capi `tangent_peg(0)`, as in
//! `fork_9bebf34_tangent_end_cases.rs`): a bar 40 x 16 x 10 whose far end is a half disc R 8
//! about (40, 8), joined with a disc R 8 about the same centre standing 10 on its top. Its
//! front top edge is rounded r 2, then the concave arc where the top meets the cylinder is
//! rounded r 2 together with the front round's end on the cylinder (capi
//! `peg_rounds_in_turn(2, 2, true)`). The concave round is a torus; the round along the front
//! round's end is a B-spline face, and the two join at the section from (32, 2, 10) on the
//! top to (33.6, 3.2, 12) on the cylinder. That section is the torus's: the circle of radius
//! 2 about (32, 2, 12) (the ball touching the plane z = 10 at radius 10 from the axis and the
//! cylinder of radius 8 at z = 12, in the radial plane through (32, 2)).
//!
//! With the fix the B-spline round ends on that circle (fork 5e8c9c1, built locally and run
//! on this case: its edge's UV trim on the round 1.6e-13 off the edge). Without it the
//! round's approximated end section is up to 1.6e-3 off it (fork 523bc25's parent 58d03ef:
//! 1.642e-3, the same as here).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point2, Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn extrude(k: &mut Kernel, cs: &[Curve]) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

fn tangent_peg(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let bar = [
        line(p(0.0, 0.0, 0.0), p(40.0, 0.0, 0.0)),
        Curve::from(curves::create_arc(p(40.0, 8.0, 0.0), 8.0, -90.0, 90.0).unwrap()),
        line(p(40.0, 16.0, 0.0), p(0.0, 16.0, 0.0)),
        line(p(0.0, 16.0, 0.0), p(0.0, 0.0, 0.0)),
    ];
    let bar = extrude(k, &bar);
    let disc = [Curve::from(
        curves::create_circle(p(40.0, 8.0, 10.0), 8.0).unwrap(),
    )];
    let disc = extrude(k, &disc);
    boolean_union(k, bar, disc).unwrap()
}

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> EdgeId {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.unwrap().0
}

/// Rounds the edges at `points` (constant radius `r`, circular), then stitches.
fn round(k: &mut Kernel, b: &mut Brep, points: &[[f64; 3]], r: f64) {
    let mut edges = Vec::new();
    for &at in points {
        let e = pick(k, b, at);
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    let radii = vec![r; edges.len()];
    let xsects = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xsects, 1, 1.0)
            .unwrap();
        s.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
}

/// The peg rounded in turn; returns how far the B-spline round's trim along the joint
/// section lies off that section's exact circle, and whether `check_brep` passes.
fn rounds_in_turn(fixes: &UpstreamFixes) -> (f64, bool) {
    let mut k = Kernel {
        fixes: *fixes,
        ..Kernel::default()
    };
    let k = &mut k;
    let mut b = tangent_peg(k);
    round(k, &mut b, &[[20.0, 0.0, 10.0]], 2.0);
    // the front round's end on the cylinder: its section at 45 degrees (capi peg_front_end)
    let c = 0.5_f64.sqrt();
    let s = 2.0 * (1.0 - c);
    let end = [40.0 - (s * (16.0 - s)).sqrt(), s, 8.0 + 2.0 * c];
    round(k, &mut b, &[[32.0, 8.0, 10.0], end], 2.0);

    let centre = Point3::new(32.0, 2.0, 12.0);
    let (a, z) = (Point3::new(32.0, 2.0, 10.0), Point3::new(33.6, 3.2, 12.0));
    let mut off = -1.0_f64;
    for e in get_edges(&b).unwrap() {
        let Some(curve) = edge::curve(&b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(&b, e).unwrap();
        let p0 = curve.evaluate_point(k, ivl.min()).unwrap();
        let p1 = curve.evaluate_point(k, ivl.max()).unwrap();
        if !((p0.distance(a) < 1e-6 && p1.distance(z) < 1e-6)
            || (p0.distance(z) < 1e-6 && p1.distance(a) < 1e-6))
        {
            continue;
        }
        for eu in edge::edgeuses(&b, e, None, None).unwrap() {
            let Some(f) = eu1::face(&b, eu).unwrap() else {
                continue;
            };
            let surface = face2::surface(&b, f).unwrap().unwrap();
            if surface.type_string() != "SmBSplineSurface_TYPE" {
                continue;
            }
            let uv = eu1::uv_trim_curve(&b, eu).unwrap().0.unwrap().clone();
            let ui = uv.natural_interval();
            for t in 0..=64 {
                let w = uv
                    .evaluate_point(k, ui.evaluate(f64::from(t) / 64.0))
                    .unwrap();
                let q = surface.evaluate_point(k, Point2::new(w.x, w.y)).unwrap();
                // off the circle: radially and out of its plane (the vertical plane through
                // the peg's axis (40, 8) and (32, 2))
                let radial = (q.distance(centre) - 2.0).abs();
                let plane = ((q.x - 40.0) * 0.6 - (q.y - 8.0) * 0.8).abs();
                off = off.max(radial).max(plane);
            }
        }
    }
    assert!(off >= 0.0, "no B-spline round along the joint section");
    let ok = check_brep(k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .is_empty();
    (off, ok)
}

fn without_exact_arcs() -> UpstreamFixes {
    let mut f = UpstreamFixes::all();
    *f.flag_mut("fillet_exact_arc_sections").unwrap() = false;
    f
}

#[test]
fn the_round_ends_on_the_exact_section_it_joins() {
    let (off, ok) = rounds_in_turn(&UpstreamFixes::all());
    assert!(off < 1e-9, "off the exact section by {off:e}");
    assert!(ok, "check_brep");
}

#[test]
fn without_it_the_approximated_section_is_off() {
    let (off, _) = rounds_in_turn(&without_exact_arcs());
    assert!(off > 1e-4, "off the exact section by only {off:e}");
}
