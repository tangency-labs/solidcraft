//! The cases of #2580, pinned against today's reader so that the port into the reader of
//! #2578 has an acceptance test: faces that wind round a torus or sphere, multi-winding
//! loops, loopless faces and edges across a closed curve, in nine corpus files.
//!
//! Expected values are the file's own, never the reader's output:
//!
//! - **Volumes** where the file carries its exporter's geometric validation volume
//!   (read through `step_import`'s parser and converted from the file's unit, see
//!   [`validation`]), where another export of the same part does ([`Volume::Sister`]),
//!   or where the geometry has a closed form (`ftc_11_rb`, below).
//! - **Counts and closedness** everywhere, from the file's census: one body per
//!   `MANIFOLD_SOLID_BREP` / `BREP_WITH_VOIDS` / `FACETED_BREP` /
//!   `SHELL_BASED_SURFACE_MODEL`, and the faces of the shells each body holds
//!   ([`validation::body_census`]). Every solid is expected closed, a surface model open.
//!
//! | file (corpus path) | bodies | faces | closed | volume expected (mm³) | bound | today |
//! |---|---|---|---|---|---|---|
//! | `nist_ftc_06_asme1_rd` (AP203 geometry only) | 1 | 144 | 1 | 3 323 556.50526143, `ftc_06_ap242-e2`'s | 1e-9 + 1.5e-15 | 3.761e-6, known failing |
//! | `nist_stc_06_asme1_ap242-e3` | 1 | 144 | 1 | the same (its own is `VOLUME_MEASURE(0.0)`) | 1e-9 + 1.5e-15 | 4.810e-6, known failing |
//! | `nist_ftc_11_asme1_rb` (AP203 geometry only) | 1 | 6 | 1 | 5129.386136 (closed form) | 1e-9 | 0.3801, known failing |
//! | `nist_ftc_11_asme1_ap242-e2` | 1 | 42 | 1 | none: another part, see below | | |
//! | `turbine` (STEP Tools AP203) | 1 | 1241 | 1 | none, see below | | |
//! | `nist_ctc_05_asme1_rd` (AP203 geometry only) | 2 | 209 | 1 | `7.746912251592E2` in³ = 12 694 914.687 | 1e-9 + 6.5e-14 | 1.095e-6, known failing |
//! | `nist_ctc_05_asme1_ap203` (AP203 with PMI) | 1 | 156 | 1 | `774.691593579675` in³ = 12 694 920.724 | 1e-9 + 6.5e-16 | 1.208e-7, known failing |
//! | `nist_ctc_05_asme1_ap242-e1` | 1 | 156 | 1 | 12 694 920.724, `ctc_05_ap203`'s | 1e-9 + 6.5e-16 | 1.545e-7, known failing |
//! | `nist_stc_10_asme1_ap242-e2` | 1 | 256 | 1 | `188312.244641244` | 1e-9 + 2.7e-15 | 3.396e-6, known failing |
//!
//! `ctc_05_rd`'s second body is its `SHELL_BASED_SURFACE_MODEL` of one face, an open
//! sheet. `stc_06`'s validation properties are all zero (volume, area, and a centroid at
//! the origin): an exporter placeholder, not a measurement. The issue lists `ctc_05`
//! twice; the corpus has three exports of it, and all three are pinned.
//!
//! **Bounds.** Each file's bound is the accuracy the test asks the integrator for,
//! [`ACCURACY`] = 1e-9 (what that argument controls is in its doc), plus, against a
//! validation volume, half a unit of the last digit the exporter wrote, relative to the
//! value ([`validation_bound`]): `7.746912251592E2` has 13 significant digits, so
//! 5e-11 / 774.69 = 6.5e-14; `774.691593579675` and `188312.244641244` have 15, and
//! `3323556.50526143` 15 (5e-9 / 3 323 556.5 = 1.5e-15). A closed form is evaluated in
//! f64 to a few ulps, so its bound is the accuracy. No bound is chosen from a
//! measurement. With these bounds every volume pinned here is beyond it today and held
//! known failing with today's deviation ([`State::KnownFailing`]); the integrator is not
//! the cause, as each volume at 1e-9 is within 7e-11 of the volume at 1e-11. The NIST
//! exporter's own two validation volumes of `ctc_05` (`rd` and `ap203`) differ by 4.8e-7.
//!
//! **Sister exports.** A file without a validation volume of its own is checked against
//! another export's where the corpus holds one of the same part ([`Volume::Sister`]), and
//! the test first checks that they are one part ([`validation::same_form`]: the sizes of
//! the vertices' box, the cylinder, sphere and torus radii, and every corner of the
//! export with fewer corners at a vertex of the other, however it is placed, within the
//! larger of the files' uncertainties):
//!
//! - `ftc_06_rd` and `stc_06` against `nist_ftc_06_asme1_ap242-e2`, whose 187 faces are
//!   the same part with its periodic faces split (cylinders 58 → 88 faces' surfaces,
//!   tori 6 → 12, spheres 5 → 8, cones 4 → 8: +43);
//! - `ctc_05_ap242-e1` against `ctc_05_ap203`: the same 156 faces, 331 edges and 249
//!   vertices. `ctc_05_rd` (209 faces) is the same part too, with its own volume.
//!
//! **No volume:** `ftc_11_ap242-e2` is not `ftc_11_rb` ([`ftc_11_exports_are_two_parts`]):
//! the washer with 30 small faces more (cylinders of radius 0.08 to 0.9 mm and tori of
//! tube 0.1 mm), with no validation property, no other export in the corpus and no closed
//! form derived. `turbine` has no validation property and no other export in the corpus;
//! a closed form of its 1241 analytic faces (866 cylinders, 129 cones, 244 planes, 2 tori)
//! is not derived. Both are checked on counts and closedness only.
//!
//! **Known failing today**, besides the deviations above: `ftc_11_rb` is a washer of six
//! faces, two of them tori whose faces run over the top of the tube; today's reader
//! closes it at 3179.68 mm³, 38 % below its closed form.
//!
//! **Follow-up:** the census and validation-volume helper reads the file through
//! `step_import::part21`; when #2591 lands it reads through the record's parser.
//!
//! The corpus tests are ignored by default; the corpus is not in the repository. Fetch it
//! with `scripts/fetch-steptools-corpus.sh` and `scripts/fetch-nist-pmi-corpus.sh` and run
//! them with a directory holding both in `SOLIDCORE_STEP_CORPUS` (searched recursively):
//!
//! ```sh
//! SOLIDCORE_STEP_CORPUS=corpus cargo test --release -p step-brep \
//!     --test issue_2580_winding_faces -- --ignored --nocapture
//! ```

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

#[path = "support/validation.rs"]
mod validation;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::Brep;
use step_brep::{import_with_report, is_closed_solid, ImportReport};
use step_import::Limits;

use validation::{corpus_dir, corpus_file, relative, validation_bound, ACCURACY};

/// What a file's volume is checked against.
#[derive(Debug, Clone, Copy)]
enum Volume {
    /// The file's own geometric validation volume.
    Validation,
    /// The geometric validation volume of another export of the same part (its corpus
    /// name), checked to be the same form first ([`validation::same_form`]).
    Sister(&'static str),
    /// A closed form of the file's geometry, in mm³.
    ClosedForm(fn() -> f64),
    /// Nothing: counts and closedness only, for the reason given.
    None(&'static str),
}

/// What today's reader gives against a file's expected volume.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Within the file's bound.
    Within,
    /// Beyond it: today's relative deviation, as measured.
    KnownFailing(f64),
}

/// One corpus file of #2580.
#[derive(Debug, Clone, Copy)]
struct Case {
    name: &'static str,
    bodies: usize,
    faces: usize,
    closed: usize,
    volume: Volume,
    state: State,
}

const NO_FTC_11_AP242: &str = "another model than ftc_11_rb (42 faces: the washer with 30 \
     small faces, cylinders of radius 0.08 to 0.9 mm and tori of tube 0.1 mm), no validation \
     property, no other export of it in the corpus, no closed form derived";
const NO_TURBINE: &str = "no validation property, no other export of it in the corpus; a \
     closed form of its 1241 analytic faces (866 cylinders, 129 cones, 244 planes, 2 tori) \
     is not derived";

/// The files of #2580 (see the module docs).
const CASES: [Case; 9] = [
    Case {
        name: "nist_ftc_06_asme1_rd.stp",
        bodies: 1,
        faces: 144,
        closed: 1,
        volume: Volume::Sister("nist_ftc_06_asme1_ap242-e2.stp"),
        state: State::KnownFailing(3.761e-6),
    },
    Case {
        name: "nist_stc_06_asme1_ap242-e3.stp",
        bodies: 1,
        faces: 144,
        closed: 1,
        volume: Volume::Sister("nist_ftc_06_asme1_ap242-e2.stp"),
        state: State::KnownFailing(4.810e-6),
    },
    Case {
        name: "nist_ftc_11_asme1_rb.stp",
        bodies: 1,
        faces: 6,
        closed: 1,
        volume: Volume::ClosedForm(ftc_11_rb_closed_form),
        state: State::KnownFailing(0.3801),
    },
    Case {
        name: "nist_ftc_11_asme1_ap242-e2.stp",
        bodies: 1,
        faces: 42,
        closed: 1,
        volume: Volume::None(NO_FTC_11_AP242),
        state: State::Within,
    },
    Case {
        name: "turbine.stp",
        bodies: 1,
        faces: 1241,
        closed: 1,
        volume: Volume::None(NO_TURBINE),
        state: State::Within,
    },
    Case {
        name: "nist_ctc_05_asme1_rd.stp",
        bodies: 2,
        faces: 209,
        closed: 1,
        volume: Volume::Validation,
        state: State::KnownFailing(1.095e-6),
    },
    Case {
        name: "nist_ctc_05_asme1_ap203.stp",
        bodies: 1,
        faces: 156,
        closed: 1,
        volume: Volume::Validation,
        state: State::KnownFailing(1.208e-7),
    },
    Case {
        name: "nist_ctc_05_asme1_ap242-e1.stp",
        bodies: 1,
        faces: 156,
        closed: 1,
        volume: Volume::Sister("nist_ctc_05_asme1_ap203.stp"),
        state: State::KnownFailing(1.545e-7),
    },
    Case {
        name: "nist_stc_10_asme1_ap242-e2.stp",
        bodies: 1,
        faces: 256,
        closed: 1,
        volume: Volume::Validation,
        state: State::KnownFailing(3.396e-6),
    },
];

/// The volume of `brep` at [`ACCURACY`], or `None` where the integrator gives none.
fn volume(brep: &Brep) -> Option<f64> {
    let kernel = Kernel::default();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = brep.clone();
    brep_volume(&kernel, &mut copy, ACCURACY, &attrs).ok()
}

/// (bodies, faces over all bodies, closed bodies) of `report`.
fn counts(report: &ImportReport) -> (usize, usize, usize) {
    (
        report.bodies.len(),
        report.bodies.iter().map(|b| b.face_list().len()).sum(),
        report.bodies.iter().filter(|b| is_closed_solid(b)).count(),
    )
}

/// The summed volume of the closed bodies of `report`.
fn closed_volume(report: &ImportReport) -> f64 {
    report
        .bodies
        .iter()
        .filter(|b| is_closed_solid(b))
        .map(|b| volume(b).expect("a closed body has a volume"))
        .sum()
}

/// `name` read from the corpus and imported by today's reader.
fn import(dir: &std::path::Path, name: &str) -> (Vec<u8>, ImportReport) {
    let bytes = corpus_file(dir, name);
    let report = import_with_report(&bytes, &Limits::default())
        .unwrap_or_else(|error| panic!("{name}: import failed: {}", error.message));
    (bytes, report)
}

/// `∫ (x + c) √(a² − x²) dx`: the r-moment of a circular arc of radius `a` about a centre
/// at radius `c`, as a function of `x = r − c` (the antiderivative, closed form).
fn arc_moment(x: f64, a: f64, c: f64) -> f64 {
    let s = (a * a - x * x).max(0.0).sqrt();
    -(s * s * s) / 3.0 + c * (x * s / 2.0 + a * a / 2.0 * (x / a).asin())
}

/// The volume of `ftc_11_rb` in closed form, from the file's own geometry: a washer about
/// the z axis, faces `#95` (cylinder r 16, z −1.5…0), `#135` (cylinder r 31.5, z
/// −1.5…0), `#115` (plane z −1.5), `#163` (plane z 0.425, r 18.9385…28.5615) and two tori
/// on z = 0 of tube radius 1.5, `#187` about radius 17.5 and `#175` about radius 30, each
/// running from its cylinder over the top of its tube down to the plane at z = 0.425.
///
/// In the (r, z) half-plane the section is the rectangle 16…31.5 × −1.5…0 under the
/// curve z(r) = √(1.5² − (r − 17.5)²) for r in 16…18.9385, 0.425 on 18.9385…28.5615, and
/// √(1.5² − (r − 30)²) on 28.5615…31.5, where 18.9385 = 17.5 + √(1.5² − 0.425²) and
/// 28.5615 = 30 − √(1.5² − 0.425²) are the plane's circles `#158` and `#147`. By Pappus,
/// V = 2π ∫∫ r dr dz over the section.
fn ftc_11_rb_closed_form() -> f64 {
    let (tube, top, base) = (1.5_f64, 0.425_f64, 1.5_f64);
    let (r_in, r_out) = (16.0_f64, 31.5_f64);
    let (c_in, c_out) = (17.5_f64, 30.0_f64);
    let reach = (tube * tube - top * top).sqrt();
    let (flat_in, flat_out) = (c_in + reach, c_out - reach);
    let rectangle = base * (r_out * r_out - r_in * r_in) / 2.0;
    let inner = arc_moment(reach, tube, c_in) - arc_moment(-tube, tube, c_in);
    let flat = top * (flat_out * flat_out - flat_in * flat_in) / 2.0;
    let outer = arc_moment(tube, tube, c_out) - arc_moment(-reach, tube, c_out);
    2.0 * std::f64::consts::PI * (rectangle + inner + flat + outer)
}

/// The same washer as [`ftc_11_rb_closed_form`], by Pappus over the pieces of its
/// section, each an area times the radius of its centroid, from the textbook area and
/// centroid of a circular segment instead of an antiderivative:
///
/// - the rectangle 16…31.5 × −1.5…0;
/// - over each torus, the upper half of its tube's disk (area π a²/2, centroid on the
///   tube's centre radius) less the half of the circular segment beyond the chord where
///   the tube meets the plane z = 0.425 (the chord at distance d = √(a² − 0.425²) from
///   the centre, half-angle α = acos(d / a): segment area a² (α − sin α cos α), centroid
///   at 4 a sin³α / (3 (2α − sin 2α)) from the centre, the same for its upper half);
/// - the strip between the two chords, 0.425 high.
fn ftc_11_rb_by_pieces() -> f64 {
    use std::f64::consts::PI;
    let (a, top, base) = (1.5_f64, 0.425_f64, 1.5_f64);
    let (r_in, r_out, c_in, c_out) = (16.0_f64, 31.5_f64, 17.5_f64, 30.0_f64);
    let d = (a * a - top * top).sqrt();
    let alpha = (d / a).acos();
    let segment = a * a * (alpha - alpha.sin() * alpha.cos());
    let offset = 4.0 * a * alpha.sin().powi(3) / (3.0 * (2.0 * alpha - (2.0 * alpha).sin()));
    let half_disk = PI * a * a / 2.0;
    // (area, centroid radius) of each piece, the segments' halves taken away
    let pieces = [
        (base * (r_out - r_in), (r_in + r_out) / 2.0),
        (half_disk, c_in),
        (-segment / 2.0, c_in + offset),
        (
            top * ((c_out - d) - (c_in + d)),
            ((c_in + d) + (c_out - d)) / 2.0,
        ),
        (half_disk, c_out),
        (-segment / 2.0, c_out - offset),
    ];
    2.0 * PI * pieces.iter().map(|(area, r)| area * r).sum::<f64>()
}

/// The file's validation volume of `name`, if it writes one.
fn validation_volume_of(name: &str, bytes: &[u8]) -> Option<validation::ValidationVolume> {
    validation::validation_volume(&validation::parse(bytes), bytes)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// One inch-unit file: a `DERIVED_UNIT` of the cubed `CONVERSION_BASED_UNIT('INCH')`.
const INCH_VOLUME: &str = "#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\
#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\
#3=(CONVERSION_BASED_UNIT('INCH',#2)LENGTH_UNIT()NAMED_UNIT(*));\
#4=DERIVED_UNIT_ELEMENT(#3,3.0);#5=VOLUME_UNIT((#4));\
#6=MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE(2.0),#5);\
#7=PRODUCT_DEFINITION_SHAPE('','',$);\
#8=PROPERTY_DEFINITION('geometric validation property','volume of part',#7);\
#9=REPRESENTATION('volume',(#6),$);#10=PROPERTY_DEFINITION_REPRESENTATION(#8,#9);\
#11=PROPERTY_DEFINITION('pmi validation property','',#7);\
#12=MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE(9.0),#5);\
#13=REPRESENTATION('volume',(#12),$);#14=PROPERTY_DEFINITION_REPRESENTATION(#11,#13);";

/// `data` as a Part 21 file.
fn part21(data: &str) -> Vec<u8> {
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;\
         DATA;{data}ENDSEC;END-ISO-10303-21;"
    )
    .into_bytes()
}

/// The validation-volume helper reads a geometric validation property only, and converts
/// it from the file's unit: 2 in³ is 2 × 25.4³ = 32 774.128 mm³. The `'pmi validation
/// property'` beside it is not a geometric one. Its written `2.0` is good to half a unit
/// of its last digit, 0.05 of 2.0.
#[test]
fn validation_volume_converts_the_files_unit() {
    let bytes = part21(INCH_VOLUME);
    let file = validation::parse(&bytes);
    let volumes = validation::validation_volumes(&file, &bytes).expect("units resolve");
    assert_eq!(volumes.len(), 1, "{volumes:?}");
    assert_eq!(volumes[0].property, 8);
    assert_eq!(volumes[0].description, "volume of part");
    assert_eq!(volumes[0].written, 2.0);
    assert_eq!(volumes[0].literal, "2.0");
    assert_eq!(volumes[0].digits(), 0.05 / 2.0);
    assert_eq!(validation_bound(1e-9, &volumes[0]), 1e-9 + 0.025);
    let expected = 2.0 * 25.4 * 25.4 * 25.4;
    assert!(
        relative(volumes[0].mm3, expected) <= 1e-15,
        "{} mm³, expected {expected}",
        volumes[0].mm3
    );
    assert_eq!(
        validation::validation_volume(&file, &bytes).expect("one value"),
        Some(volumes[0].clone())
    );
}

/// A file whose geometric validation volumes disagree beyond their written digits is
/// reported, not averaged.
#[test]
fn disagreeing_validation_volumes_are_an_error() {
    let disagreeing = INCH_VOLUME.replace(
        "'pmi validation property'",
        "'geometric validation property'",
    );
    let bytes = part21(&disagreeing);
    let file = validation::parse(&bytes);
    assert_eq!(
        validation::validation_volumes(&file, &bytes).map(|v| v.len()),
        Ok(2)
    );
    assert!(validation::validation_volume(&file, &bytes).is_err());
}

/// Half a unit of the last digit written, with and without an exponent and a sign.
#[test]
fn half_unit_of_the_last_digit_written() {
    for (literal, half) in [
        ("7.746912251592E2", 5e-11),
        ("188312.244641244", 5e-10),
        ("4.710152747498E7", 5e-6),
        ("3.", 0.5),
        ("12", 0.5),
        ("-2.50e-3", 5e-6),
    ] {
        let got = validation::half_unit_of_last_digit(literal).expect("a number");
        assert!(
            relative(got, half) <= f64::EPSILON,
            "{literal}: {got}, expected {half}"
        );
    }
    for bad in ["", "E3", "1.2.3", "x"] {
        assert!(validation::half_unit_of_last_digit(bad).is_err(), "{bad}");
    }
}

/// The literal is read from the instance it belongs to, across a line break, and not
/// from an instance whose id only ends in the same digits.
#[test]
fn written_literal_is_the_instances_own() {
    let bytes = b"#16=MEASURE_REPRESENTATION_ITEM('v',VOLUME_MEASURE(1.5),#5);\n\
#6=MEASURE_REPRESENTATION_ITEM('volume measure',VOLUME_MEASURE(\n7.746912251592E2),#5);";
    assert_eq!(
        validation::written_literal(bytes, 6, "VOLUME_MEASURE").as_deref(),
        Some("7.746912251592E2")
    );
    assert_eq!(
        validation::written_literal(bytes, 16, "VOLUME_MEASURE").as_deref(),
        Some("1.5")
    );
    assert_eq!(
        validation::written_literal(bytes, 7, "VOLUME_MEASURE"),
        None
    );
}

/// [`ftc_11_rb_closed_form`] is the washer: it agrees with the independent decomposition
/// [`ftc_11_rb_by_pieces`] to rounding (each a sum of a few terms, so within a few ulps),
/// and with 5129.386136, the value jyviko re-derived on #2593, to its six decimals (half
/// a unit of the last one).
#[test]
fn ftc_11_rb_closed_form_is_the_washer() {
    let v = ftc_11_rb_closed_form();
    let pieces = ftc_11_rb_by_pieces();
    assert!(
        relative(v, pieces) <= 8.0 * f64::EPSILON,
        "antiderivative {v}, pieces {pieces}"
    );
    assert!((v - 5129.386136).abs() <= 0.5e-6, "{v}");
}

/// Each file imports with the bodies, faces and closed solids of its census, and no body
/// is skipped.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn winding_face_files_match_their_census() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut failures = Vec::new();
    for case in CASES {
        let (name, bodies, faces, closed) = (case.name, case.bodies, case.faces, case.closed);
        let (bytes, report) = import(&dir, name);
        let census = validation::body_census(&validation::parse(&bytes));
        let census = (
            census.len(),
            census.iter().map(|(_, _, faces)| faces).sum::<usize>(),
        );
        if census != (bodies, faces) {
            failures.push(format!(
                "{name}: census (bodies, faces) {census:?}, table ({bodies}, {faces})"
            ));
        }
        let got = counts(&report);
        println!("{name}: (bodies, faces, closed) {got:?}");
        if got != (bodies, faces, closed) {
            failures.push(format!(
                "{name}: (bodies, faces, closed) {got:?}, expected ({bodies}, {faces}, {closed})"
            ));
        }
        if !report.skipped.is_empty() {
            failures.push(format!("{name}: skipped {:?}", report.skipped));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The expected volume of `case` and its relative bound, or why it has none; `own` is
/// the file's own validation volume.
fn expected(
    dir: &std::path::Path,
    case: &Case,
    bytes: &[u8],
    own: Option<&validation::ValidationVolume>,
) -> Result<(f64, f64), String> {
    let name = case.name;
    match (case.volume, own) {
        (Volume::Validation, Some(own)) => Ok((own.mm3, validation_bound(ACCURACY, own))),
        (Volume::Validation, None) => Err(format!("{name}: no validation volume in the file")),
        (_, Some(own)) if own.written != 0.0 => Err(format!(
            "{name}: carries a validation volume {} mm³; assert it",
            own.mm3
        )),
        (Volume::Sister(sister), _) => {
            let sister_bytes = corpus_file(dir, sister);
            let sister_file = validation::parse(&sister_bytes);
            validation::same_form(
                &validation::form(&validation::parse(bytes))?,
                &validation::form(&sister_file)?,
            )
            .map_err(|error| format!("{name} and {sister} are not one part: {error}"))?;
            let sister_volume = validation::validation_volume(&sister_file, &sister_bytes)?
                .ok_or_else(|| format!("{sister}: no validation volume"))?;
            Ok((
                sister_volume.mm3,
                validation_bound(ACCURACY, &sister_volume),
            ))
        }
        // The closed forms are evaluated in f64 to a few ulps, far inside the accuracy.
        (Volume::ClosedForm(closed_form), _) => Ok((closed_form(), ACCURACY)),
        (Volume::None(why), _) => Err(format!("none: {why}")),
    }
}

/// Each file's volume against its expected one ([`Volume`]), within its bound: the
/// integrator's [`ACCURACY`] plus, for a validation volume, the digits the exporter wrote
/// ([`validation_bound`]). A file that is [`State::KnownFailing`] must still be beyond its
/// bound, by a finite deviation; once it is within, mark it [`State::Within`]. `stc_06`
/// writes its zero placeholder, which is not a measurement.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn winding_face_volumes_are_within_their_bound_or_known_failing() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut failures = Vec::new();
    for case in &CASES {
        let name = case.name;
        let (bytes, report) = import(&dir, name);
        let own = validation_volume_of(name, &bytes);
        if let Some(placeholder) = own.as_ref().filter(|v| v.written == 0.0) {
            println!("{name}: validation placeholder {}", placeholder.literal);
        }
        let measured = closed_volume(&report);
        let (expected, bound) = match expected(&dir, case, &bytes, own.as_ref()) {
            Ok(expected) => expected,
            Err(why) if why.starts_with("none: ") => {
                println!("{name}: volume {measured:.6} mm³, {why}");
                continue;
            }
            Err(why) => {
                failures.push(why);
                continue;
            }
        };
        let deviation = relative(measured, expected);
        println!(
            "{name}: volume {measured:.6} mm³, expected {expected:.6} mm³ ({:?}), \
             {deviation:.3e} relative, bound {bound:.3e}",
            case.volume
        );
        match case.state {
            State::Within if deviation > bound => failures.push(format!(
                "{name}: volume {measured} mm³, expected {expected} mm³, {deviation:.3e} \
                 relative, bound {bound:.3e}"
            )),
            State::KnownFailing(today) if !deviation.is_finite() => failures.push(format!(
                "{name}: no measurable volume ({measured} mm³), so the known failure \
                 ({today:.2e}) is not the one pinned"
            )),
            State::KnownFailing(today) if deviation <= bound => failures.push(format!(
                "{name} is within its bound now ({deviation:.3e} <= {bound:.3e}, was \
                 {today:.2e}); mark it State::Within"
            )),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `ftc_11_rb` and `ftc_11_ap242-e2` are not one part, so the AP242 file has no sister
/// volume (the module docs).
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn ftc_11_exports_are_two_parts() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let form = |name| {
        validation::form(&validation::parse(&corpus_file(&dir, name)))
            .unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    let difference = validation::same_form(
        &form("nist_ftc_11_asme1_rb.stp"),
        &form("nist_ftc_11_asme1_ap242-e2.stp"),
    );
    println!("{difference:?}");
    assert!(difference.is_err());
}

/// [`validation::same_form`] finds one part however the export turns and moves it, and
/// tells a part whose corner moved by more than the uncertainty from it.
#[test]
fn same_form_finds_a_turned_part_and_a_moved_corner() {
    let corners = vec![
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 2.0, 0.0],
        [0.0, 2.0, 1.0],
    ];
    let form = |corners: Vec<[f64; 3]>| validation::Form {
        vertices: corners.clone(),
        corners,
        box_sizes: [1.0, 2.0, 4.0],
        radii: vec![("cylinder", vec![1.5])],
        uncertainty: 0.01,
    };
    let part = form(corners.clone());
    // turned a quarter about z and moved: (x, y, z) -> (10 - y, x, z + 3)
    let turned = form(
        corners
            .iter()
            .map(|p| [10.0 - p[1], p[0], p[2] + 3.0])
            .collect(),
    );
    assert_eq!(validation::same_form(&part, &turned), Ok(()));
    let mut moved = corners;
    moved[2] = [4.0, 2.05, 0.0];
    let moved = form(moved);
    assert!(validation::same_form(&part, &moved).is_err());
}

/// A volume that is not finite never counts as close to its expected value.
#[test]
fn relative_deviation_of_a_non_finite_volume_is_infinite() {
    assert_eq!(relative(f64::NAN, 1.0), f64::INFINITY);
    assert_eq!(relative(1.0, f64::INFINITY), f64::INFINITY);
    assert_eq!(relative(3.0, 4.0), 0.25);
}
