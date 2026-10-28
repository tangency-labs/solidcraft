//! Actual paired classifications; each role resolves into one retained real box.
use super::{
    brep_p1, ffi, h, status, topo_edge, Brep, ClassGeometry, ClassGeometryMut, ClassObject,
    ClassObjectKind, Curve, CurveClassification, CurveInterval, EntityRef, HarnessError, Kernel,
    MergeContext, MergeIntervalBrep, Output, PointClassType, PointClassification, Surface, Vec3,
    Volume,
};
use solidcore::topology::{EdgeId, VertexId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Metadata,
    Linked,
    LegacyNone,
    LinkedDisabled,
}
impl Mode {
    pub(super) fn from_input(input: &serde_json::Value) -> Option<Self> {
        match input.get("paired_fillet")?.as_str()? {
            "linked" => Some(Self::Linked),
            "legacy-none" => Some(Self::LegacyNone),
            "linked-disabled" => Some(Self::LinkedDisabled),
            _ => None,
        }
    }
    fn code(self) -> i32 {
        match self {
            Self::Metadata => 0,
            Self::Linked => 1,
            Self::LegacyNone => 2,
            Self::LinkedDisabled => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Target,
    Other,
}
impl Side {
    fn get<T>(self, pair: &[T; 2]) -> &T {
        let [target, other] = pair;
        match self {
            Self::Target => target,
            Self::Other => other,
        }
    }
    fn get_mut<T>(self, pair: &mut [T; 2]) -> &mut T {
        let [target, other] = pair;
        match self {
            Self::Target => target,
            Self::Other => other,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Object {
    Curve(Side),
    Topology(Side, EntityRef),
    Classification(Side),
}
impl ClassObject for Object {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve(_) => ClassObjectKind::Curve,
            Self::Topology(_, EntityRef::Vertex(_)) => ClassObjectKind::Vertex,
            Self::Topology(_, EntityRef::Edge(_)) => ClassObjectKind::Edge,
            _ => ClassObjectKind::Other,
        }
    }
}
struct Pair {
    bodies: [Brep; 2],
    curves: [Curve; 2],
}
impl ClassGeometry<Object> for Pair {
    fn curve(&self, o: Object) -> Option<&Curve> {
        if let Object::Curve(s) = o {
            Some(s.get(&self.curves))
        } else {
            None
        }
    }
    fn surface(&self, _: Object) -> Option<&Surface> {
        None
    }
    fn volume(&self, _: Object) -> Option<&Volume> {
        None
    }
    fn point(&self, o: Object) -> Option<Vec3> {
        let Object::Topology(s, EntityRef::Vertex(v)) = o else {
            return None;
        };
        Some(s.get(&self.bodies).vertex(v).ok()?.point)
    }
    fn classification_vertex_point(&self, o: Object) -> Option<Vec3> {
        self.point(o)
    }
    fn topology_entity(&self, o: Object) -> Option<(&Brep, EntityRef)> {
        let Object::Topology(s, e) = o else {
            return None;
        };
        Some((s.get(&self.bodies), e))
    }
    fn topology_zone_tol_3d(&self, o: Object) -> Option<f64> {
        let (b, e) = self.topology_entity(o)?;
        match e {
            EntityRef::Vertex(v) => Some(b.vertex(v).ok()?.tolerance),
            EntityRef::Edge(e) => Some(b.edge(e).ok()?.tolerance),
            _ => None,
        }
    }
}
impl ClassGeometryMut<Object> for Pair {
    fn curve_mut(&mut self, o: Object) -> Option<&mut Curve> {
        if let Object::Curve(s) = o {
            Some(s.get_mut(&mut self.curves))
        } else {
            None
        }
    }
    fn topology_entity_mut(&mut self, o: Object) -> Option<(&mut Brep, EntityRef)> {
        let Object::Topology(s, e) = o else {
            return None;
        };
        Some((s.get_mut(&mut self.bodies), e))
    }
}
impl MergeIntervalBrep<Object> for Pair {
    fn merge_context(&mut self, _: Object) -> Option<MergeContext<'_, Object>> {
        // This fixed, already-merged interval edits existing entities through p7.
        // Any unexpected topology-creation route remains unsupported.
        None
    }
}
fn box_at(origin: [f64; 3]) -> Result<Brep, HarnessError> {
    // built by the Rust kernel (no C++ export reaches the Rust side)
    super::super::heal_stitch_p1::own_unit_box(origin)
}
fn selected(b: &Brep) -> Result<(EdgeId, [VertexId; 2]), HarnessError> {
    let e = *brep_p1::edges(b, None)
        .map_err(|e| h(e.to_string()))?
        .first()
        .ok_or_else(|| h("no box edge"))?;
    let (a, b) = topo_edge::start_end_vertices(b, e).map_err(|e| h(e.to_string()))?;
    Ok((
        e,
        [
            a.ok_or_else(|| h("missing start vertex"))?,
            b.ok_or_else(|| h("missing end vertex"))?,
        ],
    ))
}
fn edge_curve(b: &Brep, e: EdgeId) -> Result<Curve, HarnessError> {
    b.edge(e)
        .map_err(|e| h(e.to_string()))?
        .curve
        .clone()
        .ok_or_else(|| h("missing box curve"))
}
fn snapshot(
    k: &Kernel,
    p: &Pair,
    edges: [EdgeId; 2],
    vertices: [[VertexId; 2]; 2],
) -> Result<Vec<f64>, HarnessError> {
    let mut out = Vec::new();
    for ((b, edge), vertices) in p.bodies.iter().zip(edges).zip(vertices) {
        for v in vertices {
            let pt = b.vertex(v).map_err(|e| h(e.to_string()))?.point;
            out.extend([pt.x, pt.y, pt.z]);
        }
        let e = b.edge(edge).map_err(|e| h(e.to_string()))?;
        let curve = e.curve.as_ref().ok_or_else(|| h("missing edge curve"))?;
        let d = curve.natural_interval();
        out.extend([
            curve.type_code() as f64,
            e.interval.min(),
            e.interval.max(),
            d.min(),
            d.max(),
        ]);
        for t in [0., 0.5, 1.] {
            let pt = curve
                .evaluate_point(k, e.interval.evaluate(t))
                .map_err(|e| h(e.to_string()))?;
            out.extend([pt.x, pt.y, pt.z]);
        }
    }
    Ok(out)
}
fn classification(
    k: &Kernel,
    pair: &Pair,
    side: Side,
    edge: EdgeId,
    vertices: [VertexId; 2],
) -> Result<CurveClassification<Object>, HarnessError> {
    let mut cc = CurveClassification::new(k);
    cc.set_curve(Some(Object::Curve(side)), false);
    cc.set_src_zone_tol_3d(1e-5);
    cc.set_size(k, 1).map_err(|e| h(e.to_string()))?;
    let d = side
        .get(&pair.bodies)
        .edge(edge)
        .map_err(|e| h(e.to_string()))?
        .interval;
    let pc = |kind, entity, t| {
        let mut p = PointClassification::new(1e-5);
        p.set_class_object(kind, Some(Object::Topology(side, entity)));
        p.set_t_param(t, false);
        p
    };
    let [start, end] = vertices;
    *cc.at_mut(0).map_err(|e| h(e.to_string()))? = CurveInterval {
        interval: d,
        start: pc(PointClassType::Vertex, EntityRef::Vertex(start), d.min()),
        mid: pc(PointClassType::Edge, EntityRef::Edge(edge), d.evaluate(0.5)),
        end: pc(PointClassType::Vertex, EntityRef::Vertex(end), d.max()),
    };
    Ok(cc)
}
pub(super) fn cpp(mode: Mode) -> Result<Output, HarnessError> {
    let result = if mode == Mode::Metadata {
        ffi::paired_fillet()
    } else {
        ffi::paired_fillet_scoped(mode.code())
    };
    result
        .map(|v| Output::new().values("out", v))
        .map_err(|e| h(e.to_string()))
}
pub(super) fn rust(mode: Mode) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let mut bodies = [box_at([0.; 3])?, box_at([1e-7, 0., 0.])?];
    let [target, other] = &bodies;
    let (a, av) = selected(target)?;
    let (b, bv) = selected(other)?;
    let curves = [edge_curve(target, a)?, edge_curve(other, b)?];
    for b in &mut bodies {
        b.set_editing_enabled(true);
    }
    let mut pair = Pair { bodies, curves };
    let mut left = classification(&k, &pair, Side::Target, a, av)?;
    let mut right = classification(&k, &pair, Side::Other, b, bv)?;
    let doing = mode != Mode::LinkedDisabled;
    left.set_fillet_data(
        (mode == Mode::Metadata).then_some(Object::Classification(Side::Other)),
        doing,
    );
    right.set_fillet_data(
        (mode == Mode::Metadata).then_some(Object::Classification(Side::Target)),
        doing,
    );
    let before = snapshot(&k, &pair, [a, b], [av, bv])?;
    let result = if matches!(mode, Mode::Linked | Mode::LinkedDisabled) {
        left.merge_classifications_linked_mut(&mut k, &mut pair, &mut right, false)
    } else {
        left.merge_classifications_mut(&mut k, &mut pair, &mut right, false)
    };
    let mut out = vec![];
    match result {
        Ok((a, b)) => out.extend([
            0.,
            a.faces.len() as f64,
            b.faces.len() as f64,
            a.edges.len() as f64,
            b.edges.len() as f64,
            a.vertices.len() as f64,
            b.vertices.len() as f64,
        ]),
        Err(e) => {
            return Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::new().values("out", vec![status(&e)])))
        }
    }
    out.extend(before);
    out.extend(snapshot(&k, &pair, [a, b], [av, bv])?);
    if mode != Mode::Metadata {
        out.extend([
            f64::from(left.other_classification().is_some()),
            f64::from(left.doing_fillets()),
            f64::from(right.other_classification().is_some()),
            f64::from(right.doing_fillets()),
        ]);
    }
    Ok(Output::new().values("out", out))
}
