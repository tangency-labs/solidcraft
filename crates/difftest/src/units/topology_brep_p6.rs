//! Native B-rep p6 operations over independently imported raw topology.
//! Closure passes compare actual statuses, output reset, shell ownership, context
//! mark advancement/locks, and every face-use mark. Copy and face construction
//! currently compare only empty-input/null-surface paths; other cases defer.

use super::topology_vertexuse::{self as shared, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::context::{MarkLock, MarkType};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p6::{self, FaceTopologyOptions};
use solidcore::topology::topology_traverser::{Entity, Marks};
use solidcore::topology::{Brep, EdgeId, FaceuseId, Region};
use solidcore::SolidError;
use solidcore_ffi::units::topology_brep_p6::{self as ffi, Request};
use solidcore_ffi::units::topology_vertexuse::counts;

/// Differential runner for `topology.brep_p6`.
pub struct Runner;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Inputs {
    brep: BrepIn,
    op: i32,
    #[serde(default)]
    seed: i64,
    #[serde(default)]
    edges: Vec<i64>,
    #[serde(default = "one")]
    repeat: i32,
    #[serde(default)]
    lock_mask: i32,
    #[serde(default)]
    flags: i32,
}

// Match Graph::find: a live object allocated after the input walk is -3,
// distinct from null (-1), even if its arena index is otherwise representable.
fn original_index(index: u32, original_count: usize) -> i64 {
    if (index as usize) < original_count {
        i64::from(index)
    } else {
        -3
    }
}

fn one() -> i32 {
    1
}
fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut out = Vec::new();
    match &i.brep {
        BrepIn::File { rebuild, .. } => out.extend([0.0, f64::from(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            out.push(1.0);
            shared::push_curve(&mut out, curve)?;
            out.extend([t0, t1]);
        }
        BrepIn::Face { surface, dom } => {
            out.push(2.0);
            shared::push_surface(&mut out, surface)?;
            out.extend(dom);
        }
        BrepIn::Network => out.push(3.0),
    }
    out.extend([0.0, 0.0]);
    Ok(out)
}

// Unit-local ASCII topology fixtures use the same native file reader as upstream
// files. Keep their path under this fixture directory, without parent components.
fn path_of(brep: &BrepIn) -> Result<String, HarnessError> {
    if let BrepIn::File { file, .. } = brep {
        if file.starts_with("fixtures/topology.brep_p6/") {
            let path = std::path::Path::new(file);
            if path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(harness("invalid local fixture path"));
            }
            let path = crate::case::repo_root().join(path);
            if !path.is_file() {
                return Err(harness("local fixture not found"));
            }
            return Ok(path.to_string_lossy().into_owned());
        }
    }
    shared::path_of(brep)
}

fn request(i: &Inputs, c: [usize; 10]) -> Request {
    Request {
        op: i.op,
        seed: shared::sel(i.seed, c[2]),
        indices: i.edges.iter().map(|e| shared::sel(*e, c[7])).collect(),
        repeat: i.repeat,
        lock_mask: i.lock_mask,
        flags: i.flags,
    }
}

const MARKS: [MarkType; 5] = [
    MarkType::Mark,
    MarkType::Mark2,
    MarkType::Mark3,
    MarkType::MarkIo,
    MarkType::MarkAssert,
];

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p6"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let path = path_of(&i.brep)?;
        let input = block(&i)?;
        let fixture = ffi::export(&path, &input).map_err(harness)?;
        let Some(c) = counts(&fixture.graph) else {
            return Ok(Output::rejected("native fixture constructor"));
        };
        let output = ffi::eval(&path, &input, &request(&i, c)).map_err(harness)?;
        if output.first().is_some_and(|v| *v == -1.0 || *v == -2.0) {
            return Ok(Output::rejected(format!(
                "native fixture constructor: {output:?}"
            )));
        }
        Ok(Output::new().ints(
            "state",
            output.into_iter().map(|v| v as i64).collect::<Vec<_>>(),
        ))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let fixture = ffi::export(&path_of(&i.brep)?, &block(&i)?).map_err(harness)?;
        let c = counts(&fixture.graph).ok_or_else(|| harness("fixture construction failed"))?;
        let request = request(&i, c);
        let (mut b, _) = shared::import(&fixture.graph)?;
        // SmBrep.cpp:549-552: even an empty native B-rep owns a void
        // infinite region. Re-express that input fixture, not an empty arena.
        b.set_key(ObjectKey(0xB000_0000));
        let mut destination = Brep::new();
        destination.set_key(ObjectKey(0xD000_0000));
        let infinite = destination
            .add_region_record(Region {
                in_brep: true,
                is_void: 1,
                ..Region::default()
            })
            .map_err(harness)?;
        destination.set_infinite_region_record(Some(infinite));
        let mut k = Kernel::default();
        let mut marks = Marks::default();
        let mut outer = if i.lock_mask == 0 {
            None
        } else {
            Some(
                MarkLock::new(std::slice::from_mut(&mut k.context), 0, i.lock_mask)
                    .map_err(harness)?,
            )
        };
        let before = MARKS.map(|mt| k.context.current_mark(mt));
        let mut body = Vec::new();
        let mut status = 0;
        for _ in 0..request.repeat {
            let result = match request.op {
                0 => p6::test_closure_split_region_attempt(
                    &mut k,
                    &mut b,
                    &mut marks,
                    i.flags & 1 != 0,
                    FaceuseId::from_index(request.seed as u32),
                )
                .and_then(|attempt| {
                    body = vec![
                        attempt
                            .shell
                            .map_or(-1, |s| original_index(s.index(), c[1])),
                        attempt
                            .region
                            .map_or(-1, |r| original_index(r.index(), c[0])),
                    ];
                    attempt.status
                }),
                1 => {
                    let edges: Vec<_> = request
                        .indices
                        .iter()
                        .map(|e| EdgeId::from_index(*e as u32))
                        .collect();
                    p6::copy_edges(&k, &b, &edges, &mut destination).map(|copied| {
                        body = if i.flags & 2 != 0 {
                            vec![copied.len() as i64]
                        } else {
                            vec![]
                        };
                    })
                }
                2 => {
                    let surface = (i.flags & 4 == 0).then(|| {
                        Surface::from(Plane::from_normal(
                            Vec3::new(2.0, -3.0, 5.0),
                            Vec3::new(0.0, 0.0, 1.0),
                        ))
                    });
                    let options = FaceTopologyOptions {
                        calculate_trims: i.flags & 1 != 0,
                        verify_radial_sectors: i.flags & 16 != 0,
                        ..FaceTopologyOptions::default()
                    };
                    p6::make_face_topology(
                        &mut b,
                        surface,
                        if i.flags & 8 == 0 {
                            OrientType::Same
                        } else {
                            OrientType::Opposite
                        },
                        ObjectKey(0x6000_0000),
                        &options,
                    )
                    .map(|f| {
                        body = vec![
                            1,
                            i64::from(f.shell.is_some()),
                            i64::from(f.region.is_some()),
                        ];
                    })
                }
                _ => return Err(harness("invalid operation")),
            };
            if let Err(error) = result {
                if let Some(output) = Output::unported(&error) {
                    if let Some(lock) = &mut outer {
                        lock.release(std::slice::from_mut(&mut k.context))
                            .map_err(harness)?;
                    }
                    return Ok(output);
                }
                if let SolidError::Upstream { code, .. } = error {
                    status = i64::from(code);
                    if request.op == 2 {
                        body = vec![0, 0, 0];
                    }
                    break;
                }
                return Err(harness(error));
            }
        }
        let mut output = vec![status];
        output.extend(body);
        for (mt, previous) in MARKS.into_iter().zip(before) {
            output.push(k.context.current_mark(mt).wrapping_sub(previous) as i64);
            output.push(i64::from(k.context.is_mark_locked(mt)));
            for fu in b.faceuses().ids().take(c[2]) {
                output.push(i64::from(marks.is_marked(&k, Entity::Faceuse(fu), mt)));
            }
        }
        for (_, fu) in b.faceuses().iter().take(c[2]) {
            output.push(fu.shell.map_or(-1, |s| original_index(s.index(), c[1])));
        }
        output.extend([
            b.faces().len() as i64,
            b.edges().len() as i64,
            b.vertices().len() as i64,
            destination.edges().len() as i64,
            destination.regions().len() as i64,
            destination
                .infinite_region()
                .map_or(-1, |r| i64::from(r.index())),
            i64::from(destination.region(infinite).map_err(harness)?.is_void()),
        ]);
        if let Some(lock) = &mut outer {
            lock.release(std::slice::from_mut(&mut k.context))
                .map_err(harness)?;
        }
        Ok(Output::new().ints("state", output))
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = shared::gen_brep(rng);
        let op = match &brep {
            BrepIn::Network | BrepIn::Wire { .. } => 1 + rng.below(2),
            _ => rng.below(3),
        } as i32;
        let edges = if op == 1 && rng.chance(0.7) {
            (0..=rng.below(3)).map(|_| rng.below(20) as i64).collect()
        } else {
            vec![]
        };
        let i = Inputs {
            brep,
            op,
            seed: rng.below(20) as i64,
            edges,
            repeat: 1 + rng.below(2) as i32,
            lock_mask: *rng.choose(&[0, 1, 2, 4, 8, 16])?,
            flags: rng.below(32) as i32,
        };
        serde_json::to_value(i).ok()
    }
}
