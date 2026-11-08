//! The cases of #2581, pinned against today's reader so that the port into the reader of
//! #2578 has an acceptance test: `FACETED_BREP` bodies whose faces are bounded by
//! `POLY_LOOP`s, and faces no shell references, which become single-face sheet bodies.
//!
//! Expected values are the file's own, never the reader's output. The synthetic cube is a
//! 10 mm cube, so its volume is 1000 in closed form. The corpus files carry no
//! `GEOMETRIC_VALIDATION_PROPERTY` and no closed form for their geometry, so only counts
//! and closedness are asserted, and the counts come from each file's entity census:
//!
//! | file | `FACETED_BREP` | `MANIFOLD_SOLID_BREP` | faces | bodies |
//! |---|---|---|---|---|
//! | calo | 5 | 0 | 1244 | 5 |
//! | cylcub | 2 | 0 | 24 | 2 |
//! | bernetl | 4 | 1 | 439 | 5 |
//! | cubsomcy | 1 | 1 | 10 | 2 |
//! | clip | 1 (as `FCTBR`) | 0 | 155 | 155 today, 1 declared |
//! | socks | 1 (as `FCTBR`) | 0 | 768 | 768 today, 1 declared |
//!
//! A face count is `FACE_SURFACE` plus `ADVANCED_FACE`; `POLY_LOOP` is larger than the
//! face count where faces have inner loops (calo: 1352 loops over 1244 faces).
//!
//! The corpus tests are ignored by default; the corpus is not in the repository. Fetch it
//! with `scripts/fetch-steptools-corpus.sh` and run them with its directory in
//! `SOLIDCORE_STEP_CORPUS`:
//!
//! ```sh
//! SOLIDCORE_STEP_CORPUS=corpus/steptools cargo test --release -p step-brep \
//!     --test issue_2581_faceted_brep -- --ignored --nocapture
//! ```

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

use std::path::{Path, PathBuf};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::Brep;
use step_brep::{import_with_report, is_closed_solid, ImportReport};
use step_import::Limits;

/// A 10 mm cube as a `FACETED_BREP`: six `FACE`s without a surface, each bounded by a
/// four-point `POLY_LOOP`, counter-clockwise seen from outside, in one `CLOSED_SHELL`.
const CUBE: &str = "#101=CARTESIAN_POINT('',(0.0,0.0,0.0));\
#102=CARTESIAN_POINT('',(10.0,0.0,0.0));\
#103=CARTESIAN_POINT('',(10.0,10.0,0.0));\
#104=CARTESIAN_POINT('',(0.0,10.0,0.0));\
#105=CARTESIAN_POINT('',(0.0,0.0,10.0));\
#106=CARTESIAN_POINT('',(10.0,0.0,10.0));\
#107=CARTESIAN_POINT('',(10.0,10.0,10.0));\
#108=CARTESIAN_POINT('',(0.0,10.0,10.0));\
#200=FACE('',(#201));#201=FACE_OUTER_BOUND('',#202,.T.);\
#202=POLY_LOOP('',(#101,#104,#103,#102));\
#210=FACE('',(#211));#211=FACE_OUTER_BOUND('',#212,.T.);\
#212=POLY_LOOP('',(#105,#106,#107,#108));\
#220=FACE('',(#221));#221=FACE_OUTER_BOUND('',#222,.T.);\
#222=POLY_LOOP('',(#101,#102,#106,#105));\
#230=FACE('',(#231));#231=FACE_OUTER_BOUND('',#232,.T.);\
#232=POLY_LOOP('',(#102,#103,#107,#106));\
#240=FACE('',(#241));#241=FACE_OUTER_BOUND('',#242,.T.);\
#242=POLY_LOOP('',(#103,#104,#108,#107));\
#250=FACE('',(#251));#251=FACE_OUTER_BOUND('',#252,.T.);\
#252=POLY_LOOP('',(#104,#101,#105,#108));\
#300=CLOSED_SHELL('',(#200,#210,#220,#230,#240,#250));\
#301=FACETED_BREP('cube',#300);\
#310=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
#311=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#310)) \
REPRESENTATION_CONTEXT('',''));\
#312=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#301),#311);";

/// `data` as a Part 21 file.
fn part21(data: &str) -> Vec<u8> {
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;\
         DATA;{data}ENDSEC;END-ISO-10303-21;"
    )
    .into_bytes()
}

/// The volume of `brep`, preserving the integrator's error on failure.
fn volume(brep: &Brep) -> solidcore::Result<f64> {
    let kernel = Kernel::default();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = brep.clone_standing_for_self()?;
    brep_volume(&kernel, &mut copy, 1e-9, &attrs)
}

/// (bodies, closed bodies, faces over all bodies) of `report`.
fn counts(report: &ImportReport) -> (usize, usize, usize) {
    (
        report.bodies.len(),
        report.bodies.iter().filter(|b| is_closed_solid(b)).count(),
        report.bodies.iter().map(|b| b.face_list().len()).sum(),
    )
}

/// The corpus directory, or `None` when `SOLIDCORE_STEP_CORPUS` is unset.
fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("SOLIDCORE_STEP_CORPUS").map(PathBuf::from)
}

/// `name` of the pinned AP203 set, read in place.
fn corpus_file(dir: &Path, name: &str) -> Vec<u8> {
    let path = dir.join("ap203").join(format!("{name}.stp"));
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}; fetch the corpus with scripts/fetch-steptools-corpus.sh",
            path.display()
        )
    })
}

/// A 10 mm `FACETED_BREP` cube of `POLY_LOOP` faces is one closed solid of six faces and
/// volume 1000, the closed form of its edge length. Each `FACE` has no surface of its
/// own, so its plane is the plane of its loop's points.
#[test]
fn faceted_brep_cube_of_poly_loops_has_volume_1000() {
    let report = import_with_report(&part21(CUBE), &Limits::default()).expect("imports");
    assert_eq!(counts(&report), (1, 1, 6), "(bodies, closed, faces)");
    assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
    let measured = volume(&report.bodies[0]).expect("a volume");
    assert!(
        (measured - 1000.0).abs() <= 1e-9 * 1000.0,
        "volume {measured}, closed form 1000"
    );
}

/// A standalone `FACE` without a shell imports as one open single-face sheet body.
/// This keeps loose-face import covered independently of whether `FCTBR` is recognised.
#[test]
fn standalone_face_without_shell_is_single_face_sheet() {
    let data = "#101=CARTESIAN_POINT('',(0.0,0.0,0.0));\
#102=CARTESIAN_POINT('',(10.0,0.0,0.0));\
#103=CARTESIAN_POINT('',(10.0,10.0,0.0));\
#104=CARTESIAN_POINT('',(0.0,10.0,0.0));\
#200=FACE('',(#201));#201=FACE_OUTER_BOUND('',#202,.T.);\
#202=POLY_LOOP('',(#101,#102,#103,#104));";
    let report = import_with_report(&part21(data), &Limits::default()).expect("imports");
    assert_eq!(counts(&report), (1, 0, 1), "(bodies, closed, faces)");
    assert!(report.skipped.is_empty(), "skipped: {:?}", report.skipped);
}

/// The four faceted AP203 corpus files of #2581 import as closed solids, one per
/// `FACETED_BREP` or `MANIFOLD_SOLID_BREP`, with every face of the file in a body. The
/// expected numbers are the file's entity census (see the table above), not the reader's
/// output; their geometry has no validation property and no closed form, so volume is not
/// asserted.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn faceted_corpus_files_are_closed_solids() {
    // (file, bodies, faces)
    const CASES: [(&str, usize, usize); 4] = [
        ("calo", 5, 1244),
        ("cylcub", 2, 24),
        ("bernetl", 5, 439),
        ("cubsomcy", 2, 10),
    ];
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut failures = Vec::new();
    for (name, bodies, faces) in CASES {
        let bytes = corpus_file(&dir, name);
        let report = match import_with_report(&bytes, &Limits::default()) {
            Ok(report) => report,
            Err(error) => {
                failures.push(format!("{name}: import failed: {}", error.message));
                continue;
            }
        };
        let got = counts(&report);
        println!("{name}: (bodies, closed, faces) {got:?}");
        if got != (bodies, bodies, faces) {
            failures.push(format!(
                "{name}: (bodies, closed, faces) {got:?}, expected ({bodies}, {bodies}, {faces})"
            ));
        }
        if !report.skipped.is_empty() {
            failures.push(format!("{name}: skipped {:?}", report.skipped));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `clip` and `socks` hold 155 and 768 faces, whichever way the reader groups them: today
/// as that many single-face sheet bodies, and as one closed solid once the file's body
/// entity is recognised (see [`short_name_faceted_brep_is_not_a_body_known_failing`]).
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn loose_face_corpus_files_hold_their_faces() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut failures = Vec::new();
    for (name, faces) in [("clip", 155), ("socks", 768)] {
        let report =
            import_with_report(&corpus_file(&dir, name), &Limits::default()).expect("imports");
        let (bodies, closed, got) = counts(&report);
        println!("{name}: {bodies} bodies, {closed} closed, {got} faces");
        if got != faces {
            failures.push(format!("{name}: {got} faces, expected {faces}"));
        }
        if !report.skipped.is_empty() {
            failures.push(format!("{name}: skipped {:?}", report.skipped));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `clip` and `socks` are faceted solids written in the Part 21 short names, and each
/// declares one `CLSSHL` held by one `FCTBR` (`#1593` and `#7197`). `FCTBR` is not among
/// the eleven short names `step_import::import` expands, so no body entity is recognised,
/// the shell's faces are referenced by nothing the mapper knows, and the file arrives as
/// 155 and 768 open single-face sheet bodies instead of one closed solid each.
///
/// Measured: substituting `FACETED_BREP` for `FCTBR` in the file's bytes, and changing
/// nothing else, makes `clip` one closed solid of 155 faces and `socks` one closed solid
/// of 768 faces. The short names themselves are #2582's subject, which asks for them per
/// entity in the parser rather than by text replacement over the file.
///
/// Fails when the files import as the solid they declare, which is the point: the sheet
/// bodies below are what #2581 lists as expected, and they are a consequence of the
/// missing name, not of the files.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn short_name_faceted_brep_is_not_a_body_known_failing() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut recognised = Vec::new();
    for (name, faces) in [("clip", 155), ("socks", 768)] {
        let report =
            import_with_report(&corpus_file(&dir, name), &Limits::default()).expect("imports");
        let got = counts(&report);
        println!("{name}: (bodies, closed, faces) {got:?}");
        if got != (faces, 0, faces) {
            recognised.push(format!(
                "{name}: (bodies, closed, faces) {got:?}, known failing as ({faces}, 0, {faces})"
            ));
        }
    }
    assert!(
        recognised.is_empty(),
        "the short-name body is recognised now; move these to the closed-solid test:\n{}",
        recognised.join("\n")
    );
}
