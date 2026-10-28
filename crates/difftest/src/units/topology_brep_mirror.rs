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
use solidcore::topology::brep::p3::{notify_serial, topology_ref};
use solidcore::topology::Brep;
use solidcore::{EntityRef, Result as SolidResult, SolidError};
use solidcore_ffi::units::topology_brep_mirror as ffi;
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::graph;
use super::topology_vertexuse::{import, import_raw, path_of, BrepIn};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The runner of `SmBrep::Mirror`.
pub struct Runner;

/// Upstream solids (relative to `TestFiles/pt_TestFiles`) the generated cases mirror.
const FILES: [&str; 12] = [
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

fn herr(e: impl std::fmt::Display) -> HarnessError {
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
        Err(e @ FfiError::InvalidArgument(_)) => Ok(Err(Output::rejected(format!("ffi guard: {e}")))),
        Err(e) => Err(herr(e)),
    }
}

fn status(r: SolidResult<()>) -> SolidResult<f64> {
    match r {
        Ok(()) => Ok(0.),
        Err(e) if e.is_unported() => Err(e),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
        Err(SolidError::InvalidInput { .. }) => Ok(1007.),
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

fn geometry(k: &Kernel, b: &Brep) -> SolidResult<Vec<f64>> {
    use solidcore::topology::brep::p1;
    let mut out = Vec::new();
    let edges = p1::edges(b, None)?;
    out.push(edges.len() as f64);
    for e in edges {
        let edge = b.edge(e)?;
        curve_samples(k, &mut out, edge.curve.as_ref());
        out.push(edge.edgeuses.len() as f64);
        for &eu in &edge.edgeuses {
            curve_samples(k, &mut out, b.edgeuse(eu)?.uv_trim_curve.as_ref());
        }
    }
    let faces = p1::faces(b, None)?;
    out.push(faces.len() as f64);
    for f in faces {
        surface_samples(k, &mut out, b.face(f)?.surface.as_ref());
    }
    Ok(out)
}

// The native geometry keeps its actual edge/face owners while the import labels them
// by arena index; a plain edit notifies under the kernel's identities. Bind the owned
// geometry into that context before mirroring (as `topology.brep_p5`'s transform runner).
fn bind_owners(k: &mut Kernel, b: &mut Brep) -> SolidResult<()> {
    let serial = notify_serial(&mut k.context, b)?;
    let edges: Vec<_> = b.edges().ids().collect();
    for edge in edges {
        if let Some(curve) = b.edge_mut(edge)?.curve.as_mut() {
            curve.set_owner(Some(topology_ref(serial, EntityRef::Edge(edge))), None);
        }
    }
    let faces: Vec<_> = b.faces().ids().collect();
    for face in faces {
        if let Some(surface) = b.face_mut(face)?.surface.as_mut() {
            if surface.owner().is_some() {
                surface.set_owner(Some(topology_ref(serial, EntityRef::Face(face))));
            }
        }
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
        let (mut b, _) = import_raw(raw.get(n + 1..).ok_or_else(|| herr("short graph"))?)?;
        normalize_face_shells(&mut b).map_err(herr)?;
        Ok(graph(&b, record))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        // the native side rejects the case: nothing is compared
        let Ok(raw) = native(&i, 0)? else {
            return Ok(Output::new());
        };
        let (mut b, _) = import(&raw)?;
        b.set_editing_enabled(i.editing);
        // the native face and vertex list orders (the import orders them by the walk)
        let lists = native(&i, 2)?.map_err(|_| herr("native lists"))?;
        let mut at = 0;
        let take = |at: &mut usize| -> Result<Vec<u32>, HarnessError> {
            let n = *lists.get(*at).ok_or_else(|| herr("short lists"))? as usize;
            let out = lists
                .get(*at + 1..*at + 1 + n)
                .ok_or_else(|| herr("short lists"))?
                .iter()
                .map(|&x| x as u32)
                .collect();
            *at += 1 + n;
            Ok(out)
        };
        let faces = take(&mut at)?;
        let vertices = take(&mut at)?;
        let (nf, nv) = (faces.len(), vertices.len());
        b.set_face_list(
            faces
                .iter()
                .map(|&x| solidcore::topology::FaceId::from_index(x))
                .collect(),
        )
        .map_err(|e| herr(format!("{e}: faces {nf} of {} {faces:?}", b.faces().len())))?;
        b.set_vertex_list(
            vertices
                .iter()
                .map(|&x| solidcore::topology::VertexId::from_index(x))
                .collect(),
        )
        .map_err(|e| herr(format!("{e}: vertices {nv} of {} {vertices:?}", b.vertices().len())))?;
        let mut k = Kernel::default();
        bind_owners(&mut k, &mut b).map_err(herr)?;
        let plane = Axis2Placement::new(v(i.origin), v(i.x), v(i.y));
        let mut run = || -> SolidResult<Vec<f64>> {
            let st = status(mirror(&mut k, &mut b, &plane))?;
            let mut record = vec![st];
            record.extend(geometry(&k, &b)?);
            Ok(record)
        };
        match run() {
            Ok(record) => {
                normalize_face_shells(&mut b).map_err(herr)?;
        Ok(graph(&b, record))
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
                let mut dir = || [rng.uniform(-1., 1.), rng.uniform(-1., 1.), rng.uniform(-1., 1.)];
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
fn normalize_face_shells(b: &mut Brep) -> SolidResult<()> {
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
