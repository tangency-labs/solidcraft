//! WR-708 (the fork's tangency-labs/smlib@523bc25): the executive's circular rounds take the
//! ball's exact rational circle as their section, on every kernel (the flag that made it
//! opt-in, `fillet_exact_arc_sections`, is retired: addendum 2026-11-06, #2330).
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
//! The B-spline round ends on that circle (fork 5e8c9c1, built locally and run on this case:
//! its edge's UV trim on the round 1.6e-13 off the edge). Upstream's approximated end section
//! was up to 1.6e-3 off it (fork 523bc25's parent 58d03ef: 1.642e-3, the same as the port
//! before).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point2, Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
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

#[test]
fn the_round_ends_on_the_exact_section_it_joins() {
    let (off, ok) = rounds_in_turn(&UpstreamFixes::all());
    assert!(off < 1e-9, "off the exact section by {off:e}");
    assert!(ok, "check_brep");
}

/// The box 20 x 30 x 20 on the given kernel, its edge y = 30, z = 20 rounded by the linear
/// law 1 to 5 along x (a variable round, so its face stays a B-spline whichever kernel builds
/// it): how far its end sections lie off a circle. Each end section is a station of the skin,
/// the circle along which the end ball touches the balls' envelope (WR-685), square to the
/// centre line and so not in the end plane: each boundary iso of the face's domain from the
/// top z = 20 to the back y = 30 is such a section, and its 17 points are measured off the
/// circle through its ends and middle.
fn variable_round_end_sections_off_their_circles(k: &mut Kernel) -> f64 {
    let mut b = create_box(k, Point3::ZERO, 20.0, 30.0, 20.0).unwrap();
    let e = pick(k, &b, [10.0, 30.0, 20.0]);
    let curve = edge::curve(&b, e).unwrap().unwrap().clone();
    let start = curve
        .evaluate_point(k, edge::interval(&b, e).unwrap().min())
        .unwrap();
    let (r0, r1) = if start.x < 10.0 {
        (1.0, 5.0)
    } else {
        (5.0, 1.0)
    };
    {
        let mut s = FilletSession::new(k, &mut b).unwrap();
        s.prepare_variable_radius_edges(k, &[e], r0, FilletLaw::linear(r0, r1, None), 1, 1)
            .unwrap();
        s.do_fillet(k).unwrap();
    }
    let mut off = -1.0_f64;
    for f in get_faces(&b).unwrap() {
        let surface = face2::surface(&b, f).unwrap().unwrap();
        if surface.type_string() == "SmPlane_TYPE" {
            continue;
        }
        let dom = face2::uv_domain(&b, f).unwrap();
        let (u, v) = (dom.u_interval(), dom.v_interval());
        let isos = [
            (Some(u.min()), None),
            (Some(u.max()), None),
            (None, Some(v.min())),
            (None, Some(v.max())),
        ];
        for (fixed_u, fixed_v) in isos {
            let points: Vec<Point3> = (0..=16)
                .map(|i| {
                    let t = f64::from(i) / 16.0;
                    let uv = Point2::new(
                        fixed_u.unwrap_or_else(|| u.evaluate(t)),
                        fixed_v.unwrap_or_else(|| v.evaluate(t)),
                    );
                    surface.evaluate_point(k, uv).unwrap()
                })
                .collect();
            let (p0, pm, p1) = (points[0], points[8], points[16]);
            let on = |p: Point3, q: Point3| (p.z - 20.0).abs() < 1e-9 && (q.y - 30.0).abs() < 1e-9;
            if !on(p0, p1) && !on(p1, p0) {
                continue; // along the edge, not a section
            }
            // the circle through p0, pm and p1
            let (ab, ac) = (pm - p0, p1 - p0);
            let n = ab.cross(ac);
            let centre = p0
                + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared())
                    / (2.0 * n.length_squared());
            let (radius, n) = (centre.distance(p0), n.unitize().unwrap());
            for q in &points {
                let h = (*q - centre).dot(n);
                let rho = (*q - centre - n * h).length();
                off = off.max(h.hypot(rho - radius));
            }
        }
    }
    assert!(off >= 0.0, "no end section of a round face");
    off
}

/// The default kernel (every fix off) rounds on the ball's exact circle: the exact section is
/// the default, with no flag (WR-708 addendum 2026-11-06, #2330). Before, only the retired
/// `fillet_exact_arc_sections` gave it: on SolidCraft's 52ea418 the default kernel's end
/// sections here were upstream's polynomial arcs, 3.26e-4 off a circle, and with the flag
/// 1.36e-11 off, as here (the residual of the touch circle's skin).
#[test]
fn the_default_kernel_rounds_on_the_exact_circle() {
    let off = variable_round_end_sections_off_their_circles(&mut Kernel::default());
    eprintln!("default kernel: end sections off their circles by {off:e}");
    assert!(off < 1e-10, "off the ball's circle by {off:e}");
}
