//! A variable round whose radius is 0 at an end vertex closes onto it: its rails end on
//! their side edges at the vertex and its end rib has no length. At a corner of mixed
//! convexity the Nx1 end capped the hole between the rib and the side edges' extensions,
//! extending the end face by 7.5 times the radius there: at radius 0 by nothing, and the
//! cap's rail ends were never found (`SM_ERR`, "Fillet Corner Error: Unable to Make
//! Topology"). There is no hole: it ends as at a corner of one convexity. The radius at the
//! vertex is the law curve's at the end of its domain, not read through an extension an
//! earlier evaluation past the edge cached (which rounds the curve's 0 to 3e-15).
//!
//! - The law alone: SolidCraft's taper law (cubic, 8 uniform spans, 13.5 down to 0) evaluated
//!   past its domain (which caches the extension), then at its ends and inside: bit for bit the
//!   values of a law that never extended, exactly 0 and 13.5 at its ends.
//! - SolidCraft #11360's `fillet_fit_taper_closes_to_a_point` (its #293): a wedge against a
//!   wall, its top face closing to a point at x = 40, the top front edge rounded with the
//!   cubic radius law SolidCraft's taper fit builds (8 spans, control radii 0.9 of the room
//!   along the edge at their Greville positions), 13.5 at x = 0 down to 0 at the point. The
//!   wedge is the triangle (0, 0), (40, 0), (0, 20) extruded 15 along z, the wall the
//!   quadrilateral (40, 0), (0, 20), (0, 30), (40, 30) from z = -15 to 30, joined; the edge
//!   lies between the wedge's front face y = 0 and its top face z = 15.
//! - A box edge whose radius runs linearly from 4 to 0 (and 0 to 4): the round closes to the
//!   box corner as before. Its sections are the circles each ball touches the round along,
//!   shifted `-r r' / |c'|` from the great circle, on every kernel (the G^n section's flag,
//!   `fillet_gn_section_generator`, is retired: WR-685 addendum 2026-11-06), and it removes
//!   the balls' union. Its volume 22.594877 is not a closed form but SolidCraft's numerical
//!   integration of that union (`removed_by_balls` in its
//!   `variable_radius_down_to_zero_at_one_end`, which checks it to 1e-5). The great circles
//!   removed `(1 - pi/4) L r0^2 / 3 = 22.8909`.
//!
//!   Each is checked to 1e-4 relative, as SolidCraft checks it.
//!
//! Each built through the fillet executive (`SetFilletParameters` with the B-spline law, the
//! corners, `DoFilleting`, `StitchAndOrient`), with the fixes each test names.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers

use solidcore::blend::fillet_executive::p1::{FilletLaw, FilletLawKind};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, BSplineCurveForm, CanonicalData};
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// Which fixes a leg runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixes {
    /// None.
    None,
    /// Every fix.
    All,
}

fn kernel(fixes: Fixes) -> Kernel {
    Kernel {
        fixes: match fixes {
            Fixes::None => UpstreamFixes::default(),
            Fixes::All => UpstreamFixes::all(),
        },
        ..Kernel::default()
    }
}

fn prism(k: &mut Kernel, pts: &[[f64; 2]], z0: f64, height: f64) -> Brep {
    let p = |i: usize| Point3::new(pts[i][0], pts[i][1], z0);
    let sides: Vec<Curve> = (0..pts.len())
        .map(|i| Curve::from(create_line_segment(p(i), p((i + 1) % pts.len())).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), height, true).unwrap()
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

/// SolidCraft's taper of `radius` on this edge, from x = 0: the cubic radius spline of 8
/// uniform spans whose control radii are `min(radius, 0.9 room)` at the ends (the room 15 at
/// x = 0, the front face's height under the top face's 20; 0 at the point) and `0.9 room` at
/// the inner Greville positions, `room(u) = min(radius / 0.9, 15, 20 (1 - u))`.
fn taper(radius: f64) -> (Vec<f64>, Vec<f64>) {
    let spans: u8 = 8;
    let mut knots = vec![0.0; 4];
    knots.extend((1..spans).map(|i| f64::from(i) / f64::from(spans)));
    knots.extend([1.0; 4]);
    let n = usize::from(spans) + 3;
    let greville: Vec<f64> = (0..n)
        .map(|j| (knots[j + 1] + knots[j + 2] + knots[j + 3]) / 3.0)
        .collect();
    let room = |u: f64| (radius / 0.9).min(15.0).min(20.0 * (1.0 - u));
    let radii: Vec<f64> = (0..n)
        .map(|j| match j {
            0 => (0.9 * 15.0f64).min(radius),
            j if j == n - 1 => 0.0,
            _ => 0.9 * room(greville[j]),
        })
        .collect();
    (knots, radii)
}

/// SolidCraft's adapter's `law_along`: the law as a B-spline with the radius as its x and
/// its Greville abscissa as its y, reversed when `start` is nearer the end of the edge's
/// interval.
fn law_along(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    start: Point3,
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> FilletLaw {
    let curve = edge::curve(b, e).unwrap().unwrap();
    let ivl = edge::interval(b, e).unwrap();
    let lo = curve.evaluate_point(k, ivl.min()).unwrap();
    let hi = curve.evaluate_point(k, ivl.max()).unwrap();
    let reverse = (hi - start).length() < (lo - start).length();
    let (nk, nr) = (knots.len(), radii.len());
    let kv: Vec<f64> = (0..nk)
        .map(|i| {
            if reverse {
                1.0 - knots[nk - 1 - i]
            } else {
                knots[i]
            }
        })
        .collect();
    let cps: Vec<Point3> = (0..nr)
        .map(|i| {
            let g: f64 = (1..=degree).filter_map(|j| kv.get(i + j)).sum();
            let r = if reverse { radii[nr - 1 - i] } else { radii[i] };
            Point3::new(r, g / degree as f64, 0.0)
        })
        .collect();
    let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &x in &kv {
        match (unique.last(), mults.last_mut()) {
            (Some(&u), Some(m)) if u == x => *m += 1,
            _ => {
                unique.push(x);
                mults.push(1);
            }
        }
    }
    let law_curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::new(FilletLawKind::BSpline {
        law_curve,
        extended_law: None,
    })
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

/// `b`'s edge at `at` rounded with the B-spline radius law of `degree`, `knots` and `radii`
/// from its end nearest `start`, through the fillet executive, stitched: the body, sound
/// (no check lines), or what failed.
fn round_variable(
    k: &mut Kernel,
    mut b: Brep,
    at: [f64; 3],
    start: [f64; 3],
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<Brep, String> {
    let e = pick(k, &b, at);
    let start = Point3::new(start[0], start[1], start[2]);
    let law = law_along(k, &b, e, start, degree, knots, radii);
    let size = radii[0].max(*radii.last().unwrap());
    let (done, code, comment, at_vertex) = {
        let mut s = FilletSession::new(k, &mut b).unwrap();
        let mut step = s.prepare_variable_radius_edges(k, &[e], size, law, 1, 1);
        if step.is_ok() {
            step = s.create_corners(k, None);
        }
        if step.is_ok() {
            step = s.do_filleting(k, None);
        }
        let info = s.error_info();
        let at_vertex = match info.vertex {
            Some(solidcore::blend::fillet_executive::session::FilletObject::Topology(
                _,
                solidcore::error::EntityRef::Vertex(v),
            )) => Some(v),
            _ => None,
        };
        let comment = format!("{} (corner {:?})", info.comment, info.corner_type);
        (step, info.code, comment, at_vertex)
    };
    if let Err(e) = done {
        let place = at_vertex.and_then(|v| b.vertex(v).ok()).map(|v| v.point);
        return Err(format!("{e:?}: error info {code} {comment} at {place:?}"));
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let lines = check_brep(k, &mut b, CheckLevel::Level2).unwrap().lines();
    if !lines.is_empty() {
        return Err(format!("check: {lines:?}"));
    }
    Ok(b)
}

fn has_vertex(b: &Brep, p: [f64; 3]) -> bool {
    let p = Point3::new(p[0], p[1], p[2]);
    b.vertices()
        .ids()
        .any(|v| b.vertex(v).unwrap().point.distance(p) <= 1e-9)
}

/// The taper fit's round on the wedge, with `fixes`: the body (sound, its volume less the
/// round's, the round closing to the tip (40, 0, 15)) or what failed.
fn wedge_taper(fixes: Fixes) -> Result<(), String> {
    let mut k = kernel(fixes);
    let wedge = prism(&mut k, &[[0.0, 0.0], [40.0, 0.0], [0.0, 20.0]], 0.0, 15.0);
    let wall = prism(
        &mut k,
        &[[40.0, 0.0], [0.0, 20.0], [0.0, 30.0], [40.0, 30.0]],
        -15.0,
        45.0,
    );
    let mut b = boolean_union(&mut k, wedge, wall).unwrap();
    let before = volume(&k, &mut b);
    let (knots, radii) = taper(16.0);
    let mut r = round_variable(
        &mut k,
        b,
        [20.0, 0.0, 15.0],
        [0.0, 0.0, 15.0],
        3,
        &knots,
        &radii,
    )?;
    let after = volume(&k, &mut r);
    println!("  wedge, {fixes:?}: volume {before:.9} -> {after:.9}");
    if after < before && has_vertex(&r, [40.0, 0.0, 15.0]) {
        Ok(())
    } else {
        Err(format!("volume {after} or no tip vertex"))
    }
}

/// The taper law evaluated past its domain (which caches the extension), then at its ends and
/// inside: the curve's own values, bit for bit those of a law that never extended.
#[test]
fn law_reads_its_curve_inside_its_domain() {
    let (knots, radii) = taper(16.0);
    let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &x in &knots {
        match (unique.last(), mults.last_mut()) {
            (Some(&u), Some(m)) if u == x => *m += 1,
            _ => {
                unique.push(x);
                mults.push(1);
            }
        }
    }
    let cps: Vec<Point3> = (0..radii.len())
        .map(|i| {
            let g: f64 = (1..=3).filter_map(|j| knots.get(i + j)).sum();
            Point3::new(radii[i], g / 3.0, 0.0)
        })
        .collect();
    let law = || {
        let law_curve = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &cps,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &unique,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap();
        FilletLaw::new(FilletLawKind::BSpline {
            law_curve,
            extended_law: None,
        })
    };
    let edge = Extent1d::from_raw(0.0, 1.0);
    let mut extended = law();
    let mut fresh = law();
    // past the domain: the law extends
    let past = extended.evaluate(1.05, &edge, false).unwrap();
    println!("  past the domain: {past:?}");
    for t in [0.0, 0.125, 0.3, 0.5, 0.875, 1.0] {
        let a = extended.evaluate(t, &edge, false).unwrap();
        let b = fresh.evaluate(t, &edge, false).unwrap();
        println!("  t {t}: {a:?} (no extension {b:?})");
        assert_eq!(a, b, "t {t}");
    }
    assert_eq!(extended.evaluate(1.0, &edge, false).unwrap()[0], 0.0);
    assert_eq!(extended.evaluate(0.0, &edge, false).unwrap()[0], 13.5);
}

/// The taper fit's round closing to the wedge's tip, with no fixes and with every fix: the
/// blend corner's End on its end face, whose construction is the Nx1 end's.
#[test]
fn taper_closing_to_a_point_rounds_the_wedge() {
    let mut wrong = Vec::new();
    for fixes in [Fixes::None, Fixes::All] {
        if let Err(why) = wedge_taper(fixes) {
            wrong.push(format!("{fixes:?}: {why}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// A box edge's radius running linearly from 4 to 0 (and 0 to 4) closes the round to the box
/// corner: 7 faces, the corner kept, removing the balls' union (module docs; SolidCraft's
/// numerical reference), with no fixes and with every fix. Checked to 1e-4 relative.
#[test]
fn linear_radius_to_zero_closes_a_box_round_to_the_corner() {
    // the union of the balls, integrated numerically (module docs)
    let balls = 22.594_877;
    let mut wrong = Vec::new();
    for (fixes, want) in [(Fixes::None, balls), (Fixes::All, balls)] {
        for (r0, r1) in [(4.0, 0.0), (0.0, 4.0)] {
            let mut k = kernel(fixes);
            let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
            let law = [r0, r1];
            let made = round_variable(
                &mut k,
                b,
                [10.0, 10.0, 30.0],
                [10.0, 0.0, 30.0],
                1,
                &[0.0, 0.0, 1.0, 1.0],
                &law,
            );
            let mut r = match made {
                Ok(r) => r,
                Err(why) => {
                    wrong.push(format!("{fixes:?}, {r0} -> {r1}: {why}"));
                    continue;
                }
            };
            let removed = 6000.0 - volume(&k, &mut r);
            let faces = get_faces(&r).unwrap().len();
            let corner = if r1 < r0 {
                [10.0, 20.0, 30.0]
            } else {
                [10.0, 0.0, 30.0]
            };
            println!(
                "  box, {fixes:?}, {r0} -> {r1}: removed {removed:.9} (want {want:.9}), {faces} faces"
            );
            if !((removed - want).abs() < 1e-4 * want && faces == 7 && has_vertex(&r, corner)) {
                wrong.push(format!("{fixes:?}, {r0} -> {r1}: {removed}, {faces} faces"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
