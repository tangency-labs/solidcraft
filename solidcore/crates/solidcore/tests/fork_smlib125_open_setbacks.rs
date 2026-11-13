//! tangency-labs/smlib#125 (ef57570, ce748fa, dc757a6; solidcore#2359): open setbacks.
//! A corner asked to be set back and left open
//! (`SetSetBackFill(.., open)`) is the blend corner's Setback without its fill: G1, each
//! face's spring curve is the parabola tangent to the two rails at their ends, in two
//! quadratic halves at its base point `B = (P + 2 X + Q) / 4` (`OpenSetBackSprings`), and the
//! executive trims the rounds at their creases with those curves imprinted on the faces, so
//! the hole's `3 n` edges stay free. A concave round's sheet lies in the void it fills
//! (ce748fa). The `NxN` corners keep their setback paths (the fork's dc757a6 deletes them;
//! solidcore#2365), and a setback asked at a corner that builds none is refused (the refusal
//! is solidcore's, the fork's is in its capi).
//!
//! The bodies and expected values are those of the fork's capi tests
//! `setback_open_g1_box_corner_hole` and `setback_open_concave_notch_hole` (cb2a478
//! `capi/tests/ops_kernel.cpp`): the hole's edge count, the sheet's counts and the spring
//! curves' P, X, Q. The parabolas' control points are checked in closed form. The counts,
//! refusals and volumes were also checked by running the fork's `SMLib` and `SM_API` at 18e96f1
//! (`SmFilletExecutive` with `SetSetBackFill`) on the same bodies.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::option_option
)] // test helpers

use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::p2::CurveKind;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, EdgeId, VertexId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`.
fn all_fixes() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    // (the log changes no result: issue_2489_blend_faces_are_definitions)
    k.blend_face_log = Some(solidcore::blend::definition_check::BlendFaceLog::default());
    k
}

/// jyviko's blend-definition rule (#2243, #2489): each face the blend made is a definition
/// (`blend::definition_check::is_definition`) or of a kind the ledger lists
/// (`data/blend_definition_ledger/`, one file per kind naming its follow-up issue).
fn assert_definitions_or_listed(name: &str, k: &Kernel, b: &Brep) {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/blend_definition_ledger"
    );
    let listed: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            e.unwrap()
                .path()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let made = k
        .blend_face_log
        .as_ref()
        .expect("the blend face log")
        .last()
        .to_vec();
    assert!(!made.is_empty(), "{name}: the blend made no face");
    for f in solidcore::blend::definition_check::bare_blend_faces(b, &made).unwrap() {
        let kind = f.kind.name().replace('/', "-");
        assert!(
            listed.contains(&kind),
            "{name}: bare face {} of kind {kind} ({}) is not in the ledger",
            f.face.index(),
            f.surface
        );
    }
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

/// What a blend made: the body (when built), the corner's event, the error noted.
struct Blended {
    body: Option<Brep>,
    event: Option<Option<BlendCornerEvent>>,
    code: i32,
    comment: String,
}

/// The fork capi's `smlib_blend_setback(_open)`: the edges at `at` rounded at `radius`
/// (circular, G1), the corner at `corner` set back `setback` and left `open` or filled.
fn blend(
    k: &mut Kernel,
    b: Brep,
    at: &[[f64; 3]],
    radius: f64,
    corner: [f64; 3],
    setback: f64,
    open: bool,
) -> Blended {
    blend_asked(k, b, at, radius, corner, setback, open, true, true)
}

/// [`blend`], the setback asked with `SetSetBackFill` before the corners are made only when
/// `fill`, and set on the made corner (`SetSetBackDist`) only when `dist`
/// (`AddSetBackVertex` either way).
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)] // test helper
fn blend_asked(
    k: &mut Kernel,
    mut b: Brep,
    at: &[[f64; 3]],
    radius: f64,
    corner: [f64; 3],
    setback: f64,
    open: bool,
    fill: bool,
    dist: bool,
) -> Blended {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, &b, p)).collect();
    let radii = vec![radius; edges.len()];
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let v = vertex_at(&b, corner);
    let (ok, event, code, comment) = {
        let mut s = FilletSession::new(k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
            .unwrap();
        s.add_set_back_vertex(v);
        if fill {
            s.set_set_back_fill(v, setback, 0.0, open);
        }
        let mut ok = s.create_corners(k, None).is_ok();
        let mut event = None;
        if ok {
            for c in s.fillet_corners().unwrap() {
                if s.corner(c).unwrap().vertex == Some(v) {
                    event = Some(s.corner(c).unwrap().blend.as_ref().map(|b| b.event));
                    if dist {
                        s.set_corner_setback(c, setback).unwrap();
                    }
                }
            }
            ok = s.do_filleting(k, None).is_ok();
        }
        let info = s.error_info();
        (ok, event, info.code, info.comment.clone())
    };
    Blended {
        body: ok.then_some(b),
        event,
        code,
        comment,
    }
}

/// Faces, edges and vertices of `b`.
fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        get_faces(b).unwrap().len(),
        get_edges(b).unwrap().len(),
        b.vertices().ids().count(),
    )
}

fn lamina(b: &Brep) -> Vec<EdgeId> {
    get_edges(b)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::is_lamina(b, e).unwrap())
        .collect()
}

/// The control points of `e`'s curve where it is a non-rational quadratic B-spline.
fn quadratic(b: &Brep, e: EdgeId) -> Option<[Point3; 3]> {
    let CurveKind::BSpline(c) = edge::curve(b, e).unwrap()?.kind() else {
        return None;
    };
    if c.is_rational().unwrap() || c.degree().unwrap() != 2 {
        return None;
    }
    let cps = c.control_points().unwrap();
    if cps.len() != 3 {
        return None;
    }
    Some([0, 1, 2].map(|i| Point3::new(cps[i].x, cps[i].y, cps[i].z)))
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

/// The box corner (60, 40, 20) of the 60 x 40 x 20 box, its three edges rounded 5.
const BOX_AT: [[f64; 3]; 3] = [[30.0, 40.0, 20.0], [60.0, 20.0, 20.0], [60.0, 40.0, 10.0]];

/// Set back 8 and left open: the Setback's hole of 9 edges (three sections whole, each
/// face's spring curve in two at its base point), the sheet's 9 faces, 27 edges and 19
/// vertices (fork capi `setback_open_g1_box_corner_hole`). On each face the halves are the
/// parabola (P, X, Q) split at t = 1/2: (P, (P + X) / 2, B) and (B, (X + Q) / 2, Q).
#[test]
fn open_g1_box_corner_is_the_setback_hole() {
    let mut k = all_fixes();
    let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let r = blend(&mut k, b, &BOX_AT, 5.0, [60.0, 40.0, 20.0], 8.0, true);
    assert_eq!(r.event, Some(Some(BlendCornerEvent::Setback)));
    assert_eq!(r.code, 0, "{}", r.comment);
    let b = r.body.expect("a sheet");
    assert_eq!(counts(&b), (9, 27, 19));
    assert_eq!(lamina(&b).len(), 9);
    assert!(!is_manifold_solid(&b).unwrap());
    // the springs: P, X, Q on the top, x = 60 and y = 40 faces (fork capi)
    let springs = [
        [
            p(52.0, 35.0, 20.0),
            p(55.0, 35.0, 20.0),
            p(55.0, 32.0, 20.0),
        ],
        [
            p(60.0, 32.0, 15.0),
            p(60.0, 35.0, 15.0),
            p(60.0, 35.0, 12.0),
        ],
        [
            p(52.0, 40.0, 15.0),
            p(55.0, 40.0, 15.0),
            p(55.0, 40.0, 12.0),
        ],
    ];
    let halves: Vec<[Point3; 3]> = lamina(&b)
        .into_iter()
        .filter_map(|e| quadratic(&b, e))
        .collect();
    assert_eq!(halves.len(), 6, "{halves:?}");
    let near = |a: Point3, c: Point3| a.distance(c) <= 1e-9;
    let has = |want: [Point3; 3]| {
        halves.iter().any(|h| {
            (near(h[0], want[0]) && near(h[1], want[1]) && near(h[2], want[2]))
                || (near(h[0], want[2]) && near(h[1], want[1]) && near(h[2], want[0]))
        })
    };
    for [pp, x, q] in springs {
        let base = (pp + x * 2.0 + q) * 0.25;
        assert!(has([pp, (pp + x) * 0.5, base]), "no half P-B of {pp:?}");
        assert!(has([base, (x + q) * 0.5, q]), "no half B-Q of {q:?}");
        assert!(b
            .vertices()
            .ids()
            .any(|v| near(b.vertex(v).unwrap().point, base)));
    }
}

/// The 60 x 40 x 20 box less [40, 70] x [20, 50] x [10, 30].
fn notched(k: &mut Kernel) -> Brep {
    let part = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let block = create_box(k, Point3::new(40.0, 20.0, 10.0), 30.0, 30.0, 20.0).unwrap();
    boolean_difference(k, part, block).unwrap()
}

/// The notch's three concave edges at (40, 20, 10).
const NOTCH_AT: [[f64; 3]; 3] = [[50.0, 20.0, 10.0], [40.0, 30.0, 10.0], [40.0, 20.0, 15.0]];

/// The notch's inner corner rounded 5, set back 7.5 and left open: the concave Setback's
/// hole of 9 edges, its rounds' sheets in the void they fill (ce748fa), not closed (fork
/// capi `setback_open_concave_notch_hole`).
#[test]
fn open_concave_notch_is_the_setback_hole() {
    let mut k = all_fixes();
    let b = notched(&mut k);
    let r = blend(&mut k, b, &NOTCH_AT, 5.0, [40.0, 20.0, 10.0], 7.5, true);
    assert_eq!(r.event, Some(Some(BlendCornerEvent::Setback)));
    assert_eq!(r.code, 0, "{}", r.comment);
    let b = r.body.expect("a sheet");
    assert_eq!(counts(&b), (12, 36, 25));
    assert_eq!(lamina(&b).len(), 9);
    assert!(!is_manifold_solid(&b).unwrap());
    let halves = lamina(&b)
        .into_iter()
        .filter(|&e| quadratic(&b, e).is_some())
        .count();
    assert_eq!(halves, 6);
}

/// The notch's inner corner left open with the convex edge at x = 60, y = 20 rounded too:
/// the sheet face of the corner at (60, 20, 10), between the concave rounds that add material
/// and the convex one that takes it off, is refused (ce748fa; the fork's SMLib at 18e96f1,
/// run on this body, refuses with the same reason).
#[test]
fn corner_face_between_adding_and_removing_blends_is_refused() {
    let at = [NOTCH_AT[0], NOTCH_AT[1], NOTCH_AT[2], [60.0, 20.0, 15.0]];
    let mut k = all_fixes();
    let b = notched(&mut k);
    let r = blend(&mut k, b, &at, 2.0, [40.0, 20.0, 10.0], 7.5, true);
    assert_eq!(r.event, Some(Some(BlendCornerEvent::Setback)));
    assert!(r.body.is_none());
    assert_eq!(r.code, 9); // CornerUnsupported (the fork's SM_FILERR_CORNER_UNSUPPORTED)
    assert!(
        r.comment
            .contains("between blends that add and blends that take off material"),
        "{}",
        r.comment
    );
}

/// A 40 x 40 x 20 box's corner (40, 40, 20) with two of its three edges rounded r 2: the
/// blend corner's two-blend corner, which builds no setback (`builds_setback`). Set back 4.
fn two_rounds(k: &mut Kernel, fill: bool) -> Blended {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let at = [[20.0, 40.0, 20.0], [40.0, 20.0, 20.0]];
    blend_asked(k, b, &at, 2.0, [40.0, 40.0, 20.0], 4.0, false, fill, true)
}

/// A setback asked with `SetSetBackFill` at a corner that builds none (the two-blend corner,
/// neither the Setback nor an `NxN` corner) is refused when the corners are made
/// (`CornerUnsupported`; solidcore-native, the fork refuses it only in its capi), not built
/// without it.
#[test]
fn setback_asked_where_the_setback_is_not_built_is_refused() {
    let mut k = all_fixes();
    let r = two_rounds(&mut k, true);
    assert_eq!(r.event, None);
    assert!(r.body.is_none());
    assert_eq!(r.code, 9, "{}", r.comment); // CornerUnsupported
    assert!(r.comment.contains("builds none"), "{}", r.comment);
}

/// The same corner set back only through `AddSetBackVertex` and `SetSetBackDist` on the made
/// corner: `do_filleting` refuses it (`CornerUnsupported`), as the fill's door does.
#[test]
fn setback_set_on_a_corner_that_builds_none_is_refused() {
    let mut k = all_fixes();
    let r = two_rounds(&mut k, false);
    assert!(r.event.is_some());
    assert!(r.body.is_none());
    assert_eq!(r.code, 9, "{}", r.comment); // CornerUnsupported
    assert!(r.comment.contains("builds none"), "{}", r.comment);
}

/// The box [0, 40]^2 x [0, 20] less the cylinder r 20 about (40, 40): at (20, 40, 20) a top
/// edge, the top arc and the vertical edge, all convex, one curved, rounded 2, set back 4.
/// Without `fillet_setback_beside_curved_faces` it is no Setback but the `NxN` corner,
/// whose setback path builds the setback set on it (`SetSetBackDist`): the pinned setback
/// corner (16, 39, 25). The fork's dc757a6 deletes that path
/// (its SMLib at 18e96f1 builds the corner without the setback, 11 faces); solidcore keeps
/// it (solidcore#2365). A `SetSetBackFill` request there (which only the Setback reads) is
/// ignored, as before the port: the corner without a setback (11, 24, 15, as the fork's
/// SMLib builds it) when none is set on it.
#[test]
fn nxn_corner_builds_the_setback_set_on_it() {
    use solidcore::primitive::api_primitives::create_cylinder;
    let at = [
        [10.0, 40.0, 20.0],
        [20.0, 40.0, 10.0],
        [25.0, 40.0 - 175.0_f64.sqrt(), 20.0],
    ];
    for (fill, dist) in [(false, true), (true, true), (true, false)] {
        let mut k = all_fixes();
        k.fixes.fillet_setback_beside_curved_faces = false;
        let bx = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
        let cyl = create_cylinder(&mut k, Point3::new(40.0, 40.0, -5.0), 20.0, 30.0).unwrap();
        let b = boolean_difference(&mut k, bx, cyl).unwrap();
        let r = blend_asked(
            &mut k,
            b,
            &at,
            2.0,
            [20.0, 40.0, 20.0],
            4.0,
            false,
            fill,
            dist,
        );
        assert_eq!(r.event, Some(None), "{fill} {dist}");
        assert_eq!(r.code, 0, "{}", r.comment);
        let mut b = r.body.expect("a body");
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        let want = if dist { (16, 39, 25) } else { (11, 24, 15) };
        assert_eq!(counts(&b), want, "{fill} {dist}");
    }
}

/// The bar running into the post: a 40 x 16 x 10 bar joined to an R8 x 20 cylinder about
/// (40, 8), the bar's corner (0, 0, 10) rounded r 2 on its three edges and set back 3.
fn bar_into_post(k: &mut Kernel, open: bool) -> Blended {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::create_cylinder;
    let bar = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 16.0, 10.0).unwrap();
    let post = create_cylinder(k, Point3::new(40.0, 8.0, 0.0), 8.0, 20.0).unwrap();
    let b = boolean_union(k, bar, post).unwrap();
    let at = [[0.0, 0.0, 5.0], [20.0, 0.0, 10.0], [0.0, 8.0, 10.0]];
    blend(k, b, &at, 2.0, [0.0, 0.0, 10.0], 3.0, open)
}

/// SolidCraft's report-fillet-g2-corner-of-bar-running-into-cylinder, as the fork capi's
/// `setback_open_hole_on_a_body_with_a_closed_face` (7dccbae), left open: the Setback's
/// hole of nine edges. The cylinder's face is closed; its seam, its own edge run twice by
/// it, is no free edge (7dccbae fixed the capi's hole reader for that; the kernel counts
/// lamina edges, which a seam is not). The fork's SMLib at alpha-1 6843885 run on this body
/// builds the sheet of 10 faces, 30 edges and 21 vertices, 9 of them free. In solidcore
/// the trim classified the bar's inside as outside (the B-rep ray built its face caches
/// afresh while the target was being edited, without their trims) and refused the blend
/// as `NoRoom`, until #2503 (`brep_ray_classifies_current_trims`, WR-732, which `all_fixes`
/// turns on): the trim classifies against the faces' current trims.
#[test]
fn open_hole_on_a_body_with_a_closed_face_is_the_corner_nine_edges() {
    let mut k = all_fixes();
    let r = bar_into_post(&mut k, true);
    assert_eq!(r.event, Some(Some(BlendCornerEvent::Setback)));
    assert_eq!(r.code, 0, "{}", r.comment);
    let mut b = r.body.expect("a sheet");
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(counts(&b), (10, 30, 21));
    assert_eq!(lamina(&b).len(), 9);
    assert!(!is_manifold_solid(&b).unwrap());
}

/// The same corner filled: the fork's SMLib at alpha-1 6843885 gives 16 faces, 39 edges and
/// 25 vertices, volume 9362.39265676429 (an earlier note's 9362.281 was not this body's).
#[test]
fn bar_into_post_setback_filled_is_the_fork_s_solid() {
    let mut k = all_fixes();
    let r = bar_into_post(&mut k, false);
    assert_eq!(r.event, Some(Some(BlendCornerEvent::Setback)));
    assert_eq!(r.code, 0, "{}", r.comment);
    let mut b = r.body.expect("a solid");
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(counts(&b), (16, 39, 25));
    assert!(is_manifold_solid(&b).unwrap());
    assert_definitions_or_listed("the bar's corner filled", &k, &b);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-12, &attrs).unwrap();
    assert!((v - 9362.39265676429).abs() <= 1e-9 * v, "{v}");
}
