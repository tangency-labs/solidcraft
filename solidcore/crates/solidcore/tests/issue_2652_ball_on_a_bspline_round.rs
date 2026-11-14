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
//!   and 2.5 for the rest: refused (`OffsetCollapses`), naming 2 less the fit margin.
//! - **The twisted disc (#13774).** Its 4 twisted side rounds R2 are their definitions, the
//!   walls' 2 mm round between their sections as at them (they were the trace's skinned
//!   B-splines, two of them off 2 mm by up to 1.9e-3, so the disc's top round R2 was
//!   refused as wide as the ball along part of the edge only). Its top round R2 is the
//!   ball's piece over each side round, and its round at the fit that refusal named,
//!   1.9586 (#13773), builds.
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
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::p5::DropPointOptions;
use solidcore::surface::surface::{Surface, SurfaceKind};
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
/// nothing collapses and the round is the one the exact cylinder's box gets. There that
/// round is the exact torus (R 0.5 about the wall's axis, r 1.5); here the wall's top spans
/// are the same quarter cylinder, and the round is its definition with a certified net: at
/// every point the net is within its bound of the torus, and the volumes removed differ by
/// at most the round's area times that bound, plus the integration's 1e-9. (Since #14405
/// the net has 24 knots, not 57, and its bound is 1.05e-6, not 3.86e-6: the volumes moved
/// from 1.6e-8 apart to 3.2e-7, the net from the torus to 1.03e-6.)
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
    let torus = |p: Point3| {
        let rho = ((p.x - 8.0).powi(2) + (p.y - 8.0).powi(2)).sqrt();
        (((rho - 0.5).powi(2) + (p.z + 1.5).powi(2)).sqrt() - 1.5).abs()
    };
    let mut slack = 0.0_f64;
    let ids: Vec<FaceId> = b.faces().ids().collect();
    for f in ids {
        let s = b.face(f).unwrap().surface.as_ref().unwrap().clone();
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            continue;
        };
        let d = pb.net().natural_uv_domain();
        for i in 0..=200 {
            for j in 0..=20 {
                let uv = Point2::new(
                    d.min().x + (d.max().x - d.min().x) * f64::from(i) / 200.0,
                    d.min().y + (d.max().y - d.min().y) * f64::from(j) / 20.0,
                );
                let off = torus(pb.net().evaluate_point(uv).unwrap());
                assert!(
                    off <= pb.bound(),
                    "{off:e} off the torus, bound {:e}",
                    pb.bound()
                );
            }
        }
        let area = solidcore::mass::integrator::properties::face_area(&k, &mut b, f, 1e-9);
        slack += area.unwrap() * pb.bound();
    }
    assert!(slack > 0.0, "no round by its definition");
    assert!(
        (removed - want).abs() <= slack + 1e-9 * want,
        "removed {removed}, the cylinder's {want}, the nets' slack {slack:e}"
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

/// The centre of the ball of radius `r` that `s` lies on, if it does: over a 5 x 5 grid
/// inside its domain, every point x with unit normal n has the same x + r n (or x - r n)
/// to 1e-6.
fn ball_centre(k: &Kernel, s: &Surface, r: f64) -> Option<Point3> {
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut sides: [Vec<Point3>; 2] = [Vec::new(), Vec::new()];
    for i in 1..=5 {
        for j in 1..=5 {
            let uv = Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(i) / 6.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(j) / 6.0,
            );
            let x = s.evaluate_point(k, uv).unwrap();
            let n = s.evaluate_normal(k, uv, Side::Left, Side::Left).ok()?;
            sides[0].push(x + n * r);
            sides[1].push(x - n * r);
        }
    }
    sides.into_iter().find_map(|c| {
        let first = *c.first()?;
        c.iter().all(|p| p.distance(first) <= 1e-6).then_some(first)
    })
}

/// The twisted disc of solidcraft#11848 (2 slots), its side edges rounded 2 mm, then its
/// top outline 2 mm: over each side round's top edge the ball is as wide as the side round.
/// With the side rounds their definitions, the walls' 2 mm rounds everywhere (#13774), the
/// ball is as wide as the side round along the whole edge: its offset of the side round
/// collapses onto the side round's centre line, which the top's offset crosses at one
/// point, and the top round over each side round is a piece of that one ball (#2403's
/// ball triangle). The part is a clean manifold solid. Before, two of the side rounds
/// missed 2 mm by up to 1.9e-3 between their traced sections, so the ball was as wide as
/// them along part of the edge only, and the round was refused (`OffsetCollapses`, naming
/// 1.9586, the side round's least radius of curvature along the edge, 1.9988, less the
/// fit margin; before that, 1001 in the trace).
#[test]
fn the_twisted_disc_s_top_round_at_its_side_rounds_radius_is_the_ball_s_piece() {
    let mut k = kernel();
    let mut b = side_rounded_disc(&mut k, 2);
    let top = top_edges(&b);
    assert_eq!(top.len(), 12);
    fillet_edges(&mut k, &mut b, &top, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
    // a piece of a 2 mm ball over each of the 4 side rounds, its centre 2 below the top
    let balls: Vec<Point3> = b
        .faces()
        .ids()
        .filter_map(|f| ball_centre(&k, b.face(f).unwrap().surface.as_ref().unwrap(), 2.0))
        .collect();
    assert_eq!(balls.len(), 4, "{balls:?}");
    for c in &balls {
        assert!((c.z - (HEIGHT - 2.0)).abs() <= 1e-6, "{c:?}");
    }
}

/// The twisted disc (2 slots), its side edges rounded 2 mm, then its 12 top edges rounded
/// below the side rounds' radius, at 1 and at 1.101826756514697 (#13772): a clean manifold
/// solid. With the side rounds their definitions (#13774), every vertex of the top round
/// lies on its faces to within 3.6e-7. Before, the side rounds' skinned nets missed the
/// walls' 2 mm rounds by up to 1.9e-3 between their sections, the top round's rails and
/// vertices on them took that error, and Level2 reported vertices 2.5e-5 (r = 1) and
/// 5.6e-5 (r = 1.1018) off their faces, over the 2e-5 limit.
#[test]
fn the_twisted_disc_s_top_round_below_its_side_rounds_radius_is_clean() {
    for r in [1.0, 1.101_826_756_514_697] {
        let mut k = kernel();
        let mut b = side_rounded_disc(&mut k, 2);
        let top = top_edges(&b);
        assert_eq!(top.len(), 12);
        fillet_edges(&mut k, &mut b, &top, r, 1, 1, 1.0).unwrap();
        b.refresh_cached_topology();
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "r = {r}: {lines:?}");
        assert!(is_manifold_solid(&b).unwrap(), "r = {r}");
    }
}

/// #13773: the disc's top round at 1.95861259706246, the fit the refusal at R2 named while
/// the side rounds were skinned nets (above). It ran 877 s and failed with a bare 1001 from
/// the crossing-rails refusal (WR-460, since retired), as 1.9 did. With the side rounds
/// their definitions it builds, `check_brep` L2 clean and manifold.
#[test]
fn the_twisted_disc_s_top_round_at_its_old_named_fit_builds() {
    let mut k = kernel();
    let mut b = side_rounded_disc(&mut k, 2);
    let top = top_edges(&b);
    assert_eq!(top.len(), 12);
    fillet_edges(&mut k, &mut b, &top, 1.958_612_597_062_46, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
}

/// The disc's side rounds, each with the two walls of the edge it rounds.
struct SideRounds {
    brep: Brep,
    rounds: Vec<(FaceId, [usize; 2])>,
    walls: Vec<Surface>,
}

/// The disc (2 slots) with its 4 side edges rounded 2 mm. The walls' surfaces are kept from
/// before (the fillet replaces their faces); a round is a face on none of them nor a cap,
/// matched to the side edge whose ends' middle it is nearest, with that edge's walls (the
/// two through both its ends).
fn side_rounds(k: &mut Kernel) -> SideRounds {
    let mut b = twisted_disc(k, 2);
    let sides = side_edges(&b);
    assert_eq!(sides.len(), 4);
    let walls: Vec<Surface> = b
        .faces()
        .ids()
        .map(|f| b.face(f).unwrap().surface.clone().unwrap())
        .filter(|s| matches!(s.kind(), SurfaceKind::BSpline(_)))
        .collect();
    let edges: Vec<(Point3, [usize; 2])> = sides
        .iter()
        .map(|&e| {
            let [p, q] = ends(&b, e);
            let mut by_gap: Vec<(f64, usize)> = walls
                .iter()
                .enumerate()
                .map(|(w, s)| (gap(k, s, p).max(gap(k, s, q)), w))
                .collect();
            by_gap.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(by_gap[1].0 < 1e-6, "{by_gap:?}");
            ((p + q) * 0.5, [by_gap[0].1, by_gap[1].1])
        })
        .collect();
    fillet_edges(k, &mut b, &sides, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    let rounds = b
        .faces()
        .ids()
        .filter_map(|f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            if matches!(s.kind(), SurfaceKind::Plane(_)) {
                return None;
            }
            let d = s.natural_uv_domain_with_nurb(k).unwrap();
            let x = s.evaluate_point(k, (d.min() + d.max()) * 0.5).unwrap();
            if walls.iter().any(|w| gap(k, w, x) < 1e-6) {
                return None;
            }
            let (_, on) = edges
                .iter()
                .min_by(|a, c| a.0.distance(x).total_cmp(&c.0.distance(x)))
                .unwrap();
            Some((f, *on))
        })
        .collect();
    SideRounds {
        brep: b,
        rounds,
        walls,
    }
}

/// The distance from `p` to `s`.
fn gap(k: &Kernel, s: &Surface, p: Point3) -> f64 {
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    s.drop_point::<NoObject>(k, p, &d, &DropPointOptions::default())
        .unwrap()
        .gap
}

/// The distance from `p` to `s` where its foot is inside `s`'s domain (`None` on its
/// boundary: the nearest point of the wall's continuation is past it).
fn gap_inside(k: &Kernel, s: &Surface, p: Point3) -> Option<f64> {
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let drop = s
        .drop_point::<NoObject>(k, p, &d, &DropPointOptions::default())
        .unwrap();
    let uv = drop.result_uv;
    let margin = 1e-6;
    let inside = drop.success
        && uv.x > d.min().x + margin
        && uv.x < d.max().x - margin
        && uv.y > d.min().y + margin
        && uv.y < d.max().y - margin;
    inside.then_some(drop.gap)
}

/// The largest error of a side round against the 2 mm round of its two walls, and the
/// number of points judged: over a 201 x 9 grid (v from a tenth to nine tenths across),
/// the points whose ball has its feet inside both walls. For a point x with unit normal n
/// the ball's centre is x + 2n or x - 2n (the side nearer), 2 from each wall.
fn worst_error(k: &Kernel, r: &SideRounds, round: FaceId, walls: [usize; 2]) -> (f64, usize) {
    let s = r.brep.face(round).unwrap().surface.as_ref().unwrap();
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut worst = 0.0_f64;
    let mut judged = 0;
    for i in 0..=200 {
        for j in 1..=9 {
            let uv = Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(i) / 200.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(j) / 10.0,
            );
            let x = s.evaluate_point(k, uv).unwrap();
            let n = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
            let mut err: Option<f64> = None;
            for sign in [1.0, -1.0] {
                let c = x + n * (2.0 * sign);
                let gaps: Option<Vec<f64>> = walls
                    .iter()
                    .map(|&w| gap_inside(k, &r.walls[w], c))
                    .collect();
                if let Some(gaps) = gaps {
                    let e = gaps.iter().map(|g| (g - 2.0).abs()).fold(0.0, f64::max);
                    err = Some(err.map_or(e, |o: f64| o.min(e)));
                }
            }
            if let Some(e) = err {
                worst = worst.max(e);
                judged += 1;
            }
        }
    }
    (worst, judged)
}

/// The largest distance between a round by its definition and its net (the B-spline
/// surface its certificate encloses, which tessellation and intersections read), over a
/// 1001 x 11 grid; `None` for a round that is not its definition.
fn net_off_definition(k: &Kernel, s: &Surface) -> Option<f64> {
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        return None;
    };
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut worst = 0.0_f64;
    for i in 0..=1000 {
        for j in 0..=10 {
            let uv = Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(i) / 1000.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(j) / 10.0,
            );
            let x = s.evaluate_point(k, uv).unwrap();
            worst = worst.max(x.distance(pb.net().evaluate_point(uv).unwrap()));
        }
    }
    Some(worst)
}

/// #13774: each of the disc's 4 twisted side rounds is the round by its definition (a
/// `ProceduralBlend`), and the rounded disc is a clean manifold solid.
#[test]
fn a_twisted_side_round_is_its_definition() {
    let mut k = kernel();
    let r = side_rounds(&mut k);
    assert_eq!(r.rounds.len(), 4);
    for &(f, _) in &r.rounds {
        let s = r.brep.face(f).unwrap().surface.as_ref().unwrap();
        // the definition, and its net within the round's tolerance of it (on the old
        // boundary of a wall's extension, which is only G1, a net whose stations took one
        // side's series left the definition by 2e-3)
        let net = net_off_definition(&k, s);
        let net = net.unwrap_or_else(|| panic!("{f:?} is not its definition"));
        assert!(net <= 4e-5, "{f:?} net off its definition by {net:e}");
        // and within its certified bound at every one of the 11011 samples
        let bound = s.procedural_blend().unwrap().bound();
        assert!(
            net <= bound,
            "{f:?} net off by {net:e}, over its bound {bound:e}"
        );
    }
    let mut b = r.brep;
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
}

/// #13774: between its traced sections as at them, each side round is the 2 mm round of
/// its walls: the ball's centre off a point of the round is 2 from both walls (it missed
/// by up to 1.8e-3 on two of them, the trace's skinned B-spline exact only at its
/// sections).
#[test]
fn a_twisted_side_round_is_its_walls_round_between_sections() {
    let mut k = kernel();
    let r = side_rounds(&mut k);
    assert_eq!(r.rounds.len(), 4);
    for &(f, walls) in &r.rounds {
        let (worst, judged) = worst_error(&k, &r, f, walls);
        eprintln!("{f:?} {walls:?} worst {worst:e} over {judged} points");
        assert!(judged > 1000, "{f:?} judged {judged}");
        assert!(worst <= 1e-6, "{f:?} worst {worst:e}");
    }
}

/// The twisted disc's top round at 1.10 (narrower than the side rounds it meets, which are
/// only fitted): every edge of the result carries a stored tolerance at least its measured
/// gap to its faces, and every vertex one whose half bounds its gap, so the level-2 check
/// reports no edge-use gaps. Only the vertex/face gaps over the intersection tolerance that
/// the side rounds' fit leaves remain (tangency-labs/solidcraft#13772).
#[test]
fn the_twisted_disc_s_top_round_keeps_tolerances_at_least_its_measured_gaps() {
    let mut k = kernel();
    let mut b = side_rounded_disc(&mut k, 2);
    let top = top_edges(&b);
    fillet_edges(&mut k, &mut b, &top, 1.10, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    for e in solidcore::topology::brep::p1::edges(&b, None).unwrap() {
        let tol = edge::tolerance(&b, e).unwrap();
        for eu in b.edge(e).unwrap().edgeuses.clone() {
            if let Some(g) =
                solidcore::topology::edgeuse::p1::max_edge_face_gap(&k, &mut b, eu, true).unwrap()
            {
                assert!(
                    g.gap_3d <= tol,
                    "edge {e:?}: tolerance {tol:e} under its gap {:e}",
                    g.gap_3d
                );
            }
        }
    }
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let edge_use: Vec<_> = lines.iter().filter(|l| l.contains("SmEdgeuse")).collect();
    assert!(edge_use.is_empty(), "edge-use gaps: {edge_use:#?}");
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
