//! Part-9 differential tests: real native topology, full interval and point records,
//! both cache traversal branches and explicit dependency deferrals. No absolute floors.
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tree::{
    AuxDataType, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
};
use solidcore::intersect::curve_class::p9::{
    BoundaryEdge, BoundaryEdgeuse, BoundaryFace, BoundaryLoop, BoundaryVertex,
    IntersectEdgesOptions,
};
use solidcore::intersect::curve_class::{CurveClassification, CurveInterval};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::{Vec2, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, FaceContainment, SurfaceCacheOptions, VertexListItem,
};
use solidcore::surface::trim_srf_cache::TrimSrfCache;
use solidcore::surface::volume::Volume;
use solidcore::{Result as ScResult, SolidError};
use solidcore_ffi::units::intersect_curve_class_p9 as ffi;

/// Differential runner for the two part-9 methods.
pub struct Runner;
fn yes() -> bool {
    true
}
fn one() -> f64 {
    1.
}
fn zone() -> f64 {
    1e-6
}
fn bounds() -> [f64; 2] {
    [0., 1.]
}
fn source() -> [[f64; 3]; 2] {
    [[0.2, 0.5, 0.], [0.8, 0.5, 0.]]
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct EdgeIn {
    a: [f64; 3],
    b: [f64; 3],
    #[serde(default = "bounds")]
    interval: [f64; 2],
    #[serde(default)]
    same: bool,
    #[serde(default = "zone")]
    zone: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Independent native flags.
struct Input {
    op: usize,
    #[serde(default)]
    kind: usize,
    #[serde(default)]
    state: usize,
    #[serde(default)]
    check3d: bool,
    #[serde(default)]
    cross: bool,
    #[serde(default)]
    big: bool,
    #[serde(default = "yes")]
    uv: bool,
    #[serde(default)]
    defaults: bool,
    #[serde(default)]
    dim3: bool,
    #[serde(default)]
    rectangle: bool,
    #[serde(default = "one")]
    scale: f64,
    #[serde(default = "zone")]
    zone: f64,
    #[serde(default = "bounds")]
    interval: [f64; 2],
    #[serde(default = "source")]
    source: [[f64; 3]; 2],
    #[serde(default)]
    vertices: Vec<[f64; 3]>,
    #[serde(default)]
    edges: Vec<EdgeIn>,
}
impl Input {
    fn flat(&self) -> Vec<f64> {
        let mut out = vec![
            self.op as f64,
            self.kind as f64,
            self.state as f64,
            f64::from(self.check3d),
            f64::from(self.cross),
            f64::from(self.big),
            f64::from(self.uv),
            f64::from(self.defaults),
            f64::from(self.dim3),
            f64::from(self.rectangle),
            self.scale,
            self.zone,
            self.vertices.len() as f64,
            self.edges.len() as f64,
            self.interval[0],
            self.interval[1],
        ];
        out.extend(self.source.iter().flatten());
        out.extend(self.vertices.iter().flatten());
        for e in &self.edges {
            out.extend(e.a);
            out.extend(e.b);
            out.extend(e.interval);
            out.extend([f64::from(e.same), e.zone]);
        }
        out
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum H {
    #[default]
    Source,
    EdgeCurve(usize),
    Trim(usize),
    Vertex(usize),
    Edge(usize),
    Surface,
    Vu(usize),
    Eu(usize),
}
impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Source | Self::EdgeCurve(_) | Self::Trim(_) => ClassObjectKind::Curve,
            Self::Vertex(_) => ClassObjectKind::Vertex,
            Self::Edge(_) => ClassObjectKind::Edge,
            Self::Surface => ClassObjectKind::Surface,
            Self::Vu(_) => ClassObjectKind::Vertexuse,
            Self::Eu(_) => ClassObjectKind::Edgeuse,
        }
    }
}
struct Pool {
    source: Curve,
    edges: Vec<Curve>,
    trims: Vec<Curve>,
    vertices: Vec<Vec3>,
    surface: Surface,
    zone: f64,
    edge_zones: Vec<f64>,
}
impl ClassGeometry<H> for Pool {
    fn classification_vertex_point(&self, o: H) -> Option<Vec3> {
        if let H::Vertex(j) = o {
            self.vertices.get(j).copied()
        } else {
            None
        }
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        match o {
            H::Vertex(_) => Some(self.zone),
            H::Edge(j) => self.edge_zones.get(j).copied(),
            _ => None,
        }
    }
    fn curve(&self, o: H) -> Option<&Curve> {
        match o {
            H::Source => Some(&self.source),
            H::EdgeCurve(j) => self.edges.get(j),
            H::Trim(j) => self.trims.get(j),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        (o == H::Surface).then_some(&self.surface)
    }
    fn volume(&self, _: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _: H) -> Option<Vec3> {
        None
    }
}
fn v(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
fn line(a: Vec3, b: Vec3, dim: u64) -> Curve {
    Curve::from(Line::from_points(a, b, dim))
}

fn surface(i: &Input) -> ScResult<Surface> {
    let domain = Extent2d::from_bounds(0., 0., 1., 1.);
    if i.kind == 1 {
        return Ok(Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            Vec3::new(0., 1., 0.),
            Vec2::new(i.scale, i.scale),
            &domain,
        )));
    }
    let net = (0..2)
        .map(|a| {
            (0..2)
                .map(|b| {
                    CPoint::new(
                        i.scale * f64::from(a),
                        i.scale * f64::from(b) * if i.kind == 2 { f64::from(a) } else { 1. },
                        0.,
                        NOW,
                    )
                })
                .collect()
        })
        .collect();
    let knots = vec![0., 0., 1., 1.];
    Ok(Surface::from(BSplineSurface::from_nurb(
        NlSurface::new(net, 1, 1, knots.clone(), knots)?,
        false,
    )))
}
fn put_pc(out: &mut Vec<f64>, p: &PointClassification<H>) {
    let uv = p.uv_param();
    let uvw = p.uvw_param();
    let sss = p.sss_point();
    out.extend([
        f64::from(p.point_class().code()),
        p.object().map_or(-1., |h| match h {
            H::Vertex(j) => 100. + j as f64,
            H::Edge(j) => 200. + j as f64,
            _ => -2.,
        }),
        p.deviation(),
        p.t_param(),
        uv.x,
        uv.y,
        uvw.x,
        uvw.y,
        uvw.z,
        f64::from(p.are_parameters_set()),
        f64::from(p.sss_point_flag()),
        sss.x,
        sss.y,
        sss.z,
        p.pre_snap_param(),
        p.src_zone_tol_3d(),
    ]);
}
fn cache(i: &Input, pool: &Pool, loops: &[BoundaryLoop<H>]) -> TrimSrfCache<H> {
    let mut cache = TrimSrfCache::new(pool.surface.clone(), SurfaceCacheOptions::default());
    let count = if i.state == 4 || i.state == 6 { 3 } else { 1 };
    let split_v = i.state == 6;
    let mut tree = Tree::new(None);
    for j in 0..count {
        let mut node = TreeNode::new();
        node.aux_type = AuxDataType::SurfaceData;
        let lo = if j == 2 { 0.5 } else { 0. };
        let hi = if j == 1 { 0.5 } else { 1. };
        let (u0, u1, v0, v1) = if split_v {
            (0., 1., lo, hi)
        } else {
            (lo, hi, 0., 1.)
        };
        let domain = Extent2d::from_bounds(u0, v0, u1, v1);
        node.geom_type = NodeGeomType::Default;
        node.bbox = Extent3d::from_points(
            Vec3::new(
                u0 * i.scale,
                v0 * i.scale * if i.kind == 2 { u0 } else { 1. },
                0.,
            ),
            Vec3::new(
                u1 * i.scale,
                v1 * i.scale * if i.kind == 2 { u1 } else { 1. },
                0.,
            ),
        );
        node.data = NodeData::Surface(SurfaceAux {
            split_dir: if count == 3 && j == 0 {
                if split_v {
                    SplitDir::V
                } else {
                    SplitDir::U
                }
            } else {
                SplitDir::Unknown
            },
            slot: Some(j),
            ..SurfaceAux::default()
        });
        if count == 3 {
            if j == 0 {
                node.child1 = Some(NodeId(1));
                node.child2 = Some(NodeId(2));
            } else {
                node.parent = Some(NodeId(0));
            }
        }
        tree.add_node(node);
        cache.base_mut().auxs_mut().push(BezierAux2d {
            uv_domain: domain,
            owning_tree_node: Some(NodeId(j)),
            vertex_list: loops
                .iter()
                .flat_map(|lp| &lp.vertices)
                .map(|x| VertexListItem {
                    vertexuse: Some(x.vertexuse),
                    // Choose the lower-leaf representative of this collapsed pole.
                    uv_point: if split_v && i.kind == 2 && x.point == Vec3::ZERO {
                        Vec2::ZERO
                    } else {
                        x.uv
                    },
                    uv_tol: Vec2::ZERO,
                })
                .filter(|x| !split_v || domain.contains_point(x.uv_point, 0.))
                .collect(),
            edgeuse_list: loops
                .iter()
                .flat_map(|lp| &lp.edges)
                .map(|x| x.edgeuse)
                .collect(),
            ..BezierAux2d::default()
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    cache.base_mut().set_tree(Some(tree));
    cache.base_mut().face_was_modified = i.state == 2;
    cache.base_mut().have_t_surface_cache = i.state != 0;
    cache.base_mut().face_containment_done = match i.state {
        3 => FaceContainment::NotDone,
        5 => FaceContainment::UnderConstruction,
        _ => FaceContainment::Done,
    };
    cache
}
// Native ComputeUVPoint projects loop vertices even when m_sUVPoint was set.
// Use the existing topology operation so both sides receive projected UVs rather
// than assuming the input coordinates survive a surface solve bit for bit.
fn loop_vertex_uv(k: &Kernel, surface: &Surface, point: Vec3, zone: f64) -> ScResult<Vec2> {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::{
        Brep, Face, Faceuse, Loopuse, LoopuseStart, Vertex, Vertexuse, VertexuseOwner,
    };
    let mut b = Brep::new();
    let shell = b.add_shell()?;
    let f = b.add_face_record(Face {
        surface: Some(surface.clone()),
        uv_domain: Extent2d::from_bounds(0., 0., 1., 1.),
        tolerance: 1e-5,
        ..Face::new(shell)
    })?;
    let fu = b.add_faceuse_record(Faceuse {
        shell: Some(shell),
        face: Some(f),
        orientation: OrientType::Same,
        ..Faceuse::default()
    })?;
    b.face_mut(f)?.faceuse = Some(fu);
    let lu = b.add_loopuse_record(Loopuse {
        faceuse: Some(fu),
        ..Loopuse::default()
    })?;
    b.faceuse_mut(fu)?.loopuses.push(lu);
    let vertex = b.add_vertex_record(Vertex {
        point,
        tolerance: zone,
        ..Vertex::default()
    })?;
    let vu = b.add_vertexuse_record(Vertexuse {
        vertex: Some(vertex),
        owner: VertexuseOwner::Loopuse(Some(lu)),
        ..Vertexuse::default()
    })?;
    b.loopuse_mut(lu)?.start = LoopuseStart::Vertexuse(Some(vu));
    solidcore::topology::vertexuse::compute_uv_point(k, &mut b, vu, true)
}
fn rust(i: &Input) -> ScResult<Output> {
    let k = Kernel::default();
    let factor = if i.op == 1 { 1. } else { i.scale };
    let mut pool = Pool {
        source: line(
            v(i.source[0]) * factor,
            v(i.source[1]) * factor,
            if i.dim3 { 3 } else { 2 },
        ),
        edges: Vec::new(),
        trims: Vec::new(),
        vertices: i.vertices.iter().map(|x| v(*x) * i.scale).collect(),
        surface: surface(i)?,
        zone: i.zone,
        edge_zones: Vec::new(),
    };
    let mut loops = Vec::new();
    for (j, p) in i.vertices.iter().enumerate() {
        loops.push(BoundaryLoop {
            vertices: vec![BoundaryVertex {
                vertexuse: H::Vu(j),
                vertex: H::Vertex(j),
                uv: loop_vertex_uv(&k, &pool.surface, v(*p) * i.scale, i.zone)?,
                point: v(*p) * i.scale,
                zone_tol_3d: i.zone,
            }],
            edges: Vec::new(),
        });
    }
    if i.rectangle {
        let corners = [
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            Vec3::new(1., 1., 0.),
            Vec3::new(0., 1., 0.),
        ];
        let mut lp = BoundaryLoop {
            vertices: Vec::new(),
            edges: Vec::new(),
        };
        for (j, (&a, &b)) in corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .enumerate()
        {
            pool.vertices.push(a * i.scale);
            pool.edge_zones.push(0.);
            pool.edges
                .push(Curve::from(BSplineCurve::create_line_segment(
                    3,
                    a * i.scale,
                    b * i.scale,
                )?));
            pool.trims
                .push(Curve::from(BSplineCurve::create_line_segment(2, a, b)?));
            lp.vertices.push(BoundaryVertex {
                vertexuse: H::Vu(j),
                vertex: H::Vertex(j),
                uv: Vec2::new(a.x, a.y),
                point: a * i.scale,
                zone_tol_3d: i.zone,
            });
            lp.edges.push(BoundaryEdgeuse {
                edgeuse: H::Eu(j),
                edge: BoundaryEdge {
                    edge: H::Edge(j),
                    curve: H::EdgeCurve(j),
                    interval: Extent1d::from_min_max(0., 1.),
                    face_gap: None,
                    zone_tol_3d: 0.,
                },
                trim_curve: Some(H::Trim(j)),
            });
        }
        loops.push(lp);
    }
    let mut edges = Vec::new();
    for (j, e) in i.edges.iter().enumerate() {
        pool.edges.push(line(v(e.a) * i.scale, v(e.b) * i.scale, 3));
        pool.edge_zones.push(e.zone);
        edges.push(BoundaryEdge {
            edge: H::Edge(j),
            curve: if e.same { H::Source } else { H::EdgeCurve(j) },
            interval: Extent1d::from_min_max(e.interval[0], e.interval[1]),
            face_gap: None,
            zone_tol_3d: e.zone,
        });
    }
    let interval = Extent1d::from_min_max(i.interval[0], i.interval[1]);
    let mut cc = CurveClassification::with_curve_interval(&k, H::Source, interval, i.zone);
    cc.set_surface1(Some(H::Surface));
    cc.set_src_zone_tol_3d(i.zone);
    cc.set_doing_cross_hatch(i.cross);
    *cc.at_mut(0)? = CurveInterval::with_interval(&k, &interval, Some(&cc));
    let cache = cache(i, &pool, &loops);
    let result = if i.op == 1 {
        cc.intersect_ts_bound(
            &k,
            &pool,
            &BoundaryFace {
                surface: &pool.surface,
                loops: &loops,
                cache: &cache,
            },
            i.check3d,
        )
    } else {
        cc.intersect_edges(
            &k,
            &pool,
            &edges,
            &pool.surface,
            if i.defaults {
                IntersectEdgesOptions::default()
            } else {
                IntersectEdgesOptions {
                    has_big_gaps: i.big,
                    uv_space_okay: i.uv,
                }
            },
        )
    };
    if let Err(ref e) = result {
        if let Some(out) = Output::unported(e) {
            return Ok(out);
        }
    }
    let status = result.as_ref().err().map_or(0., |e| match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => 1007.,
    });
    let mut out = vec![
        status,
        cc.size() as f64,
        cc.interval().min(),
        cc.interval().max(),
    ];
    for iv in cc.intervals() {
        out.extend([iv.min(), iv.max()]);
        for pc in [&iv.start, &iv.mid, &iv.end] {
            put_pc(&mut out, pc);
        }
    }
    Ok(Output::new().values("out", out))
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p9"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(384),
                ..ctx.clone()
            },
        )
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = rng.below(2);
        let scale = rng.choose(&[1e-6, 1., 1e6, 1e8]).copied()?;
        let mut out = json!({"op":op,"kind":rng.below(3),"state":rng.below(7),"dim3":op==0,
            "scale":scale,"zone":scale*1e-7,"cross":rng.below(2)==1,"check3d":rng.below(2)==1,
            "big":rng.below(2)==1,"defaults":rng.below(3)==0});
        let object = out.as_object_mut()?;
        if op == 1 {
            let u = rng.uniform(0.1, 0.9);
            let gap = rng.choose(&[0., 0., 0., 1e-8, 1e-6]).copied()?;
            object.insert("vertices".into(), json!([[u, 0.5, gap]]));
            if rng.below(4) == 0 {
                object.insert("source".into(), json!([[0.1, 0.7, 0.], [0.9, 0.7, 0.]]));
            }
        } else {
            let y = rng.choose(&[0.5, 0.7, 0.50000001]).copied()?;
            object.insert(
                "edges".into(),
                json!([{"a":[0.5,y,-1.],"b":[0.5,y,1.],"zone":scale*1e-7}]),
            );
            if rng.below(5) == 0 {
                object.insert("edges".into(), json!([]));
            }
        }
        if op == 1 && rng.below(5) == 0 {
            let kind = object.get("kind")?.as_u64()?;
            object.insert("source".into(), json!([[0.2, 0.5, 0.], [0.8, 0.5, 0.]]));
            object.insert(
                "vertices".into(),
                json!([[0.2, if kind == 2 { 0.1 } else { 0.5 }, 0.]]),
            );
        }
        if op == 1 && object.get("kind")?.as_u64()? == 2 && rng.below(4) == 0 {
            object.insert("state".into(), json!(6));
            object.insert("source".into(), json!([[0., 0.75, 0.], [0.2, 0.75, 0.]]));
            object.insert("vertices".into(), json!([[0., 0., 0.]]));
        }
        Some(out)
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Input = case.parse()?;
        let flat = i.flat();
        ffi::validate(&flat).map_err(|e| HarnessError::new(e.to_string()))?;
        match ffi::run(&flat) {
            Ok(v) => Ok(Output::new().values("out", v)),
            Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => Ok(
                Output::rejected("native topology fixture construction failed"),
            ),
            Err(e) => Err(HarnessError::new(e.to_string())),
        }
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Input = case.parse()?;
        ffi::validate(&i.flat()).map_err(|e| HarnessError::new(e.to_string()))?;
        rust(&i).map_err(|e| HarnessError::new(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_pole_in_a_disjoint_v_leaf_is_not_visited() {
        let input: Input = serde_json::from_value(json!({
            "op":1,"kind":2,"state":6,
            "source":[[0.,0.75,0.],[0.2,0.75,0.]],"vertices":[[0.,0.,0.]]
        }))
        .unwrap();
        let result = rust(&input).unwrap();
        assert!(result.deferred.is_none());
        assert!(result.error.is_none());
        let out = &result.values["out"];
        assert_eq!(&out[..4], &[0., 1., 0., 1.]);
        assert_eq!(out[7], -1.); // No pole vertex classification at the source start.
    }
    #[test]
    fn positive_start_vertex_changes_the_classification_record() {
        let input: Input =
            serde_json::from_value(json!({"op":1,"vertices":[[0.2,0.5,0.]]})).unwrap();
        let result = rust(&input).unwrap();
        assert!(result.deferred.is_none());
        assert_eq!(result.values["out"][7], 100.);
    }
}
