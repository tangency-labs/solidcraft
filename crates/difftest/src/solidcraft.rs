//! `--solidcraft-cases`: whole-operation regression cases from SolidCraft, scored
//! against analytic expected values.
//!
//! Each file in `fixtures/solidcraft/` holds the cases of one SolidCraft issue that was
//! found and fixed on SolidCraft's earlier kernel: the issue, the fixing PR and its test,
//! and per case a list of `SM_API`-level steps (primitives, booleans, fillet, offset)
//! whose last body, `result`, is checked against values known in closed form (volume,
//! lump count, face bounds). They keep those bugs fixed once the operations run end to
//! end on solidcore.
//!
//! Like `--ssi-corpus`, this is a correctness report, not the gate: it never changes a
//! unit's difftest status and verdicts never fail the run. Only the Rust engine runs for
//! now (the C++ engine needs its own shim). A step that reaches unported code stops the
//! case as `DEFERRED` on the unit returned in [`SolidError::Unported`]; a step whose
//! `SM_API` entry point has no Rust function yet is deferred on the unit that owns that
//! entry point (`harness/callgraph/units.json`), and one with no `SM_API` entry point at
//! all says so. Nothing is skipped or faked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{json, Value};

use solidcore::boolean::api_brep as boolean;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives as primitives;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::Brep;
use solidcore::SolidError;

use crate::ssi::glob_match;
use crate::HarnessError;

/// Accuracy passed to `SmApiBrepComputeVolume`.
pub const VOLUME_ACCURACY: f64 = 1e-6;

/// The body a case's expectations are checked on.
pub const RESULT: &str = "result";

/// Verdicts, in summary order.
pub const VERDICTS: [&str; 5] = ["PASS", "FAIL", "UNCHECKED", "ERROR", "DEFERRED"];

type P3 = [f64; 3];

/// One issue's file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseFile {
    /// The SolidCraft issue.
    pub issue: String,
    /// The SolidCraft PR that fixed it.
    pub pr: String,
    /// That PR's test the cases come from.
    pub test: String,
    /// What broke, where the geometry and values come from.
    pub summary: String,
    /// The cases.
    pub cases: Vec<Case>,
}

/// One case: steps building `result`, and what `result` must be.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Unique within its file.
    pub name: String,
    /// What this variant is.
    #[serde(default)]
    pub note: Option<String>,
    /// Run in order; each writes the body `id`.
    pub steps: Vec<Step>,
    /// Checked on the body `result`.
    pub expect: Expect,
}

/// One operation (an `SM_API` call). `id` is the body it writes (replacing any body of
/// that name); `a`, `b` and `body` name bodies written by earlier steps.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// `SmApiCreateBox`.
    Box {
        /// Body written.
        id: String,
        /// Lower corner.
        origin: P3,
        /// Size along X, Y, Z.
        size: P3,
    },
    /// `SmApiCreateSphere`.
    Sphere {
        /// Body written.
        id: String,
        /// Centre.
        center: P3,
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateCylinder`, built along Z and, for another `axis`, turned onto it
    /// with `SmApiRotate` about `origin`.
    Cylinder {
        /// Body written.
        id: String,
        /// Centre of the base.
        origin: P3,
        /// Axis direction (default +Z).
        #[serde(default = "z_axis")]
        axis: P3,
        /// Radius.
        radius: f64,
        /// Length along the axis.
        height: f64,
    },
    /// `SmApiBooleanUnion(a, b)`.
    Union {
        /// Body written.
        id: String,
        /// Target.
        a: String,
        /// Tool.
        b: String,
    },
    /// `SmApiBooleanDifference(a, b)`.
    Difference {
        /// Body written.
        id: String,
        /// Target.
        a: String,
        /// Tool.
        b: String,
        /// When given, `b` is cut once per position (moved with `SmApiTranslate`), one
        /// cut after the other.
        #[serde(default)]
        pattern: Option<Pattern>,
    },
    /// `SmApiBooleanIntersection(a, b)`.
    Intersection {
        /// Body written.
        id: String,
        /// Target.
        a: String,
        /// Tool.
        b: String,
    },
    /// `SmApiFilletEdges`, circular section.
    Fillet {
        /// Body written.
        id: String,
        /// Body filleted.
        body: String,
        /// A point on each edge to fillet.
        edges: Vec<P3>,
        /// Radius.
        radius: f64,
    },
    /// Offset selected faces along their outward normals, the rest of the body
    /// following (SolidCraft's Offset Face).
    OffsetFaces {
        /// Body written.
        id: String,
        /// Body offset.
        body: String,
        /// A point on each face to offset.
        faces: Vec<P3>,
        /// Offset distance (negative: inward).
        distance: f64,
    },
}

/// A grid of positions: `pitch[0] * i + pitch[1] * j` for `i < count[0]`,
/// `j < count[1]`, `i` varying fastest.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// Positions along each pitch vector.
    pub count: [usize; 2],
    /// The two pitch vectors.
    pub pitch: [P3; 2],
}

/// What `result` must be. Absent fields are not checked.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Bounded regions (`0`: the operation leaves nothing).
    #[serde(default)]
    pub lumps: Option<usize>,
    /// Enclosed volume.
    #[serde(default)]
    pub volume: Option<VolumeExpect>,
    /// At most this many faces on cylinders.
    #[serde(default)]
    pub cylinder_faces_max: Option<usize>,
}

/// An analytic volume.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeExpect {
    /// mm^3.
    pub value: f64,
    /// Absolute tolerance, mm^3.
    pub tol: f64,
    /// How `value` was derived.
    pub formula: String,
}

fn z_axis() -> P3 {
    [0.0, 0.0, 1.0]
}

/// A loaded case: `<file stem>/<name>` and its file.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// `<file stem>/<name>`.
    pub id: String,
    /// The file's fields (shared by its cases).
    pub file: CaseFile,
    /// The case.
    pub case: Case,
}

/// The case directory.
pub fn cases_dir(root: &Path) -> PathBuf {
    root.join("fixtures").join("solidcraft")
}

/// Every case in `dir` whose id matches `glob`, sorted by file then file order, after
/// checking each case ([`validate`]).
///
/// # Errors
///
/// An unreadable or invalid file, or duplicate case names in one file.
pub fn load(dir: &Path, glob: Option<&str>) -> Result<Vec<Loaded>, HarnessError> {
    let rd =
        std::fs::read_dir(dir).map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        let file: CaseFile = serde_json::from_str(&text)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        let mut names = std::collections::BTreeSet::new();
        for case in &file.cases {
            if !names.insert(case.name.as_str()) {
                return Err(HarnessError::new(format!(
                    "{}: duplicate case `{}`",
                    path.display(),
                    case.name
                )));
            }
            validate(case).map_err(|e| {
                HarnessError::new(format!("{}: {}: {e}", path.display(), case.name))
            })?;
        }
        for case in &file.cases {
            let id = format!("{stem}/{}", case.name);
            if glob.is_none_or(|g| glob_match(g, &id)) {
                out.push(Loaded {
                    id,
                    file: file.clone(),
                    case: case.clone(),
                });
            }
        }
    }
    Ok(out)
}

impl Step {
    /// The body this step writes.
    pub fn id(&self) -> &str {
        match self {
            Self::Box { id, .. }
            | Self::Sphere { id, .. }
            | Self::Cylinder { id, .. }
            | Self::Union { id, .. }
            | Self::Difference { id, .. }
            | Self::Intersection { id, .. }
            | Self::Fillet { id, .. }
            | Self::OffsetFaces { id, .. } => id,
        }
    }

    /// The bodies this step reads.
    pub fn inputs(&self) -> Vec<&str> {
        match self {
            Self::Box { .. } | Self::Sphere { .. } | Self::Cylinder { .. } => Vec::new(),
            Self::Union { a, b, .. }
            | Self::Difference { a, b, .. }
            | Self::Intersection { a, b, .. } => vec![a, b],
            Self::Fillet { body, .. } | Self::OffsetFaces { body, .. } => vec![body],
        }
    }
}

/// Every step reads only bodies written before it, the last step writes [`RESULT`],
/// and every number is finite.
///
/// # Errors
///
/// What is wrong, as text.
pub fn validate(case: &Case) -> Result<(), String> {
    let mut defined = std::collections::BTreeSet::new();
    for (i, step) in case.steps.iter().enumerate() {
        for input in step.inputs() {
            if !defined.contains(input) {
                return Err(format!("step {i} reads `{input}` before it is written"));
            }
        }
        let finite = match step {
            Step::Box { origin, size, .. } => origin.iter().chain(size).all(|x| x.is_finite()),
            Step::Sphere { center, radius, .. } => {
                center.iter().all(|x| x.is_finite()) && radius.is_finite()
            }
            Step::Cylinder {
                origin,
                axis,
                radius,
                height,
                ..
            } => {
                origin.iter().chain(axis).all(|x| x.is_finite())
                    && radius.is_finite()
                    && height.is_finite()
            }
            Step::Difference {
                pattern: Some(p), ..
            } => p.pitch.iter().flatten().all(|x| x.is_finite()),
            Step::Fillet { edges, radius, .. } => {
                edges.iter().flatten().all(|x| x.is_finite()) && radius.is_finite()
            }
            Step::OffsetFaces {
                faces, distance, ..
            } => faces.iter().flatten().all(|x| x.is_finite()) && distance.is_finite(),
            Step::Union { .. } | Step::Difference { .. } | Step::Intersection { .. } => true,
        };
        if !finite {
            return Err(format!("step {i} has a non-finite number"));
        }
        defined.insert(step.id());
    }
    if case.steps.last().map(Step::id) != Some(RESULT) {
        return Err(format!("the last step must write `{RESULT}`"));
    }
    if let Some(v) = &case.expect.volume {
        if !(v.value.is_finite() && v.tol.is_finite() && v.tol > 0.0) {
            return Err("volume value and tol must be finite, tol positive".into());
        }
    }
    Ok(())
}

// ---- the Rust engine -----------------------------------------------------------------

/// What blocks a deferred case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deferral {
    /// The plan unit that will port the missing code, or `no SM_API entry point`.
    pub blocked_by: String,
    /// The upstream symbol (or what is missing).
    pub symbol: String,
}

/// What `result` measured as.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// `SmApiBrepComputeVolume`.
    pub volume: f64,
    /// Regions other than the infinite one.
    pub lumps: usize,
}

/// How a case's run ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Run {
    /// Every step ran; `result` measured.
    Done(Measured),
    /// A step reached code that is not ported yet.
    Deferred(Deferral),
    /// A step returned an error other than `Unported`.
    Error(String),
}

fn p3(p: P3) -> Point3 {
    let [x, y, z] = p;
    Vec3::new(x, y, z)
}

fn solid(e: &SolidError) -> Run {
    match e {
        SolidError::Unported { upstream, unit } => Run::Deferred(Deferral {
            blocked_by: (*unit).to_string(),
            symbol: (*upstream).to_string(),
        }),
        _ => Run::Error(e.to_string()),
    }
}

fn take(bodies: &BTreeMap<String, Brep>, id: &str) -> Result<Brep, Run> {
    bodies
        .get(id)
        .cloned()
        .ok_or_else(|| Run::Error(format!("no body `{id}`")))
}

/// A cylinder along Z turned onto `axis` about `origin`.
fn cylinder(k: &mut Kernel, origin: P3, axis: P3, radius: f64, height: f64) -> Result<Brep, Run> {
    let mut b = primitives::create_cylinder(p3(origin), radius, height).map_err(|e| solid(&e))?;
    let a = p3(axis).unitize().map_err(|e| solid(&e))?;
    let z = Vec3::new(0.0, 0.0, 1.0);
    let cos = a.dot(z).clamp(-1.0, 1.0);
    if cos < 1.0 {
        let turn = if cos > -1.0 {
            z.cross(a)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut b)),
            p3(origin),
            turn,
            cos.acos().to_degrees(),
        )
        .map_err(|e| solid(&e))?;
    }
    Ok(b)
}

fn step_rust(k: &mut Kernel, bodies: &mut BTreeMap<String, Brep>, step: &Step) -> Result<(), Run> {
    let body = match step {
        Step::Box { origin, size, .. } => {
            let [x, y, z] = *size;
            primitives::create_box(p3(*origin), x, y, z).map_err(|e| solid(&e))?
        }
        Step::Sphere { center, radius, .. } => {
            primitives::create_sphere(p3(*center), *radius).map_err(|e| solid(&e))?
        }
        Step::Cylinder {
            origin,
            axis,
            radius,
            height,
            ..
        } => cylinder(k, *origin, *axis, *radius, *height)?,
        Step::Union { a, b, .. } => {
            boolean::boolean_union(k, take(bodies, a)?, take(bodies, b)?).map_err(|e| solid(&e))?
        }
        Step::Intersection { a, b, .. } => {
            boolean::boolean_intersection(k, take(bodies, a)?, take(bodies, b)?)
                .map_err(|e| solid(&e))?
        }
        Step::Difference {
            a,
            b,
            pattern: None,
            ..
        } => boolean::boolean_difference(k, take(bodies, a)?, take(bodies, b)?)
            .map_err(|e| solid(&e))?,
        Step::Difference {
            a,
            b,
            pattern: Some(p),
            ..
        } => {
            let mut target = take(bodies, a)?;
            let [p0, p1] = p.pitch;
            for j in 0..p.count[1] {
                for i in 0..p.count[0] {
                    let mut tool = take(bodies, b)?;
                    let (fi, fj) = (i as f64, j as f64);
                    let shift = Vec3::new(
                        p0[0] * fi + p1[0] * fj,
                        p0[1] * fi + p1[1] * fj,
                        p0[2] * fi + p1[2] * fj,
                    );
                    api_translate(k, Some(TransformObject::Brep(&mut tool)), shift)
                        .map_err(|e| solid(&e))?;
                    target = boolean::boolean_difference(k, target, tool).map_err(|e| solid(&e))?;
                }
            }
            target
        }
        // No Rust `SmApiFilletEdges` yet: it belongs to `blend.fillet_executive_p1`.
        Step::Fillet { .. } => {
            return Err(Run::Deferred(Deferral {
                blocked_by: "blend.fillet_executive_p1".into(),
                symbol: "SmApiFilletEdges".into(),
            }))
        }
        // SM_API offsets or shells whole bodies only (SmApiOffsetBrepFull,
        // SmApiShellBrepFull); nothing offsets selected faces.
        Step::OffsetFaces { .. } => {
            return Err(Run::Deferred(Deferral {
                blocked_by: "no SM_API entry point".into(),
                symbol: "offset of selected faces (SmApiOffsetBrepFull moves every face)".into(),
            }))
        }
    };
    bodies.insert(step.id().to_string(), body);
    Ok(())
}

fn measure(k: &Kernel, b: &Brep) -> Result<Measured, SolidError> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(k, b, VOLUME_ACCURACY, &attrs)?;
    let infinite = b.infinite_region();
    let lumps = regions(b, None)?
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count();
    Ok(Measured { volume, lumps })
}

/// Runs `case` on solidcore.
pub fn run_rust(case: &Case) -> Run {
    let mut k = Kernel::default();
    let mut bodies = BTreeMap::new();
    for step in &case.steps {
        if let Err(stop) = step_rust(&mut k, &mut bodies, step) {
            return stop;
        }
    }
    match bodies.get(RESULT) {
        Some(b) => measure(&k, b).map_or_else(|e| solid(&e), Run::Done),
        None => Run::Error(format!("no body `{RESULT}`")),
    }
}

// ---- scoring --------------------------------------------------------------------------

/// One expectation checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// `lumps`, `volume`, `cylinder_faces_max`.
    pub name: &'static str,
    /// `pass`, `fail` or `unchecked`.
    pub status: &'static str,
    /// Expected and measured, as text.
    pub detail: String,
}

/// Checks `m` against `e`.
pub fn checks(e: &Expect, m: &Measured) -> Vec<Check> {
    let mut out = Vec::new();
    let pass = |ok: bool| if ok { "pass" } else { "fail" };
    if let Some(n) = e.lumps {
        out.push(Check {
            name: "lumps",
            status: pass(m.lumps == n),
            detail: format!("expected {n}, got {}", m.lumps),
        });
    }
    if let Some(v) = &e.volume {
        let err = (m.volume - v.value).abs();
        out.push(Check {
            name: "volume",
            // A NaN volume fails: `err <= tol` is false.
            status: pass(err <= v.tol),
            detail: format!(
                "expected {} +- {}, got {} (off by {err})",
                v.value, v.tol, m.volume
            ),
        });
    }
    if let Some(n) = e.cylinder_faces_max {
        out.push(Check {
            name: "cylinder_faces_max",
            status: "unchecked",
            detail: format!("at most {n}; solidcore has no cylinder surface kind yet"),
        });
    }
    out
}

/// A run's verdict and its checks.
pub fn verdict(e: &Expect, run: &Run) -> (&'static str, Vec<Check>) {
    match run {
        Run::Deferred(_) => ("DEFERRED", Vec::new()),
        Run::Error(_) => ("ERROR", Vec::new()),
        Run::Done(m) => {
            let c = checks(e, m);
            let v = if c.iter().any(|c| c.status == "fail") {
                "FAIL"
            } else if c.iter().any(|c| c.status == "unchecked") {
                "UNCHECKED"
            } else {
                "PASS"
            };
            (v, c)
        }
    }
}

/// Options of a `--solidcraft-cases` run.
#[derive(Debug, Clone, Default)]
pub struct CasesOptions {
    /// Case id glob (`<file stem>/<name>`; `*` and `?`).
    pub glob: Option<String>,
    /// Report directory (default `reports/`).
    pub report_dir: Option<PathBuf>,
    /// Do not write the report.
    pub no_report: bool,
}

fn run_entry(run: &Run, verdict: &str, checks: &[Check]) -> Value {
    let mut e = serde_json::Map::new();
    e.insert("verdict".into(), json!(verdict));
    match run {
        Run::Deferred(d) => {
            e.insert(
                "deferred".into(),
                json!({"blocked_by": d.blocked_by, "symbol": d.symbol}),
            );
        }
        Run::Error(msg) => {
            e.insert("error".into(), json!(msg));
        }
        Run::Done(m) => {
            e.insert(
                "measured".into(),
                json!({"volume": crate::json::num(m.volume), "lumps": m.lumps}),
            );
            e.insert(
                "checks".into(),
                Value::Array(
                    checks
                        .iter()
                        .map(|c| json!({"name": c.name, "status": c.status, "detail": c.detail}))
                        .collect(),
                ),
            );
        }
    }
    Value::Object(e)
}

/// Runs every case matching `opts.glob`, prints one line per case and the summary, and
/// writes `reports/solidcraft-cases.json` (`reports/solidcraft-cases_<glob>.json` for
/// a glob). Exit `0` whatever the verdicts; `2` for an unreadable case file or report.
pub fn run_cases(root: &Path, opts: &CasesOptions) -> ExitCode {
    let cases = match load(&cases_dir(root), opts.glob.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };
    if cases.is_empty() {
        println!(
            "SOLIDCRAFT-CASES: no case matches `{}`",
            opts.glob.as_deref().unwrap_or("*")
        );
        return ExitCode::SUCCESS;
    }
    println!(
        "SOLIDCRAFT-CASES: {} case(s), Rust engine only",
        cases.len()
    );
    let mut counts: BTreeMap<&str, usize> = VERDICTS.iter().map(|v| (*v, 0)).collect();
    let mut blocked: BTreeMap<String, usize> = BTreeMap::new();
    let mut entries = Vec::new();
    for l in &cases {
        let run = run_rust(&l.case);
        let (v, c) = verdict(&l.case.expect, &run);
        *counts.entry(v).or_default() += 1;
        let detail = match &run {
            Run::Deferred(d) => {
                *blocked.entry(d.blocked_by.clone()).or_default() += 1;
                format!("{}: {}", d.blocked_by, d.symbol)
            }
            Run::Error(msg) => msg.clone(),
            Run::Done(_) => c
                .iter()
                .map(|c| format!("{} {}: {}", c.name, c.status, c.detail))
                .collect::<Vec<_>>()
                .join("; "),
        };
        println!("{:<64} rust {v:<10} {detail}", l.id);
        entries.push(json!({
            "id": l.id,
            "issue": l.file.issue,
            "pr": l.file.pr,
            "test": l.file.test,
            "rust": run_entry(&run, v, &c),
        }));
    }
    let summary = VERDICTS
        .iter()
        .map(|v| format!("{v}={}", counts.get(v).copied().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(" ");
    println!("SOLIDCRAFT-CASES rust: {summary} (cases={})", cases.len());
    if !blocked.is_empty() {
        let by = blocked
            .iter()
            .map(|(u, n)| format!("{u}({n})"))
            .collect::<Vec<_>>()
            .join(",");
        println!("SOLIDCRAFT-CASES rust: blocked_by={by}");
    }

    if !opts.no_report {
        let report = json!({
            "glob": opts.glob,
            "volume_accuracy": VOLUME_ACCURACY,
            "summary": {"rust": counts, "blocked_by": blocked},
            "cases": entries,
        });
        let dir = opts
            .report_dir
            .clone()
            .unwrap_or_else(|| root.join("reports"));
        let mut name = "solidcraft-cases".to_string();
        if let Some(g) = &opts.glob {
            name.push('_');
            name.extend(g.chars().filter(|c| c.is_alphanumeric() || *c == '_'));
        }
        let path = dir.join(format!("{name}.json"));
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, crate::json::to_canonical_string(&report)));
        match written {
            Ok(()) => println!(
                "  report: {}",
                path.strip_prefix(root).unwrap_or(&path).display()
            ),
            Err(e) => {
                eprintln!("difftest: {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Loaded> {
        load(&cases_dir(&crate::case::repo_root()), None).unwrap()
    }

    #[test]
    fn every_case_file_loads() {
        let cases = all();
        assert_eq!(
            cases.len(),
            15,
            "{:?}",
            cases.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        for l in &cases {
            assert!(
                l.file.issue.contains("/issues/") && l.file.pr.contains("/pull/"),
                "{}",
                l.id
            );
            let e = &l.case.expect;
            assert!(
                e.lumps.is_some() || e.volume.is_some(),
                "{}: nothing to check",
                l.id
            );
        }
    }

    #[test]
    fn every_case_reaches_a_verdict_without_a_harness_error() {
        for l in all() {
            let run = run_rust(&l.case);
            assert!(
                !matches!(run, Run::Error(ref m) if m.starts_with("no body")),
                "{}: {run:?}",
                l.id
            );
            let (v, _) = verdict(&l.case.expect, &run);
            assert!(VERDICTS.contains(&v), "{}", l.id);
        }
    }

    #[test]
    fn deferrals_name_a_plan_unit() {
        let units: Vec<String> = crate::plan::load(&crate::case::repo_root())
            .unwrap()
            .into_iter()
            .map(|u| u.id)
            .collect();
        for l in all() {
            if let Run::Deferred(d) = run_rust(&l.case) {
                assert!(
                    d.blocked_by == "no SM_API entry point" || units.contains(&d.blocked_by),
                    "{}: {d:?}",
                    l.id
                );
            }
        }
    }

    #[test]
    fn scoring() {
        let e = Expect {
            lumps: Some(2),
            volume: Some(VolumeExpect {
                value: 268.08,
                tol: 0.27,
                formula: String::new(),
            }),
            cylinder_faces_max: None,
        };
        let ok = Measured {
            volume: 268.02,
            lumps: 2,
        };
        assert_eq!(verdict(&e, &Run::Done(ok)).0, "PASS");
        // The bug: the whole intersect came out empty.
        let empty = Measured {
            volume: 0.0,
            lumps: 0,
        };
        let (v, c) = verdict(&e, &Run::Done(empty));
        assert_eq!(v, "FAIL");
        assert!(c.iter().all(|c| c.status == "fail"));
        let nan = Measured {
            volume: f64::NAN,
            lumps: 2,
        };
        assert_eq!(verdict(&e, &Run::Done(nan)).0, "FAIL");
        let faces = Expect {
            cylinder_faces_max: Some(4),
            ..e.clone()
        };
        assert_eq!(verdict(&faces, &Run::Done(ok)).0, "UNCHECKED");
        assert_eq!(verdict(&faces, &Run::Done(empty)).0, "FAIL");
        let d = Run::Deferred(Deferral {
            blocked_by: "u".into(),
            symbol: "s".into(),
        });
        assert_eq!(verdict(&e, &d).0, "DEFERRED");
        assert_eq!(verdict(&e, &Run::Error("x".into())).0, "ERROR");
    }

    #[test]
    fn validation() {
        let step = |json: &str| serde_json::from_str::<Step>(json).unwrap();
        let case = |steps: Vec<Step>| Case {
            name: "c".into(),
            note: None,
            steps,
            expect: Expect::default(),
        };
        let bx = r#"{"op": "box", "id": "result", "origin": [0, 0, 0], "size": [1, 1, 1]}"#;
        assert!(validate(&case(vec![step(bx)])).is_ok());
        let late = r#"{"op": "union", "id": "result", "a": "x", "b": "x"}"#;
        assert!(validate(&case(vec![step(late)])).is_err());
        let other = r#"{"op": "box", "id": "other", "origin": [0, 0, 0], "size": [1, 1, 1]}"#;
        assert!(validate(&case(vec![step(other)])).is_err());
        assert!(serde_json::from_str::<Step>(
            r#"{"op": "box", "id": "a", "origin": [0, 0, 0], "size": [1, 1, 1], "typo": 1}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Step>(r#"{"op": "chamfer", "id": "a"}"#).is_err());
    }
}
