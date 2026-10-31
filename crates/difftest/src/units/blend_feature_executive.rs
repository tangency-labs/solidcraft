//! Differential coverage for `blend.feature_executive` (`SmFeatureExecutive`).
//!
//! The fixtures are upstream `.smb` solids, read by the native `SmBrep::ReadFromFile` and
//! by the Rust reader (no C++ state reaches the Rust side). A case is a script: features
//! added by faces (`AddFeature(sFaces, ..)`), by a face and loop edges
//! (`AddFeature(pFace, rLoopEdges, ..)`) or by a face and caller-made loops
//! (`AddFeature(pFace, rFLoops)`), given as indices of the fixture's `GetFaces` /
//! `GetEdges`, then `DoDefeature`, the executive's rebuild-type and extension-distance
//! setters and `RestoreFeatures`, run on the native executive and on
//! `blend::feature_executive::FeatureExecutive`. A case compares a record (which
//! features were made and their loops' sorted edges, each op's status, then every
//! feature's state flags, face and loop counts and per loop its rebuild types,
//! distances, edge counts, patches and flags), and for the base B-rep and every feature
//! B-rep the whole graph (topology, vertex points, tolerances, intervals, UV domains)
//! and samples of every edge curve and face surface. Inputs the native reader cannot
//! build, indices past the fixture and a `RestoreFeatures` upstream would run into a
//! deleted feature (two or more restorable features) are `rejected`, not failed.
//!
//! The raw graph the native side exports carries B-spline surfaces and planes only; a
//! B-rep with any other face surface compares its geometry record alone, on both sides.

use serde::{Deserialize, Serialize};
use solidcore::blend::feature_executive::{FeatureExecutive, FeatureLoop, RebuildBrepType};
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::{Brep, EdgeId, FaceId};
use solidcore::Result as SolidResult;
use solidcore_ffi::units::blend_feature_executive as ffi;
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::graph;
use super::topology_brep_mirror::{geometry, herr, normalize_face_shells, status};
use super::topology_vertexuse::{import_raw_with_owners, path_of, BrepIn};
use crate::{Case, HarnessError, Output, UnitDiff};

/// The runner of `SmFeatureExecutive`.
pub struct Runner;

/// One `SmFeatureLoop` of an `AddFeature(pFace, rFLoops)` feature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct LoopSpec {
    base_rb: u8,
    feat_rb: u8,
    edges: Vec<u32>,
}

/// One `AddFeature` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct FeatureSpec {
    /// `faces` (by faces), `edges` (a face and loop edges) or `loops` (a face and loops).
    kind: String,
    base_rb: u8,
    feat_rb: u8,
    faces: Vec<u32>,
    face: u32,
    edges: Vec<u32>,
    loops: Vec<LoopSpec>,
}

impl Default for FeatureSpec {
    fn default() -> Self {
        Self {
            kind: "faces".into(),
            base_rb: 2,
            feat_rb: 3,
            faces: Vec::new(),
            face: 0,
            edges: Vec::new(),
            loops: Vec::new(),
        }
    }
}

/// An `SmLongAttribute(id, 7, behavior)` put on a face or edge of the fixture before the
/// features are added.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct AttrSpec {
    /// `face` or `edge`.
    kind: String,
    /// The index in the fixture's `GetFaces` / `GetEdges`.
    index: u32,
    /// The attribute id (above 10000).
    id: u32,
    /// `SmAttributeBehaviorType` (0 copy .. 4 temp).
    behavior: u8,
}

/// One op after the features.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpSpec {
    Defeature,
    SetBaseRb { t: u8 },
    SetFeatureRb { t: u8 },
    SetBaseExtDist { d: f64 },
    SetFeatureExtDist { d: f64 },
    Restore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Rebuild the B-rep's topology on reading (`ReadFromFile`'s flag).
    rebuild: bool,
    /// Attributes put on the fixture first.
    attributes: Vec<AttrSpec>,
    features: Vec<FeatureSpec>,
    ops: Vec<OpSpec>,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            file: "Defeature/defillet.smb".into(),
            rebuild: false,
            attributes: Vec::new(),
            features: Vec::new(),
            ops: vec![OpSpec::Defeature],
        }
    }
}

fn kind_code(kind: &str) -> Result<f64, HarnessError> {
    match kind {
        "faces" => Ok(0.),
        "edges" => Ok(1.),
        "loops" => Ok(2.),
        _ => Err(herr(format!("feature kind {kind}"))),
    }
}

/// The shim's script (layout in `solidcore_ffi::units::blend_feature_executive`).
fn script(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut s = vec![i.attributes.len() as f64];
    for a in &i.attributes {
        let kind = match a.kind.as_str() {
            "face" => 0.,
            "edge" => 1.,
            other => return Err(herr(format!("attribute kind {other}"))),
        };
        s.extend([kind, f64::from(a.index), f64::from(a.id), f64::from(a.behavior)]);
    }
    s.push(i.features.len() as f64);
    let list = |s: &mut Vec<f64>, v: &[u32]| {
        s.push(v.len() as f64);
        s.extend(v.iter().map(|&x| f64::from(x)));
    };
    for f in &i.features {
        s.extend([kind_code(&f.kind)?, f64::from(f.base_rb), f64::from(f.feat_rb)]);
        match f.kind.as_str() {
            "faces" => list(&mut s, &f.faces),
            "edges" => {
                s.push(f64::from(f.face));
                list(&mut s, &f.edges);
            }
            _ => {
                s.push(f64::from(f.face));
                s.push(f.loops.len() as f64);
                for l in &f.loops {
                    s.extend([f64::from(l.base_rb), f64::from(l.feat_rb)]);
                    list(&mut s, &l.edges);
                }
            }
        }
    }
    s.push(i.ops.len() as f64);
    for op in &i.ops {
        match op {
            OpSpec::Defeature => s.push(0.),
            OpSpec::SetBaseRb { t } => s.extend([1., f64::from(*t)]),
            OpSpec::SetFeatureRb { t } => s.extend([2., f64::from(*t)]),
            OpSpec::SetBaseExtDist { d } => s.extend([3., *d]),
            OpSpec::SetFeatureExtDist { d } => s.extend([4., *d]),
            OpSpec::Restore => s.push(5.),
        }
    }
    Ok(s)
}

/// The shared B-rep block of `ReadFromFile(file, rebuild)`; no validate-only, no gaps.
fn block(i: &Inputs) -> Vec<f64> {
    vec![0.0, f64::from(i.rebuild), 0.0, 0.0]
}

fn path(i: &Inputs) -> Result<String, HarnessError> {
    path_of(&BrepIn::File {
        file: i.file.clone(),
        rebuild: i.rebuild,
    })
}

/// The native import's refusal of a face surface that is neither a B-spline nor a plane.
const NOT_EXPORTABLE: &str = "a surface is not a B-spline or plane";

/// Whether every face surface of `b` is an `SmBSplineSurface` (or absent).
fn graph_exportable(b: &Brep) -> SolidResult<bool> {
    for f in bp1::faces(b, None)? {
        if let Some(s) = b.face(f)?.surface.as_ref() {
            if !s.is_kind_of(solidcore::nurbs::b_spline_surface::BSplineSurface::TYPE)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Drops the Rust-only logical lists (faces' `loops`, loops' `edges`) the native export
/// never has.
fn strip_unrecorded(b: &mut Brep) -> SolidResult<()> {
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        b.face_mut(id)?.loops.clear();
    }
    let loops: Vec<_> = b.loops().ids().collect();
    for id in loops {
        b.loop_mut(id)?.edges.clear();
    }
    Ok(())
}

/// Moves `o`'s values and ints into `out` under `prefix`.
fn merge(out: &mut Output, prefix: &str, o: Output) {
    for (k, v) in o.values {
        out.values.insert(format!("{prefix}{k}"), v);
    }
    for (k, v) in o.ints {
        out.ints.insert(format!("{prefix}{k}"), v);
    }
}

/// Reads one `put_brep` block from the native output at `*at`.
fn native_brep(raw: &[f64], at: &mut usize) -> Result<Output, HarnessError> {
    let take = |at: &mut usize| -> Result<Vec<f64>, HarnessError> {
        let n = *raw.get(*at).ok_or_else(|| herr("short brep"))? as usize;
        let v = raw
            .get(*at + 1..*at + 1 + n)
            .ok_or_else(|| herr("short brep"))?
            .to_vec();
        *at += 1 + n;
        Ok(v)
    };
    let geom = take(at)?;
    let g = take(at)?;
    match import_raw_with_owners(&g) {
        Ok((mut b, _)) => {
            strip_unrecorded(&mut b).map_err(herr)?;
            normalize_face_shells(&mut b).map_err(herr)?;
            Ok(graph(&b, geom))
        }
        Err(e) if e.to_string().contains(NOT_EXPORTABLE) => {
            Ok(Output::new().values("record", geom))
        }
        Err(e) => Err(e),
    }
}

/// The Rust counterpart of [`native_brep`].
fn rust_brep(k: &Kernel, b: &Brep) -> Result<Output, HarnessError> {
    let geom = geometry(k, b, false).map_err(herr)?;
    if !graph_exportable(b).map_err(herr)? {
        return Ok(Output::new().values("record", geom));
    }
    let mut b = b.clone();
    strip_unrecorded(&mut b).map_err(herr)?;
    normalize_face_shells(&mut b).map_err(herr)?;
    Ok(graph(&b, geom))
}

fn rb(t: u8) -> SolidResult<RebuildBrepType> {
    Ok(match t {
        0 => RebuildBrepType::None,
        1 => RebuildBrepType::PlanarCap,
        2 => RebuildBrepType::ExtendSurface,
        3 => RebuildBrepType::FromOtherBrep,
        _ => {
            return Err(solidcore::SolidError::invalid_input(
                "difftest::blend_feature_executive",
                "rebuild type",
            ))
        }
    })
}

/// The executive's state after the script, in the shim's `put_feature` layout.
fn put_features(ex: &FeatureExecutive<'_>, record: &mut Vec<f64>) -> SolidResult<()> {
    let flag = |b: bool| if b { 1. } else { 0. };
    let patch_faces = |p: Option<solidcore::blend::feature_executive::PatchId>| -> SolidResult<f64> {
        match p.and_then(|p| ex.patch(p)) {
            Some(b) => Ok(bp1::faces(b, None)?.len() as f64),
            None => Ok(-1.),
        }
    };
    record.push(ex.features().len() as f64);
    for f in ex.features() {
        record.extend([
            flag(f.is_partitioned),
            flag(f.defeature_ready),
            flag(f.base_rebuilt),
            flag(f.feat_rebuilt),
            flag(f.base_was_applied),
            flag(f.feat_was_applied),
            flag(f.can_restore),
            f.faces.len() as f64,
            flag(f.fbrep.is_some()),
            f.loops.len() as f64,
        ]);
        for l in &f.loops {
            record.extend([
                f64::from(l.base_rb_type as u8),
                f64::from(l.feat_rb_type as u8),
                l.base_ext_dist,
                l.feat_ext_dist,
                l.base_edges.len() as f64,
                l.feat_edges.len() as f64,
                flag(l.base_patch.is_some()),
                match l.feat_patch {
                    None => 0.,
                    Some(p) if Some(p) == l.base_patch => 2.,
                    Some(_) => 1.,
                },
                patch_faces(l.base_patch)?,
                patch_faces(l.feat_patch)?,
                flag(l.use_b_patch),
                flag(l.use_f_patch),
                flag(l.tried_ext_surf),
            ]);
        }
    }
    Ok(())
}

/// What the Rust side's script run gives: the record, or an index past the fixture.
enum RustRun {
    Record(Vec<f64>, Vec<Option<Brep>>),
    PastFixture,
}

/// The numbers of faces and edges that carry each spec's attribute id, of the base B-rep
/// and then of each feature B-rep (`-1 -1` for none).
fn put_attributes(
    ex: &FeatureExecutive<'_>,
    specs: &[AttrSpec],
    record: &mut Vec<f64>,
) -> SolidResult<()> {
    let counts = |b: &Brep, id: u64, record: &mut Vec<f64>| -> SolidResult<()> {
        let faces = bp1::faces(b, None)?;
        let edges = bp1::edges(b, None)?;
        let nf = faces
            .iter()
            .filter(|&&f| b.find_attribute(EntityRef::Face(f), id).is_some())
            .count();
        let ne = edges
            .iter()
            .filter(|&&e| b.find_attribute(EntityRef::Edge(e), id).is_some())
            .count();
        record.extend([nf as f64, ne as f64]);
        Ok(())
    };
    let fbs = ex.feature_breps();
    for a in specs {
        let id = u64::from(a.id);
        counts(ex.base_brep(), id, record)?;
        for fb in &fbs {
            match fb {
                Some(fb) => counts(fb, id, record)?,
                None => record.extend([-1., -1.]),
            }
        }
    }
    Ok(())
}

fn run_script(k: &mut Kernel, b: &mut Brep, i: &Inputs) -> SolidResult<RustRun> {
    let faces = bp1::faces(b, None)?;
    let edges = bp1::edges(b, None)?;
    let face_at = |x: u32| faces.get(x as usize).copied();
    let edge_list = |v: &[u32]| -> Option<Vec<EdgeId>> {
        v.iter().map(|&x| edges.get(x as usize).copied()).collect()
    };
    for a in &i.attributes {
        let e = match a.kind.as_str() {
            "face" => face_at(a.index).map(EntityRef::Face),
            _ => edges.get(a.index as usize).copied().map(EntityRef::Edge),
        };
        let Some(e) = e else {
            return Ok(RustRun::PastFixture);
        };
        let behavior = Behavior::from_code(i32::from(a.behavior))?;
        b.add_attribute(e, Attribute::long(u64::from(a.id), 7, behavior))?;
    }
    let mut record = Vec::new();
    let mut ex = FeatureExecutive::new(k, b)?;
    for f in &i.features {
        let made = match f.kind.as_str() {
            "faces" => {
                let Some(fs) = f
                    .faces
                    .iter()
                    .map(|&x| face_at(x))
                    .collect::<Option<Vec<FaceId>>>()
                else {
                    return Ok(RustRun::PastFixture);
                };
                ex.add_feature(k, &fs, rb(f.base_rb)?, rb(f.feat_rb)?)?
            }
            "edges" => {
                let Some(es) = edge_list(&f.edges) else {
                    return Ok(RustRun::PastFixture);
                };
                let Some(face) = face_at(f.face) else {
                    return Ok(RustRun::PastFixture);
                };
                ex.add_feature_with_edges(k, face, &es, rb(f.base_rb)?, rb(f.feat_rb)?)?
            }
            _ => {
                let mut loops = Vec::new();
                for l in &f.loops {
                    let Some(es) = edge_list(&l.edges) else {
                        return Ok(RustRun::PastFixture);
                    };
                    loops.push(FeatureLoop::new(es, rb(l.base_rb)?, rb(l.feat_rb)?));
                }
                let Some(face) = face_at(f.face) else {
                    return Ok(RustRun::PastFixture);
                };
                ex.add_feature_with_loops(k, face, &loops)?
            }
        };
        record.push(if made.is_some() { 1. } else { 0. });
        if let Some(fi) = made {
            let feature = ex
                .features()
                .get(fi)
                .ok_or_else(|| solidcore::SolidError::null_pointer("difftest"))?;
            record.push(feature.loops.len() as f64);
            for l in &feature.loops {
                record.push(l.base_edges.len() as f64);
                for e in &l.base_edges {
                    record.push(edges.iter().position(|x| x == e).map_or(-1., |p| p as f64));
                }
            }
        }
    }
    for op in &i.ops {
        match op {
            OpSpec::Defeature => {
                record.push(status(ex.do_defeature(k))?);
                record.push(1.);
                record.push(ex.features().len() as f64);
            }
            OpSpec::SetBaseRb { t } => ex.set_base_rb_type(rb(*t)?),
            OpSpec::SetFeatureRb { t } => ex.set_feature_rb_type(rb(*t)?),
            OpSpec::SetBaseExtDist { d } => ex.set_base_ext_dist(*d),
            OpSpec::SetFeatureExtDist { d } => ex.set_feature_ext_dist(*d),
            OpSpec::Restore => record.push(status(ex.restore_features(k))?),
        }
    }
    put_features(&ex, &mut record)?;
    put_attributes(&ex, &i.attributes, &mut record)?;
    Ok(RustRun::Record(record, ex.into_feature_breps()))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.feature_executive"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match ffi::eval(&path(&i)?, &block(&i), &script(&i)?) {
            Ok(raw) if raw.first() == Some(&-999.) => {
                return Ok(Output::rejected(format!(
                    "fixture constructor status {}",
                    raw.get(1).copied().unwrap_or(-1.)
                )))
            }
            Ok(raw) if raw.first() == Some(&-998.) => {
                return Ok(Output::rejected("index past the fixture's faces or edges"))
            }
            Ok(raw) if raw.first() == Some(&-997.) => {
                return Ok(Output::rejected(
                    "RestoreFeatures with two or more restorable features (upstream deletes \
                     a feature it keeps; upstream issue \
                     UB-277)",
                ))
            }
            Ok(raw) if raw.first() == Some(&-996.) => {
                return Ok(Output::rejected(
                    "DoDefeature with a loop edge an applied patch's stitch deleted (upstream \
                     reads it; UB-279)",
                ))
            }
            Ok(raw) if raw.first() == Some(&-995.) => {
                return Ok(Output::rejected(
                    "DoDefeature extends a surface without an extension (upstream \
                     dereferences the NULL surface; upstream issue \
                     UB-278)",
                ))
            }
            Err(FfiError::Status(solidcore_ffi::SC_ERR_CHILD_CRASHED)) => {
                return Ok(Output::rejected(
                    "the native script crashed (or ran past 60 s) in its fresh \
                     child process (UB-280, \
                     HANG-52)",
                ))
            }
            Ok(raw) => raw,
            Err(e @ FfiError::InvalidArgument(_)) => {
                return Ok(Output::rejected(format!("ffi guard: {e}")))
            }
            Err(e) => return Err(herr(e)),
        };
        let n = *raw.first().ok_or_else(|| herr("short record"))? as usize;
        let record = raw.get(1..=n).ok_or_else(|| herr("short record"))?.to_vec();
        let mut at = n + 1;
        let mut out = Output::new().values("record", record);
        merge(&mut out, "base_", native_brep(&raw, &mut at)?);
        let mut fi = 0;
        while at < raw.len() {
            let has = *raw.get(at).ok_or_else(|| herr("short feature"))?;
            at += 1;
            if has == 0. {
                out = out.int(&format!("feature{fi}_none"), 1);
            } else {
                merge(&mut out, &format!("feature{fi}_"), native_brep(&raw, &mut at)?);
            }
            fi += 1;
        }
        Ok(out)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        // the Rust reader's B-rep of the same file (no C++ export reaches the Rust side)
        let mut b = match crate::own_reader::read_file_rebuild(&mut k, &path(&i)?, i.rebuild)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust read: {e}")));
            }
        };
        match run_script(&mut k, &mut b, &i) {
            Ok(RustRun::PastFixture) => Ok(Output::rejected(
                "index past the fixture's faces or edges",
            )),
            Ok(RustRun::Record(record, fbreps)) => {
                let mut out = Output::new().values("record", record);
                merge(&mut out, "base_", rust_brep(&k, &b)?);
                for (fi, fb) in fbreps.iter().enumerate() {
                    match fb {
                        Some(fb) => merge(&mut out, &format!("feature{fi}_"), rust_brep(&k, fb)?),
                        None => out = out.int(&format!("feature{fi}_none"), 1),
                    }
                }
                Ok(out)
            }
            Err(e) => Output::unported(&e).ok_or_else(|| herr(e)),
        }
    }
}
