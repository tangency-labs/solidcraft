//! Wire/shell-vertex boolean stage on identical raw input topology. Construction
//! uses the shared real-B-rep fixture builder; no C++ merge results enter Rust.
use super::boolean_merge_p1::Canon;
use super::topology_vertexuse::{self as shared, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::boolean::merge::p1::VertexCopy;
use solidcore::boolean::merge::p2::TopologyCopy;
use solidcore::boolean::merge::p5::process_wires_and_shell_vertices;
use solidcore::boolean::topology_intersector::p1::Relations;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::MarkLock;
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::topology::topology::Topology;
use solidcore::topology::topology_traverser::{Entity, Marks};
use solidcore::topology::{edge, vertex, vertexuse, Brep, EdgeId, VertexId, VertexuseId};
use solidcore_ffi::units::boolean_merge_p5::{self as ffi, Operand, Request};
use std::collections::BTreeMap;

/// Differential runner for `SmMerge::ProcessWiresAndShellVertices`.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Spec {
    Empty,
    Vertex { point: [f64; 3] },
    Graph { brep: BrepIn },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    primary: Spec,
    other: Spec,
    operation: i32,
    /// Bit 1 marks edges, bit 2 marks vertices.
    #[serde(default)]
    primary_pre: u8,
    #[serde(default)]
    other_pre: u8,
    #[serde(default)]
    primary_unmark: Vec<usize>,
    #[serde(default)]
    other_unmark: Vec<usize>,
    #[serde(default)]
    mates: Option<Vec<i64>>,
    #[serde(default = "default_marks")]
    marks: [i32; 2],
    #[serde(default)]
    prefill: bool,
    #[serde(default = "one")]
    repeat: i32,
}
fn default_marks() -> [i32; 2] {
    [1, 2]
}
fn one() -> i32 {
    1
}
fn err(e: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(e.to_string())
}

pub(crate) fn program(s: &Spec) -> Result<(String, Vec<f64>), HarnessError> {
    let mut block = Vec::new();
    let path = match s {
        Spec::Empty => {
            block.push(5.0);
            String::new()
        }
        Spec::Vertex { point } => {
            block.push(4.0);
            block.extend(point);
            String::new()
        }
        Spec::Graph { brep } => {
            match brep {
                BrepIn::File { rebuild, .. } => block.extend([0.0, f64::from(*rebuild)]),
                BrepIn::Wire { curve, t0, t1 } => {
                    block.push(1.0);
                    shared::push_curve(&mut block, curve)?;
                    block.extend([t0, t1]);
                }
                BrepIn::Face { surface, dom } => {
                    block.push(2.0);
                    shared::push_surface(&mut block, surface)?;
                    block.extend(dom);
                }
                BrepIn::Network => block.push(3.0),
            }
            block.extend([0.0, 0.0]);
            shared::path_of(brep)?
        }
    };
    Ok((path, block))
}
pub(crate) fn operand(p: &(String, Vec<f64>)) -> Operand<'_> {
    Operand {
        path: &p.0,
        block: &p.1,
    }
}
fn flags(c: [usize; 10], pre: u8, unmark: &[usize]) -> Vec<i64> {
    (0..c[7] + c[9])
        .map(|i| i64::from(pre & if i < c[7] { 1 } else { 2 } != 0 && !unmark.contains(&i)))
        .collect()
}
fn mapping(i: &Inputs, a: [usize; 10], b: [usize; 10]) -> Vec<i64> {
    i.mates.clone().unwrap_or_else(|| {
        (0..b[7])
            .map(|j| if j < a[7] { j as i64 } else { -1 })
            .collect()
    })
}
pub(crate) fn import(
    f: &solidcore_ffi::units::topology_topology_traverser::Fixture,
) -> Result<Brep, HarnessError> {
    let (mut b, _) = shared::import(&f.graph)?;
    for (i, list) in f.vertexuses.iter().enumerate() {
        b.vertex_mut(VertexId::from_index(i as u32))
            .map_err(|e| err(&e))?
            .vertexuses = list.iter().map(|&j| VertexuseId::from_index(j)).collect();
    }
    Ok(b)
}
fn seed(
    k: &Kernel,
    b: &Brep,
    marks: &mut Marks,
    mt: solidcore::geom::context::MarkType,
    f: &[i64],
) {
    for (i, e) in b.edges().ids().enumerate() {
        if f.get(i) == Some(&1) {
            marks.mark(k, Entity::Edge(e), mt);
        }
    }
    for (i, v) in b.vertices().ids().enumerate() {
        if f.get(b.edges().len() + i) == Some(&1) {
            marks.mark(k, Entity::Vertex(v), mt);
        }
    }
}
/// The B-rep's topology in the shim's numbering ([`Canon::of_native`]: the edge and
/// vertex lists, then the walk from the regions), so records an operation creates
/// compare by their place in the topology rather than by arena allocation order.
#[allow(clippy::too_many_lines)]
pub(crate) fn snapshot(
    k: &Kernel,
    b: &Brep,
    marks: &Marks,
    mt: solidcore::geom::context::MarkType,
    ids: &mut Vec<i64>,
    values: &mut Vec<f64>,
) -> Result<(), HarnessError> {
    use solidcore::topology::{EdgeuseOwner, LoopuseStart, VertexuseOwner};
    let c = Canon::of_native(b);
    ids.extend(
        [
            b.regions().len(),
            b.shells().len(),
            b.faceuses().len(),
            b.faces().len(),
            b.loopuses().len(),
            b.loops().len(),
            b.edgeuses().len(),
            b.edges().len(),
            b.vertexuses().len(),
            b.vertices().len(),
        ]
        .map(|n| n as i64),
    );
    for &e in &c.edges.items {
        let r = b.edge(e).map_err(|e| err(&e))?;
        let v = edge::vertex(b, e).map_err(|e| err(&e))?;
        ids.extend([
            c.vertices.of(v),
            c.vertices
                .of(edge::other_vertex(b, e, v).map_err(|e| err(&e))?),
            i64::from(vertexuse::edge_is_wire(b, e).map_err(|e| err(&e))?),
            i64::from(marks.is_marked(k, Entity::Edge(e), mt)),
            r.edgeuses.len() as i64,
        ]);
        ids.extend(r.edgeuses.iter().map(|&x| c.edgeuses.of(Some(x))));
        values.extend([r.interval.min(), r.interval.max(), r.tolerance]);
    }
    for &v in &c.vertices.items {
        let r = b.vertex(v).map_err(|e| err(&e))?;
        ids.extend([
            i64::from(vertex::is_shell_vertex(b, v).map_err(|e| err(&e))?),
            i64::from(marks.is_marked(k, Entity::Vertex(v), mt)),
            r.vertexuses.len() as i64,
        ]);
        ids.extend(r.vertexuses.iter().map(|&x| c.vertexuses.of(Some(x))));
        values.extend([r.point.x, r.point.y, r.point.z, r.tolerance]);
    }
    for &x in &c.regions.items {
        ids.push(i64::from(b.region(x).map_err(|e| err(&e))?.in_brep));
    }
    for &x in &c.shells.items {
        ids.push(c.regions.of(b.shell(x).map_err(|e| err(&e))?.region));
    }
    for &x in &c.faces.items {
        let f = b.face(x).map_err(|e| err(&e))?;
        ids.push(c.faceuses.of(f.faceuse));
        values.extend([
            f.tolerance,
            f.uv_domain.min().x,
            f.uv_domain.min().y,
            f.uv_domain.max().x,
            f.uv_domain.max().y,
        ]);
    }
    for &x in &c.faceuses.items {
        let fu = b.faceuse(x).map_err(|e| err(&e))?;
        ids.extend([
            c.shells.of(fu.shell),
            c.faces.of(fu.face),
            c.faceuses.of(fu.mate),
            i64::from(fu.orientation as i32),
            fu.loopuses.len() as i64,
        ]);
        ids.extend(fu.loopuses.iter().map(|&l| c.loopuses.of(Some(l))));
    }
    for &x in &c.loops.items {
        ids.push(c.loopuses.of(b.loop_(x).map_err(|e| err(&e))?.loopuse));
    }
    for &x in &c.loopuses.items {
        let lu = b.loopuse(x).map_err(|e| err(&e))?;
        ids.extend([
            c.faceuses.of(lu.faceuse),
            c.loops.of(lu.loop_),
            c.loopuses.of(lu.mate),
            i64::from(lu.orientation as i32),
        ]);
        ids.extend(match lu.start {
            LoopuseStart::Unknown => [0, -1],
            LoopuseStart::Edgeuse(e) => [1, c.edgeuses.of(e)],
            LoopuseStart::Vertexuse(v) => [2, c.vertexuses.of(v)],
        });
    }
    for &x in &c.edgeuses.items {
        let eu = b.edgeuse(x).map_err(|e| err(&e))?;
        ids.extend([
            c.edges.of(eu.edge),
            c.vertexuses.of(eu.vertexuse),
            c.edgeuses.of(eu.cw),
            c.edgeuses.of(eu.ccw),
        ]);
        ids.extend(match eu.owner {
            EdgeuseOwner::Unknown => [0, -1],
            EdgeuseOwner::Loopuse(l) => [1, c.loopuses.of(l)],
            EdgeuseOwner::Shell(sh) => [2, c.shells.of(sh)],
        });
        ids.push(i64::from(eu.orientation as i32));
    }
    for &x in &c.vertexuses.items {
        let vu = b.vertexuse(x).map_err(|e| err(&e))?;
        ids.push(c.vertices.of(vu.vertex));
        ids.extend(match vu.owner {
            VertexuseOwner::Unknown => [0, -1],
            VertexuseOwner::Edgeuse(e) => [1, c.edgeuses.of(e)],
            VertexuseOwner::Loopuse(l) => [2, c.loopuses.of(l)],
            VertexuseOwner::Shell(sh) => [3, c.shells.of(sh)],
        });
    }
    Ok(())
}
fn output(raw: &[f64]) -> Result<Output, HarnessError> {
    let n = *raw.first().ok_or_else(|| err("empty result"))? as usize;
    let end = n.checked_add(1).ok_or_else(|| err("output overflow"))?;
    let ints: Vec<i64> = raw
        .get(1..end)
        .ok_or_else(|| err("truncated topology"))?
        .iter()
        .map(|&x| x as i64)
        .collect();
    let values = raw
        .get(end..)
        .ok_or_else(|| err("truncated values"))?
        .to_vec();
    Ok(Output::new()
        .ints("topology", ints)
        .values("geometry", values))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.merge_p5"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let a = program(&i.primary)?;
        let b = program(&i.other)?;
        let aa = ffi::export(operand(&a)).map_err(|e| err(&e))?;
        let bb = ffi::export(operand(&b)).map_err(|e| err(&e))?;
        let ac = solidcore_ffi::units::topology_vertexuse::counts(&aa.graph)
            .ok_or_else(|| err("primary graph"))?;
        let bc = solidcore_ffi::units::topology_vertexuse::counts(&bb.graph)
            .ok_or_else(|| err("other graph"))?;
        let am = flags(ac, i.primary_pre, &i.primary_unmark);
        let bm = flags(bc, i.other_pre, &i.other_unmark);
        let mates = mapping(&i, ac, bc);
        ffi::validate_mates(&mates, ac[7], bc[7]).map_err(|e| err(&e))?;
        let r = Request {
            operation: i.operation,
            marks: i.marks,
            primary_marked: &am,
            other_marked: &bm,
            mates: &mates,
            prefill: i.prefill,
            repeat: i.repeat,
        };
        output(&ffi::eval(operand(&a), operand(&b), r).map_err(|e| err(&e))?)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let a = program(&i.primary)?;
        let b = program(&i.other)?;
        let aa = ffi::export(operand(&a)).map_err(|e| err(&e))?;
        let bb = ffi::export(operand(&b)).map_err(|e| err(&e))?;
        let ac = solidcore_ffi::units::topology_vertexuse::counts(&aa.graph)
            .ok_or_else(|| err("primary graph"))?;
        let bc = solidcore_ffi::units::topology_vertexuse::counts(&bb.graph)
            .ok_or_else(|| err("other graph"))?;
        let edge_mates = mapping(&i, ac, bc);
        ffi::validate_mates(&edge_mates, ac[7], bc[7]).map_err(|e| err(&e))?;
        let mut a = import(&aa)?;
        let mut b = import(&bb)?;
        let mut k = Kernel::default();
        let la = MarkLock::new(std::slice::from_mut(&mut k.context), 0, i.marks[0])
            .map_err(|e| err(&e))?;
        let lb = if i.marks[0] == i.marks[1] {
            None
        } else {
            Some(
                MarkLock::new(std::slice::from_mut(&mut k.context), 0, i.marks[1])
                    .map_err(|e| err(&e))?,
            )
        };
        let ma = la.mark_type();
        let mb = lb.as_ref().map_or(ma, MarkLock::mark_type);
        let mut am = Marks::default();
        let mut bm = Marks::default();
        seed(
            &k,
            &a,
            &mut am,
            ma,
            &flags(ac, i.primary_pre, &i.primary_unmark),
        );
        seed(
            &k,
            &b,
            &mut bm,
            mb,
            &flags(bc, i.other_pre, &i.other_unmark),
        );
        let mut relations = Relations::default();
        // Fixture setup mirrors SmTopologyIntersector::Relate, including the
        // tolerance propagation it performs BEFORE the unit under test is called.
        for (j, m) in edge_mates.into_iter().enumerate() {
            if m < 0 {
                continue;
            }
            let e = EdgeId::from_index(m as u32);
            let oe = EdgeId::from_index(j as u32);
            let at = a.edge(e).map_err(|e| err(&e))?.tolerance;
            let bt = b.edge(oe).map_err(|e| err(&e))?.tolerance;
            if at < bt {
                edge::set_tolerance(&mut a, e, bt, true, true).map_err(|e| err(&e))?;
            }
            if bt < at {
                edge::set_tolerance(&mut b, oe, at, true, true).map_err(|e| err(&e))?;
            }
            relations.edges.record(e, oe).map_err(|e| err(&e))?;
        }
        // A fresh SmTopology record per vertex and edge of both operands, under the
        // identities the copies are notified with (a copy propagates its flags).
        let mut records = BTreeMap::new();
        for (base, brep) in [(0, &a), (1_000_000, &b)] {
            for v in brep.vertices().ids() {
                records.insert(ObjectKey(base + u64::from(v.index())), Topology::new());
            }
            for e in brep.edges().ids() {
                records.insert(
                    ObjectKey(2_000_000 + base + u64::from(e.index())),
                    Topology::new(),
                );
            }
        }
        let mut attributes = AttributeStore::new();
        let mut copy = TopologyCopy {
            vertices: VertexCopy {
                records: &mut records,
                attributes: &mut attributes,
                source_key: &|v| ObjectRef {
                    key: ObjectKey(1_000_000 + u64::from(v.index())),
                    type_code: 16021,
                },
                destination_key: &|e| match e {
                    solidcore::EntityRef::Vertex(v) => ObjectRef {
                        key: ObjectKey(u64::from(v.index())),
                        type_code: 16021,
                    },
                    solidcore::EntityRef::Vertexuse(v) => ObjectRef {
                        key: ObjectKey(8_000_000 + u64::from(v.index())),
                        type_code: 16006,
                    },
                    _ => ObjectRef {
                        key: ObjectKey(9_000_000),
                        type_code: 16001,
                    },
                },
            },
            edge_source_key: &|e| ObjectRef {
                key: ObjectKey(3_000_000 + u64::from(e.index())),
                type_code: 16016,
            },
            edge_destination_key: &|e| ObjectRef {
                key: ObjectKey(2_000_000 + u64::from(e.index())),
                type_code: 16016,
            },
            face_source_key: &|f| ObjectRef {
                key: ObjectKey(5_000_000 + u64::from(f.index())),
                type_code: 16002,
            },
            face_destination_key: &|f| ObjectRef {
                key: ObjectKey(4_000_000 + u64::from(f.index())),
                type_code: 16002,
            },
        };
        let operation =
            BooleanOperation::from_code(i.operation).ok_or_else(|| err("boolean operation"))?;
        let mut keep = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..i.repeat {
            if i.prefill && ac[7] > 0 {
                keep.push(EdgeId::from_index(0));
            }
            match process_wires_and_shell_vertices(
                &k,
                &mut a,
                &mut b,
                &mut am,
                &mut bm,
                &mut relations,
                &mut copy,
                None,
                operation,
                ma,
                mb,
                &mut keep,
            ) {
                Ok(()) => {
                    ids.extend([0, keep.len() as i64]);
                    ids.extend(keep.iter().map(|e| i64::from(e.index())));
                }
                Err(e) => return Output::unported(&e).ok_or_else(|| err(&e)),
            }
        }
        let mut values = Vec::new();
        snapshot(&k, &a, &am, ma, &mut ids, &mut values)?;
        snapshot(&k, &b, &bm, mb, &mut ids, &mut values)?;
        Ok(Output::new()
            .ints("topology", ids)
            .values("geometry", values))
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = *rng.choose(&[1.0e-9, 1.0, 1.0e9])?;
        let (primary, _) = generated_operand(rng, scale);
        let (other, other_wire) = generated_operand(rng, scale);
        // Topology comes from construction only, before the operation. Count
        // actual edges because tiny face domains can collapse during setup.
        let edge_count = |v: &Value| {
            let spec: Spec = serde_json::from_value(v.clone()).ok()?;
            let p = program(&spec).ok()?;
            let f = ffi::export(operand(&p)).ok()?;
            solidcore_ffi::units::topology_vertexuse::counts(&f.graph).map(|c| c[7])
        };
        let primary_edges = edge_count(&primary)?;
        let other_edges = edge_count(&other)?;
        let operation = rng.below(11);
        let primary_pre = if rng.below(2) == 0 { 3 } else { rng.below(4) };
        let mut other_pre = if rng.below(2) == 0 { 3 } else { rng.below(4) };
        let keeps_mates = matches!(operation, 1 | 2 | 5 | 6);
        // A null keep-list entry has no Rust representation. Exercise unmapped
        // wires through classification, or through the seven non-keep gates.
        if keeps_mates && other_wire && primary_edges < other_edges {
            other_pre &= !1;
        }
        let mut targets: Vec<i64> = (0..primary_edges as i64).collect();
        for j in (1..targets.len()).rev() {
            let at = rng.below((j + 1) as u64) as usize;
            targets.swap(j, at);
        }
        let mates: Vec<i64> = (0..other_edges)
            .map(|j| {
                if (!keeps_mates || other_pre & 1 == 0) && rng.below(2) == 0 {
                    -1
                } else {
                    targets.get(j).copied().unwrap_or(-1)
                }
            })
            .collect();
        let mask_pairs = [[1, 2], [4, 8], [8, 16], [2, 4], [16, 1], [1, 1]];
        Some(
            json!({"primary":primary,"other":other,"operation":operation,
            "primary_pre":primary_pre,"other_pre":other_pre,"mates":mates,
            "marks":rng.choose(&mask_pairs)?,"prefill":rng.below(2)==0,"repeat":1+rng.below(2)}),
        )
    }
}

pub(crate) fn generated_operand(rng: &mut Rng, scale: f64) -> (Value, bool) {
    match rng.below(5) {
        0 => (json!({"kind":"empty"}), false),
        1 => (
            json!({"kind":"vertex","point":rng.point3(-scale,scale)}),
            false,
        ),
        2 => (json!({"kind":"graph","brep":{"kind":"network"}}), true),
        3 => {
            let extent = scale.min(1.0e3);
            (
                json!({"kind":"graph","brep":{"kind":"face","surface":{"plane":{"o":rng.point3(-scale,scale),"n":[0.0,0.0,1.0]}},"dom":[-extent,-extent,extent,extent]}}),
                false,
            )
        }
        _ => {
            let p = rng.point3(-scale, scale);
            let q = [p[0] + scale, p[1] + scale, p[2] + scale];
            (
                json!({"kind":"graph","brep":{"kind":"wire","curve":{"dim":3,"cpts":[[p[0],p[1],p[2],-f64::MAX],[q[0],q[1],q[2],-f64::MAX]],"p":1,"u":[0.0,0.0,1.0,1.0]},"t0":0.2,"t1":0.8}}),
                true,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_mates_are_rejected_by_both_engines() {
        let case = Case {
            unit: "boolean.merge_p5".into(),
            name: "conflicting_relate".into(),
            source: crate::CaseSource::Inline,
            inputs: json!({"primary":{"kind":"graph","brep":{"kind":"network"}},
                "other":{"kind":"graph","brep":{"kind":"network"}},"operation":5,
                "primary_pre":3,"other_pre":3,"mates":[0,0,1,2,3]}),
        };
        assert!(Runner.run_cpp(&case).is_err());
        assert!(Runner.run_rust(&case).is_err());
    }
}
