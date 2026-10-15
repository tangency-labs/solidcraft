//! `SmBrepCache` differential runner: real poly geometry, tree structure and order,
//! object and node boxes, spatial queries and exact-vertex resize requests. Exact
//! `BuildTrees` explicitly defers to the unported `query.brep` box calculation.
use crate::case::CaseContext;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::a_object::{BREP_TYPE, POLY_BREP_TYPE};
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tree::{AuxDataType, NodeId, Tree, TreeOwner};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::query::poly::PolyBrep;
use solidcore::topology::brep_cache::{self, arena_identity, BrepCache, CacheObject, CacheSource};
use solidcore::topology::{Brep, Vertex};
use solidcore::Result as SolidResult;
use solidcore_ffi::units::topology_brep_cache as ffi;

/// Differential coverage of `SmBrepCache`.
pub struct Runner;

#[derive(Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    exact: bool,
    #[serde(default = "yes")]
    build: bool,
    tol: f64,
    #[serde(default)]
    vertices: Vec<[f64; 4]>,
    #[serde(default)]
    faces: Vec<Vec<usize>>,
    #[serde(default)]
    resize: Vec<[f64; 4]>,
    #[serde(default)]
    resize_edges: Vec<[f64; 9]>,
    #[serde(default)]
    queries: Vec<[f64; 6]>,
}
fn yes() -> bool {
    true
}
fn encode(i: &Inputs) -> Vec<f64> {
    let mut v = vec![
        f64::from(i.exact),
        f64::from(i.build),
        i.tol,
        i.vertices.len() as f64,
    ];
    v.extend(i.vertices.iter().flatten());
    v.push(i.faces.len() as f64);
    for f in &i.faces {
        v.push(f.len() as f64);
        v.extend(f.iter().map(|&x| x as f64));
    }
    v.push(i.resize.len() as f64);
    v.extend(i.resize.iter().flatten());
    v.push(i.resize_edges.len() as f64);
    v.extend(i.resize_edges.iter().flatten());
    v.push(i.queries.len() as f64);
    v.extend(i.queries.iter().flatten());
    v
}
fn ext(v: [f64; 6]) -> Extent3d {
    Extent3d::from_bounds(v[0], v[1], v[2], v[3], v[4], v[5])
}
fn put_box(out: &mut Vec<f64>, b: &Extent3d) {
    out.extend(b.min().to_array());
    out.extend(b.max().to_array());
}
fn idx(id: Option<NodeId>) -> i64 {
    id.map_or(-1, |n| n.0 as i64)
}
fn dump(
    tree: Option<&Tree<i64>>,
    queries: &[[f64; 6]],
    ints: &mut Vec<i64>,
    floats: &mut Vec<f64>,
) -> SolidResult<()> {
    ints.push(i64::from(tree.is_some()));
    let Some(t) = tree else {
        return Ok(());
    };
    ints.push(i64::from(t.owner() == Some(TreeOwner::BrepCache)));
    ints.push(idx(t.top_node()));
    let nodes = t.all_tree_nodes();
    ints.push(nodes.len() as i64);
    for id in nodes {
        let n = t.node(id)?;
        ints.extend([
            idx(n.parent),
            idx(n.child1),
            idx(n.child2),
            i64::from(n.aux_type.code()),
            i64::from(n.geom_type.code()),
        ]);
        put_box(floats, &n.bbox);
        let items = if n.aux_type == AuxDataType::ObjectList {
            t.node_object_list(id)?
        } else {
            Vec::new()
        };
        ints.push(items.len() as i64);
        for item in items {
            let item = t.list(item)?;
            ints.push(item.object.unwrap_or(-1));
            put_box(floats, &item.bbox);
        }
    }
    let all = t.object_list()?;
    ints.push(all.len() as i64);
    for id in all {
        let item = t.list(id)?;
        ints.push(item.object.unwrap_or(-1));
        put_box(floats, &item.bbox);
    }
    for &q in queries {
        let hits = t.objects_in_box(&ext(q))?;
        ints.push(hits.len() as i64);
        ints.extend(hits.into_iter().map(|o| o.unwrap_or(-1)));
    }
    Ok(())
}
fn run(i: &Inputs) -> SolidResult<Output> {
    run_with_intermediate_owners(i, false)
}

fn run_with_intermediate_owners(i: &Inputs, intermediate_owners: bool) -> SolidResult<Output> {
    let k = Kernel::default();
    let mut poly = PolyBrep::new(&k, i.tol);
    let mut vertices = Vec::new();
    for &[x, y, z, t] in &i.vertices {
        vertices.push(poly.add_vertex(&k, Point3::new(x, y, z), t)?);
    }
    if !i.faces.is_empty() {
        let region = poly.add_region()?;
        let shell = poly.add_shell(region)?;
        for face in &i.faces {
            let f = poly.add_face(&k, shell, i.tol, None)?;
            let lp = poly.start_poly_edge_loop(f)?;
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)) {
                let missing = || {
                    solidcore::SolidError::invalid_input(
                        "difftest::topology_brep_cache",
                        "polygon vertex index",
                    )
                };
                let a = *vertices.get(a).ok_or_else(missing)?;
                let b = *vertices.get(b).ok_or_else(missing)?;
                poly.add_poly_edge(
                    &k,
                    lp,
                    i.tol,
                    poly.vertex_point(a)?,
                    poly.vertex_point(b)?,
                    a,
                )?;
            }
            poly.finish_poly_edge_loop(lp)?;
        }
    }
    let edges = poly.poly_edges(None)?;
    let faces = poly.poly_faces(None)?;
    let mut cache = BrepCache::new(if i.exact { BREP_TYPE } else { POLY_BREP_TYPE })?;
    let mut ints = vec![brep_cache::TYPE];
    ints.extend(
        [
            brep_cache::TYPE,
            solidcore::geom::object::OBJECT_TYPE,
            12106,
            BREP_TYPE,
            brep_cache::TYPE + 1,
        ]
        .map(|t| i64::from(brep_cache::is_kind_of(t))),
    );
    ints.push(0);
    let b = Brep::new();
    if i.build {
        let source = if i.exact {
            CacheSource::Brep(&b)
        } else {
            CacheSource::PolyBrep(&poly)
        };
        cache.build_trees(
            &k,
            source,
            |object| match object {
                CacheObject::PolyVertex(v) => vertices
                    .iter()
                    .position(|&x| x == v)
                    .map_or(-2, |p| p as i64),
                CacheObject::PolyEdge(e) => edges
                    .iter()
                    .position(|&x| x == e)
                    .map_or(-2, |p| 100000 + p as i64),
                CacheObject::PolyFace(f) => faces
                    .iter()
                    .position(|&x| x == f)
                    .map_or(-2, |p| 200000 + p as i64),
                _ => -2,
            },
            &arena_identity,
        )?;
    }
    if i.build && (!i.resize.is_empty() || !i.resize_edges.is_empty()) {
        let mut b = Brep::new();
        let mut ids = Vec::new();
        for &[x, y, z, t] in &i.resize {
            ids.push(b.add_vertex_record(Vertex {
                point: Point3::new(x, y, z),
                tolerance: solidcore::geom::tol::zone_tol3d(&k, t),
                ..Vertex::default()
            })?);
        }
        let mut edge_ids = Vec::new();
        let mut owners = std::collections::BTreeMap::new();
        for &edge in &i.resize_edges {
            let e = add_wire(&k, &mut b, edge)?;
            if intermediate_owners {
                use solidcore::geom::object::ObjectRef;
                let generated = ObjectRef {
                    type_code: solidcore::curve::curve::p2::TYPE,
                    key: solidcore::geom::attribute::ObjectKey(2000000000 + u64::from(e.index())),
                };
                let edgeuse = ObjectRef {
                    type_code: solidcore::topology::edgeuse::p1::TYPE,
                    key: generated.key,
                };
                b.edge_mut(e)?
                    .curve
                    .as_mut()
                    .ok_or(solidcore::SolidError::null_pointer(
                        "difftest::topology_brep_cache::owner_chain",
                    ))?
                    .set_owner(Some(generated), None);
                owners.insert(generated, edgeuse);
                owners.insert(edgeuse, identity(CacheObject::Edge(e)));
            }
            edge_ids.push(e);
        }
        cache.resize_trees(&k, &b, &[], &edge_ids, &ids, &identity, &|o| {
            owners.get(&o).copied()
        })?;
    }
    ints.push(0);
    // Separate streams ensure topology and ids compare exactly, boxes relatively.
    let mut floats = Vec::new();
    for t in [
        cache.vertex_tree(),
        cache.curve_tree(),
        cache.surface_tree(),
    ] {
        dump(t, &i.queries, &mut ints, &mut floats)?;
    }
    Ok(Output::new().ints("topology", ints).values("boxes", floats))
}

fn identity(object: CacheObject) -> solidcore::geom::object::ObjectRef {
    let mut r = arena_identity(object);
    r.key.0 += 1000000000;
    r
}

fn add_wire(k: &Kernel, b: &mut Brep, data: [f64; 9]) -> SolidResult<solidcore::topology::EdgeId> {
    use solidcore::curve::curve::Curve;
    use solidcore::curve::line::Line;
    use solidcore::geom::attribute::ObjectKey;
    use solidcore::geom::object::ObjectRef;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::{Edge, Edgeuse, EdgeuseOwner, Shell, Vertexuse, VertexuseOwner};
    let [ax, ay, az, bx, by, bz, et, at, bt] = data;
    let et = solidcore::geom::tol::zone_tol3d(k, et);
    let at = solidcore::geom::tol::zone_tol3d(k, at);
    let bt = solidcore::geom::tol::zone_tol3d(k, bt);
    let a = Point3::new(ax, ay, az);
    let c = Point3::new(bx, by, bz);
    let first = b.add_vertex_record(Vertex {
        point: a,
        tolerance: at,
        ..Vertex::default()
    })?;
    let last = b.add_vertex_record(Vertex {
        point: c,
        tolerance: bt,
        ..Vertex::default()
    })?;
    let shell = b.add_shell_record(Shell::default())?;
    let edge = b.add_edge_record(Edge::new([first, last]))?;
    let mut curve = Curve::from(Line::create_line_segment(3, a, c, None)?);
    curve.set_owner(
        Some(ObjectRef {
            type_code: solidcore::topology::edge::TYPE,
            key: ObjectKey(1000000000 + u64::from(edge.index())),
        }),
        None,
    );
    let e1 = b.add_edgeuse_record(Edgeuse {
        edge: Some(edge),
        owner: EdgeuseOwner::Shell(Some(shell)),
        orientation: OrientType::Same,
        ..Edgeuse::default()
    })?;
    let e2 = b.add_edgeuse_record(Edgeuse {
        edge: Some(edge),
        owner: EdgeuseOwner::Shell(Some(shell)),
        orientation: OrientType::Opposite,
        ..Edgeuse::default()
    })?;
    let v1 = b.add_vertexuse_record(Vertexuse {
        vertex: Some(first),
        owner: VertexuseOwner::Edgeuse(Some(e1)),
        ..Vertexuse::default()
    })?;
    let v2 = b.add_vertexuse_record(Vertexuse {
        vertex: Some(last),
        owner: VertexuseOwner::Edgeuse(Some(e2)),
        ..Vertexuse::default()
    })?;
    b.edgeuse_mut(e1)?.vertexuse = Some(v1);
    b.edgeuse_mut(e2)?.vertexuse = Some(v2);
    let out = b.edge_mut(edge)?;
    out.edgeuses = vec![e1, e2];
    out.interval = curve.natural_interval();
    out.curve = Some(curve);
    out.tolerance = et;
    Ok(edge)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_cache"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let v = ffi::eval(&encode(&i)).map_err(|e| HarnessError::new(e.to_string()))?;
        let n = *v
            .first()
            .ok_or_else(|| HarnessError::new("missing topology length"))? as usize;
        let end = n
            .checked_add(1)
            .ok_or_else(|| HarnessError::new("topology length overflow"))?;
        let ints = v
            .get(1..end)
            .ok_or_else(|| HarnessError::new("invalid topology length"))?;
        let floats = v
            .get(end..)
            .ok_or_else(|| HarnessError::new("invalid box offset"))?;
        Ok(Output::new()
            .ints(
                "topology",
                ints.iter().map(|&x| x as i64).collect::<Vec<_>>(),
            )
            .values("boxes", floats.to_vec()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match run(&i) {
            Ok(o) => Ok(o),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        }
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(600);
        crate::runner::default_cases(self, &ctx)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = *[1e-9, 1e-3, 1.0, 1e3, 1e12].get(rng.below(5) as usize)?;
        let n = 3 + rng.below(60) as usize;
        let tol = if rng.chance(0.3) {
            0.0
        } else {
            scale * rng.uniform(1e-6, 0.01)
        };
        let z = scale * rng.uniform(-5.0, 5.0);
        let vertices = (0..n)
            .map(|j| {
                let t = 2.0 * std::f64::consts::PI * j as f64 / n as f64;
                [scale * t.cos(), scale * t.sin(), z, tol]
            })
            .collect();
        let faces = if rng.chance(0.7) {
            vec![(0..n).collect()]
        } else {
            Vec::new()
        };
        let resize = if rng.chance(0.6) {
            vec![
                [scale * rng.uniform(-30.0, 30.0), 0.0, z, tol * 10.0],
                [0.0, scale * 30.0, z, 0.0],
            ]
        } else {
            Vec::new()
        };
        let queries = vec![
            [
                -2.0 * scale,
                -2.0 * scale,
                z - scale,
                2.0 * scale,
                2.0 * scale,
                z + scale,
            ],
            [0.0, 0.0, z, scale, scale, z],
        ];
        serde_json::to_value(Inputs {
            exact: false,
            build: true,
            tol,
            vertices,
            faces,
            resize,
            resize_edges: if rng.chance(0.4) {
                vec![[
                    0.0,
                    0.0,
                    z,
                    scale * 12.0,
                    scale * -9.0,
                    z,
                    tol * 2.0,
                    tol * 10.0,
                    tol,
                ]]
            } else {
                Vec::new()
            },
            queries,
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tolerance_normalization_matches_rust() {
        for tolerance in [-0.001, solidcore::geom::tol::USE_DEFAULT] {
            let input = Inputs {
                exact: false,
                build: true,
                tol: tolerance,
                vertices: vec![
                    [0.0, 0.0, 0.0, tolerance],
                    [1.0, 0.0, 0.0, tolerance],
                    [0.0, 1.0, 0.0, tolerance],
                ],
                faces: vec![vec![0, 1, 2]],
                resize: vec![[20.0, 0.0, 0.0, tolerance]],
                resize_edges: vec![[
                    1.0, 1.0, 1.0, 5.0, 6.0, 7.0, tolerance, tolerance, tolerance,
                ]],
                queries: Vec::new(),
            };
            let case = Case {
                unit: Runner.id().into(),
                name: format!("tolerance_{tolerance}"),
                source: crate::case::CaseSource::Inline,
                inputs: serde_json::to_value(&input).unwrap(),
            };
            let cpp = Runner.run_cpp(&case).unwrap();
            let rust = Runner.run_rust(&case).unwrap();
            assert!(cpp
                .values
                .get("boxes")
                .is_some_and(|values| !values.is_empty()));
            assert_eq!(cpp, rust);
            // Equivalent geometry reached through generated-curve and edgeuse
            // identities must resolve to the same native edge bounding boxes.
            assert_eq!(cpp, run_with_intermediate_owners(&input, true).unwrap());
        }
    }
}
