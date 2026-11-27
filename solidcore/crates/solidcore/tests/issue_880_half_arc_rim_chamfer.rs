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
//! clean check (4 faces to 8: the chamfer is two faces, split at its crease), since the
//! generic blend's sections and checks are the default (WR-685 addendum 2026-11-06);
//! main's default kernel refused it with 1001. The
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
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, transform, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId, EdgeuseId, FaceId};
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
/// and 2 down the wall builds a closed solid (4 faces to 8, the chamfer split at its crease
/// where its wall foot crosses the wall's G1 boundary) that SMLib's check passes.
#[test]
fn issue_880_one_half_chamfers() {
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    let before = p1::faces(&b, None).unwrap().len();
    let e = rims(&k, &b)[0];
    let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
    chamfer(&mut k, &mut b, &[(e, u)], (1.0, 2.0)).unwrap();
    assert_eq!(finish(&mut k, &mut b, before), (4, 8, true));
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
/// (4 faces to 8) that the check passes, as the setbacks 1 and 2 do. It was refused with
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
    assert_eq!(finish(&mut k, &mut b, originals.len()), (4, 8, true));
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
        let chamfer_face = |f: &FaceId| {
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

// ---- #16735: `brep_volume` closes the tolerant edges' gaps ------------------------------
//
// The integrator closes each face onto its edges' curves and vertices with ruled strips
// and corner fans, so the faces bound one closed surface and every volume integral is
// that of the region it bounds, whatever the origin. That region differs from any solid
// whose faces are these faces outside the edges' tolerance tubes and the vertices'
// tolerance balls by at most the tubes' and balls' volume ([`tolerance_volume`]).

/// The B-rep's mass properties about `origin` to `accuracy`.
fn properties_about(
    k: &Kernel,
    b: &mut Brep,
    origin: Point3,
    accuracy: f64,
) -> solidcore::mass::integrator::properties::PreciseProperties {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{
        brep_properties, MassPropertyBehavior, PropertyAttributes,
    };
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_properties(
        k,
        b,
        accuracy,
        origin,
        MassPropertyBehavior::default(),
        &attrs,
    )
    .unwrap()
}

/// The volume the tolerances leave open: every edge's tube, pi tol^2 times its length,
/// and every vertex's ball, 4/3 pi tol^3.
fn tolerance_volume(k: &Kernel, b: &Brep) -> f64 {
    use solidcore::mass::integrator::properties::edge_length;
    use solidcore::topology::vertex;
    let mut v = 0.0;
    for e in p1::edges(b, None).unwrap() {
        let t = edge::tolerance(b, e).unwrap();
        v += PI * t * t * edge_length(k, b, e, 1e-9).unwrap();
    }
    for x in p1::vertices(b, None).unwrap() {
        let t = vertex::tolerance(b, x).unwrap();
        v += 4.0 / 3.0 * PI * t * t * t;
    }
    v
}

/// The part, with the first rim half chamfered by `offsets` when given.
fn part(offsets: Option<(f64, f64)>) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    let mut b = cut_cylinder(&mut k);
    if let Some(offsets) = offsets {
        let before = p1::faces(&b, None).unwrap().len();
        let e = rims(&k, &b)[0];
        let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
        chamfer(&mut k, &mut b, &[(e, u)], offsets).unwrap();
        let _ = finish(&mut k, &mut b, before);
    }
    (k, b)
}

/// The equal offsets the setbacks 1 and 1 give.
const EQUAL: f64 = 1.443_730_684_954_035_7;

/// #16735: the cut cylinder's volume is its closed form, 250 sqrt(5) pi (the disc of
/// radius 5 times the axis length 20 sqrt 5 / 2 at its centre), to the volume its
/// tolerances leave open plus the integrator's requested accuracy (1e-10 of the volume,
/// requested, not certified). It read 3.6e-4 low before: x / 3 leaked through every
/// edge's gap.
#[test]
fn issue_16735_the_original_part_is_its_closed_form() {
    let (k, mut b) = part(None);
    let exact = 250.0 * 5.0_f64.sqrt() * PI;
    let open = tolerance_volume(&k, &b);
    let v = volume_to(&k, &mut b, 1e-10);
    eprintln!(
        "the original part: {v} against {exact} ({:e}), open {open:e}",
        v - exact
    );
    assert!(
        (v - exact).abs() <= open + 1e-10 * exact,
        "{v} against {exact}"
    );
}

/// [`volume`] to `accuracy`.
fn volume_to(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

/// #16735: the volume and the first moments about the world origin do not depend on the
/// origin the integrals are taken about, checked on `offsets`' part.
fn volume_does_not_depend_on_the_origin(offsets: Option<(f64, f64)>) {
    let (k, mut b) = part(offsets);
    let mut seen: Vec<(f64, Vec3)> = Vec::new();
    for o in [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, -100.0),
        Point3::new(37.0, -20.0, 55.0),
    ] {
        let p = properties_about(&k, &mut b, o, 1e-10);
        // the first moments about o, moved to the world origin
        seen.push((p.volume, p.moments[4] + o * p.volume));
    }
    let (v0, m0) = seen[0];
    for &(v, m) in &seen[1..] {
        eprintln!(
            "{offsets:?}: volume {v} against {v0} ({:e}); first moments {:e} apart",
            v - v0,
            (m - m0).length()
        );
        assert!(
            (v - v0).abs() <= 1e-10 * v0,
            "{offsets:?}: {v} against {v0}"
        );
        assert!(
            (m - m0).length() <= 1e-10 * v0 * 50.0,
            "{offsets:?}: {m:?} against {m0:?}"
        );
    }
}

/// [`volume_does_not_depend_on_the_origin`] on the uncut part. MEASURED: the volumes agree
/// to 1.1e-8 (6e-12 of the volume) at the accuracy 1e-10, the integrator's quadrature, so
/// 1e-10 of the volume has a 15x margin, and the first moments to 2.6e-7 against 1e-10 of
/// the volume times 50 (the part's size), a 30x margin. Before, each origin read the
/// tolerant edges' leak differently.
#[test]
fn issue_16735_the_volume_does_not_depend_on_the_origin() {
    volume_does_not_depend_on_the_origin(None);
}

/// [`volume_does_not_depend_on_the_origin`] on the half-chamfered parts, whose chamfer face
/// is evaluated by its definition. It fails today: at (1, 2) the volume about (0, 0, -100)
/// reads 5.0e-7 below the one about the world origin, and the first moments differ by
/// 2.6e-5 (#17778: suspected to be the clamped integration `Unclamped::where_trims_may_leave`
/// documents for a face kind with no B-spline part). The fitted net these parts were built
/// as before #16580 agreed to 1.9e-8 and 8.5e-7.
#[test]
#[ignore = "#17778: a procedural blend face's volume depends on the integration origin"]
fn issue_16735_the_chamfered_volume_does_not_depend_on_the_origin() {
    for offsets in [(1.0, 2.0), (EQUAL, EQUAL)] {
        volume_does_not_depend_on_the_origin(Some(offsets));
    }
}

/// #16735: the half chamfers' volumes against #16726's by their definitions
/// (1743.4242016449 and 1741.1795726555, converged to 6e-8).
/// The chamfer's faces are the definitions, so its volume is theirs to the volume its
/// tolerances leave open plus that convergence (1e-7). (The build that fitted the
/// definitions to a distance d, bounded by d A, went with `fillet_round_as_definition`,
/// #16580.)
///
/// They read 2.4e-4 to 1.9e-3 low before.
#[test]
fn issue_16735_the_half_chamfers_match_their_definitions() {
    for (offsets, by_definitions) in [
        ((1.0, 2.0), 1_743.424_201_644_9),
        ((EQUAL, EQUAL), 1_741.179_572_655_5),
    ] {
        let (k, mut b) = part(Some(offsets));
        let open = tolerance_volume(&k, &b);
        let v = volume_to(&k, &mut b, 1e-10);
        let bound = open + 1e-7;
        eprintln!(
            "{offsets:?}: {v} against {by_definitions} ({:e}), bound {bound:e}",
            v - by_definitions
        );
        assert!(
            (v - by_definitions).abs() <= bound,
            "{offsets:?}: {v} against {by_definitions}"
        );
    }
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

/// The distance from `x` to the segment from `p` to `q`.
fn segment_distance(x: Point3, p: Point3, q: Point3) -> f64 {
    let e = q - p;
    let t = ((x - p).dot(e) / e.dot(e)).clamp(0.0, 1.0);
    (p + e * t).distance(x)
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
    // atan2 of the sine and the cosine, not the cosine's acos: near 0 the acos of a dot
    // rounded one ulp short of 1 is already 8.5e-7 degrees (two ulps 1.2e-6), so a crease
    // of none read as one at the 1e-6 bound
    a.cross(b).length().atan2(a.dot(b)).to_degrees()
}

/// #16651: the half rim's chamfer with the rounds as their definitions. Where the chamfer
/// runs on past the seam, its wall's foot crosses u = 1 of the wall's extension, its old
/// boundary, where the extension is only G1: the foot's s-derivative solves
/// `(I + r W) F' = c'` and the shape operator W jumps there, so F' does (by 8.46 degrees
/// at the setbacks 1 and 2), and the chord's normal `C x ((1 - w) F1' + w F2')` turns with
/// it. The chamfer by its definition creases there (its one net held a C0 knot,
/// `check_brep`'s Test #4). It is now split there into two chamfers by the definition, one
/// each side, joined by an edge along the chord: closed, clean, every knot inside each face
/// G1 by the control net's bound, one face more per crease than the chamfer as one face,
/// and the edge's dihedral, from the definitions' own sided partials, more than the G1
/// angle at the wall's end (7.78 and 4.78 degrees) and none at the cut face's, whose foot
/// does not jump; and the edge on the definition's ruling. (The volume of the half rim
/// chamfered at these setbacks is certified in the goldens, `blend.fillet_executive_p3`'s
/// `round_definition_rows`: the integrator's converged volume lies in the body's certified
/// enclosure; and by its definitions in `issue_880_one_half_chamfer_volume_by_its_definitions`.)
#[test]
fn issue_880_one_half_chamfer_splits_at_its_crease_as_a_definition() {
    let sin_g1 = (0.5_f64).to_radians().sin(); // SM_CONTINUITY_ANGLE
    for offsets in [
        (1.0, 2.0),
        (1.443_730_684_954_035_7, 1.443_730_684_954_035_7),
    ] {
        let mut k = Kernel::default();
        let mut b = cut_cylinder(&mut k);
        let originals = p1::faces(&b, None).unwrap();
        let e = rims(&k, &b)[0];
        let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
        chamfer(&mut k, &mut b, &[(e, u)], offsets).unwrap();
        let outcome = finish(&mut k, &mut b, originals.len());
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
                let shared: Vec<EdgeId> = p1::edges(&b, None)
                    .unwrap()
                    .into_iter()
                    .filter(|&e| {
                        let faces = edge::faces(&b, e, None).unwrap();
                        faces.contains(fa) && faces.contains(fb)
                    })
                    .collect();
                assert_eq!(
                    shared.len(),
                    1,
                    "{offsets:?}: {fa:?} and {fb:?} share {shared:?}"
                );
                let knot = da.max().x;
                // the edge is the chord there: its middle on the definition's ruling, the
                // segment between its ends (t = 0 and 1) at the knot
                let e = shared[0];
                let curve = edge::curve(&b, e).unwrap().unwrap();
                let mid = curve
                    .evaluate_point(&k, edge::interval(&b, e).unwrap().evaluate(0.5))
                    .unwrap();
                let end = |t: f64| {
                    let st = a.definition_parameters(Point2::new(knot, t));
                    a.evaluate_definition_sided(&k, st.x, st.y, 0, 0, true)
                        .unwrap()[0][0]
                };
                let (p0, p1) = (end(da.min().y), end(da.max().y));
                let off = segment_distance(mid, p0, p1);
                let tolerance = edge::tolerance(&b, e).unwrap();
                assert!(
                    off <= tolerance,
                    "{offsets:?}: the edge is {off:e} off the ruling (tolerance {tolerance:e})"
                );
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
        assert_eq!(
            creases,
            1,
            "{offsets:?}: one crease, where the run-on's wall foot crosses the wall's G1 \
             boundary, among {} faces",
            chamfers.len()
        );
        // one face more for each crease than the 4 faces to 7 the chamfer builds as one face
        assert_eq!(
            outcome,
            (originals.len(), 7 + creases, true),
            "{offsets:?}: {outcome:?} with {creases} creases"
        );
    }
}

// ---- #16725: the volume by the divergence theorem with F = (z - 10) a -------------------
//
// The part's faces are the cut plane z = 10, the wall (the cylinder of radius 5 about the
// axis a = (1, 0, 2) / sqrt 5 through (0, 0, -10)), the bottom cap and the chamfer's new
// faces. F = (z - 10) a has divergence a_z = 2 / sqrt 5; its flux through the cut plane
// is 0 (F = 0 there) and through the wall 0 (a is tangent to it), so with the bottom cap
// alone the original part's volume is 250 sqrt(5) pi, and a chamfered part's is that plus
// (sqrt 5 / 2) times F's flux out of its new faces. A face's tolerant edges then enter
// only through those new faces' own regions: the cut plane's and the wall's trims, which
// the integrator's x / 3 reads, do not enter at all.

/// The cylinder's axis.
fn axis() -> Vec3 {
    Vec3::new(1.0, 0.0, 2.0) * (1.0 / 5.0_f64.sqrt())
}

/// (sqrt 5 / 2) F at `p`: the field whose flux out of a closed surface is its volume.
fn axial_field(p: Point3) -> Vec3 {
    axis() * ((p.z - 10.0) * 0.5 * 5.0_f64.sqrt())
}

/// The original part's volume, 250 sqrt(5) pi: the disc of radius 5 times the axis
/// length from the cap at (0, 0, -10) to z = 10 at its centre.
fn original_volume() -> f64 {
    250.0 * 5.0_f64.sqrt() * PI
}

/// Gauss-Legendre nodes and weights on [-1, 1] (Newton on `P_n` from the Chebyshev guess).
fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for i in 0..n {
        let mut x = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        let mut dp = 0.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for kk in 2..=n {
                let p2 = ((2 * kk - 1) as f64 * x * p1 - (kk - 1) as f64 * p0) / kk as f64;
                p0 = p1;
                p1 = p2;
            }
            dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
            let dx = p1 / dp;
            x -= dx;
            if dx.abs() < 1e-16 {
                break;
            }
        }
        out.push((x, 2.0 / ((1.0 - x * x) * dp * dp)));
    }
    out
}

/// +1 where face `f`'s outward normal is its surface's, else -1: the face use on the
/// solid's material region has the surface's normal (`Same`) on its inner side.
fn outward_sign(b: &Brep, f: FaceId) -> f64 {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    let up = b.face(f).unwrap().faceuse.unwrap();
    let mate = b.faceuse(up).unwrap().mate.unwrap();
    let material = |fu| {
        let r = solidcore::topology::edgeuse::p1::faceuse_region(b, fu)
            .unwrap()
            .unwrap();
        b.region(r).unwrap().is_void() == 0
    };
    let fu = if material(up) { up } else { mate };
    if b.faceuse(fu).unwrap().orientation == OrientType::Same {
        -1.0
    } else {
        1.0
    }
}

/// The trim curves of face `f`, face on their left: in (u, v) or in 3D.
fn trim_curves(k: &Kernel, b: &mut Brep, f: FaceId, uv: bool) -> Vec<Curve> {
    let key_of = |_e: solidcore::error::EntityRef| solidcore::geom::object::ObjectRef {
        type_code: 0,
        key: solidcore::geom::attribute::ObjectKey(0),
    };
    solidcore::topology::face::p1::create_oriented_trim_boundary(k, b, f, uv, uv, None, &key_of)
        .unwrap()
        .curves
}

/// The line integral over `curve` of `integrand(point, tangent)`, `panels` panels of a
/// 16-point Gauss rule in each knot span.
fn along(k: &Kernel, curve: &Curve, panels: usize, integrand: &dyn Fn(Point3, Vec3) -> f64) -> f64 {
    let ivl = curve.natural_interval();
    let (mut breaks, _) = curve.knots(k, None).unwrap_or((Vec::new(), Vec::new()));
    breaks.retain(|&t| t > ivl.min() && t < ivl.max());
    breaks.insert(0, ivl.min());
    breaks.push(ivl.max());
    let rule = gauss_legendre(16);
    let mut sum = 0.0;
    for span in breaks.windows(2) {
        for pnl in 0..panels {
            let t0 = span[0] + (span[1] - span[0]) * pnl as f64 / panels as f64;
            let t1 = span[0] + (span[1] - span[0]) * (pnl + 1) as f64 / panels as f64;
            for &(x, w) in &rule {
                let t = 0.5 * (t0 + t1) + 0.5 * (t1 - t0) * x;
                let e = curve.evaluate(k, t, 1, true, false).unwrap();
                sum += 0.5 * (t1 - t0) * w * integrand(e[0], e[1]);
            }
        }
    }
    sum
}

/// [`trims_flux`]'s precondition, asserted on its face's `surface`: its 4-point rule in v
/// is exact only where `g = F . (S_u x S_v)` is, on each line of constant u, one polynomial
/// of degree 7 or less in v (no knot of v inside). F is affine in the point, so it holds
/// for:
/// - a plane (S affine or bilinear: g of degree 2 or less);
/// - a surface of extrusion along the axis a, the wall: S along its generator is parallel
///   to F, so g = 0;
/// - a B-spline, or a blend evaluated by its net, not rational, of degree q in v with
///   3q - 1 <= 7 and no interior knot of v (g of degree 3q - 1);
/// - a blend evaluated by its definition whose section is the chord (order 0, linear in
///   t) and whose v moves t only (S affine in v on each line: g of degree 2 or less).
///
/// Anything else fails the test.
fn assert_v_exact(k: &Kernel, surface: &Surface) {
    let polynomial = |s: &BSplineSurface| {
        let d = s.natural_uv_domain();
        let (knots, _) = s.knots(SurfParamType::V, None).unwrap();
        let q = s.degree(SurfParamType::V);
        assert!(!s.is_rational(), "trims_flux: a rational surface");
        assert!(3 * q <= 8, "trims_flux: degree {q} in v");
        assert!(
            knots.iter().all(|&t| t <= d.min().y || t >= d.max().y),
            "trims_flux: knots of v inside the domain: {knots:?}"
        );
    };
    match surface.kind() {
        SurfaceKind::Plane(_) => {}
        SurfaceKind::SurfOfExtrusion(e) => {
            use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
            // (the generator curve runs along this parameter; the extrusion along the other)
            let (_, along_curve) = e.gen_dir_param_extent();
            let mid = surface.natural_uv_domain_with_nurb(k).unwrap().mid();
            let d = surface
                .evaluate(k, mid, 1, 1, EvaluateFlags::default())
                .unwrap();
            let extrusion = if along_curve == SurfParamType::U {
                d[0][1]
            } else {
                d[1][0]
            };
            assert!(
                extrusion.cross(axis()).length() <= 1e-12 * extrusion.length(),
                "trims_flux: an extrusion along {extrusion:?}, not the axis"
            );
        }
        SurfaceKind::BSpline(s) => polynomial(s),
        SurfaceKind::ProceduralBlend(pb) if pb.net_answers(k) => polynomial(pb.net()),
        SurfaceKind::ProceduralBlend(pb) => {
            let order = surface.procedural_blend().unwrap().order();
            assert_eq!(order, 0, "trims_flux: a blend's section of order {order}");
            let st = pb.definition_parameters(Point2::new(0.0, 0.0));
            let sv = pb.definition_parameters(Point2::new(0.0, 1.0)) - st;
            assert!(
                sv.x == 0.0 && sv.y != 0.0,
                "trims_flux: v moves the definition's s: {sv:?}"
            );
        }
        _ => panic!("trims_flux: a surface its rule is not known to be exact on"),
    }
}

/// The flux of [`axial_field`] out of face `f` over its trims, by Green's theorem in its
/// (u, v): minus the loop integral of H du, H(u, v) the integral of
/// `g = F . (S_u x S_v)` in v from 0, exact by a 4-point rule where g is a polynomial of
/// degree 7 or less in v ([`assert_v_exact`] asserts it of the face's surface).
fn trims_flux(k: &Kernel, b: &mut Brep, f: FaceId, panels: usize) -> f64 {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    let surface = b.face(f).unwrap().surface.clone().unwrap();
    assert_v_exact(k, &surface);
    let trims = trim_curves(k, b, f, true);
    let g = |u: f64, v: f64| -> f64 {
        let d = surface
            .evaluate(k, Point2::new(u, v), 1, 1, EvaluateFlags::default())
            .unwrap();
        axial_field(d[0][0]).dot(d[1][0].cross(d[0][1]))
    };
    let inner = gauss_legendre(4);
    let h = |u: f64, v: f64| -> f64 {
        let mut sum = 0.0;
        for &(x, w) in &inner {
            sum += w * g(u, 0.5 * v * (1.0 + x));
        }
        0.5 * v * sum
    };
    let mut flux = 0.0;
    for c in &trims {
        flux -= along(k, c, panels, &|q, dq| h(q.x, q.y) * dq.x);
    }
    outward_sign(b, f) * flux
}

/// The flux of [`axial_field`] out of a plane face `f`: F . n times the integral of
/// z - 10 over the face, by Green's theorem in the plane on its 3D edge curves (exact
/// lines and conics to their curves' accuracy), the region's area taken positive.
fn plane_flux(k: &Kernel, b: &mut Brep, f: FaceId) -> f64 {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    let surface = b.face(f).unwrap().surface.clone().unwrap();
    let d = surface
        .evaluate(k, Point2::new(0.0, 0.0), 1, 1, EvaluateFlags::default())
        .unwrap();
    let p0 = d[0][0];
    let n = d[1][0].cross(d[0][1]).unitize().unwrap();
    let e1 = d[1][0].unitize().unwrap();
    let e2 = n.cross(e1);
    let (mut area, mut mx, mut my) = (0.0, 0.0, 0.0);
    for c in &trim_curves(k, b, f, false) {
        let xi = |p: Point3| (p - p0).dot(e1);
        let eta = |p: Point3| (p - p0).dot(e2);
        area += along(k, c, 8, &|p, t| {
            0.5 * (xi(p) * t.dot(e2) - eta(p) * t.dot(e1))
        });
        mx += along(k, c, 8, &|p, t| 0.5 * xi(p) * xi(p) * t.dot(e2));
        my -= along(k, c, 8, &|p, t| 0.5 * eta(p) * eta(p) * t.dot(e1));
    }
    let s = area.signum();
    let integral = s * ((p0.z - 10.0) * area + e1.z * mx + e2.z * my);
    outward_sign(b, f) * 0.5 * 5.0_f64.sqrt() * axis().dot(n) * integral
}

/// Where a ruled patch's region is clipped: inside the cylinder of `radius` about `axis`
/// (a unit vector) through `centre`, and on the side of each cap plane (a point, a normal)
/// the normal points to.
struct Clip {
    centre: Point3,
    axis: Vec3,
    radius: f64,
    caps: Vec<(Point3, Vec3)>,
}

/// Which constraint holds a bound of v: the patch's own end (0), the cylinder (1) or
/// cap i (2 + i).
type Bound = usize;

/// How far in t a constraint must cut into the range to take a bound from the one
/// holding it: an end lying on the cylinder (a chamfer's rail on the wall) puts its root
/// within rounding (2e-15 measured) of the end, which would otherwise hand the bound back
/// and forth between them from one u to the next.
const TIE: f64 = 1e-12;

/// The range of t in [0, 1] along the line from `p0` to `p1` that lies in `clip`, with
/// the constraints holding each end ([`TIE`]); `None` where it is empty.
fn clip_line(clip: &Clip, p0: Point3, p1: Point3) -> Option<(f64, f64, Bound, Bound)> {
    let perp = |w: Vec3| w - clip.axis * w.dot(clip.axis);
    let (mut lo, mut hi, mut lo_by, mut hi_by) = (0.0_f64, 1.0_f64, 0, 0);
    let q0 = perp(p0 - clip.centre);
    let dq = perp(p1 - p0);
    let (al, be, ga) = (
        dq.dot(dq),
        2.0 * q0.dot(dq),
        q0.dot(q0) - clip.radius * clip.radius,
    );
    let disc = be * be - 4.0 * al * ga;
    if disc < 0.0 {
        return None;
    }
    let r = disc.sqrt();
    let (ra, rb) = ((-be - r) / (2.0 * al), (-be + r) / (2.0 * al));
    if ra.min(rb) > lo + TIE {
        (lo, lo_by) = (ra.min(rb), 1);
    }
    if ra.max(rb) < hi - TIE {
        (hi, hi_by) = (ra.max(rb), 1);
    }
    for (i, &(cp, cn)) in clip.caps.iter().enumerate() {
        let (h0, h1) = ((p0 - cp).dot(cn), (p1 - cp).dot(cn));
        if h0 < 0.0 && h1 < 0.0 {
            return None;
        }
        if h0 < 0.0 || h1 < 0.0 {
            let tc = h0 / (h0 - h1);
            if h0 < 0.0 && tc > lo + TIE {
                (lo, lo_by) = (tc, 2 + i);
            } else if h1 < 0.0 && tc < hi - TIE {
                (hi, hi_by) = (tc, 2 + i);
            }
        }
    }
    (hi > lo).then_some((lo, hi, lo_by, hi_by))
}

/// The integral of `g` over the region of a ruled patch `at` (a line in v for each u) on
/// `us` x `vs` that lies in `clip`: u split at `knots` (where the patch is only as smooth
/// as its rails) and wherever a bound of v changes the constraint holding it (found on a
/// 400-cell grid in u and bisected; a constraint taking a bound and handing it back
/// within one cell is not seen). The grid does not change with `n`, so a missed switch
/// is not found by refining it. ARGUED only, and only for a switch that holds across a
/// cell's end: it leaves a kink inside a span, where a Gauss rule converges slowly in
/// `n`, so it would show as a gap between the caller's n = 16, 24 and 32 well above the
/// rule's error elsewhere. A region (or a bound's excursion) that enters and leaves
/// within one cell is NOT covered by that argument: every rule can miss it alike and
/// agree (e.g. at(u, v) = (v, 2000 (u - 0.50125), 0), g = 1, the unit cylinder about z,
/// no caps: the region is u in (0.50075, 0.50175), inside the cell [0.5, 0.5025], and the
/// 16-, 24- and 32-point rules all give 0, not PI / 4000). Nothing here isolates such
/// events from the clipping equations; the volume test's bound on an omission is its
/// independent check against the B-rep's trims (1e-4). v exact by
/// a 4-point rule (`g` must be a polynomial of degree 7 or less in v); u by an `n`-point
/// Gauss rule per span.
fn ruled_flux(
    at: &dyn Fn(f64, f64) -> Point3,
    g: &dyn Fn(f64, f64) -> f64,
    us: (f64, f64),
    vs: (f64, f64),
    clip: &Clip,
    knots: &[f64],
    n: usize,
) -> f64 {
    let range = |u: f64| clip_line(clip, at(u, vs.0), at(u, vs.1));
    let which = |u: f64| range(u).map(|(_, _, l, h)| (l, h));
    let mut breaks = vec![us.0, us.1];
    let grid = 400;
    for i in 0..grid {
        let mut x0 = us.0 + (us.1 - us.0) * f64::from(i) / f64::from(grid);
        let end = us.0 + (us.1 - us.0) * f64::from(i + 1) / f64::from(grid);
        // every change between the cell's ends: bisect to one, then go on from it
        for _ in 0..8 {
            let w0 = which(x0);
            if w0 == which(end) {
                break;
            }
            let mut x1 = end;
            for _ in 0..100 {
                let m = 0.5 * (x0 + x1);
                if which(m) == w0 {
                    x0 = m;
                } else {
                    x1 = m;
                }
            }
            breaks.push(0.5 * (x0 + x1));
            x0 = x1;
        }
    }
    breaks.extend(knots.iter().copied().filter(|&x| x > us.0 && x < us.1));
    breaks.sort_by(f64::total_cmp);
    let inner = gauss_legendre(4);
    let outer = gauss_legendre(n);
    let mut sum = 0.0;
    for span in breaks.windows(2) {
        let (lo, hi) = (span[0], span[1]);
        for &(x, w) in &outer {
            let u = 0.5 * (lo + hi) + 0.5 * (hi - lo) * x;
            let Some((tl, th, _, _)) = range(u) else {
                continue;
            };
            let (vl, vh) = (vs.0 + (vs.1 - vs.0) * tl, vs.0 + (vs.1 - vs.0) * th);
            let mut col = 0.0;
            for &(y, wy) in &inner {
                col += wy * g(u, 0.5 * (vl + vh) + 0.5 * (vh - vl) * y);
            }
            sum += 0.5 * (hi - lo) * w * 0.5 * (vh - vl) * col;
        }
    }
    sum
}

/// [`ruled_flux`] against closed forms: the patch (2v, u, 0) on [0, 0.95] x [0, 1] inside
/// the unit cylinder about z, between the caps x >= 0.1 and x <= 0.6, of g = 1 + v. Its
/// upper bound of v passes from the cap (v = 0.3) to the cylinder (v = sqrt(1 - u^2) / 2)
/// at u = 0.8, where nothing about the bounds' being 1 or not changes; the lower bound is
/// the cap's v = 0.05. The integral of (1 + v) dv from lo to hi is (hi - lo) +
/// (hi^2 - lo^2) / 2, so it is 0.8 (0.25 + 0.04375) plus, on [0.8, 0.95],
/// (A(0.95) - A(0.8)) / 2 - 0.05 (0.15) + ((u - u^3/3) / 8 from 0.8 to 0.95) - 0.00125
/// (0.15), with A(u) = (u sqrt(1 - u^2) + asin u) / 2.
#[test]
fn ruled_flux_matches_closed_forms_across_a_cap_cylinder_switch() {
    let clip = Clip {
        centre: Point3::new(0.0, 0.0, 0.0),
        axis: Vec3::new(0.0, 0.0, 1.0),
        radius: 1.0,
        caps: vec![
            (Point3::new(0.1, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
            (Point3::new(0.6, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)),
        ],
    };
    let at = |u: f64, v: f64| Point3::new(2.0 * v, u, 0.0);
    let g = |_u: f64, v: f64| 1.0 + v;
    let a = |u: f64| 0.5 * (u * (1.0 - u * u).sqrt() + u.asin());
    let cube = |u: f64| (u - u * u * u / 3.0) / 8.0;
    let exact = 0.8 * (0.25 + 0.04375) + 0.5 * (a(0.95) - a(0.8)) - 0.05 * 0.15
        + (cube(0.95) - cube(0.8))
        - 0.00125 * 0.15;
    for n in [16, 24] {
        let got = ruled_flux(&at, &g, (0.0, 0.95), (0.0, 1.0), &clip, &[], n);
        assert!(
            (got - exact).abs() <= 1e-13,
            "n = {n}: {got} against {exact}"
        );
    }
    // and one rule over the whole range, the switch not split, is not
    let whole = {
        let mut sum = 0.0;
        for &(x, w) in &gauss_legendre(24) {
            let u = 0.475 + 0.475 * x;
            let hi = 0.3_f64.min(0.5 * (1.0 - u * u).sqrt());
            sum += 0.475 * w * ((hi - 0.05) + 0.5 * (hi * hi - 0.0025));
        }
        sum
    };
    assert!((whole - exact).abs() > 1e-7, "{whole} against {exact}");
}

/// The flux of [`axial_field`] out of a chamfer face by its definition over its exact
/// region ([`ruled_flux`]): the net's domain where the chord at u lies inside the
/// cylinder and on the part's side of each new plane face beside it (`caps`), u split at
/// the net's knots (the spine's, where the definition is only as smooth as it); g is
/// quadratic in v on a chord, the definition being linear in v.
fn chord_flux(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    pb: &ProceduralBlend,
    caps: &[(Point3, Vec3)],
    n: usize,
) -> f64 {
    let d = pb.net().natural_uv_domain();
    let (vs, us) = ((d.min().y, d.max().y), (d.min().x, d.max().x));
    let at = |u: f64, v: f64| {
        let st = pb.definition_parameters(Point2::new(u, v));
        pb.evaluate_definition(k, st.x, st.y, 0, 0).unwrap()[0][0]
    };
    // each cap's kept side: the side of the face's end farther from it
    let vm = 0.5 * (vs.0 + vs.1);
    let caps = caps
        .iter()
        .map(|&(cp, cn)| {
            let (h0, h1) = ((at(us.0, vm) - cp).dot(cn), (at(us.1, vm) - cp).dot(cn));
            let keep = if h0.abs() > h1.abs() {
                h0.signum()
            } else {
                h1.signum()
            };
            (cp, cn * keep)
        })
        .collect();
    let clip = Clip {
        centre: Point3::new(0.0, 0.0, -10.0),
        axis: axis(),
        radius: 5.0,
        caps,
    };
    let g = |u: f64, v: f64| -> f64 {
        let st = pb.definition_parameters(Point2::new(u, v));
        let su = pb.definition_parameters(Point2::new(u + 1.0, v)) - st;
        let sv = pb.definition_parameters(Point2::new(u, v + 1.0)) - st;
        let dd = pb.evaluate_definition(k, st.x, st.y, 1, 1).unwrap();
        let (ps, pt) = (dd[1][0], dd[0][1]);
        let (pu, pv) = (ps * su.x + pt * su.y, ps * sv.x + pt * sv.y);
        axial_field(dd[0][0]).dot(pu.cross(pv))
    };
    let (knots, _) = pb.net().knots(SurfParamType::U, None).unwrap();
    outward_sign(b, f) * ruled_flux(&at, &g, us, vs, &clip, &knots, n)
}

/// The chamfered part's volume by its definitions: [`original_volume`] plus the flux out
/// of its new faces (`originals` the part's faces before), each chamfer by its definition
/// over its exact region ([`chord_flux`], `n` points a span), each plane face by
/// [`plane_flux`].
fn definition_volume(k: &Kernel, b: &mut Brep, originals: &[FaceId], n: usize) -> f64 {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    let new: Vec<FaceId> = p1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|f| !originals.contains(f))
        .collect();
    let mut caps = Vec::new();
    for &f in &new {
        let s = b.face(f).unwrap().surface.clone().unwrap();
        if matches!(s.kind(), SurfaceKind::Plane(_)) {
            let d = s
                .evaluate(k, Point2::new(0.0, 0.0), 1, 1, EvaluateFlags::default())
                .unwrap();
            caps.push((f, d[0][0], d[1][0].cross(d[0][1]).unitize().unwrap()));
        }
    }
    let edges = p1::edges(b, None).unwrap();
    let mut volume = original_volume();
    for &f in &new {
        let s = b.face(f).unwrap().surface.clone().unwrap();
        match s.kind() {
            SurfaceKind::ProceduralBlend(pb) => {
                let beside: Vec<(Point3, Vec3)> = caps
                    .iter()
                    .filter(|(c, _, _)| {
                        edges.iter().any(|&e| {
                            let fs = edge::faces(b, e, None).unwrap();
                            fs.contains(c) && fs.contains(&f)
                        })
                    })
                    .map(|&(_, p, n)| (p, n))
                    .collect();
                volume += chord_flux(k, b, f, pb, &beside, n);
            }
            SurfaceKind::Plane(_) => volume += plane_flux(k, b, f),
            // (a wall face made again: F is tangent to it)
            _ => volume += trims_flux(k, b, f, 8),
        }
    }
    volume
}

/// The part's volume over its faces' trims ([`trims_flux`] on every face but the bottom
/// cap, whose flux is the original part's volume), as `b`'s B-rep bounds it.
fn trims_volume(k: &Kernel, b: &mut Brep, originals: &[FaceId], panels: usize) -> f64 {
    let mut volume = original_volume();
    for f in p1::faces(b, None).unwrap() {
        if originals.contains(&f) {
            continue;
        }
        let s = b.face(f).unwrap().surface.clone().unwrap();
        volume += if matches!(s.kind(), SurfaceKind::Plane(_)) {
            plane_flux(k, b, f)
        } else {
            trims_flux(k, b, f, panels)
        };
    }
    volume
}

/// #16725: the volume of the half rim's chamfer by its definitions, and which build's
/// `brep_volume` is right.
///
/// The method uses the divergence theorem with F = (z - 10) a (see above).
/// - The original part's volume over its faces' trims equals its closed form,
///   250 sqrt(5) pi, to 1e-12, with the bottom cap by `plane_flux` and again by
///   `trims_flux` (the only face with flux: F is 0 on the cut plane, tangent to the wall).
/// - The chamfered part's volume is the closed form plus two fluxes: out of the chamfer's
///   definitions over their exact regions (each chord inside the cylinder and on the
///   part's side of the bevel cap, with Gauss rules split at the spine's knots; n = 16
///   and 24 within 1e-7 and 2.5e-8 of n = 32, MEASURED; that agreement cannot show a
///   piece of the region that enters and leaves within one of `ruled_flux`'s grid
///   cells, which only the trims check below bounds), and out of the bevel cap
///   (Green's theorem on its B-rep face's edges).
///
/// The B-rep bounds that volume to within its faces' fit: its trims are within 1e-4 of it
/// (MEASURED, a 1.47x margin). `brep_volume` reads low, as it reads the original part low
/// by 3.6e-4: its x / 3 crosses every tolerant edge's gap.
#[test]
fn issue_880_one_half_chamfer_volume_by_its_definitions() {
    for offsets in [
        (1.0, 2.0),
        (1.443_730_684_954_035_7, 1.443_730_684_954_035_7),
    ] {
        let mut k = Kernel::default();
        let mut b = cut_cylinder(&mut k);
        let originals = p1::faces(&b, None).unwrap();
        // the method: the original part over its trims is its closed form
        let mut all = 0.0;
        for f in p1::faces(&b, None).unwrap() {
            let s = b.face(f).unwrap().surface.clone().unwrap();
            all += if matches!(s.kind(), SurfaceKind::Plane(_)) {
                plane_flux(&k, &mut b, f)
            } else {
                trims_flux(&k, &mut b, f, 8)
            };
        }
        let exact = original_volume();
        assert!(
            (all - exact).abs() <= 1e-12 * exact,
            "{offsets:?}: the original over its trims {all} against {exact}"
        );
        // and with trims_flux on every face, the planes too: g = 0 on the wall and the cut
        // plane, so this checks trims_flux's Green's theorem in (u, v) and its outward
        // sign on the bottom cap against the closed form (3.9e-16 relative measured)
        let mut by_trims = 0.0;
        for f in p1::faces(&b, None).unwrap() {
            by_trims += trims_flux(&k, &mut b, f, 8);
        }
        assert!(
            (by_trims - exact).abs() <= 1e-12 * exact,
            "{offsets:?}: the original by trims_flux {by_trims} against {exact}"
        );
        let original_by_integrator = volume(&k, &mut b);
        let e = rims(&k, &b)[0];
        let u = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
        chamfer(&mut k, &mut b, &[(e, u)], offsets).unwrap();
        let _ = finish(&mut k, &mut b, originals.len());
        // the definitions' volume, converged
        let (v16, v24, v32) = (
            definition_volume(&k, &mut b, &originals, 16),
            definition_volume(&k, &mut b, &originals, 24),
            definition_volume(&k, &mut b, &originals, 32),
        );
        let (d16, d24) = ((v16 - v32).abs(), (v24 - v32).abs());
        eprintln!("{offsets:?}: |v16 - v32| = {d16:e}, |v24 - v32| = {d24:e}");
        // MEASURED: |v16 - v32| is 4.06e-8 and 2.14e-8 at the two setbacks, |v24 - v32|
        // 1.07e-8 and 5.66e-9; each limit is about 2.4x the larger of its two (1e-7 is
        // 2.46x, 2.5e-8 is 2.33x), the n = 24 one tighter as the rule converges
        assert!(d16 <= 1e-7, "{offsets:?}: n = 16 {v16} against {v32}");
        assert!(d24 <= 2.5e-8, "{offsets:?}: n = 24 {v24} against {v32}");
        // the definition build's B-rep bounds it to within its trims' fit. MEASURED: the
        // largest of the four differences here is 6.8e-5, so 1e-4 has a 1.47x margin; it is
        // the faces' fit to their definitions and the trims' to the edges, which nothing
        // here bounds a priori
        let on = trims_volume(&k, &mut b, &originals, 64);
        assert!(
            (on - v32).abs() <= 1e-4,
            "{offsets:?}: its trims {on} against {v32}"
        );
        let integrator = volume(&k, &mut b);
        eprintln!(
            "{offsets:?}: by its definitions {v32:.10}; over the trims {on:.10}; brep_volume \
             {integrator:.10} ({:e}); the original part {:e}",
            integrator - v32,
            original_by_integrator - exact
        );
    }
}
