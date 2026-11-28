//! WR-613 (#18640): a fillet corner patch between a face of the body and a round may take
//! the round's twist at that corner, so the patch meets the face exactly (the kernel's
//! own tests, `face_sided_patch` in `nurbs::b_spline_surface_n_lib::p2`, bound its seams).
//! Each row with corner-patch sides on plane faces is checked here, on the finished body:
//! - `check_brep` Level 2 reports nothing and the body is one closed manifold solid, and
//!   the golden's mass status is 0;
//! - the corner-patch sides on plane faces number [`Row::face_sides`];
//! - on the rows that take a face-sided patch ([`Row::enclosed`]), the row's volume lies
//!   in the body's certified volume enclosure ([`enclose_volume`]): a consistency check
//!   of the mass integrator on the new body.
//!
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume

use super::{apply, build, RealSolid};
use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::math::Vec3;
use solidcore::query::continuity::{seam_angle_rounding, SEAM_DOT_ROUNDING};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology_traverser::face_edges;
use solidcore::topology::Brep;
use solidcore::math::det;

/// A row with corner-patch sides on plane faces.
struct Row {
    /// The row's name.
    name: &'static str,
    /// Whether the row takes a face-sided patch at some corner; its volume is then
    /// checked against the body's certified enclosure.
    enclosed: bool,
    /// The corner-patch sides on plane faces.
    face_sides: usize,
}

#[test]
fn angle_rounding_bounds_acos() {
    // the floor is the half-order loss of acos next to 1
    let c = 1.0 - SEAM_DOT_ROUNDING;
    assert!(det::acos(c) <= seam_angle_rounding(0.0));
    // away from 0 it is the first-order bound, and smaller
    assert!(seam_angle_rounding(6.8e-4) < 5e-12);
    assert!(seam_angle_rounding(6.8e-4) >= SEAM_DOT_ROUNDING / det::sin(6.8e-4_f64));
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
fn patch_sides(b: &Brep) -> usize {
    let faces = p1::faces(b, None).expect("faces");
    let mut planes = Vec::new();
    for &f in &faces {
        let s = solidcore::topology::face::surface(b, f)
            .expect("surface")
            .expect("a surface");
        if let SurfaceKind::Plane(p) = s.kind() {
            let (o, n, _) = p.is_planar();
            planes.push((f, o, n));
        }
    }
    let mut count = 0;
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
        let (nu, nv) = (net.u_count, net.v_count);
        let at = |i: usize, j: usize| net.points[i * nv + j];
        // each side's boundary row
        let rows: [Vec<_>; 4] = [
            (0..nv).map(|j| at(0, j)).collect(),
            (0..nv).map(|j| at(nu - 1, j)).collect(),
            (0..nu).map(|i| at(i, 0)).collect(),
            (0..nu).map(|i| at(i, nv - 1)).collect(),
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
            for boundary in &rows {
                let on = boundary.iter().all(|&p| d(p).abs() <= 1e-9);
                let span = boundary[0].distance(boundary[boundary.len() - 1]);
                if on && span > 1e-9 {
                    count += 1;
                }
            }
        }
    }
    count
}

/// The row named `name`, checked on its finished body.
fn check(name: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.name == name).collect();
    assert_eq!(picked.len(), 1, "{name}: one row of that name");
    let row = picked[0];
    let input = load(name);
    let mut k = Kernel::default();
    let mut body = body(&mut k, &input, name);
    // (the enclosure below measures this copy, the body as built: the check writes edge
    // curves)
    let mut measured = body.clone_standing_for_self().expect("copy");
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

    assert_eq!(
        patch_sides(&body),
        row.face_sides,
        "{name}: corner-patch sides on plane faces"
    );
    if !row.enclosed {
        return;
    }

    // the integrator's volume of the new body lies in its certified enclosure
    let e = enclose_volume(&k, &mut measured, 1e-7 * volume).expect("enclosure");
    eprintln!(
        "{name}: volume in [{}, {}] (width {:e}, {:?}), row {volume}",
        e.lo,
        e.hi,
        e.width(),
        e.stop,
    );
    assert!(
        e.lo <= volume && volume <= e.hi,
        "{name}: row volume {volume} not in [{}, {}]",
        e.lo,
        e.hi
    );
}

/// The 24 rows with corner-patch sides on plane faces.
const ROWS: &[Row] = &[
    Row { name: "corner3x2-union-inset-side-step-edges1-14-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges1-18-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges12-13-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges12-17-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges13-19-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges14-27-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges15-19-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-inset-side-step-edges15-27-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-overhang-corner-edges6-12-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-overhang-corner-edges6-13-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-overhang-front-edges15-22-r1", enclosed: false, face_sides: 1 },
    Row { name: "corner3x2-union-overhang-front-edges15-24-r1", enclosed: false, face_sides: 1 },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.1", enclosed: true, face_sides: 4 },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.25", enclosed: true, face_sides: 4 },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.5", enclosed: true, face_sides: 4 },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r1.0", enclosed: true, face_sides: 4 },
    Row { name: "curve-sih-box_diff_cyl-all-edges-step2", enclosed: true, face_sides: 4 },
    Row { name: "source-file-box_diff_cyl-all-edges", enclosed: true, face_sides: 2 },
    Row { name: "stubs3-union-inset-side-step-edges16-22-r1", enclosed: false, face_sides: 1 },
    Row { name: "stubs3-union-inset-side-step-edges17-22-r1", enclosed: false, face_sides: 1 },
    Row { name: "stubs3-union-overhang-corner-edges14-22-r1", enclosed: false, face_sides: 1 },
    Row { name: "stubs3-union-overhang-corner-edges15-22-r1", enclosed: false, face_sides: 1 },
    Row { name: "stubs3-union-overhang-front-edges14-22-r1", enclosed: false, face_sides: 1 },
    Row { name: "stubs4-Heart-all-edges-r0.1", enclosed: false, face_sides: 2 },
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
