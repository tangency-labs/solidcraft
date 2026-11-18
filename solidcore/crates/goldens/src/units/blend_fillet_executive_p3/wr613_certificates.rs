//! WR-613 (#18640): a fillet corner patch between a face of the body and a round may take
//! the round's twist at that corner, so the patch meets the face exactly. The kernel
//! takes such a patch only where it is shown no worse than main's mean-twist patch at
//! every station of every side, and otherwise keeps main's patch bit for bit
//! (`face_sided_patch` in `nurbs::b_spline_surface_n_lib::p2`). Each candidate row is
//! checked against main's figures, recorded in [`ROWS`] by this module's own measures:
//! - `check_brep` Level 2 reports nothing and the body is one closed manifold solid;
//! - the corner-patch sides on plane faces number [`Row::face_sides`], as on main;
//! - a row whose face-sided patch was taken ([`Row::moved`]): every such side holds the
//!   tangency certificate of [`patch_sides`] (its next control row in the plane to
//!   [`IN_PLANE`], the net on one side of it), where main's next rows were off the plane
//!   by [`Row::main_next_row_off`] (more than `IN_PLANE`): CLOSER; and every seam of those
//!   patches, face side or blend side, reads a largest normal angle ([`seam_angle`]) that
//!   is not above main's on that seam by more than the two readings' rounding
//!   ([`angle_rounding`], at each angle): CLOSER where it is below by more, else EQUAL;
//! - a row where main's patch was kept: its volume, next-row distance and every seam's
//!   angle are main's, bit for bit (EQUAL);
//! - the row's volume lies in the body's certified volume enclosure ([`enclose_volume`]):
//!   a consistency check of the mass integrator on the new body, not evidence of which
//!   body is closer.
//!
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume

use super::{apply, build, RealSolid};
use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::math::Vec3;
use solidcore::query::continuity::continuity_over;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2::faceuses;
use solidcore::topology::topology_traverser::face_edges;
use solidcore::topology::{Brep, EdgeId, FaceId};
use std::f64::consts::PI;

/// A row with corner-patch sides on plane faces, and main's figures for it (main's
/// library, measured by this module's own [`patch_sides`] and [`seam_angle`]).
struct Row {
    /// The row's name.
    name: &'static str,
    /// Whether the face-sided patch was taken (else main's patch is kept).
    moved: bool,
    /// Main's volume (the golden's on main).
    main_volume: f64,
    /// The corner-patch sides on plane faces: the same on main.
    face_sides: usize,
    /// Main's largest next-row distance from those planes.
    main_next_row_off: f64,
    /// Main's largest normal angle on each seam of those patches: (the edge's index in
    /// the body's edge list, radians).
    main_seams: &'static [(usize, f64)],
}

/// How far a control point may lie off a plane and count as in it: the rounding of
/// the distance `(p - o) . n` as this module reads it and as the kernel placed the
/// point. With coordinates within 30 (asserted in [`patch_sides`]), `|p - o| <= 128`;
/// the difference rounds by `u` a coordinate, the dot product by `γ_3`, and `n`'s
/// length is within `2u` of 1: at most `6u x 128 = 768u` a reading. The kernel put the
/// point in the plane from a boundary control point and a normal of its own, each read
/// to the same order. `8192u` (`9.1e-13`) bounds both readings with a factor 5 to spare.
const IN_PLANE: f64 = 8192.0 * (f64::EPSILON / 2.0);

/// The rounding of the dot product of two unit normals that [`seam_angle`]'s `acos`
/// takes: `δ = 8u` (the product `γ_3`, each normal's length within `2u` of 1, twice).
const DOT_ROUNDING: f64 = 8.0 * (f64::EPSILON / 2.0);

/// The rounding of a [`seam_angle`] reading `theta`: `|acos(c + d) - acos(c)|` for
/// `|d| <= δ` ([`DOT_ROUNDING`]). It is at most `sqrt(2 δ)` everywhere (worst at
/// `c = 1`). Where `theta >= 2 sqrt(2 δ)` the exact angle is within `sqrt(2 δ) <= theta / 2`
/// of the reading, so the derivative `1 / sin` of `acos` between them is at most
/// `1 / sin(theta / 2) <= pi / theta`, and the rounding at most `pi δ / theta`:
/// `4.1e-12` at Heart's `6.8e-4` radians, against `4.22e-8` at 0.
fn angle_rounding(theta: f64) -> f64 {
    let floor = (2.0 * DOT_ROUNDING).sqrt();
    if theta >= 2.0 * floor {
        PI * DOT_ROUNDING / theta
    } else {
        floor
    }
}

#[test]
fn angle_rounding_bounds_acos() {
    // the floor is the half-order loss of acos next to 1
    let c = 1.0 - DOT_ROUNDING;
    assert!(c.acos() <= angle_rounding(0.0));
    // away from 0 it is the first-order bound, and smaller
    assert!(angle_rounding(6.8e-4) < 5e-12);
    assert!(angle_rounding(6.8e-4) >= DOT_ROUNDING / (6.8e-4_f64).sin());
}

/// The row's input, read as the goldens gate reads it (upstream file paths resolved).
fn load(name: &str) -> RealSolid {
    let case = crate::case::regression_case(&super::super::Runner, name);
    let inputs: super::super::Inputs = case.parse().expect("inputs");
    inputs.real_solid.expect("a real_solid case")
}

/// The row's body: its input rounded as the goldens gate rounds it.
fn body(k: &mut Kernel, input: &RealSolid, name: &str) -> Brep {
    let mut body = build(k, input).expect("fixture").expect("a body");
    let edges = p1::edges(&body, None).expect("edges");
    let selected: Vec<_> = if input.all_edges {
        edges
    } else {
        input
            .edge_indices()
            .expect("edge indices")
            .iter()
            .map(|&i| *edges.get(i).expect("selected edge"))
            .collect()
    };
    apply(k, &mut body, &selected, input).unwrap_or_else(|e| panic!("{name}: the round failed: {e:?}"));
    body
}

/// The corner-patch sides on plane faces of `b`: each side of a non-rational B-spline
/// face whose boundary row of control points lies on a plane face the patch shares an
/// edge with (to `1e-9`; the patch's side is then on the plane, as a B-spline curve
/// lies in the hull of its control points).
struct PatchSides {
    /// How many such sides.
    count: usize,
    /// The largest distance of a next row (the row beside the boundary row) from its
    /// side's plane.
    next_row: f64,
    /// Whether every such side holds the certificate: its next row lies in the plane
    /// too (to [`IN_PLANE`]), so the cross-boundary derivative along the side, a
    /// combination of the two rows' differences, lies in the plane and the patch is
    /// tangent to it all along the side; and the whole net lies on one side of the
    /// plane, so the patch, in the convex hull of its net, does not cross it.
    certified: bool,
    /// The patches with such a side.
    patches: Vec<FaceId>,
}

/// [`PatchSides`] of `b`.
fn patch_sides(b: &Brep) -> PatchSides {
    let faces = p1::faces(b, None).expect("faces");
    let mut planes = Vec::new();
    for &f in &faces {
        let s = solidcore::topology::face::surface(b, f)
            .expect("surface")
            .expect("a surface");
        if let SurfaceKind::Plane(p) = s.kind() {
            let (o, n, _) = p.is_planar();
            assert!(o.x.abs() <= 30.0 && o.y.abs() <= 30.0 && o.z.abs() <= 30.0);
            planes.push((f, o, n));
        }
    }
    let mut out = PatchSides {
        count: 0,
        next_row: 0.0,
        certified: true,
        patches: Vec::new(),
    };
    for &f in &faces {
        let s = solidcore::topology::face::surface(b, f)
            .expect("surface")
            .expect("a surface");
        let SurfaceKind::BSpline(patch) = s.kind() else {
            continue;
        };
        if patch.is_rational() {
            continue;
        }
        let net = patch.control_point_net().expect("net");
        // the coordinates [`IN_PLANE`] is derived for
        assert!(
            net.points
                .iter()
                .all(|p| p.x.abs() <= 30.0 && p.y.abs() <= 30.0 && p.z.abs() <= 30.0),
            "a corner patch net beyond 30 of the origin"
        );
        let (nu, nv) = (net.u_count, net.v_count);
        let at = |i: usize, j: usize| net.points[i * nv + j];
        // each side: its boundary row and the row next to it
        let rows: [(Vec<_>, Vec<_>); 4] = [
            ((0..nv).map(|j| at(0, j)).collect(), (0..nv).map(|j| at(1, j)).collect()),
            (
                (0..nv).map(|j| at(nu - 1, j)).collect(),
                (0..nv).map(|j| at(nu - 2, j)).collect(),
            ),
            ((0..nu).map(|i| at(i, 0)).collect(), (0..nu).map(|i| at(i, 1)).collect()),
            (
                (0..nu).map(|i| at(i, nv - 1)).collect(),
                (0..nu).map(|i| at(i, nv - 2)).collect(),
            ),
        ];
        let patch_edges = face_edges(b, f).expect("patch edges");
        for &(plane, o, n) in &planes {
            // only the plane face the patch meets across an edge: a side along a
            // body edge also lies in the other face at that edge, which the patch
            // does not touch tangentially
            let plane_edges = face_edges(b, plane).expect("plane edges");
            if !patch_edges.iter().any(|e| plane_edges.contains(e)) {
                continue;
            }
            let d = |p: Vec3| (p - o).dot(n);
            for (boundary, next) in &rows {
                let on = boundary.iter().all(|&p| d(p).abs() <= 1e-9);
                let span = boundary[0].distance(boundary[boundary.len() - 1]);
                if !on || span <= 1e-9 {
                    continue;
                }
                let off = next.iter().map(|&p| d(p).abs()).fold(0.0, f64::max);
                let lo = net.points.iter().map(|&p| d(p)).fold(f64::INFINITY, f64::min);
                let hi = net
                    .points
                    .iter()
                    .map(|&p| d(p))
                    .fold(f64::NEG_INFINITY, f64::max);
                // NaN fails both comparisons
                let holds = off <= IN_PLANE && (lo >= -IN_PLANE || hi <= IN_PLANE);
                out.certified &= holds;
                out.count += 1;
                out.next_row = out.next_row.max(off);
                if !out.patches.contains(&f) {
                    out.patches.push(f);
                }
            }
        }
    }
    out
}

/// The largest angle between the normals of the two faces at edge `e`, either way
/// round (`min(θ, π - θ)`), read by [`continuity_over`] at 257 even stations along
/// the edge and 129 over each end's 1/32 (where a corner patch's kink sits).
fn seam_angle(k: &Kernel, b: &Brep, e: EdgeId) -> f64 {
    let mut fs = solidcore::topology::edge::faces(b, e, None).expect("edge faces");
    fs.sort();
    fs.dedup();
    let &[fa, fb] = fs.as_slice() else {
        panic!("edge {e:?} is not a seam of two faces: {fs:?}");
    };
    let use_of = |f| faceuses(b, f).expect("faceuses").0.expect("a faceuse");
    let edge = b.edge(e).expect("edge");
    let curve = edge.curve.as_ref().expect("an edge curve");
    let mut worst = 0.0_f64;
    let mut stations = 0;
    for (part, n) in [([0.0, 1.0], 257), ([0.0, 1.0 / 32.0], 129), ([31.0 / 32.0, 1.0], 129)] {
        let measured = continuity_over(
            k,
            curve,
            edge.interval,
            part,
            [(b, use_of(fa)), (b, use_of(fb))],
            n,
        )
        .expect("continuity");
        for station in measured {
            let t = station.measure.normal_angle;
            assert!(t.is_finite(), "edge {e:?}: angle {t}");
            worst = worst.max(t.min(PI - t));
            stations += 1;
        }
    }
    assert!(stations >= 257, "edge {e:?}: only {stations} stations measured");
    worst
}

/// The seams of `patches`: each edge's index in `b`'s edge list, with its angle.
fn seams(k: &Kernel, b: &Brep, patches: &[FaceId]) -> Vec<(usize, f64)> {
    let edges = p1::edges(b, None).expect("edges");
    let mut out: Vec<(usize, f64)> = Vec::new();
    for &f in patches {
        for e in face_edges(b, f).expect("patch edges") {
            let index = edges.iter().position(|&x| x == e).expect("a body edge");
            if out.iter().all(|s| s.0 != index) {
                out.push((index, seam_angle(k, b, e)));
            }
        }
    }
    out.sort_by_key(|s| s.0);
    out
}

/// The row named `name`, checked against main's figures in [`ROWS`].
fn check(name: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.name == name).collect();
    assert_eq!(picked.len(), 1, "{name}: one row of that name");
    let row = picked[0];
    let input = load(name);
    let mut k = Kernel::default();
    let mut body = body(&mut k, &input, name);
    let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
        .expect("check")
        .lines();
    assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
    assert!(
        is_manifold_solid(&body).expect("manifold check"),
        "{name}: not one closed manifold solid"
    );
    let golden = super::rust(&input).unwrap_or_else(|e| panic!("{name}: harness: {e:?}"));
    assert_eq!(golden.ints["mass_status"], [0], "{name}: mass status");
    let volume = *golden.values["mass_properties"].get(1).expect("volume");

    let sides = patch_sides(&body);
    assert_eq!(sides.count, row.face_sides, "{name}: corner-patch sides on plane faces");
    let measured = seams(&k, &body, &sides.patches);
    let edges: Vec<usize> = measured.iter().map(|s| s.0).collect();
    let main_edges: Vec<usize> = row.main_seams.iter().map(|s| s.0).collect();
    assert_eq!(edges, main_edges, "{name}: the corner patches' seams");
    for (&(edge, angle), &(_, main)) in measured.iter().zip(row.main_seams) {
        eprintln!("{name}: edge {edge}: {angle:e} rad (main {main:e})");
    }

    if !row.moved {
        // main's patch kept: main's figures, bit for bit
        assert_eq!(volume.to_bits(), row.main_volume.to_bits(), "{name}: volume {volume}");
        assert_eq!(
            sides.next_row.to_bits(),
            row.main_next_row_off.to_bits(),
            "{name}: next row {:e}",
            sides.next_row
        );
        for (&(edge, angle), &(_, main)) in measured.iter().zip(row.main_seams) {
            assert_eq!(angle.to_bits(), main.to_bits(), "{name}: edge {edge}: {angle:e} vs {main:e}");
        }
        eprintln!("{name}: main's patch kept, every figure equal");
        return;
    }

    // the face sides: each certified, main's off the plane
    assert!(sides.certified, "{name}: a face side misses its certificate");
    assert!(
        row.main_next_row_off > IN_PLANE,
        "{name}: main's next row {:e} was in the plane already",
        row.main_next_row_off
    );
    eprintln!(
        "{name}: {} face sides certified, next rows within {:e} (main {:e})",
        sides.count, sides.next_row, row.main_next_row_off
    );

    // every seam of those patches: no farther from tangent than main's, beyond rounding
    let (mut closer, mut equal, mut farther) = (0, 0, Vec::new());
    for (&(edge, angle), &(_, main)) in measured.iter().zip(row.main_seams) {
        let slack = angle_rounding(angle) + angle_rounding(main);
        if angle > main + slack {
            farther.push(edge);
        } else if angle < main - slack {
            closer += 1;
        } else {
            equal += 1;
        }
    }
    eprintln!("{name}: seams closer {closer}, equal {equal}");
    assert!(farther.is_empty(), "{name}: seams of edges {farther:?} farther than main's");

    // the integrator's volume of the new body lies in its certified enclosure
    let e = enclose_volume(&k, &mut body, 1e-7 * volume).expect("enclosure");
    eprintln!(
        "{name}: volume in [{}, {}] (width {:e}, {:?}), row {volume}, main {}",
        e.lo,
        e.hi,
        e.width(),
        e.stop,
        row.main_volume
    );
    assert!(
        e.lo <= volume && volume <= e.hi,
        "{name}: row volume {volume} not in [{}, {}]",
        e.lo,
        e.hi
    );
}

/// The 24 rows with corner-patch sides on plane faces, each with main's figures
/// (recorded once from main's library, commit `f2f3e16c6`, by this module's own
/// measures) and whether the face-sided patch is taken.
const ROWS: &[Row] = &[
    Row { name: "corner3x2-union-inset-side-step-edges1-14-r1", moved: false, main_volume: 2600.6420851687185, face_sides: 1, main_next_row_off: 3.832959412086723e-5, main_seams: &[(30, 3.8359806295674647e-3), (31, 4.944232823646056e-3), (32, 6.23440552138508e-3), (37, 5.016364759578573e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges1-18-r1", moved: false, main_volume: 2599.9982794302828, face_sides: 1, main_next_row_off: 3.8329594122643584e-5, main_seams: &[(30, 3.8359794345056825e-3), (31, 4.944232825801736e-3), (32, 6.234405521278232e-3), (37, 5.1714294764415926e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges12-13-r1", moved: false, main_volume: 2600.6420849383753, face_sides: 1, main_next_row_off: 3.8329594121755406e-5, main_seams: &[(30, 3.8359806945432043e-3), (31, 4.94423282568946e-3), (32, 6.234405520797412e-3), (37, 3.5114351597007764e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges12-17-r1", moved: false, main_volume: 2599.998279429345, face_sides: 1, main_next_row_off: 3.8329594122643584e-5, main_seams: &[(30, 3.835984257614266e-3), (31, 4.944232825846645e-3), (32, 6.234405521082342e-3), (37, 3.0909929475806324e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges13-19-r1", moved: false, main_volume: 2600.642084895695, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 3.8359831371960755e-3), (30, 4.944232826610115e-3), (31, 6.234405521135767e-3), (37, 3.105872445006255e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges14-27-r1", moved: false, main_volume: 2600.6420849395254, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(28, 3.8359849326377042e-3), (29, 4.94423282613856e-3), (30, 6.234405521723435e-3), (37, 5.1458501663917805e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges15-19-r1", moved: false, main_volume: 2601.071288569917, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(30, 3.8359830027296627e-3), (31, 4.9442328278675955e-3), (32, 6.234405521527545e-3), (37, 3.675019403609314e-5)] },
    Row { name: "corner3x2-union-inset-side-step-edges15-27-r1", moved: false, main_volume: 2601.07128861311, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 3.835985296935559e-3), (30, 4.944232824723896e-3), (31, 6.234405521331656e-3), (37, 5.3033234788566415e-5)] },
    Row { name: "corner3x2-union-overhang-corner-edges6-12-r1", moved: false, main_volume: 2598.710668595906, face_sides: 1, main_next_row_off: 3.8329594122643584e-5, main_seams: &[(28, 3.8359835582501495e-3), (29, 4.944232826250836e-3), (30, 6.234405521224807e-3), (36, 3.113719844376749e-5)] },
    Row { name: "corner3x2-union-overhang-corner-edges6-13-r1", moved: false, main_volume: 2599.5690759416725, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(28, 3.835983582474939e-3), (29, 4.94423282537509e-3), (30, 6.234405521830283e-3), (36, 3.1215275986131e-5)] },
    Row { name: "corner3x2-union-overhang-front-edges15-22-r1", moved: false, main_volume: 2599.998279385677, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(28, 3.835983187671628e-3), (29, 4.9442328272613105e-3), (30, 6.234405521260424e-3), (35, 3.1512792353149705e-5)] },
    Row { name: "corner3x2-union-overhang-front-edges15-24-r1", moved: false, main_volume: 2599.9982794279304, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(28, 3.8359826407469827e-3), (29, 4.94423282568946e-3), (30, 6.2344055211535744e-3), (35, 5.28937113769538e-5)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.1", moved: true, main_volume: 950.6479346142942, face_sides: 4, main_next_row_off: 3.7687186704715714e-11, main_seams: &[(26, 2.1476597587614735e-3), (27, 0e0), (28, 4.307331468154267e-3), (29, 4.308603566563169e-3), (30, 0e0), (31, 2.1490285982051775e-3), (32, 2.1476595554466467e-3), (33, 0e0), (34, 4.308606168864467e-3), (35, 4.307331512539287e-3), (36, 0e0), (37, 2.14766389173986e-3), (70, 6.10261808574068e-3), (72, 6.102617056435018e-3), (73, 6.102618668961659e-3), (75, 6.102617784087809e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.25", moved: false, main_volume: 949.2763535526113, face_sides: 4, main_next_row_off: 4.3016790129968285e-10, main_seams: &[(26, 1.065590352696303e-3), (27, 1.4901161193847656e-8), (28, 1.7413585711547235e-3), (29, 1.8175831203211463e-3), (30, 1.4901161193847656e-8), (31, 1.0655903441528411e-3), (32, 1.0655903540507544e-3), (33, 1.4901161193847656e-8), (34, 1.8175831143350696e-3), (35, 1.7413585795067821e-3), (36, 1.4901161193847656e-8), (37, 1.0655903455072924e-3), (70, 2.1250223989407236e-3), (72, 2.125022396380705e-3), (73, 2.125022400351346e-3), (75, 2.125022401239516e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.5", moved: false, main_volume: 944.4771092314381, face_sides: 4, main_next_row_off: 8.602851764294428e-10, main_seams: &[(24, 2.927077601423078e-3), (25, 1.4901161193847656e-8), (26, 3.76907175142291e-3), (27, 3.769071754987111e-3), (28, 1.4901161193847656e-8), (29, 2.927077588261557e-3), (30, 2.927077600436912e-3), (31, 1.4901161193847656e-8), (32, 3.7690717561359036e-3), (33, 3.7690717516585598e-3), (34, 1.4901161193847656e-8), (35, 2.9270775902338884e-3), (69, 4.947288444525646e-3), (71, 4.947288441428769e-3), (73, 4.947288439453949e-3), (75, 4.947288441810268e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r1.0", moved: false, main_volume: 925.9490134066059, face_sides: 4, main_next_row_off: 1.070619148890728e-10, main_seams: &[(24, 7.0176816800739525e-3), (25, 0e0), (26, 6.777931877701719e-3), (27, 6.7779318764404515e-3), (28, 0e0), (29, 7.017681677052238e-3), (30, 7.017681683079847e-3), (31, 0e0), (32, 6.777931874212759e-3), (33, 6.777931872984252e-3), (34, 0e0), (35, 7.017681676055546e-3), (69, 8.385535504448382e-3), (71, 8.385535508618949e-3), (73, 8.385535503071433e-3), (75, 8.385535509082345e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-step2", moved: true, main_volume: 950.6479346142942, face_sides: 4, main_next_row_off: 3.7687186704715714e-11, main_seams: &[(26, 2.1476597587614735e-3), (27, 0e0), (28, 4.307331468154267e-3), (29, 4.308603566563169e-3), (30, 0e0), (31, 2.1490285982051775e-3), (32, 2.1476595554466467e-3), (33, 0e0), (34, 4.308606168864467e-3), (35, 4.307331512539287e-3), (36, 0e0), (37, 2.14766389173986e-3), (70, 6.10261808574068e-3), (72, 6.102617056435018e-3), (73, 6.102618668961659e-3), (75, 6.102617784087809e-3)] },
    Row { name: "source-file-box_diff_cyl-all-edges", moved: true, main_volume: 950.648098797075, face_sides: 2, main_next_row_off: 3.7687186704715714e-11, main_seams: &[(32, 2.1476595554466467e-3), (33, 0e0), (34, 4.308606168864467e-3), (35, 4.307331512539287e-3), (36, 0e0), (37, 2.14766389173986e-3), (73, 6.102618668961659e-3), (75, 6.102617784087809e-3)] },
    Row { name: "stubs3-union-inset-side-step-edges16-22-r1", moved: false, main_volume: 2601.7150940784886, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(30, 3.8359833419925227e-3), (31, 4.94423282663257e-3), (32, 6.2344055212960395e-3), (37, 3.674735418723121e-5)] },
    Row { name: "stubs3-union-inset-side-step-edges17-22-r1", moved: false, main_volume: 2600.6420848961548, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 3.835984271506616e-3), (30, 4.944232826340655e-3), (31, 6.234405521456313e-3), (37, 3.095368973196893e-5)] },
    Row { name: "stubs3-union-overhang-corner-edges14-22-r1", moved: false, main_volume: 2599.5690757124053, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(29, 3.8359818943511496e-3), (30, 4.9442328269693955e-3), (31, 6.234405521563162e-3), (36, 3.63087639163081e-5)] },
    Row { name: "stubs3-union-overhang-corner-edges15-22-r1", moved: false, main_volume: 2598.710668410454, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 3.8359873391677957e-3), (30, 4.944232827508315e-3), (31, 6.234405521527545e-3), (36, 3.14303040534913e-5)] },
    Row { name: "stubs3-union-overhang-front-edges14-22-r1", moved: false, main_volume: 2598.7106683663496, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(29, 3.8359831066328964e-3), (30, 4.94423282703676e-3), (31, 6.234405521189191e-3), (36, 3.107451656219666e-5)] },
    Row { name: "stubs4-Heart-all-edges-r0.1", moved: false, main_volume: 326.05828850789356, face_sides: 2, main_next_row_off: 1.609444153816142e-5, main_seams: &[(8, 5.834382971169943e-4), (9, 6.771257499057234e-4), (10, 1.7263064311850678e-3), (11, 6.770965791146431e-4), (12, 6.771191332215269e-4), (13, 1.5194066439734085e-3), (14, 6.770965794425795e-4), (15, 5.834382967364149e-4)] },
];

/// One test per row, so the test runner spreads them over its threads.
macro_rules! row_tests {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check($name);
            }
        )*

        /// The rows the generated tests take, in [`ROWS`]'s order.
        const ROW_TESTS: &[&str] = &[$($name),*];
    };
}

row_tests! {
    corner3x2_union_inset_side_step_edges1_14_r1 => "corner3x2-union-inset-side-step-edges1-14-r1",
    corner3x2_union_inset_side_step_edges1_18_r1 => "corner3x2-union-inset-side-step-edges1-18-r1",
    corner3x2_union_inset_side_step_edges12_13_r1 => "corner3x2-union-inset-side-step-edges12-13-r1",
    corner3x2_union_inset_side_step_edges12_17_r1 => "corner3x2-union-inset-side-step-edges12-17-r1",
    corner3x2_union_inset_side_step_edges13_19_r1 => "corner3x2-union-inset-side-step-edges13-19-r1",
    corner3x2_union_inset_side_step_edges14_27_r1 => "corner3x2-union-inset-side-step-edges14-27-r1",
    corner3x2_union_inset_side_step_edges15_19_r1 => "corner3x2-union-inset-side-step-edges15-19-r1",
    corner3x2_union_inset_side_step_edges15_27_r1 => "corner3x2-union-inset-side-step-edges15-27-r1",
    corner3x2_union_overhang_corner_edges6_12_r1 => "corner3x2-union-overhang-corner-edges6-12-r1",
    corner3x2_union_overhang_corner_edges6_13_r1 => "corner3x2-union-overhang-corner-edges6-13-r1",
    corner3x2_union_overhang_front_edges15_22_r1 => "corner3x2-union-overhang-front-edges15-22-r1",
    corner3x2_union_overhang_front_edges15_24_r1 => "corner3x2-union-overhang-front-edges15-24-r1",
    curve_sih_box_diff_cyl_all_edges_r0_1 => "curve-sih-box_diff_cyl-all-edges-r0.1",
    curve_sih_box_diff_cyl_all_edges_r0_25 => "curve-sih-box_diff_cyl-all-edges-r0.25",
    curve_sih_box_diff_cyl_all_edges_r0_5 => "curve-sih-box_diff_cyl-all-edges-r0.5",
    curve_sih_box_diff_cyl_all_edges_r1_0 => "curve-sih-box_diff_cyl-all-edges-r1.0",
    curve_sih_box_diff_cyl_all_edges_step2 => "curve-sih-box_diff_cyl-all-edges-step2",
    source_file_box_diff_cyl_all_edges => "source-file-box_diff_cyl-all-edges",
    stubs3_union_inset_side_step_edges16_22_r1 => "stubs3-union-inset-side-step-edges16-22-r1",
    stubs3_union_inset_side_step_edges17_22_r1 => "stubs3-union-inset-side-step-edges17-22-r1",
    stubs3_union_overhang_corner_edges14_22_r1 => "stubs3-union-overhang-corner-edges14-22-r1",
    stubs3_union_overhang_corner_edges15_22_r1 => "stubs3-union-overhang-corner-edges15-22-r1",
    stubs3_union_overhang_front_edges14_22_r1 => "stubs3-union-overhang-front-edges14-22-r1",
    stubs4_heart_all_edges_r0_1 => "stubs4-Heart-all-edges-r0.1",
}

/// Every row of [`ROWS`] has exactly one test.
#[test]
fn every_row_has_its_own_test() {
    let rows: Vec<&str> = ROWS.iter().map(|row| row.name).collect();
    assert_eq!(rows, ROW_TESTS);
}
