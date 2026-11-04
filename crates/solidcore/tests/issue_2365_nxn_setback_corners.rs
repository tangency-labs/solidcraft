//! solidcore#2365 (SolidCraft rig, jyviko's review): a corner set back through
//! `AddSetBackVertex` and `SetSetBackDist` on the N x N corner builds its setback with
//! `Kernel::fixes.fillet_blend_corner`, as before smlib#125's port. The fork's dc757a6
//! deletes the N x N corners' setback paths, which left these corners refused
//! (`CornerUnsupported`) by the check that a setback asked is built; solidcore keeps the
//! paths, and the check refuses only a corner that builds no setback.
//!
//! The seven cases are SolidCraft's kernel-rig cases that built before the port (their
//! entries are in `tests/kernel_cases/`), driven as SolidCraft's `round_edges` drives the
//! session: circular rounds per edge, each set-back vertex marked (`add_set_back_vertex`)
//! before the corners are made, then `set_corner_setback` on the vertex's corner, and with
//! a G2 corner the corner's patch marked (`set_mark_corner_patches`) for SolidCraft to
//! replace; then `StitchAndOrient`. The fixes are the corpus's `fixes.json`. The values are
//! those the same calls give on main without the port (origin/main 48b1bd207), every one:
//! counts, the patch's faces and the volume to 1e-9. Where the
//! case's rig value is the kernel's own (013's filled corner, and the cut that takes the
//! whole patch away in `...-outside-setback`) it is the rig's measured value too; the G2
//! cases' rig volumes are SolidCraft's patch's, so here the corner's own N x N patch is
//! checked: its six faces marked, and the body's faces the rig's less SolidCraft's one G2
//! patch plus these six.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletCornerType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, EdgeId, VertexId};
use solidcore::{Brep, Kernel};

/// The corpus's fix list (`tests/kernel_cases/fixes.json`, SolidCraft's), and
/// `closed_smooth_pole_tangent_limit`, which the rig run turned on too.
fn solidcraft_fixes() -> Kernel {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in names
        .iter()
        .map(String::as_str)
        .chain(["closed_smooth_pole_tangent_limit"])
    {
        if let Some(flag) = f.flag_mut(name) {
            *flag = true;
        }
    }
    assert!(f.fillet_blend_corner);
    Kernel {
        fixes: f,
        ..Kernel::default()
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

/// What the rounds made.
#[derive(Debug)]
struct Made {
    /// The set-back corner's type.
    corner: FilletCornerType,
    /// The faces marked as its patch.
    patch: usize,
    /// Faces, edges and vertices after the stitch.
    counts: (usize, usize, usize),
    volume: f64,
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        get_faces(b).unwrap().len(),
        get_edges(b).unwrap().len(),
        b.vertices().ids().count(),
    )
}

/// SolidCraft's `blend_edges` with circular rounds: the edges at `at` rounded `radius`, the
/// vertex at `corner` marked a set-back vertex and its corner set back `setback`; with
/// `mark`, the corner's patch marked. Refused: the error noted.
fn round_set_back(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[[f64; 3]],
    radius: f64,
    corner: [f64; 3],
    setback: f64,
    mark: bool,
) -> Result<(FilletCornerType, usize), (i32, String)> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let radii = vec![radius; edges.len()];
    let v = vertex_at(b, corner);
    let mut s = FilletSession::new(k, b).unwrap();
    s.prepare_per_edge_constant_radius(
        k,
        &edges,
        &radii,
        &[FilletSurfaceGeneratorType::Circular],
        1,
        1.0,
    )
    .unwrap();
    s.add_set_back_vertex(v);
    let refused = |s: &FilletSession<'_>| {
        let info = s.error_info();
        (info.code, info.comment.clone())
    };
    if s.create_corners(k, None).is_err() {
        return Err(refused(&s));
    }
    let mut kind = None;
    for (i, c) in s.fillet_corners().unwrap().into_iter().enumerate() {
        if s.corner(c).unwrap().vertex == Some(v) {
            kind = Some((i, s.corner(c).unwrap().corner_type()));
            s.set_corner_setback(c, setback).unwrap();
        }
    }
    let (index, kind) = kind.expect("the set-back vertex's corner");
    s.set_mark_corner_patches(mark);
    if s.do_filleting(k, None).is_err() {
        return Err(refused(&s));
    }
    let patch = s
        .corner_patch_faces()
        .unwrap()
        .into_iter()
        .filter(|&(i, _)| i == index)
        .count();
    Ok((kind, patch))
}

fn finish(k: &mut Kernel, mut b: Brep, kind: FilletCornerType, patch: usize) -> Made {
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    Made {
        corner: kind,
        patch,
        counts: counts(&b),
        volume: volume(k, &mut b),
    }
}

/// A box `l x w x h` at the origin, the three edges at its corner `(l, w, h)` (picked at
/// `at`) rounded `radius`, set back `setback`.
fn box_corner(
    l: f64,
    w: f64,
    h: f64,
    at: &[[f64; 3]],
    radius: f64,
    setback: f64,
    mark: bool,
) -> Made {
    let mut k = solidcraft_fixes();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), l, w, h).unwrap();
    let (kind, patch) =
        round_set_back(&mut k, &mut b, at, radius, [l, w, h], setback, mark).unwrap();
    finish(&mut k, b, kind, patch)
}

/// The bar running into the post: a 40 x 16 x 10 bar whose far end is a half disc R8 about
/// (40, 8), joined to the R8 post about it 20 high, its top corner (0, 0, 10) rounded r 2
/// on its three edges, set back 3, its patch marked.
fn bar_into_post(k: &mut Kernel) -> (Brep, FilletCornerType, usize) {
    let bar = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 16.0, 10.0).unwrap();
    let post = create_cylinder(k, Point3::new(40.0, 8.0, 0.0), 8.0, 20.0).unwrap();
    let mut b = boolean_union(k, bar, post).unwrap();
    let at = [[0.0, 0.0, 4.0], [20.0, 0.0, 10.0], [0.0, 8.0, 10.0]];
    let (kind, patch) = round_set_back(k, &mut b, &at, 2.0, [0.0, 0.0, 10.0], 3.0, true).unwrap();
    (b, kind, patch)
}

fn assert_made(
    name: &str,
    m: &Made,
    corner: FilletCornerType,
    patch: usize,
    counts: (usize, usize, usize),
    v: f64,
) {
    assert_eq!(m.corner, corner, "{name}");
    assert_eq!(m.patch, patch, "{name}");
    assert_eq!(m.counts, counts, "{name}");
    assert!(
        (m.volume - v).abs() <= 1e-9 * v,
        "{name}: {} != {v}",
        m.volume
    );
}

const BOX_EDGES: [[f64; 3]; 3] = [[30.0, 40.0, 20.0], [60.0, 20.0, 20.0], [60.0, 40.0, 10.0]];

/// ladder 07-fillets/013-corner-setback: three r5 rounds at the box corner (60, 40, 20),
/// set back 8, filled (rig: V 47325.531677, 15 faces, 36 edges; the volume here at 1e-12
/// is 9e-13 from the rig's).
#[test]
fn ladder_013_corner_setback_builds_its_setback() {
    let m = box_corner(60.0, 40.0, 20.0, &BOX_EDGES, 5.0, 8.0, false);
    assert_made(
        "013",
        &m,
        FilletCornerType::NByNConvex,
        0,
        (15, 36, 23),
        47325.53167668245,
    );
}

/// ladder 07-fillets/015-corner-g2-patch and fillets/9-fillet-corner-r5-curvature-default-
/// setback (the same calls): three r5 rounds at the box corner, the G2 corner's default
/// setback 1.5 r = 7.5, its patch marked (rig, with SolidCraft's G2 patch in place of the
/// marked faces: V 47370.105567, 10 faces, 24 edges).
#[test]
fn ladder_015_and_9_g2_corner_patch_is_marked() {
    let m = box_corner(60.0, 40.0, 20.0, &BOX_EDGES, 5.0, 7.5, true);
    assert_made(
        "015",
        &m,
        FilletCornerType::NByNConvex,
        6,
        (15, 36, 23),
        47334.86499451741,
    );
}

/// fillets/every-box-corner-three-edges-r3-curvature-setback-3-5: a 40 x 30 x 20 box's
/// corner (40, 30, 20), three r3 rounds, the G2 corner set back 3.5, its patch marked (rig:
/// V 23830.420377, 10 faces, 24 edges).
#[test]
fn every_box_corner_r3_setback_3_5_patch_is_marked() {
    let at = [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]];
    let m = box_corner(40.0, 30.0, 20.0, &at, 3.0, 3.5, true);
    assert_made(
        "every-box-corner",
        &m,
        FilletCornerType::NByNConvex,
        6,
        (15, 36, 23),
        23826.81119824473,
    );
}

/// fillets/report-fillet-g2-corner-of-bar-running-into-cylinder (rig: V 9362.194188, 11
/// faces, 27 edges).
#[test]
fn bar_into_post_g2_corner_patch_is_marked() {
    let mut k = solidcraft_fixes();
    let (b, kind, patch) = bar_into_post(&mut k);
    let m = finish(&mut k, b, kind, patch);
    assert_made(
        "bar",
        &m,
        FilletCornerType::NByNConvex,
        6,
        (16, 39, 25),
        9359.757405169972,
    );
}

/// fillets/report-fillet-g2-corner-of-bar-running-into-cylinder-outside-setback: the bar's
/// corner less the cube [-1, 3] x [-1, 3] x [7, 11] about it, which takes the whole patch
/// away: the three rounds outside it (rig: V 9342.029547, 13 faces, 33 edges).
#[test]
fn bar_into_post_outside_the_setback_cube() {
    let mut k = solidcraft_fixes();
    let (mut b, kind, patch) = bar_into_post(&mut k);
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let cube = create_box(&mut k, Point3::new(-1.0, -1.0, 7.0), 4.0, 4.0, 4.0).unwrap();
    let cut = boolean_difference(&mut k, b, cube).unwrap();
    let m = finish(&mut k, cut, kind, patch);
    assert_made(
        "outside",
        &m,
        FilletCornerType::NByNConvex,
        6,
        (13, 33, 22),
        9342.029547320612,
    );
}

/// ladder 07-fillets/019-concave-corner-g2-patch: the 60 x 40 x 20 box less the notch
/// [40, 70] x [20, 50] x [10, 30], the notch's three concave edges at its inner corner
/// (40, 20, 10) rounded r5, the G2 corner set back 7.5, its patch marked (rig: V
/// 44254.341161, 13 faces, 33 edges).
#[test]
fn ladder_019_concave_corner_g2_patch_is_marked() {
    let mut k = solidcraft_fixes();
    let bx = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let notch = create_box(&mut k, Point3::new(40.0, 20.0, 10.0), 30.0, 30.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, bx, notch).unwrap();
    let at = [[50.0, 20.0, 10.0], [40.0, 30.0, 10.0], [40.0, 20.0, 15.0]];
    let (kind, patch) =
        round_set_back(&mut k, &mut b, &at, 5.0, [40.0, 20.0, 10.0], 7.5, true).unwrap();
    let m = finish(&mut k, b, kind, patch);
    assert_made(
        "019",
        &m,
        FilletCornerType::NByNConvex,
        6,
        (18, 45, 29),
        44289.58139495591,
    );
}

/// `(faces, edges, vertices)` of a body, and whether it is a closed solid.
type Sheet = ((usize, usize, usize), bool);

/// SolidCraft's G2 corners asked as the blend corner's open Setback instead: each set-back
/// vertex marked, `set_set_back_fill(vertex, setback, ease, open = true)` before the corners
/// are made, the patches marked. The corner is the Setback; the fill's patches (6 faces)
/// are made and marked only for a G2 fill, which an ease (`ease > 0`) or curvature-continuous
/// sections ask for. A G1 corner (circular rounds, no ease) is the open hole (ef57570): a
/// sheet, no patch. `(faces, edges, vertices)` after the stitch, and whether it is a solid,
/// or `None` where refused.
#[allow(clippy::too_many_arguments)] // test helper
fn open_fill(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[[f64; 3]],
    corner: [f64; 3],
    radius: f64,
    setback: f64,
    generator: FilletSurfaceGeneratorType,
    ease: f64,
) -> (usize, Option<Sheet>) {
    use solidcore::blend::blend_corner::BlendCornerEvent;
    use solidcore::blend::fillet_section::section::GN_THUMBWEIGHT_EQUAL_AREA;
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let radii = vec![radius; edges.len()];
    let v = vertex_at(b, corner);
    let (ok, patch) = {
        let mut s = FilletSession::new(k, b).unwrap();
        let (continuity, thumbweight) = if generator == FilletSurfaceGeneratorType::BlendCurve {
            (2, GN_THUMBWEIGHT_EQUAL_AREA)
        } else {
            (1, 1.0)
        };
        s.prepare_per_edge_constant_radius(
            k,
            &edges,
            &radii,
            &[generator],
            continuity,
            thumbweight,
        )
        .unwrap();
        s.add_set_back_vertex(v);
        s.set_set_back_fill(v, setback, ease, true);
        s.create_corners(k, None).unwrap();
        let mut event = None;
        for c in s.fillet_corners().unwrap() {
            if s.corner(c).unwrap().vertex == Some(v) {
                event = s.corner(c).unwrap().blend.map(|b| b.event);
            }
        }
        assert_eq!(event, Some(BlendCornerEvent::Setback));
        s.set_mark_corner_patches(true);
        let ok = s.do_filleting(k, None).is_ok();
        let patch = if ok {
            s.corner_patch_faces().unwrap().len()
        } else {
            0
        };
        (ok, patch)
    };
    let made = ok.then(|| {
        stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
        (counts(b), is_manifold_solid(b).unwrap())
    });
    (patch, made)
}

/// jyviko's question on #2365: the call sequence that makes a three-round box corner the
/// open G2 Setback with its patches handed back is `set_set_back_fill(vertex, setback, ease,
/// true)` with an ease (`ease > 0`), or with curvature-continuous sections; with circular
/// rounds and no ease it is G1, the open hole, with no patch to hand back. The bar's corner
/// beside the post and the notch's concave corner the same (the bar with no ease is refused
/// `NoRoom`: #2503's trim, `fork_smlib125_open_setbacks`).
#[test]
fn open_setback_with_an_ease_is_the_g2_setback_with_its_patch_marked() {
    use FilletSurfaceGeneratorType::{BlendCurve, Circular};
    let boxed =
        |k: &mut Kernel| create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let corner = [60.0, 40.0, 20.0];
    for (generator, ease, want) in [
        (Circular, 0.0, (0, Some(((9, 27, 19), false)))),
        (Circular, 2.0, (6, Some(((15, 36, 23), true)))),
        (BlendCurve, 0.0, (6, Some(((15, 36, 23), true)))),
    ] {
        let mut k = solidcraft_fixes();
        let mut b = boxed(&mut k);
        let got = open_fill(
            &mut k, &mut b, &BOX_EDGES, corner, 5.0, 7.5, generator, ease,
        );
        assert_eq!(got, want, "box {generator:?} ease {ease}");
    }
    let mut k = solidcraft_fixes();
    let bar = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 16.0, 10.0).unwrap();
    let post = create_cylinder(&mut k, Point3::new(40.0, 8.0, 0.0), 8.0, 20.0).unwrap();
    let mut b = boolean_union(&mut k, bar, post).unwrap();
    let at = [[0.0, 0.0, 4.0], [20.0, 0.0, 10.0], [0.0, 8.0, 10.0]];
    let got = open_fill(
        &mut k,
        &mut b,
        &at,
        [0.0, 0.0, 10.0],
        2.0,
        3.0,
        Circular,
        1.0,
    );
    assert_eq!(got, (6, Some(((16, 39, 25), true))), "bar");
    let mut k = solidcraft_fixes();
    let bx = boxed(&mut k);
    let notch = create_box(&mut k, Point3::new(40.0, 20.0, 10.0), 30.0, 30.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, bx, notch).unwrap();
    let at = [[50.0, 20.0, 10.0], [40.0, 30.0, 10.0], [40.0, 20.0, 15.0]];
    let got = open_fill(
        &mut k,
        &mut b,
        &at,
        [40.0, 20.0, 10.0],
        5.0,
        7.5,
        Circular,
        2.0,
    );
    assert_eq!(got, (6, Some(((18, 45, 29), true))), "notch");
}

/// The box corner (60, 40, 20), three r5 rounds, asked a fill of 8 with
/// `set_set_back_fill` (closed), with or without `add_set_back_vertex`: the corner's event,
/// and the body's counts and volume, or the error noted.
fn fill_asked(
    mark_vertex: bool,
) -> (
    Option<solidcore::blend::blend_corner::BlendCornerEvent>,
    Result<Made, (i32, String)>,
) {
    let mut k = solidcraft_fixes();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let edges: Vec<EdgeId> = BOX_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
    let v = vertex_at(&b, [60.0, 40.0, 20.0]);
    let (event, kind, done) = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &edges,
            &[5.0; 3],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        if mark_vertex {
            s.add_set_back_vertex(v);
        }
        s.set_set_back_fill(v, 8.0, 0.0, false);
        let mut done = s.create_corners(&k, None).is_ok();
        let (mut event, mut kind) = (None, None);
        if done {
            for c in s.fillet_corners().unwrap() {
                let corner = s.corner(c).unwrap();
                if corner.vertex == Some(v) {
                    event = corner.blend.map(|b| b.event);
                    kind = Some(corner.corner_type());
                }
            }
            done = s.do_filleting(&mut k, None).is_ok();
        }
        let info = s.error_info();
        (
            event,
            kind,
            done.then_some(())
                .ok_or_else(|| (info.code, info.comment.clone())),
        )
    };
    (
        event,
        done.map(|()| finish(&mut k, b, kind.expect("the corner"), 0)),
    )
}

/// A fill asked with `set_set_back_fill` alone keeps the vertex from the Ball, as
/// `add_set_back_vertex` does (jyviko's and `CodeRabbit`'s review of #2365): three equal
/// rounds at a box corner are the Setback by either call, the same body. Before, the fill
/// alone left the vertex to the Ball, which builds no setback, and the corner was refused
/// `CornerUnsupported`.
#[test]
fn a_fill_asked_alone_is_the_setback_as_with_the_vertex_marked() {
    use solidcore::blend::blend_corner::BlendCornerEvent;
    let (event, alone) = fill_asked(false);
    assert_eq!(event, Some(BlendCornerEvent::Setback));
    let alone = alone.unwrap();
    let (event, marked) = fill_asked(true);
    assert_eq!(event, Some(BlendCornerEvent::Setback));
    let marked = marked.unwrap();
    assert_eq!(alone.counts, marked.counts);
    assert_eq!(alone.volume.to_bits(), marked.volume.to_bits());
    assert_eq!(alone.counts.0, 6 + 3 + 6); // the box's faces, the rounds, the fill's six
}
