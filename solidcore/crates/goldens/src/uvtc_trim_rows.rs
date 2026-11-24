//! The golden rows whose UV trims the retirement of `uv_trim_corner_end_points_only`
//! moves: `sm_FixUVTrimCurves` closes a face's trim corners by moving only the trims' end
//! control points (spread over the control points next to the end), no longer by trimming
//! each trim at the vertex's drop point and stretching it back over its interval.
//!
//! Each row is replayed with `dump::capture` on; the replay reproduces the blessed row, so
//! the captured B-rep (`"uv_trims"`, dumped after the operation whose trims the row
//! records) is the one the row measures. Every UV trim of it is certified on its edge
//! (`trims_on_edges::certify_brep`: a rounded-interval bound on the distance between the
//! trim's image on its face and the edge's curve, both ways), within the edge's tolerance.
//!
//! Run (each replay certifies every trim of a body, so `#[ignore]`d; the goldens job checks
//! the rows themselves):
//! `cargo test --profile goldens -p goldens --lib uvtc_trim_rows -- --include-ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr
)]

use solidcore::kernel::Kernel;

use crate::runner::UnitDiff;
use crate::trims_on_edges::certify_brep;
use crate::units::{topology_brep_p10, topology_brep_p3, topology_face_p2, topology_face_p3};

/// The moved rows, as (unit, case). `uv_trim_f4_rebuild` and `topology.brep_p10`'s `gen0115`
/// (its read's trims, dumped before its op) move within the golden compare's `1e-9` (each
/// blessed line is main's), so each replay is checked against the line by that compare, as
/// every row's is.
const ROWS: [(&str, &str); 18] = [
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_46"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_46_gaps"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_46_gaps_own"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_46_own"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_47"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_47_gaps"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_47_gaps_own"),
    ("topology.face_p3", "Mouse_rebuilt_uv_trim_47_own"),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f4_rebuild",
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f6_rebuild",
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f7",
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f7_rebuild",
    ),
    ("topology.brep_p3", "nurbs_Mouse"),
    ("topology.brep_p3", "nurbs_Mouse_rebuild"),
    ("topology.brep_p3", "nurbs_off_Mouse_-0.05"),
    ("topology.brep_p3", "nurbs_off_Mouse_0.1"),
    ("topology.face_p2", "fix_file_Mouse.smb_0"),
    ("topology.brep_p10", "gen0115"),
];

/// Every moved row's replay reproduces the row, and every UV trim of the B-rep it records
/// lies on its edge within the edge's tolerance.
#[test]
#[ignore = "certifies every trim of each row's body (minutes); see the module docs"]
fn uvtc_moved_trim_rows_lie_on_their_edges() {
    let dir = crate::golden::default_dir();
    for (unit, name) in ROWS {
        let runner: &dyn UnitDiff = match unit {
            "topology.face_p3" => &topology_face_p3::Runner,
            "topology.brep_p3" => &topology_brep_p3::Runner,
            "topology.brep_p10" => &topology_brep_p10::Runner,
            _ => &topology_face_p2::Runner,
        };
        let case = crate::case::regression_case(runner, name);
        crate::dump::capture();
        let out = runner.run_rust(&case).unwrap();
        let got = crate::dump::take();
        let rows = crate::golden::read(&dir, unit).unwrap();
        assert!(
            crate::golden::diff(
                &rows[name],
                &crate::golden::encode(name, &out),
                runner.abs_floors()
            )
            .is_none(),
            "{unit} {name}: the replay does not reproduce the row"
        );
        let bodies: Vec<_> = got.into_iter().filter(|c| c.label == "uv_trims").collect();
        assert_eq!(bodies.len(), 1, "{unit} {name}: one recorded B-rep");
        for c in bodies {
            let b = c.brep.unwrap();
            let certs = certify_brep(&Kernel::default(), &b).unwrap();
            assert!(!certs.is_empty(), "{unit} {name}: no trims");
            let worst = certs
                .iter()
                .map(|c| c.bound / c.tolerance)
                .fold(0.0, f64::max);
            eprintln!(
                "UVTC {unit} {name}: {} trims certified, worst bound/tolerance {worst:e}",
                certs.len()
            );
            for t in &certs {
                assert!(
                    t.holds(),
                    "{unit} {name}: edge use {:?}'s trim is {:e} off edge {:?} (tolerance {:e})",
                    t.edgeuse,
                    t.bound,
                    t.edge,
                    t.tolerance
                );
            }
        }
    }
}
