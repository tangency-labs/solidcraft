//! A quarter-round notch along the tilted edge where a box's face meets a slanted face
//! (#2538): the cut must give one closed solid of the closed-form volume.
//!
//! SolidCraft reports the design's cut failing with
//! `boolean::merge::p2::make_faceuse: upstream status 1001`, and the same cut with the
//! tool 2 mm longer refused with `SmFace::Test #36` ("Face Surface Seam has suspicious
//! Edge near seam"); the C++ fork refuses both. The mirror image about x = 0, whose
//! seam points away from the material, passes on both.
//!
//! The geometry says why #36 names the seam. The tool's axis is the edge itself, so the
//! cylinder meets the slanted face in the two lines a radius to either side of it, and
//! the design turns the seam onto world +y, which is exactly the line on the material
//! side. The seam is therefore an edge of the result, not a line inside a face. The
//! mirror turns the seam to -y, onto the line on the empty side, where no edge lands.
//!
//! Which line the seam lands on is therefore decided to the last bit, and that is the
//! defect the last test here pins: with the slanted plane holding the axis exactly the
//! cut passes, but build the half space the way the issue's repro builds it, from a
//! rotation written out to twelve digits, and the plane misses the axis by 1.5e-06 mm
//! and 4.5e-07 rad. The seam then lies that far from the edge instead of on it,
//! `has_near_misses` (`intersect/curve_class/p7.rs`, threshold `EFF_ZERO * 1000` =
//! 1e-09 mm) calls it a near miss, face test 36 fires, and the merge leaves a ninth,
//! sliver face and a result that is no longer a closed solid
//! (`plan/findings/boolean.api_brep--2538-tilted-notch-on-a-slant-edge.md`).
//!
//! The case: the box 198 x 105.5 x 71.5 with its lower left corner at (-99, -6.5, -55),
//! first cut by the half space that leaves an edge along the line through the issue's
//! sketch origin (-12.07395, -6.5, 0.84551) along the unit of its `y_dir`
//! [0.069857, 0, 0.997557] (4.005773878 degrees from z), then cut by the cylinder of
//! radius 8 about that same line. The line lies in the box's own y = -6.5 face, so the
//! slanted face and that face meet along it at a right angle and the cylinder takes a
//! quarter of its own volume out of the wedge.
//!
//! The same case is in the kernel-rig corpus as
//! `tests/kernel_cases/booleans/2538-slant-edge-minus-tilted-notch-caps-on-faces`,
//! whose `expected.method` carries the derivation.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::field_reassign_with_default
)] // test

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives as api;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::Kernel;
use std::f64::consts::PI;

/// Tool radius.
const R: f64 = 8.0;
/// The box, as the issue builds it.
const BOX_LO: Point3 = Point3::new(-99.0, -6.5, -55.0);
const BOX_SIZE: (f64, f64, f64) = (198.0, 105.5, 71.5);
/// The issue's sketch origin: a point of the edge the notch runs along.
const SKETCH_O: Point3 = Point3::new(-12.07395, -6.5, 0.84551);
/// The box's top and bottom face planes, which the design's cap centres sit on.
const TOP_Z: f64 = 16.5;
const BOT_Z: f64 = -55.0;
/// The half space's cube, as the issue builds it (`solid.box` 2500 a side).
const HALF_SPACE: f64 = 2500.0;
/// The issue's `solid.move` on that cube: the axis, angle and offset as it prints them.
const ROT_AXIS: [f64; 3] = [-0.590_342_071_131, 0.590_342_071_131, -0.550_447_525_296];
const ROT_DEG: f64 = 122.339_047_500_510_76;
const MOVE: [f64; 3] = [1_230.961_628_400_702, 0.0, -142.320_693_724_926];
/// The integrator runs at accuracy 1e-7; `boolean_api.rs` grades volumes at 1e-6.
const VOLUME_REL_TOL: f64 = 1e-6;

/// How the tool body is built.
#[derive(Clone, Copy, Debug)]
enum Tool {
    /// `create_cylinder` placed by a rigid transform.
    Primitive,
    /// The issue's own path: the profile rectangle revolved a full turn.
    Revolve,
}

/// How far past the box's two faces the tool's cap centres sit, in mm along the axis.
///
/// 0 is the design (`v5` in the issue), where both cap centres land exactly on the face
/// planes; 2 is the issue's `v13`, where the whole of both caps is clear of the box and
/// the box's own faces do all the cutting.
#[derive(Clone, Copy, Debug)]
struct Past(f64);

/// Sine and cosine of the tilt: the unit of the issue's sketch `y_dir`.
fn axis_sin_cos() -> (f64, f64) {
    let (dx, dz) = (0.069857_f64, 0.997557_f64);
    let n = (dx * dx + dz * dz).sqrt();
    (dx / n, dz / n)
}

/// The notch's axis, pointing up, mirrored about x = 0 or not.
fn axis(mirror: bool) -> Vec3 {
    let (s, c) = axis_sin_cos();
    Vec3::new(if mirror { -s } else { s }, 0.0, c)
}

/// A point of the axis: the sketch origin, mirrored about x = 0 or not.
fn axis_point(mirror: bool) -> Point3 {
    let x = if mirror { -SKETCH_O.x } else { SKETCH_O.x };
    Point3::new(x, SKETCH_O.y, SKETCH_O.z)
}

/// The outward normal of the slanted face: perpendicular to the axis, in the xz plane,
/// pointing away from the material.
fn slant_normal(mirror: bool) -> Vec3 {
    let (s, c) = axis_sin_cos();
    Vec3::new(if mirror { -c } else { c }, 0.0, -s)
}

/// The seam direction the issue gives the tool: world +y unmirrored, which is the line
/// where the cylinder meets the slanted face on the material side, and -y mirrored, the
/// line on the empty side.
fn seam(mirror: bool) -> Vec3 {
    Vec3::new(0.0, if mirror { -1.0 } else { 1.0 }, 0.0)
}

/// The axial coordinate, measured from the axis point, at which the axis crosses `z`.
fn axial_at_z(z: f64) -> f64 {
    let (_, c) = axis_sin_cos();
    (z - SKETCH_O.z) / c
}

/// The box's height measured along the axis.
fn axial_height() -> f64 {
    let (_, c) = axis_sin_cos();
    BOX_SIZE.2 / c
}

/// The volume the two cuts must leave, for cap centres `past.0` mm clear of the faces.
///
/// Put (p, q, s) on the axis: p along the slanted face's outward normal, q along world
/// +y from the box's y = -6.5 face, s along the axis from the axis point. The frame is
/// orthonormal, the material of the clipped box is p <= 0 and q >= 0 near the axis, and
/// the box's own top and bottom faces bound s to an interval of the fixed length
/// `L = 71.5/cos(t)` that slides with p:
///
/// ```text
/// s in [axial_at_z(-55) + k p, axial_at_z(16.5) + k p],    k = tan(t)
/// ```
///
/// The clipped box is a prism: its cross section in x at height z runs from -99 to
/// -12.07395 + k (z - 0.84551), so
///
/// ```text
/// V_prism = 105.5 * integral(99 - 12.07395 + k (z - 0.84551) dz, -55 .. 16.5)
/// ```
///
/// The notch is the tool's quarter disc p <= 0, q >= 0 of radius 8 times the length in
/// s that both the box and the tool keep. With the caps `past` mm clear, the box's
/// bounds bind for every p and that length is L, giving `16 pi L`. With the cap centres
/// on the faces (`past` 0) the tool's own bottom cap is the lower bound instead, so the
/// length is `L + k p` and the notch leaves standing the ungula the tilted cap spares:
///
/// ```text
/// integral(p sqrt(64 - p^2) dp, -8 .. 0) = -512/3
/// ```
///
/// Past `8 k` the whole cap is clear of the face and nothing is spared, so the two
/// forms meet; 2 mm, the issue's `v13`, is well past that.
fn expected_volume(past: Past) -> f64 {
    let (sin_t, cos_t) = axis_sin_cos();
    let k = sin_t / cos_t;
    let top = TOP_Z - SKETCH_O.z;
    let bot = BOT_Z - SKETCH_O.z;
    let prism = BOX_SIZE.1 * (BOX_SIZE.2 * (99.0 + SKETCH_O.x) + k * (top * top - bot * bot) / 2.0);
    let spared = if past.0 >= R * k {
        0.0
    } else if past.0 == 0.0 {
        512.0 / 3.0 * k
    } else {
        // Only the two offsets the issue measures are derived here.
        panic!("no closed form for a cap {} mm clear of the face", past.0);
    };
    prism - 16.0 * PI * axial_height() + spared
}

fn the_box(k: &mut Kernel) -> Brep {
    api::create_box(k, BOX_LO, BOX_SIZE.0, BOX_SIZE.1, BOX_SIZE.2).unwrap()
}

/// The half space that leaves the slanted face: the issue's 2500 cube, placed so its
/// own face plane holds the axis exactly rather than to a rounded rotation.
fn the_half_space(k: &mut Kernel, mirror: bool) -> Brep {
    let half = HALF_SPACE / 2.0;
    let mut b = api::create_box(
        k,
        Point3::new(0.0, -half, -half),
        HALF_SPACE,
        HALF_SPACE,
        HALF_SPACE,
    )
    .unwrap();
    // Local +x onto the slanted face's outward normal, so the cube's local x = 0 face
    // is that face and the cube fills the side the cut takes away.
    let n = slant_normal(mirror);
    let pl = Axis2Placement::new(axis_point(mirror), n, Vec3::new(0.0, 1.0, 0.0));
    solidcore::topology::brep::p5::transform(k, &mut b, &pl, None, false).unwrap();
    b
}

/// The columns of the issue's rotation, from the axis and angle its `solid.move`
/// prints. `Axis2Placement` makes its frame canonical, so the 5e-13 by which that
/// rounded axis misses unit length is not what this measures.
fn rot_cols() -> [Vec3; 3] {
    let n =
        (ROT_AXIS[0] * ROT_AXIS[0] + ROT_AXIS[1] * ROT_AXIS[1] + ROT_AXIS[2] * ROT_AXIS[2]).sqrt();
    let u = [ROT_AXIS[0] / n, ROT_AXIS[1] / n, ROT_AXIS[2] / n];
    let (s, c) = (ROT_DEG.to_radians().sin(), ROT_DEG.to_radians().cos());
    let m = |i: usize, j: usize| {
        let kr = [[0.0, -u[2], u[1]], [u[2], 0.0, -u[0]], [-u[1], u[0], 0.0]];
        let kk: f64 = (0..3).map(|t| kr[i][t] * kr[t][j]).sum();
        (if i == j { 1.0 } else { 0.0 }) + s * kr[i][j] + (1.0 - c) * kk
    };
    [
        Vec3::new(m(0, 0), m(1, 0), m(2, 0)),
        Vec3::new(m(0, 1), m(1, 1), m(2, 1)),
        Vec3::new(m(0, 2), m(1, 2), m(2, 2)),
    ]
}

/// The half space the way the issue's repro builds it: the 2500 cube turned by that
/// rotation and moved, so its face plane misses the axis by 1.5e-06 mm and 4.5e-07 rad.
fn the_repros_half_space(k: &mut Kernel) -> Brep {
    let half = HALF_SPACE / 2.0;
    let mut b = api::create_box(
        k,
        Point3::new(-half, -half, -half),
        HALF_SPACE,
        HALF_SPACE,
        HALF_SPACE,
    )
    .unwrap();
    let cols = rot_cols();
    let pl = Axis2Placement::new(Point3::new(MOVE[0], MOVE[1], MOVE[2]), cols[0], cols[1]);
    solidcore::topology::brep::p5::transform(k, &mut b, &pl, None, false).unwrap();
    b
}

/// The notch tool: radius 8 about the axis, its seam turned as the issue turns it, its
/// cap centres `past.0` mm clear of the box's two face planes along the axis.
fn the_tool(k: &mut Kernel, tool: Tool, past: Past, mirror: bool) -> Brep {
    let ax = axis(mirror);
    let sm = seam(mirror);
    let hi = axial_at_z(TOP_Z) + past.0;
    let lo = axial_at_z(BOT_Z) - past.0;
    let h = hi - lo;
    let origin = axis_point(mirror);
    let top = origin + ax * hi;
    match tool {
        Tool::Primitive => {
            // Local: the top cap centre at the origin, the body from z = -h to 0, seam
            // along local +x. The placement maps that origin onto the cap centre
            // exactly, so the design's degeneracy is exact and not a rounded rotation.
            let mut b = api::create_cylinder(k, Point3::new(0.0, 0.0, -h), R, h).unwrap();
            let pl = Axis2Placement::new(top, sm, ax.cross(sm));
            solidcore::topology::brep::p5::transform(k, &mut b, &pl, None, false).unwrap();
            b
        }
        Tool::Revolve => {
            // The issue's sketch: x along the seam, y along the axis, the profile the
            // rectangle R wide from the axis, revolved a full turn about the axis.
            let at = |sx: f64, sy: f64| origin + sm * sx + ax * sy;
            let cs = [
                Curve::from(create_line_segment(at(0.0, lo), at(R, lo)).unwrap()),
                Curve::from(create_line_segment(at(R, lo), at(R, hi)).unwrap()),
                Curve::from(create_line_segment(at(R, hi), at(0.0, hi)).unwrap()),
                Curve::from(create_line_segment(at(0.0, hi), at(0.0, lo)).unwrap()),
            ];
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_rotational_sweep(k, &refs, origin, ax, 360.0, false).unwrap()
        }
    }
}

/// Checks a cut result and returns its volume and face count.
fn graded(k: &mut Kernel, brep: &mut Brep, what: &str) -> (f64, usize) {
    solidcore::topology::integrity::check(brep).unwrap();
    assert!(
        solidcore::topology::brep::p2::is_manifold_solid(brep).unwrap(),
        "{what}: the result is not a closed manifold solid"
    );
    // Level2 runs face test 36, the one SolidCraft reports refusing this cut.
    let check = check_brep(k, brep, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "{what}: Level2 check: {:?}",
        check.reports
    );
    let faces = solidcore::topology::brep::p1::faces(brep, None)
        .unwrap()
        .len();
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(k, brep, 1e-7, &attrs).unwrap();
    (v, faces)
}

/// Clips the box by the half space, cuts the notch out of it and grades the result
/// against the closed form.
fn notch(tool: Tool, past: Past, mirror: bool, fixes: &UpstreamFixes) -> (f64, usize) {
    let mut k = Kernel::default();
    k.fixes = *fixes;
    let what = format!("{tool:?} past {} mirror {mirror}", past.0);
    let a = the_box(&mut k);
    let hs = the_half_space(&mut k, mirror);
    let clipped = boolean_difference(&mut k, a, hs)
        .unwrap_or_else(|e| panic!("{what}: the half space cut was refused: {e}"));
    let b = the_tool(&mut k, tool, past, mirror);
    let mut brep = boolean_difference(&mut k, clipped, b)
        .unwrap_or_else(|e| panic!("{what}: the notch cut was refused: {e}"));
    let (v, faces) = graded(&mut k, &mut brep, &what);
    let want = expected_volume(past);
    let rel = (v - want).abs() / want.abs();
    // Printed so a run reports what it measured, not only that it passed.
    println!(
        "past {:<5} mirror {mirror:<5} {tool:<12} faces {faces}  volume {v:.6}  \
         closed form {want:.6}  rel {rel:.1e}",
        past.0,
        tool = format!("{tool:?}")
    );
    assert!(
        rel <= VOLUME_REL_TOL,
        "{what}: volume {v} vs the closed form {want} (rel {rel:.2e})"
    );
    (v, faces)
}

/// The design of the issue (`v5`): both cap centres exactly on the box's face planes,
/// and the seam on the line where the cylinder meets the slanted face on the material
/// side. One closed solid of 641498.856 mm3, not a refusal, however the tool is built
/// and whichever upstream fixes are on (SolidCraft runs all but its held-back ones).
#[test]
fn the_design_notch_leaves_one_closed_solid() {
    let want = expected_volume(Past(0.0));
    assert!(
        (want - 641_498.856_368_302_7).abs() < 1e-7,
        "the closed form of the design is the corpus entry's value, got {want}"
    );
    let (s, c) = axis_sin_cos();
    let top = axis_point(false) + axis(false) * axial_at_z(TOP_Z);
    let bot = axis_point(false) + axis(false) * axial_at_z(BOT_Z);
    assert_eq!(top.z, TOP_Z, "the top degeneracy must be exact");
    assert_eq!(bot.z, BOT_Z, "the bottom degeneracy must be exact");
    assert!(
        axis(false).dot(slant_normal(false)).abs() < 1e-16,
        "the slanted face holds the axis"
    );
    assert!((s * s + c * c - 1.0).abs() < 1e-15);
    for tool in [Tool::Primitive, Tool::Revolve] {
        for fixes in [UpstreamFixes::default(), UpstreamFixes::all()] {
            let (_, faces) = notch(tool, Past(0.0), false, &fixes);
            assert_eq!(
                faces, 8,
                "{tool:?}: the prism's 6 faces, the notch wall and the cap face the \
                 tilted bottom cap plane leaves standing"
            );
        }
    }
}

/// The issue's `v13`, the tool 2 mm past both faces: the caps are clear of the box
/// altogether, so no cap plane cuts and the notch is a plain quarter cylinder through
/// the slab. The seam degeneracy is untouched, and this is the variant the issue
/// reports refused with `SmFace::Test #36` on both kernels.
#[test]
fn the_notch_two_mm_past_both_faces_leaves_one_closed_solid() {
    let (sin_t, cos_t) = axis_sin_cos();
    assert!(
        2.0 > R * sin_t / cos_t,
        "2 mm is past the offset at which the cap clears the face"
    );
    for tool in [Tool::Primitive, Tool::Revolve] {
        let (_, faces) = notch(tool, Past(2.0), false, &UpstreamFixes::all());
        assert_eq!(faces, 7, "{tool:?}: the prism's 6 faces and the notch wall");
    }
    // Clear caps cut nothing, so they leave strictly less behind than the design's
    // spared ungula.
    assert!(expected_volume(Past(2.0)) < expected_volume(Past(0.0)));
}

/// The mirror image about x = 0 (`r1_pos`), which the issue reports passing on both
/// kernels: the same geometry, the same volume, and the seam turned onto the line where
/// the cylinder meets the slanted face on the empty side.
#[test]
fn the_mirrored_notch_leaves_the_same_solid() {
    for tool in [Tool::Primitive, Tool::Revolve] {
        let (v, faces) = notch(tool, Past(0.0), true, &UpstreamFixes::all());
        assert_eq!(faces, 8, "{tool:?}: as many faces as the design");
        let (v0, _) = notch(tool, Past(0.0), false, &UpstreamFixes::all());
        let rel = (v - v0).abs() / v0;
        assert!(
            rel <= VOLUME_REL_TOL,
            "{tool:?}: the mirror's volume {v} against the design's {v0} (rel {rel:.2e})"
        );
    }
    // The seam lies on the slanted face a radius from the axis, on the material side
    // unmirrored and on the empty side mirrored.
    for mirror in [false, true] {
        let on_face = axis_point(mirror) + seam(mirror) * R;
        let n = slant_normal(mirror);
        assert!(
            n.dot(on_face - axis_point(mirror)).abs() < 1e-15,
            "the seam line lies in the slanted face"
        );
        assert_eq!(
            on_face.y > BOX_LO.y,
            !mirror,
            "the seam is on the material side only unmirrored"
        );
    }
}

/// **Known failing** (`e8397421e`): the issue's own construction of the half space.
///
/// The issue builds it by turning a 2500 cube about the axis
/// [-0.590342071131, 0.590342071131, -0.550447525296] by 122.33904750051076 degrees and
/// moving it, numbers written out to twelve digits. The plane that comes out misses the
/// notch's axis: its normal is tilted 4.4721e-07 rad from the exact one, and along the
/// seam the gap runs from -2.3540e-05 mm at the lower cap through +1.4954e-06 mm at the
/// sketch origin to +8.5134e-06 mm at the upper cap, so the seam crosses the edge where
/// the cylinder meets the slanted face instead of lying on it.
///
/// That puts part of the seam in the band between `has_near_misses`'s threshold
/// (`EFF_ZERO * 1000` = 1e-09 mm, `intersect/curve_class/p7.rs`) and the bodies' 2e-05
/// tolerance, where the seam still classifies as coincident with that edge but not
/// exactly on it. Face test 36 fires, and at this gap the merge also leaves a ninth,
/// sliver face and an edge whose radial angles do not increase, so the result is no
/// longer a closed solid.
///
/// The lower gap exceeds intersection tolerance, so this is a partial-coincidence
/// case. It stays pinned as known failing while the wholly coincident rows below
/// are repaired. The corrected closed form, including the first-order notch change,
/// is approximately 641498.90887433 mm3 (see `perturbed_expected_volume` below).
#[test]
fn the_repros_rounded_slant_plane_leaves_a_sliver_near_the_seam() {
    let mut k = Kernel::default();
    k.fixes = UpstreamFixes::all();
    let a = the_box(&mut k);
    let hs = the_repros_half_space(&mut k);
    let clipped = boolean_difference(&mut k, a, hs).expect("the half space cut");
    let b = the_tool(&mut k, Tool::Primitive, Past(0.0), false);
    let mut brep = boolean_difference(&mut k, clipped, b).expect("the notch cut");
    solidcore::topology::integrity::check(&brep).unwrap();
    let faces = solidcore::topology::brep::p1::faces(&brep, None)
        .unwrap()
        .len();
    let solid = solidcore::topology::brep::p2::is_manifold_solid(&brep).unwrap();
    let check = check_brep(&mut k, &mut brep, CheckLevel::Level2).unwrap();
    let names: Vec<String> = check
        .reports
        .reports()
        .iter()
        .filter(|r| !r.ok)
        .map(|r| format!("{}#{}", r.owner_type_string.unwrap_or("?"), r.test_index))
        .collect();
    println!("the repro's own rounded rotation: faces {faces} solid {solid} {names:?}");
    assert_eq!(faces, 9, "today: a ninth, sliver face");
    assert!(!solid, "today: not a closed manifold solid");
    assert!(!check.is_valid(), "today: the Level2 check refuses it");
    assert!(
        names.contains(&"SmFace#36".to_owned()),
        "the issue's own report, face test 36: {names:?}"
    );
    assert!(
        names.contains(&"SmEdge#9".to_owned()),
        "the sliver's edge, test 9: {names:?}"
    );
}

/// The retained side is normal · (point - origin) <= 0.
#[derive(Clone, Copy)]
struct SlantPlane {
    origin: Point3,
    normal: Vec3,
}

fn perturbed_slant(tilt: f64, shift: f64) -> SlantPlane {
    let n = slant_normal(false);
    let (s, c) = tilt.sin_cos();
    // Right-handed rotation about world y through the sketch origin.
    let normal = Vec3::new(c * n.x + s * n.z, n.y, -s * n.x + c * n.z);
    let p = Axis2Placement::new(axis_point(false) + normal * shift, normal, seam(false));
    SlantPlane {
        origin: p.origin(),
        normal: p.x_axis(),
    }
}

fn repro_slant() -> SlantPlane {
    let cols = rot_cols();
    let p = Axis2Placement::new(Point3::new(MOVE[0], MOVE[1], MOVE[2]), cols[0], cols[1]);
    let normal = p.z_axis();
    SlantPlane {
        origin: p.origin() - normal * (HALF_SPACE / 2.0),
        normal,
    }
}

fn perturbed_half_space(k: &mut Kernel, plane: SlantPlane) -> Brep {
    let half = HALF_SPACE / 2.0;
    let mut b = api::create_box(
        k,
        Point3::new(0.0, -half, -half),
        HALF_SPACE,
        HALF_SPACE,
        HALF_SPACE,
    )
    .unwrap();
    let p = Axis2Placement::new(plane.origin, plane.normal, seam(false));
    solidcore::topology::brep::p5::transform(k, &mut b, &p, None, false).unwrap();
    b
}

/// [constant, coefficient of p, coefficient of q] in the notch frame.
type AxialBound = [f64; 3];
/// [coefficient of p, coefficient of q, constant], retained side >= 0.
type HalfPlane = [f64; 3];

/// Exact affine-plane volume, including finite tool caps and the box's z planes.
///
/// Write X=O+p*n0+q*seam+s*axis. The slant is A*p+B*q+C*s+D<=0, from its
/// actual normal. The prism is the affine x(y,z) integral over the yz rectangle.
/// In the cylinder p²+q²<=R², q>=0 and
/// max(lo,lo+k*p)<=s<=min(hi,hi+k*p), k=sin(t)/cos(t). The slant adds the
/// upper bound -(A*p+B*q+D)/C when C>0, or a lower bound when C<0.
///
/// For B=0, the unclipped section at s has boundary delta=-(C*s+D)/A:
///
///     integral(-R .. delta, sqrt(R²-p²) dp)
///     = pi*R²/4 + (delta*sqrt(R²-delta²)+R²*asin(delta/R))/2.
///
/// Its derivative at zero is R=8, so the notch changes to first order.
/// Integrating an infinite cylinder alone omits the original 512*k/3 cap ungula.
///
/// We partition the half disc by which affine upper/lower bounds are active.
/// Each axial length is a+b*p+c*q; Green's theorem integrates it using exact
/// area and first moments of straight segments and circle arcs. This includes
/// the rounded repro's nonzero B, without discarding a small normal component.
///
/// The quadrature test independently integrates in p=R*sin(phi) with Simpson,
/// using trapezoids for the piecewise-linear q sections. Every row is checked
/// to 1e-7 mm³ independently of the kernel's mass integrator. The exact design
/// is 641498.8563683027; the rounded repro is approximately 641498.90887433,
/// replacing the earlier prism-only figure 641498.913182.
fn perturbed_expected_volume(plane: SlantPlane) -> f64 {
    let n = plane.normal;
    let y = BOX_LO.y + BOX_SIZE.1 / 2.0;
    let z = (BOT_Z + TOP_Z) / 2.0;
    let x = plane.origin.x - n.y / n.x * (y - plane.origin.y) - n.z / n.x * (z - plane.origin.z);
    let prism = BOX_SIZE.1 * BOX_SIZE.2 * (x - BOX_LO.x);
    let [a, b, c, d] = slant_coefficients(plane);
    let (s, co) = axis_sin_cos();
    let (lo, hi) = (axial_at_z(BOT_Z), axial_at_z(TOP_Z));
    let mut lower: Vec<AxialBound> = vec![[lo, 0.0, 0.0], [lo, s / co, 0.0]];
    let mut upper: Vec<AxialBound> = vec![[hi, 0.0, 0.0], [hi, s / co, 0.0]];
    if c > 0.0 {
        upper.push([-d / c, -a / c, -b / c]);
    } else if c < 0.0 {
        lower.push([-d / c, -a / c, -b / c]);
    }
    let mut notch = 0.0;
    for (li, l) in lower.iter().enumerate() {
        for (ui, u) in upper.iter().enumerate() {
            let h = [u[0] - l[0], u[1] - l[1], u[2] - l[2]];
            let mut cuts = vec![[0.0, 1.0, 0.0], [h[1], h[2], h[0]]];
            for (i, v) in lower.iter().enumerate() {
                if i != li {
                    cuts.push([l[1] - v[1], l[2] - v[2], l[0] - v[0]]);
                }
            }
            for (i, v) in upper.iter().enumerate() {
                if i != ui {
                    cuts.push([v[1] - u[1], v[2] - u[2], v[0] - u[0]]);
                }
            }
            if c == 0.0 {
                cuts.push([-a, -b, -d]);
            }
            let m = clipped_disc_moments(&cuts);
            notch += h[0] * m[0] + h[1] * m[1] + h[2] * m[2];
        }
    }
    prism - notch
}

fn slant_coefficients(plane: SlantPlane) -> [f64; 4] {
    [
        plane.normal.dot(slant_normal(false)),
        plane.normal.dot(seam(false)),
        plane.normal.dot(axis(false)),
        plane.normal.dot(axis_point(false) - plane.origin),
    ]
}

/// [area, integral(p), integral(q)] of a disc clipped by linear half planes.
/// Straight boundaries have the retained side on the left, arcs run CCW.
/// Coincident parallel bounds contribute only once.
fn clipped_disc_moments(input: &[HalfPlane]) -> [f64; 3] {
    let mut cuts: Vec<HalfPlane> = Vec::new();
    for &[a, b, c] in input {
        let length = a.hypot(b);
        if length == 0.0 {
            if c < 0.0 {
                return [0.0; 3];
            }
            continue;
        }
        let cut = [a / length, b / length, c / length];
        if let Some(old) = cuts.iter_mut().find(|h| h[0] == cut[0] && h[1] == cut[1]) {
            old[2] = old[2].min(cut[2]);
        } else {
            cuts.push(cut);
        }
    }
    let mut result = [0.0; 3];
    let mut angles = vec![0.0, 2.0 * PI];
    for &[a, b, c] in &cuts {
        if c.abs() > R {
            continue;
        }
        let base = [-a * c, -b * c];
        let tangent = [b, -a];
        let radius = (R * R - c * c).max(0.0).sqrt();
        let (mut lo, mut hi) = (-radius, radius);
        for &[aa, bb, cc] in &cuts {
            if [aa, bb, cc] == [a, b, c] {
                continue;
            }
            let value = aa * base[0] + bb * base[1] + cc;
            let slope = aa * tangent[0] + bb * tangent[1];
            if slope > 0.0 {
                lo = lo.max(-value / slope);
            } else if slope < 0.0 {
                hi = hi.min(-value / slope);
            } else if value < 0.0 {
                lo = 1.0;
                hi = 0.0;
            }
        }
        if hi > lo {
            let p = [base[0] + lo * tangent[0], base[1] + lo * tangent[1]];
            let q = [base[0] + hi * tangent[0], base[1] + hi * tangent[1]];
            result[0] += (p[0] * q[1] - p[1] * q[0]) / 2.0;
            result[1] += (q[1] - p[1]) * (p[0] * p[0] + p[0] * q[0] + q[0] * q[0]) / 6.0;
            result[2] -= (q[0] - p[0]) * (p[1] * p[1] + p[1] * q[1] + q[1] * q[1]) / 6.0;
        }
        let center = b.atan2(a);
        let angle = (-c / R).acos();
        angles.extend([
            (center - angle).rem_euclid(2.0 * PI),
            (center + angle).rem_euclid(2.0 * PI),
        ]);
    }
    angles.sort_by(f64::total_cmp);
    for pair in angles.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mid = (a + b) / 2.0;
        if !cuts
            .iter()
            .all(|h| R * (h[0] * mid.cos() + h[1] * mid.sin()) + h[2] >= 0.0)
        {
            continue;
        }
        // Stable differences of sin - sin³/3 near the top of the circle.
        let ds = 2.0 * mid.cos() * ((b - a) / 2.0).sin();
        let dc = -2.0 * mid.sin() * ((b - a) / 2.0).sin();
        result[0] += R * R * (b - a) / 2.0;
        result[1] +=
            R.powi(3) / 2.0 * ds * ((a.cos().powi(2) + b.cos().powi(2)) / 2.0 + ds * ds / 6.0);
        result[2] -=
            R.powi(3) / 2.0 * dc * ((a.sin().powi(2) + b.sin().powi(2)) / 2.0 + dc * dc / 6.0);
    }
    result
}

/// Independent quadrature: exact piecewise-linear q integration, Simpson in phi.
fn perturbed_notch_quadrature(plane: SlantPlane) -> f64 {
    let [a, b, c, d] = slant_coefficients(plane);
    let (s, co) = axis_sin_cos();
    let k = s / co;
    let (lo, hi) = (axial_at_z(BOT_Z), axial_at_z(TOP_Z));
    let mut breaks = vec![-PI / 2.0, 0.0, PI / 2.0];
    for bound in [[lo, 0.0], [lo, k], [hi, 0.0], [hi, k]] {
        let x = a + c * bound[1];
        let z = d + c * bound[0];
        if x != 0.0 && (-z / x).abs() < R {
            breaks.push((-z / x / R).asin());
        }
        let length = x.hypot(b);
        if length > 0.0 && z.abs() <= R * length {
            let (nx, ny, distance) = (x / length, b / length, -z / length);
            let radius = (R * R - distance * distance).max(0.0).sqrt();
            for sign in [-1.0, 1.0] {
                let p = nx * distance + sign * ny * radius;
                let q = ny * distance - sign * nx * radius;
                if q >= 0.0 {
                    breaks.push((p / R).clamp(-1.0, 1.0).asin());
                }
            }
        }
    }
    breaks.sort_by(f64::total_cmp);
    let mut volume = 0.0;
    for pair in breaks.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let ignore_plane = c == 0.0 && b == 0.0;
        if ignore_plane && a * (R * ((left + right) / 2.0).sin()) + d > 0.0 {
            continue;
        }
        let value = |phi: f64| {
            let p = R * phi.sin();
            let qmax = (R * R - p * p).max(0.0).sqrt();
            let lower = lo + (k * p).max(0.0);
            let upper = hi + (k * p).min(0.0);
            let height = |q: f64| {
                if ignore_plane {
                    return upper - lower;
                }
                let offset = a * p + b * q + d;
                if c > 0.0 {
                    (upper.min(-offset / c) - lower).max(0.0)
                } else if c < 0.0 {
                    (upper - lower.max(-offset / c)).max(0.0)
                } else if offset <= 0.0 {
                    upper - lower
                } else {
                    0.0
                }
            };
            let mut qs = vec![0.0, qmax];
            if b != 0.0 {
                for z in [lower, upper] {
                    let q = -(a * p + d + c * z) / b;
                    if q > 0.0 && q < qmax {
                        qs.push(q);
                    }
                }
            }
            qs.sort_by(f64::total_cmp);
            let section: f64 = qs
                .windows(2)
                .map(|v| (height(v[0]) + height(v[1])) * (v[1] - v[0]) / 2.0)
                .sum();
            section * R * phi.cos()
        };
        let steps = 512;
        let width = (right - left) / f64::from(steps);
        let mut sum = value(left) + value(right);
        for i in 1..steps {
            let weight = if i % 2 == 0 { 2.0 } else { 4.0 };
            sum += weight * value(left + f64::from(i) * width);
        }
        volume += sum * width / 3.0;
    }
    volume
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeamBand {
    InBand,
    Partial,
    Past,
}

fn seam_band(plane: SlantPlane, tolerance: f64) -> (SeamBand, [f64; 2]) {
    let gaps = [BOT_Z, TOP_Z].map(|z| {
        let p = axis_point(false) + seam(false) * R + axis(false) * axial_at_z(z);
        plane.normal.dot(p - plane.origin)
    });
    let (a, b) = (gaps[0].abs(), gaps[1].abs());
    let band = if a.max(b) <= tolerance {
        SeamBand::InBand
    }
    // Opposite signs with both caps far outside the band describe two
    // separate edges crossing once, rather than wholly coincident edges.
    else if a.min(b) > tolerance {
        SeamBand::Past
    } else {
        SeamBand::Partial
    };
    (band, gaps)
}

struct PerturbedResult {
    faces: usize,
    solid: bool,
    failures: Vec<String>,
    volume: f64,
    expected: f64,
    band: SeamBand,
}

fn measure_perturbed(tilt: f64, shift: f64, repro: bool) -> PerturbedResult {
    let mut k = Kernel::default();
    k.fixes = UpstreamFixes::all();
    let plane = if repro {
        repro_slant()
    } else {
        perturbed_slant(tilt, shift)
    };
    let a = the_box(&mut k);
    let hs = if repro {
        the_repros_half_space(&mut k)
    } else {
        perturbed_half_space(&mut k, plane)
    };
    let tool = the_tool(&mut k, Tool::Primitive, Past(0.0), false);
    let tolerance =
        solidcore::geom::tol::xsect_tol_3d_from_zones(&k, hs.tolerance(), tool.tolerance());
    let (band, gaps) = seam_band(plane, tolerance);
    let clipped = boolean_difference(&mut k, a, hs).expect("half space cut");
    let mut result = boolean_difference(&mut k, clipped, tool).expect("notch cut");
    solidcore::topology::integrity::check(&result).unwrap();
    let faces = solidcore::topology::brep::p1::faces(&result, None)
        .unwrap()
        .len();
    let solid = solidcore::topology::brep::p2::is_manifold_solid(&result).unwrap();
    let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
    let failures: Vec<String> = check
        .reports
        .reports()
        .iter()
        .filter(|r| !r.ok)
        .map(|r| {
            format!(
                "{}#{}:{}",
                r.owner_type_string.unwrap_or("?"),
                r.test_index,
                r.name
            )
        })
        .collect();
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&k, &mut result, 1e-7, &attrs).unwrap();
    let expected = perturbed_expected_volume(plane);
    println!("ROW tilt={tilt:e} shift={shift:e} repro={repro} band={band:?} gaps={gaps:?} tol={tolerance:e} faces={faces} manifold={solid} Level2={failures:?} volume={volume:.12} expected={expected:.12} delta={:.12e}",volume-expected);
    PerturbedResult {
        faces,
        solid,
        failures,
        volume,
        expected,
        band,
    }
}

fn assert_perturbed(tilt: f64, shift: f64, expected_band: SeamBand, faces: usize) {
    let result = measure_perturbed(tilt, shift, false);
    assert_eq!(
        result.band, expected_band,
        "row classification follows the cap gaps"
    );
    assert_eq!(result.faces, faces);
    assert!(result.solid, "closed manifold solid");
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert!((result.volume - result.expected).abs() / result.expected.abs() <= VOLUME_REL_TOL);
}

macro_rules! perturbed_case {
    ($name:ident,$tilt:expr,$shift:expr,$band:ident,$faces:expr) => {
        #[test]
        fn $name() {
            assert_perturbed($tilt, $shift, SeamBand::$band, $faces);
        }
    };
}
perturbed_case!(in_band_tilt_3e11, 3e-11, 0.0, InBand, 8);
perturbed_case!(in_band_tilt_1e10, 1e-10, 0.0, InBand, 8);
perturbed_case!(in_band_tilt_1e9, 1e-9, 0.0, InBand, 8);
perturbed_case!(in_band_tilt_minus_1e7, -1e-7, 0.0, InBand, 8);
perturbed_case!(in_band_tilt_minus_2e7, -2e-7, 0.0, InBand, 8);
perturbed_case!(in_band_tilt_minus_3e7, -3e-7, 0.0, InBand, 8);
perturbed_case!(in_band_shift_1e9, 0.0, 1e-9, InBand, 8);
perturbed_case!(in_band_shift_3e9, 0.0, 3e-9, InBand, 8);
perturbed_case!(in_band_shift_1e8, 0.0, 1e-8, InBand, 8);
perturbed_case!(past_tilt_minus_1e5, -1e-5, 0.0, Past, 9);
perturbed_case!(past_tilt_minus_2e5, -2e-5, 0.0, Past, 9);

fn perturbation_rows() -> [(f64, f64, bool); 16] {
    [
        (0.0, 0.0, false),
        (3e-11, 0.0, false),
        (1e-10, 0.0, false),
        (1e-9, 0.0, false),
        (-1e-7, 0.0, false),
        (-2e-7, 0.0, false),
        (-3e-7, 0.0, false),
        (-4e-7, 0.0, false),
        (-4.46e-7, 0.0, false),
        (-1e-6, 0.0, false),
        (-1e-5, 0.0, false),
        (-2e-5, 0.0, false),
        (0.0, 1e-9, false),
        (0.0, 3e-9, false),
        (0.0, 1e-8, false),
        (0.0, 0.0, true),
    ]
}

#[test]
fn perturbed_closed_forms_agree_with_quadrature() {
    assert!(
        (perturbed_expected_volume(perturbed_slant(0.0, 0.0)) - expected_volume(Past(0.0))).abs()
            < 1e-7
    );
    for (tilt, shift, repro) in perturbation_rows() {
        let plane = if repro {
            repro_slant()
        } else {
            perturbed_slant(tilt, shift)
        };
        let n = plane.normal;
        let y = BOX_LO.y + BOX_SIZE.1 / 2.0;
        let z = (BOT_Z + TOP_Z) / 2.0;
        let prism = BOX_SIZE.1
            * BOX_SIZE.2
            * (plane.origin.x
                - n.y / n.x * (y - plane.origin.y)
                - n.z / n.x * (z - plane.origin.z)
                - BOX_LO.x);
        let closed = perturbed_expected_volume(plane);
        let quadrature = prism - perturbed_notch_quadrature(plane);
        println!("FORM tilt={tilt:e} shift={shift:e} repro={repro} ABCD={:?} closed={closed:.12} quadrature={quadrature:.12} delta={:.3e}",slant_coefficients(plane),closed-quadrature);
        assert!(
            (closed - quadrature).abs() < 1e-7,
            "closed form vs independent quadrature"
        );
    }
    assert!((perturbed_expected_volume(repro_slant()) - 641_498.908_874_33).abs() < 1e-7);
}

#[test]
fn measure_the_full_perturbation_band() {
    for (tilt, shift, repro) in perturbation_rows() {
        let result = measure_perturbed(tilt, shift, repro);
        assert!((result.volume - result.expected).abs() / result.expected.abs() <= VOLUME_REL_TOL);
    }
}
