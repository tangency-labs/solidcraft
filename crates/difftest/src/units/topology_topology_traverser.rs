//! Differential topology traversal on real SMLib graphs. Fixture construction and
//! raw arena import reuse topology.vertexuse; no traversal result is imported.
use super::topology_vertexuse::{self as shared, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::context::MarkLock;
use solidcore::kernel::Kernel;
use solidcore::topology::topology_traverser::{self as tr, Entity, Marks};
use solidcore::topology::{ArenaId, EdgeId, EdgeuseId, FaceId, FaceuseId, ShellId, VertexId};
use solidcore::SolidError;
use solidcore_ffi::units::topology_topology_traverser::{self as ffi, Request};
use solidcore_ffi::units::topology_vertexuse as fixture;

/// Differential runner for the owned topology traversals.
pub struct Runner;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Inputs {
    brep: BrepIn,
    op: i32,
    start: i64,
    target: i64,
    mask: i32,
    want: i32,
    pre: i32,
    prefill: bool,
    repeat: i32,
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

fn export(i: &Inputs) -> Result<ffi::Fixture, HarnessError> {
    ffi::export(&shared::path_of(&i.brep)?, &block(i)?)
        .map_err(|e| HarnessError::new(e.to_string()))
}

fn request(i: &Inputs, c: [usize; 10]) -> Request {
    let kind = match i.op {
        0 => 3,
        1 => 7,
        2 | 3 | 5 => 2,
        4 => 9,
        6 => 1,
        _ => 0,
    };
    Request {
        op: i.op,
        start: shared::sel(i.start, c.get(kind).copied().unwrap_or(0)),
        target: shared::sel(i.target, c[1]),
        mask: i.mask,
        want: i.want,
        pre: i.pre,
        prefill: i.prefill,
        repeat: i.repeat,
    }
}

fn list<I: ArenaId>(out: &mut Vec<i64>, ids: &[I]) {
    out.push(ids.len() as i64);
    out.extend(ids.iter().map(|x| i64::from(x.index())));
}

fn seed<I: ArenaId>(n: usize, prefill: bool) -> Vec<I> {
    if n > 0 && prefill {
        vec![I::from_index(0)]
    } else {
        Vec::new()
    }
}

fn available(r: Request) -> bool {
    (r.op <= 1 || r.op == 7 || r.start >= 0)
        && (!(4..=7).contains(&r.op) || r.target >= 0)
        && (r.op != 6 || r.start != r.target)
}

// Generator eligibility is stricter than `available`: explicit fixtures may use
// null starts, while random cases should exercise entities the fixture contains.
fn eligible_ops(c: [usize; 10]) -> Vec<i32> {
    (0..8)
        .filter(|op| match op {
            0 => c[3] > 0,
            1 => c[7] > 0,
            2 | 3 => c[2] > 0,
            4 => c[9] > 0 && c[1] > 0,
            5 | 7 => c[2] > 0 && c[1] > 0,
            6 => c[1] > 1,
            _ => false,
        })
        .collect()
}

fn select_generated_op(i: &mut Inputs, counts: [usize; 10], rng: &mut Rng) {
    let ops = eligible_ops(counts);
    if let Some(&op) = rng.choose(&ops) {
        i.op = op;
        if op == 6 {
            let shells = counts[1] as u64;
            i.start = rng.below(shells) as i64;
            i.target = (i.start + 1 + rng.below(shells - 1) as i64) % shells as i64;
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.topology_traverser"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let ex = export(&i)?;
        let Some(c) = fixture::counts(&ex.graph) else {
            return Ok(Output::new().values("build", ex.graph));
        };
        let r = request(&i, c);
        if !available(r) {
            return Ok(Output::err("no seed of requested kind"));
        }
        ffi::eval(&shared::path_of(&i.brep)?, &block(&i)?, r)
            .map(|v| {
                Output::new().ints(
                    "traversal",
                    v.into_iter().map(|x| x as i64).collect::<Vec<_>>(),
                )
            })
            .map_err(|e| HarnessError::new(e.to_string()))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let ex = export(&i)?;
        let Some(c) = fixture::counts(&ex.graph) else {
            return Ok(Output::new().values("build", ex.graph));
        };
        let r = request(&i, c);
        if !available(r) {
            return Ok(Output::err("no seed of requested kind"));
        }
        let (mut b, _) = shared::import(&ex.graph)?;
        for (index, list) in ex.vertexuses.iter().enumerate() {
            b.vertex_mut(VertexId::from_index(index as u32))
                .map_err(|e| HarnessError::new(e.to_string()))?
                .vertexuses = list
                .iter()
                .copied()
                .map(solidcore::topology::VertexuseId::from_index)
                .collect();
        }
        let mut k = Kernel::default();
        let lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, r.mask)
            .map_err(|e| HarnessError::new(e.to_string()))?;
        let mt = lock.mark_type();
        let mut marks = Marks::default();
        let start_face = (r.start >= 0).then(|| FaceId::from_index(r.start as u32));
        let start_edge = (r.start >= 0).then(|| EdgeId::from_index(r.start as u32));
        let start_fu = FaceuseId::from_index(r.start as u32);
        let target = ShellId::from_index(r.target as u32);
        if r.pre & 1 != 0 {
            let entity = match r.op {
                0 => start_face.map(Entity::Face),
                1 => start_edge.map(Entity::Edge),
                2 | 3 | 5 if r.start >= 0 => Some(Entity::Faceuse(start_fu)),
                _ => None,
            };
            if let Some(entity) = entity {
                marks.mark(&k, entity, mt);
            }
        }
        for (bit, n, entity) in [
            (2, c[7], Entity::Edge(EdgeId::from_index(0))),
            (4, c[9], Entity::Vertex(VertexId::from_index(0))),
            (8, c[2], Entity::Faceuse(FaceuseId::from_index(0))),
            (
                16,
                c[8],
                Entity::Vertexuse(solidcore::topology::VertexuseId::from_index(0)),
            ),
        ] {
            if r.pre & bit != 0 && n > 0 {
                marks.mark(&k, entity, mt);
            }
        }
        let mut fs: Vec<FaceId> = seed(c[3], r.prefill);
        let mut es: Vec<EdgeId> = seed(c[7], r.prefill);
        let mut vs: Vec<VertexId> = seed(c[9], r.prefill);
        let mut fus: Vec<FaceuseId> = seed(c[2], r.prefill);
        let mut eus: Vec<EdgeuseId> = seed(c[6], r.prefill);
        let mut closed = 2i64; // UNSURE
        let mut status = 0;
        for _ in 0..r.repeat {
            let result = match r.op {
                0 => tr::collect_faces(
                    &k,
                    &b,
                    &mut marks,
                    start_face,
                    mt,
                    &mut fs,
                    (r.want & 1 != 0).then_some(&mut es),
                    (r.want & 2 != 0).then_some(&mut vs),
                ),
                1 => tr::collect_wire_edges(
                    &k,
                    &b,
                    &mut marks,
                    start_edge,
                    mt,
                    &mut es,
                    (r.want & 2 != 0).then_some(&mut vs),
                ),
                2 => tr::faceuse_traversal(
                    &k,
                    &b,
                    &mut marks,
                    start_fu,
                    mt,
                    (r.want & 1 != 0).then_some(&mut fus),
                )
                .map(|v| closed = i64::from(v)),
                3 => tr::closure_traversal(
                    &mut k,
                    &b,
                    &mut marks,
                    start_fu,
                    &lock,
                    (r.want & 1 != 0).then_some(&mut fus),
                )
                .map(|v| closed = i64::from(v)),
                4 => tr::shell_traversal(
                    &mut k,
                    &b,
                    &mut marks,
                    VertexId::from_index(r.start as u32),
                    target,
                    (r.want & 1 != 0).then_some(&mut eus),
                    (r.want & 2 != 0).then_some(&mut fus),
                    &lock,
                ),
                5 => tr::faceuse_shell_replacement(
                    &mut k, &mut b, &mut marks, start_fu, target, &mut fus, &lock,
                ),
                6 => tr::remove_shell(&mut k, &mut b, ShellId::from_index(r.start as u32), target),
                _ => {
                    let faceuses = b
                        .faceuses()
                        .iter()
                        .filter_map(|(id, fu)| (fu.shell == Some(target)).then_some(id))
                        .collect::<Vec<_>>();
                    tr::is_inner_shell(&mut k, &mut b, &faceuses).map(|v| closed = i64::from(v))
                }
            };
            if let Err(e) = result {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                match e {
                    SolidError::Upstream { code, .. } => status = code,
                    _ => return Err(HarnessError::new(e.to_string())),
                }
                break;
            }
        }
        let mut out = vec![i64::from(status)];
        if [2, 3, 7].contains(&r.op) {
            out.push(closed);
        }
        match r.op {
            0 => {
                list(&mut out, &fs);
                if r.want & 1 != 0 {
                    list(&mut out, &es);
                }
                if r.want & 2 != 0 {
                    list(&mut out, &vs);
                }
            }
            1 => {
                list(&mut out, &es);
                if r.want & 2 != 0 {
                    list(&mut out, &vs);
                }
            }
            2 | 3 if r.want & 1 != 0 => list(&mut out, &fus),
            4 => {
                if r.want & 1 != 0 {
                    list(&mut out, &eus);
                }
                if r.want & 2 != 0 {
                    list(&mut out, &fus);
                }
            }
            5 => list(&mut out, &fus),
            _ => (),
        }
        let entities = b
            .faces()
            .ids()
            .map(Entity::Face)
            .chain(b.edges().ids().map(Entity::Edge))
            .chain(b.vertices().ids().map(Entity::Vertex))
            .chain(b.faceuses().ids().map(Entity::Faceuse))
            .chain(b.vertexuses().ids().map(Entity::Vertexuse));
        for entity in entities {
            out.push(i64::from(marks.is_marked(&k, entity, mt)));
        }
        for (_, fu) in b.faceuses().iter() {
            out.push(fu.shell.map_or(-1, |s| i64::from(s.index())));
        }
        Ok(Output::new().ints("traversal", out))
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let mut i = Inputs {
            brep: shared::gen_brep(rng),
            op: rng.below(8) as i32,
            start: rng.below(10) as i64,
            target: rng.below(5) as i64,
            mask: 1_i32 << rng.below(5),
            want: rng.below(4) as i32,
            pre: rng.below(32) as i32,
            prefill: rng.below(2) != 0,
            repeat: 1 + rng.below(2) as i32,
        };
        // Failed exports and empty graphs keep their original case so errors
        // remain visible. Never stop generation or discard a difficult input.
        if let Some(counts) = export(&i).ok().and_then(|ex| fixture::counts(&ex.graph)) {
            select_generated_op(&mut i, counts, rng);
        }
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_requires_nonempty_entity_kinds() {
        assert!(eligible_ops([0; 10]).is_empty());
        assert_eq!(eligible_ops([1, 1, 0, 0, 0, 0, 2, 1, 2, 2]), vec![1, 4]);
        assert_eq!(
            eligible_ops([1, 1, 2, 1, 2, 1, 8, 4, 8, 4]),
            vec![0, 1, 2, 3, 4, 5, 7]
        );
        assert_eq!(
            eligible_ops([1, 2, 4, 2, 4, 2, 16, 8, 16, 8]),
            (0..8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generated_shell_removal_uses_distinct_shells() {
        let mut rng = Rng::new(7);
        for shells in [2, 3] {
            let counts = [1, shells, 0, 0, 0, 0, 0, 0, 0, 0];
            for _ in 0..100 {
                let mut i = Inputs {
                    brep: BrepIn::Network,
                    op: 0,
                    start: 0,
                    target: 0,
                    mask: 1,
                    want: 3,
                    pre: 0,
                    prefill: false,
                    repeat: 1,
                };
                select_generated_op(&mut i, counts, &mut rng);
                let r = request(&i, counts);
                assert_eq!(r.op, 6);
                assert_ne!(r.start, r.target);
                assert!(available(r));
            }
        }
    }

    #[test]
    fn empty_graph_keeps_the_original_case() {
        let mut i = Inputs {
            brep: BrepIn::Network,
            op: 6,
            start: 8,
            target: 3,
            mask: 2,
            want: 1,
            pre: 4,
            prefill: true,
            repeat: 2,
        };
        let before = serde_json::to_value(&i).unwrap();
        select_generated_op(&mut i, [0; 10], &mut Rng::new(7));
        assert_eq!(serde_json::to_value(&i).unwrap(), before);
    }
}
