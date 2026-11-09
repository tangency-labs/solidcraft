//! A panic fuzz of STEP import (#2490): the repo's STEP files and files written by the
//! exporter, mutated (truncated, bytes flipped, numbers replaced by huge counts, NaN or
//! infinities, references broken or made cyclic, lines dropped or repeated, entity names
//! swapped, lists nested deep), parsed and built into B-reps with
//! [`step_brep::import_with_report_options`], and the built bodies exported again. Also
//! [`step_import::flat::decode`] on mutated arrays. A result of `Err` is fine; a panic
//! fails the test with the reproducing seed and the mutations.
//!
//! The harness is solidcore's (`crates/solidcore/tests/support/panic_fuzz.rs`); see it
//! for `SOLIDCORE_PANIC_FUZZ_CASES` and the other variables.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "../../solidcore/tests/support/panic_fuzz.rs"]
mod panic_fuzz;

use std::sync::OnceLock;

use panic_fuzz::{assert_no_findings, fuzz, Family, Rng, Settings};
use solidcore::primitive::api_primitives as prim;
use solidcore::{CancelToken, Kernel, Point3};
use step_brep::{BuildMode, BuildOptions};
use step_import::Limits;

/// The files mutated: the repo's STEP fixtures and the exporter's output for a few
/// primitives.
fn bases() -> &'static [Vec<u8>] {
    static BASES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    BASES.get_or_init(|| {
        let mut out = vec![
            include_bytes!("fixtures/discord-2026-10-30-fillet.step").to_vec(),
            include_bytes!("../../goldens/src/units/topology_brep_constructor_tetra.stp").to_vec(),
        ];
        let mut k = Kernel::default();
        let o = Point3::new(0.0, 0.0, 0.0);
        let bodies = [
            prim::create_box(&mut k, o, 1.0, 2.0, 3.0),
            prim::create_cylinder(&mut k, o, 1.0, 2.0),
            prim::create_cone(&mut k, o, 2.0, 1.0, 3.0),
            // (not the torus: building it back takes a debug build seconds per case)
            prim::create_sphere(&mut k, o, 2.0),
        ];
        for b in bodies {
            let b = b.expect("a primitive");
            let model = step_brep::brep_to_model(&b).expect("export model");
            out.push(step_export::write(&model).expect("export").into_bytes());
        }
        out
    })
}

/// One edit at a place given as a fraction of the file.
#[derive(Clone, Debug)]
enum Edit {
    Truncate(f64),
    FlipBit(f64, u8),
    /// The number token at or after the place becomes `text`.
    Number(f64, &'static str),
    /// The reference (`#n`) at or after the place becomes `#text`.
    Reference(f64, &'static str),
    DropLine(f64),
    RepeatLine(f64, usize),
    /// The entity name at or after the place becomes `name`.
    Entity(f64, &'static str),
    /// `depth` nested parentheses inserted at the place.
    Nest(f64, usize),
    Insert(f64, &'static str),
}

const NUMBERS: &[&str] = &[
    "NAN",
    "nan",
    "1.E999",
    "-1.E999",
    "INF",
    "1.E308",
    "1.E-320",
    "0.",
    "-0.",
    "-1",
    "0",
    "4294967296",
    "18446744073709551616",
    "99999999999999999999999999",
    "9223372036854775807",
    "-9223372036854775808",
    "1.E-300",
    "1.",
    "2",
    "3",
    ".5",
    "1E",
    "--1",
];

const REFERENCES: &[&str] = &[
    "0",
    "-1",
    "1",
    "2",
    "99999999",
    "18446744073709551616",
    "",
    "#",
    "10",
];

const ENTITIES: &[&str] = &[
    "CARTESIAN_POINT",
    "DIRECTION",
    "VECTOR",
    "LINE",
    "CIRCLE",
    "ELLIPSE",
    "PLANE",
    "CYLINDRICAL_SURFACE",
    "CONICAL_SURFACE",
    "SPHERICAL_SURFACE",
    "TOROIDAL_SURFACE",
    "B_SPLINE_CURVE_WITH_KNOTS",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "AXIS2_PLACEMENT_3D",
    "EDGE_CURVE",
    "VERTEX_POINT",
    "ORIENTED_EDGE",
    "EDGE_LOOP",
    "FACE_BOUND",
    "FACE_OUTER_BOUND",
    "ADVANCED_FACE",
    "CLOSED_SHELL",
    "OPEN_SHELL",
    "MANIFOLD_SOLID_BREP",
    "SURFACE_OF_REVOLUTION",
    "SURFACE_OF_LINEAR_EXTRUSION",
    "TRIMMED_CURVE",
    "COMPOSITE_CURVE",
    "PCURVE",
    "SEAM_CURVE",
    "POLY_LOOP",
    "FCSRF",
    "PLYLP",
];

const INSERTS: &[&str] = &[
    "'",
    "(",
    ")",
    ";",
    "#1=#1;",
    "#1=CARTESIAN_POINT('',(1.,2.,3.));",
    "*",
    "$",
    ".T.",
    ".X.",
    "/*",
    "\\X2\\",
    "\u{0}",
    "\u{feff}",
    "ENDSEC;",
    "(1.,(2.,(3.)))",
];

fn edit(r: &mut Rng) -> Edit {
    let at = r.unit();
    match r.below(10) {
        0 => Edit::Truncate(at),
        1 => Edit::FlipBit(at, r.below(8) as u8),
        2 | 3 => Edit::Number(at, r.pick(NUMBERS)),
        4 => Edit::Reference(at, r.pick(REFERENCES)),
        5 => Edit::DropLine(at),
        6 => Edit::RepeatLine(at, 1 + r.below(3)),
        7 => Edit::Entity(at, r.pick(ENTITIES)),
        8 => Edit::Nest(at, r.pick(&[2, 8, 31, 33, 200])),
        _ => Edit::Insert(at, r.pick(INSERTS)),
    }
}

fn place(bytes: &[u8], at: f64) -> usize {
    ((at * bytes.len() as f64) as usize).min(bytes.len())
}

/// The token at or after `i` for which `start` holds, as a range continuing while
/// `cont` holds.
fn token(
    bytes: &[u8],
    i: usize,
    start: impl Fn(u8) -> bool,
    cont: impl Fn(u8) -> bool,
) -> Option<(usize, usize)> {
    let s = (i..bytes.len()).find(|&j| start(bytes[j]))?;
    let e = (s + 1..bytes.len())
        .find(|&j| !cont(bytes[j]))
        .unwrap_or(bytes.len());
    Some((s, e))
}

/// The entity name of the first instance definition (`#<id> = NAME(` or, for a complex
/// instance, `#<id> = (NAME(`) at or after `i`, so an edit swaps a type and never hits
/// `.T.`, `.F.`, an exponent's `E` or a string.
fn entity_name(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    let skip = |mut j: usize, f: &dyn Fn(u8) -> bool| {
        while j < bytes.len() && f(bytes[j]) {
            j += 1;
        }
        j
    };
    let mut from = i;
    while let Some((_, id_end)) = token(bytes, from, |b| b == b'#', |b| b.is_ascii_digit()) {
        from = id_end;
        let j = skip(id_end, &|b| b.is_ascii_whitespace());
        if bytes.get(j) != Some(&b'=') {
            continue;
        }
        let s = skip(j + 1, &|b| b.is_ascii_whitespace() || b == b'(');
        if !bytes.get(s).is_some_and(u8::is_ascii_uppercase) {
            continue;
        }
        let e = skip(s, &|b| {
            b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit()
        });
        return Some((s, e));
    }
    None
}

fn line_at(bytes: &[u8], i: usize) -> (usize, usize) {
    let s = bytes[..i]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let e = bytes[i..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |p| i + p + 1);
    (s, e)
}

fn apply(bytes: &mut Vec<u8>, e: &Edit) {
    if bytes.is_empty() {
        return;
    }
    let splice = |bytes: &mut Vec<u8>, (s, e): (usize, usize), text: &[u8]| {
        bytes.splice(s..e, text.iter().copied());
    };
    match *e {
        Edit::Truncate(at) => bytes.truncate(place(bytes, at)),
        Edit::FlipBit(at, bit) => {
            let i = place(bytes, at).min(bytes.len() - 1);
            bytes[i] ^= 1 << bit;
        }
        Edit::Number(at, text) => {
            let is_num =
                |b: u8| b.is_ascii_digit() || b == b'.' || b == b'E' || b == b'-' || b == b'+';
            // a number not part of a reference
            let mut i = place(bytes, at);
            while let Some((s, end)) = token(bytes, i, |b| b.is_ascii_digit(), is_num) {
                if s > 0 && bytes[s - 1] == b'#' {
                    i = end;
                    continue;
                }
                splice(bytes, (s, end), text.as_bytes());
                break;
            }
        }
        Edit::Reference(at, text) => {
            if let Some((s, end)) = token(
                bytes,
                place(bytes, at),
                |b| b == b'#',
                |b| b.is_ascii_digit(),
            ) {
                let mut t = b"#".to_vec();
                t.extend_from_slice(text.as_bytes());
                splice(bytes, (s, end), &t);
            }
        }
        Edit::DropLine(at) => {
            let i = place(bytes, at).min(bytes.len() - 1);
            let range = line_at(bytes, i);
            splice(bytes, range, b"");
        }
        Edit::RepeatLine(at, n) => {
            let i = place(bytes, at).min(bytes.len() - 1);
            let (s, end) = line_at(bytes, i);
            let line = bytes[s..end].to_vec();
            for _ in 0..n {
                bytes.splice(end..end, line.iter().copied());
            }
        }
        Edit::Entity(at, name) => {
            if let Some(range) = entity_name(bytes, place(bytes, at)) {
                splice(bytes, range, name.as_bytes());
            }
        }
        Edit::Nest(at, depth) => {
            let i = place(bytes, at);
            let mut t = "(".repeat(depth);
            t.push_str("1.");
            t.push_str(&")".repeat(depth));
            bytes.splice(i..i, t.into_bytes());
        }
        Edit::Insert(at, text) => {
            let i = place(bytes, at);
            bytes.splice(i..i, text.bytes());
        }
    }
}

#[derive(Clone, Debug)]
struct StepCase {
    base: usize,
    edits: Vec<Edit>,
    options: BuildOptions,
    tight_limits: bool,
}

struct StepImport;

impl Family for StepImport {
    type Input = StepCase;

    fn name(&self) -> &'static str {
        "step"
    }

    fn generate(&self, r: &mut Rng) -> StepCase {
        StepCase {
            base: r.below(bases().len()),
            edits: (0..=r.below(4)).map(|_| edit(r)).collect(),
            options: BuildOptions {
                heal: r.chance(0.2),
                skip_cone_seams: r.chance(0.2),
                mode: r.pick(&[BuildMode::Default, BuildMode::Strict, BuildMode::StitchOnly]),
                file_pcurves: r.chance(0.3),
            },
            tight_limits: r.chance(0.2),
        }
    }

    fn run(case: StepCase, _cancel: &CancelToken) {
        let mut bytes = bases()[case.base].clone();
        for e in &case.edits {
            apply(&mut bytes, e);
        }
        let limits = if case.tight_limits {
            Limits {
                max_entities: 200,
                max_list_depth: 4,
                max_ref_depth: 4,
                max_list_len: 16,
            }
        } else {
            Limits::default()
        };
        if let Ok(model) = step_import::import(&bytes, &limits) {
            for body in &model.bodies {
                let flat = step_import::flat::encode(body);
                let _ = step_import::flat::decode(&flat);
            }
        }
        if let Ok(report) = step_brep::import_with_report_options(&bytes, &limits, case.options) {
            for b in &report.bodies {
                let _ = step_brep::is_closed_solid(b);
                let _ = step_brep::has_uv_trims(b);
                let _ = step_brep::validate_for_display(b);
                if let Ok(model) = step_brep::brep_to_model(b) {
                    let _ = step_export::write(&model);
                }
            }
        }
    }
}

/// `flat::decode` on a body's array with values replaced by hostile ones (counts,
/// kinds and coordinates alike) or the array cut short.
#[derive(Clone, Debug)]
struct FlatCase {
    base: usize,
    edits: Vec<(f64, f64)>,
    truncate: Option<f64>,
}

struct FlatDecode;

impl Family for FlatDecode {
    type Input = FlatCase;

    fn name(&self) -> &'static str {
        "step-flat"
    }

    fn generate(&self, r: &mut Rng) -> FlatCase {
        FlatCase {
            base: r.below(bases().len()),
            edits: (0..=r.below(4))
                .map(|_| {
                    let v = if r.chance(0.5) {
                        r.pick(panic_fuzz::HOSTILE)
                    } else {
                        r.pick(&[-1.0, 0.0, 1.0, 2.0, 3.0, 7.0, 1e9, 4294967296.0, 0.5, 1e19])
                    };
                    (r.unit(), v)
                })
                .collect(),
            truncate: r.chance(0.3).then(|| r.unit()),
        }
    }

    fn run(case: FlatCase, _cancel: &CancelToken) {
        let Ok(model) = step_import::import(&bases()[case.base], &Limits::default()) else {
            return;
        };
        for body in &model.bodies {
            let mut flat = step_import::flat::encode(body);
            if flat.is_empty() {
                continue;
            }
            for &(at, v) in &case.edits {
                let i = ((at * flat.len() as f64) as usize).min(flat.len() - 1);
                flat[i] = v;
            }
            if let Some(at) = case.truncate {
                flat.truncate((at * flat.len() as f64) as usize);
            }
            let _ = step_import::flat::decode(&flat);
        }
    }
}

/// `Edit::Entity` replaces an entity name of an instance definition, wherever it lands in
/// the file, and nothing else (`.T.`, `.F.`, an exponent's `E`, a string).
#[test]
fn entity_edit_lands_on_an_entity_name() {
    let bytes = include_bytes!("fixtures/discord-2026-10-30-fillet.step");
    let text = std::str::from_utf8(bytes).unwrap();
    let mut complex = 0;
    for k in 0..1000 {
        let at = place(bytes, f64::from(k) / 1000.0);
        let Some((s, e)) = entity_name(bytes, at) else {
            // past the last definition only
            assert!(!text[at..].contains("\n#"), "no name found from byte {at}");
            continue;
        };
        let name = &text[s..e];
        assert!(
            name.bytes().next().unwrap().is_ascii_uppercase()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit()),
            "{name:?} at byte {s}"
        );
        assert_eq!(
            bytes.get(e),
            Some(&b'('),
            "{name:?} at byte {s} is not followed by `(`"
        );
        // what precedes the name is `#<id>=`, then optionally `(` for a complex instance
        let head = text[..s].trim_end();
        let head = match head.strip_suffix('(') {
            Some(h) => {
                complex += 1;
                h.trim_end()
            }
            None => head,
        };
        let head = head
            .strip_suffix('=')
            .unwrap_or_else(|| panic!("{name:?} at byte {s}"));
        let id = head.trim_end().rsplit('#').next().unwrap();
        assert!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
            "{name:?} at byte {s} does not follow `#<id>=`"
        );
        assert!(s >= at, "the name starts before the edit's place");
    }
    assert!(complex > 0, "no complex instance was reached");
}

#[test]
fn panic_fuzz_step_import() {
    let findings = fuzz(&StepImport, &Settings::for_family("step", 30));
    assert_no_findings("step", &findings);
}

#[test]
fn panic_fuzz_step_flat_decode() {
    let findings = fuzz(&FlatDecode, &Settings::for_family("step-flat", 200));
    assert_no_findings("step-flat", &findings);
}
