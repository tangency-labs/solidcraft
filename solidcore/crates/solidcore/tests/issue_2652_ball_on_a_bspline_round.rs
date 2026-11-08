//! #2652 (solidcraft#11848; the generic blend, hub #2243): a round whose ball is as wide as
//! a face it rolls on, where that face is a B-spline. The rule (jyviko on #2659): the event
//! holds only where the face's radius across the edge is the ball's along the whole edge;
//! a face as wide as the ball along part of the edge only is refused, naming the smallest
//! size that fits.
//!
//! - **Along the whole edge.** The ball's offset of that face collapses onto its focal
//!   curve, which the other face's offset crosses at one point: the ball's centre does not
//!   move and the round is a piece of that one ball (#2403's ball triangle, read before only
//!   off a cylinder or torus). The part: a 10 x 10 x 6 block (z from -6 to 0) whose upright
//!   edge at (10, 10) is a quarter round of radius 2 about (8, 8), built as a plain rational
//!   B-spline wall (cubic in z over three spans). Its bottom row's middle point is pushed
//!   out 0.5 along the diagonal, so no cylinder is read off the wall, while its top two
//!   spans (z from -4 to 0) are exactly the quarter cylinder. Its top outline R2 failed with
//!   1001 before; now it is the ball's piece about (8, 8, -2), and the part is the box's
//!   whose upright round is an exact cylinder, to 1e-12 of the volume removed.
//! - **Along part of the edge.** The same block with a corner of radius 2 for half its turn
//!   and 2.5 for the rest: refused (`OffsetCollapses`), naming 2 less the fit margin. The
//!   twisted disc's side rounds, two of which miss 2 mm by up to 1.8e-3, are refused the same
//!   way, before the trace that failed with 1001.
//! - **Nowhere.** The B-spline block at R1.5 rounds as the exact cylinder's box does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::{create_face_from_surface, fillet_edges};
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::curve::curve::Curve;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::tol;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{create_planar_faces_with_3d_curves, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId, FaceId};
use solidcore::{Brep, Kernel, SolidError};

/// SolidCraft's fix list (`tests/kernel_cases/fixes.json`).
fn kernel() -> Kernel {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in &names {
        if let Some(flag) = f.flag_mut(name) {
            *flag = true;
        }
    }
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

/// The block's rows in z: cubic over three spans, at their Greville abscissae, z = -6 + 6 g.
const ROWS: [f64; 6] = [0.0, 1.0 / 9.0, 1.0 / 3.0, 2.0 / 3.0, 8.0 / 9.0, 1.0];

/// How far the bottom row's middle point of the round wall is pushed out.
const BULGE: f64 = 0.5;

/// The control points of `p` on row `j` (at `z`): a quarter round's middle point pushed out
/// on the bottom row.
fn row(p: &Piece, j: usize, z: f64) -> Vec<Point3> {
    p.points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let d = if j == 0 && p.degree == 2 && p.points.len() == 3 && i == 1 {
                BULGE
            } else {
                0.0
            };
            Point3::new(q.x + d, q.y + d, z)
        })
        .collect()
}

/// The wall over `p`: `p` along u, z along v.
fn wall(p: &Piece) -> Surface {
    let mut pts = Vec::new();
    let mut weights = Vec::new();
    let rows: Vec<Vec<Point3>> = ROWS
        .iter()
        .enumerate()
        .map(|(j, g)| row(p, j, -6.0 + 6.0 * g))
        .collect();
    for i in 0..p.points.len() {
        for r in &rows {
            pts.push(r[i]);
            weights.push(p.weights.as_ref().map_or(1.0, |w| w[i]));
        }
    }
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: p.degree,
            v_degree: 3,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &p.mults,
            v_multiplicities: &[4, 1, 1, 4],
            u_knots: &p.knots,
            v_knots: &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: p.weights.as_ref().map(|_| weights.as_slice()),
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// The block's outline, counter-clockwise: the quarter round about (8, 8) at its corner.
fn outline() -> Vec<Piece> {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    vec![
        line(p(0.0, 0.0), p(10.0, 0.0)),
        line(p(10.0, 0.0), p(10.0, 8.0)),
        arc(
            p(8.0, 8.0),
            2.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            p(10.0, 8.0),
            p(8.0, 10.0),
        ),
        line(p(8.0, 10.0), p(0.0, 10.0)),
        line(p(0.0, 10.0), p(0.0, 0.0)),
    ]
}

/// A block's outline whose corner is one B-spline of two arcs: radius 2 about (8, 8) for its
/// first 45 degrees, then radius 2.5, tangent there, to the back wall (y = 10 + h).
fn two_radius_outline() -> Vec<Piece> {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let d = std::f64::consts::FRAC_1_SQRT_2;
    let q = std::f64::consts::FRAC_PI_4;
    let at45 = p(8.0 + 2.0 * d, 8.0 + 2.0 * d);
    let c2 = p(8.0 - 0.5 * d, 8.0 - 0.5 * d);
    let end = p(c2.x, c2.y + 2.5);
    let a = arc(p(8.0, 8.0), 2.0, 0.0, q, p(10.0, 8.0), at45);
    let b = arc(c2, 2.5, q, 2.0 * q, at45, end);
    let mut points = a.points.clone();
    points.extend_from_slice(&b.points[1..]);
    let mut weights = a.weights.clone().unwrap();
    weights.extend_from_slice(&b.weights.clone().unwrap()[1..]);
    let corner = Piece {
        degree: 2,
        points,
        weights: Some(weights),
        knots: vec![0.0, 1.0, 2.0],
        mults: vec![3, 2, 3],
    };
    vec![
        line(p(0.0, 0.0), p(10.0, 0.0)),
        line(p(10.0, 0.0), p(10.0, 8.0)),
        corner,
        line(end, p(0.0, end.y)),
        line(p(0.0, end.y), p(0.0, 0.0)),
    ]
}

/// The block with its B-spline round wall.
fn block(k: &mut Kernel) -> Brep {
    block_of(k, &outline())
}

/// The block of `pieces`, 6 high.
fn block_of(k: &mut Kernel, pieces: &[Piece]) -> Brep {
    let mut b =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    for p in pieces {
        create_face_from_surface(k, &mut b, &wall(p)).unwrap();
    }
    for (j, z) in [(0, -6.0), (ROWS.len() - 1, 0.0)] {
        let cap: Vec<Curve> = pieces
            .iter()
            .map(|p| {
                let pts = row(p, j, z);
                curve_of(
                    &Piece {
                        degree: p.degree,
                        points: pts,
                        weights: p.weights.clone(),
                        knots: p.knots.clone(),
                        mults: p.mults.clone(),
                    },
                    |q| q,
                )
            })
            .collect();
        with_face_env(k, &mut b, |k, b, fenv| {
            let tol = b.tolerance();
            let region = b.infinite_region();
            create_planar_faces_with_3d_curves(k, b, region, &cap, tol, fenv).unwrap();
        });
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    b
}

/// The box with the same outline: its upright edge at (10, 10) rounded R2, an exact
/// cylinder.
fn reference(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, -6.0), 10.0, 10.0, 6.0).unwrap();
    let upright: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            ends(&b, e)
                .iter()
                .all(|p| (p.x - 10.0).abs() < 1e-9 && (p.y - 10.0).abs() < 1e-9)
        })
        .collect();
    assert_eq!(upright.len(), 1);
    fillet_edges(k, &mut b, &upright, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

fn top_outline(b: &Brep) -> Vec<EdgeId> {
    b.edges()
        .ids()
        .filter(|&e| ends(b, e).iter().all(|p| p.z.abs() < 1e-9))
        .collect()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// The volume the top outline's round R2 removes, and the part after it.
fn round_top(k: &mut Kernel, b: Brep) -> solidcore::Result<(f64, Brep)> {
    round_top_at(k, b, 2.0)
}

/// The volume the top outline's round of radius `r` removes, and the part after it.
fn round_top_at(k: &mut Kernel, mut b: Brep, r: f64) -> solidcore::Result<(f64, Brep)> {
    let before = volume(k, &mut b);
    let top = top_outline(&b);
    assert_eq!(top.len(), 5);
    fillet_edges(k, &mut b, &top, r, 1, 1, 1.0)?;
    let after = volume(k, &mut b);
    Ok((before - after, b))
}

/// The faces every point of whose (natural) surface is on the ball about `c` of radius 2.
fn faces_on_the_ball(k: &Kernel, b: &Brep, c: Point3) -> Vec<FaceId> {
    b.faces()
        .ids()
        .filter(|&f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let d = s.natural_uv_domain_with_nurb(k).unwrap();
            (0..=4).all(|i| {
                (0..=4).all(|j| {
                    let uv = Point2::new(
                        d.min().x + (d.max().x - d.min().x) * f64::from(i) / 4.0,
                        d.min().y + (d.max().y - d.min().y) * f64::from(j) / 4.0,
                    );
                    (s.evaluate_point(k, uv).unwrap().distance(c) - 2.0).abs() <= 1e-9
                })
            })
        })
        .collect()
}

#[test]
fn the_round_over_a_bspline_round_of_its_radius_is_the_ball_s_piece() {
    let mut k = kernel();
    let b = block(&mut k);
    // the round wall is a B-spline, the ball's radius across its top arc
    let (removed, mut b) = round_top(&mut k, b).unwrap();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
    // the round over the arc: one face on the ball about the fixed centre
    let ball = faces_on_the_ball(&k, &b, Point3::new(8.0, 8.0, -2.0));
    assert_eq!(ball.len(), 1, "{ball:?}");
    // the same part as the box whose upright round is an exact cylinder
    let mut kr = kernel();
    let r = reference(&mut kr);
    let (want, _) = round_top(&mut kr, r).unwrap();
    assert!(
        (removed - want).abs() <= 1e-12 * want,
        "removed {removed}, the cylinder's {want}"
    );
}

/// A ball smaller than the B-spline round (R1.5 against 2): it matches the face nowhere, so
/// nothing collapses and the round is the one the exact cylinder's box gets.
#[test]
fn a_ball_narrower_than_the_bspline_round_rolls_as_on_the_cylinder() {
    let mut k = kernel();
    let b = block(&mut k);
    let (removed, mut b) = round_top_at(&mut k, b, 1.5).unwrap();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let mut kr = kernel();
    let r = reference(&mut kr);
    let (want, _) = round_top_at(&mut kr, r, 1.5).unwrap();
    assert!(
        (removed - want).abs() <= 1e-9 * want,
        "removed {removed}, the cylinder's {want}"
    );
}

/// A B-spline corner of radius 2 for half its turn and 2.5 for the rest: the ball R2 is as
/// wide as the face along part of its top edge only. Refused (`OffsetCollapses`), naming
/// the face's least radius there, 2, less the fit margin (open).
#[test]
fn a_ball_as_wide_as_the_bspline_along_part_of_the_edge_is_refused_with_its_fit() {
    let mut k = kernel();
    let b = block_of(&mut k, &two_radius_outline());
    let margin = 1000.0
        * top_outline(&b)
            .iter()
            .map(|&e| edge::tolerance(&b, e).unwrap())
            .fold(0.0, f64::max);
    match round_top(&mut k, b) {
        Err(SolidError::FilletRefused {
            reason: Some(NoRoomReason::OffsetCollapses),
            fit,
            ..
        }) => {
            assert_eq!(fit.kind, FitKind::Radius);
            let want = 2.0 - margin;
            assert!(
                (fit.size - want).abs() <= 1e-9 * want,
                "fit {}, want {want}",
                fit.size
            );
        }
        other => panic!("expected OffsetCollapses, got {:?}", other.map(|(v, _)| v)),
    }
}

/// The twisted disc of solidcraft#11848 (2 slots), its side edges rounded 2 mm, then its
/// top outline 2 mm: over each side round's top edge the ball is as wide as the side round,
/// but two of the four side rounds miss 2 mm by up to 1.8e-3 between their traced sections
/// (`plan/findings/general--twisted-disc-side-round-fit-and-vertex-gaps.md`), so there the
/// face is as wide as the ball along part of the edge only. Refused (`OffsetCollapses`)
/// before the trace, naming the side round's least radius of curvature along the edge less
/// the fit margin, on a side round's top edge. Before, it failed with 1001 in the trace.
#[test]
fn the_twisted_disc_s_top_round_at_its_side_rounds_radius_is_refused_with_its_fit() {
    let mut k = kernel();
    let mut b = side_rounded_disc(&mut k, 2);
    let top = top_edges(&b);
    assert_eq!(top.len(), 12);
    match fillet_edges(&mut k, &mut b, &top, 2.0, 1, 1, 1.0) {
        Err(SolidError::FilletRefused {
            reason: Some(NoRoomReason::OffsetCollapses),
            fit,
            ..
        }) => {
            assert_eq!(fit.kind, FitKind::Radius);
            // the side round's least radius along its top edge, 1.9988, less the margin
            assert!(
                (fit.size - 1.958_612_597_062_46).abs() <= 1e-9,
                "fit {}",
                fit.size
            );
            let e = fit.edge.unwrap();
            assert!(top.contains(&e));
        }
        other => panic!("expected OffsetCollapses, got {other:?}"),
    }
}

// ---- helpers (as `issue_2648_twisted_disc_top_round.rs`'s) -----------------------------

/// The disc's radius, height and twist (degrees over the height).
const RADIUS: f64 = 30.0;
const HEIGHT: f64 = 15.0;
const TWIST: f64 = 30.0;

fn rotate(p: Point3, a: f64) -> Point3 {
    let (s, c) = a.sin_cos();
    Point3::new(c * p.x - s * p.y, s * p.x + c * p.y, p.z)
}

/// The profile of a disc of radius 30 with `n` V-slots (walls at 45 degrees to the slot's
/// axis, a 2 mm arc at the bottom 2.17 mm into the disc), the first slot's axis along +y.
fn profile(n: usize) -> Vec<Piece> {
    let s2 = std::f64::consts::SQRT_2;
    let centre_y = 27.828_427_124_746_188;
    let a = Point3::new(-4.639_136_501_002_611_5, 29.639_136_501_002_61, 0.0);
    let b = Point3::new(-s2, centre_y - s2, 0.0);
    let c = Point3::new(s2, centre_y - s2, 0.0);
    let d = Point3::new(4.639_136_501_002_611_5, 29.639_136_501_002_61, 0.0);
    let step = std::f64::consts::TAU / n as f64;
    let slots: Vec<[Point3; 4]> = (0..n)
        .map(|k| {
            let t = step * k as f64;
            [rotate(a, t), rotate(b, t), rotate(c, t), rotate(d, t)]
        })
        .collect();
    let q = std::f64::consts::FRAC_PI_4;
    let mut pieces = Vec::new();
    for k in 0..n {
        let t = step * k as f64;
        let [a, b, c, d] = slots[k];
        let centre = rotate(Point3::new(0.0, centre_y, 0.0), t);
        pieces.push(line(a, b));
        pieces.push(arc(centre, 2.0, t - 3.0 * q, t - q, b, c));
        pieces.push(line(c, d));
        // the outer arc from this slot's right wall to the next slot's left wall (clockwise)
        let next = slots[(k + n - 1) % n][0];
        let ad = d.y.atan2(d.x);
        let an = next.y.atan2(next.x);
        let mut an = an;
        while an > ad {
            an -= std::f64::consts::TAU;
        }
        pieces.push(arc(Point3::ZERO, RADIUS, ad, an, d, next));
    }
    pieces
}

/// The twist's v knots, as SolidCraft's sweep makes them (the corpus entry's walls): cubic,
/// 28 spans, the interior knots `k / 30` for `k = 2 .. 28`.
fn twist_knots() -> (Vec<f64>, Vec<usize>) {
    let mut knots = vec![0.0];
    let mut mults = vec![4];
    for k in 2..=28 {
        knots.push(f64::from(k) / 30.0);
        mults.push(1);
    }
    knots.push(1.0);
    mults.push(4);
    (knots, mults)
}

/// The rows of the twist in v: each row the profile turned by the twist at its Greville
/// abscissa and lifted to its height (z exactly linear in v).
fn twist_rows() -> Vec<(f64, f64)> {
    let (knots, mults) = twist_knots();
    let mut flat = Vec::new();
    for (&k, &m) in knots.iter().zip(&mults) {
        flat.extend(std::iter::repeat_n(k, m));
    }
    (0..flat.len() - 4)
        .map(|j| {
            let g = (flat[j + 1] + flat[j + 2] + flat[j + 3]) / 3.0;
            (TWIST.to_radians() * g, HEIGHT * g)
        })
        .collect()
}

fn place(p: Point3, (angle, z): (f64, f64)) -> Point3 {
    let q = rotate(p, angle);
    Point3::new(q.x, q.y, z)
}

fn twisted_wall(p: &Piece) -> Surface {
    let rows = twist_rows();
    let (vk, vm) = twist_knots();
    // u along the profile, v up: control points u-major
    let mut pts = Vec::new();
    let mut weights = Vec::new();
    for (i, &q) in p.points.iter().enumerate() {
        for &row in &rows {
            pts.push(place(q, row));
            weights.push(p.weights.as_ref().map_or(1.0, |w| w[i]));
        }
    }
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: p.degree,
            v_degree: 3,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &p.mults,
            v_multiplicities: &vm,
            u_knots: &p.knots,
            v_knots: &vk,
            knot_type: KnotType::Unspecified,
            weights: p.weights.as_ref().map(|_| weights.as_slice()),
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// The disc with `n` slots swept up 15 with a 30 degree twist.
fn twisted_disc(k: &mut Kernel, n: usize) -> Brep {
    let pieces = profile(n);
    let mut b =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    for p in &pieces {
        create_face_from_surface(k, &mut b, &twisted_wall(p)).unwrap();
    }
    let rows = twist_rows();
    for row in [rows[0], rows[rows.len() - 1]] {
        let cap: Vec<Curve> = pieces
            .iter()
            .map(|p| curve_of(p, |q| place(q, row)))
            .collect();
        with_face_env(k, &mut b, |k, b, fenv| {
            let tol = b.tolerance();
            let region = b.infinite_region();
            create_planar_faces_with_3d_curves(k, b, region, &cap, tol, fenv).unwrap();
        });
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    b
}

/// The side edges where the outer walls meet the slots' walls: the edges running from the
/// bottom to the top whose bottom end is at the disc's radius.
fn side_edges(b: &Brep) -> Vec<EdgeId> {
    b.edges()
        .ids()
        .filter(|&e| {
            let [p, q] = ends(b, e);
            let (lo, hi) = if p.z < q.z { (p, q) } else { (q, p) };
            lo.z.abs() < 1e-6
                && (hi.z - HEIGHT).abs() < 1e-6
                && ((lo.x * lo.x + lo.y * lo.y).sqrt() - RADIUS).abs() < 1e-3
        })
        .collect()
}

/// The top outline: every edge with both ends on the top.
fn top_edges(b: &Brep) -> Vec<EdgeId> {
    b.edges()
        .ids()
        .filter(|&e| ends(b, e).iter().all(|p| (p.z - HEIGHT).abs() < 1e-6))
        .collect()
}

fn side_rounded_disc(k: &mut Kernel, n: usize) -> Brep {
    let mut b = twisted_disc(k, n);
    let sides = side_edges(&b);
    assert_eq!(sides.len(), 2 * n);
    fillet_edges(k, &mut b, &sides, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

/// A profile curve in the XY plane as a (rational) B-spline: its control points, weights
/// (`None`: 1), knots with multiplicities, and degree.
struct Piece {
    degree: usize,
    points: Vec<Point3>,
    weights: Option<Vec<f64>>,
    knots: Vec<f64>,
    mults: Vec<usize>,
}

fn line(a: Point3, b: Point3) -> Piece {
    Piece {
        degree: 1,
        points: vec![a, b],
        weights: None,
        knots: vec![0.0, 1.0],
        mults: vec![2, 2],
    }
}

/// The circular arc about `c` of radius `r` from angle `a0` to `a1` (radians, either
/// way), in segments of at most 90 degrees, from `start` to `end` (its exact ends).
fn arc(c: Point3, r: f64, a0: f64, a1: f64, start: Point3, end: Point3) -> Piece {
    let n = ((a1 - a0).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let d = (a1 - a0) / n as f64;
    let w = (0.5 * d).cos();
    let at = |a: f64, rr: f64| Point3::new(c.x + rr * a.cos(), c.y + rr * a.sin(), 0.0);
    let mut points = vec![start];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0];
    let mut mults = vec![3];
    for i in 0..n {
        let mid = a0 + d * (i as f64 + 0.5);
        points.push(at(mid, r / w));
        weights.push(w);
        if i + 1 == n {
            points.push(end);
        } else {
            points.push(at(a0 + d * (i as f64 + 1.0), r));
        }
        weights.push(1.0);
        knots.push((i + 1) as f64);
        mults.push(if i + 1 == n { 3 } else { 2 });
    }
    Piece {
        degree: 2,
        points,
        weights: Some(weights),
        knots,
        mults,
    }
}

/// The curve of `p`'s control points under `map`.
fn curve_of(p: &Piece, map: impl Fn(Point3) -> Point3) -> Curve {
    let pts: Vec<Point3> = p.points.iter().map(|&q| map(q)).collect();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: p.degree,
            control_points: &pts,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &p.mults,
            knots: &p.knots,
            knot_type: KnotType::Unspecified,
            weights: p.weights.as_deref(),
            trim: None,
        })
        .unwrap(),
    )
}

fn with_face_env<T>(
    k: &mut Kernel,
    b: &mut Brep,
    body: impl FnOnce(&mut Kernel, &mut Brep, &mut FaceBuildEnv<'_, '_>) -> T,
) -> T {
    let mut state = solidcore::topology::brep::p2::ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = solidcore::topology::topology_traverser::Marks::default();
    let this_face = |f: FaceId| solidcore::topology::loopuse::notify_ref(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    body(k, b, &mut fenv)
}

fn ends(b: &Brep, e: EdgeId) -> [Point3; 2] {
    let v0 = edge::vertex(b, e).unwrap().unwrap();
    let v1 = edge::other_vertex(b, e, Some(v0)).unwrap().unwrap();
    [vertex::point(b, v0).unwrap(), vertex::point(b, v1).unwrap()]
}
