//! Real Rust/C++ comparisons for curve-classification interval integration (part 6).
//! Classifications are built with p1 setters; native geometry uses its true dynamic type.
//! Unported p2/p7 calls defer the whole case. Construction failures are harness errors.
//! Explicit `expected_rejection` fixtures check FFI rejection only; they are reported
//! as both-errored cases, separately from successful kernel comparisons.
//!
//! The topology objects are those of the shim's `SmApiCreateBox(0, 1, 1, 1)` B-rep (face,
//! edge, vertex and region: the first of `GetFaces`, `GetEdges`, `GetVertices`,
//! `GetRegions`). The Rust side imports the same box through unit `heal.stitch_p1`'s
//! export (`topology.brep_p1`'s importer) and resolves the handles to it through the
//! `ClassGeometry` hooks, so their tolerances, points and topology are the ported ones.

use crate::{Case, CaseContext, CaseSource, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::OnceCell;
use std::rc::Rc;
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
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
use solidcore_ffi::units::topology_brep_p1::counts;
use solidcore_ffi::units::intersect_curve_class_p6 as ffi;

/// Curve-classification part 6 oracle.
pub struct Runner;

#[derive(Clone, Serialize, Deserialize)]
struct Fixture {
    #[serde(default)]
    expected_rejection: bool,
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
#[derive(Clone, Copy, PartialEq)]
struct Handle(i32);
impl ClassObject for Handle {
    fn class_kind(&self) -> ClassObjectKind {
        match self.0 {
            1 => ClassObjectKind::Curve,
            2 => ClassObjectKind::Face,
            3 => ClassObjectKind::Edge,
            4 => ClassObjectKind::Vertex,
            5 => ClassObjectKind::Region,
            _ => ClassObjectKind::Other,
        }
    }
}
/// The classification curve and the shim's box B-rep with its face, edge, vertex and
/// region objects (handles 2 to 5).
struct Geometry(Curve, Rc<BoxTopology>);
/// The box B-rep and its face, edge, vertex and region.
type BoxTopology = (Brep, [EntityRef; 4]);
impl Geometry {
    fn entity_of(&self, o: Handle) -> Option<EntityRef> {
        let i = usize::try_from(o.0.checked_sub(2)?).ok()?;
        self.1 .1.get(i).copied()
    }
}
impl ClassGeometry<Handle> for Geometry {
    fn curve(&self, h: Handle) -> Option<&Curve> {
        (h.0 == 1).then_some(&self.0)
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
        match self.entity_of(o)? {
            EntityRef::Vertex(v) => topo_vertex::point(&self.1 .0, v).ok(),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: Handle) -> Option<f64> {
        match self.entity_of(o)? {
            EntityRef::Vertex(v) => topo_vertex::tolerance(&self.1 .0, v).ok(),
            EntityRef::Edge(e) => topo_edge::tolerance(&self.1 .0, e).ok(),
            EntityRef::Face(f) => topo_face::tolerance(&self.1 .0, f).ok(),
            _ => None,
        }
    }
    fn topology_entity(&self, o: Handle) -> Option<(&Brep, EntityRef)> {
        self.entity_of(o).map(|e| (&self.1 .0, e))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<Handle> {
        let i = self.1 .1.iter().position(|x| *x == e)?;
        Some(Handle(i32::try_from(i).ok()? + 2))
    }
}

thread_local! {
    /// The box, built once per run: cases only read it (classification takes the
    /// geometry by shared reference and `Brep` has no interior mutability).
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
/// block) and its first face, edge, vertex and region.
fn build_box() -> Result<BoxTopology, HarnessError> {
    let ex = super::heal_stitch_p1::export(&super::heal_stitch_p1::StitchBrep::Box {
        p: [0.0; 3],
        size: [1.0; 3],
    })?;
    let (_, graph_at) = counts(&ex).ok_or_else(|| h("box export failed"))?;
    let (b, _) = super::topology_brep_p1::import(&ex, graph_at)?;
    let e = |e: SolidError| h(e.to_string());
    let first = |v: Option<EntityRef>| v.ok_or_else(|| h("box without that entity"));
    let face = first(brep_p1::faces(&b, None).map_err(e)?.first().map(|x| EntityRef::Face(*x)))?;
    let edge = first(brep_p1::edges(&b, None).map_err(e)?.first().map(|x| EntityRef::Edge(*x)))?;
    let vertex = first(
        brep_p1::vertices(&b, None)
            .map_err(e)?
            .first()
            .map(|x| EntityRef::Vertex(*x)),
    )?;
    let region = first(
        brep_p1::regions(&b, None)
            .map_err(e)?
            .first()
            .map(|x| EntityRef::Region(*x)),
    )?;
    Ok((b, [face, edge, vertex, region]))
}
fn h(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn geometry(k: &Kernel, kind: i32, scale: f64) -> Result<Geometry, HarnessError> {
    let domain = Extent1d::from_min_max(0.0, 1.0);
    let c = match kind {
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
    Ok(Geometry(c, box_topology()?))
}
fn pc(p: &[f64; 8], tol: f64) -> PointClassification<Handle> {
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
        if kind == 0 {
            None
        } else {
            Some(Handle(if kind == 6 { 1 } else { kind }))
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
fn ivl(p: &[f64], tol: f64) -> Result<CurveInterval<Handle>, HarnessError> {
    let [lo, hi, rest @ ..] = p else {
        return Err(h("short interval"));
    };
    let [start, mid, end] = rest.as_chunks::<8>().0 else {
        return Err(h("short classifications"));
    };
    Ok(CurveInterval {
        interval: Extent1d::from_min_max(*lo, *hi),
        start: pc(start, tol),
        mid: pc(mid, tol),
        end: pc(end, tol),
    })
}

fn classification(
    k: &Kernel,
    p: &[f64],
    tol: f64,
) -> Result<CurveClassification<Handle>, HarnessError> {
    let mut c = CurveClassification::new(k);
    c.set_curve(Some(Handle(1)), false);
    c.set_src_zone_tol_3d(tol);
    c.set_size(k, p.len() / ffi::WIDTH)
        .map_err(|e| h(e.to_string()))?;
    for (i, p) in p.as_chunks::<{ ffi::WIDTH }>().0.iter().enumerate() {
        *c.at_mut(i).map_err(|e| h(e.to_string()))? = ivl(p, tol)?;
    }
    Ok(c)
}
fn put_pc(out: &mut Vec<f64>, pc: &PointClassification<Handle>) {
    let p = pc.sss_point();
    out.extend([
        f64::from(pc.point_class().code()),
        f64::from(pc.object().map_or(0, |o| o.0)),
        pc.deviation(),
        pc.t_param(),
        f64::from(pc.face().map_or(0, |o| o.0)),
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
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.0,
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
        put_pc(out, &i.start);
        put_pc(out, &i.mid);
        put_pc(out, &i.end);
        for t in [i.min(), i.interval.evaluate(0.5), i.max()] {
            match g.0.evaluate_point(k, t) {
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
        expected_rejection: false,
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
fn add(v: &mut Vec<Case>, name: String, i: Fixture) -> Result<(), HarnessError> {
    v.push(Case {
        unit: "intersect.curve_class_p6".into(),
        name,
        source: CaseSource::Inline,
        inputs: serde_json::to_value(i).map_err(|e| h(e.to_string()))?,
    });
    Ok(())
}
fn rejected<T>(result: Result<T, solidcore_ffi::FfiError>) -> Result<Output, HarnessError> {
    match result {
        Err(
            solidcore_ffi::FfiError::InvalidArgument(_)
            | solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG),
        ) => Ok(Output::err("fixture_rejected")),
        Err(e) => Err(h(format!("unexpected rejection: {e}"))),
        Ok(_) => Err(h("expected invalid fixture to be rejected")),
    }
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p6"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Fixture = case.parse()?;
        let result = ffi::run(
            i.op, i.kind, i.scale, i.tol, i.flags, i.index, &i.a, &i.b, &i.input,
        );
        if i.expected_rejection {
            return rejected(result);
        }
        result
            .map(|v| Output::new().values("out", v))
            .map_err(|e| h(e.to_string()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Fixture = case.parse()?;
        let validation = ffi::validate(
            i.op, i.kind, i.scale, i.tol, i.flags, i.index, &i.a, &i.b, &i.input,
        );
        if i.expected_rejection {
            return rejected(validation);
        }
        validation.map_err(|e| h(e.to_string()))?;
        let k = Kernel::default();
        let g = geometry(&k, i.kind, i.scale)?;
        let point: &[f64; 3] = i
            .input
            .get(7..10)
            .and_then(|p| p.try_into().ok())
            .ok_or_else(|| h("missing endpoint repair point"))?;
        let [x, y, z] = *point;
        let mut a = classification(&k, &i.a, i.tol)?;
        let mut b = classification(&k, &i.b, i.tol)?;
        a.set_fillet_data((i.flags & 2 != 0).then_some(Handle(7)), i.flags & 2 != 0);
        let result = match i.op {
            0 => a
                .insert_curve_interval(&k, &g, &ivl(&i.input, i.tol)?)
                .map(|()| (None, None)),
            1 => a.homogenize(&k, &g, &mut b).map(|()| (None, None)),
            2 => a
                .fix_end_point_class(&k, &g, i.index, Vec3::new(x, y, z))
                .map(|changed| (Some(changed), None)),
            _ => a
                .merge_interval(&k, &g, i.index, i.flags & 1 != 0)
                .map(|merged| (None, Some(merged))),
        };
        match result {
            Err(e) => Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::new().values("out", vec![status(&e)]))),
            Ok((changed, merged)) => {
                let mut out = vec![0.0];
                if let Some(merged) = merged {
                    for items in [merged.new_faces, merged.new_edges, merged.new_vertices] {
                        out.push(items.len() as f64);
                        out.extend(items.iter().map(|o| f64::from(o.0)));
                    }
                }
                if let Some(b) = changed {
                    out.push(f64::from(b));
                }
                put_cc(&k, &mut out, &a, &g)?;
                put_cc(&k, &mut out, &b, &g)?;
                Ok(Output::new().values("out", out))
            }
        }
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut out = crate::runner::default_cases(self, ctx)?;
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
