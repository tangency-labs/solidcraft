//! Runner for unit `geom.tree`: spatial object trees and subdivision tree node
//! relations.
//!
//! C++: `solidcore_ffi::units::geom_tree` (upstream `source/SMLib/src/SmTree.cpp`).
//! Rust: `solidcore::geom::tree`.
//!
//! A case is a small program run against one tree on both sides:
//!
//! | `kind`    | constructor                          | ops |
//! |-----------|--------------------------------------|-----|
//! | `spatial` | `SmTree(extent)` / `SmTree(point, size, NULL, list)` | `add` (`AddToSpatialTree`), `rm` (`RmFromSpatialTree`), `find` (`FindNode`), `in_box` (`GetObjectsInBox`), `intersects` (`IntersectsBox`), `node_list` (`SmTreeNode::GetObjectList`), `dump` (`GetAllTreeNodes`, node fields, `GetBoundingBox`, `SmTree::GetObjectList`), `info` (`GetType`, `IsKindOf`, `GetOwner`, `GetOwnerObject`) |
//! | `subdiv`  | nodes given explicitly (`SmTree(NULL)` + `SmBezierAux2d` data) | `neighbors` (`GetLeft/Right/Bot/TopNeighbors`, `GetNeighbors`), `propagate` (`PropagateToParents`), `set`, `dump`, `first_patch` (`GetFirstBezierPatch`), `owner_object` (`SmTreeNode::GetOwnerObject`) |
//!
//! Outputs: `ints` (statuses, flags, node indices in allocation order, object
//! ids; `-1` = null) and `floats` (boxes). `first_patch` on a path that needs
//! `SmBezierPatch` or a subdivision surface is deferred on the Rust side (the
//! shim does not run it). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
};
use solidcore::math::Vec3;
use solidcore::SolidError;
use solidcore_ffi::units::geom_tree as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `geom.tree` runner.
pub struct Runner;

type P3 = [f64; 3];
type E6 = [f64; 6];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Ctor {
    Spatial { bbox: E6 },
    Point { p: P3, size: f64, with_list: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum SOp {
    Add { bbox: E6, obj: i64, geom: i64 },
    Rm { bbox: E6, obj: i64 },
    Find { bbox: E6 },
    InBox { bbox: E6 },
    Intersects { bbox: E6 },
    Dump,
    NodeList { node: i64 },
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeDesc {
    parent: i64,
    c1: i64,
    c2: i64,
    aux: i64,
    split: i64,
    marked: bool,
    class: i64,
    face: i64,
    surface: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum DOp {
    Neighbors {
        node: i64,
    },
    Propagate {
        node: i64,
    },
    Set {
        node: i64,
        marked: bool,
        class: i64,
        face: i64,
    },
    Dump,
    FirstPatch {
        node: i64,
    },
    OwnerObject {
        node: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inputs {
    Spatial { ctor: Ctor, ops: Vec<SOp> },
    Subdiv { nodes: Vec<NodeDesc>, ops: Vec<DOp> },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn ext(e: E6) -> Extent3d {
    Extent3d::from_bounds(e[0], e[1], e[2], e[3], e[4], e[5])
}

fn put_ext(o: &mut Vec<f64>, e: &Extent3d) {
    let (a, b) = (e.min(), e.max());
    o.extend([a.x, a.y, a.z, b.x, b.y, b.z]);
}

fn obj(id: i64) -> Option<i64> {
    (id >= 0).then_some(id)
}

fn obj_id(o: Option<i64>) -> i64 {
    o.unwrap_or(-1)
}

fn idx(n: Option<NodeId>) -> i64 {
    n.map_or(-1, |n| n.0 as i64)
}

fn flag(b: bool) -> i64 {
    i64::from(b)
}

/// Upstream status of a Rust error.
fn status_of(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } => 1007,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => 1001,
    }
}

fn spatial_prog(ctor: &Ctor, ops: &[SOp]) -> Vec<f64> {
    let mut v = Vec::new();
    match ctor {
        Ctor::Spatial { bbox } => {
            v.push(0.0);
            v.extend(bbox);
        }
        Ctor::Point { p, size, with_list } => {
            v.push(1.0);
            v.extend(p);
            v.push(*size);
            v.push(if *with_list { 1.0 } else { 0.0 });
        }
    }
    for op in ops {
        match op {
            SOp::Add { bbox, obj, geom } => {
                v.push(2.0);
                v.extend(bbox);
                v.push(*obj as f64);
                v.push(*geom as f64);
            }
            SOp::Rm { bbox, obj } => {
                v.push(3.0);
                v.extend(bbox);
                v.push(*obj as f64);
            }
            SOp::Find { bbox } => {
                v.push(4.0);
                v.extend(bbox);
            }
            SOp::InBox { bbox } => {
                v.push(5.0);
                v.extend(bbox);
            }
            SOp::Intersects { bbox } => {
                v.push(6.0);
                v.extend(bbox);
            }
            SOp::Dump => v.push(7.0),
            SOp::NodeList { node } => {
                v.push(8.0);
                v.push(*node as f64);
            }
            SOp::Info => v.push(9.0),
        }
    }
    v
}

fn run_spatial_rust(ctor: &Ctor, ops: &[SOp]) -> solidcore::Result<(Vec<i64>, Vec<f64>)> {
    let mut t: Tree<i64> = match ctor {
        Ctor::Spatial { bbox } => Tree::new_spatial(&ext(*bbox)),
        Ctor::Point { p, size, with_list } => {
            Tree::from_point(Vec3::new(p[0], p[1], p[2]), *size, *with_list)
        }
    };
    let mut oi = Vec::new();
    let mut of = Vec::new();
    for op in ops {
        match op {
            SOp::Add { bbox, obj: o, geom } => {
                let g = NodeGeomType::from_code(*geom as i32).unwrap_or_default();
                match t.add_to_spatial_tree(&ext(*bbox), obj(*o), g) {
                    Ok(()) => oi.push(0),
                    Err(e @ SolidError::Unported { .. }) => return Err(e),
                    Err(e) => oi.push(status_of(&e)),
                }
            }
            SOp::Rm { bbox, obj: o } => {
                oi.push(flag(t.rm_from_spatial_tree(&ext(*bbox), obj(*o))?));
            }
            SOp::Find { bbox } => oi.push(idx(t.find_node(&ext(*bbox))?)),
            SOp::InBox { bbox } => {
                let objs = t.objects_in_box(&ext(*bbox))?;
                oi.push(0);
                oi.push(objs.len() as i64);
                oi.extend(objs.into_iter().map(obj_id));
            }
            SOp::Intersects { bbox } => oi.push(flag(t.intersects_box(&ext(*bbox))?)),
            SOp::Dump => {
                oi.push(idx(t.top_node()));
                let ids = t.all_tree_nodes();
                oi.push(ids.len() as i64);
                for id in ids {
                    let n = *t.node(id)?;
                    oi.extend([
                        idx(n.parent),
                        idx(n.child1),
                        idx(n.child2),
                        i64::from(n.aux_type.code()),
                        i64::from(n.geom_type.code()),
                    ]);
                    put_ext(&mut of, n.bounding_box());
                    let items = if n.aux_type == AuxDataType::ObjectList {
                        t.node_object_list(id)?
                    } else {
                        Vec::new()
                    };
                    oi.push(items.len() as i64);
                    for it in items {
                        let item = t.list(it)?;
                        oi.push(obj_id(item.object));
                        put_ext(&mut of, &item.bbox);
                    }
                }
                let all = t.object_list()?;
                oi.push(0);
                oi.push(all.len() as i64);
                for it in all {
                    let item = t.list(it)?;
                    oi.push(obj_id(item.object));
                    put_ext(&mut of, &item.bbox);
                }
            }
            SOp::NodeList { node } => {
                let k = *node;
                if k < 0 || k as usize >= t.nodes().len() {
                    oi.push(-3);
                } else {
                    match t.node_object_list(NodeId(k as usize)) {
                        Ok(items) => {
                            oi.push(0);
                            oi.push(items.len() as i64);
                            for it in items {
                                oi.push(obj_id(t.list(it)?.object));
                            }
                        }
                        Err(e) => {
                            oi.push(status_of(&e));
                            oi.push(0);
                        }
                    }
                }
            }
            SOp::Info => {
                oi.push(i64::from(t.type_code()));
                oi.push(flag(t.is_kind_of(Tree::<i64>::TYPE)));
                oi.push(flag(t.is_kind_of(11001)));
                oi.push(flag(t.is_kind_of(Tree::<i64>::TYPE + 1)));
                oi.push(flag(t.owner().is_none()));
                oi.push(flag(t.owner_object()?.is_none()));
                let top = t.top_node().ok_or(SolidError::InvalidInput {
                    operation: "difftest::geom_tree",
                    reason: "no top node",
                })?;
                oi.push(flag(t.node_owner_object(top)?.is_none()));
            }
        }
    }
    Ok((oi, of))
}

fn split_dir(code: i64) -> SplitDir {
    match code {
        0 => SplitDir::U,
        1 => SplitDir::V,
        _ => SplitDir::Unknown,
    }
}

fn node_class(code: i64) -> NodeClass {
    match code {
        1 => NodeClass::Inside,
        2 => NodeClass::Outside,
        3 => NodeClass::OnBoundary,
        _ => NodeClass::Unknown,
    }
}

fn link(v: i64) -> Option<NodeId> {
    (v >= 0).then_some(NodeId(v as usize))
}

fn subdiv_desc(nodes: &[NodeDesc]) -> Vec<i64> {
    nodes
        .iter()
        .flat_map(|d| {
            [
                d.parent,
                d.c1,
                d.c2,
                d.aux,
                d.split,
                i64::from(d.marked),
                d.class,
                d.face,
                d.surface,
            ]
        })
        .collect()
}

fn subdiv_prog(ops: &[DOp]) -> Vec<i64> {
    let mut v = Vec::new();
    for op in ops {
        match op {
            DOp::Neighbors { node } => v.extend([0, *node]),
            DOp::Propagate { node } => v.extend([1, *node]),
            DOp::Set {
                node,
                marked,
                class,
                face,
            } => v.extend([2, *node, i64::from(*marked), *class, *face]),
            DOp::Dump => v.push(3),
            DOp::FirstPatch { node } => v.extend([4, *node]),
            DOp::OwnerObject { node } => v.extend([5, *node]),
        }
    }
    v
}

fn run_subdiv_rust(nodes: &[NodeDesc], ops: &[DOp]) -> solidcore::Result<Vec<i64>> {
    let mut t: Tree<i64> = Tree::new(None);
    for d in nodes {
        t.add_node(TreeNode {
            parent: link(d.parent),
            child1: link(d.c1),
            child2: link(d.c2),
            aux_type: AuxDataType::from_code(d.aux as i32).unwrap_or_default(),
            data: NodeData::Surface(SurfaceAux {
                split_dir: split_dir(d.split),
                is_marked: d.marked,
                node_class: node_class(d.class),
                face: obj(d.face),
                surface: obj(d.surface),
                slot: None,
            }),
            ..TreeNode::new()
        });
    }
    t.set_top_node(Some(NodeId(0)));
    let mut o = Vec::new();
    let put = |o: &mut Vec<i64>, l: &[NodeId]| {
        o.push(l.len() as i64);
        o.extend(l.iter().map(|n| n.0 as i64));
    };
    for op in ops {
        match op {
            DOp::Neighbors { node } => {
                let n = NodeId(*node as usize);
                put(&mut o, &t.left_neighbors(n)?);
                put(&mut o, &t.right_neighbors(n)?);
                put(&mut o, &t.bot_neighbors(n)?);
                put(&mut o, &t.top_neighbors(n)?);
                put(&mut o, &t.neighbors(n)?);
            }
            DOp::Propagate { node } => t.propagate_to_parents(NodeId(*node as usize))?,
            DOp::Set {
                node,
                marked,
                class,
                face,
            } => {
                if let NodeData::Surface(a) = &mut t.node_mut(NodeId(*node as usize))?.data {
                    a.is_marked = *marked;
                    a.node_class = node_class(*class);
                    a.face = obj(*face);
                }
            }
            DOp::Dump => {
                for n in t.nodes() {
                    if let NodeData::Surface(a) = &n.data {
                        o.extend([
                            i64::from(a.is_marked),
                            i64::from(a.node_class.code()),
                            obj_id(a.face),
                        ]);
                    }
                }
            }
            DOp::FirstPatch { node } => {
                let p = t.first_bezier_patch(NodeId(*node as usize))?;
                o.push(i64::from(p.is_some()));
            }
            DOp::OwnerObject { node } => {
                o.push(i64::from(
                    t.node_owner_object(NodeId(*node as usize))?.is_some(),
                ));
            }
        }
    }
    Ok(o)
}

fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

fn ffi_err(e: &FfiError) -> HarnessError {
    harness(e.to_string())
}

// ---- generation -------------------------------------------------------

fn rand_box(rng: &mut Rng, lo: f64, hi: f64, max_size: f64) -> E6 {
    let c = rng.point3(lo, hi);
    let kind = rng.below(10);
    let s = |rng: &mut Rng| match kind {
        0 => 0.0,
        1 => rng.uniform(0.0, 1e-9),
        _ => rng.uniform(0.0, max_size),
    };
    let (sx, sy, sz) = (s(rng), s(rng), s(rng));
    [c[0], c[1], c[2], c[0] + sx, c[1] + sy, c[2] + sz]
}

fn gen_spatial(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 10.0, 1000.0, 1e-3]).unwrap_or(&1.0);
    if rng.chance(0.1) {
        let p = rng.point3(-scale, scale);
        let ctor = Ctor::Point {
            p,
            size: rng.uniform(0.0, scale),
            with_list: rng.chance(0.5),
        };
        let mut ops = Vec::new();
        for _ in 0..rng.range_i64(1, 6) {
            let b = rand_box(rng, -scale, scale, scale);
            ops.push(match rng.below(5) {
                0 => SOp::Find { bbox: b },
                1 => SOp::InBox { bbox: b },
                2 => SOp::Intersects { bbox: b },
                3 => SOp::Rm {
                    bbox: b,
                    obj: rng.range_i64(-1, 2),
                },
                _ => SOp::NodeList { node: 0 },
            });
        }
        ops.push(SOp::Dump);
        ops.push(SOp::Info);
        return Inputs::Spatial { ctor, ops };
    }
    // a world box; objects mostly inside, sometimes outside (forced rebuilds)
    let world = [-scale, -scale, -scale, scale, scale, scale];
    let start = if rng.chance(0.2) {
        rand_box(rng, -scale, scale, scale * 0.1)
    } else {
        world
    };
    let ctor = Ctor::Spatial { bbox: start };
    let n_objs = rng.range_i64(1, 120);
    let max_size = scale * *rng.choose(&[0.01, 0.1, 0.5]).unwrap_or(&0.1);
    let clustered = rng.chance(0.2);
    let mut boxes: Vec<E6> = Vec::new();
    let mut ops = Vec::new();
    for i in 0..n_objs {
        let b = if clustered {
            let c = scale * 0.25;
            rand_box(rng, c, c + scale * 1e-4, scale * 1e-5)
        } else if rng.chance(0.05) {
            rand_box(rng, -3.0 * scale, 3.0 * scale, max_size)
        } else {
            rand_box(rng, -scale, scale - max_size, max_size)
        };
        let id = if rng.chance(0.1) && i > 0 {
            rng.range_i64(0, i - 1) // re-add: same object, maybe new box
        } else {
            i
        };
        let o = if rng.chance(0.03) { -1 } else { id };
        ops.push(SOp::Add {
            bbox: b,
            obj: o,
            geom: rng.range_i64(0, 3),
        });
        boxes.push(b);
        if rng.chance(0.05) {
            ops.push(SOp::Dump);
        }
    }
    for _ in 0..rng.range_i64(0, 8) {
        let k = rng.below(boxes.len() as u64) as usize;
        let b = boxes.get(k).copied().unwrap_or(world);
        let o = if rng.chance(0.8) {
            k as i64
        } else {
            rng.range_i64(-1, n_objs + 3)
        };
        ops.push(SOp::Rm { bbox: b, obj: o });
    }
    for _ in 0..rng.range_i64(1, 6) {
        let b = rand_box(rng, -1.5 * scale, 1.5 * scale, scale);
        ops.push(match rng.below(4) {
            0 => SOp::Find { bbox: b },
            1 => SOp::InBox { bbox: b },
            2 => SOp::Intersects { bbox: b },
            _ => SOp::NodeList {
                node: rng.range_i64(0, 12),
            },
        });
    }
    ops.push(SOp::Dump);
    ops.push(SOp::Info);
    Inputs::Spatial { ctor, ops }
}

fn gen_subdiv(rng: &mut Rng) -> Inputs {
    let mut nodes = vec![NodeDesc {
        parent: -1,
        c1: -1,
        c2: -1,
        aux: 4,
        split: 2,
        marked: false,
        class: 0,
        face: -1,
        surface: -1,
    }];
    let target = 1 + 2 * rng.range_i64(0, 30) as usize;
    while nodes.len() < target {
        let leaves: Vec<usize> = (0..nodes.len())
            .filter(|&k| nodes.get(k).is_some_and(|n| n.c1 < 0))
            .collect();
        let Some(&leaf) = rng.choose(&leaves) else {
            break;
        };
        let a = nodes.len() as i64;
        if let Some(n) = nodes.get_mut(leaf) {
            n.c1 = a;
            n.c2 = a + 1;
            n.split = rng.range_i64(0, 1);
        }
        for _ in 0..2 {
            nodes.push(NodeDesc {
                parent: leaf as i64,
                c1: -1,
                c2: -1,
                aux: 6,
                split: 2,
                marked: false,
                class: 0,
                face: -1,
                surface: -1,
            });
        }
    }
    let first_patch = rng.chance(0.15);
    for n in &mut nodes {
        n.aux = match rng.below(20) {
            0 => 0,
            1 => 8,
            2 => 1,
            3..=7 => 4,
            8..=11 if !first_patch => 3,
            _ => 6,
        };
        if n.c1 >= 0 && rng.chance(0.03) {
            n.split = 2; // bad split direction data: WARN branches
        }
        n.marked = rng.chance(0.5);
        n.class = rng.range_i64(0, 3);
        n.face = rng.range_i64(-1, 3);
        n.surface = if first_patch && rng.chance(0.1) {
            7
        } else {
            -1
        };
    }
    let count = nodes.len() as i64;
    let mut ops = Vec::new();
    if first_patch {
        for _ in 0..rng.range_i64(1, 4) {
            ops.push(DOp::FirstPatch {
                node: rng.range_i64(0, count - 1),
            });
        }
        ops.push(DOp::OwnerObject {
            node: rng.range_i64(0, count - 1),
        });
    } else {
        for k in 0..count {
            if count < 16 || rng.chance(0.5) {
                ops.push(DOp::Neighbors { node: k });
            }
        }
        for _ in 0..rng.range_i64(0, 6) {
            let node = rng.range_i64(0, count - 1);
            if rng.chance(0.6) {
                ops.push(DOp::Set {
                    node,
                    marked: rng.chance(0.7),
                    class: rng.range_i64(0, 3),
                    face: rng.range_i64(-1, 3),
                });
            }
            ops.push(DOp::Propagate { node });
        }
        ops.push(DOp::Dump);
    }
    Inputs::Subdiv { nodes, ops }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.tree"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::Spatial { ctor, ops } => {
                let (oi, of) = ffi::spatial(&spatial_prog(&ctor, &ops)).map_err(|e| ffi_err(&e))?;
                Ok(Output::new().ints("ints", oi).values("floats", of))
            }
            Inputs::Subdiv { nodes, ops } => {
                let o = ffi::subdiv(&subdiv_desc(&nodes), &subdiv_prog(&ops))
                    .map_err(|e| ffi_err(&e))?;
                Ok(Output::new().ints("ints", o))
            }
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::Spatial { ctor, ops } => match run_spatial_rust(&ctor, &ops) {
                Ok((oi, of)) => Ok(Output::new().ints("ints", oi).values("floats", of)),
                Err(e) => Ok(rust_err(&e)),
            },
            Inputs::Subdiv { nodes, ops } => match run_subdiv_rust(&nodes, &ops) {
                Ok(o) => Ok(Output::new().ints("ints", o)),
                Err(e) => Ok(rust_err(&e)),
            },
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = if rng.chance(0.6) {
            gen_spatial(rng)
        } else {
            gen_subdiv(rng)
        };
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}
