//! Differential coverage for `topology.brep_mirror` (`SmBrep::Mirror`).
//!
//! The fixtures are upstream `.smb` solids read by the native `SmBrep::ReadFromFile`
//! and imported into Rust from the native raw graph; each is mirrored about an
//! axis-aligned or a generated plane by the real `SmBrep::Mirror` and by
//! `topology::brep::mirror::mirror`. A case compares the status, the whole B-rep graph
//! (topology, vertex points, tolerances, intervals, UV domains) and samples of every
//! edge curve, every edge use's UV trim curve and every face surface (type code,
//! natural domain, evaluated points). Inputs the native reader cannot build are
//! `rejected`, not failed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep::mirror::mirror;
use solidcore::topology::brep::p3::notify_serial;
use solidcore::topology::Brep;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::topology_brep_mirror as ffi;
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::{graph_owned, OwnerSource};
use super::topology_vertexuse::{import_raw_with_owners, path_of, BrepIn};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The runner of `SmBrep::Mirror`.
pub struct Runner;

/// Upstream solids (relative to `TestFiles/pt_TestFiles`) the generated cases mirror.
pub(super) const FILES: [&str; 12] = [
    "Solids/box.smb",
    "Solids/cylinder.smb",
    "Solids/sphere.smb",
    "Solids/pyramid.smb",
    "Solids/revolution.smb",
    "Solids/extrusion.smb",
    "Solids/l_shape_with_boss.smb",
    "Solids/nurb_fillet.smb",
    "cone.smb",
    "disk.smb",
    "box_with_large_fillet.smb",
    "plane_circ.smb",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Rebuild the B-rep's topology on reading (`ReadFromFile`'s flag).
    rebuild: bool,
    /// `m_bEditingEnabled` of the B-rep.
    editing: bool,
    /// The mirror placement's origin, X axis and Y axis (the plane is its XY plane).
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            file: "Solids/box.smb".into(),
            rebuild: false,
            editing: true,
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
        }
    }
}

pub(super) fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn args(i: &Inputs) -> [f64; 10] {
    [
        f64::from(i.editing),
        i.origin[0],
        i.origin[1],
        i.origin[2],
        i.x[0],
        i.x[1],
        i.x[2],
        i.y[0],
        i.y[1],
        i.y[2],
    ]
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

/// The native output, or `Err(output)` when the FFI guard refuses the input.
fn native(i: &Inputs, op: u8) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    match ffi::eval(op, &path(i)?, &block(i), &args(i)) {
        Ok(raw) if raw.first() == Some(&-999.) => Ok(Err(Output::rejected(format!(
            "fixture constructor status {}",
            raw.get(1).copied().unwrap_or(-1.)
        )))),
        Ok(raw) => Ok(Ok(raw)),
        Err(e @ FfiError::InvalidArgument(_)) => {
            Ok(Err(Output::rejected(format!("ffi guard: {e}"))))
        }
        Err(e) => Err(herr(e)),
    }
}

pub(super) fn status(r: SolidResult<()>) -> SolidResult<f64> {
    match r {
        Ok(()) => Ok(0.),
        Err(e) if e.is_unported() => Err(e),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(1007.),
        Err(e) => Err(e),
    }
}

// ---- geometry samples (the shim's `put_geometry`) ------------------------------------

fn curve_samples(k: &Kernel, out: &mut Vec<f64>, c: Option<&solidcore::curve::curve::Curve>) {
    let Some(c) = c else {
        out.push(-1.);
        return;
    };
    out.push(c.type_code() as f64);
    let d = c.natural_interval();
    out.extend([d.min(), d.max()]);
    for i in 0..5 {
        match c.evaluate_point(k, d.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => out.extend([0., p.x, p.y, p.z]),
            Err(_) => out.push(1.),
        }
    }
}

fn surface_samples(
    k: &Kernel,
    out: &mut Vec<f64>,
    s: Option<&solidcore::surface::surface::Surface>,
) {
    let Some(s) = s else {
        out.push(-1.);
        return;
    };
    out.push(s.type_code() as f64);
    let d = s.natural_uv_domain_if_built();
    let (a, z) = (d.min(), d.max());
    out.extend([a.x, a.y, z.x, z.y]);
    for i in 0..3 {
        for j in 0..3 {
            let uv = solidcore::geom::vector2d::Point2::new(
                a.x + (z.x - a.x) * f64::from(i) / 2.0,
                a.y + (z.y - a.y) * f64::from(j) / 2.0,
            );
            match s.evaluate_point(k, uv) {
                Ok(p) => out.extend([0., p.x, p.y, p.z]),
                Err(_) => out.push(1.),
            }
        }
    }
}

/// The geometry record of the shim's `put_geometry`; `uv_trim` samples the edge uses' UV trim
/// curves too.
pub(super) fn geometry(k: &Kernel, b: &Brep, uv_trim: bool) -> SolidResult<Vec<f64>> {
    use solidcore::topology::brep::p1;
    let mut out = Vec::new();
    let edges = p1::edges(b, None)?;
    out.push(edges.len() as f64);
    for e in edges {
        let edge = b.edge(e)?;
        curve_samples(k, &mut out, edge.curve.as_ref());
        out.push(edge.edgeuses.len() as f64);
        for &eu in &edge.edgeuses {
            if uv_trim {
                curve_samples(k, &mut out, b.edgeuse(eu)?.uv_trim_curve.as_ref());
            }
        }
    }
    let faces = p1::faces(b, None)?;
    out.push(faces.len() as f64);
    for f in faces {
        surface_samples(k, &mut out, b.face(f)?.surface.as_ref());
    }
    Ok(out)
}

/// Clears the faces' `loops` and the loops' `edges` lists: Rust-only logical records
/// the native export walk never has, so the import leaves them empty while the Rust
/// reader fills them (as `topology.brep_cutting` and `topology.brep_local_operation`).
fn clear_logical_lists(b: &mut Brep) -> SolidResult<()> {
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

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_mirror"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match native(&i, 1)? {
            Ok(raw) => raw,
            Err(refused) => return Ok(refused),
        };
        let n = *raw.first().ok_or_else(|| herr("short record"))? as usize;
        let record = raw.get(1..=n).ok_or_else(|| herr("short record"))?.to_vec();
        let (mut b, owners) =
            import_raw_with_owners(raw.get(n + 1..).ok_or_else(|| herr("short graph"))?)?;
        clear_logical_lists(&mut b).map_err(herr)?;
        normalize_face_shells(&mut b).map_err(herr)?;
        graph_owned(&b, record, OwnerSource::Native(&owners))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        // the Rust reader's B-rep of the same file, its geometry owners named by the
        // Rust reader (no C++ export or owner reaches the Rust side)
        let mut b = match crate::own_reader::read_file_rebuild(&mut k, &path(&i)?, i.rebuild)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust read: {e}")));
            }
        };
        b.set_editing_enabled(i.editing);
        let plane = Axis2Placement::new(v(i.origin), v(i.x), v(i.y));
        let mut run = || -> SolidResult<Vec<f64>> {
            let st = status(mirror(&mut k, &mut b, &plane))?;
            let mut record = vec![st];
            record.extend(geometry(&k, &b, true)?);
            Ok(record)
        };
        match run() {
            Ok(record) => {
                clear_logical_lists(&mut b).map_err(herr)?;
                normalize_face_shells(&mut b).map_err(herr)?;
                // (the serial whose topology keys name the edit's new geometry)
                let serial = notify_serial(&mut k.context, &mut b).map_err(herr)?;
                graph_owned(&b, record, OwnerSource::Rust { serial: Some(serial) })
            }
            Err(e) => Output::unported(&e).ok_or_else(|| herr(e)),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let file = FILES.get(rng.below(FILES.len() as u64) as usize)?;
        let mut i = Inputs {
            file: (*file).into(),
            ..Inputs::default()
        };
        let scale = 10f64.powi(rng.range_i64(-1, 2) as i32);
        if rng.below(3) == 0 {
            // a plane perpendicular to a coordinate axis (Z of X x Y)
            {
                let at = rng.uniform(-10., 10.) * scale;
                match rng.below(3) {
                    0 => {
                        i.origin = [at, 0., 0.];
                        i.x = [0., 1., 0.];
                        i.y = [0., 0., 1.];
                    }
                    1 => {
                        i.origin = [0., at, 0.];
                        i.x = [0., 0., 1.];
                        i.y = [1., 0., 0.];
                    }
                    _ => i.origin = [0., 0., at],
                }
            }
        } else {
            // an arbitrary plane
            {
                let mut dir = || {
                    [
                        rng.uniform(-1., 1.),
                        rng.uniform(-1., 1.),
                        rng.uniform(-1., 1.),
                    ]
                };
                i.x = dir();
                i.y = dir();
                i.origin = [
                    rng.uniform(-10., 10.) * scale,
                    rng.uniform(-10., 10.) * scale,
                    rng.uniform(-10., 10.) * scale,
                ];
            }
        }
        i.editing = rng.chance(0.9);
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(128);
        crate::runner::default_cases(self, &ctx)
    }
}

/// `Face::shell` and `Shell::faces` have no upstream counterpart: the import derives them
/// from the first face use in walk order, the port keeps them with the face's first
/// face use (`swap_reflection_shells`). Both are recomputed here from the face's own
/// first face use (`m_pFaceuse`, exported and compared), so the record the graph
/// compares is the same on both sides; the face uses' own shells are compared as they are.
pub(super) fn normalize_face_shells(b: &mut Brep) -> SolidResult<()> {
    // the faces in the graph's walk order (the import numbers them so), then the rest
    let mut faces: Vec<_> = super::boolean_merge_p1::Canon::reachable(b).faces.items;
    for f in b.faces().ids() {
        if !faces.contains(&f) {
            faces.push(f);
        }
    }
    let mut shell_of = Vec::new();
    for &f in &faces {
        let face = b.face(f)?;
        let shell = match face.faceuse {
            Some(fu) => b.faceuse(fu)?.shell.unwrap_or(face.shell),
            None => face.shell,
        };
        shell_of.push(shell);
    }
    for (&f, &shell) in faces.iter().zip(&shell_of) {
        b.face_mut(f)?.shell = shell;
    }
    let shells: Vec<_> = b.shells().ids().collect();
    for s in shells {
        b.shell_mut(s)?.faces = faces
            .iter()
            .zip(&shell_of)
            .filter(|(_, &sh)| sh == s)
            .map(|(&f, _)| f)
            .collect();
    }
    Ok(())
}
