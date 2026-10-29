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
//! ids; `-1` = null), `floats` (boxes), and `patches` (native NURBS control nets,
//! degrees and knots selected by `first_patch`). Construction patches and temporary
//! subdivision surfaces carry distinct rational bilinear payloads; Rust supplies
//! those same payloads through the tree's existing caller-owned resolvers.
//! Every run has at least [`MIN_GENERATED`] generated cases. Additional
//! `patch_lookup` fixtures compare the selected pool identity and the actual NURB,
//! including distinct patches with identical geometry.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
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
    ClearPatch {
        node: i64,
    },
    Owner {
        surface: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatchNode {
    parent: i64,
    aux: i64,
    patch: i64,
    surface: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inputs {
    Spatial {
        ctor: Ctor,
        ops: Vec<SOp>,
    },
    Subdiv {
        nodes: Vec<NodeDesc>,
        ops: Vec<DOp>,
        #[serde(default = "default_patch_scale")]
        patch_scale: f64,
    },
    PatchLookup {
        surfaces: Vec<[[f64; 4]; 4]>,
        owner: i64,
        nodes: Vec<PatchNode>,
        queries: Vec<usize>,
    },
}

fn patch_program(
    surfaces: &[[[f64; 4]; 4]],
    owner: i64,
    nodes: &[PatchNode],
    queries: &[usize],
) -> Vec<f64> {
    let mut out = vec![surfaces.len() as f64, owner as f64];
    out.extend(surfaces.iter().flatten().flatten().copied());
    out.push(nodes.len() as f64);
    for n in nodes {
        out.extend([
            n.parent as f64,
            n.aux as f64,
            n.patch as f64,
            n.surface as f64,
        ]);
    }
    out.push(queries.len() as f64);
    out.extend(queries.iter().map(|&q| q as f64));
    out
}

fn rust_patch_lookup(
    surfaces: &[[[f64; 4]; 4]],
    owner: i64,
    nodes: &[PatchNode],
    queries: &[usize],
) -> solidcore::Result<Vec<f64>> {
    use solidcore::nurbs::{math::CPoint, srf_basic::Surface};
    let surfaces = surfaces
        .iter()
        .map(|points| {
            Surface::new(
                points
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
                            .collect()
                    })
                    .collect(),
                1,
                1,
                vec![0., 0., 1., 1.],
                vec![0., 0., 1., 1.],
            )
        })
        .collect::<solidcore::Result<Vec<_>>>()?;
    let mut tree: Tree<i64> = Tree::new(None);
    tree.set_owner_object(obj(owner).map(|_| TreeOwner::SurfaceCache));
    tree.set_owner_surface(obj(owner));
    for n in nodes {
        let id = tree.add_node(TreeNode {
            parent: link(n.parent),
            aux_type: AuxDataType::from_code(n.aux as i32).unwrap_or_default(),
            data: NodeData::Surface(SurfaceAux {
                surface: obj(n.surface),
                ..SurfaceAux::default()
            }),
            ..TreeNode::new()
        });
        if let Some(parent) = link(n.parent) {
            let p = tree.node_mut(parent)?;
            if p.child1.is_none() {
                p.child1 = Some(id);
            } else {
                p.child2 = Some(id);
            }
        }
    }
    tree.set_top_node(Some(NodeId(0)));
    let mut out = Vec::new();
    for &query in queries {
        let selected = tree.first_bezier_patch(
            NodeId(query),
            |n| {
                let node = nodes.get(n.0).ok_or_else(|| {
                    SolidError::invalid_input("difftest::patch_lookup", "node index")
                })?;
                obj(node.patch)
                    .map(|i| {
                        surfaces.get(i as usize).map(|s| (i, s)).ok_or_else(|| {
                            SolidError::invalid_input("difftest::patch_lookup", "patch index")
                        })
                    })
                    .transpose()
            },
            |&i| {
                surfaces
                    .get(i as usize)
                    .map(|s| Some((i, s)))
                    .ok_or_else(|| {
                        SolidError::invalid_input("difftest::patch_lookup", "surface index")
                    })
            },
        )?;
        let Some((id, surface)) = selected else {
            out.push(-1.0);
            continue;
        };
        out.push(id as f64);
        let (n, m, r, s) = surface.array_sizes();
        let (p, q) = surface.degrees();
        out.extend([n as f64, m as f64, p as f64, q as f64, r as f64, s as f64]);
        for row in surface.rows() {
            for p in row {
                out.extend([p.x, p.y, p.z, p.w]);
            }
        }
        out.extend(surface.u_knots());
        out.extend(surface.v_knots());
    }
    Ok(out)
}

fn default_patch_scale() -> f64 {
    1.0
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007,
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
            DOp::ClearPatch { node } => v.extend([6, *node]),
            DOp::Owner { surface } => v.extend([7, *surface]),
        }
    }
    v
}

// Same fixture geometry as the native shim, not a replacement for the tree walk.
// Homogeneous coordinates, V fastest; non-unit weights and non-unit domains make
// returning a different patch (or just its presence) observable.
fn native_patch(seed: i64, scale: f64) -> Vec<f64> {
    let mut values = vec![1.0, 1.0, 1.0, 1.0, 3.0, 3.0];
    for u in 0..2 {
        for v in 0..2 {
            let w = 1.0 + 0.25 * (seed % 3 + u + v) as f64;
            values.extend([
                (seed + u) as f64 * scale * w,
                (v - seed) as f64 * scale * w,
                (seed + u * v) as f64 * scale * w,
                w,
            ]);
        }
    }
    values.extend([-2.0, -2.0, 3.0, 3.0, 0.5, 0.5, 4.0, 4.0]);
    values
}

fn run_subdiv_rust(
    nodes: &[NodeDesc],
    ops: &[DOp],
    scale: f64,
) -> solidcore::Result<(Vec<i64>, Vec<f64>)> {
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
    let mut values = Vec::new();
    let mut cleared = vec![false; nodes.len()];
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
                let p = t.first_bezier_patch(
                    NodeId(*node as usize),
                    |id| {
                        let is_cleared = cleared.get(id.0).ok_or(SolidError::InvalidInput {
                            operation: "difftest::geom_tree::first_patch",
                            reason: "invalid construction patch node",
                        })?;
                        Ok((!*is_cleared).then(|| native_patch(id.0 as i64 + 1, scale)))
                    },
                    |surface| Ok(Some(native_patch(*surface + nodes.len() as i64 + 1, scale))),
                )?;
                o.push(i64::from(p.is_some()));
                if let Some(patch) = p {
                    values.extend(patch);
                }
            }
            DOp::OwnerObject { node } => {
                o.push(i64::from(
                    t.node_owner_object(NodeId(*node as usize))?.is_some(),
                ));
            }
            DOp::ClearPatch { node } => {
                *cleared
                    .get_mut(*node as usize)
                    .ok_or(SolidError::InvalidInput {
                        operation: "difftest::geom_tree::clear_patch",
                        reason: "invalid construction patch node",
                    })? = true;
            }
            DOp::Owner { surface } => {
                t.set_owner_object((*surface >= 0).then_some(TreeOwner::SurfaceCache));
                t.set_owner_surface(obj(*surface));
            }
        }
    }
    Ok((o, values))
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
            8..=11 => 3,
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
        for node in 0..count {
            ops.push(DOp::FirstPatch { node });
            ops.push(DOp::ClearPatch { node });
            ops.push(DOp::FirstPatch { node });
        }
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
    Inputs::Subdiv {
        nodes,
        ops,
        patch_scale: *rng
            .choose(&[0.0, 1e-100, 1e-3, 1.0, -7.0, 1e100])
            .unwrap_or(&1.0),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.tree"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::PatchLookup {
                surfaces,
                owner,
                nodes,
                queries,
            } => {
                let out = ffi::patch_lookup(&patch_program(&surfaces, owner, &nodes, &queries))
                    .map_err(|e| ffi_err(&e))?;
                Ok(Output::new().values("patches", out))
            }
            Inputs::Spatial { ctor, ops } => {
                let (oi, of) = ffi::spatial(&spatial_prog(&ctor, &ops)).map_err(|e| ffi_err(&e))?;
                Ok(Output::new().ints("ints", oi).values("floats", of))
            }
            Inputs::Subdiv {
                nodes,
                ops,
                patch_scale,
            } => {
                let desc = subdiv_desc(&nodes);
                let prog = subdiv_prog(&ops);
                let o = ffi::subdiv(&desc, &prog).map_err(|e| ffi_err(&e))?;
                let patches =
                    ffi::subdiv_patches(&desc, &prog, patch_scale).map_err(|e| ffi_err(&e))?;
                Ok(Output::new().ints("ints", o).values("patches", patches))
            }
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::PatchLookup {
                surfaces,
                owner,
                nodes,
                queries,
            } => {
                ffi::validate_patch_lookup(&patch_program(&surfaces, owner, &nodes, &queries))
                    .map_err(|e| ffi_err(&e))?;
                match rust_patch_lookup(&surfaces, owner, &nodes, &queries) {
                    Ok(out) => Ok(Output::new().values("patches", out)),
                    Err(e) => Ok(rust_err(&e)),
                }
            }
            Inputs::Spatial { ctor, ops } => match run_spatial_rust(&ctor, &ops) {
                Ok((oi, of)) => Ok(Output::new().ints("ints", oi).values("floats", of)),
                Err(e) => Ok(rust_err(&e)),
            },
            Inputs::Subdiv {
                nodes,
                ops,
                patch_scale,
            } => match run_subdiv_rust(&nodes, &ops, patch_scale) {
                Ok((o, patches)) => Ok(Output::new().ints("ints", o).values("patches", patches)),
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
        let mut cases = default_cases(self, &ctx)?;
        for scale in [0.0, 1e-100, 1e-3, 1.0, -7.0, 1e100] {
            let nodes = [
                // Construction patch wins over a distinct subdivision surface.
                (3, 7),
                // Surface-head and aux-data paths resolve the real B-spline.
                (4, 8),
                (6, 7),
                // Empty construction patch and non-surface data return null.
                (3, -1),
                (0, 7),
            ]
            .into_iter()
            .map(|(aux, surface)| NodeDesc {
                parent: -1,
                c1: -1,
                c2: -1,
                aux,
                split: 2,
                marked: false,
                class: 0,
                face: -1,
                surface,
            })
            .collect();
            let ops = (0..5)
                .flat_map(|node| {
                    [
                        DOp::FirstPatch { node },
                        DOp::ClearPatch { node },
                        DOp::FirstPatch { node },
                    ]
                })
                .collect();
            cases.push(Case {
                unit: self.id().into(),
                name: format!("native-first-patch-scale-{scale}"),
                source: crate::case::CaseSource::Inline,
                inputs: serde_json::to_value(Inputs::Subdiv {
                    nodes,
                    ops,
                    patch_scale: scale,
                })
                .map_err(|e| harness(e.to_string()))?,
            });
            let root = NodeDesc {
                parent: -1,
                c1: 1,
                c2: 2,
                aux: 3,
                split: 0,
                marked: false,
                class: 0,
                face: -1,
                surface: 7,
            };
            let mut child = root.clone();
            child.parent = 0;
            child.c1 = -1;
            child.c2 = -1;
            child.aux = 6;
            child.surface = -1;
            let mut sibling = child.clone();
            sibling.aux = 3;
            sibling.surface = 7;
            // Keep the explicit parent/child structure: null child surface climbs
            // to the ancestor only with a non-null owner; matching owner surfaces
            // on non-root nodes are suppressed, but a root may return its owner.
            let inputs = Inputs::Subdiv {
                nodes: vec![root, child, sibling],
                ops: vec![
                    DOp::Owner { surface: 7 },
                    DOp::OwnerObject { node: 0 },
                    DOp::FirstPatch { node: 1 },
                    DOp::FirstPatch { node: 2 },
                    DOp::ClearPatch { node: 2 },
                    DOp::FirstPatch { node: 2 },
                    DOp::FirstPatch { node: 0 },
                    DOp::ClearPatch { node: 0 },
                    DOp::FirstPatch { node: 1 },
                    DOp::FirstPatch { node: 0 },
                    DOp::Owner { surface: 8 },
                    DOp::FirstPatch { node: 2 },
                    DOp::FirstPatch { node: 1 },
                    DOp::Owner { surface: -1 },
                    DOp::FirstPatch { node: 1 },
                    DOp::FirstPatch { node: 2 },
                    DOp::OwnerObject { node: 0 },
                ],
                patch_scale: scale,
            };
            cases.push(Case {
                unit: self.id().into(),
                name: format!("native-first-patch-owner-ancestor-scale-{scale}"),
                source: crate::case::CaseSource::Inline,
                inputs: serde_json::to_value(inputs).map_err(|e| harness(e.to_string()))?,
            });
        }
        Ok(cases)
    }
}

#[cfg(test)]
mod patch_lookup_tests {
    use super::*;

    #[test]
    fn real_patch_identity_geometry_and_owner_boundary() {
        let net = [
            [0., 0., 0., 1.],
            [0., 2., 0., 1.],
            [3., 0., 1., 1.],
            [3., 2., 1., 1.],
        ];
        let surfaces = vec![net, net, net];
        let nodes = vec![
            PatchNode {
                parent: -1,
                aux: 3,
                patch: 1,
                surface: -1,
            },
            PatchNode {
                parent: 0,
                aux: 3,
                patch: 2,
                surface: -1,
            },
            PatchNode {
                parent: 0,
                aux: 6,
                patch: -1,
                surface: 0,
            },
        ];
        let queries = [0, 1, 2];
        let native = ffi::patch_lookup(&patch_program(&surfaces, 0, &nodes, &queries)).unwrap();
        let mut actual = rust_patch_lookup(&surfaces, 0, &nodes, &queries).unwrap();
        assert_eq!(native, actual);
        assert_eq!(actual.len(), 63);
        assert_eq!([actual[0], actual[31], actual[62]], [1., 2., -1.]);
        assert_eq!(&actual[1..31], &actual[32..62]);
        actual[0] = 2.;
        assert!(!crate::compare::compare(
            &Output::new().values("patches", native),
            &Output::new().values("patches", actual),
            &[]
        )
        .pass());
    }

    #[test]
    fn malformed_patch_programs_are_refused_before_native_access() {
        let net = [[0., 0., 0., 1.]; 4];
        let mut nodes = vec![PatchNode {
            parent: -1,
            aux: 3,
            patch: 0,
            surface: -1,
        }];
        let good = patch_program(&[net], 0, &nodes, &[0]);
        assert!(ffi::validate_patch_lookup(&good).is_ok());
        for len in 0..good.len() {
            assert!(ffi::validate_patch_lookup(&good[..len]).is_err());
        }
        nodes[0].parent = 0;
        assert!(ffi::patch_lookup(&patch_program(&[net], 0, &nodes, &[0])).is_err());
        nodes[0].parent = -1;
        assert!(ffi::patch_lookup(&patch_program(&[net], 0, &nodes, &[1])).is_err());
        let mut bad_weight = net;
        bad_weight[0][3] = 0.;
        assert!(ffi::patch_lookup(&patch_program(&[bad_weight], 0, &nodes, &[0])).is_err());
    }
}
