//! The cases of #2582, pinned against today's reader (`step_import` + `step_brep`) so that
//! the port into the reader of #2578 has an acceptance test: the Part 21 short entity
//! names, the whole-file dangling-reference report, and the checks on the file's
//! PCURVEs.
//!
//! **Short names.** Each case is a small synthetic file written with the long names, whose
//! expected result is a closed form, and the same file with one entity written in its
//! short name. A short name is read correctly when both files import the same. Each
//! short name is the one ISO's published short-name list for ISO 10303 gives for its
//! entity (<https://standards.iso.org/iso/10303/tech/short_names/short-names.txt>, one
//! name per entity across all parts, so it holds for the AP203 `CONFIG_CONTROL_DESIGN` and
//! AP214 `AUTOMOTIVE_DESIGN` schemas alike), and each occurs in the corpus: `steptools/ap203/clip.stp` and
//! `socks.stp` (CATIA V4 exports) write every entity in its short form. They are read
//! today through a text replacement over the file (`step_import::import`), for eleven
//! simple-instance names; #2582 asks for the mapping per entity in the parser. Today:
//!
//! | short name | entity | today |
//! |---|---|---|
//! | `FCSRF` | `FACE_SURFACE` | read |
//! | `FCOTBN` | `FACE_OUTER_BOUND` | read |
//! | `PLYLP` | `POLY_LOOP` | read |
//! | `CRTPNT` | `CARTESIAN_POINT` | read |
//! | `DRCTN` | `DIRECTION` | read |
//! | `A2PL3D` | `AXIS2_PLACEMENT_3D` | read |
//! | `CLSSHL` | `CLOSED_SHELL` | read |
//! | `BSSWK` | `B_SPLINE_SURFACE_WITH_KNOTS` | read |
//! | `BSCWK` | `B_SPLINE_CURVE_WITH_KNOTS` | read |
//! | `TRMCRV` | `TRIMMED_CURVE` | read |
//! | `FCTBR` | `FACETED_BREP` | known failing: six open faces, not one solid |
//! | `LNGUNT` | `LENGTH_UNIT` | known failing: the inch is lost (1000 mm³, not 1000 in³) |
//! | `GUAC` | `GLOBAL_UNIT_ASSIGNED_CONTEXT` | known failing: the inch is lost |
//! | `SUNT` | `SI_UNIT` | known failing: the body is skipped (unsupported unit) |
//! | `CNBSUN` | `CONVERSION_BASED_UNIT` | known failing: the body is skipped |
//! | `CMCRSG` | `COMPOSITE_CURVE_SEGMENT` | known failing: read as `COMPOSITE_CURVE` |
//! | `CMPCRV` | `COMPOSITE_CURVE` | known failing: not read |
//! | `FBSR` | `FACETED_BREP_SHAPE_REPRESENTATION` | not observable through the import result |
//! | `NMDUNT` | `NAMED_UNIT` | not observable through the import result |
//! | `LMWU` | `LENGTH_MEASURE_WITH_UNIT` | not observable through the import result |
//! | `GMRPCN` | `GEOMETRIC_REPRESENTATION_CONTEXT` | not observable through the import result |
//! | `RPRCNT` | `REPRESENTATION_CONTEXT` | not observable through the import result |
//!
//! The replacement only matches `=NAME(`, so no name inside a complex instance
//! `(A() B())` is ever read. `CMCRSG` is mapped to `COMPOSITE_CURVE`, but both schemas'
//! tables give `CMCRSG` for `COMPOSITE_CURVE_SEGMENT` and `CMPCRV` for `COMPOSITE_CURVE`,
//! as `socks.stp` uses them (`#7218=CMCRSG(.CONTINUOUS.,.T.,#7205)` has a segment's
//! transition, same sense and parent curve; `#7204=CMPCRV('*CCV1231',(#7218,…),.U.)` is
//! the composite curve holding it).
//!
//! A name is "not observable" where today's reader never looks at that entity's name on
//! the path these files take, so the short name and the long one import the same and the
//! test cannot tell whether it is read: units are told apart by their `LENGTH_UNIT`,
//! `SI_UNIT` and `CONVERSION_BASED_UNIT` parts (never `NAMED_UNIT`), a conversion factor's
//! simple record is read by position whatever its name (`LMWU`), a context's units are
//! found from its `GLOBAL_UNIT_ASSIGNED_CONTEXT` part only (`GMRPCN`, `RPRCNT`), and a
//! body without a recognised representation (`FBSR` does not end in `REPRESENTATION`)
//! takes the file's one unit context as its fallback. Those cases pin that the import
//! does not change, nothing more.
//!
//! **Dangling references.** `step_import::validate` reports every reference to an
//! instance the file does not have, over the whole file, mapped or not.
//!
//! **File PCURVEs.** `step_brep`'s checks on the file's pcurves
//! ([`step_brep::BuildOptions::file_pcurves`], `crates/step-brep/src/pcurves.rs`), and
//! where each has its case:
//!
//! - within tolerance of the edge ("edge off its pcurve"):
//!   `a_pcurve_off_its_edge_leaves_the_face_to_projection` in `crates/step-brep/src/lib.rs`;
//! - chained around the loop ("loop does not close in UV"):
//!   `chain_shifts_seam_uses_by_whole_periods` in `crates/step-brep/src/pcurves.rs`;
//! - along the edge ("pcurve not monotonic along the edge", "pcurve ends off the edge's
//!   ends", "pcurve off the edge's 3D curve"): here, one synthetic case each, on a 10 mm
//!   square whose pcurves the file gives;
//! - inside the face's domain ("pcurve outside the face's UV domain"): here, the case
//!   that shows it cannot refuse on a plane (the kernel widens a plane face's UV domain
//!   by 0.5); a refusing case would need a periodic surface, and none is pinned;
//! - on the corpus: `file_pcurves_corpus.rs`.
//!
//! The corpus test is ignored by default; the corpus is not in the repository. Fetch it
//! with `scripts/fetch-steptools-corpus.sh` and run it with its directory in
//! `SOLIDCORE_STEP_CORPUS` (searched recursively):
//!
//! ```sh
//! SOLIDCORE_STEP_CORPUS=corpus cargo test --release -p step-brep \
//!     --test issue_2582_short_names_validate_pcurves -- --ignored --nocapture
//! ```

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::topology::Brep;
use step_brep::{
    import_with_report, import_with_report_options, is_closed_solid, BuildMode, BuildOptions,
};
use step_import::part21::{Instance, Param, Record, StepFile};
use step_import::Limits;

/// mm per inch.
const INCH: f64 = 25.4;

/// A 10 in cube as a `FACETED_BREP` of six `FACE_SURFACE`s on planes, each bounded by a
/// four-point `POLY_LOOP` counter-clockwise seen from outside, in one `CLOSED_SHELL`, in
/// an inch context. Volume 1000 in³.
const CUBE: &str = "#101=CARTESIAN_POINT('',(0.0,0.0,0.0));\
#102=CARTESIAN_POINT('',(10.0,0.0,0.0));\
#103=CARTESIAN_POINT('',(10.0,10.0,0.0));\
#104=CARTESIAN_POINT('',(0.0,10.0,0.0));\
#105=CARTESIAN_POINT('',(0.0,0.0,10.0));\
#106=CARTESIAN_POINT('',(10.0,0.0,10.0));\
#107=CARTESIAN_POINT('',(10.0,10.0,10.0));\
#108=CARTESIAN_POINT('',(0.0,10.0,10.0));\
#111=DIRECTION('',(1.0,0.0,0.0));#112=DIRECTION('',(-1.0,0.0,0.0));\
#113=DIRECTION('',(0.0,1.0,0.0));#114=DIRECTION('',(0.0,-1.0,0.0));\
#115=DIRECTION('',(0.0,0.0,1.0));#116=DIRECTION('',(0.0,0.0,-1.0));\
#121=AXIS2_PLACEMENT_3D('',#101,#116,#111);#122=PLANE('',#121);\
#123=AXIS2_PLACEMENT_3D('',#105,#115,#111);#124=PLANE('',#123);\
#125=AXIS2_PLACEMENT_3D('',#101,#114,#111);#126=PLANE('',#125);\
#127=AXIS2_PLACEMENT_3D('',#102,#111,#113);#128=PLANE('',#127);\
#129=AXIS2_PLACEMENT_3D('',#104,#113,#111);#130=PLANE('',#129);\
#131=AXIS2_PLACEMENT_3D('',#101,#112,#113);#132=PLANE('',#131);\
#200=FACE_SURFACE('',(#201),#122,.T.);#201=FACE_OUTER_BOUND('',#202,.T.);\
#202=POLY_LOOP('',(#101,#104,#103,#102));\
#210=FACE_SURFACE('',(#211),#124,.T.);#211=FACE_OUTER_BOUND('',#212,.T.);\
#212=POLY_LOOP('',(#105,#106,#107,#108));\
#220=FACE_SURFACE('',(#221),#126,.T.);#221=FACE_OUTER_BOUND('',#222,.T.);\
#222=POLY_LOOP('',(#101,#102,#106,#105));\
#230=FACE_SURFACE('',(#231),#128,.T.);#231=FACE_OUTER_BOUND('',#232,.T.);\
#232=POLY_LOOP('',(#102,#103,#107,#106));\
#240=FACE_SURFACE('',(#241),#130,.T.);#241=FACE_OUTER_BOUND('',#242,.T.);\
#242=POLY_LOOP('',(#103,#104,#108,#107));\
#250=FACE_SURFACE('',(#251),#132,.T.);#251=FACE_OUTER_BOUND('',#252,.T.);\
#252=POLY_LOOP('',(#104,#101,#105,#108));\
#300=CLOSED_SHELL('',(#200,#210,#220,#230,#240,#250));\
#301=FACETED_BREP('cube',#300);\
#310=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\
#311=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#310);\
#312=(CONVERSION_BASED_UNIT('INCH',#311)LENGTH_UNIT()NAMED_UNIT(*));\
#313=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#312))\
REPRESENTATION_CONTEXT('',''));\
#314=FACETED_BREP_SHAPE_REPRESENTATION('',(#301),#313);";

/// A 10 in square sheet in z = 0: one `ADVANCED_FACE` on `{surface}` bounded by four
/// `EDGE_CURVE`s, the first on `{curve}` (`#401` → `#402`), the others on `LINE`s, in a
/// `SHELL_BASED_SURFACE_MODEL`, in an inch context. Area 100 in².
const SQUARE: &str = "#401=CARTESIAN_POINT('',(0.0,0.0,0.0));\
#402=CARTESIAN_POINT('',(10.0,0.0,0.0));\
#403=CARTESIAN_POINT('',(10.0,10.0,0.0));\
#404=CARTESIAN_POINT('',(0.0,10.0,0.0));\
#411=VERTEX_POINT('',#401);#412=VERTEX_POINT('',#402);\
#413=VERTEX_POINT('',#403);#414=VERTEX_POINT('',#404);\
#421=DIRECTION('',(1.0,0.0,0.0));#422=DIRECTION('',(0.0,1.0,0.0));\
#423=DIRECTION('',(-1.0,0.0,0.0));#424=DIRECTION('',(0.0,-1.0,0.0));\
#425=DIRECTION('',(0.0,0.0,1.0));\
#431=LINE('',#401,#441);#441=VECTOR('',#421,1.0);\
#432=LINE('',#402,#442);#442=VECTOR('',#422,1.0);\
#433=LINE('',#403,#443);#443=VECTOR('',#423,1.0);\
#434=LINE('',#404,#444);#444=VECTOR('',#424,1.0);\
#450={curve};\
#451=EDGE_CURVE('',#411,#412,#450,.T.);#452=EDGE_CURVE('',#412,#413,#432,.T.);\
#453=EDGE_CURVE('',#413,#414,#433,.T.);#454=EDGE_CURVE('',#414,#411,#434,.T.);\
#461=ORIENTED_EDGE('',*,*,#451,.T.);#462=ORIENTED_EDGE('',*,*,#452,.T.);\
#463=ORIENTED_EDGE('',*,*,#453,.T.);#464=ORIENTED_EDGE('',*,*,#454,.T.);\
#470=EDGE_LOOP('',(#461,#462,#463,#464));#471=FACE_OUTER_BOUND('',#470,.T.);\
#480=AXIS2_PLACEMENT_3D('',#401,#425,#421);#481={surface};\
#490=ADVANCED_FACE('',(#471),#481,.T.);\
#491=OPEN_SHELL('',(#490));#492=SHELL_BASED_SURFACE_MODEL('',(#491));\
#310=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\
#311=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#310);\
#312=(CONVERSION_BASED_UNIT('INCH',#311)LENGTH_UNIT()NAMED_UNIT(*));\
#313=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#312))\
REPRESENTATION_CONTEXT('',''));\
#314=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#492),#313);";

/// The first edge's curve: a line, as the other three.
const LINE_CURVE: &str = "LINE('',#401,#441)";
/// The first edge's curve: a degree-1 B-spline from `#401` to `#402`.
const BSPLINE_CURVE: &str = "B_SPLINE_CURVE_WITH_KNOTS('',1,(#401,#402),.POLYLINE_FORM.,.F.,.F.,\
(2,2),(0.0,10.0),.UNSPECIFIED.)";
/// The first edge's curve: the line `#431` trimmed to 0…10.
const TRIMMED_CURVE: &str =
    "TRIMMED_CURVE('',#431,(PARAMETER_VALUE(0.0)),(PARAMETER_VALUE(10.0)),.T.,.PARAMETER.)";
/// The first edge's "curve": a composite curve's segment on the line `#431`. A segment is
/// not a curve, so the body is refused; what matters is that it is refused as a segment,
/// which shows which entity the reader took the name for.
const SEGMENT_CURVE: &str = "COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#431)";
/// The first edge's curve: a composite curve of one segment on the line `#431` (the
/// segment `#455` is appended to the file). Composite curves are not mapped today, so
/// the body is refused as `unsupported: COMPOSITE_CURVE`.
const COMPOSITE_CURVE: &str = "COMPOSITE_CURVE('',(#455),.F.)";
/// The face's surface: the plane z = 0.
const PLANE_SURFACE: &str = "PLANE('',#480)";
/// The face's surface: a bilinear B-spline patch over the square.
const BSPLINE_SURFACE: &str = "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#401,#404),(#402,#403)),\
.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.0,1.0),(0.0,1.0),.UNSPECIFIED.)";

/// [`SQUARE`] with its first edge on `curve` and its face on `surface`.
fn square(curve: &str, surface: &str) -> String {
    SQUARE
        .replace("{curve}", curve)
        .replace("{surface}", surface)
}

/// `data` as a Part 21 file.
fn part21(data: &str) -> Vec<u8> {
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;\
         DATA;{data}ENDSEC;END-ISO-10303-21;"
    )
    .into_bytes()
}

/// `data` with every instance of the entity `long` written as `short`: the name where it
/// starts a record (after `=`, `(` or `)`, and before `(`), never inside a longer name.
fn rename(data: &str, long: &str, short: &str) -> String {
    let mut out = String::with_capacity(data.len());
    let mut rest = data;
    while let Some(at) = rest.find(long) {
        let before = rest[..at].chars().last();
        let after = rest[at + long.len()..].chars().next();
        out.push_str(&rest[..at]);
        if matches!(before, Some('=' | '(' | ')')) && after == Some('(') {
            out.push_str(short);
        } else {
            out.push_str(long);
        }
        rest = &rest[at + long.len()..];
    }
    out.push_str(rest);
    out
}

/// The volume of a closed `brep`, or the area of an open one.
fn measure(brep: &Brep) -> f64 {
    let kernel = Kernel::default();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = brep.clone();
    if is_closed_solid(brep) {
        brep_volume(&kernel, &mut copy, 1e-9, &attrs).expect("volume")
    } else {
        brep_area(&kernel, &mut copy, 1e-9).expect("area")
    }
}

/// What one import gave: (bodies, closed bodies, faces, volume of the closed bodies or
/// area of the open ones in mm³ / mm², skipped bodies' reasons).
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    bodies: usize,
    closed: usize,
    faces: usize,
    measure: f64,
    skipped: Vec<String>,
}

fn outcome(data: &str) -> Outcome {
    let report = import_with_report(&part21(data), &Limits::default()).expect("imports");
    Outcome {
        bodies: report.bodies.len(),
        closed: report.bodies.iter().filter(|b| is_closed_solid(b)).count(),
        faces: report.bodies.iter().map(|b| b.face_list().len()).sum(),
        measure: report.bodies.iter().map(measure).sum(),
        skipped: report.skipped.iter().map(|s| s.reason.clone()).collect(),
    }
}

/// Whether two outcomes are the same import (measures within 1e-9 relative).
fn same(a: &Outcome, b: &Outcome) -> bool {
    a.bodies == b.bodies
        && a.closed == b.closed
        && a.faces == b.faces
        && a.skipped == b.skipped
        && (a.measure - b.measure).abs() <= 1e-9 * a.measure.abs().max(b.measure.abs())
}

/// A short-name case: (short name, entity, the long-name file).
type ShortName = (&'static str, &'static str, fn() -> String);

fn cube() -> String {
    CUBE.to_string()
}
fn line_square() -> String {
    square(LINE_CURVE, PLANE_SURFACE)
}
fn bspline_curve_square() -> String {
    square(BSPLINE_CURVE, PLANE_SURFACE)
}
fn trimmed_square() -> String {
    square(TRIMMED_CURVE, PLANE_SURFACE)
}
fn bspline_surface_square() -> String {
    square(LINE_CURVE, BSPLINE_SURFACE)
}
fn segment_square() -> String {
    square(SEGMENT_CURVE, PLANE_SURFACE)
}
fn composite_square() -> String {
    square(COMPOSITE_CURVE, PLANE_SURFACE) + "#455=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#431);"
}

/// The short names read today: the file with one of them imports as with the long name.
const READ: [ShortName; 10] = [
    ("FCSRF", "FACE_SURFACE", cube),
    ("FCOTBN", "FACE_OUTER_BOUND", cube),
    ("PLYLP", "POLY_LOOP", cube),
    ("CRTPNT", "CARTESIAN_POINT", cube),
    ("DRCTN", "DIRECTION", cube),
    ("A2PL3D", "AXIS2_PLACEMENT_3D", cube),
    ("CLSSHL", "CLOSED_SHELL", cube),
    (
        "BSSWK",
        "B_SPLINE_SURFACE_WITH_KNOTS",
        bspline_surface_square,
    ),
    ("BSCWK", "B_SPLINE_CURVE_WITH_KNOTS", bspline_curve_square),
    ("TRMCRV", "TRIMMED_CURVE", trimmed_square),
];

/// The short names not read, or read as another entity, today (known failing).
const KNOWN_FAILING: [ShortName; 7] = [
    ("FCTBR", "FACETED_BREP", cube),
    ("LNGUNT", "LENGTH_UNIT", cube),
    ("GUAC", "GLOBAL_UNIT_ASSIGNED_CONTEXT", cube),
    ("SUNT", "SI_UNIT", cube),
    ("CNBSUN", "CONVERSION_BASED_UNIT", cube),
    ("CMCRSG", "COMPOSITE_CURVE_SEGMENT", segment_square),
    ("CMPCRV", "COMPOSITE_CURVE", composite_square),
];

/// The short names whose reading these files cannot show (see the module docs).
const NOT_OBSERVABLE: [ShortName; 5] = [
    ("FBSR", "FACETED_BREP_SHAPE_REPRESENTATION", cube),
    ("NMDUNT", "NAMED_UNIT", cube),
    ("LMWU", "LENGTH_MEASURE_WITH_UNIT", cube),
    ("GMRPCN", "GEOMETRIC_REPRESENTATION_CONTEXT", cube),
    ("RPRCNT", "REPRESENTATION_CONTEXT", line_square),
];

/// Each case's (short name, long-name outcome, short-name outcome).
fn renamed_outcomes(cases: &[ShortName]) -> Vec<(String, Outcome, Outcome)> {
    cases
        .iter()
        .map(|(short, long, file)| {
            let file = file();
            let renamed = rename(&file, long, short);
            assert_ne!(renamed, file, "{short}: {long} is in its case's file");
            let (a, b) = (outcome(&file), outcome(&renamed));
            println!("{short} ({long}): long {a:?}, short {b:?}");
            (format!("{short} ({long})"), a, b)
        })
        .collect()
}

/// The long-name files give their closed forms: the cube one closed solid of six faces
/// and 1000 in³, each square one open face of 100 in², and the segment and composite
/// squares no body (a segment is not a curve; composite curves are not mapped today).
#[test]
fn long_name_files_give_their_closed_forms() {
    let cube = outcome(CUBE);
    assert_eq!(
        (cube.bodies, cube.closed, cube.faces),
        (1, 1, 6),
        "{cube:?}"
    );
    let volume = 1000.0 * INCH * INCH * INCH;
    assert!(
        (cube.measure - volume).abs() <= 1e-9 * volume,
        "{cube:?}, volume {volume}"
    );
    let area = 100.0 * INCH * INCH;
    for (name, file) in [
        ("line", line_square()),
        ("B-spline curve", bspline_curve_square()),
        ("trimmed curve", trimmed_square()),
        ("B-spline surface", bspline_surface_square()),
    ] {
        let sheet = outcome(&file);
        assert_eq!(
            (sheet.bodies, sheet.closed, sheet.faces),
            (1, 0, 1),
            "{name}: {sheet:?}"
        );
        assert!(
            (sheet.measure - area).abs() <= 1e-9 * area,
            "{name}: {sheet:?}, area {area}"
        );
    }
    for (file, reason) in [
        (segment_square(), "unsupported: COMPOSITE_CURVE_SEGMENT"),
        (composite_square(), "unsupported: COMPOSITE_CURVE"),
    ] {
        let refused = outcome(&file);
        assert_eq!(refused.bodies, 0, "{refused:?}");
        assert_eq!(refused.skipped, [reason], "{refused:?}");
    }
}

/// Each short name read today: one entity written short imports as written long.
#[test]
fn short_names_read_today() {
    let failures: Vec<String> = renamed_outcomes(&READ)
        .into_iter()
        .filter(|(_, a, b)| !same(a, b))
        .map(|(name, a, b)| format!("{name}: long {a:?}, short {b:?}"))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Each short name not read today, or read as another entity: the file with it imports
/// differently from the long-name file, as measured today:
///
/// - `FCTBR`: six open single-face bodies (6 × 100 in²), not one closed solid;
/// - `LNGUNT`, `GUAC`: one closed solid of 1000 mm³, not 1000 in³ (the inch is lost);
/// - `SUNT`, `CNBSUN`: the solid is skipped as an unsupported unit, and its six faces
///   arrive as loose sheets in mm;
/// - `CMCRSG`: refused as `unsupported: COMPOSITE_CURVE`, the entity it is mapped to,
///   not as the segment it is;
/// - `CMPCRV`: refused as `unsupported: CMPCRV`, not as the composite curve it is.
///
/// Fails for a name once it imports as its long name: move it to [`READ`] then.
#[test]
fn short_names_not_read_known_failing() {
    let read: Vec<String> = renamed_outcomes(&KNOWN_FAILING)
        .into_iter()
        .filter(|(_, a, b)| same(a, b))
        .map(|(name, _, _)| name)
        .collect();
    assert!(
        read.is_empty(),
        "now read as their entities; move them to READ: {}",
        read.join(", ")
    );
}

/// The short names today's reader never consults on these files' path (see the module
/// docs): they import the same as the long names, which pins that the import does not
/// change and shows nothing about whether the name is read. Fails when one of them
/// starts to change the import: it is observable then, and belongs in [`READ`] or
/// [`KNOWN_FAILING`].
#[test]
fn short_names_not_observable_through_the_import_result() {
    let changed: Vec<String> = renamed_outcomes(&NOT_OBSERVABLE)
        .into_iter()
        .filter(|(_, a, b)| !same(a, b))
        .map(|(name, a, b)| format!("{name}: long {a:?}, short {b:?}"))
        .collect();
    assert!(
        changed.is_empty(),
        "now change the import; classify them:\n{}",
        changed.join("\n")
    );
}

/// `rename` touches entity names only, never a longer name holding the short one's
/// entity.
#[test]
fn rename_changes_whole_entity_names_only() {
    let text =
        "#1=(LENGTH_UNIT()NAMED_UNIT(*));#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#1);\
#3=PLANE('',#4);#5=(NAMED_UNIT(*)PLANE_ANGLE_UNIT());#6=COMPOSITE_CURVE_SEGMENT(.C.,.T.,#7);";
    assert_eq!(
        rename(text, "LENGTH_UNIT", "LNGUNT"),
        "#1=(LNGUNT()NAMED_UNIT(*));#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#1);\
#3=PLANE('',#4);#5=(NAMED_UNIT(*)PLANE_ANGLE_UNIT());#6=COMPOSITE_CURVE_SEGMENT(.C.,.T.,#7);"
    );
    // `PLN` is an arbitrary token here, not a short name (PLANE's short name is PLANE).
    assert_eq!(
        rename(text, "PLANE", "PLN"),
        text.replace("#3=PLANE(", "#3=PLN(")
    );
    assert_eq!(rename(text, "COMPOSITE_CURVE", "CMPCRV"), text);
}

/// The corpus directory, or `None` when `SOLIDCORE_STEP_CORPUS` is unset.
fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("SOLIDCORE_STEP_CORPUS").map(PathBuf::from)
}

/// The first path named `name` under `dir`, depth first in sorted order.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    paths.into_iter().find_map(|path| {
        if path.is_dir() {
            find(&path, name)
        } else {
            (path.file_name().and_then(std::ffi::OsStr::to_str) == Some(name)).then_some(path)
        }
    })
}

/// The file named `name` anywhere under the corpus `dir`, read in place.
fn corpus_file(dir: &Path, name: &str) -> Vec<u8> {
    let path = find(dir, name).unwrap_or_else(|| {
        panic!(
            "{name} is not under {}; fetch the corpus with scripts/fetch-steptools-corpus.sh",
            dir.display()
        )
    });
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The record named any of `names` in `instance`.
fn record_of<'a>(instance: &'a Instance, names: &[&str]) -> Option<&'a Record> {
    names.iter().find_map(|name| instance.record(name))
}

/// The mm per unit of the file's `'INCH'` length unit, from the file itself: the
/// `CONVERSION_BASED_UNIT` (`CNBSUN`) named `'INCH'` that is a `LENGTH_UNIT` (`LNGUNT`),
/// its `LENGTH_MEASURE_WITH_UNIT` (`LMWU`) factor, and that factor's unit, an `SI_UNIT`
/// (`SUNT`) with prefix `.MILLI.` and name `.METRE.` (ISO 10303-41). Read through
/// `step_import`'s Part 21 parser, which stores the names as written.
fn file_inch_mm(file: &StepFile) -> f64 {
    let inches: Vec<f64> = file
        .entities
        .values()
        .filter(|instance| record_of(instance, &["LENGTH_UNIT", "LNGUNT"]).is_some())
        .filter_map(|instance| record_of(instance, &["CONVERSION_BASED_UNIT", "CNBSUN"]))
        .filter(|unit| matches!(unit.params.first(), Some(Param::Str(name)) if name == "INCH"))
        .map(|unit| {
            let factor = unit
                .params
                .get(1)
                .and_then(Param::as_ref_id)
                .and_then(|id| file.entities.get(&id))
                .and_then(|i| record_of(i, &["LENGTH_MEASURE_WITH_UNIT", "LMWU"]))
                .expect("the inch's conversion factor");
            let value = factor
                .params
                .first()
                .and_then(Param::as_f64)
                .expect("a number");
            let si = factor
                .params
                .get(1)
                .and_then(Param::as_ref_id)
                .and_then(|id| file.entities.get(&id))
                .and_then(|i| record_of(i, &["SI_UNIT", "SUNT"]))
                .expect("the factor's SI unit");
            let enums: Vec<Option<&str>> = si.params.iter().map(Param::as_enum).collect();
            assert_eq!(enums, [Some("MILLI"), Some("METRE")], "{si:?}");
            value
        })
        .collect();
    assert_eq!(inches.len(), 1, "one inch unit: {inches:?}");
    inches[0]
}

/// `clip` and `socks` are in inches, written in short names inside complex instances
/// (`#28=(LNGUNT()NMDUNT(*)SUNT(.MILLI.,.METRE.))`, `#29=LMWU(LENGTH_MEASURE(25.4),#28)`,
/// `#30=(CNBSUN('INCH',#29)LNGUNT()NMDUNT(#27))`, `#48=(GMRPCN(3)GC((#49))
/// GUAC((#30,#33,#34))RPRCNT('*MASTER','WORKSPACE'))`), so every body of theirs is at
/// 25.4 mm per file unit. Today's text replacement reaches none of those names, the
/// context's units are not found, and every body is read at 1 mm per file unit: 25.4
/// times too small in length.
///
/// Only the scale is asserted: the files carry no validation property and their faceted
/// geometry has no closed form. That their one `FCTBR` solid arrives as loose faces is
/// #2581's known-failing case (`short_name_faceted_brep_is_not_a_body_known_failing` in
/// #2589) and is not repeated here. Fails when the bodies are read in inches: then
/// assert the file's scale instead.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn short_name_inch_unit_is_lost_known_failing() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut changed = Vec::new();
    for name in ["clip.stp", "socks.stp"] {
        let bytes = corpus_file(&dir, name);
        let file = step_import::part21::parse(&bytes, &Limits::default()).expect("parses");
        let inch = file_inch_mm(&file);
        assert!((inch - INCH).abs() <= 1e-12, "{name}: inch {inch} mm");
        let model = step_import::import(&bytes, &Limits::default()).expect("imports");
        assert!(!model.bodies.is_empty(), "{name}: no bodies");
        let mut scales: BTreeMap<String, usize> = BTreeMap::new();
        for body in &model.bodies {
            *scales.entry(format!("{}", body.unit_mm)).or_default() += 1;
        }
        println!(
            "{name}: file unit {inch} mm; {} bodies, mm per unit {scales:?}",
            model.bodies.len()
        );
        if model.bodies.iter().any(|body| body.unit_mm != 1.0) {
            changed.push(format!(
                "{name}: mm per unit {scales:?}, known failing as 1 (the file's is {inch})"
            ));
        }
    }
    assert!(
        changed.is_empty(),
        "the inch is read now; assert the file's scale instead:\n{}",
        changed.join("\n")
    );
}

/// `step_import::validate` reports each reference to a missing instance, over the whole
/// file: one from a mapped body's face (`#211` → `#999`), one from an unmapped entity
/// (`#500` → `#998`), each once, in file order; the complete file has none.
#[test]
fn validate_reports_every_dangling_reference() {
    let complete = part21(CUBE);
    let file = step_import::part21::parse(&complete, &Limits::default()).expect("parses");
    let model = step_import::map::map_file(&file, &Limits::default());
    assert_eq!(step_import::validate::validate(&file, &model), []);

    let dangling = CUBE.replace(
        "FACE_OUTER_BOUND('',#212,.T.)",
        "FACE_OUTER_BOUND('',#999,.T.)",
    ) + "#500=PRODUCT('p','p','',(#998));";
    let bytes = part21(&dangling);
    let file = step_import::part21::parse(&bytes, &Limits::default()).expect("parses");
    let model = step_import::map::map_file(&file, &Limits::default());
    let errors: Vec<String> = step_import::validate::validate(&file, &model)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        errors,
        [
            "#211: dangling reference #999",
            "#500: dangling reference #998"
        ],
        "{errors:?}"
    );
}

/// A 10 mm square sheet in z = 0 on the plane `#81` (u = x, v = y, in mm), its four
/// `LINE` edges each a `SURFACE_CURVE` with one `PCURVE` on that plane. The pcurves of
/// edges 2–4 are their exact 2D lines; edge 1's (`#451`, (0, 0) → (10, 0)) is
/// `{pcurve}` with any extra 2D points in `{points}`. Area 100.
const PCURVE_SQUARE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));\
#2=CARTESIAN_POINT('',(10.,0.,0.));#3=CARTESIAN_POINT('',(10.,10.,0.));\
#4=CARTESIAN_POINT('',(0.,10.,0.));\
#11=VERTEX_POINT('',#1);#12=VERTEX_POINT('',#2);\
#13=VERTEX_POINT('',#3);#14=VERTEX_POINT('',#4);\
#21=DIRECTION('',(1.,0.,0.));#22=DIRECTION('',(0.,1.,0.));\
#23=DIRECTION('',(-1.,0.,0.));#24=DIRECTION('',(0.,-1.,0.));#25=DIRECTION('',(0.,0.,1.));\
#31=LINE('',#1,#41);#41=VECTOR('',#21,1.);#32=LINE('',#2,#42);#42=VECTOR('',#22,1.);\
#33=LINE('',#3,#43);#43=VECTOR('',#23,1.);#34=LINE('',#4,#44);#44=VECTOR('',#24,1.);\
#80=AXIS2_PLACEMENT_3D('',#1,#25,#21);#81=PLANE('',#80);\
#99=PARAMETRIC_REPRESENTATION_CONTEXT('','');\
#111=CARTESIAN_POINT('',(0.,0.));#112=CARTESIAN_POINT('',(10.,0.));\
#113=CARTESIAN_POINT('',(10.,10.));#114=CARTESIAN_POINT('',(0.,10.));\
#121=DIRECTION('',(1.,0.));#122=DIRECTION('',(0.,1.));\
#123=DIRECTION('',(-1.,0.));#124=DIRECTION('',(0.,-1.));\
#131=VECTOR('',#121,1.);#132=VECTOR('',#122,1.);#133=VECTOR('',#123,1.);#134=VECTOR('',#124,1.);\
#140={pcurve};{points}\
#142=LINE('',#112,#132);#143=LINE('',#113,#133);#144=LINE('',#114,#134);\
#151=DEFINITIONAL_REPRESENTATION('',(#140),#99);#152=DEFINITIONAL_REPRESENTATION('',(#142),#99);\
#153=DEFINITIONAL_REPRESENTATION('',(#143),#99);#154=DEFINITIONAL_REPRESENTATION('',(#144),#99);\
#161=PCURVE('',#81,#151);#162=PCURVE('',#81,#152);#163=PCURVE('',#81,#153);#164=PCURVE('',#81,#154);\
#171=SURFACE_CURVE('',#31,(#161),.CURVE_3D.);#172=SURFACE_CURVE('',#32,(#162),.CURVE_3D.);\
#173=SURFACE_CURVE('',#33,(#163),.CURVE_3D.);#174=SURFACE_CURVE('',#34,(#164),.CURVE_3D.);\
#451=EDGE_CURVE('',#11,#12,#171,.T.);#452=EDGE_CURVE('',#12,#13,#172,.T.);\
#453=EDGE_CURVE('',#13,#14,#173,.T.);#454=EDGE_CURVE('',#14,#11,#174,.T.);\
#461=ORIENTED_EDGE('',*,*,#451,.T.);#462=ORIENTED_EDGE('',*,*,#452,.T.);\
#463=ORIENTED_EDGE('',*,*,#453,.T.);#464=ORIENTED_EDGE('',*,*,#454,.T.);\
#470=EDGE_LOOP('',(#461,#462,#463,#464));#471=FACE_OUTER_BOUND('',#470,.T.);\
#490=ADVANCED_FACE('',(#471),#81,.T.);\
#491=OPEN_SHELL('',(#490));#492=SHELL_BASED_SURFACE_MODEL('',(#491));\
#310=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\
#313=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#310))\
REPRESENTATION_CONTEXT('',''));\
#314=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#492),#313);";

/// Edge 1's pcurve as a degree-1 2D B-spline through `points` (in mm), one knot per
/// point (0, 1, 2, …), the ends doubled. The points are `#181`, `#182`, ….
fn polyline_pcurve(points: &[[f64; 2]]) -> (String, String) {
    let ids: Vec<String> = (0..points.len()).map(|i| format!("#{}", 181 + i)).collect();
    let defs = points
        .iter()
        .zip(&ids)
        .fold(String::new(), |mut defs, ([x, y], id)| {
            // Writing to a `String` cannot fail.
            let _ = write!(defs, "{id}=CARTESIAN_POINT('',({x:?},{y:?}));");
            defs
        });
    let last = points.len() - 1;
    let mults: Vec<&str> = (0..points.len())
        .map(|i| if i == 0 || i == last { "2" } else { "1" })
        .collect();
    let knots: Vec<String> = (0..points.len()).map(|i| format!("{i}.")).collect();
    let curve = format!(
        "B_SPLINE_CURVE_WITH_KNOTS('',1,({}),.POLYLINE_FORM.,.F.,.F.,({}),({}),.UNSPECIFIED.)",
        ids.join(","),
        mults.join(","),
        knots.join(",")
    );
    (curve, defs)
}

/// [`PCURVE_SQUARE`] with edge 1's pcurve `curve` and its extra points `points`,
/// imported with the file's pcurves on the stitched build (the only one that uses them):
/// (bodies, area, pcurve stats).
fn pcurve_import(curve: &str, points: &str) -> (usize, f64, step_brep::PcurveStats) {
    let data = PCURVE_SQUARE
        .replace("{pcurve}", curve)
        .replace("{points}", points);
    let options = BuildOptions {
        file_pcurves: true,
        mode: BuildMode::StitchOnly,
        ..BuildOptions::default()
    };
    let report =
        import_with_report_options(&part21(&data), &Limits::default(), options).expect("imports");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let area = report.bodies.iter().map(measure).sum();
    (report.bodies.len(), area, report.pcurves)
}

/// With every pcurve exact (edge 1's the 2D line u = t, v = 0), the face takes all four
/// as its UV trims, none falls back, and the sheet's area is 100.
#[test]
fn exact_file_pcurves_become_the_trims() {
    let (bodies, area, stats) = pcurve_import("LINE('',#111,#131)", "");
    assert_eq!(bodies, 1);
    assert!((area - 100.0).abs() <= 1e-9 * 100.0, "area {area}");
    assert_eq!(
        (
            stats.faces,
            stats.uses,
            stats.uses_projected,
            stats.seam_uses
        ),
        (1, 4, 0, 0),
        "{stats:?}"
    );
    assert!(stats.fallbacks.is_empty(), "{stats:?}");
}

/// Edge 1's pcurve fails one check: the face is left to the kernel's projection for that
/// reason alone, and the sheet still builds with area 100.
fn assert_left_to_projection(curve: &str, points: &str, reason: &str) {
    let (bodies, area, stats) = pcurve_import(curve, points);
    assert_eq!(bodies, 1, "{reason}");
    assert!(
        (area - 100.0).abs() <= 1e-9 * 100.0,
        "{reason}: area {area}"
    );
    assert_eq!(stats.faces, 0, "{reason}: {stats:?}");
    assert_eq!(
        stats.fallbacks,
        BTreeMap::from([(reason, 1)]),
        "{reason}: {stats:?}"
    );
}

/// Along the edge, monotonic: edge 1's pcurve visits the edge's points out of order. The
/// polyline (0, 0) (2.5, 0) (2.5, 5) (7.5, 5) (7.5, 0) (5, 0) (5, −5) (10, −5) (10, 0)
/// holds every point of the edge's x axis segment (so the edge is on it), but its
/// parameters at x = 0, 2.5, 5, 7.5, 10 are 0, 1, 5, 4, 8: not monotonic.
#[test]
fn a_pcurve_out_of_order_along_its_edge_is_refused() {
    let (curve, points) = polyline_pcurve(&[
        [0.0, 0.0],
        [2.5, 0.0],
        [2.5, 5.0],
        [7.5, 5.0],
        [7.5, 0.0],
        [5.0, 0.0],
        [5.0, -5.0],
        [10.0, -5.0],
        [10.0, 0.0],
    ]);
    assert_left_to_projection(&curve, &points, "pcurve not monotonic along the edge");
}

/// Along the edge, its ends: edge 1's pcurve (2.5, 0) (10, 0) (10, 3) (0, 3) (0, 0) holds
/// the edge but starts at x = 2.5, so its start is 2.5 mm from the edge's start vertex
/// (0, 0), which it reaches only at its far end.
#[test]
fn a_pcurve_ending_off_its_edge_ends_is_refused() {
    let (curve, points) =
        polyline_pcurve(&[[2.5, 0.0], [10.0, 0.0], [10.0, 3.0], [0.0, 3.0], [0.0, 0.0]]);
    assert_left_to_projection(&curve, &points, "pcurve ends off the edge's ends");
}

/// Along the edge, on its 3D curve: edge 1's pcurve zigzags through (0, 0) (1.25, 0.5)
/// (2.5, 0) … (10, 0), on the edge at x = 0, 2.5, 5, 7.5, 10 and 0.5 mm off it between.
#[test]
fn a_pcurve_off_its_edge_between_its_points_is_refused() {
    let zigzag: Vec<[f64; 2]> = (0..=8u32)
        .map(|i| {
            let y = if i % 2 == 1 { 0.5 } else { 0.0 };
            [1.25 * f64::from(i), y]
        })
        .collect();
    let (curve, points) = polyline_pcurve(&zigzag);
    assert_left_to_projection(&curve, &points, "pcurve off the edge's 3D curve");
}

/// Inside the face's domain, on a plane: edge 1's pcurve is the edge's line moved to
/// v = −5e-6 mm, within the 1e-5 mm zone tolerance of the edge everywhere and outside
/// the loop's UV box v ≥ 0, and it is taken. The domain check ("pcurve outside the face's
/// UV domain") cannot refuse a pcurve that passes the edge checks on a plane: the kernel
/// gives a plane face the loop's UV box widened by 0.5 (measured: (−0.500001, −0.500001)
/// to (10.500001, 10.500001) for this square), far more than the tolerance by which such
/// a pcurve can leave the loop. It can only refuse a chained piece on a periodic surface;
/// no small synthetic case reaching it was found (see the PR).
#[test]
fn a_pcurve_just_outside_the_loop_is_inside_a_plane_face_domain() {
    let (bodies, area, stats) = pcurve_import(
        "LINE('',#141,#131)",
        "#141=CARTESIAN_POINT('',(0.,-5.E-06));",
    );
    assert_eq!(bodies, 1);
    assert!((area - 100.0).abs() <= 1e-6 * 100.0, "area {area}");
    assert_eq!((stats.faces, stats.uses), (1, 4), "{stats:?}");
    assert!(stats.fallbacks.is_empty(), "{stats:?}");
}
