//! Runner for unit `topology.brep_cutting` (`SmApiCut`: `SmBrepCutting::DoCut` with an
//! `SmPlaneCutter`). Ported from upstream `SmBrepCutting`; checked against its golden.
//!
//! The case builds the body: an upstream `.smb` solid (the Rust reader
//! `own_reader::read_file`) or a box (`create_box`), optionally filleted on every edge
//! (`circular_fillet`), then cuts it by the plane through `point` with normal `normal`
//! (`topology::brep::cutting::cut`). The output is the status, samples of every edge curve
//! and face surface in list order (type code, natural domain, evaluated points; as
//! `topology.brep_local_operation`'s) and the whole B-rep graph after the call (topology,
//! vertex points, tolerances, intervals, UV domains).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep::cutting::cut;
use solidcore::topology::Brep;
use solidcore::Result as SolidResult;

use super::boolean_merge_p1::{graph_owned, Canon};
use super::topology_brep_mirror::{geometry, herr, normalize_face_shells, status};
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = crate::runner_kernel();
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
                Ok(graph_owned(&b, record, Some(serial)))
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

/// WR-458, WR-633 and WR-650 (#18921): the 4 rows the offset surface's extension from its
/// base moves (generated boxes filleted on every edge, then cut). Each moves only its graph
/// record (and `gen0056` its geometry samples): the corner blends now solve on the rails'
/// extensions from their bases. Each row is replayed as its record is, and the cut body the
/// golden records is certified: `check_brep` Level 2 reports nothing, it is the kept
/// side's one shell open only along the plane (as `SmApiCut` leaves it), its counts are unchanged, and every blend face that is a definition
/// re-certifies its net within its bound (at least one does).
#[cfg(test)]
mod wr458_moved_rows {
    use super::*;
    use solidcore::surface::surface::Surface;
    use solidcore::topology::brep::p1;
    use solidcore::topology::edge;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    /// Row, and its vertex, edge and face counts (the golden's, before and after).
    const MOVED: [(&str, [usize; 3]); 4] = [
        ("gen0056", [20, 36, 17]),
        ("gen0060", [28, 52, 25]),
        ("gen0065", [20, 36, 17]),
        ("gen0070", [28, 51, 24]),
    ];

    #[test]
    fn each_moved_row_cuts_a_valid_body_of_certified_blends() {
        let ctx = CaseContext { root: crate::case::repo_root(), seeds: 16 };
        let (mut seen, mut wrong) = (0, Vec::new());
        for c in Runner.cases(&ctx).unwrap() {
            let Some(&(name, after)) = MOVED.iter().find(|r| r.0 == c.name) else {
                continue;
            };
            seen += 1;
            let i: Inputs = c.parse().unwrap();
            assert!(matches!(i.body, Body::Box { fillet, .. } if fillet > 0.0), "{name}");
            let mut k = Kernel::default();
            let mut b = build(&mut k, &i).unwrap().unwrap();
            let st = status(cut(&mut k, &mut b, v(i.point), v(i.normal))).unwrap();
            assert_eq!(st, 0.0, "{name}: the cut succeeds");
            let got = [b.vertices().ids().count(), b.edges().ids().count(), b.faces().ids().count()];
            eprintln!("{name}: counts {got:?}");
            if got != after {
                wrong.push(name);
            }
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap().lines();
            assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
            // `SmApiCut` removes the normal's side without capping it, so the body left
            // is one shell open along the plane: every vertex lies on the kept side (or on
            // the plane) within its tolerance, every edge is manifold except the cut's own,
            // and each of those is a lamina edge (one face, two uses) whose ends lie on
            // the plane within their tolerances.
            let n = v(i.normal);
            let n = n * (1.0 / n.length());
            let o = v(i.point);
            let side = |x: solidcore::topology::VertexId| {
                let vx = b.vertex(x).unwrap();
                ((vx.point - o).dot(n), vx.tolerance)
            };
            for x in b.vertices().ids() {
                let (d, tol) = side(x);
                assert!(d <= tol, "{name}: {x:?} lies {d:e} past the plane (tolerance {tol:e})");
            }
            let mut open = 0;
            for e in b.edges().ids() {
                if edge::is_manifold(&b, e).unwrap() {
                    continue;
                }
                let uses = b.edge(e).unwrap().edgeuses.len();
                assert_eq!(uses, 2, "{name}: {e:?} is neither manifold nor a lamina edge");
                for x in edge::vertices(&b, e, None).unwrap().into_iter().flatten() {
                    let (d, tol) = side(x);
                    assert!(d.abs() <= tol, "{name}: open {e:?} ends {d:e} off the plane");
                }
                open += 1;
            }
            assert!(open > 0, "{name}: the cut left no open edge on the plane");
            let mut blends = 0;
            for f in p1::faces(&b, None).unwrap() {
                let surface = b.face(f).unwrap().surface.clone();
                let Some(blend) = surface.as_ref().and_then(Surface::procedural_blend) else {
                    continue;
                };
                let d = blend
                    .recertify(&k)
                    .unwrap_or_else(|e| panic!("{name}: {f:?} does not re-certify: {e}"));
                assert!(d <= blend.bound(), "{name}: {f:?} proved {d:e} past its bound");
                blends += 1;
            }
            assert!(blends > 0, "{name}: no blend face is a definition");
        }
        assert!(wrong.is_empty(), "counts differ: {wrong:?}");
        assert_eq!(seen, MOVED.len());
    }
}
