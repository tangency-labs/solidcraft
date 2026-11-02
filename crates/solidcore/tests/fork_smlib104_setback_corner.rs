//! The setback corner of tangency-labs/smlib#104 (tangency-labs/solidcore#2310): the G1 fill
//! as a graph over the corner's axis, certified convex (`SmBlendSetback::FillHole`), and the
//! least scale of the setbacks that certifies (`SmBlendSetback::CertifiedScale`), on the box
//! corner of the fork's capi `setback_g1_fill_keeps_the_body_convex` cases.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::needless_range_loop
)] // test helpers

use solidcore::blend::blend_setback::{
    certified_scale, containment, distances, fill_hole, Edge, Face, Round,
};
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use solidcore::surface::surface::Surface;

fn unit(i: usize) -> Vec3 {
    let mut d = [0.0; 3];
    d[i] = 1.0;
    Vec3::new(d[0], d[1], d[2])
}

/// The capi's `box_corner_faces`: `x, y, z >= 0`, face f where coordinate f is 0.
fn box_corner_faces() -> Vec<Face> {
    (0..3)
        .map(|f| Face {
            point: Point3::new(0.0, 0.0, 0.0),
            // n[f] = -1, the others +0 (as the capi builds it)
            normal: {
                let mut n = [0.0; 3];
                n[f] = -1.0;
                Vec3::new(n[0], n[1], n[2])
            },
        })
        .collect()
}

/// The capi's `box_corner_edges`.
fn box_corner_edges(radii: [f64; 3]) -> Vec<Edge> {
    (0..3)
        .map(|a| Edge {
            away: unit(a),
            radius: radii[a],
            faces: [(a + 1) % 3, (a + 2) % 3],
        })
        .collect()
}

fn axis_point(a: usize, along: f64, f: usize, at_f: f64, g: usize, at_g: f64) -> Point3 {
    let mut p = [0.0; 3];
    p[a] = along;
    p[f] = at_f;
    p[g] = at_g;
    Point3::new(p[0], p[1], p[2])
}

/// The capi's `box_corner_round` of order 1: the ball's circle along axis a over
/// `[s, s + len]`.
fn box_corner_round(a: usize, r: f64, s: f64, len: f64) -> BSplineSurface {
    let (f, g) = ((a + 1) % 3, (a + 2) % 3);
    let mut pts = Vec::new();
    let mut w = Vec::new();
    for i in 0..2 {
        let t = s + f64::from(i) * len;
        pts.extend([
            axis_point(a, t, f, 0.0, g, r),
            axis_point(a, t, f, 0.0, g, 0.0),
            axis_point(a, t, f, r, g, 0.0),
        ]);
        w.extend([1.0, 0.5f64.sqrt(), 1.0]);
    }
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 1,
        v_degree: 2,
        control_points: &pts,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &[2, 2],
        v_multiplicities: &[3, 3],
        u_knots: &[s, s + len],
        v_knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&w),
        uv_domain: None,
    })
    .unwrap()
}

/// The fork driver's printout (bin/fork-driver/sb.cpp's format), for a local comparison.
fn dump(radii: [f64; 3], setback: f64) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let faces = box_corner_faces();
    let edges = box_corner_edges(radii);
    let mut setbacks = distances(&faces, &edges).unwrap().setbacks;
    if setback > 0.0 {
        setbacks = vec![setback; 3];
    }
    writeln!(
        out,
        "distances {:e} {:e} {:e}",
        setbacks[0], setbacks[1], setbacks[2]
    )
    .unwrap();
    let kernel = Kernel::default();
    let scale = certified_scale(&kernel, &faces, &edges, &setbacks, None);
    let k_scale = match &scale {
        Ok(s) => {
            writeln!(
                out,
                "scale {:e} solves {} why {:?}",
                s.scale, s.solves, s.reason
            )
            .unwrap();
            s.scale
        }
        Err(e) => {
            writeln!(out, "scale refused {:?}", e.reason).unwrap();
            0.0
        }
    };
    for pass in 0..2 {
        let k = if pass == 0 { 1.0 } else { k_scale };
        if pass == 1 && k_scale <= 0.0 {
            break;
        }
        let rs: Vec<Surface> = (0..3)
            .map(|a| Surface::from(box_corner_round(a, radii[a], setbacks[a] * k, 10.0)))
            .collect();
        let rounds: Vec<Round<'_>> = (0..3)
            .map(|a| Round {
                surface: &rs[a],
                along: SurfParamType::U,
                end: setbacks[a] * k,
                corner_above: false,
            })
            .collect();
        match fill_hole(&kernel, &faces, &rounds, 1, 1e-9) {
            Ok(fill) => {
                writeln!(
                    out,
                    "fill k {k:e} degree {} patches {}",
                    fill.degree,
                    fill.patches.len()
                )
                .unwrap();
                for (p, patch) in fill.patches.iter().enumerate() {
                    for i in 0..=4 {
                        for j in 0..=4 {
                            let q = patch
                                .evaluate_point(Point2::new(f64::from(i) / 4.0, f64::from(j) / 4.0))
                                .unwrap();
                            writeln!(out, "pt {p} {i} {j} {:e} {:e} {:e}", q.x, q.y, q.z).unwrap();
                        }
                    }
                }
                for (p, patch) in fill.patches.iter().enumerate() {
                    for (j, c) in patch.canonical().unwrap().control_points.iter().enumerate() {
                        writeln!(out, "cp {p} {j} {:e} {:e} {:e}", c.x, c.y, c.z).unwrap();
                    }
                }
                let worst = containment(&fill, &faces, true).unwrap();
                writeln!(out, "containment {worst:e}").unwrap();
            }
            Err(e) => writeln!(out, "fill k {k:e} refused {:?}", e.reason).unwrap(),
        }
    }
    out
}

#[test]
#[ignore = "prints the fills for a local comparison with the fork driver"]
fn dump_box_corners() {
    for (r, s) in [
        ([5.0, 5.0, 5.0], 8.0),
        ([2.0, 3.0, 4.0], 0.0),
        ([1.0, 1.5, 2.0], 0.0),
        ([3.0, 3.0, 3.0], 0.0),
    ] {
        println!("== {r:?} {s}");
        print!("{}", dump(r, s));
    }
}

// ---- the executive: the fork's capi `smlib_blend` / `smlib_blend_setback` on a box ----

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::BIG_DOUBLE;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId, VertexId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner`.
fn all_fixes(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
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

fn vertex_at(b: &Brep, at: [f64; 3]) -> VertexId {
    let p = Point3::new(at[0], at[1], at[2]);
    b.vertices()
        .ids()
        .find(|&v| b.vertex(v).unwrap().point.distance(p) < 1e-9)
        .unwrap()
}

/// The corner set back: its vertex, setback, ease and whether it is left open.
struct SetbackAsk {
    at: [f64; 3],
    setback: f64,
    ease: f64,
    open: bool,
}

/// What a blend made: the body (when built), the status and the fit.
struct Blended {
    body: Option<Brep>,
    code: i32,
    comment: String,
    fit: f64,
}

/// The fork capi's `smlib_blend_setback` on a box `dims`: the edges at `at` rounded at
/// `radii` (circular, G1, thumbweight 1), the corner `ask` set back and filled.
fn blend_box(
    k: &mut Kernel,
    dims: [f64; 3],
    at: &[[f64; 3]],
    radii: &[f64],
    ask: Option<&SetbackAsk>,
) -> Blended {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), dims[0], dims[1], dims[2]).unwrap();
    blend_body(k, b, at, radii, ask)
}

/// The box `[0, 2c]^3` less `[c, 2c]^3` (the fork driver's `NOTCH=c`).
fn notched(k: &mut Kernel, c: f64) -> Brep {
    let part = create_box(k, Point3::new(0.0, 0.0, 0.0), 2.0 * c, 2.0 * c, 2.0 * c).unwrap();
    let cut = create_box(k, Point3::new(c, c, c), c, c, c).unwrap();
    boolean_difference(k, part, cut).unwrap()
}

/// [`blend_box`] on the body `b`.
fn blend_body(
    k: &mut Kernel,
    mut b: Brep,
    at: &[[f64; 3]],
    radii: &[f64],
    ask: Option<&SetbackAsk>,
) -> Blended {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, &b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let vertex = ask.map(|a| vertex_at(&b, a.at));
    let (ok, code, comment, fit) = {
        let mut s = FilletSession::new(k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, radii, &xs, 1, 1.0)
            .unwrap();
        if let (Some(a), Some(v)) = (ask, vertex) {
            s.add_set_back_vertex(v);
            s.set_set_back_fill(v, a.setback, a.ease, a.open);
        }
        let mut ok = s.create_corners(k, None).is_ok();
        if ok {
            if let (Some(a), Some(v)) = (ask, vertex) {
                for c in s.fillet_corners().unwrap() {
                    if s.corner(c).unwrap().vertex == Some(v) {
                        s.set_corner_setback(c, a.setback).unwrap();
                        s.set_corner_ease(c, a.ease).unwrap();
                    }
                }
            }
            ok = s.do_filleting(k, None).is_ok();
        }
        let info = s.error_info();
        (ok, info.code, info.comment.clone(), s.fit_setback())
    };
    if !ok {
        return Blended {
            body: None,
            code,
            comment,
            fit,
        };
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    Blended {
        body: Some(b),
        code,
        comment,
        fit,
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

/// Faces, edges and vertices of `b`.
fn counts(b: &Brep) -> (usize, usize, usize) {
    let edges = get_edges(b).unwrap();
    (
        get_faces(b).unwrap().len(),
        edges.len(),
        b.vertices().ids().count(),
    )
}

/// The fork blend driver's printout (`blend dx dy dz n x y z r ... [cx cy cz sb ease]`);
/// `FLAGOFF` set runs it without `fillet_blend_corner`.
fn dump_blend(dims: [f64; 3], at: &[[f64; 3]], radii: &[f64], ask: Option<&SetbackAsk>) -> String {
    let mut k = all_fixes(std::env::var("FLAGOFF").is_err());
    let r = blend_box(&mut k, dims, at, radii, ask);
    dump_blended(k, r)
}

fn dump_blended(mut k: Kernel, mut r: Blended) -> String {
    use std::fmt::Write;
    let mut out = format!("code {} fit {:e} comment {}\n", r.code, r.fit, r.comment);
    if let Some(b) = r.body.as_mut() {
        let lines = check_brep(&mut k, b, CheckLevel::Level2).unwrap().lines();
        let v = volume(&k, b);
        let (f, e, n) = counts(b);
        writeln!(
            out,
            "check {lines:?} volume {v:.15e} faces {f} edges {e} vertices {n}"
        )
        .unwrap();
        for vid in b.vertices().ids() {
            let p = b.vertex(vid).unwrap().point;
            writeln!(out, "v {:e} {:e} {:e}", p.x, p.y, p.z).unwrap();
        }
    }
    out
}

#[test]
#[ignore = "prints the blends for a local comparison with the fork driver"]
fn dump_blends() {
    print!(
        "{}",
        dump_blend([40.0, 30.0, 20.0], &UNEQUAL_AT, &[2.0, 3.0, 4.0], None)
    );
    for (sb, ease) in [(8.0, 0.0), (20.0, 0.0), (0.0, 0.0), (5.0, 0.0), (8.0, 2.0)] {
        println!("== setback {sb} ease {ease}");
        print!(
            "{}",
            dump_blend(
                [60.0, 40.0, 20.0],
                &ASKED_AT,
                &[5.0; 3],
                Some(&asked(sb, ease))
            )
        );
    }
    for sb in [4.25, 4.5, 4.6, 4.7515] {
        println!("== short {sb}");
        print!(
            "{}",
            dump_blend([20.0; 3], &SHORT_AT, &[2.0, 3.0, 4.0], Some(&short(sb)))
        );
    }
    println!("== shrink");
    print!(
        "{}",
        dump_blend([20.0, 20.0, 5.5], &SHRINK_AT, &[2.0, 3.0, 4.0], None)
    );
    println!("== concave");
    let mut k = all_fixes(std::env::var("FLAGOFF").is_err());
    let b = notched(&mut k, 10.0);
    let r = blend_body(&mut k, b, &CONCAVE_AT, &[1.0, 1.5, 2.0], None);
    print!("{}", dump_blended(k, r));
}

// ---- regression cases: the fork's capi setback cases, run through the executive ----
//
// Each value is the fork's own (tangency-labs/smlib b5bb747, built locally and run through
// `SmFilletExecutive` as these build the body): its status and comment, `GetFitSetBack`,
// face, edge and vertex counts, `SmApiBrepComputeVolume` at 1e-12 (printed to 15
// digits) and every vertex, which the port matched bit for bit. The values without the
// flag are the port's pinned corners (difftest gates those), recorded to show the flag is
// what changes them.

/// The edges at the box corner (40, 30, 20) of a 40 x 30 x 20 box.
const UNEQUAL_AT: [[f64; 3]; 3] = [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]];
/// The edges at the corner (60, 40, 20) of a 60 x 40 x 20 box.
const ASKED_AT: [[f64; 3]; 3] = [[30.0, 40.0, 20.0], [60.0, 20.0, 20.0], [60.0, 40.0, 10.0]];
/// The edges at the origin of the box `[0, 20]^3`.
const SHORT_AT: [[f64; 3]; 3] = [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]];
/// The edges at the origin of the box `[0, 20]^2 x [0, 5.5]`.
const SHRINK_AT: [[f64; 3]; 3] = [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 2.75]];
/// The notch's three concave edges at (10, 10, 10).
const CONCAVE_AT: [[f64; 3]; 3] = [[15.0, 10.0, 10.0], [10.0, 15.0, 10.0], [10.0, 10.0, 15.0]];

fn asked(setback: f64, ease: f64) -> SetbackAsk {
    SetbackAsk {
        at: [60.0, 40.0, 20.0],
        setback,
        ease,
        open: false,
    }
}

fn short(setback: f64) -> SetbackAsk {
    SetbackAsk {
        at: [0.0; 3],
        setback,
        ease: 0.0,
        open: false,
    }
}

/// A built blend: sound (no check lines), its counts and its volume to the fork's printed
/// 15 digits; returns the body.
fn built(k: &mut Kernel, r: Blended, counts_want: (usize, usize, usize), volume_want: f64) -> Brep {
    assert_eq!(r.code, 0, "{}", r.comment);
    let mut b = r.body.expect("a body");
    let lines = check_brep(k, &mut b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(counts(&b), counts_want);
    let v = volume(k, &mut b);
    println!("  volume {v:.17e}");
    assert!(
        (v - volume_want).abs() <= 1e-14 * volume_want,
        "{v} against {volume_want}"
    );
    b
}

fn has_vertex(b: &Brep, p: [f64; 3]) -> bool {
    let p = Point3::new(p[0], p[1], p[2]);
    b.vertices()
        .ids()
        .any(|v| b.vertex(v).unwrap().point.distance(p) <= 1e-9)
}

/// Rounds r 2, 3 and 4 at a box corner (the capi's `scenario_three_unequal_rounds_at_a_corner`):
/// no ball touches all three, so the corner is the Setback, its rule's setbacks grown to the
/// least scale whose G1 fill is certified convex. Without the flag, the pinned corner patch.
#[test]
fn three_unequal_rounds_are_the_certified_setback_fill() {
    let mut k = all_fixes(true);
    let r = blend_box(
        &mut k,
        [40.0, 30.0, 20.0],
        &UNEQUAL_AT,
        &[2.0, 3.0, 4.0],
        None,
    );
    built(&mut k, r, (15, 36, 23), 23841.6143900444);
    let mut k = all_fixes(false);
    let r = blend_box(
        &mut k,
        [40.0, 30.0, 20.0],
        &UNEQUAL_AT,
        &[2.0, 3.0, 4.0],
        None,
    );
    built(&mut k, r, (10, 22, 14), 23842.34001926002);
}

/// Three rounds r 5 at a box corner set back 8 on request (`SetSetBackFill`): the Setback's
/// three rounds end at 8 along their edges, r from them; filled, 6 patches. Without the flag
/// the request is the pinned setback corner's.
#[test]
fn asked_setback_is_the_setback_fill() {
    let mut k = all_fixes(true);
    let r = blend_box(
        &mut k,
        [60.0, 40.0, 20.0],
        &ASKED_AT,
        &[5.0; 3],
        Some(&asked(8.0, 0.0)),
    );
    let b = built(&mut k, r, (15, 36, 23), 47369.8443840603);
    for p in [
        [52.0, 40.0, 15.0],
        [52.0, 35.0, 20.0],
        [55.0, 32.0, 20.0],
        [60.0, 32.0, 15.0],
        [60.0, 35.0, 12.0],
        [55.0, 40.0, 12.0],
    ] {
        assert!(has_vertex(&b, p), "no end-section vertex at {p:?}");
    }
    let mut k = all_fixes(false);
    let r = blend_box(
        &mut k,
        [60.0, 40.0, 20.0],
        &ASKED_AT,
        &[5.0; 3],
        Some(&asked(8.0, 0.0)),
    );
    built(&mut k, r, (15, 36, 23), 47325.53167676825);
}

/// The same corner eased over 2 before its setback of 8.
#[test]
fn eased_asked_setback_is_the_setback_fill() {
    let ask = asked(8.0, 2.0);
    let mut k = all_fixes(true);
    let r = blend_box(&mut k, [60.0, 40.0, 20.0], &ASKED_AT, &[5.0; 3], Some(&ask));
    built(&mut k, r, (15, 36, 23), 47413.5259896674);
    let mut k = all_fixes(false);
    let r = blend_box(&mut k, [60.0, 40.0, 20.0], &ASKED_AT, &[5.0; 3], Some(&ask));
    built(&mut k, r, (15, 36, 23), 47344.78966041768);
}

/// A setback asked for that does not fit on the corner's edges (20, the upright edge's
/// length) is refused as no room, naming the setback that fits; one that stops short of
/// where the rails cross (0), or whose rails cross behind their ends (5), is refused with
/// its reason (the fork's `SM_FILERR_CORNER_UNSUPPORTED`, this port's 9).
#[test]
fn asked_setback_that_does_not_fit_is_refused() {
    let mut k = all_fixes(true);
    let r = blend_box(
        &mut k,
        [60.0, 40.0, 20.0],
        &ASKED_AT,
        &[5.0; 3],
        Some(&asked(20.0, 0.0)),
    );
    assert!(r.body.is_none());
    assert_eq!(r.code, 8);
    assert_eq!(r.comment, "Fillet Error: a setback of 20 (with its ease, 20) does not fit on the corner's edges: a setback below 20 fits");
    assert!(r.fit < 0.0);
    let r = blend_box(
        &mut k,
        [60.0, 40.0, 20.0],
        &ASKED_AT,
        &[5.0; 3],
        Some(&asked(0.0, 0.0)),
    );
    assert_eq!(r.code, 9);
    assert_eq!(r.comment, "Fillet Error: a setback of 0 stops short of where the rails of the rounds beside an edge cross it, 5 from the vertex");
    let r = blend_box(
        &mut k,
        [60.0, 40.0, 20.0],
        &ASKED_AT,
        &[5.0; 3],
        Some(&asked(5.0, 0.0)),
    );
    assert_eq!(r.code, 9);
    assert_eq!(r.comment, "SmBlendSetback: two rounds' rails on a face cross behind their ends (no corner region between them)");
    // without the flag, the pinned corner builds
    let mut k = all_fixes(false);
    let r = blend_box(
        &mut k,
        [60.0, 40.0, 20.0],
        &ASKED_AT,
        &[5.0; 3],
        Some(&asked(20.0, 0.0)),
    );
    assert_eq!(r.code, 0, "{}", r.comment);
}

/// Rounds r 2, 3, 4 at a corner set back 4.25 on request: past the rails' crossings but too
/// short for a certified convex fill, so refused as no room, `GetFitSetBack` naming the
/// setback that builds (the least certifying scale, to 1e-2); at that setback it builds.
#[test]
fn asked_setback_too_short_names_what_builds() {
    let mut k = all_fixes(true);
    let r = blend_box(
        &mut k,
        [20.0; 3],
        &SHORT_AT,
        &[2.0, 3.0, 4.0],
        Some(&short(4.25)),
    );
    assert!(r.body.is_none());
    assert_eq!(r.code, 8);
    assert_eq!(r.fit.to_bits(), 4.7515f64.to_bits(), "{}", r.fit);
    assert_eq!(r.comment, "Fillet Error: a setback of 4.25 is too short for the setback corner's fill to stay convex: a setback of 4.7515 builds it");
    let r = blend_box(
        &mut k,
        [20.0; 3],
        &SHORT_AT,
        &[2.0, 3.0, 4.0],
        Some(&short(r.fit)),
    );
    built(&mut k, r, (15, 36, 23), 7878.54334757591);
    // without the flag the asked setback builds the pinned corner, and no fit is named
    let mut k = all_fixes(false);
    let r = blend_box(
        &mut k,
        [20.0; 3],
        &SHORT_AT,
        &[2.0, 3.0, 4.0],
        Some(&short(4.25)),
    );
    assert!(r.fit < 0.0);
    built(&mut k, r, (15, 36, 23), 7873.207687501852);
}

/// The rule's setback of the upright edge (6) is longer than the edge (5.5): it shrinks to
/// the edge's room less a quarter of its radius, the others growing to a certified fill
/// (the capi's `setback_default_shrinks_to_fit`).
#[test]
fn default_setback_shrinks_to_fit() {
    let mut k = all_fixes(true);
    let r = blend_box(
        &mut k,
        [20.0, 20.0, 5.5],
        &SHRINK_AT,
        &[2.0, 3.0, 4.0],
        None,
    );
    let b = built(&mut k, r, (15, 36, 23), 2128.88386364731);
    // the upright round's end section at 4.5, r 4 from its edge on each face
    assert!(has_vertex(&b, [4.0, 0.0, 4.5]) && has_vertex(&b, [0.0, 4.0, 4.5]));
    let mut k = all_fixes(false);
    let r = blend_box(
        &mut k,
        [20.0, 20.0, 5.5],
        &SHRINK_AT,
        &[2.0, 3.0, 4.0],
        None,
    );
    built(&mut k, r, (10, 22, 14), 2128.610459220636);
}

/// The reentrant corner of a notched box, its three concave edges rounded r 1, 1.5 and 2:
/// a concave Setback, 18 faces (the capi's `setback_concave_corner_rounds`).
#[test]
fn concave_corner_is_the_setback_fill() {
    let mut k = all_fixes(true);
    let b = notched(&mut k, 10.0);
    let r = blend_body(&mut k, b, &CONCAVE_AT, &[1.0, 1.5, 2.0], None);
    built(&mut k, r, (18, 45, 29), 7015.2379122164);
    let mut k = all_fixes(false);
    let b = notched(&mut k, 10.0);
    let r = blend_body(&mut k, b, &CONCAVE_AT, &[1.0, 1.5, 2.0], None);
    built(&mut k, r, (13, 31, 20), 7015.147079173456);
}
