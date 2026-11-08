//! Shared corpus helpers (`#[path]`-included): the exporter's own geometric validation
//! volumes of a STEP file, read through `step_import`'s Part 21 parser and converted to
//! mm³, and the file's body and face census, so that expected values come from the file
//! and never from the reader under test.
//!
//! A validation volume is written as the CAx-IF Recommended Practices for Geometric and
//! Assembly Validation Properties (release 4.2, §4 and its summary table) describe it:
//! a `PROPERTY_DEFINITION` named `'geometric validation property'`, linked by a
//! `PROPERTY_DEFINITION_REPRESENTATION` to a `REPRESENTATION` whose items include a
//! `MEASURE_REPRESENTATION_ITEM` with a `VOLUME_MEASURE` value and a volume unit. The
//! unit is a `DERIVED_UNIT` (or `VOLUME_UNIT`) of `DERIVED_UNIT_ELEMENT`s whose length
//! units, an `SI_UNIT` or a `CONVERSION_BASED_UNIT` such as `'INCH'` (25.4 mm), are
//! resolved through the file (ISO 10303-41); the volume in mm³ is the written value
//! times the product of each element's mm-per-unit raised to its exponent.
//!
//! **Bounds.** A measured volume is compared with an expected one within
//! [`ACCURACY`], the accuracy the tests ask the integrator for, plus, against a validation
//! volume, half a unit of the last digit the exporter wrote, relative to the value
//! ([`ValidationVolume::digits`], [`validation_bound`]). The part21 parser keeps the
//! number, not its text, so the literal is read from the file's bytes
//! ([`written_literal`]).
//!
//! Follow-up: everything here reads the file through `step_import::part21`, the crate
//! being replaced. When #2591 lands, it reads through the record's parser instead, so the
//! expected values do not depend on `step_import`.
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    dead_code
)] // test helpers; each test uses a subset

use std::path::{Path, PathBuf};

use step_import::part21::{self, Instance, Param, Record, StepFile};
use step_import::Limits;

/// The accuracy every volume of these tests is computed at: `brep_volume`'s `accuracy`.
///
/// What the argument controls (`solidcore::mass::integrator`): each face's trims are
/// integrated knot span by knot span with adaptive Cash–Karp steps, and a step is kept
/// only when its estimated error in every component is at most `accuracy / 10` of that
/// component's scale, the running value plus the step's increment and at least a floor
/// (for the volume, a tenth of the estimated area times a body length,
/// `integrator::properties::boundary_properties`); the iso-curve integrals inside each
/// step run at `accuracy / 10` too. It is a relative error target per step, not a proven
/// bound on the sum, and it does not cover the trims themselves. The tests take it as the
/// relative accuracy of the volume, as agreed on #2593; on every corpus file of #2580
/// with a volume the volume at 1e-9 is within 7e-11 of the volume at 1e-11 (#2593).
pub const ACCURACY: f64 = 1e-9;

/// One geometric validation volume of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationVolume {
    /// The `PROPERTY_DEFINITION`'s `#id`.
    pub property: u64,
    /// What the property is defined on (its `definition` attribute's `#id`).
    pub definition: u64,
    /// The property's description, e.g. `'volume of NIST_CTC_05_ASME1_RD'` (may be empty).
    pub description: String,
    /// The value as written, in the file's volume unit.
    pub written: f64,
    /// The number as the file writes it, e.g. `7.746912251592E2`.
    pub literal: String,
    /// The value in mm³.
    pub mm3: f64,
}

impl ValidationVolume {
    /// Half a unit of the last digit written, relative to the value: how far the
    /// exporter's number can be from the one it rounded (`7.746912251592E2` is
    /// 5e-11 / 774.69… = 6.5e-14).
    pub fn digits(&self) -> f64 {
        half_unit_of_last_digit(&self.literal).unwrap_or(f64::INFINITY) / self.written.abs()
    }
}

/// The relative bound of a volume computed at `accuracy` against `validation`: the
/// accuracy plus the digits the exporter wrote ([`ValidationVolume::digits`]).
pub fn validation_bound(accuracy: f64, validation: &ValidationVolume) -> f64 {
    accuracy + validation.digits()
}

/// Half a unit of the last digit of the Part 21 real `literal`: `7.746912251592E2` is
/// 0.5e-12 × 10² = 5e-11, `188312.244641244` 5e-10, `3.` 0.5.
///
/// # Errors
///
/// When `literal` is not a Part 21 number.
pub fn half_unit_of_last_digit(literal: &str) -> Result<f64, String> {
    let upper = literal.to_ascii_uppercase();
    let (mantissa, exponent) = match upper.split_once('E') {
        Some((m, e)) => (
            m,
            e.parse::<i32>()
                .map_err(|_| format!("{literal}: exponent is not an integer"))?,
        ),
        None => (upper.as_str(), 0),
    };
    let digits = mantissa.trim_start_matches(['+', '-']);
    let decimals = match digits.split_once('.') {
        Some((whole, fraction))
            if whole
                .chars()
                .chain(fraction.chars())
                .all(|c| c.is_ascii_digit()) =>
        {
            fraction.len()
        }
        None if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => 0,
        _ => return Err(format!("{literal}: not a number")),
    };
    let decimals = i32::try_from(decimals).map_err(|_| format!("{literal}: too long"))?;
    Ok(0.5 * 10f64.powi(exponent - decimals))
}

/// The number written as `keyword(…)` in the instance `#id` of the Part 21 `bytes`, as
/// text: the first one after `#id=`, whitespace between the tokens skipped (exporters
/// break long instances over lines).
pub fn written_literal(bytes: &[u8], id: u64, keyword: &str) -> Option<String> {
    let label = format!("#{id}");
    let mut from = 0;
    while let Some(at) = find_bytes(bytes.get(from..)?, label.as_bytes()) {
        let start = from + at;
        from = start + label.len();
        if start > 0 && bytes.get(start - 1).is_some_and(u8::is_ascii_digit) {
            continue;
        }
        let rest = bytes.get(from..)?;
        let after = rest.iter().position(|b| !b.is_ascii_whitespace())?;
        if rest.get(after) != Some(&b'=') {
            continue;
        }
        let instance = rest.get(after..)?;
        let end = instance.iter().position(|&b| b == b';')?;
        let instance = instance.get(..end)?;
        let open = format!("{keyword}(");
        let at = find_bytes(instance, open.as_bytes())?;
        let text: String = instance
            .get(at + open.len()..)?
            .iter()
            .map(|&b| char::from(b))
            .skip_while(char::is_ascii_whitespace)
            .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'E' | 'e'))
            .collect();
        return (!text.is_empty()).then_some(text);
    }
    None
}

/// The first position of `needle` in `haystack`.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The corpus directory, or `None` when `SOLIDCORE_STEP_CORPUS` is unset.
pub fn corpus_dir() -> Option<PathBuf> {
    std::env::var_os("SOLIDCORE_STEP_CORPUS").map(PathBuf::from)
}

/// The file named `name` anywhere under the corpus `dir` (searched recursively, in
/// sorted order, so the STEP Tools and NIST directories can sit side by side or be
/// given on their own), read in place.
pub fn corpus_file(dir: &Path, name: &str) -> Vec<u8> {
    let path = find(dir, name).unwrap_or_else(|| {
        panic!(
            "{name} is not under {}; fetch the corpus with scripts/fetch-steptools-corpus.sh \
             and scripts/fetch-nist-pmi-corpus.sh",
            dir.display()
        )
    });
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The first path named `name` under `dir`, depth first in sorted order.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if path.file_name().and_then(std::ffi::OsStr::to_str) == Some(name) {
            return Some(path);
        }
    }
    None
}

/// `bytes` parsed by `step_import`'s Part 21 parser.
pub fn parse(bytes: &[u8]) -> StepFile {
    part21::parse(bytes, &Limits::default()).expect("the file parses")
}

/// Every geometric validation volume of `file` (parsed from `bytes`), in file order,
/// converted to mm³.
///
/// # Errors
///
/// When a volume's unit cannot be resolved to lengths, or its written number cannot be
/// read back from `bytes`.
pub fn validation_volumes(file: &StepFile, bytes: &[u8]) -> Result<Vec<ValidationVolume>, String> {
    let mut out = Vec::new();
    for instance in file.entities.values() {
        let Some(link) = instance.record("PROPERTY_DEFINITION_REPRESENTATION") else {
            continue;
        };
        let (Some(property), Some(representation)) = (ref_at(link, 0), ref_at(link, 1)) else {
            continue;
        };
        let Some(definition) = record(file, property, "PROPERTY_DEFINITION") else {
            continue;
        };
        if str_at(definition, 0) != Some("geometric validation property") {
            continue;
        }
        let Some(representation) = record(file, representation, "REPRESENTATION") else {
            continue;
        };
        let items = representation
            .params
            .get(1)
            .and_then(Param::as_list)
            .unwrap_or_default();
        for item in items.iter().filter_map(Param::as_ref_id) {
            let Some(measure) = record(file, item, "MEASURE_REPRESENTATION_ITEM") else {
                continue;
            };
            let Some(Param::Typed(kind, value)) = measure.params.get(1) else {
                continue;
            };
            if kind != "VOLUME_MEASURE" {
                continue;
            }
            let written = value
                .as_f64()
                .ok_or_else(|| format!("#{item}: VOLUME_MEASURE is not a number"))?;
            let unit = ref_at(measure, 2).ok_or_else(|| format!("#{item}: no unit"))?;
            let factor = volume_mm3(file, unit).map_err(|error| format!("#{item}: {error}"))?;
            let literal = written_literal(bytes, item, "VOLUME_MEASURE")
                .filter(|text| text.parse::<f64>() == Ok(written))
                .ok_or_else(|| format!("#{item}: the written VOLUME_MEASURE is not {written}"))?;
            out.push(ValidationVolume {
                property,
                definition: ref_at(definition, 2).unwrap_or_default(),
                description: str_at(definition, 1).unwrap_or_default().to_string(),
                written,
                literal,
                mm3: written * factor,
            });
        }
    }
    Ok(out)
}

/// The one validation volume of `file` (parsed from `bytes`): `None` where the file has
/// none, else the first, when every validation volume of the file gives the same value
/// to the digits written (a file often writes it twice, on the part and on its shape
/// aspect).
///
/// # Errors
///
/// When a unit cannot be resolved, or two of the file's validation volumes differ by
/// more than their written digits allow.
pub fn validation_volume(
    file: &StepFile,
    bytes: &[u8],
) -> Result<Option<ValidationVolume>, String> {
    let volumes = validation_volumes(file, bytes)?;
    let Some(first) = volumes.first() else {
        return Ok(None);
    };
    for other in &volumes {
        let allowed = first.digits() * first.mm3.abs() + other.digits() * other.mm3.abs();
        if (other.mm3 - first.mm3).abs() > allowed {
            return Err(format!(
                "validation volumes disagree: #{} {} mm³, #{} {} mm³",
                first.property, first.mm3, other.property, other.mm3
            ));
        }
    }
    Ok(Some(first.clone()))
}

/// The body entities `step_import` maps, as the file declares them: (`#id`, entity,
/// faces of its shells). Faces are counted over every shell the body holds (a solid's
/// outer shell and voids, a surface model's shells), so they are the file's, not the
/// reader's.
pub fn body_census(file: &StepFile) -> Vec<(u64, String, usize)> {
    const BODIES: [&str; 4] = [
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
        "FACETED_BREP",
        "SHELL_BASED_SURFACE_MODEL",
    ];
    let mut out = Vec::new();
    for (&id, instance) in &file.entities {
        let Instance::Simple(body) = instance else {
            continue;
        };
        if !BODIES.contains(&body.name.as_str()) {
            continue;
        }
        let mut shells: Vec<u64> = Vec::new();
        for param in body.params.iter().skip(1) {
            match param {
                Param::Ref(shell) => shells.push(*shell),
                Param::List(list) => shells.extend(list.iter().filter_map(Param::as_ref_id)),
                _ => {}
            }
        }
        let faces = shells.iter().map(|&shell| shell_faces(file, shell)).sum();
        out.push((id, body.name.clone(), faces));
    }
    out
}

/// The number of faces of the shell `#id` (through an `ORIENTED_CLOSED_SHELL` or
/// `ORIENTED_OPEN_SHELL` to the shell it orients).
fn shell_faces(file: &StepFile, id: u64) -> usize {
    let Some(Instance::Simple(shell)) = file.entities.get(&id) else {
        return 0;
    };
    match shell.name.as_str() {
        "CLOSED_SHELL" | "OPEN_SHELL" => shell
            .params
            .get(1)
            .and_then(Param::as_list)
            .map_or(0, <[Param]>::len),
        "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
            ref_at(shell, 2).map_or(0, |inner| shell_faces(file, inner))
        }
        _ => 0,
    }
}

/// The shape a file describes, as far as two exports of one part must agree on it
/// whatever their faces: the sizes of its vertices' box, its curved-surface radii and
/// its corners, all in mm. Two exporters split a part's periodic faces differently (one
/// cylinder face or two halves) and put a full circle's one vertex anywhere on it, so
/// face, edge and vertex counts and the vertices of closed edges are not compared.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    /// Every `VERTEX_POINT`'s point.
    pub vertices: Vec<[f64; 3]>,
    /// The points of the vertices that are not the one vertex of a closed edge (an
    /// `EDGE_CURVE` from a vertex to itself): where faces meet in a corner.
    pub corners: Vec<[f64; 3]>,
    /// The box of the `VERTEX_POINT`s, its three sizes in ascending order (exports place
    /// the part differently, so the axes are not compared).
    pub box_sizes: [f64; 3],
    /// The radii of the `CYLINDRICAL_SURFACE`s, `SPHERICAL_SURFACE`s and
    /// `TOROIDAL_SURFACE`s (major, minor), in file order. Cones are left out: their
    /// radius depends on where the export places the axis.
    pub radii: Vec<(&'static str, Vec<f64>)>,
    /// The file's largest `UNCERTAINTY_MEASURE_WITH_UNIT`, in mm: how far it allows its
    /// geometry to be from the model.
    pub uncertainty: f64,
}

/// The [`Form`] of `file`.
///
/// # Errors
///
/// When its length unit or uncertainty cannot be resolved, or it has no vertex.
pub fn form(file: &StepFile) -> Result<Form, String> {
    let mm = file_length_mm(file)?;
    let mut uncertainty: f64 = 0.0;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut radii: Vec<(&'static str, Vec<f64>)> = Vec::new();
    let mut seams: Vec<u64> = Vec::new();
    for instance in file.entities.values() {
        let Some(edge) = instance.record("EDGE_CURVE") else {
            continue;
        };
        if let (Some(start), Some(end)) = (ref_at(edge, 1), ref_at(edge, 2)) {
            if start == end {
                seams.push(start);
            }
        }
    }
    let mut vertices = Vec::new();
    let mut corners = Vec::new();
    for (&id, instance) in &file.entities {
        if let Some(measure) = instance.record("UNCERTAINTY_MEASURE_WITH_UNIT") {
            let value = measure
                .params
                .first()
                .and_then(Param::as_f64)
                .ok_or("uncertainty value")?;
            let unit = ref_at(measure, 1).ok_or("uncertainty unit")?;
            uncertainty = uncertainty.max(value * length_mm(file, unit, 0)?);
        }
        let Instance::Simple(record) = instance else {
            continue;
        };
        let number = |index: usize| record.params.get(index).and_then(Param::as_f64);
        match record.name.as_str() {
            "VERTEX_POINT" => {
                let point = ref_at(record, 1)
                    .and_then(|p| file.entities.get(&p))
                    .and_then(|p| p.record("CARTESIAN_POINT"))
                    .and_then(|p| p.params.get(1))
                    .and_then(Param::as_list)
                    .ok_or("a vertex without a point")?;
                let mut at = [0.0; 3];
                for (axis, coordinate) in point.iter().take(3).enumerate() {
                    let value = coordinate.as_f64().ok_or("a coordinate")? * mm;
                    low[axis] = low[axis].min(value);
                    high[axis] = high[axis].max(value);
                    at[axis] = value;
                }
                vertices.push(at);
                if !seams.contains(&id) {
                    corners.push(at);
                }
            }
            "CYLINDRICAL_SURFACE" | "SPHERICAL_SURFACE" => {
                let kind = if record.name == "CYLINDRICAL_SURFACE" {
                    "cylinder"
                } else {
                    "sphere"
                };
                radii.push((kind, vec![number(2).ok_or("a radius")? * mm]));
            }
            "TOROIDAL_SURFACE" => radii.push((
                "torus",
                vec![
                    number(2).ok_or("a major radius")? * mm,
                    number(3).ok_or("a minor radius")? * mm,
                ],
            )),
            _ => {}
        }
    }
    if !low[0].is_finite() {
        return Err("no vertex".into());
    }
    let mut box_sizes = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    box_sizes.sort_by(f64::total_cmp);
    Ok(Form {
        vertices,
        corners,
        box_sizes,
        radii,
        uncertainty,
    })
}

/// Whether `a` and `b` are one part: the same box sizes, the same set of radii (each
/// kind and radius of one has a match in the other), and every corner of the one with
/// fewer corners at a vertex of the other (an export that splits more faces has more
/// corners), all within the larger of the two files' uncertainties. The exports may
/// place the part differently: the corners are compared under each of the 48 signed
/// permutations of the axes, with the two vertex boxes' low corners matched.
///
/// # Errors
///
/// What differs.
pub fn same_form(a: &Form, b: &Form) -> Result<(), String> {
    let tolerance = a.uncertainty.max(b.uncertainty);
    let close = |x: &f64, y: &f64| (x - y).abs() <= tolerance;
    if !a
        .box_sizes
        .iter()
        .zip(&b.box_sizes)
        .all(|(x, y)| close(x, y))
    {
        return Err(format!(
            "box sizes {:?} and {:?} mm differ by more than {tolerance} mm",
            a.box_sizes, b.box_sizes
        ));
    }
    let unmatched = |from: &Form, to: &Form| -> Vec<(&'static str, Vec<f64>)> {
        from.radii
            .iter()
            .filter(|(kind, radii)| {
                !to.radii.iter().any(|(k, r)| {
                    k == kind
                        && r.len() == radii.len()
                        && r.iter().zip(radii).all(|(p, q)| close(p, q))
                })
            })
            .cloned()
            .collect()
    };
    let (only_a, only_b) = (unmatched(a, b), unmatched(b, a));
    if !only_a.is_empty() || !only_b.is_empty() {
        return Err(format!(
            "radii without a match within {tolerance} mm: first {only_a:?}, second {only_b:?}"
        ));
    }
    let (coarse, fine) = if a.corners.len() <= b.corners.len() {
        (a, b)
    } else {
        (b, a)
    };
    let low = |points: &[[f64; 3]]| {
        points.iter().fold([f64::INFINITY; 3], |m, p| {
            [m[0].min(p[0]), m[1].min(p[1]), m[2].min(p[2])]
        })
    };
    let fine_low = low(&fine.vertices);
    let mut fewest = usize::MAX;
    for permutation in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        for signs in 0..8_u32 {
            let map = |p: &[f64; 3]| -> [f64; 3] {
                let mut q = [0.0; 3];
                for (axis, &from) in permutation.iter().enumerate() {
                    let sign = if signs & (1 << axis) == 0 { 1.0 } else { -1.0 };
                    q[axis] = sign * p[from];
                }
                q
            };
            let mapped: Vec<[f64; 3]> = coarse.vertices.iter().map(map).collect();
            let coarse_low = low(&mapped);
            let shift = [
                fine_low[0] - coarse_low[0],
                fine_low[1] - coarse_low[1],
                fine_low[2] - coarse_low[2],
            ];
            let missing = coarse
                .corners
                .iter()
                .map(map)
                .filter(|p| {
                    !fine.vertices.iter().any(|q| {
                        (0..3).all(|axis| (p[axis] + shift[axis] - q[axis]).abs() <= tolerance)
                    })
                })
                .count();
            if missing == 0 {
                return Ok(());
            }
            fewest = fewest.min(missing);
        }
    }
    Err(format!(
        "{fewest} of {} corners have no vertex within {tolerance} mm in the other export, \
         however the part is turned",
        coarse.corners.len()
    ))
}

/// mm per unit of the length unit of `file`'s geometry: the `LENGTH_UNIT` of its
/// `GLOBAL_UNIT_ASSIGNED_CONTEXT`s, which must all agree.
fn file_length_mm(file: &StepFile) -> Result<f64, String> {
    let mut found: Option<f64> = None;
    for instance in file.entities.values() {
        let Some(context) = instance.record("GLOBAL_UNIT_ASSIGNED_CONTEXT") else {
            continue;
        };
        let units = context
            .params
            .first()
            .and_then(Param::as_list)
            .unwrap_or_default();
        for unit in units.iter().filter_map(Param::as_ref_id) {
            if file
                .entities
                .get(&unit)
                .is_some_and(|u| u.record("LENGTH_UNIT").is_some())
            {
                let mm = length_mm(file, unit, 0)?;
                if found.is_some_and(|f| f != mm) {
                    return Err(format!("length units disagree: {found:?} and {mm} mm"));
                }
                found = Some(mm);
            }
        }
    }
    found.ok_or_else(|| "no length unit".into())
}

/// mm³ per unit of the volume unit `#id`.
fn volume_mm3(file: &StepFile, id: u64) -> Result<f64, String> {
    let instance = file
        .entities
        .get(&id)
        .ok_or_else(|| format!("unit #{id} missing"))?;
    let derived = instance
        .record("DERIVED_UNIT")
        .or_else(|| instance.record("VOLUME_UNIT"))
        .ok_or_else(|| {
            format!(
                "unit #{id} ({}) is not a derived unit",
                instance.type_name()
            )
        })?;
    let elements = derived
        .params
        .first()
        .and_then(Param::as_list)
        .ok_or_else(|| format!("unit #{id}: no elements"))?;
    let mut factor = 1.0;
    let mut dimension = 0.0;
    for element in elements.iter().filter_map(Param::as_ref_id) {
        let element = record(file, element, "DERIVED_UNIT_ELEMENT")
            .ok_or_else(|| format!("unit #{id}: element is not a DERIVED_UNIT_ELEMENT"))?;
        let unit = ref_at(element, 0).ok_or_else(|| format!("unit #{id}: element unit"))?;
        let exponent = element
            .params
            .get(1)
            .and_then(Param::as_f64)
            .ok_or_else(|| format!("unit #{id}: element exponent"))?;
        factor *= length_mm(file, unit, 0)?.powf(exponent);
        dimension += exponent;
    }
    if dimension != 3.0 {
        return Err(format!(
            "unit #{id} has length dimension {dimension}, not 3"
        ));
    }
    Ok(factor)
}

/// mm per unit of the length unit `#id`: an `SI_UNIT` of metres with its prefix, or a
/// `CONVERSION_BASED_UNIT` whose `MEASURE_WITH_UNIT` gives it in another length unit.
fn length_mm(file: &StepFile, id: u64, depth: usize) -> Result<f64, String> {
    if depth > 8 {
        return Err(format!("length unit #{id}: conversion chain too deep"));
    }
    let instance = file
        .entities
        .get(&id)
        .ok_or_else(|| format!("length unit #{id} missing"))?;
    if instance.record("LENGTH_UNIT").is_none() {
        return Err(format!(
            "unit #{id} ({}) is not a length unit",
            instance.type_name()
        ));
    }
    if let Some(si) = instance.record("SI_UNIT") {
        if si.params.get(1).and_then(Param::as_enum) != Some("METRE") {
            return Err(format!("length unit #{id}: SI unit is not METRE"));
        }
        let prefix = match si.params.first() {
            Some(Param::Null) | None => 1.0,
            Some(prefix) => match prefix.as_enum() {
                Some("MILLI") => 1e-3,
                Some("CENTI") => 1e-2,
                Some("DECI") => 1e-1,
                Some("KILO") => 1e3,
                Some("MICRO") => 1e-6,
                Some("NANO") => 1e-9,
                other => return Err(format!("length unit #{id}: prefix {other:?}")),
            },
        };
        return Ok(prefix * 1000.0);
    }
    if let Some(conversion) = instance.record("CONVERSION_BASED_UNIT") {
        let measure = ref_at(conversion, 1)
            .and_then(|m| file.entities.get(&m))
            .ok_or_else(|| format!("length unit #{id}: no conversion factor"))?;
        let measure = measure
            .record("LENGTH_MEASURE_WITH_UNIT")
            .or_else(|| measure.record("MEASURE_WITH_UNIT"))
            .ok_or_else(|| format!("length unit #{id}: conversion is not a measure"))?;
        let value = measure
            .params
            .first()
            .and_then(Param::as_f64)
            .ok_or_else(|| format!("length unit #{id}: conversion value"))?;
        let unit = ref_at(measure, 1).ok_or_else(|| format!("length unit #{id}: base unit"))?;
        return Ok(value * length_mm(file, unit, depth + 1)?);
    }
    Err(format!(
        "length unit #{id} ({}) is neither SI nor conversion based",
        instance.type_name()
    ))
}

/// The record `name` of the instance `#id`.
fn record<'a>(file: &'a StepFile, id: u64, name: &str) -> Option<&'a Record> {
    file.entities.get(&id).and_then(|i| i.record(name))
}

/// The reference at parameter `index` of `record`.
fn ref_at(record: &Record, index: usize) -> Option<u64> {
    record.params.get(index).and_then(Param::as_ref_id)
}

/// The string at parameter `index` of `record`.
fn str_at(record: &Record, index: usize) -> Option<&str> {
    match record.params.get(index) {
        Some(Param::Str(s)) => Some(s),
        _ => None,
    }
}

/// `|a - b| / max(|a|, |b|)`, and infinite when either value is not finite, so a NaN or
/// infinite volume fails every `deviation <= tolerance` check instead of slipping past a
/// `deviation > tolerance` one (`NaN > t` is false).
pub fn relative(a: f64, b: f64) -> f64 {
    if !a.is_finite() || !b.is_finite() {
        return f64::INFINITY;
    }
    (a - b).abs() / a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}
