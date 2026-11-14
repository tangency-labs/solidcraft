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
/// That puts the whole seam in the band between `has_near_misses`'s threshold
/// (`EFF_ZERO * 1000` = 1e-09 mm, `intersect/curve_class/p7.rs`) and the bodies' 2e-05
/// tolerance, where the seam still classifies as coincident with that edge but not
/// exactly on it. Face test 36 fires, and at this gap the merge also leaves a ninth,
/// sliver face and an edge whose radial angles do not increase, so the result is no
/// longer a closed solid.
///
/// What must happen: the same one closed solid of 8 faces the exact plane gives, of
/// 641498.913182 mm3 (the closed form plus the 0.056814 mm3 the offset plane adds to
/// the prism, derived in the finding). This test holds today's answer instead, so that
/// a fix shows up as this test failing.
///
/// Since #15718 (exactly placed definitions combine within their summed tolerances, not
/// five times them) the defect has changed form: the result is one closed solid that
/// passes Level 2, but still has 9 faces. The ninth is a sliver triangle whose base is the
/// two cap points 2.36e-5 apart (an Edge/Face and a Vertex/Face definition, each of
/// tolerance 1e-5, kept apart at their summed radius) and whose two long edges, 52.6 long,
/// diverge linearly from a shared apex, so they lie within the intersection tolerance
/// 2e-5 of each other over the first 85% of their length: two coincident edges
/// (validity invariant 7), which Level 2 does not check. The target stays 8 faces
/// (tangency-labs/solidcraft#16349).
#[test]
fn the_repros_rounded_slant_plane_leaves_a_sliver_near_the_seam() {
    use solidcore::topology::brep::p1 as bp1;
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
    assert!(solid, "today: a closed manifold solid");
    assert!(check.is_valid(), "today: Level 2 passes: {names:?}");
    // the sliver: a triangle of area about 6.2e-4 whose base, between the two cap points,
    // is longer than the intersection tolerance 2e-5 but under 3e-5, and whose two other
    // edges, over 50 long, meet at an apex; so they lie within 2e-5 of each other over
    // more than 2/3 of their length
    let mut slivers = Vec::new();
    for f in bp1::faces(&brep, None).unwrap() {
        let area =
            solidcore::mass::integrator::properties::face_area(&k, &mut brep, f, 1e-9).unwrap();
        if area >= 1e-2 {
            continue;
        }
        // each side: its chord length and N points along its curve
        let mut sides: Vec<(f64, Vec<Point3>)> =
            solidcore::topology::face::p4::edges(&brep, f, None, false, None)
                .unwrap()
                .into_iter()
                .map(|e| {
                    let ed = brep.edge(e).unwrap();
                    let c = ed.curve.as_ref().unwrap();
                    let n = 4000;
                    let pts: Vec<Point3> = (0..=n)
                        .map(|i| {
                            c.evaluate_point(&k, ed.interval.evaluate(f64::from(i) / f64::from(n)))
                                .unwrap()
                        })
                        .collect();
                    (pts[0].distance(pts[n as usize]), pts)
                })
                .collect();
        sides.sort_by(|a, b| a.0.total_cmp(&b.0));
        slivers.push((area, sides));
    }
    assert_eq!(slivers.len(), 1, "one sliver face: {slivers:?}");
    let (area, sides) = &slivers[0];
    let lengths: Vec<f64> = sides.iter().map(|s| s.0).collect();
    assert!((5e-4..8e-4).contains(area), "sliver area {area}");
    assert_eq!(lengths.len(), 3, "a triangle: {lengths:?}");
    let tol = 2e-5;
    assert!(
        lengths[0] > tol && lengths[0] < 3e-5,
        "sliver base {lengths:?}"
    );
    assert!(
        lengths[1] > 50.0 && lengths[2] > 50.0,
        "sliver sides {lengths:?}"
    );
    // How much of each long side lies within the tolerance of the other, measured on the
    // curves: 4001 points along each side, each one's distance to the other side's
    // polyline through its 4001 points, the share of the side's polyline length whose
    // both ends are within the tolerance. (The polyline is within its chord sagitta of
    // the curve; for these 52-long, nearly straight sides that is far below 2e-5.)
    let to_polyline = |p: Point3, line: &[Point3]| {
        line.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let ab = b - a;
                let len2 = ab.dot(ab);
                let t = if len2 > 0.0 {
                    ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                p.distance(a + ab * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let within = |side: &[Point3], other: &[Point3]| {
        let near: Vec<bool> = side.iter().map(|&p| to_polyline(p, other) <= tol).collect();
        let (mut inside, mut total) = (0.0, 0.0);
        for (w, n) in side.windows(2).zip(near.windows(2)) {
            let l = w[0].distance(w[1]);
            total += l;
            if n[0] && n[1] {
                inside += l;
            }
        }
        inside / total
    };
    let f1 = within(&sides[1].1, &sides[2].1);
    let f2 = within(&sides[2].1, &sides[1].1);
    println!("long sides within the tolerance of each other: {f1:.4}, {f2:.4}");
    assert!(
        (0.8..0.9).contains(&f1) && (0.8..0.9).contains(&f2),
        "the long sides lie within {tol} of each other over 80-90% of their length: {f1}, {f2}"
    );
}
