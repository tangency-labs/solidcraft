//! Runner for unit `topology.api_files`: the `SM_API` wrappers `SmApiReadBrepFromFile`,
//! `SmApiReadPartFromFile` and `SmApiTurnToNurbs` (`SmApiAddCurveOutputSlots` is file
//! static, see the unit notes). Ported from upstream's `SmApi` file wrappers; checked
//! against its golden.
//!
//! The case reads its input: an upstream test file (with the Rust reader), or a box
//! (`create_box`), optionally filleted on every edge (`circular_fillet`). The round trip
//! (upstream `TestSmImportExport`) writes the box to a file (`BrepData::from_brep` and
//! `write_to_file_with_nurbs`) and reads it back, ASCII or binary. Foreign-naming
//! fixtures use `write_brep_to_file` with caller keys independent of the box's geometry
//! owners (PR #1832). Recorded: the status (`status`) and the record (`record`).
//!
//! A `write_bytes` case (`SmApiWriteBrepToFile`) writes one B-rep, ASCII or binary, and
//! records the file byte for byte (`rec`: the source's status, then on success the
//! write's status, then on success the file's length and bytes): an upstream B-rep test
//! file read with the Rust reader, under per-object identities, as `corpus.parity`, or a
//! B-rep made by `primitive::api_primitives`, its records read under the kernel's
//! identities. Primitive arguments outside the constructors' input contract
//! ([`write_rejection`]) are rejected. Besides the fixtures, [`WRITE_GENERATED`] such
//! cases are generated from their own seeds (the other generated cases do not move).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_primitives;
use solidcore::surface::surface::Surface;
use solidcore::topology::api_files;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::topology_ref;
use solidcore::topology::brep_data::{BrepData, Keyed};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as fl;
use solidcore::topology::part_file::{Part, PartEnv};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::{EntityRef, Result as SolidResult, SolidError};

use super::topology_brep_mirror::FILES as WRITE_FILES;
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated `write_bytes` cases per run.
pub const WRITE_GENERATED: usize = 200;

/// The runner of the `SM_API` files and conversion wrappers.
pub struct Runner;

/// Upstream B-rep files (relative to `TestFiles/pt_TestFiles`; the files of the
/// `topology.brep_p7` read cases).
const FILES: [&str; 15] = [
    "Mouse.smb",
    "Endcap.smb",
    "box_with_large_fillet.smb",
    "cone.smb",
    "disk.smb",
    "ellips_ts2.smb",
    "plane_circ.smb",
    "plane_square.smb",
    "sphere.smb",
    "surf_extrude.smb",
    "surf_rev.smb",
    "surf_sphere150.smb",
    "torus.smb",
    "toruswvy.smb",
    "vase_rev.smb",
];

/// Upstream part files (relative to `TestFiles/pt_TestFiles`; files of the
/// `topology.part_file` cases).
const PART_FILES: [&str; 8] = [
    "cci_bug_rhino1.smp",
    "loft-smlib.smp",
    "MakeFace/ApproximateSphere.smp",
    "ssi_2pl.smp",
    "ssi_plncyl1.smp",
    "ssi_sph_plane1.smp",
    "ssi_cone_plane1.smp",
    "ss_plane_tor1.smp",
];

/// The B-rep [`Op::TurnToNurbs`] converts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Body {
    /// A null B-rep.
    Null,
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`, read ASCII
    /// without UV trim rebuild.
    File { file: String },
    /// `SmApiCreateBox(0, size)`, then `SmApiCircularFillet(fillet)` when `fillet > 0`.
    Box { size: [f64; 3], fillet: f64 },
    /// `SmApiCreateCylinder(0, radius, height)`.
    Cylinder { radius: f64, height: f64 },
    /// `SmApiCreateCone(0, radius_base, radius_top, height)`.
    Cone {
        radius_base: f64,
        radius_top: f64,
        height: f64,
    },
    /// `SmApiCreateSphere(0, radius)`.
    Sphere { radius: f64 },
    /// `SmApiCreateTorus(0, major, minor)`.
    Torus { major: f64, minor: f64 },
    /// `SmApiCreateBox(0, size)` with face 0's surface replaced by its
    /// `CopyAnalyticAsNurb` (`ReplaceSurface(face, nurb, FALSE)`), as `SM_API_test`
    /// `TestSmBrepCopyRepresentation` makes a mixed model.
    MixedBox { size: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    /// `SmApiReadBrepFromFile(file, ascii, rebuild)`; no file is a null file name.
    ReadBrep {
        file: Option<String>,
        rebuild: bool,
        /// `bAscii` (default `true`; `false` reads the text file as binary).
        #[serde(default = "ascii_default")]
        ascii: bool,
    },
    /// `SmApiReadPartFromFile(file, TRUE, ...)` with the output arrays the `prefill` bits
    /// give one entry each (1 curves, 2 surfaces, 4 Boolean tree nodes, 8 B-reps).
    ReadPart { file: Option<String>, prefill: u8 },
    /// `SmApiTurnToNurbs(body)`.
    TurnToNurbs { body: Body },
    /// `SmApiWriteBrepToFile(box, file, ascii)`, then `SmApiReadBrepFromFile(file, ascii,
    /// rebuild)` (upstream `TestSmImportExport`).
    RoundTrip {
        size: [f64; 3],
        ascii: bool,
        rebuild: bool,
        /// Name topology independently of its geometry owners (PR #1832).
        #[serde(default)]
        foreign_naming: bool,
    },
    /// `SmApiWriteBrepToFile(brep, file, ascii)`, the file compared byte for byte.
    WriteBytes { brep: WriteSrc, ascii: bool },
}

const fn ascii_default() -> bool {
    true
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn upstream_path(file: &str) -> Result<String, HarnessError> {
    let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
    Ok(p.to_string_lossy().into_owned())
}

fn read_bytes(file: &str) -> Result<Vec<u8>, HarnessError> {
    let p = upstream_path(file)?;
    std::fs::read(&p).map_err(|e| herr(format!("{p}: {e}")))
}

fn output(raw: &[f64]) -> Result<Output, HarnessError> {
    let (&st, record) = raw.split_first().ok_or_else(|| herr("empty record"))?;
    Ok(Output::new()
        .scalar("status", st)
        .values("record", record.to_vec()))
}

/// The status code of a wrapper error (`SM_SUCCESS` is `0`).
fn status(e: &SolidError) -> Option<f64> {
    match e {
        up_err if crate::output::recorded_status(up_err).is_some() => Some(f64::from(crate::output::recorded_status(up_err).unwrap_or(-1))),
        SolidError::InvalidInput { .. } => Some(1007.),
        _ => None,
    }
}

/// The Rust output of a result: its record on success, its status code otherwise; an
/// unported callee defers.
fn finish(r: SolidResult<Vec<f64>>) -> Result<Output, HarnessError> {
    match r {
        Ok(record) => output(&record),
        Err(e) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => match status(&e) {
                Some(code) => output(&[code]),
                None => Err(herr(format!("rust: {e}"))),
            },
        },
    }
}

/// The shim's B-rep record.
fn put_brep(k: &mut Kernel, o: &mut Vec<f64>, b: &Brep) -> SolidResult<()> {
    // the recorded body, for the tolerance certificate (`certify::trim_tolerance`)
    #[cfg(test)]
    crate::certify::written(b);
    let faces = bl::faces(b, None)?;
    let edges = bl::edges(b, None)?;
    let vertices = bl::vertices(b, None)?;
    let mut loops = 0usize;
    for &f in &faces {
        loops += fl::loops(b, f, false, None)?.len();
    }
    o.push(b.tolerance());
    o.push(bl::regions(b, None)?.len() as f64);
    o.push(bl::shells(k, b, None)?.len() as f64);
    o.push(faces.len() as f64);
    o.push(loops as f64);
    o.push(edges.len() as f64);
    o.push(vertices.len() as f64);
    for &f in &faces {
        let s = b.face(f)?.surface.as_ref();
        o.push(s.map_or(-1.0, |s| s.type_code() as f64));
    }
    for &e in &edges {
        let c = b.edge(e)?.curve.as_ref();
        o.push(c.map_or(-1.0, |c| c.type_code() as f64));
        let ivl = edge::interval(b, e)?;
        o.extend([ivl.min(), ivl.max()]);
    }
    for &v in &vertices {
        let p = b.vertex(v)?.point;
        o.extend([p.x, p.y, p.z]);
    }
    Ok(())
}

/// The shim's `status x y z` of an evaluation.
fn put_point(o: &mut Vec<f64>, p: SolidResult<Vec3>) -> SolidResult<()> {
    match p {
        Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
        Err(e) if e.is_unported() => return Err(e),
        Err(_) => o.push(1.0),
    }
    Ok(())
}

/// The shim's geometry record after `SmApiTurnToNurbs`.
fn put_geometry(k: &mut Kernel, o: &mut Vec<f64>, b: &Brep) -> SolidResult<()> {
    put_brep(k, o, b)?;
    for f in bl::faces(b, None)? {
        let Some(s) = b.face(f)?.surface.as_ref() else {
            continue;
        };
        let d = s.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (d.min(), d.max());
        let (u0, v0, u1, v1) = (lo.x, lo.y, hi.x, hi.y);
        o.extend([u0, v0, u1, v1]);
        for i in 0..3 {
            for j in 0..3 {
                let uv = Point2::new(
                    u0 + (u1 - u0) * (f64::from(i) * 0.5),
                    v0 + (v1 - v0) * (f64::from(j) * 0.5),
                );
                put_point(o, s.evaluate_point(k, uv))?;
            }
        }
    }
    for e in bl::edges(b, None)? {
        let Some(c) = b.edge(e)?.curve.as_ref() else {
            continue;
        };
        let ivl = edge::interval(b, e)?;
        let (t0, t1) = (ivl.min(), ivl.max());
        for i in 0..5 {
            put_point(
                o,
                c.evaluate_point(k, t0 + (t1 - t0) * (f64::from(i) * 0.25)),
            )?;
        }
    }
    Ok(())
}

/// The identity of topology entity `e` of the `brep`th B-rep of a part (as the
/// `topology.part_file` runner; geometry keys start at `1 << 62`).
fn part_key_of(brep: usize, e: EntityRef) -> ObjectRef {
    let r = crate::own_reader::read_key_of(e);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(((brep as u64 + 1) << 52) | r.key.0),
    }
}

/// The `topology.part_file` record of a part, with a B-rep record per B-rep.
fn put_part(k: &mut Kernel, o: &mut Vec<f64>, part: &Part) -> SolidResult<()> {
    o.push(part.curves.len() as f64);
    for c in &part.curves {
        let ivl = c.object.natural_interval();
        let (ty, dim) = (c.object.type_code() as f64, c.object.dim() as f64);
        o.extend([ty, dim, ivl.min(), ivl.max()]);
    }
    o.push(part.surfaces.len() as f64);
    for s in &part.surfaces {
        let d = s.object.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (d.min(), d.max());
        o.extend([s.object.type_code() as f64, lo.x, lo.y, hi.x, hi.y]);
    }
    o.push(part.boolean_tree_nodes.len() as f64);
    o.extend(part.boolean_tree_nodes.iter().map(|&n| n as f64));
    o.push(part.breps.len() as f64);
    for b in &part.breps {
        put_brep(k, o, b)?;
    }
    Ok(())
}

/// A part with the entries the `prefill` bits give (the shim's are null pointers and the
/// node 7; the wrapper refuses before it reads them).
fn prefilled(prefill: u8) -> SolidResult<Part> {
    let mut part = Part {
        curves: Vec::new(),
        surfaces: Vec::new(),
        boolean_tree_nodes: Vec::new(),
        breps: Vec::new(),
    };
    if prefill & 1 != 0 {
        let line = BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0))?;
        part.curves.push(Keyed {
            key: ObjectKey(1),
            object: Curve::from(line),
        });
    }
    if prefill & 2 != 0 {
        let plane = BSplineSurface::create_bilinear_surface(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        )?;
        part.surfaces.push(Keyed {
            key: ObjectKey(2),
            object: Surface::from(plane),
        });
    }
    if prefill & 4 != 0 {
        part.boolean_tree_nodes.push(7);
    }
    if prefill & 8 != 0 {
        part.breps.push(Brep::new());
    }
    Ok(part)
}

/// `create_box(0, size)`, then `circular_fillet(fillet)` when `fillet > 0`.
fn build_box(k: &mut Kernel, size: [f64; 3], fillet: f64) -> SolidResult<Brep> {
    let mut b = api_primitives::create_box(k, Vec3::ZERO, size[0], size[1], size[2])?;
    if fillet > 0.0 {
        solidcore::blend::fillet_executive::api::circular_fillet(k, &mut b, fillet)?;
    }
    Ok(b)
}

/// `api_files::read_brep_from_file` on `data`, then its B-rep record.
fn read_record(
    k: &mut Kernel,
    data: Option<Vec<u8>>,
    ascii: bool,
    rebuild: bool,
) -> SolidResult<Vec<f64>> {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &crate::own_reader::read_key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    let b = api_files::read_brep_from_file(k, data, ascii, rebuild, &mut env)?;
    let mut o = vec![0.0];
    put_brep(k, &mut o, &b)?;
    put_trims(k, &mut o, &b)?;
    Ok(o)
}

/// The shim's `put_trims`: per edge and per edge use with a UV trim curve, its type,
/// natural interval and three samples, then `util::primitive::append`.
fn put_trims(k: &Kernel, o: &mut Vec<f64>, b: &Brep) -> SolidResult<()> {
    for e in bl::edges(b, None)? {
        for eu in edge::edgeuses(b, e, None, None)? {
            let Some(c) = &b.edgeuse(eu)?.uv_trim_curve else {
                continue;
            };
            let d = c.natural_interval();
            o.extend([c.type_code() as f64, d.min(), d.max()]);
            for f in [0.17, 0.37, 0.83] {
                match c.evaluate_point(k, d.evaluate(f)) {
                    Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
                    Err(e) if e.is_unported() => return Err(e),
                    Err(up_err) if crate::output::recorded_status(&up_err).is_some() => o.push(f64::from(crate::output::recorded_status(&up_err).unwrap_or(-1))),
                    Err(_) => o.push(-1.0),
                }
            }
        }
    }
    crate::util::primitive::append(k, b, o)
}

/// The mixed box: `create_box(0, size)`, then face 0's plane replaced by a plain
/// B-spline copy of its NURB (`CopyAnalyticAsNurb`, `SmBSplineSurface::Copy`) with
/// `replace_surface(.., Delete, true)` (`ReplaceSurface(face, nurb, FALSE)`).
fn build_mixed_box(k: &mut Kernel, size: [f64; 3]) -> SolidResult<Brep> {
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::p2::{replace_surface, ArenaNotifyState, OldSurface};
    use solidcore::topology::brep_cache::{arena_identity, CacheObject};
    let mut b = build_box(k, size, 0.0)?;
    let face = *bl::faces(&b, None)?.first().ok_or_else(|| {
        solidcore::error::Failure::new(
            "difftest::topology_api_files::build_mixed_box",
            solidcore::error::query::QueryFailure::BrepWithoutFaces,
        )
        .into_error()
    })?;
    let nurb = match b.face(face)?.surface.as_ref().map(Surface::kind) {
        Some(SurfaceKind::Plane(p)) => Surface::from(p.base_with_nurb()?.into_owned()),
        _ => {
            return Err(crate::output::routine_failure(
                "difftest::topology_api_files::build_mixed_box: face 0 is not a plane",
            ))
        }
    };
    let mut state = ArenaNotifyState::new(&b);
    let mut env = state.env(None);
    replace_surface(
        k,
        &mut b,
        &mut env,
        face,
        nurb,
        arena_identity(CacheObject::Face(face)),
        OldSurface::Delete,
        true,
    )?;
    Ok(b)
}

/// `api_files::read_part_from_file` on `data` into the [`prefilled`] part, then its
/// part record.
fn part_record(k: &mut Kernel, data: Option<Vec<u8>>, prefill: u8) -> SolidResult<Vec<f64>> {
    let mut part = prefilled(prefill)?;
    let mut store = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let mut env = PartEnv {
        attributes: &mut store,
        key_of: &part_key_of,
        records: &mut records,
        next_key: 1 << 62,
    };
    api_files::read_part_from_file(k, data, true, &mut part, &mut env)?;
    let mut o = vec![0.0];
    put_part(k, &mut o, &part)?;
    Ok(o)
}

/// The contents of `b` written by `SmBrep::WriteToFile(name, type, TRUE, FALSE)`:
/// `BrepData::from_brep`, then `write_to_file_with_nurbs` (as `corpus.parity` writes).
fn write_bytes(
    k: &Kernel,
    b: &Brep,
    file_type: FileType,
    foreign_naming: bool,
) -> SolidResult<Vec<u8>> {
    let attributes = AttributeStore::default();
    let records = NotifyRecords::new();
    // Foreign caller keys deliberately bypass the geometry-owner substitution below.
    // `FromBrep` keeps the surfaces and curves their faces and edges own: a face's (an
    // edge's) identity is the owner its geometry records under the box's own naming
    let key_of = |e: EntityRef| -> ObjectRef {
        if foreign_naming {
            return write_file_key(e);
        }
        let owner = match e {
            EntityRef::Face(f) => b.face(f).ok().and_then(|x| x.surface.as_ref()?.owner()),
            EntityRef::Edge(x) => b.edge(x).ok().and_then(|y| y.curve.as_ref()?.owner()),
            _ => None,
        };
        owner.unwrap_or_else(|| crate::own_reader::read_key_of(e))
    };
    let env = FromBrepEnv {
        attributes: &attributes,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    if foreign_naming {
        return api_files::write_brep_to_file(k, Some(b), &env, file_type == FileType::Ascii);
    }
    let mut all = Vec::new();
    let mut data = BrepData::from_brep(b, &env, &mut all)?;
    data.write_to_file_with_nurbs(k, file_type)
}

fn rust(op: &Op) -> Result<Output, HarnessError> {
    let mut k = crate::runner_kernel();
    match op {
        Op::ReadBrep {
            file,
            rebuild,
            ascii,
        } => {
            let data = file.as_deref().map(read_bytes).transpose()?;
            finish(read_record(&mut k, data, *ascii, *rebuild))
        }
        Op::ReadPart { file, prefill } => {
            let data = file.as_deref().map(read_bytes).transpose()?;
            finish(part_record(&mut k, data, *prefill))
        }
        Op::TurnToNurbs { body } => {
            let mut b = match body {
                Body::Null => {
                    return finish(api_files::turn_to_nurbs(&mut k, None).map(|()| vec![0.0]))
                }
                Body::File { file } => crate::own_reader::read_file(&mut k, &upstream_path(file)?)?,
                Body::Box { size, fillet } => build_box(&mut k, *size, *fillet),
                Body::Cylinder { radius, height } => {
                    api_primitives::create_cylinder(&mut k, Vec3::ZERO, *radius, *height)
                }
                Body::Cone {
                    radius_base,
                    radius_top,
                    height,
                } => api_primitives::create_cone(
                    &mut k,
                    Vec3::ZERO,
                    *radius_base,
                    *radius_top,
                    *height,
                ),
                Body::Sphere { radius } => {
                    api_primitives::create_sphere(&mut k, Vec3::ZERO, *radius)
                }
                Body::Torus { major, minor } => {
                    api_primitives::create_torus(&mut k, Vec3::ZERO, *major, *minor)
                }
                Body::MixedBox { size } => build_mixed_box(&mut k, *size),
            };
            let b = match &mut b {
                Ok(b) => b,
                Err(e) => {
                    return Output::unported(e)
                        .ok_or_else(|| herr(format!("rust body build: {e}")));
                }
            };
            let r = match api_files::turn_to_nurbs(&mut k, Some(&mut *b)) {
                Ok(()) => {
                    let mut o = vec![0.0];
                    match put_geometry(&mut k, &mut o, b) {
                        Ok(()) => put_trims(&k, &mut o, b).map(|()| o),
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            };
            finish(r)
        }
        Op::RoundTrip {
            size,
            ascii,
            rebuild,
            foreign_naming,
        } => {
            let file_type = if *ascii {
                FileType::Ascii
            } else {
                FileType::Binary
            };
            let bytes = match build_box(&mut k, *size, 0.0)
                .and_then(|b| write_bytes(&k, &b, file_type, *foreign_naming))
            {
                Ok(bytes) => bytes,
                Err(e) => {
                    return Output::unported(&e).ok_or_else(|| herr(format!("rust write: {e}")));
                }
            };
            finish(read_record(&mut k, Some(bytes), *ascii, *rebuild))
        }
        Op::WriteBytes { brep, ascii } => rust_write(brep, *ascii),
    }
}

// ---- the byte-for-byte write ---------------------------------------------------------

/// The B-rep a `write_bytes` case writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
enum WriteSrc {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`, read ASCII.
    File {
        file: String,
        #[serde(default)]
        rebuild: bool,
    },
    Box {
        origin: [f64; 3],
        size: [f64; 3],
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    Cylinder {
        origin: [f64; 3],
        radius: f64,
        height: f64,
    },
    Cone {
        origin: [f64; 3],
        radius_base: f64,
        radius_top: f64,
        height: f64,
    },
    Torus {
        center: [f64; 3],
        major: f64,
        minor: f64,
    },
    Plane {
        origin: [f64; 3],
        size: [f64; 2],
    },
}

/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1.0e-12;

/// The largest sphere radius magnitude `SmApiCreateSphere` builds (above it upstream's
/// `SmPrimitiveCreation::CreateSphere` crashes).
const MAX_SPHERE_RADIUS: f64 = 6.0e9;

/// The largest coordinate or dimension magnitude of the entry points upstream does not
/// validate (`SmApiCreateTorus`, `SmApiCreatePlaneBrep`).
const MAX_KERNEL_COORD: f64 = 1.0e6;

/// Why `s` is rejected (the input contract of `primitive.api_primitives`'s constructors):
/// a non-finite argument, a valid sphere radius above [`MAX_SPHERE_RADIUS`], or a torus or
/// plane argument above [`MAX_KERNEL_COORD`].
fn write_rejection(s: &WriteSrc) -> Option<String> {
    let args: Vec<f64> = match s {
        WriteSrc::File { .. } => return None,
        WriteSrc::Box { origin, size } => [&origin[..], &size[..]].concat(),
        WriteSrc::Sphere { center, radius } => [&center[..], &[*radius]].concat(),
        WriteSrc::Cylinder {
            origin,
            radius,
            height,
        } => [&origin[..], &[*radius, *height]].concat(),
        WriteSrc::Cone {
            origin,
            radius_base,
            radius_top,
            height,
        } => [&origin[..], &[*radius_base, *radius_top, *height]].concat(),
        WriteSrc::Torus {
            center,
            major,
            minor,
        } => [&center[..], &[*major, *minor]].concat(),
        WriteSrc::Plane { origin, size } => [&origin[..], &size[..]].concat(),
    };
    if !args.iter().all(|a| a.is_finite()) {
        return Some("non-finite primitive argument".into());
    }
    match s {
        // (`SM_IS_VALID_DOUBLE`: within `SM_BIG_DOUBLE / 100`)
        WriteSrc::Sphere { radius, .. }
            if radius.abs() <= 1.0e18
                && radius.abs() > EFF_ZERO
                && radius.abs() > MAX_SPHERE_RADIUS =>
        {
            Some("sphere: radius above the kernel's domain".into())
        }
        WriteSrc::Torus { .. } | WriteSrc::Plane { .. }
            if args.iter().any(|x| x.abs() > MAX_KERNEL_COORD) =>
        {
            Some("argument out of bounds".into())
        }
        _ => None,
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn write_status(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } => 1007,
        up_err if crate::output::recorded_status(up_err).is_some() => i64::from(crate::output::recorded_status(up_err).unwrap_or(-1)),
        _ => -1,
    }
}

/// A distinct identity for every topology object of the B-rep read from a file (as
/// `corpus.parity`'s).
fn write_file_key(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey((1 << 36) | ((kind + 1) << 32) | u64::from(i)),
    }
}

/// The first key of the geometry the readers name (above every topology key).
const WRITE_GEOMETRY_KEYS: u64 = 1 << 62;

/// The write of `b` under `key_of`, `records` and `attributes`: `[0, write_status, n,
/// bytes...]` (the source succeeded).
fn write_rec(
    k: &Kernel,
    b: &Brep,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
    records: &NotifyRecords,
    attributes: &AttributeStore,
    ascii: bool,
) -> Output {
    let env = FromBrepEnv {
        attributes,
        key_of,
        records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    match api_files::write_brep_to_file(k, Some(b), &env, ascii) {
        Ok(bytes) => {
            let mut rec = vec![0, 0, bytes.len() as i64];
            rec.extend(bytes.iter().map(|&c| i64::from(c)));
            Output::new().ints("rec", rec)
        }
        Err(e) => Output::unported(&e)
            .unwrap_or_else(|| Output::new().ints("rec", vec![0, write_status(&e)])),
    }
}

fn rust_write(s: &WriteSrc, ascii: bool) -> Result<Output, HarnessError> {
    if let Some(why) = write_rejection(s) {
        return Ok(Output::rejected(why));
    }
    let mut k = crate::runner_kernel();
    let made = match s {
        WriteSrc::File { file, rebuild } => {
            let bytes = read_bytes(file)?;
            let mut records = NotifyRecords::new();
            let mut attributes = AttributeStore::default();
            let mut b = Brep::new_with_infinite_region().map_err(herr)?;
            let read = {
                let mut env = TopologyEnv {
                    all_attributes: &[],
                    attributes: &mut attributes,
                    key_of: &write_file_key,
                    records: &mut records,
                    next_key: WRITE_GEOMETRY_KEYS,
                };
                solidcore::topology::brep::p7::read_from_file(
                    &mut k,
                    &mut b,
                    bytes,
                    FileType::Ascii,
                    *rebuild,
                    &mut env,
                )
            };
            return Ok(match read {
                Ok(()) => write_rec(&k, &b, &write_file_key, &records, &attributes, ascii),
                Err(e) => Output::unported(&e)
                    .unwrap_or_else(|| Output::new().ints("rec", vec![write_status(&e)])),
            });
        }
        WriteSrc::Box { origin, size } => api_primitives::create_box(
            &mut k,
            Point3::from_array(*origin),
            size[0],
            size[1],
            size[2],
        ),
        WriteSrc::Sphere { center, radius } => {
            api_primitives::create_sphere(&mut k, Point3::from_array(*center), *radius)
        }
        WriteSrc::Cylinder {
            origin,
            radius,
            height,
        } => api_primitives::create_cylinder(&mut k, Point3::from_array(*origin), *radius, *height),
        WriteSrc::Cone {
            origin,
            radius_base,
            radius_top,
            height,
        } => api_primitives::create_cone(
            &mut k,
            Point3::from_array(*origin),
            *radius_base,
            *radius_top,
            *height,
        ),
        WriteSrc::Torus {
            center,
            major,
            minor,
        } => api_primitives::create_torus(&mut k, Point3::from_array(*center), *major, *minor),
        WriteSrc::Plane { origin, size } => {
            api_primitives::create_plane(&mut k, Point3::from_array(*origin), size[0], size[1])
        }
    };
    let b = match made {
        Ok(b) => b,
        Err(e) => {
            return Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::new().ints("rec", vec![write_status(&e)])));
        }
    };
    // the primitive's records are the kernel's, under its identities
    let serial = b.notify_serial();
    // `FromBrep` keeps the surfaces and curves their faces and edges own: a face's (an
    // edge's) identity is the owner its geometry records
    let key_of = |e: EntityRef| -> ObjectRef {
        let owner = match e {
            EntityRef::Face(f) => b.face(f).ok().and_then(|x| x.surface.as_ref()?.owner()),
            EntityRef::Edge(x) => b.edge(x).ok().and_then(|y| y.curve.as_ref()?.owner()),
            _ => None,
        };
        owner.unwrap_or_else(|| match serial {
            Some(s) => topology_ref(s, e),
            None => write_file_key(e),
        })
    };
    let attributes = AttributeStore::default();
    Ok(write_rec(
        &k,
        &b,
        &key_of,
        &k.notify_records,
        &attributes,
        ascii,
    ))
}

/// A primitive at scale `s` (an origin up to `20 s` from the world origin).
fn gen_write_primitive(rng: &mut Rng) -> WriteSrc {
    let s = *rng.choose(&[1.0e-3, 1.0, 1.0, 1.0, 1.0e3]).unwrap_or(&1.0);
    let o = rng.point3(-20.0 * s, 20.0 * s);
    let dim = |rng: &mut Rng| rng.uniform(0.1, 10.0) * s;
    match rng.below(6) {
        0 => WriteSrc::Box {
            origin: o,
            size: [dim(rng), dim(rng), dim(rng)],
        },
        1 => WriteSrc::Sphere {
            center: o,
            radius: dim(rng),
        },
        2 => WriteSrc::Cylinder {
            origin: o,
            radius: dim(rng),
            height: dim(rng),
        },
        3 => {
            let base = dim(rng);
            let top = if rng.chance(0.3) { 0.0 } else { dim(rng) };
            WriteSrc::Cone {
                origin: o,
                radius_base: base,
                radius_top: top,
                height: dim(rng),
            }
        }
        4 => {
            let major = dim(rng);
            WriteSrc::Torus {
                center: o,
                major,
                minor: major * rng.uniform(0.05, 0.9),
            }
        }
        _ => WriteSrc::Plane {
            origin: o,
            size: [dim(rng), dim(rng)],
        },
    }
}

/// One generated `write_bytes` case: a primitive, or (30%) an upstream B-rep file.
fn gen_write(rng: &mut Rng) -> Op {
    let brep = if rng.chance(0.3) {
        let file = rng.choose(&WRITE_FILES).copied().unwrap_or("Solids/box.smb");
        WriteSrc::File {
            file: file.to_string(),
            rebuild: rng.chance(0.2),
        }
    } else {
        gen_write_primitive(rng)
    };
    Op::WriteBytes {
        brep,
        ascii: rng.chance(0.5),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.api_files"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut cases = default_cases(self, ctx)?;
        let unit = "topology.api_files/write_bytes";
        for i in 0..WRITE_GENERATED {
            let seed = CaseContext::seed_for(unit, i);
            let mut rng = Rng::new(seed);
            let inputs = serde_json::to_value(gen_write(&mut rng)).map_err(herr)?;
            cases.push(Case {
                unit: self.id().to_string(),
                name: format!("write_bytes{i:04}"),
                source: CaseSource::Generated { seed },
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let op: Op = case.parse()?;
        rust(&op)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = 10f64.powi(rng.range_i64(-2, 2) as i32);
        let mut size = || rng.uniform(1.0, 20.0) * scale;
        let size = [size(), size(), size()];
        let op = match rng.below(4) {
            0 => Op::ReadBrep {
                file: (!rng.chance(0.1))
                    .then(|| (*rng.choose(&FILES).unwrap_or(&"disk.smb")).to_string()),
                rebuild: rng.chance(0.5),
                ascii: true,
            },
            1 => Op::ReadPart {
                file: (!rng.chance(0.1))
                    .then(|| (*rng.choose(&PART_FILES).unwrap_or(&"ssi_2pl.smp")).to_string()),
                prefill: if rng.chance(0.7) {
                    0
                } else {
                    rng.below(16) as u8
                },
            },
            2 => Op::TurnToNurbs {
                body: match rng.below(16) {
                    0 => Body::Null,
                    10 => Body::Cylinder {
                        radius: size[0],
                        height: size[1],
                    },
                    11 => Body::Cone {
                        radius_base: size[0],
                        radius_top: if rng.chance(0.5) {
                            0.5 * size[0]
                        } else {
                            size[1]
                        },
                        height: size[2],
                    },
                    12 => Body::Sphere { radius: size[0] },
                    13 => Body::Torus {
                        major: size[0] + size[1],
                        minor: size[1],
                    },
                    14 => Body::MixedBox { size },
                    1..=4 => Body::File {
                        file: (*rng.choose(&FILES).unwrap_or(&"disk.smb")).to_string(),
                    },
                    _ => {
                        let smallest = size[0].min(size[1]).min(size[2]);
                        Body::Box {
                            size,
                            fillet: if rng.chance(0.3) {
                                rng.uniform(0.02, 0.2) * smallest
                            } else {
                                0.0
                            },
                        }
                    }
                },
            },
            _ => Op::RoundTrip {
                size,
                ascii: rng.chance(0.5),
                rebuild: rng.chance(0.3),
                foreign_naming: false,
            },
        };
        serde_json::to_value(op).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::topology::brep::p7::read_from_file;
    use solidcore::topology::database_io::{fixed16, round_trip};

    /// The file a `write_bytes` output records (`rec`: `0, 0, n, bytes...`).
    fn file_of(out: &Output) -> Option<Vec<u8>> {
        let rec = out.ints.get("rec")?;
        if rec.first() != Some(&0) || rec.get(1) != Some(&0) {
            return None;
        }
        Some(rec.get(3..)?.iter().map(|&c| c as u8).collect())
    }

    /// `bytes` read with the Rust reader under the write cases' file identities, then
    /// written as a binary file under the same identities (binary doubles are exact).
    fn binary_after_reading(bytes: Vec<u8>, file_type: FileType) -> Vec<u8> {
        let mut k = crate::runner_kernel();
        let mut records = NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        {
            let mut env = TopologyEnv {
                all_attributes: &[],
                attributes: &mut attributes,
                key_of: &write_file_key,
                records: &mut records,
                next_key: WRITE_GEOMETRY_KEYS,
            };
            read_from_file(&mut k, &mut b, bytes, file_type, false, &mut env).unwrap();
        }
        let out = write_rec(&k, &b, &write_file_key, &records, &attributes, false);
        file_of(&out).unwrap()
    }

    /// The ASCII `write_bytes` cases that write a file: all 114 moved with the WR-655 fix
    /// (the expectations commit of the flag's retirement), and every one is certified
    /// below.
    const ASCII_FILES: usize = 114;

    /// Certificate for the ASCII `write_bytes` rows (WR-655): every finite double in each
    /// ASCII file is written as the shortest text that reads back as itself (`round_trip`,
    /// no `%.16f` text left), and the file carries every double exactly: read back and
    /// written as a binary file, it gives the same bytes as the case's own binary file read
    /// back and written the same way. The certified cases must be exactly the
    /// [`ASCII_FILES`] ASCII write cases that write a file, each with a committed golden
    /// row.
    #[test]
    fn ascii_files_carry_every_double_exactly() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let cases = Runner.cases(&ctx).unwrap();
        let golden = std::fs::read_to_string(
            crate::case::repo_root().join("crates/goldens/expected/topology.api_files.jsonl"),
        )
        .unwrap();
        let golden: std::collections::BTreeSet<String> = golden
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v.get("case")?.as_str().map(str::to_string))
            .collect();
        let mut covered = Vec::new();
        let mut lossy = Vec::new();
        for case in &cases {
            let op: Op = case
                .parse()
                .unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
            // the ASCII writes only: the other ops and the binary writes do not move
            let Op::WriteBytes { brep, ascii: true } = op else {
                continue;
            };
            let ascii = rust(&Op::WriteBytes {
                brep: brep.clone(),
                ascii: true,
            })
            .unwrap();
            let binary = rust(&Op::WriteBytes {
                brep: brep.clone(),
                ascii: false,
            })
            .unwrap();
            let name = &case.name;
            let Some(text) = file_of(&ascii) else {
                // a refused source or write: the binary case is refused the same way
                assert_eq!(file_of(&binary), None, "{name}");
                continue;
            };
            let bin = file_of(&binary).unwrap_or_else(|| panic!("{name}: no binary file"));
            let s = String::from_utf8(text.clone()).unwrap();
            let mut loses = false;
            for token in s.split_whitespace() {
                let Ok(v) = token.parse::<f64>() else {
                    continue;
                };
                if token.contains(['.', 'e', 'n', 'i']) {
                    assert_eq!(round_trip(v), token, "{name}: double {token}");
                    // upstream's `%.16f` text of the same double
                    loses |= !v.is_nan()
                        && fixed16(v).parse::<f64>().map_or(true, |w| w.to_bits() != v.to_bits());
                }
            }
            assert_eq!(
                binary_after_reading(text, FileType::Ascii),
                binary_after_reading(bin, FileType::Binary),
                "{name}: the ASCII file does not carry the binary file's doubles"
            );
            if loses {
                lossy.push(name.clone());
            }
            assert!(golden.contains(name), "{name}: no committed golden row");
            covered.push(name.clone());
        }
        println!("ASCII_EXACT covered {}: {covered:?}", covered.len());
        println!("ASCII_EXACT %.16f loses a double in {}: {lossy:?}", lossy.len());
        assert_eq!(covered.len(), ASCII_FILES);
    }

    /// The surface part of a found sphere or torus, or of a B-spline surface.
    fn nurb_part(s: &Surface) -> Option<&BSplineSurface> {
        use solidcore::surface::surface::SurfaceKind;
        match s.kind() {
            SurfaceKind::BSpline(b) => Some(b),
            SurfaceKind::Sphere(sp) => Some(sp.base().base()),
            SurfaceKind::Torus(t) => Some(t.base().base()),
            _ => None,
        }
    }

    /// A NURB surface's control points and knots as bits.
    fn surface_bits(s: &BSplineSurface) -> Vec<u64> {
        let n = s.gw_nurb().expect("a NURB");
        let mut out: Vec<u64> = n
            .rows()
            .flat_map(|r| r.iter().flat_map(|p| [p.x, p.y, p.z, p.w]))
            .map(f64::to_bits)
            .collect();
        out.extend(n.u_knots().iter().chain(n.v_knots()).map(|x| x.to_bits()));
        out
    }

    /// A UV curve's NURB as written (`Debug` writes every double as the shortest text
    /// that reads back as itself, so equal text is equal bits). A line's record is its
    /// NURB: the written file can hold a source polyline trim as the `Line` over that same
    /// NURB (`Solids/sphere.smb`), so the NURB is what must be the source's. Any other kind is
    /// compared by its whole record.
    fn curve_text(c: &Curve) -> String {
        use solidcore::curve::curve::CurveKind;
        match c.kind() {
            CurveKind::BSpline(b) => format!("{:?}", b.gw_nurb()),
            CurveKind::Line(l) => format!("{:?}", l.base().gw_nurb()),
            k => format!("{k:?}"),
        }
    }

    /// The UV curves of the edge uses on `face`'s loops (sorted).
    fn face_trims(d: &BrepData, face: usize) -> Vec<String> {
        let f = &d.faces[face];
        let loops = f.start_loop..f.start_loop + f.num_loops;
        let mut out: Vec<String> = d
            .edgeuses
            .iter()
            .filter(|eu| loops.contains(&eu.loop_))
            .map(|eu| curve_text(&d.uv_curves[eu.uv_curve as usize].object))
            .collect();
        out.sort();
        out
    }

    /// WR-493 (#18195): the `write_bytes` rows the retirement moved. Each writes a file
    /// with a sphere or torus found in a NURB, and the file written holds, for every such
    /// face, the source file's own surface (its NURB, bit for bit), and on it the source's own
    /// UV trims (the same NURBs, bit for bit; a polyline trim can be written as the line over
    /// that NURB) but those the read remade (counted; as on any surface, the healer remakes
    /// a trim that misses its edge, see `wr493_class_proofs`): the retirement moved these rows only
    /// by writing the file's own surface and trims where main wrote the NURB
    /// made from the fitted circles and the trims rebuilt on it.
    #[test]
    fn wr493_written_found_faces_are_the_source_files() {
        use solidcore::surface::surface::SurfaceKind;
        // (case, found faces, trims on them written as the source's, trims the read remade)
        const MOVED: [(&str, usize, usize, usize); 14] = [
            ("write_bytes0004", 1, 2, 0),
            ("write_bytes0026", 1, 2, 0),
            ("write_bytes0061", 1, 2, 0),
            ("write_bytes0085", 1, 12, 2),
            ("write_bytes0094", 1, 2, 0),
            ("write_bytes0106", 1, 2, 0),
            ("write_bytes0115", 1, 12, 2),
            ("write_bytes0153", 1, 4, 10),
            ("write_bytes0179", 1, 12, 2),
            ("write_bytes0180", 1, 2, 0),
            ("write_bytes_file_nurb_fillet_ascii", 1, 12, 2),
            ("write_bytes_file_nurb_fillet_binary", 1, 12, 2),
            ("write_bytes_file_sphere_ascii", 1, 2, 0),
            ("write_bytes_file_sphere_binary", 1, 2, 0),
        ];
        let mut measured = Vec::new();
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let cases = Runner.cases(&ctx).unwrap();
        let k = crate::runner_kernel();
        for (name, ..) in MOVED {
            let case = cases.iter().find(|c| c.name == name).unwrap();
            let op: Op = case.parse().unwrap();
            let Op::WriteBytes {
                brep: WriteSrc::File { file, .. },
                ascii,
            } = &op
            else {
                panic!("{name}: not a file write");
            };
            let written = file_of(&rust(&op).unwrap()).unwrap_or_else(|| panic!("{name}"));
            let file_type = if *ascii {
                FileType::Ascii
            } else {
                FileType::Binary
            };
            let w = BrepData::read_from_file(&k, written, file_type).unwrap();
            let src = BrepData::read_from_file(&k, read_bytes(file).unwrap(), FileType::Ascii)
                .unwrap();
            assert_eq!(w.faces.len(), src.faces.len(), "{name}");
            let (mut found, mut kept, mut remade) = (0, 0, 0);
            for (i, (wf, sf)) in w.faces.iter().zip(&src.faces).enumerate() {
                let ws = &w.surfaces[wf.surface as usize].object;
                if !matches!(ws.kind(), SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)) {
                    continue;
                }
                found += 1;
                let ss = &src.surfaces[sf.surface as usize].object;
                assert!(
                    matches!(ss.kind(), SurfaceKind::BSpline(_)),
                    "{name}: face {i}: the source's surface is not a NURB"
                );
                assert_eq!(
                    surface_bits(nurb_part(ws).unwrap()),
                    surface_bits(nurb_part(ss).unwrap()),
                    "{name} ({file}): face {i}: the written surface is not the source's"
                );
                let (wt, st) = (face_trims(&w, i), face_trims(&src, i));
                assert!(!wt.is_empty(), "{name}: face {i}: no trims");
                // one-to-one: each written trim uses up one matching source trim
                let mut left = st.clone();
                let same = wt
                    .iter()
                    .filter(|t| {
                        left.iter()
                            .position(|s| s == *t)
                            .map(|at| left.swap_remove(at))
                            .is_some()
                    })
                    .count();
                kept += same;
                remade += wt.len() - same;
            }
            assert!(found > 0, "{name} ({file}): no sphere or torus found");
            measured.push((name, found, kept, remade));
        }
        assert_eq!(measured, MOVED, "{measured:#?}");
    }
}
