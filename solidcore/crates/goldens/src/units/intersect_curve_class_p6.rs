//! Curve-classification interval integration (part 6), ported from upstream
//! `source/SMLib/src/SmCurveClass.cpp`; checked against its golden.
//! Classifications are built with p1 setters. Unported p2/p7 calls defer the whole case.
//! Construction failures are harness errors.
//!
//! The topology objects are those of an `SmApiCreateBox(0, 1, 1, 1)` B-rep (face,
//! edge, vertex and region: the first of `GetFaces`, `GetEdges`, `GetVertices`,
//! `GetRegions`), built with the Rust kernel (unit `heal.stitch_p1`'s `own_unit_box`); the
//! handles resolve to it through the `ClassGeometry` hooks, so their tolerances, points and
//! topology are the ported ones. Each case works on a fresh copy of that box, and
//! `MergeInterval` edits it through `MergeIntervalBrep`; a merge also reports the box
//! after it (counts, sorted vertex and edge tolerances, the new vertices and edges).
//! `InsertCurveInterval` runs through `insert_curve_interval_mut` (its `Combine` may move
//! the picked vertex to a surf/surf/surf point, `SmVertex::SetPoint`) and reports that
//! vertex's point after it.
//! The `imprint_*` cases pick other box objects and place lines on the box, with its
//! editing enabled, so the vertex, face-split, coincident-edge and wire-edge branches
//! run to completion.

use crate::{Case, CaseContext, CaseSource, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::intersect::curve_class::p5::ClassGeometryMut;
use solidcore::intersect::curve_class::p6::{MergeContext, MergeIntervalBrep, MergeIntervalOutput};
use solidcore::intersect::curve_class::{CurveClassification, CurveInterval};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::edge as topo_edge;
use solidcore::topology::face::p2 as topo_face;
use solidcore::topology::vertex as topo_vertex;
use solidcore::topology::Brep;
use solidcore::{EntityRef, SolidError};
use std::cell::OnceCell;
use std::rc::Rc;

/// Numbers per interval (two endpoints, three eight-number classifications).
const WIDTH: usize = 26;

/// Curve-classification part 6 runner.
pub struct Runner;

#[path = "intersect_curve_class_p6/paired_fillet.rs"]
mod paired_fillet;

#[derive(Clone, Serialize, Deserialize)]
struct Fixture {
    /// The box's face, edge, vertex and region (indices into its lists) the
    /// classification kinds 2 to 5 name; the first of each by default.
    #[serde(default)]
    pick: [i32; 4],
    /// Whether the box's editing is enabled (what its edge splits and
    /// `MakeEdgeInFaceWithVU` require); off by default.
    #[serde(default)]
    editing: bool,
    op: i32,
    kind: i32,
    scale: f64,
    tol: f64,
    flags: i32,
    index: usize,
    a: Vec<f64>,
    b: Vec<f64>,
    input: Vec<f64>,
}
/// A classification object: the classification curve, an entity of the case's box
/// (the shim's `SmObject` pointers, new vertices, edges and faces included) or another
/// object (the fillet data's other classification).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Handle {
    Curve,
    Topo(EntityRef),
    Other,
}
impl ClassObject for Handle {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Handle::Curve => ClassObjectKind::Curve,
            Handle::Topo(EntityRef::Face(_)) => ClassObjectKind::Face,
            Handle::Topo(EntityRef::Edge(_)) => ClassObjectKind::Edge,
            Handle::Topo(EntityRef::Vertex(_)) => ClassObjectKind::Vertex,
            Handle::Topo(EntityRef::Region(_)) => ClassObjectKind::Region,
            Handle::Topo(EntityRef::Loop(_)) => ClassObjectKind::Loop,
            Handle::Topo(EntityRef::Vertexuse(_)) => ClassObjectKind::Vertexuse,
            Handle::Topo(EntityRef::Edgeuse(_)) => ClassObjectKind::Edgeuse,
            Handle::Topo(EntityRef::Faceuse(_)) => ClassObjectKind::Faceuse,
            Handle::Topo(EntityRef::Loopuse(_)) => ClassObjectKind::Loopuse,
            Handle::Topo(_) | Handle::Other => ClassObjectKind::Other,
        }
    }
}
/// The classification curve and a fresh copy of the shim's box B-rep (the shim builds a
/// box per case, and `MergeInterval` edits it), with the box's first face, edge, vertex
/// and region (the shim's objects 2 to 5) and the notification state of the edits.
struct Geometry {
    curve: Curve,
    brep: Brep,
    first: [EntityRef; 4],
    records: solidcore::topology::topology::NotifyRecords,
    attributes: solidcore::geom::attribute::AttributeStore,
}
/// The box B-rep and its faces, edges, vertices and regions (in `GetFaces`, `GetEdges`,
/// `GetVertices`, `GetRegions` order).
type BoxTopology = (Brep, [Vec<EntityRef>; 4]);
impl Geometry {
    /// The shim's `Objects::id`: 1 for the curve, 2 to 5 for the box's first face,
    /// edge, vertex and region, -1 for any other object.
    fn id(&self, o: Option<Handle>) -> f64 {
        match o {
            None => 0.0,
            Some(Handle::Curve) => 1.0,
            Some(Handle::Topo(e)) => self
                .first
                .iter()
                .position(|x| *x == e)
                .map_or(-1.0, |i| i as f64 + 2.0),
            Some(Handle::Other) => -1.0,
        }
    }
}
fn curve_ref(edge: solidcore::topology::EdgeId, kind: i64) -> solidcore::geom::object::ObjectRef {
    solidcore::geom::object::ObjectRef {
        key: solidcore::geom::attribute::ObjectKey(((kind as u64) << 32) | u64::from(edge.index())),
        type_code: kind,
    }
}
impl ClassGeometry<Handle> for Geometry {
    fn curve(&self, h: Handle) -> Option<&Curve> {
        (h == Handle::Curve).then_some(&self.curve)
    }
    fn surface(&self, _: Handle) -> Option<&Surface> {
        None
    }
    fn volume(&self, _: Handle) -> Option<&Volume> {
        None
    }
    fn point(&self, _: Handle) -> Option<Vec3> {
        None
    }
    fn classification_vertex_point(&self, o: Handle) -> Option<Vec3> {
        match self_entity(o)? {
            EntityRef::Vertex(v) => topo_vertex::point(&self.brep, v).ok(),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: Handle) -> Option<f64> {
        match self_entity(o)? {
            EntityRef::Vertex(v) => topo_vertex::tolerance(&self.brep, v).ok(),
            EntityRef::Edge(e) => topo_edge::tolerance(&self.brep, e).ok(),
            EntityRef::Face(f) => topo_face::tolerance(&self.brep, f).ok(),
            _ => None,
        }
    }
    fn topology_entity(&self, o: Handle) -> Option<(&Brep, EntityRef)> {
        self_entity(o).map(|e| (&self.brep, e))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<Handle> {
        Some(Handle::Topo(e))
    }
}
impl ClassGeometryMut<Handle> for Geometry {
    fn curve_mut(&mut self, h: Handle) -> Option<&mut Curve> {
        (h == Handle::Curve).then_some(&mut self.curve)
    }
    fn topology_entity_mut(&mut self, o: Handle) -> Option<(&mut Brep, EntityRef)> {
        self_entity(o).map(|e| (&mut self.brep, e))
    }
}
impl MergeIntervalBrep<Handle> for Geometry {
    fn merge_context(&mut self, o: Handle) -> Option<MergeContext<'_, Handle>> {
        Some(MergeContext {
            brep: &mut self.brep,
            target: self_entity(o)?,
            handle: Box::new(Handle::Topo),
            notifications: solidcore::topology::edge::NotifyEnv {
                records: &mut self.records,
                attributes: &mut self.attributes,
                context: None,
                key_of: &super::boolean_topology_intersector_p1::p9_entity_ref,
            },
            curve_ref: &curve_ref,
        })
    }
}
/// The box entity a handle names.
fn self_entity(o: Handle) -> Option<EntityRef> {
    match o {
        Handle::Topo(e) => Some(e),
        _ => None,
    }
}

thread_local! {
    /// The box, imported once per run; each case edits a copy of it.
    static BOX: OnceCell<Rc<BoxTopology>> = const { OnceCell::new() };
}

/// The shared box of [`build_box`].
fn box_topology() -> Result<Rc<BoxTopology>, HarnessError> {
    BOX.with(|cell| {
        if let Some(b) = cell.get() {
            return Ok(Rc::clone(b));
        }
        let b = Rc::new(build_box()?);
        Ok(Rc::clone(cell.get_or_init(|| b)))
    })
}

/// The shim's box (`SmApiCreateBox(0, 1, 1, 1)`, the same call as `heal.stitch_p1`'s box
/// block) and its faces, edges, vertices and regions.
fn build_box() -> Result<BoxTopology, HarnessError> {
    // built by the Rust kernel (no C++ export reaches the Rust side)
    let b = super::heal_stitch_p1::own_unit_box([0.0; 3])?;
    let e = |e: SolidError| h(e.to_string());
    let faces = brep_p1::faces(&b, None).map_err(e)?;
    let edges = brep_p1::edges(&b, None).map_err(e)?;
    let vertices = brep_p1::vertices(&b, None).map_err(e)?;
    let regions = brep_p1::regions(&b, None).map_err(e)?;
    let lists = [
        faces.into_iter().map(EntityRef::Face).collect(),
        edges.into_iter().map(EntityRef::Edge).collect(),
        vertices.into_iter().map(EntityRef::Vertex).collect(),
        regions.into_iter().map(EntityRef::Region).collect(),
    ];
    Ok((b, lists))
}
fn h(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn geometry(k: &Kernel, kind: i32, scale: f64, pick: [i32; 4]) -> Result<Geometry, HarnessError> {
    let domain = Extent1d::from_min_max(0.0, 1.0);
    let c = match kind {
        // the shim's lines placed on the box
        5..=7 => {
            let (o, d) = match kind {
                5 => (Vec3::ZERO, Vec3::new(0.5, 1.0, 0.0)),
                6 => (Vec3::new(1.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0)),
                _ => (Vec3::new(0.25, 0.25, 0.0), Vec3::new(1.0, 0.0, 0.0)),
            };
            Curve::from(Line::bounded(
                o,
                d,
                &domain,
                &BoundedLineOptions {
                    scale,
                    ..Default::default()
                },
            ))
        }
        0 => Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            &domain,
            &BoundedLineOptions {
                scale,
                ..Default::default()
            },
        )),
        1 => Curve::from(
            Ellipse::new_circle(
                k,
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                domain,
                scale,
                &EllipseOptions::default(),
            )
            .map_err(|e| h(e.to_string()))?,
        ),
        2 => Curve::from(
            Ellipse::new(
                k,
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                domain,
                scale,
                scale * 0.5,
                &EllipseOptions::default(),
            )
            .map_err(|e| h(e.to_string()))?,
        ),
        _ => {
            let mut c = allocate_nurb_curve(2, 2, 5).map_err(|e| h(e.to_string()))?;
            for (i, p) in c.cpts_mut().iter_mut().enumerate() {
                p.x = if kind == 4 { scale } else { scale * i as f64 };
                p.y = if kind == 4 {
                    0.0
                } else {
                    scale * if i == 1 { 1.0 } else { 0.0 }
                };
                p.z = 0.0;
                p.w = 1.0;
            }
            c.knots_mut()
                .copy_from_slice(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            Curve::from(BSplineCurve::from_nurb(c, 3, false))
        }
    };
    let b = box_topology()?;
    let mut first = [EntityRef::Face(solidcore::topology::FaceId::from_index(0)); 4];
    for ((slot, list), at) in first.iter_mut().zip(&b.1).zip(pick) {
        *slot = *usize::try_from(at)
            .ok()
            .and_then(|at| list.get(at))
            .ok_or_else(|| h("box without that entity"))?;
    }
    Ok(Geometry {
        curve: c,
        brep: b
            .0
            .clone_standing_for_self()
            .map_err(|e| h(e.to_string()))?,
        first,
        records: solidcore::topology::topology::NotifyRecords::new(),
        attributes: solidcore::geom::attribute::AttributeStore::default(),
    })
}
fn pc(p: &[f64; 8], tol: f64, first: &[EntityRef; 4]) -> PointClassification<Handle> {
    let kind = p[0] as i32;
    let class = match kind {
        1 => PointClassType::Curve,
        2 => PointClassType::Face,
        3 => PointClassType::Edge,
        4 => PointClassType::Vertex,
        5 => PointClassType::Region,
        6 => PointClassType::Nothing,
        _ => PointClassType::Unknown,
    };
    let mut pc = PointClassification::new(tol);
    pc.set_class_object(
        class,
        match kind {
            0 => None,
            1 | 6 => Some(Handle::Curve),
            _ => usize::try_from(kind - 2)
                .ok()
                .and_then(|i| first.get(i))
                .map(|e| Handle::Topo(*e)),
        },
    );
    pc.set_gap_3d(p[1]);
    pc.set_t_param(p[2], false);
    pc.set_pre_snap_param(p[3]);
    if p[4] != 0.0 {
        pc.set_sss_point(Vec3::new(p[5], p[6], p[7]));
    }
    pc
}
fn ivl(p: &[f64], tol: f64, first: &[EntityRef; 4]) -> Result<CurveInterval<Handle>, HarnessError> {
    let [lo, hi, rest @ ..] = p else {
        return Err(h("short interval"));
    };
    let [start, mid, end] = rest.as_chunks::<8>().0 else {
        return Err(h("short classifications"));
    };
    Ok(CurveInterval {
        interval: Extent1d::from_min_max(*lo, *hi),
        start: pc(start, tol, first),
        mid: pc(mid, tol, first),
        end: pc(end, tol, first),
    })
}

fn classification(
    k: &Kernel,
    p: &[f64],
    tol: f64,
    first: &[EntityRef; 4],
) -> Result<CurveClassification<Handle>, HarnessError> {
    let mut c = CurveClassification::new(k);
    c.set_curve(Some(Handle::Curve), false);
    c.set_src_zone_tol_3d(tol);
    c.set_size(k, p.len() / WIDTH)
        .map_err(|e| h(e.to_string()))?;
    for (i, p) in p.as_chunks::<WIDTH>().0.iter().enumerate() {
        *c.at_mut(i).map_err(|e| h(e.to_string()))? = ivl(p, tol, first)?;
    }
    Ok(c)
}
fn put_pc(out: &mut Vec<f64>, pc: &PointClassification<Handle>, g: &Geometry) {
    let p = pc.sss_point();
    out.extend([
        f64::from(pc.point_class().code()),
        g.id(pc.object()),
        pc.deviation(),
        pc.t_param(),
        g.id(pc.face()),
        pc.uv_param().x,
        pc.uv_param().y,
        pc.uvw_param().x,
        pc.uvw_param().y,
        pc.uvw_param().z,
        f64::from(pc.are_parameters_set()),
        pc.pre_snap_param(),
        f64::from(pc.sss_point_flag()),
        p.x,
        p.y,
        p.z,
        pc.src_zone_tol_3d(),
    ]);
}
fn status(e: &SolidError) -> f64 {
    match e {
        e if solidcore::error::upstream_status(e).is_some() => {
            f64::from(solidcore::error::upstream_status(e).unwrap_or(-1))
        }
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        _ => -1.0,
    }
}
fn put_cc(
    k: &Kernel,
    out: &mut Vec<f64>,
    cc: &CurveClassification<Handle>,
    g: &Geometry,
) -> Result<(), HarnessError> {
    out.extend([
        cc.size() as f64,
        cc.interval().min(),
        cc.interval().max(),
        cc.src_zone_tol_3d(),
    ]);
    for i in cc.intervals() {
        out.extend([i.min(), i.max()]);
        put_pc(out, &i.start, g);
        put_pc(out, &i.mid, g);
        put_pc(out, &i.end, g);
        for t in [i.min(), i.interval.evaluate(0.5), i.max()] {
            match g.curve.evaluate_point(k, t) {
                Ok(p) => out.extend([0.0, p.x, p.y, p.z]),
                Err(e) => {
                    if e.is_unported() {
                        return Err(h(e.to_string()));
                    }
                    out.push(status(&e));
                }
            }
        }
    }
    Ok(())
}
/// The box after a merge (the shim's `put_brep`): its vertex, edge, face and region
/// counts, the sorted vertex and edge tolerances, then each new vertex's point and
/// tolerance and each new edge's tolerance.
fn put_brep(
    out: &mut Vec<f64>,
    g: &Geometry,
    merged: &MergeIntervalOutput<Handle>,
) -> solidcore::Result<()> {
    let b = &g.brep;
    let vertices = brep_p1::vertices(b, None)?;
    let edges = brep_p1::edges(b, None)?;
    out.extend([
        vertices.len() as f64,
        edges.len() as f64,
        brep_p1::faces(b, None)?.len() as f64,
        brep_p1::regions(b, None)?.len() as f64,
    ]);
    let mut tols = vertices
        .iter()
        .map(|v| topo_vertex::tolerance(b, *v))
        .collect::<solidcore::Result<Vec<_>>>()?;
    tols.sort_by(f64::total_cmp);
    out.extend(tols);
    let mut tols = edges
        .iter()
        .map(|e| topo_edge::tolerance(b, *e))
        .collect::<solidcore::Result<Vec<_>>>()?;
    tols.sort_by(f64::total_cmp);
    out.extend(tols);
    for v in &merged.new_vertices {
        let Handle::Topo(EntityRef::Vertex(v)) = v else {
            return Err(SolidError::invalid_input("difftest", "new vertex handle"));
        };
        let p = topo_vertex::point(b, *v)?;
        out.extend([p.x, p.y, p.z, topo_vertex::tolerance(b, *v)?]);
    }
    for e in &merged.new_edges {
        let Handle::Topo(EntityRef::Edge(e)) = e else {
            return Err(SolidError::invalid_input("difftest", "new edge handle"));
        };
        out.push(topo_edge::tolerance(b, *e)?);
    }
    Ok(())
}
fn record_pc(kind: i32, t: f64, stored: bool) -> [f64; 8] {
    [
        f64::from(kind),
        0.0,
        t,
        1e20,
        f64::from(stored),
        t,
        2.0 * t,
        -t,
    ]
}
fn record(bounds: [f64; 2], classes: [i32; 3], stored: bool) -> Vec<f64> {
    let mut p = bounds.to_vec();
    for (kind, t) in classes
        .into_iter()
        .zip([bounds[0], (bounds[0] + bounds[1]) / 2.0, bounds[1]])
    {
        p.extend(record_pc(kind, t, stored));
    }
    p
}
fn base() -> Fixture {
    let p = record([0.0, 1.0], [0, 0, 0], false);
    Fixture {
        pick: [0; 4],
        editing: false,
        op: 0,
        kind: 0,
        scale: 1.0,
        tol: 1e-6,
        flags: 0,
        index: 0,
        a: p.clone(),
        b: p.clone(),
        input: p,
    }
}
/// An imprinting case: name, curve kind, scale, pick, classes and the edge
/// parameters of the ends.
type ImprintCase = (&'static str, i32, f64, [i32; 4], [i32; 3], [f64; 2]);
fn add(v: &mut Vec<Case>, name: String, i: Fixture) -> Result<(), HarnessError> {
    v.push(Case {
        unit: "intersect.curve_class_p6".into(),
        name,
        source: CaseSource::Inline,
        inputs: serde_json::to_value(i).map_err(|e| h(e.to_string()))?,
    });
    Ok(())
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p6"
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("paired_fillet").and_then(Value::as_bool) == Some(true) {
            return paired_fillet::rust(paired_fillet::Mode::Metadata);
        }
        if let Some(mode) = paired_fillet::Mode::from_input(&case.inputs) {
            return paired_fillet::rust(mode);
        }
        let i: Fixture = case.parse()?;
        let mut k = Kernel::default();
        let mut g = geometry(&k, i.kind, i.scale, i.pick)?;
        if i.editing {
            g.brep.set_editing_enabled(true);
        }
        let point: &[f64; 3] = i
            .input
            .get(7..10)
            .and_then(|p| p.try_into().ok())
            .ok_or_else(|| h("missing endpoint repair point"))?;
        let [x, y, z] = *point;
        let mut a = classification(&k, &i.a, i.tol, &g.first)?;
        let mut b = classification(&k, &i.b, i.tol, &g.first)?;
        a.set_fillet_data(
            (i.flags & 2 != 0).then_some(Handle::Other),
            i.flags & 2 != 0,
        );
        let result = match i.op {
            // a surf/surf/surf point combined into a vertex boundary moves the box's
            // vertex (SmVertex::SetPoint), as the shim's InsertCurveInterval does
            0 => {
                let input = ivl(&i.input, i.tol, &g.first)?;
                a.insert_curve_interval_mut(&mut k, &mut g, &input)
                    .map(|()| (None, None))
            }
            1 => a.homogenize(&k, &g, &mut b).map(|()| (None, None)),
            2 => a
                .fix_end_point_class(&k, &g, i.index, Vec3::new(x, y, z))
                .map(|changed| (Some(changed), None)),
            // the shim's SetFilletData names `other` (Handle::Other here): pass it so
            // the fillet geometry step reads that classification
            _ => a
                .merge_interval_mut(&mut k, &mut g, i.index, i.flags & 1 != 0, Some(&b))
                .map(|merged| (None, Some(merged))),
        };
        match result {
            Err(e) => Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::new().values("out", vec![status(&e)]))),
            Ok((changed, merged)) => {
                let mut out = vec![0.0];
                if let Some(merged) = &merged {
                    for items in [&merged.new_faces, &merged.new_edges, &merged.new_vertices] {
                        out.push(items.len() as f64);
                        out.extend(items.iter().map(|o| g.id(Some(*o))));
                    }
                }
                if let Some(b) = changed {
                    out.push(f64::from(b));
                }
                put_cc(&k, &mut out, &a, &g)?;
                put_cc(&k, &mut out, &b, &g)?;
                if let Some(merged) = &merged {
                    put_brep(&mut out, &g, merged).map_err(|e| h(e.to_string()))?;
                }
                // the picked vertex after InsertCurveInterval (a combined
                // surf/surf/surf point moves it through SmVertex::SetPoint)
                if i.op == 0 {
                    let Some(EntityRef::Vertex(v)) = g.first.get(2).copied() else {
                        return Err(h("picked vertex is not a vertex"));
                    };
                    let p = topo_vertex::point(&g.brep, v).map_err(|e| h(e.to_string()))?;
                    out.extend([p.x, p.y, p.z]);
                }
                Ok(Output::new().values("out", out))
            }
        }
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut out = crate::runner::default_cases(self, ctx)?;
        out.push(Case {
            unit: self.id().into(),
            name: "paired-fillet-real-boxes".into(),
            source: CaseSource::Inline,
            inputs: serde_json::json!({"paired_fillet":true}),
        });
        for (name, mode) in [
            ("linked", "linked"),
            ("legacy-no-peer", "legacy-none"),
            ("linked-disabled", "linked-disabled"),
        ] {
            out.push(Case {
                unit: self.id().into(),
                name: format!("paired-fillet-real-boxes-{name}"),
                source: CaseSource::Inline,
                inputs: serde_json::json!({"paired_fillet":mode}),
            });
        }
        for kind in 0..5 {
            // Competing classifications exercise receiver/argument order, gap
            // precedence and saved-point propagation, rather than only assigning
            // a class to UNKNOWN. These stay on the fully ported insertion path.
            for target in 0..27 {
                for incoming in 0..27 {
                    let classes = |code| {
                        let k = |n| match n {
                            0 => 0,
                            1 => 1,
                            _ => 6,
                        };
                        [k(code / 9), k(code / 3 % 3), k(code % 3)]
                    };
                    let mut i = base();
                    i.kind = kind;
                    i.flags = (target + incoming) % 4;
                    i.a = record([0.0, 1.0], classes(target), target % 2 == 0);
                    i.input = record([0.0, 1.0], classes(incoming), incoming % 2 == 0);
                    for pc in
                        i.a.get_mut(2..)
                            .ok_or_else(|| h("short interval"))?
                            .as_chunks_mut::<8>()
                            .0
                    {
                        pc[1] = 0.125;
                    }
                    for pc in i
                        .input
                        .get_mut(2..)
                        .ok_or_else(|| h("short interval"))?
                        .as_chunks_mut::<8>()
                        .0
                    {
                        pc[1] = 0.25;
                        pc[2] += 0.125;
                        pc[3] = pc[2];
                        pc[5] += 3.0;
                    }
                    add(
                        &mut out,
                        format!("precedence_k{kind}_{target}_{incoming}"),
                        i,
                    )?;
                }
            }
            for flags in 0..4 {
                for op in 0..4 {
                    for classes in [
                        [0, 0, 0],
                        [1, 1, 1],
                        [2, 2, 2],
                        [3, 3, 3],
                        [4, 3, 4],
                        [5, 5, 5],
                        [6, 6, 6],
                    ] {
                        // MergeInterval's contract is topological classification. Geometry
                        // and NOTHING interiors do not denote a target B-rep region/face/edge.
                        if op == 3 && flags & 1 == 0 && matches!(classes[1], 1 | 6) {
                            continue;
                        }
                        let mut i = base();
                        i.kind = kind;
                        i.flags = flags;
                        i.op = op;
                        i.a = record([0.0, 1.0], classes, false);
                        i.input = record([0.0, 1.0], classes, true);
                        add(
                            &mut out,
                            format!(
                                "matrix_k{kind}_f{flags}_o{op}_c{}{}{}",
                                classes[0], classes[1], classes[2]
                            ),
                            i,
                        )?;
                    }
                }
            }
            for (n, scale) in [1e-15, 1e-9, 1e-6, 1.0, 1e6, 1e9].into_iter().enumerate() {
                for bounds in [
                    [0.0, 1.0],
                    [0.0, 0.0],
                    [1.0, 1.0],
                    [0.25, 0.75],
                    [0.0, 1e-12],
                    [1.0 - 1e-12, 1.0],
                ] {
                    let mut i = base();
                    i.kind = kind;
                    i.scale = scale;
                    i.input = record(bounds, [0, 0, 0], true);
                    add(
                        &mut out,
                        format!("scale_k{kind}_{n}_{}_{}", bounds[0], bounds[1]),
                        i,
                    )?;
                }
            }
            for (n, bounds) in [
                [0.0, 0.25, 1.0],
                [0.0, 1e-10, 1.0],
                [0.0, 0.9999999999, 1.0],
            ]
            .into_iter()
            .enumerate()
            {
                for op in 0..3 {
                    let mut i = base();
                    i.kind = kind;
                    i.op = op;
                    i.a = record([bounds[0], bounds[1]], [0, 0, 0], false);
                    i.a.extend(record([bounds[1], bounds[2]], [0, 0, 0], false));
                    add(&mut out, format!("partition_k{kind}_o{op}_{n}"), i)?;
                }
            }
        }
        // MergeInterval imprinting on the box (faces walk F(z=0) first; edges in id
        // order, 11 = (0,1,0)-(1,1,0); vertex 2 = (0,0,0)): the curve kinds placed on
        // it, the classification kinds naming the picked face, edge and vertex.
        // (name, curve kind, scale, pick, classes, edge parameters of the ends)
        let imprints: [ImprintCase; 7] = [
            // vertex -> face -> edge: splits the far edge, then the face
            ("face_split", 5, 1.0, [0, 11, 2, 0], [4, 2, 3], [0.0, 0.5]),
            // face -> face -> face inside the face: two vertex loops joined by an edge
            ("face_chord", 7, 0.5, [0, 0, 0, 0], [2, 2, 2], [0.0, 1.0]),
            // a degenerate chord: one vertex loop
            ("face_point", 7, 1e-12, [0, 0, 0, 0], [2, 2, 2], [0.0, 1.0]),
            // edge -> edge -> edge along the edge (1,0,1)-(1,1,1): its end vertices
            (
                "edge_coincident",
                6,
                1.0,
                [0, 0, 0, 0],
                [3, 3, 3],
                [0.0, 1.0],
            ),
            // part of that edge: splits it at both ends
            ("edge_inner", 6, 0.5, [0, 0, 0, 0], [3, 3, 3], [0.0, 0.5]),
            // region -> region -> region: a wire edge between two shell vertices
            ("region_wire", 7, 0.5, [0, 0, 0, 0], [5, 5, 5], [0.0, 1.0]),
            // face -> region -> face: unmergeable mid on face end vertices
            ("face_region", 7, 0.5, [0, 0, 0, 0], [2, 5, 2], [0.0, 1.0]),
        ];
        for (name, kind, scale, pick, classes, params) in imprints {
            for flags in 0..4 {
                let mut i = base();
                i.op = 3;
                i.kind = kind;
                i.scale = scale;
                i.flags = flags;
                i.pick = pick;
                i.editing = true;
                i.a = record([0.0, 1.0], classes, false);
                // the ends' parameters on the objects they classify to
                for (at, t) in [(2 + 2, params[0]), (2 + 16 + 2, params[1])] {
                    *i.a.get_mut(at).ok_or_else(|| h("short interval"))? = t;
                }
                i.b.clone_from(&i.a);
                add(&mut out, format!("imprint_{name}_f{flags}"), i)?;
            }
        }
        let mut empty = base();
        empty.op = 1;
        empty.a.clear();
        empty.b.clear();
        add(&mut out, "empty_pair".into(), empty)?;
        Ok(out)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let mut i = base();
        i.op = rng.below(4) as i32;
        i.kind = rng.below(5) as i32;
        i.flags = rng.below(4) as i32;
        i.scale = 10.0_f64.powf(rng.uniform(-6.0, 6.0));
        i.tol = 10.0_f64.powf(rng.uniform(-10.0, -3.0));
        let a = rng.unit();
        let b = rng.unit();
        i.input = record([a.min(b), a.max(b)], [0, 0, 0], rng.chance(0.5));
        if i.op == 2 {
            i.index = rng.below(2) as usize;
            let point: Vec<f64> = (0..3).map(|_| rng.uniform(-2.0, 2.0)).collect();
            i.input.get_mut(7..10)?.copy_from_slice(&point);
        }
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::geom::tol::scaled_zero_of_curve;
    use solidcore::intersect::curve_class::p1::xsect_tol_2d_or_3d_for_classify_pair;
    use solidcore::curve::curve::p2::CurveKind;
    use solidcore::math::EFF_ZERO_PARAM;

    /// #18342: `scale_k3_5_0_0.000000000001` and `scale_k3_5_0.999999999999_1` insert a
    /// 1e-12 parameter range at an end of the quadratic B-spline of scale 1e9. That range
    /// is 2.83e-3 long in 3D: at least the classification's pair tolerance and more than
    /// the curve's scaled zero (2.27e-3, a margin of only 1.25x, so it is asserted here).
    /// The parameter snap used to merge it into the whole-curve interval; it now stays an
    /// interval of its own, next to the rest of the curve.
    #[test]
    fn a_short_end_range_distinct_in_3d_stays_an_interval() {
        for bounds in [[0.0, 1e-12], [1.0 - 1e-12, 1.0]] {
            let mut i = base();
            i.kind = 3;
            i.scale = 1e9;
            i.input = record(bounds, [0, 0, 0], true);
            let mut k = Kernel::default();
            let mut g = geometry(&k, i.kind, i.scale, i.pick).unwrap();
            let mut a = classification(&k, &i.a, i.tol, &g.first).unwrap();
            let input = ivl(&i.input, i.tol, &g.first).unwrap();
            a.insert_curve_interval_mut(&mut k, &mut g, &input).unwrap();
            assert_eq!(a.size(), 2, "{bounds:?}");
            let scaled_zero = scaled_zero_of_curve(&k, &g.curve).unwrap();
            let mut short = 0;
            for iv in a.intervals() {
                let (lo, hi) = (iv.min(), iv.max());
                if hi - lo > EFF_ZERO_PARAM {
                    continue;
                }
                short += 1;
                assert_eq!((lo, hi), (bounds[0], bounds[1]));
                let zone_a = iv.start.obj_zone_tol_3d(&k, &g).unwrap();
                let zone_b = iv.end.obj_zone_tol_3d(&k, &g).unwrap();
                let tol = xsect_tol_2d_or_3d_for_classify_pair(
                    &k, &g.curve, None, lo, zone_a, hi, zone_b,
                )
                .unwrap();
                let d = g
                    .curve
                    .evaluate_point(&k, lo)
                    .unwrap()
                    .distance(g.curve.evaluate_point(&k, hi).unwrap());
                assert!(
                    d >= tol && d > scaled_zero,
                    "{bounds:?}: {d:e} {tol:e} {scaled_zero:e}"
                );
            }
            assert_eq!(short, 1, "{bounds:?}");
        }
    }

    /// WR-469 (tangency-labs/solidcraft#17985): `imprint_face_split_f0` and `_f2` place the
    /// line from the origin along (1, 2, 0)/sqrt 5, of length 1, and end its new edge at a
    /// new vertex 0.059 past the line's end. `MakeEdgeInFaceWithVU`'s step makes that
    /// vertex's tolerance exactly twice its gap (0.11804298874989479), so the end lies within
    /// half of it; without the step `RefreshTolerance(FALSE)` left 0.059022494374947436,
    /// 1e-6 over the gap and under twice it (MEASURED, before the step became the only
    /// path). The row's two moved values are the new vertex's tolerance: in the sorted
    /// vertex tolerances (`out[159]`, the only one past 1e-5) and after its point
    /// (`out[177]`). Replays both rows.
    #[test]
    fn imprint_face_split_new_vertex_tolerance_is_twice_its_gap() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let cases = Runner.cases(&ctx).unwrap();
        // the placed line's end, as the case builds it
        let k = Kernel::default();
        let g = geometry(&k, 5, 1.0, [0, 11, 2, 0]).unwrap();
        let end = g.curve.evaluate_point(&k, 1.0).unwrap();
        // the allowances are relative 1e-12: the values are unit-scale distances, which
        // rounding moves by a few ulp (about 1e-16 relative), and doubling is exact; a
        // missing step moves the tolerance by half (0.059 against 0.118)
        assert!(end.distance(Vec3::new(1.0, 2.0, 0.0) * (1.0 / 5f64.sqrt())) <= 1e-12);
        for name in ["imprint_face_split_f0", "imprint_face_split_f2"] {
            let case = cases.iter().find(|c| c.name == name).unwrap();
            let out = &Runner.run_rust(case).unwrap().values["out"];
            assert_eq!(out.len(), 181, "{name}");
            let vertex = Vec3::new(out[174], out[175], out[176]);
            let gap = vertex.distance(end);
            assert!(gap > 0.059 && gap < 0.0591, "{name}: gap {gap:e}");
            assert!(
                (out[177] - 2.0 * gap).abs() <= 1e-12 * out[177],
                "{name}: {} {gap:e}",
                out[177]
            );
            assert_eq!(out[159], out[177], "{name}");
            let others = out[151..159].iter().chain(&out[160..174]);
            assert!(others.copied().all(|t| t == 1e-5), "{name}: {out:?}");
        }
    }

    /// The ends of line edge `e` (asserting it is a line).
    fn line_ends(k: &Kernel, br: &Brep, e: solidcore::topology::EdgeId) -> (Vec3, Vec3) {
        let c = topo_edge::curve(br, e).unwrap().unwrap();
        assert!(matches!(c.kind(), CurveKind::Line(_)), "{e:?}");
        let iv = topo_edge::interval(br, e).unwrap();
        (
            c.evaluate_point(k, iv.min()).unwrap(),
            c.evaluate_point(k, iv.max()).unwrap(),
        )
    }

    /// The distance from `p` to the segment `a`-`b`: the clamped projection.
    fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f64 {
        let d = b - a;
        let t = ((p - a).dot(d) / d.dot(d)).clamp(0.0, 1.0);
        p.distance(a + d * t)
    }

    /// The distance from `p` to the convex planar polygon `poly`: the height over its plane
    /// when the foot falls inside, else the distance to its nearest side.
    fn polygon_distance(p: Vec3, poly: &[Vec3]) -> f64 {
        let n = (poly[1] - poly[0]).cross(poly[2] - poly[0]).unitize().unwrap();
        let h = (p - poly[0]).dot(n);
        let foot = p - n * h;
        let sides = poly.iter().zip(poly.iter().cycle().skip(1));
        let inside = sides
            .clone()
            .all(|(a, b)| (*b - *a).cross(foot - *a).dot(n) >= 0.0)
            || sides.clone().all(|(a, b)| (*b - *a).cross(foot - *a).dot(n) <= 0.0);
        if inside {
            h.abs()
        } else {
            sides
                .map(|(a, b)| segment_distance(p, *a, *b))
                .fold(f64::INFINITY, f64::min)
        }
    }

    /// WR-469 at a gap the validity rules accept (the companion of
    /// `imprint_face_split_new_vertex_tolerance_is_twice_its_gap`, whose made-up 0.059 gap is
    /// 5900 times the box's tolerance, past the 2000 times that §5.1 of
    /// `docs/validity-and-tolerances.md` refuses whatever the clearance, on main as with the
    /// step). The same `imprint_face_split_f0` imprint with the line shortened to
    /// `sqrt(1.25) - 4e-5`: its end lies on the ray through the edge point (0.5, 1, 0), 4e-5
    /// short of it, and the new vertex lands between the two. Asserts, from the body:
    ///
    /// - the step: the vertex tolerance `t` is twice `d`, the farthest end of an edge at the
    ///   vertex (so every end lies within `t/2`, `AssertValid`'s limit), and `d` is at least
    ///   half the 4e-5 span (the vertex lies between the ends) and at most all of it;
    /// - §5.1: `t` is under 1000 times the body's tolerance (the ratio test passes), and
    ///   `validate_pointers` reports no Test #82;
    /// - §8 / §9 invariant 7: the exact clearance from the vertex to every vertex, edge and face
    ///   it does not bound (the box's edges are lines and its faces planar polygons, so the
    ///   distances are closed form) is greater than `t`, and `vertex_ball_reaches` agrees.
    ///
    /// Allowances: the ends and the vertex are unit-scale coordinates (ulp 2.2e-16), so `d`
    /// (about 2.45e-5) carries an absolute error of a few ulp of 1; 1e-14 on `t - 2d` is
    /// about 45 such ulp and two orders below the 2.45e-5 a missing step would move it by.
    /// MEASURED: t = 4.9e-5, clearance 0.49999 (the face x = 0 and the edge (0, y, 0)).
    /// The box's edges fail `ValidateGeometry` (Test #61) before the imprint as on main (12
    /// of 12), a property of the case's box, so only Test #61 on edges may be reported.
    #[test]
    fn imprint_face_split_at_a_valid_gap_keeps_clear_of_the_box() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let cases = Runner.cases(&ctx).unwrap();
        let case = cases
            .iter()
            .find(|c| c.name == "imprint_face_split_f0")
            .unwrap();
        let mut i: Fixture = case.parse().unwrap();
        i.scale = 1.25f64.sqrt() - 4e-5;
        let mut k = Kernel::default();
        let mut g = geometry(&k, i.kind, i.scale, i.pick).unwrap();
        g.brep.set_editing_enabled(true);
        let mut a = classification(&k, &i.a, i.tol, &g.first).unwrap();
        let b = classification(&k, &i.b, i.tol, &g.first).unwrap();
        a.set_fillet_data(None, false);
        a.merge_interval_mut(&mut k, &mut g, i.index, false, Some(&b))
            .unwrap();
        let br = &g.brep;
        let body = br.tolerance();
        assert_eq!(body, 1e-5);
        let vertices = brep_p1::vertices(br, None).unwrap();
        let wide: Vec<_> = vertices
            .iter()
            .copied()
            .filter(|v| topo_vertex::tolerance(br, *v).unwrap() > body)
            .collect();
        assert_eq!(wide.len(), 1, "{wide:?}");
        let v = wide[0];
        let p = topo_vertex::point(br, v).unwrap();
        let t = topo_vertex::tolerance(br, v).unwrap();

        // the step: t = 2d, d the farthest edge end at the vertex
        let at_v = topo_vertex::edges(br, v, None).unwrap();
        let mut d: f64 = 0.0;
        for e in &at_v {
            let vs = topo_vertex::edge_vertices(br, *e).unwrap();
            let (s0, s1) = line_ends(&k, br, *e);
            for (end, ev) in [(s0, vs[0]), (s1, vs[1])] {
                if ev == Some(v) {
                    d = d.max(p.distance(end));
                }
            }
        }
        assert!((2e-5..=4e-5).contains(&d), "d {d:e}");
        assert!((t - 2.0 * d).abs() <= 1e-14, "t {t:e} d {d:e}");

        // §5.1: under the ratio limit, and no Test #82
        assert!(t < solidcore::geom::tol::LARGE_SMALL_SIZE_RATIO * body, "t {t:e}");
        let mut reports = solidcore::geom::assert_array::AssertArray::new();
        solidcore::topology::brep::validation::validate_pointers(
            &k,
            &mut g.brep,
            Some(&mut reports),
            0,
            false,
            &crate::own_reader::read_key_of,
        )
        .unwrap();
        let br = &g.brep;
        let edges = brep_p1::edges(br, None).unwrap();
        for r in reports.reports() {
            assert!(r.test_index == 61 && r.owner_type_string == Some("SmEdge"), "{r:?}");
        }
        assert!(reports.len() <= edges.len());

        // §8: the exact clearance to everything the vertex does not bound
        let faces_at_v = topo_vertex::faces(br, v).unwrap();
        let mut bound = vec![v];
        for e in &at_v {
            bound.extend(topo_vertex::edge_vertices(br, *e).unwrap().into_iter().flatten());
        }
        let mut clearance = f64::INFINITY;
        for w in vertices.iter().filter(|w| !bound.contains(w)) {
            clearance = clearance.min(p.distance(topo_vertex::point(br, *w).unwrap()));
        }
        for e in edges.iter().filter(|e| !at_v.contains(e)) {
            let (s0, s1) = line_ends(&k, br, *e);
            clearance = clearance.min(segment_distance(p, s0, s1));
        }
        for f in brep_p1::faces(br, None).unwrap() {
            if faces_at_v.contains(&f) {
                continue;
            }
            let loops = topo_face::loops(br, f, false, None).unwrap();
            assert_eq!(loops.len(), 1, "{f:?}");
            // its sides are edges, all lines (`line_ends` asserted it for every edge above)
            let poly: Vec<Vec3> = topo_vertex::loop_vertices(br, loops[0])
                .unwrap()
                .into_iter()
                .map(|w| topo_vertex::point(br, w.unwrap()).unwrap())
                .collect();
            let n = (poly[1] - poly[0]).cross(poly[2] - poly[0]).unitize().unwrap();
            assert!(poly.iter().all(|q| (*q - poly[0]).dot(n).abs() <= 1e-15), "{f:?}");
            clearance = clearance.min(polygon_distance(p, &poly));
        }
        assert!(clearance > t, "clearance {clearance:e} t {t:e}");
        assert!(
            !solidcore::topology::brep::validation::vertex_ball_reaches(&k, &mut g.brep, v)
                .unwrap()
        );
    }
}
