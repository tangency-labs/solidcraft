//! WR-458, WR-633 and WR-650 (#18921): an offset surface extends from its base surface,
//! not from its last extension, and a cylinder, cone, sphere, torus or circle revolution
//! extends as itself. The real-solid rows this moves keep their status and counts and
//! change only their topology order, geometry and mass values. Each row is checked here,
//! one test per row, on the body the golden records (checked by a [`with_hook`] hook at the
//! record's own [`observe`] call, so it is the golden's body bit for bit):
//! - the record's status is a success and the body's vertex, edge and face counts are the
//!   golden's;
//! - `check_brep` Level 2 reports nothing and, for a closed result, the body is one closed
//!   manifold solid;
//! - every blend face that is a definition re-certifies its net within its bound;
//! - for a closed result, the volume the mass integrator converges to lies in the body's
//!   certified volume enclosure ([`enclose_volume`], widened by the definitions' spread),
//!   and every other mass value lies in its certified band ([`mass_bands`]). This is a
//!   consistency check: it certifies that the recorded values are this body's.
//!
//! [`with_hook`]: crate::trim_hook::with_hook
//! [`observe`]: crate::trim_hook::observe
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
//! [`mass_bands`]: super::round_definition_proof::mass_bands

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::round_definition_proof::{mass_bands, volume_band};
use super::round_definition_rows::{counts, inputs};
use super::*;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::FaceId;
use std::cell::RefCell;
use std::rc::Rc;

/// A moved row: name, main's volume (`None`: an open result), and its status and counts
/// (the same on main).
type Row = (&'static str, Option<f64>, [i64; 4]);

const ROWS: &[Row] = &[
    ("corner3x2-union-inset-side-step-edges1-14-r1", Some(2600.6420851684975), [0, 25, 38, 15]),
    ("corner3x2-union-inset-side-step-edges12-13-r1", Some(2600.6420849381543), [0, 25, 38, 15]),
    ("curve-sih-box_diff_cyl-all-edges-r0.25", Some(949.2763524350879), [0, 40, 76, 39]),
    ("issue_2020_mitred_put_back_d9_full", Some(1006.3206273131401), [0, 8, 12, 6]),
    ("issue_2176_tabbed_plate_tab_side_r0_5", Some(4054.672334381553), [0, 14, 21, 9]),
    ("issue_2176_tabbed_plate_tab_top_r0_5", Some(4054.77021264696), [0, 14, 21, 9]),
    ("source-file-box_with_large_fillet-edge4", Some(862.6278909025616), [0, 14, 22, 10]),
    ("source-file-corner_box-all-edges", Some(455.9755263923814), [0, 30, 60, 32]),
    ("source-file-nurb_box-all-edges", Some(899.6454001648498), [0, 24, 48, 26]),
    ("stubs3-union-inset-side-step-edges13-17-r1", Some(2602.8856942157045), [0, 23, 35, 14]),
    ("stubs3-union-inset-side-step-edges14-15-r1", Some(2603.9587033987177), [0, 23, 35, 14]),
    ("stubs3-union-inset-side-step-edges14-18-r1", Some(2602.8856942157045), [0, 23, 35, 14]),
    ("stubs3-union-overhang-corner-edges14-15-r1", Some(2602.6710923791024), [0, 23, 35, 14]),
    ("stubs3-union-overhang-front-edges14-15-r1", Some(2603.100296052308), [0, 22, 34, 14]),
    ("stubs4-Heart-all-edges-r0.1", Some(326.05828850759485), [0, 14, 26, 14]),
    ("stubs4-coons_01_03_29-e3-r0.5", None, [0, 8, 12, 5]),
    ("stubs4-degenerate-brepForFillet2-e7-r0.5", Some(3614.981459311338), [0, 11, 17, 8]),
    ("stubs4-degenerate-brepForFillet2-e9-r0.5", Some(3616.232731993582), [0, 11, 17, 8]),
    ("sweep5-ext_w_hole-e15-r1.0", Some(234.29244337666438), [0, 16, 24, 10]),
    ("sweep5-nurb_box1-all-r0.5", Some(3793.323246921), [0, 18, 36, 20]),
];

/// One test per moved row, so the test runner spreads the rows over its threads.
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
    corner3x2_union_inset_side_step_edges12_13_r1 => "corner3x2-union-inset-side-step-edges12-13-r1",
    curve_sih_box_diff_cyl_all_edges_r0_25 => "curve-sih-box_diff_cyl-all-edges-r0.25",
    issue_2020_mitred_put_back_d9_full => "issue_2020_mitred_put_back_d9_full",
    issue_2176_tabbed_plate_tab_side_r0_5 => "issue_2176_tabbed_plate_tab_side_r0_5",
    issue_2176_tabbed_plate_tab_top_r0_5 => "issue_2176_tabbed_plate_tab_top_r0_5",
    source_file_box_with_large_fillet_edge4 => "source-file-box_with_large_fillet-edge4",
    source_file_corner_box_all_edges => "source-file-corner_box-all-edges",
    source_file_nurb_box_all_edges => "source-file-nurb_box-all-edges",
    stubs3_union_inset_side_step_edges13_17_r1 => "stubs3-union-inset-side-step-edges13-17-r1",
    stubs3_union_inset_side_step_edges14_15_r1 => "stubs3-union-inset-side-step-edges14-15-r1",
    stubs3_union_inset_side_step_edges14_18_r1 => "stubs3-union-inset-side-step-edges14-18-r1",
    stubs3_union_overhang_corner_edges14_15_r1 => "stubs3-union-overhang-corner-edges14-15-r1",
    stubs3_union_overhang_front_edges14_15_r1 => "stubs3-union-overhang-front-edges14-15-r1",
    stubs4_heart_all_edges_r0_1 => "stubs4-Heart-all-edges-r0.1",
    stubs4_coons_01_03_29_e3_r0_5 => "stubs4-coons_01_03_29-e3-r0.5",
    stubs4_degenerate_brepforfillet2_e7_r0_5 => "stubs4-degenerate-brepForFillet2-e7-r0.5",
    stubs4_degenerate_brepforfillet2_e9_r0_5 => "stubs4-degenerate-brepForFillet2-e9-r0.5",
    sweep5_ext_w_hole_e15_r1_0 => "sweep5-ext_w_hole-e15-r1.0",
    sweep5_nurb_box1_all_r0_5 => "sweep5-nurb_box1-all-r0.5",
}

/// Every row of [`ROWS`] has exactly one test.
#[test]
fn every_moved_row_has_its_own_test() {
    let rows: Vec<&str> = ROWS.iter().map(|row| row.0).collect();
    assert_eq!(rows, ROW_TESTS);
}

/// The faces of `b` whose surface is a blend by its definition, each with its proved net
/// distance (`recertify`, at most its bound); none is fine (a bare net has no spread).
fn definitions(k: &Kernel, b: &Brep, name: &str) -> Vec<(FaceId, f64)> {
    let mut blends = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let surface = b.face(f).unwrap().surface.clone();
        let Some(blend) = surface.as_ref().and_then(Surface::procedural_blend) else {
            continue;
        };
        let d = blend
            .recertify(k)
            .unwrap_or_else(|e| panic!("{name}: {f:?} does not re-certify: {e}"));
        let bound = blend.bound();
        assert!(d <= bound, "{name}: {f:?} proved {d:e} past its bound {bound:e}");
        blends.push((f, d));
    }
    blends
}

/// The [`ROWS`] row named `row`, as the module header says.
fn check(row: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.0 == row).collect();
    assert_eq!(picked.len(), 1, "{row}: one row of that name");
    for &&(name, old_volume, after) in &picked {
        let input = inputs(name).real_solid.expect("a real solid");
        assert_eq!(input.checkpoint, Checkpoint::Full, "{name}");
        let seen: Rc<RefCell<usize>> = Rc::default();
        let into = Rc::clone(&seen);
        // the checks run on the recorded body itself, at the record's `observe` call
        let output = crate::trim_hook::with_hook(
            Box::new(move |body: &mut Brep| {
                *into.borrow_mut() += 1;
                certify(body, name, old_volume, after);
            }),
            || rust(&input),
        );
        let output = output.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let recorded = output.ints.get("api_status_vertex_edge_face_counts");
        assert_eq!(recorded.map(Vec::as_slice), Some(&after[..]), "{name}: a success, the golden's counts");
        assert_eq!(*seen.borrow(), 1, "{name}: one recorded body");
    }
}

/// The certificates of the recorded body of row `name`, as the module header says.
fn certify(body: &mut Brep, name: &str, old_volume: Option<f64>, after: [i64; 4]) {
    assert_eq!(counts(body, 0), after, "{name}");
    let mut k = Kernel::default();
    let lines = check_brep(&mut k, body, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
    let blends = definitions(&k, body, name);
    if let Some(old) = old_volume {
        assert!(is_manifold_solid(body).unwrap(), "{name}: not one closed manifold solid");
        let estimates = volume_band(&k, body, &blends, old, name);
        mass_bands(&k, body, &estimates, name);
    }
}

/// The surface-surface row the extension moves (`surface-surface-mirror-complement-approx`:
/// a round between two bilinear sheets, open). It moves only its geometry. Its result is
/// one face, a blend by its definition whose net re-certifies within its bound, and its
/// counts are the golden's.
#[test]
fn surface_surface_row_is_a_certified_definition() {
    use solidcore::blend::fillet_executive::api::{surface_surface_fillet, FilletSurfaceArg};
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    let name = "surface-surface-mirror-complement-approx";
    let input = inputs(name).surface_surface.expect("a surface-surface input");
    let mut k = crate::runner_kernel();
    let sheet = |c: &[[f64; 3]; 4]| {
        let [a, b, c, d] = c.map(Vec3::from);
        Surface::from(BSplineSurface::create_bilinear_surface(a, b, c, d).unwrap())
    };
    let (first, second) = (sheet(&input.first), sheet(&input.second));
    let body = surface_surface_fillet(
        &mut k,
        FilletSurfaceArg::Free(&first),
        FilletSurfaceArg::Free(&second),
        input.radius1,
        input.radius2,
        input.tolerance,
        input.xsect,
        input.mirror,
        input.complement,
    )
    .unwrap()
    .expect("a result body");
    assert_eq!(counts(&body, 0), [0, 4, 4, 1], "{name}");
    let blends = definitions(&k, &body, name);
    assert_eq!(blends.len(), 1, "{name}: its one face is a definition");
}
