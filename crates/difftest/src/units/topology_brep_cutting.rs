//! Differential coverage for `topology.brep_cutting` (`SmApiCut`: `SmBrepCutting::DoCut`
//! with an `SmPlaneCutter`).
//!
//! Each side builds the body itself: an upstream `.smb` solid (the native
//! `SmBrep::ReadFromFile`, the Rust reader `own_reader::read_file`) or a box
//! (`SmApiCreateBox` / `create_box`), optionally filleted on every edge
//! (`SmApiCircularFillet` / `circular_fillet`). No C++-built geometry reaches the Rust
//! side. The case cuts the body by the plane through `point` with normal `normal`
//! (`SmApiCut` / `topology::brep::cutting::cut`) and compares the status, samples of
//! every edge curve and face surface in list order (type code, natural domain, evaluated
//! points; as `topology.brep_local_operation`'s) and the whole B-rep graph after the call
//! (topology, vertex points, tolerances, intervals, UV domains). The native side runs in a
//! fresh child process. Inputs the native build or the FFI guard refuse are `rejected`,
//! not failed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep::cutting::cut;
use solidcore::topology::Brep;
use solidcore::Result as SolidResult;
use solidcore_ffi::units::topology_brep_cutting::{self as ffi, NARGS};
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::{graph_owned, Canon, OwnerSource};
use super::topology_brep_mirror::{geometry, herr, normalize_face_shells, status};
use super::topology_vertexuse::import_raw_with_owners;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The runner of `SmApiCut`.
pub struct Runner;

/// Upstream solids (relative to `TestFiles/pt_TestFiles`) the generated cases cut.
const FILES: [&str; 10] = [
    "Solids/box.smb",
    "Solids/cylinder.smb",
    "Solids/sphere.smb",
    "Solids/pyramid.smb",
    "Solids/revolution.smb",
    "Solids/extrusion.smb",
    "Solids/l_shape_with_boss.smb",
    "Solids/nurb_fillet.smb",
    "cone.smb",
    "box_with_large_fillet.smb",
];

/// The cut body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Body {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File { file: String },
    /// `SmApiCreateBox(0, size)`, then `SmApiCircularFillet(fillet)` when `fillet > 0`.
    Box { size: [f64; 3], fillet: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    body: Body,
    /// A point on the cutting plane.
    point: [f64; 3],
    /// The plane normal; the side it points to is cut away.
    normal: [f64; 3],
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            body: Body::Box {
                size: [10.0, 10.0, 10.0],
                fillet: 0.0,
            },
            point: [0.0, 0.0, 5.0],
            normal: [0.0, 0.0, 1.0],
        }
    }
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn path(i: &Inputs) -> Result<String, HarnessError> {
    match &i.body {
        Body::File { file } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        Body::Box { .. } => Ok(String::new()),
    }
}

fn args(i: &Inputs) -> [f64; NARGS] {
    let (kind, size, fillet) = match &i.body {
        Body::File { .. } => (0.0, [0.0; 3], 0.0),
        Body::Box { size, fillet } => (1.0, *size, *fillet),
    };
    [
        kind,
        size[0],
        size[1],
        size[2],
        fillet,
        i.point[0],
        i.point[1],
        i.point[2],
        i.normal[0],
        i.normal[1],
        i.normal[2],
    ]
}

/// The native output, or `Err(output)` when the body or the arguments are refused.
fn native(i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    match ffi::eval(&path(i)?, &args(i)) {
        Ok(raw) if raw.first() == Some(&-999.) => Ok(Err(Output::rejected(format!(
            "body build status {}",
            raw.get(1).copied().unwrap_or(-1.)
        )))),
        Ok(raw) => Ok(Ok(raw)),
        Err(e @ FfiError::InvalidArgument(_)) => {
            Ok(Err(Output::rejected(format!("ffi guard: {e}"))))
        }
        Err(e) => Err(herr(e)),
    }
}

/// The Rust body of `i`.
fn build(k: &mut Kernel, i: &Inputs) -> Result<SolidResult<Brep>, HarnessError> {
    match &i.body {
        Body::File { .. } => crate::own_reader::read_file(k, &path(i)?),
        Body::Box { size, fillet } => Ok((|| {
            let mut b = solidcore::primitive::api_primitives::create_box(
                k,
                Vec3::ZERO,
                size[0],
                size[1],
                size[2],
            )?;
            if *fillet > 0.0 {
                solidcore::blend::fillet_executive::api::circular_fillet(k, &mut b, *fillet)?;
            }
            Ok(b)
        })()),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_cutting"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match native(&i)? {
            Ok(raw) => raw,
            Err(refused) => return Ok(refused),
        };
        let n = *raw.first().ok_or_else(|| herr("short record"))? as usize;
        let record = raw.get(1..=n).ok_or_else(|| herr("short record"))?.to_vec();
        let (mut b, owners) =
            import_raw_with_owners(raw.get(n + 1..).ok_or_else(|| herr("short graph"))?)?;
        strip_unrecorded(&mut b).map_err(herr)?;
        normalize_face_shells(&mut b).map_err(herr)?;
        drop_unreached_shell_faces(&mut b).map_err(herr)?;
        graph_owned(&b, record, OwnerSource::Native(&owners))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        let mut b = match build(&mut k, &i)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust body build: {e}")));
            }
        };
        let mut run = || -> SolidResult<Vec<f64>> {
            let st = status(cut(&mut k, &mut b, v(i.point), v(i.normal)))?;
            let mut record = vec![st];
            record.extend(geometry(&k, &b, false)?);
            Ok(record)
        };
        match run() {
            Ok(record) => {
                strip_unrecorded(&mut b).map_err(herr)?;
                normalize_face_shells(&mut b).map_err(herr)?;
                drop_unreached_shell_faces(&mut b).map_err(herr)?;
                let serial = solidcore::topology::brep::p3::notify_serial(&mut k.context, &mut b)
                    .map_err(herr)?;
                graph_owned(&b, record, OwnerSource::Rust { serial: Some(serial) })
            }
            Err(e) => Output::unported(&e).ok_or_else(|| herr(e)),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let mut i = Inputs::default();
        let scale = 10f64.powi(rng.range_i64(-2, 2) as i32);
        if rng.below(3) == 0 {
            let file = FILES.get(rng.below(FILES.len() as u64) as usize)?;
            i.body = Body::File {
                file: (*file).into(),
            };
        } else {
            let size = [
                rng.uniform(1., 20.) * scale,
                rng.uniform(1., 20.) * scale,
                rng.uniform(1., 20.) * scale,
            ];
            let smallest = size[0].min(size[1]).min(size[2]);
            let fillet = if rng.chance(0.3) {
                rng.uniform(0.02, 0.2) * smallest
            } else {
                0.0
            };
            i.body = Body::Box { size, fillet };
        }
        // a plane through the body's box (or past it), axis-aligned or arbitrary
        let reach = match &i.body {
            Body::Box { size, .. } => *size,
            Body::File { .. } => [10.; 3],
        };
        i.point = [
            rng.uniform(-0.2, 1.2) * reach[0],
            rng.uniform(-0.2, 1.2) * reach[1],
            rng.uniform(-0.2, 1.2) * reach[2],
        ];
        i.normal = match rng.below(5) {
            0 => [1., 0., 0.],
            1 => [0., -1., 0.],
            2 => [0., 0., rng.uniform(0.5, 5.)],
            3 => [0., 0., 0.],
            _ => [
                rng.uniform(-1., 1.),
                rng.uniform(-1., 1.),
                rng.uniform(-1., 1.),
            ],
        };
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(96);
        crate::runner::default_cases(self, &ctx)
    }
}

/// Drops what the comparison does not record: the edge uses' UV trim curves (lazily made
/// caches), and the faces' `loops` and the loops' `edges` lists (Rust-only logical
/// records the native export walk never has, so the import leaves them empty), as
/// `topology.brep_local_operation` does for the faces.
fn strip_unrecorded(b: &mut Brep) -> SolidResult<()> {
    let uses: Vec<_> = b.edgeuses().ids().collect();
    for id in uses {
        b.edgeuse_mut(id)?.uv_trim_curve = None;
    }
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

/// Keeps only reached faces on the shells' face lists: `normalize_face_shells` lists every
/// face record, and the faces the cut deleted stay in the Rust arena, unlinked (the arena
/// model's documented deviation; the native export walk reaches no deleted face).
fn drop_unreached_shell_faces(b: &mut Brep) -> SolidResult<()> {
    let reached = Canon::reachable(b).faces.items;
    let shells: Vec<_> = b.shells().ids().collect();
    for id in shells {
        b.shell_mut(id)?.faces.retain(|f| reached.contains(f));
    }
    Ok(())
}
