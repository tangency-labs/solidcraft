//! tangency-labs/solidcore#880 (WR-582): a chamfer of one edge of a smooth rim chain. The
//! fork refused SolidCraft's chamfer of half the elliptical rim a flat cut leaves on its
//! tilted cylinder with "a rail leaves the face it rolls on" (`NO_ROOM`); pinned SMLib
//! refuses it with the generic 1001.
//!
//! SolidCraft's `build::cylinder` (at 6c60f1a9) extrudes `Loop2::circle_from(0, r,
//! SEAM_ANGLE)`: two half-circle arcs from `SEAM_ANGLE = 0.6131` rad, on
//! `Plane::from_normal(base, axis)`, whose x axis is `Z × n` projected. Here the same two
//! arcs (radius 5, from 35.128 to 215.128 degrees and from -144.872 to 35.128) are swept
//! 40 along +Z and capped (`create_linear_sweep`), then placed from (0, 0, -10) with
//! x = (0, 1, 0) and y = n × x for n = (1, 0, 2) / `√5`, and cut by the box above
//! z = 10. The wall is two faces and the rim two half-ellipse edges meeting tangentially
//! at the seam lines. The chamfers use a constant radius solver given a second radius, a
//! linear cross section and bevel corners, at the default fixes.
//!
//! One half edge alone (the issue's pick: offsets from setbacks 1 on the cut face and 2
//! down the wall, as the fork's `sides_offsets` derives them) builds a closed solid with a
//! clean check (4 faces to 7), since the generic blend's sections and checks are the
//! default (WR-685 addendum 2026-11-06); main's default kernel refused it with 1001. The
//! whole chain, both halves in one session, builds a closed solid (4 faces to 6).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point2, Point3, Vec3, PI};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::procedural_blend::ProceduralBlend;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, transform, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId, EdgeuseId};
use solidcore::{Brep, Kernel};

/// SolidCraft's `SEAM_ANGLE`, in radians.
const SEAM_ANGLE: f64 = 0.6131;

/// SolidCraft's tilted cylinder, cut flat at z = 10.
fn cut_cylinder(k: &mut Kernel) -> Brep {
    let seam = SEAM_ANGLE.to_degrees();
    // (`create_arc` takes degrees within [-360, 360]: the second half circle, from
    // seam + 180 to seam + 360, is given as seam - 180 to seam)
    let a = Curve::from(create_arc(Point3::ZERO, 5.0, seam, seam + 180.0).unwrap());
    let b = Curve::from(create_arc(Point3::ZERO, 5.0, seam - 180.0, seam).unwrap());
    let up = Vec3::new(0.0, 0.0, 1.0);
    let mut cylinder = create_linear_sweep(k, &[Some(&a), Some(&b)], up, 40.0, true).unwrap();
    let s = 5.0_f64.sqrt();
    let placement = Axis2Placement::new(
        Point3::new(0.0, 0.0, -10.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-2.0 / s, 0.0, 1.0 / s),
    );
    transform(k, &mut cylinder, &placement, None, false).unwrap();
    let cut = create_box(k, Point3::new(-50.0, -50.0, 10.0), 100.0, 100.0, 40.0).unwrap();
    boolean_difference(k, cylinder, cut).unwrap()
}

/// The edges whose middle lies at z = 10, in edge list order.
fn rims(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let Some(curve) = edge::curve(b, e).unwrap() else {
                return false;
            };
            let mid = edge::interval(b, e).unwrap().evaluate(0.5);
            (curve.evaluate_point(k, mid).unwrap().z - 10.0).abs() < 1e-6
        })
        .collect()
}

/// The chamfer of `edges` (each with its blend edge use) by the offsets `r0` (on each
/// edge use's face) and `r1`: one constant radius solver per edge given a second radius at
/// `SetFilletParameters`' defaults (approximation tolerance 4 times the edge's, 20 and 10
/// degrees), a linear cross section, every corner a bevel; the session is gone before the
/// caller's `StitchAndOrient`.
fn chamfer(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[(EdgeId, EdgeuseId)],
    (r0, r1): (f64, f64),
) -> solidcore::Result<()> {
    let tolerances = edges
        .iter()
        .map(|&(e, _)| edge::tolerance(b, e))
        .collect::<solidcore::Result<Vec<f64>>>()?;
    let mut session = FilletSession::new(k, b)?;
    let generator = session.add_surface_generator(FilletSurfaceGenerator::Linear)?;
    for (&(_, u0), tolerance) in edges.iter().zip(tolerances) {
        let solver = session.new_constant_radius_solver_with_second(
            k,
            u0,
            r0,
            r1,
            4.0 * tolerance,
            20.0 * PI / 180.0,
            10.0 * PI / 180.0,
        )?;
        session.load_solver(k, solver)?;
        session.set_solver_generator(solver, generator)?;
    }
    session.create_corners(k, None)?;
    for corner in session.fillet_corners()? {
        session.set_corner_bevel(corner, true)?;
    }
    session.do_filleting(k, None)
}

/// The face counts before and after and whether the result is a manifold solid.
type Outcome = (usize, usize, bool);

/// `StitchAndOrient` (its status ignored, as `SmApiSetBevelCorners` ignores it) and the
/// outcome.
fn finish(k: &mut Kernel, b: &mut Brep, before: usize) -> Outcome {
    let _ = stitch_and_orient(k, b, &StitchAndOrientOptions::default());
    let after = p1::faces(b, None).unwrap().len();
    (before, after, is_manifold_solid(b).unwrap())
}

/// Both rim halves (the whole smooth rim) chamfered in one session by `offsets`.
fn chamfer_whole(offsets: (f64, f64)) -> solidcore::Result<Outcome> {
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = p1::faces(&b, None).unwrap().len();
    let mut edges = Vec::new();
    for e in rims(&k, &b) {
        edges.push((e, edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap()));
    }
    chamfer(&mut k, &mut b, &edges, offsets)?;
    Ok(finish(&mut k, &mut b, before))
}

/// The whole tangent chain, both halves in one session, builds a closed solid with one
/// chamfer face per half (4 faces to 6), as the C++ cases
/// `issue-880-half-arc-rim-whole-offsets-*` compare.
#[test]
fn issue_880_whole_rim_chamfers() {
    for offsets in [
        (1.0, 1.0),
        (1.443_730_684_954_035_7, 1.443_730_684_954_035_7),
        (1.0, 2.0),
    ] {
        let outcome = chamfer_whole(offsets);
        assert!(
            matches!(outcome, Ok((4, 6, true))),
            "offsets {offsets:?}: {outcome:?}"
        );
    }
}

/// The issue's own pick: one rim half chamfered by the offsets of setbacks 1 on the cut face
/// and 2 down the wall builds a closed solid (4 faces to 7) that SMLib's check passes.
#[test]
fn issue_880_one_half_chamfers() {
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = p1::faces(&b, None).unwrap().len();
    let e = rims(&k, &b)[0];
    let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
    chamfer(&mut k, &mut b, &[(e, u)], (1.0, 2.0)).unwrap();
    assert_eq!(finish(&mut k, &mut b, before), (4, 7, true));
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// The volume of `b`, by the mass integrator.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The first rim half chamfered by the equal offsets the setbacks 1 and 1 give
/// (1.4437..., the goldens' `issue-880-half-arc-rim-0-setbacks-1-1`) builds a closed solid
/// (4 faces to 7) that the check passes, as the setbacks 1 and 2 do. It was refused with
/// 1001: at the half's End on the seam line through (6.78, 4.09, 10), the rail on the wall
/// crossed the End's plane off the wall by more than its zone and less than the rail's
/// tolerance, so the rail end read as off its face (#15906). The End is capped there; at
/// the other seam vertex the chamfer runs on past the vertex and leaves the part through the
/// rim (the global merge cuts it). The material removed lies between two wedges of the same
/// chamfer (constant offsets d from the cut face and the wall), integrated independently in
/// the planes square to the chamfer's spine (converged to 2e-6): the half's wedge between the
/// End planes at both seam vertices (11.609075), which the chamfer contains, and the wedge
/// from the capped End to the plane square to the rim where the run-on leaves the part
/// (19.620616), which contains it, its run-on rising from the chamfer to the rim.
#[test]
fn issue_880_one_half_chamfers_at_equal_setbacks() {
    let d = 1.443_730_684_954_035_7;
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = volume(&k, &mut b);
    let originals = p1::faces(&b, None).unwrap();
    let e = rims(&k, &b)[0];
    // the rim half's start (where the chamfer runs on) and middle, and its wall
    let (v1, mid) = {
        let curve = edge::curve(&b, e).unwrap().unwrap();
        let ivl = edge::interval(&b, e).unwrap();
        (
            curve.evaluate_point(&k, ivl.min()).unwrap(),
            curve.evaluate_point(&k, ivl.evaluate(0.5)).unwrap(),
        )
    };
    let wall = edge::faces(&b, e, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::SurfOfExtrusion(_)
            )
        })
        .unwrap();
    let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
    chamfer(&mut k, &mut b, &[(e, u)], (d, d)).unwrap();
    assert_eq!(finish(&mut k, &mut b, originals.len()), (4, 7, true));
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    // The chamfer's rails, on their closed forms. The ball of radius d touching both faces
    // has its centre d inside the wall (5 - d from the axis, the line through (0, 0, -10)
    // along (1, 0, 2)) and d below the cut face (z = 10 - d). A rail point p on the wall is a
    // centre's foot there, so (p - d rho(p)).z = 10 - d (rho the wall's outward normal); one
    // on the cut face is p = centre + d z, so p - d z lies 5 - d from the axis. A rail is an
    // edge between the chamfer (a face that is neither planar nor a wall's extrusion) and the
    // wall or the cut face (not the End's new planar cap).
    let s5 = 5.0_f64.sqrt();
    let axis = Vec3::new(1.0 / s5, 0.0, 2.0 / s5);
    let origin = Point3::new(0.0, 0.0, -10.0);
    let radial = |q: Point3| {
        let w = q - origin;
        w - axis * w.dot(axis)
    };
    let kind = |f| b.face(f).unwrap().surface.as_ref().unwrap().kind().clone();
    let samples = |edge_id| {
        let curve = edge::curve(&b, edge_id).unwrap().unwrap();
        let ivl = edge::interval(&b, edge_id).unwrap();
        (0..=64)
            .map(|i| {
                curve
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 64.0))
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    // each rail with whether it lies on the cut face
    let mut rails = Vec::new();
    for edge_id in p1::edges(&b, None).unwrap() {
        let faces = edge::faces(&b, edge_id, None).unwrap();
        let chamfer_face = |f: &solidcore::topology::FaceId| {
            !matches!(
                kind(*f),
                SurfaceKind::Plane(_) | SurfaceKind::SurfOfExtrusion(_)
            )
        };
        let Some(other) = faces.iter().find(|f| !chamfer_face(f)) else {
            continue;
        };
        if faces.iter().any(chamfer_face) {
            match kind(*other) {
                SurfaceKind::SurfOfExtrusion(_) => rails.push((edge_id, false)),
                SurfaceKind::Plane(_) if originals.contains(other) => rails.push((edge_id, true)),
                _ => {}
            }
        }
    }
    // Past the rim half's start the chamfer runs on and leaves the part through the rim, off
    // the ball's closed form. Its last section is the plane through the centre of the wall
    // rail's end on the seam line there, square to the spine (spanned by z and rho): every
    // rail point on the half's side of it is checked, the wall rail on the half's own wall
    // (which ends there, nearest the start) the whole length.
    let start = rails
        .iter()
        .filter(|&&(edge_id, on_cut)| {
            !on_cut && edge::faces(&b, edge_id, None).unwrap().contains(&wall)
        })
        .flat_map(|&(edge_id, _)| {
            let ends = samples(edge_id);
            [ends[0], ends[64]]
        })
        .min_by(|p, q| (*p - v1).length().total_cmp(&(*q - v1).length()))
        .unwrap();
    let rho = radial(start).unitize_or_z();
    let centre = start - rho * d;
    let mut normal = Vec3::new(0.0, 0.0, 1.0).cross(rho);
    if (mid - centre).dot(normal) < 0.0 {
        normal = -normal;
    }
    let (mut on_wall, mut on_cut_face) = (0, 0);
    for &(edge_id, on_cut) in &rails {
        for p in samples(edge_id) {
            if (p - centre).dot(normal) < -1e-9 {
                continue;
            }
            let off = if on_cut {
                on_cut_face += 1;
                (radial(p - Vec3::new(0.0, 0.0, d)).length() - (5.0 - d)).abs()
            } else {
                on_wall += 1;
                ((p - radial(p).unitize_or_z() * d).z - (10.0 - d)).abs()
            };
            assert!(
                off <= 4e-5,
                "rail {edge_id:?} off its closed form by {off} at {p:?}"
            );
        }
    }
    assert!(
        on_wall >= 65 && on_cut_face >= 65,
        "{on_wall} {on_cut_face}"
    );
    let removed = before - volume(&k, &mut b);
    assert!(
        removed > 11.609_075 && removed < 19.620_616,
        "removed {removed}"
    );
}

/// The largest sine of the angle between the u tangents on the two sides of an interior
/// knot of `net`, over every such knot and all of v, bounded from the control net (not
/// sampled), for a polynomial net of degree 1 in v whose interior u knots have full
/// multiplicity (the Hermite nets a chamfer is built as). At the knot's row `i` the u
/// tangents are `S-(v) = (1 - v) a0 + v a1` and `S+(v) = (1 - v) b0 + v b1` up to positive
/// factors, `aj = P[i][j] - P[i-1][j]` and `bj = P[i+1][j] - P[i][j]`. Their cross product
/// is quadratic in v, bounded by its Bernstein coefficients; their dot product's
/// Bernstein coefficients are all positive (the tangents never turn back); and each side's
/// length is at least the distance from the origin to its segment. So
/// `sin <= max |c_k| / (|S-|min |S+|min)`.
fn u_knot_g1_sine(net: &BSplineSurface) -> f64 {
    assert!(!net.is_rational());
    assert_eq!(net.degree(SurfParamType::V), 1);
    let p = net.degree(SurfParamType::U);
    let (knots, mults) = net.knots(SurfParamType::U, None).unwrap();
    let cpn = net.control_point_net().unwrap();
    assert_eq!(cpn.v_count, 2);
    let at = |i: usize, j: usize| cpn.points[i * cpn.v_count + j];
    let dist0 = |a: Vec3, b: Vec3| {
        // the distance from the origin to the segment a b
        let d = b - a;
        if d.dot(d) == 0.0 {
            return a.length();
        }
        let t = (-(a.dot(d)) / d.dot(d)).clamp(0.0, 1.0);
        (a + d * t).length()
    };
    let (vd, _) = net.knots(SurfParamType::V, None).unwrap();
    let mut worst = 0.0_f64;
    let mut first = mults[0];
    for (kk, (&u, &m)) in knots.iter().zip(&mults).enumerate().skip(1) {
        if kk + 1 == knots.len() {
            break;
        }
        assert_eq!(m, p, "an interior knot below full multiplicity");
        // (the knot's first place in the full knot vector, less one)
        let i = first - 1;
        first += m;
        // the row interpolates the knot's iso-curve: the indexing is the net's
        let on = net
            .evaluate_point(Point2::new(u, vd[0]))
            .unwrap()
            .distance(at(i, 0));
        assert!(on <= 1e-9, "row {i} is not the knot's: {on:e}");
        let a = [at(i, 0) - at(i - 1, 0), at(i, 1) - at(i - 1, 1)];
        let b = [at(i + 1, 0) - at(i, 0), at(i + 1, 1) - at(i, 1)];
        let dots = [
            a[0].dot(b[0]),
            0.5 * (a[0].dot(b[1]) + a[1].dot(b[0])),
            a[1].dot(b[1]),
        ];
        assert!(
            dots.iter().all(|&d| d > 0.0),
            "knot {u}: the tangents turn back"
        );
        let c = [
            a[0].cross(b[0]).length(),
            (a[0].cross(b[1]) + a[1].cross(b[0])).length() * 0.5,
            a[1].cross(b[1]).length(),
        ];
        let (la, lb) = (dist0(a[0], a[1]), dist0(b[0], b[1]));
        assert!(la > 0.0 && lb > 0.0, "knot {u}: a tangent vanishes");
        worst = worst.max(c.iter().fold(0.0_f64, |x, &y| x.max(y)) / (la * lb));
    }
    worst
}

/// The angle in degrees between the normals of two chamfers by their definitions where
/// `before` ends and `after` starts (at u = `knot` of both), at `t` across: each the
/// cross product of the definition's s and t partials on its own side of the station
/// (`EvaluateDefinition`'s `bBefore`), the dihedral of the edge between them.
fn crease_degrees(
    k: &Kernel,
    before: &ProceduralBlend,
    after: &ProceduralBlend,
    knot: f64,
    t: f64,
) -> f64 {
    let normal = |pb: &ProceduralBlend, side_before: bool| {
        let st = pb.definition_parameters(Point2::new(knot, t));
        let d = pb
            .evaluate_definition_sided(k, st.x, st.y, 1, 1, side_before)
            .unwrap();
        d[1][0].cross(d[0][1])
    };
    let (a, b) = (normal(before, true), normal(after, false));
    (a.dot(b) / (a.length() * b.length()))
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// #16651: the half rim's chamfer with the rounds as their definitions. Where the chamfer
/// runs on past the seam, its wall's foot crosses u = 1 of the wall's extension, its old
/// boundary, where the extension is only G1: the foot's s-derivative solves
/// `(I + r W) F' = c'` and the shape operator W jumps there, so F' does (by 8.46 degrees
/// at the setbacks 1 and 2), and the chord's normal `C x ((1 - w) F1' + w F2')` turns with
/// it. The chamfer by its definition creases there (its one net held a C0 knot,
/// `check_brep`'s Test #4). It is now split there into two chamfers by the definition, one
/// each side, joined by an edge along the chord: closed, clean, every knot inside each face
/// G1 by the control net's bound, one face more than without the flag per crease, and the
/// edge's dihedral, from the definitions' own sided partials, more than the G1 angle at
/// the wall's end (7.78 and 4.78 degrees) and none at the cut face's, whose foot does not
/// jump.
#[test]
fn issue_880_one_half_chamfer_splits_at_its_crease_as_a_definition() {
    let sin_g1 = (0.5_f64).to_radians().sin(); // SM_CONTINUITY_ANGLE
    for offsets in [
        (1.0, 2.0),
        (1.443_730_684_954_035_7, 1.443_730_684_954_035_7),
    ] {
        let built = |definition: bool| {
            let mut k = Kernel::default();
            k.fixes.fillet_round_as_definition = definition;
            let mut b = cut_cylinder(&mut k);
            let originals = p1::faces(&b, None).unwrap();
            let e = rims(&k, &b)[0];
            let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
            chamfer(&mut k, &mut b, &[(e, u)], offsets).unwrap();
            let outcome = finish(&mut k, &mut b, originals.len());
            (k, b, originals, outcome)
        };
        let (mut k, mut b, originals, outcome) = built(true);
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{offsets:?}: {lines:?}");
        assert!(outcome.2, "{offsets:?}: {outcome:?}");
        // the chamfer's faces, each a chamfer by its definition, G1 inside
        let mut chamfers = Vec::new();
        for f in p1::faces(&b, None).unwrap() {
            if originals.contains(&f) {
                continue;
            }
            let s = b.face(f).unwrap().surface.clone().unwrap();
            if let SurfaceKind::ProceduralBlend(pb) = s.kind() {
                assert!(
                    pb.chamfer().is_some(),
                    "{offsets:?}: {f:?} is not a chamfer"
                );
                let sine = u_knot_g1_sine(pb.net());
                assert!(sine <= sin_g1, "{offsets:?}: {f:?} turns by sin {sine:e}");
                let d = pb.net().natural_uv_domain();
                chamfers.push((f, (**pb).clone(), d));
            }
        }
        // where one ends the next starts: the crease, an edge of both
        let mut creases = 0;
        for (fa, a, da) in &chamfers {
            for (fb, b2, db) in &chamfers {
                if da.max().x != db.min().x {
                    continue;
                }
                let shared = p1::edges(&b, None)
                    .unwrap()
                    .into_iter()
                    .filter(|&e| {
                        let faces = edge::faces(&b, e, None).unwrap();
                        faces.contains(fa) && faces.contains(fb)
                    })
                    .count();
                assert_eq!(
                    shared, 1,
                    "{offsets:?}: {fa:?} and {fb:?} share {shared} edges"
                );
                let knot = da.max().x;
                // (the foot on the cut face, a plane, does not jump: no crease at its end)
                let mut angles = Vec::new();
                for i in 0..=4 {
                    let t = da.min().y + (da.max().y - da.min().y) * f64::from(i) / 4.0;
                    let angle = crease_degrees(&k, a, b2, knot, t);
                    eprintln!("{offsets:?}: crease at u {knot} t {t}: {angle} degrees");
                    angles.push(angle);
                }
                let widest = angles.iter().fold(0.0_f64, |x, &y| x.max(y));
                assert!(
                    widest > 0.5,
                    "{offsets:?}: the crease is G1 ({widest} degrees)"
                );
                assert!(
                    angles[0].min(angles[4]) < 1e-6,
                    "{offsets:?}: both feet jump ({angles:?})"
                );
                creases += 1;
            }
        }
        assert!(
            creases >= 1,
            "{offsets:?}: no crease among {} faces",
            chamfers.len()
        );
        // one face more than without the flag (one chamfer face, smoothed across it) for
        // each crease
        let (k0, mut b0, _, off) = built(false);
        assert_eq!(
            outcome.1,
            off.1 + creases,
            "{offsets:?}: {outcome:?} against {off:?}"
        );
        // the volume removed: at equal offsets, between the independently integrated
        // wedges of `issue_880_one_half_chamfers_at_equal_setbacks` (the build without the
        // flag smooths the crease, so it is no reference to an exact volume; its
        // difference is reported only)
        let before = volume(&k, &mut cut_cylinder(&mut Kernel::default()));
        let (v, v0) = (volume(&k, &mut b), volume(&k0, &mut b0));
        eprintln!(
            "{offsets:?}: removed {} ({:e} from the build without the flag)",
            before - v,
            v - v0
        );
        if offsets.0 == offsets.1 {
            let removed = before - v;
            assert!(
                removed > 11.609_075 && removed < 19.620_616,
                "{offsets:?}: removed {removed}"
            );
        }
    }
}
