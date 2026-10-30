//! Differential coverage for `topology.api_files`: the `SM_API` wrappers
//! `SmApiReadBrepFromFile`, `SmApiReadPartFromFile` and `SmApiTurnToNurbs`
//! (`SmApiAddCurveOutputSlots` is file static: no shim can call it, see the unit notes).
//!
//! Each side reads its own input: an upstream test file (the Rust side reads the same
//! bytes with its own reader), or a box (`SmApiCreateBox` / `create_box`), optionally
//! filleted on every edge (`SmApiCircularFillet` / `circular_fillet`). The round trip
//! (upstream `TestSmImportExport`) writes each side's own box to a file of its own
//! (`SmApiWriteBrepToFile` / `BrepData::from_brep` and `write_to_file_with_nurbs`) and
//! reads it back, ASCII or binary. No C++-built geometry reaches the Rust side. Compared:
//! the status (`status`) and the record (`record`, layout in
//! `solidcore_ffi::units::topology_api_files`). The native side runs in a fresh child
//! process. Inputs the native build or the FFI guard refuse are `rejected`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_primitives;
use solidcore::surface::surface::Surface;
use solidcore::topology::api_files;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data::{BrepData, Keyed};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as fl;
use solidcore::topology::part_file::{Part, PartEnv};
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::{EntityRef, Result as SolidResult, SolidError};
use solidcore_ffi::units::topology_api_files::{self as ffi, NARGS};
use solidcore_ffi::FfiError;

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

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
    },
}

const fn ascii_default() -> bool {
    true
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn upstream_path(file: &str) -> Result<String, HarnessError> {
    let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
    Ok(p.to_string_lossy().into_owned())
}

fn read_bytes(file: &str) -> Result<Vec<u8>, HarnessError> {
    let p = upstream_path(file)?;
    std::fs::read(&p).map_err(|e| herr(format!("{p}: {e}")))
}

/// A new file in the temporary directory for the native round trip of `case`.
fn temp_path(case: &str) -> String {
    let name: String = case
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    std::env::temp_dir()
        .join(format!("sc_api_files_{}_{name}.smb", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

/// The shim's path and arguments of `op`.
fn native_args(op: &Op, case: &str) -> Result<(u8, String, [f64; NARGS]), HarnessError> {
    let mut a = [0.0; NARGS];
    let (code, path) = match op {
        Op::ReadBrep {
            file,
            rebuild,
            ascii,
        } => {
            a[0] = flag(file.is_some());
            a[1] = flag(*ascii);
            a[2] = flag(*rebuild);
            let path = file.as_deref().map(upstream_path).transpose()?;
            (1, path.unwrap_or_default())
        }
        Op::ReadPart { file, prefill } => {
            a[0] = flag(file.is_some());
            a[1] = 1.0;
            a[3] = f64::from(*prefill);
            let path = file.as_deref().map(upstream_path).transpose()?;
            (2, path.unwrap_or_default())
        }
        Op::TurnToNurbs { body } => match body {
            Body::Null => (3, String::new()),
            Body::File { file } => {
                a[4] = 1.0;
                (3, upstream_path(file)?)
            }
            Body::Box { size, fillet } => {
                a[4] = 2.0;
                a[5] = size[0];
                a[6] = size[1];
                a[7] = size[2];
                a[8] = *fillet;
                (3, String::new())
            }
            Body::Cylinder { radius, height } => {
                (a[4], a[5], a[6]) = (3.0, *radius, *height);
                (3, String::new())
            }
            Body::Cone {
                radius_base,
                radius_top,
                height,
            } => {
                (a[4], a[5], a[6], a[7]) = (4.0, *radius_base, *radius_top, *height);
                (3, String::new())
            }
            Body::Sphere { radius } => {
                (a[4], a[5]) = (5.0, *radius);
                (3, String::new())
            }
            Body::Torus { major, minor } => {
                (a[4], a[5], a[6]) = (6.0, *major, *minor);
                (3, String::new())
            }
            Body::MixedBox { size } => {
                (a[4], a[5], a[6], a[7]) = (7.0, size[0], size[1], size[2]);
                (3, String::new())
            }
        },
        Op::RoundTrip {
            size,
            ascii,
            rebuild,
        } => {
            a[1] = flag(*ascii);
            a[2] = flag(*rebuild);
            a[5] = size[0];
            a[6] = size[1];
            a[7] = size[2];
            (4, temp_path(case))
        }
    };
    Ok((code, path, a))
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
        SolidError::Upstream { code, .. } => Some(f64::from(*code)),
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
    let mut records = solidcore::topology::topology::NotifyRecords::new();
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
                    Err(SolidError::Upstream { code, .. }) => o.push(f64::from(code)),
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
    let face = *bl::faces(&b, None)?
        .first()
        .ok_or_else(|| SolidError::sm_err("difftest::topology_api_files::build_mixed_box"))?;
    let nurb = match b.face(face)?.surface.as_ref().map(Surface::kind) {
        Some(SurfaceKind::Plane(p)) => Surface::from(p.base_with_nurb()?.into_owned()),
        _ => {
            return Err(SolidError::sm_err(
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
    let mut records = solidcore::topology::topology::NotifyRecords::new();
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
fn write_bytes(k: &Kernel, b: &Brep, file_type: FileType) -> SolidResult<Vec<u8>> {
    let attributes = AttributeStore::default();
    let records = solidcore::topology::topology::NotifyRecords::new();
    // `FromBrep` keeps the surfaces and curves their faces and edges own: a face's (an
    // edge's) identity is the owner its geometry records under the box's own naming
    let key_of = |e: EntityRef| -> ObjectRef {
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
    let mut all = Vec::new();
    let mut data = BrepData::from_brep(b, &env, &mut all)?;
    data.write_to_file_with_nurbs(k, file_type)
}

fn rust(op: &Op) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
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
        } => {
            let file_type = if *ascii {
                FileType::Ascii
            } else {
                FileType::Binary
            };
            let bytes = match build_box(&mut k, *size, 0.0)
                .and_then(|b| write_bytes(&k, &b, file_type))
            {
                Ok(bytes) => bytes,
                Err(e) => {
                    return Output::unported(&e).ok_or_else(|| herr(format!("rust write: {e}")));
                }
            };
            finish(read_record(&mut k, Some(bytes), *ascii, *rebuild))
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.api_files"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let op: Op = case.parse()?;
        let (code, path, args) = native_args(&op, &case.name)?;
        match ffi::eval(code, &path, &args) {
            Ok(raw) if raw.first() == Some(&-999.) => Ok(Output::rejected(format!(
                "native build status {}",
                raw.get(1).copied().unwrap_or(-1.)
            ))),
            Ok(raw) => output(&raw),
            Err(e @ FfiError::InvalidArgument(_)) => {
                Ok(Output::rejected(format!("ffi guard: {e}")))
            }
            Err(e) => Err(herr(e)),
        }
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
            },
        };
        serde_json::to_value(op).ok()
    }
}
