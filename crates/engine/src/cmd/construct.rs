//! SOLID ▸ CONSTRUCT: construction axes, points and the planes built from model geometry.
//!
//! References are given as `{face: [x,y,z]}`, `{edge: [x,y,z]}`, `{vertex: [x,y,z]}`,
//! `{point: [x,y,z]}`, `{plane: XY|name|{origin, normal}}`, `{axis: X|Y|Z|name}`,
//! `{construct_point: name}`, `{sketch, point}` or `{sketch, curve}`, or as a tagged
//! `GeoRef` (`{"type": "edge", "at": …}`). Picked faces and edges are stored with their
//! persistent names.

use serde_json::Value;
use solidcraft_doc::construct::{AxisDef, GeoRef, PointDef};
use solidcraft_doc::{FeatureKind, PlaneRef};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use super::features::{add_feature, check_expr, plane_param, sketch_id};
use crate::params::{bad, req_expr, str_, vec3};
use crate::schema::construct::{
    AxisEdgeParams, AxisNormalToFaceParams, AxisThroughCylinderParams, AxisTwoPlanesParams, AxisTwoPointsParams, MidplaneParams,
    PlaneAlongPathParams, PlanePerpendicularParams, PlaneTangentParams, PlaneThreePointsParams, PlaneTwoEdgesParams, PointAlongPathParams,
    PointCenterParams, PointEdgePlaneParams, PointThreePlanesParams, PointTwoEdgesParams, PointVertexParams,
};
use crate::schema::features::FeatureResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

pub(crate) const REF: &str =
    "{face|edge|vertex|point: [x,y,z]} | {plane: XY|name|{origin, normal}} | {axis: X|Y|Z|name} | {construct_point: name} | {sketch, point|curve}";

const AXIS_THROUGH_CYLINDER_CONTRACT: TypedContract<AxisThroughCylinderParams> = TypedContract::new();
const AXIS_NORMAL_TO_FACE_CONTRACT: TypedContract<AxisNormalToFaceParams> = TypedContract::new();
const AXIS_TWO_PLANES_CONTRACT: TypedContract<AxisTwoPlanesParams> = TypedContract::new();
const AXIS_TWO_POINTS_CONTRACT: TypedContract<AxisTwoPointsParams> = TypedContract::new();
const AXIS_EDGE_CONTRACT: TypedContract<AxisEdgeParams> = TypedContract::new();
const POINT_VERTEX_CONTRACT: TypedContract<PointVertexParams> = TypedContract::new();
const POINT_TWO_EDGES_CONTRACT: TypedContract<PointTwoEdgesParams> = TypedContract::new();
const POINT_THREE_PLANES_CONTRACT: TypedContract<PointThreePlanesParams> = TypedContract::new();
const POINT_CENTER_CONTRACT: TypedContract<PointCenterParams> = TypedContract::new();
const POINT_EDGE_PLANE_CONTRACT: TypedContract<PointEdgePlaneParams> = TypedContract::new();
const POINT_ALONG_PATH_CONTRACT: TypedContract<PointAlongPathParams> = TypedContract::new();
const PLANE_MIDPLANE_CONTRACT: TypedContract<MidplaneParams> = TypedContract::new();
const PLANE_TWO_EDGES_CONTRACT: TypedContract<PlaneTwoEdgesParams> = TypedContract::new();
const PLANE_THREE_POINTS_CONTRACT: TypedContract<PlaneThreePointsParams> = TypedContract::new();
const PLANE_TANGENT_CONTRACT: TypedContract<PlaneTangentParams> = TypedContract::new();
const PLANE_ALONG_PATH_CONTRACT: TypedContract<PlaneAlongPathParams> = TypedContract::new();
const PLANE_PERPENDICULAR_CONTRACT: TypedContract<PlanePerpendicularParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("construct.axis.through_cylinder", "Axis Through Cylinder/Cone/Torus", crate::checked!("construct.axis.through_cylinder", axis_face))
        .at("SOLID", "CONSTRUCT")
        .icon("axis_through_cylinder")
        .contract(&AXIS_THROUGH_CYLINDER_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("face: ref (a cylinder, cone or torus face, or a circular edge); name?"),
    CommandSpec::new("construct.axis.normal_to_face", "Axis Perpendicular To Face", crate::checked!("construct.axis.normal_to_face", axis_normal))
        .at("SOLID", "CONSTRUCT")
        .icon("axis_normal_to_face")
        .contract(&AXIS_NORMAL_TO_FACE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("face: ref; point: ref (the axis goes through it, square to the face); name?"),
    CommandSpec::new("construct.axis.two_planes", "Axis Through Two Planes", crate::checked!("construct.axis.two_planes", axis_planes))
        .at("SOLID", "CONSTRUCT")
        .icon("axis_two_planes")
        .contract(&AXIS_TWO_PLANES_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b: plane or planar face refs; name?"),
    CommandSpec::new("construct.axis.two_points", "Axis Through Two Points", crate::checked!("construct.axis.two_points", axis_points))
        .at("SOLID", "CONSTRUCT")
        .icon("axis_two_points")
        .contract(&AXIS_TWO_POINTS_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b: point refs (vertices, sketch points, points); name?"),
    CommandSpec::new("construct.axis.edge", "Axis Through Edge", crate::checked!("construct.axis.edge", axis_edge))
        .at("SOLID", "CONSTRUCT")
        .icon("axis_edge")
        .contract(&AXIS_EDGE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("edge: ref (a straight edge or sketch line); name?"),
    CommandSpec::new("construct.point.vertex", "Point At Vertex", crate::checked!("construct.point.vertex", point_at))
        .at("SOLID", "CONSTRUCT")
        .icon("point_vertex")
        .contract(&POINT_VERTEX_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("point: ref (vertex, sketch point, point); name?"),
    CommandSpec::new("construct.point.two_edges", "Point Through Two Edges", crate::checked!("construct.point.two_edges", point_edges))
        .at("SOLID", "CONSTRUCT")
        .icon("point_two_edges")
        .contract(&POINT_TWO_EDGES_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b: edge or line refs (where they meet, or come closest); name?"),
    CommandSpec::new("construct.point.three_planes", "Point Through Three Planes", crate::checked!("construct.point.three_planes", point_planes))
        .at("SOLID", "CONSTRUCT")
        .icon("point_three_planes")
        .contract(&POINT_THREE_PLANES_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b, c: plane or planar face refs; name?"),
    CommandSpec::new("construct.point.center", "Point At Center Of Circle/Sphere/Torus", crate::checked!("construct.point.center", point_center))
        .at("SOLID", "CONSTRUCT")
        .icon("point_center")
        .contract(&POINT_CENTER_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("of: ref (circular edge, sketch circle or arc, sphere or torus face); name?"),
    CommandSpec::new("construct.point.edge_plane", "Point At Edge And Plane", crate::checked!("construct.point.edge_plane", point_edge_plane))
        .at("SOLID", "CONSTRUCT")
        .icon("point_edge_plane")
        .contract(&POINT_EDGE_PLANE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("edge: edge or line ref; plane: plane or planar face ref; name?"),
    CommandSpec::new("construct.point.along_path", "Point Along Path", crate::checked!("construct.point.along_path", point_along))
        .at("SOLID", "CONSTRUCT")
        .icon("point_along_path")
        .contract(&POINT_ALONG_PATH_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("path: edge or sketch curve ref; t: expr, 0…1 along it (default 0.5); name?"),
    CommandSpec::new("construct.plane.midplane", "Midplane", crate::checked!("construct.plane.midplane", plane_mid))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_midplane")
        .contract(&PLANE_MIDPLANE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b: plane or planar face refs (halfway between; bisecting when they meet at an angle); name?"),
    CommandSpec::new("construct.plane.two_edges", "Plane Through Two Edges", crate::checked!("construct.plane.two_edges", plane_edges))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_two_edges")
        .contract(&PLANE_TWO_EDGES_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b: straight edge or line refs in one plane; name?"),
    CommandSpec::new("construct.plane.three_points", "Plane Through Three Points", crate::checked!("construct.plane.three_points", plane_points))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_three_points")
        .contract(&PLANE_THREE_POINTS_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("a, b, c: point refs; name?"),
    CommandSpec::new("construct.plane.tangent", "Tangent Plane", crate::checked!("construct.plane.tangent", plane_tangent))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_tangent")
        .contract(&PLANE_TANGENT_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("face: ref (cylinder, cone, sphere or torus); at?: [x,y,z] where it touches (default: the face pick); name?"),
    CommandSpec::new("construct.plane.along_path", "Plane Along Path", crate::checked!("construct.plane.along_path", plane_along))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_along_path")
        .contract(&PLANE_ALONG_PATH_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("path: edge or sketch curve ref; t: expr, 0…1 along it (default 0.5); name? The plane is square to the path there"),
    CommandSpec::new("construct.plane.perpendicular", "Perpendicular Plane", crate::checked!("construct.plane.perpendicular", plane_perpendicular))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_perpendicular")
        .contract(&PLANE_PERPENDICULAR_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
            "plane: plane or planar face ref; line: straight edge, sketch line or axis ref; name? The plane holds the line and is square to the plane",
        ),
];

/// Whether [`model_axis_param`] reads `v` as an edge or face of the model: `{edge: …}`,
/// `{face: …}`, or as saved (`{type: edge|face, at, name?}`), as an edit dialog gives back one it
/// kept.
pub(crate) fn is_model_axis(v: &Value) -> bool {
    v.get("edge").is_some() || v.get("face").is_some() || matches!(v.get("type").and_then(Value::as_str), Some("edge" | "face"))
}

/// An axis on an edge or round face of the model (`{edge: [x,y,z]}` or `{face: [x,y,z]}`, the
/// point on it), with the line it gives now (`solidcraft_doc::construct::model_axis`); `None`
/// for any other form of `axis`.
pub(super) fn model_axis_param(s: &Session, v: Option<&Value>, cmd: &str) -> Result<Option<(GeoRef, Vec3, Vec3)>> {
    let Some(v) = v.filter(|v| is_model_axis(v)) else { return Ok(None) };
    let g = geo(s, Some(v), cmd, "axis")?;
    let (o, d) = solidcraft_doc::construct::model_axis(&*s.world_state()?, &g).map_err(|e| bad(cmd, format!("`axis`: {e}")))?;
    Ok(Some((g, o, d)))
}

/// A reference as [`geo`] reads it before looking anything up in the design: which form it has.
pub(crate) enum GeoForm<'a> {
    /// As saved (`{type, …}`), read whole.
    Saved(GeoRef),
    Face(Vec3),
    Edge(Vec3),
    Vertex(Vec3),
    /// `{sketch, point|curve}`: the sketch as given, and the point or curve, when one is given.
    Sketch {
        sketch: &'a Value,
        part: Option<SketchPart<'a>>,
    },
    Point(Vec3),
    /// `{plane: …}`'s value, or a bare string: a plane, as `plane_param` reads it.
    Plane(&'a Value),
    /// `{axis: X|Y|Z|name}`.
    Axis(&'a str),
    /// `{construct_point: name}`.
    ConstructPoint(&'a str),
}

/// A sketch reference's point or curve id.
pub(crate) enum SketchPart<'a> {
    Point(&'a str),
    Curve(&'a str),
}

/// Why [`geo_form`] refuses a reference.
pub(crate) enum GeoFormError {
    /// A saved form serde refuses, with its message.
    Saved(String),
    /// None of the forms.
    Unknown,
}

/// What [`geo`] says when a sketch reference has neither `point` nor `curve`.
pub(crate) const SKETCH_PART: &str = "a sketch reference needs `point` or `curve`";

/// The form [`geo`] reads `v` as. The first that fits wins, in this order: `type` (read whole), a
/// point at `face`, `edge` or `vertex`, `sketch` (whatever else it has), a point at `point`,
/// `plane` (whatever it holds), `axis` and `construct_point` as text, and a bare string.
pub(crate) fn geo_form(v: &Value) -> std::result::Result<GeoForm<'_>, GeoFormError> {
    let p3 = |k: &str| v.get(k).and_then(vec3);
    Ok(if v.get("type").is_some() {
        GeoForm::Saved(serde_json::from_value::<GeoRef>(v.clone()).map_err(|e| GeoFormError::Saved(e.to_string()))?)
    } else if let Some(at) = p3("face") {
        GeoForm::Face(at)
    } else if let Some(at) = p3("edge") {
        GeoForm::Edge(at)
    } else if let Some(at) = p3("vertex") {
        GeoForm::Vertex(at)
    } else if let Some(sketch) = v.get("sketch") {
        let part = match (str_(v, "point"), str_(v, "curve")) {
            (Some(point), _) => Some(SketchPart::Point(point)),
            (None, Some(curve)) => Some(SketchPart::Curve(curve)),
            _ => None,
        };
        GeoForm::Sketch { sketch, part }
    } else if let Some(p) = p3("point") {
        GeoForm::Point(p)
    } else if let Some(plane) = v.get("plane") {
        GeoForm::Plane(plane)
    } else if let Some(a) = str_(v, "axis") {
        GeoForm::Axis(a)
    } else if let Some(n) = str_(v, "construct_point") {
        GeoForm::ConstructPoint(n)
    } else if v.is_string() {
        GeoForm::Plane(v)
    } else {
        return Err(GeoFormError::Unknown);
    })
}

/// A reference from its JSON form.
pub(super) fn geo(s: &Session, v: Option<&Value>, cmd: &str, what: &str) -> Result<GeoRef> {
    let v = v.ok_or_else(|| bad(cmd, format!("`{what}` is required: {REF}")))?;
    let mut g = match geo_form(v) {
        Ok(GeoForm::Saved(g)) => g,
        Ok(GeoForm::Face(at)) => GeoRef::Face { at, name: None },
        Ok(GeoForm::Edge(at)) => GeoRef::Edge { at, name: None },
        Ok(GeoForm::Vertex(at)) => GeoRef::Vertex { at, edges: None },
        Ok(GeoForm::Sketch { sketch, part }) => {
            let sketch = sketch_id(s, Some(sketch), cmd, what)?;
            match part {
                Some(SketchPart::Point(point)) => GeoRef::SketchPoint { sketch, point: point.to_string() },
                Some(SketchPart::Curve(curve)) => GeoRef::SketchCurve { sketch, curve: curve.to_string() },
                None => return Err(bad(cmd, format!("`{what}`: {SKETCH_PART}"))),
            }
        }
        Ok(GeoForm::Point(p)) => GeoRef::Point { p },
        Ok(GeoForm::Plane(plane)) => GeoRef::Plane { plane: plane_param(s, Some(plane), cmd)? },
        Ok(GeoForm::Axis(a)) => {
            if ["x", "y", "z"].contains(&a.to_ascii_lowercase().as_str()) {
                GeoRef::World { axis: a.to_ascii_uppercase() }
            } else {
                match s.doc.find_feature(a).map(|f| &f.kind) {
                    Some(FeatureKind::ConstructionAxis { .. }) => GeoRef::Axis { name: a.into() },
                    _ => return Err(bad(cmd, format!("no axis `{a}`"))),
                }
            }
        }
        Ok(GeoForm::ConstructPoint(n)) => match s.doc.find_feature(n).map(|f| &f.kind) {
            Some(FeatureKind::ConstructionPoint { .. }) => GeoRef::ConstructPoint { name: n.into() },
            _ => return Err(bad(cmd, format!("no construction point `{n}`"))),
        },
        Err(GeoFormError::Saved(e)) => return Err(bad(cmd, format!("`{what}`: {e}"))),
        Err(GeoFormError::Unknown) => return Err(bad(cmd, format!("`{what}` must be {REF}"))),
    };
    g.name_picks(&s.model.state());
    Ok(g)
}

fn t_param(s: &Session, p: &Value, cmd: &str) -> Result<solidcraft_doc::Expr> {
    let t = if p.get("t").is_some() { req_expr(cmd, p, "t")? } else { "0.5".into() };
    check_expr(s, &t, solidcraft_doc::expr::Kind::Unitless, cmd, "t")?;
    Ok(t)
}

fn axis(s: &mut Session, p: &Value, def: AxisDef) -> Result<Value> {
    add_feature(s, p, FeatureKind::ConstructionAxis { def })
}

fn point(s: &mut Session, p: &Value, def: PointDef) -> Result<Value> {
    add_feature(s, p, FeatureKind::ConstructionPoint { def })
}

fn plane(s: &mut Session, p: &Value, plane: PlaneRef) -> Result<Value> {
    add_feature(s, p, FeatureKind::ConstructionPlane { plane })
}

fn axis_face(s: &mut Session, p: &Value) -> Result<Value> {
    let face = geo(s, p.get("face"), "construct.axis.through_cylinder", "face")?;
    axis(s, p, AxisDef::ThroughFace { face })
}

fn axis_normal(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.axis.normal_to_face";
    let face = geo(s, p.get("face"), cmd, "face")?;
    let point = geo(s, p.get("point"), cmd, "point")?;
    axis(s, p, AxisDef::NormalToFace { face, point })
}

fn axis_planes(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.axis.two_planes";
    let (a, b) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?);
    axis(s, p, AxisDef::TwoPlanes { a, b })
}

fn axis_points(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.axis.two_points";
    let (a, b) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?);
    axis(s, p, AxisDef::TwoPoints { a, b })
}

fn axis_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let edge = geo(s, p.get("edge"), "construct.axis.edge", "edge")?;
    axis(s, p, AxisDef::Edge { edge })
}

fn point_at(s: &mut Session, p: &Value) -> Result<Value> {
    let at = geo(s, p.get("point"), "construct.point.vertex", "point")?;
    point(s, p, PointDef::At { point: at })
}

fn point_edges(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.point.two_edges";
    let (a, b) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?);
    point(s, p, PointDef::TwoEdges { a, b })
}

fn point_planes(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.point.three_planes";
    let (a, b, c) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?, geo(s, p.get("c"), cmd, "c")?);
    point(s, p, PointDef::ThreePlanes { a, b, c })
}

fn point_center(s: &mut Session, p: &Value) -> Result<Value> {
    let of = geo(s, p.get("of"), "construct.point.center", "of")?;
    point(s, p, PointDef::Center { of })
}

fn point_edge_plane(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.point.edge_plane";
    let (edge, pl) = (geo(s, p.get("edge"), cmd, "edge")?, geo(s, p.get("plane"), cmd, "plane")?);
    point(s, p, PointDef::EdgeAndPlane { edge, plane: pl })
}

fn point_along(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.point.along_path";
    let path = geo(s, p.get("path"), cmd, "path")?;
    let t = t_param(s, p, cmd)?;
    point(s, p, PointDef::AlongPath { path, t })
}

fn plane_mid(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.midplane";
    let (a, b) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?);
    plane(s, p, PlaneRef::Midplane { a: Box::new(a), b: Box::new(b) })
}

fn plane_edges(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.two_edges";
    let (a, b) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?);
    plane(s, p, PlaneRef::TwoEdges { a: Box::new(a), b: Box::new(b) })
}

fn plane_points(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.three_points";
    let (a, b, c) = (geo(s, p.get("a"), cmd, "a")?, geo(s, p.get("b"), cmd, "b")?, geo(s, p.get("c"), cmd, "c")?);
    plane(s, p, PlaneRef::ThreePoints { a: Box::new(a), b: Box::new(b), c: Box::new(c) })
}

fn plane_tangent(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.tangent";
    let face = geo(s, p.get("face"), cmd, "face")?;
    let at = match (p.get("at").and_then(vec3), &face) {
        (Some(a), _) => a,
        (None, GeoRef::Face { at, .. }) => *at,
        _ => return Err(bad(cmd, "`at` is required: where the plane touches the face")),
    };
    plane(s, p, PlaneRef::Tangent { face: Box::new(face), at })
}

fn plane_along(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.along_path";
    let path = geo(s, p.get("path"), cmd, "path")?;
    let t = t_param(s, p, cmd)?;
    plane(s, p, PlaneRef::AlongPath { path: Box::new(path), t })
}

fn plane_perpendicular(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "construct.plane.perpendicular";
    let (base, line) = (geo(s, p.get("plane"), cmd, "plane")?, geo(s, p.get("line"), cmd, "line")?);
    plane(s, p, PlaneRef::Perpendicular { plane: Box::new(base), line: Box::new(line) })
}
