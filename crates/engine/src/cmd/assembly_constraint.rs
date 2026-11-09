//! Assembly constraints (`assembly.constraint.create`): a geometric rule between a reference on
//! each of two parts (a mate, a flush, concentric holes…), in the words of the Constrain dialog.
//! Each rule is kept as a joint of the kind it needs, made where the parts are and tagged with the
//! rule (`Joint::constraint`), so the one joint solver places, counts and checks it with the
//! others: several constraints between one pair add up, and b keeps every motion they leave free.

use serde_json::Value;
use solidcraft_doc::joints::{Constraint, JointKind, JointOrigin, Snap};

use super::{CommandSpec, add_joint, new_joint, origin_param, value};
use crate::params::{bad, bool_, str_};
use crate::schema::joints::{CreateConstraintParams, JointAddedResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CREATE_CONTRACT: TypedContract<CreateConstraintParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new("assembly.constraint.create", "Constrain", crate::checked!("assembly.constraint.create", create))
        .icon("joint")
        .contract(&CREATE_CONTRACT)
        .output(output_schema_for::<JointAddedResult>)
        .params(
            "type: mate|flush|distance|concentric|insert|parallel|perpendicular|angle|tangent|point_on_axis; a, b: references, as joint.create's \
         origins (a face or a datum plane for a plane, a circle or an axis for an axis, a point; b moves); offset?: the gap (mm or an expression; needed for distance); \
         angle?: deg or an expression (needed for angle); flip?: turn b over; name? (default: the rule and a number, Mate1); quiet?. \
         Kept as a joint of the kind the rule needs (mate and flush planar, distance planar or point on plane, concentric cylindrical, \
         insert revolute, tangent line on plane, point on axis point on line), made where the parts are, so b moves no further than the \
         rule needs → joint, name, dof, conflicts, fixed as joint.create",
        )];

/// What a reference is, for the rules that need one kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// A planar face or a frame: its plane and normal.
    Plane,
    /// A round face's or edge's centre and axis.
    Round,
    /// A point (a vertex, an edge, a sketch point): its position.
    Point,
    /// A datum or construction axis: a line, with no rim or radius.
    Axis,
}

fn shape(o: &JointOrigin) -> Shape {
    match o.snap {
        Snap::FaceCenter { .. } | Snap::Frame { .. } | Snap::Plane { .. } => Shape::Plane,
        Snap::Axis { .. } => Shape::Axis,
        Snap::CircleCenter { .. } => Shape::Round,
        Snap::Point { .. } | Snap::Edge { .. } => Shape::Point,
    }
}

/// How a rule is kept: the joint's kind, whether b is turned to face along a (`flip`), and whether
/// the two references were given the other way round (the plane or axis goes on side a).
struct Kept {
    kind: JointKind,
    /// `Some` where the rule fixes the facing; `None` keeps the parts' facing now.
    flip: Option<bool>,
    swap: bool,
}

fn kept(c: Constraint, a: Shape, b: Shape, cmd: &str) -> Result<Kept> {
    use Shape::*;
    let fixed = |kind, flip| Ok(Kept { kind, flip: Some(flip), swap: false });
    let facing = |kind| Ok(Kept { kind, flip: None, swap: false });
    // A line: a round face's or edge's axis, or an axis itself.
    let axial = |s: Shape| matches!(s, Round | Axis);
    // A rule between one plane or axis and something else: the plane or axis on side a.
    let onto = |kind, want: &dyn Fn(Shape) -> bool, other_ok: &dyn Fn(Shape) -> bool| {
        if want(a) && other_ok(b) {
            Ok(Kept { kind, flip: Some(false), swap: false })
        } else if want(b) && other_ok(a) {
            Ok(Kept { kind, flip: Some(false), swap: true })
        } else {
            Err(bad(cmd, format!("{} needs {}", c.name(), needs(c))))
        }
    };
    match (c, a, b) {
        (Constraint::Mate, Plane, Plane) => fixed(JointKind::Planar, false),
        (Constraint::Flush, Plane, Plane) => fixed(JointKind::Planar, true),
        (Constraint::Distance, Plane, Plane) => facing(JointKind::Planar),
        (Constraint::Distance, ..) => onto(JointKind::PointOnPlane, &|s| s == Plane, &|s| matches!(s, Round | Point)),
        (Constraint::Concentric, ..) if axial(a) && axial(b) => facing(JointKind::Cylindrical),
        (Constraint::Insert, Round, Round) => facing(JointKind::Revolute),
        (Constraint::Parallel, ..) => facing(JointKind::Parallel),
        (Constraint::Perpendicular, ..) => fixed(JointKind::Perpendicular, false),
        (Constraint::Angle, ..) => fixed(JointKind::Angle, false),
        (Constraint::Tangent, ..) => onto(JointKind::LineOnPlane, &|s| s == Plane, &|s| s == Round),
        (Constraint::PointOnAxis, ..) => onto(JointKind::PointOnLine, &axial, &|s| s == Point),
        _ => Err(bad(cmd, format!("{} needs {}", c.name(), needs(c)))),
    }
}

/// The references a rule takes, for its refusal.
fn needs(c: Constraint) -> &'static str {
    match c {
        Constraint::Mate | Constraint::Flush => "two planar faces",
        Constraint::Distance => "two planar faces, or a planar face and a point",
        Constraint::Concentric => "two round faces, round edges or axes",
        Constraint::Insert => "two round edges",
        Constraint::Parallel | Constraint::Perpendicular | Constraint::Angle => "two faces or axes",
        Constraint::Tangent => "a planar face and a round face",
        Constraint::PointOnAxis => "a round face or edge (its axis) or an axis, and a point",
    }
}

/// The rule's default name: `Mate1`, `Point on axis2`…, the first number no joint's name has.
fn default_name(s: &Session, c: Constraint) -> String {
    let word = c.name().replace('_', " ");
    let mut label = word.clone();
    if let Some(first) = label.get(..1) {
        label = first.to_ascii_uppercase() + label.get(1..).unwrap_or("");
    }
    let names: std::collections::BTreeSet<&str> = s.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
    let n = (1..=s.doc.assembly.joints.len().saturating_add(1)).find(|n| !names.contains(format!("{label}{n}").as_str())).unwrap_or(1);
    format!("{label}{n}")
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "assembly.constraint.create";
    let c = str_(p, "type").and_then(Constraint::parse).ok_or_else(|| {
        bad(cmd, "`type` must be mate, flush, distance, concentric, insert, parallel, perpendicular, angle, tangent or point_on_axis")
    })?;
    let a = origin_param(s, p.get("a"), cmd)?;
    let b = origin_param(s, p.get("b"), cmd)?;
    // The numbers are read before the references are checked, in the order the contract reads them.
    for (key, angle) in [("offset", false), ("angle", true)] {
        if let Some(v) = p.get(key) {
            value(s, v, angle, cmd)?;
        }
    }
    let rule = kept(c, shape(&a), shape(&b), cmd)?;
    let (a, b) = if rule.swap { (b, a) } else { (a, b) };
    if c == Constraint::Distance && p.get("offset").is_none() {
        return Err(bad(cmd, "distance needs `offset`"));
    }
    if c == Constraint::Angle && p.get("angle").is_none() {
        return Err(bad(cmd, "angle needs `angle`"));
    }
    let flip = {
        let st = s.model.state();
        // The way b faces a now: along (true) or against. A rule that fixes neither keeps it, so b is
        // not turned over.
        let along = || -> Result<bool> {
            let za = s.doc.origin_world(&st, &a).map_err(|e| bad(cmd, e.to_string()))?;
            let zb = s.doc.origin_world(&st, &b).map_err(|e| bad(cmd, e.to_string()))?;
            Ok((0..3).map(|i| za[2][i] * zb[2][i]).sum::<f64>() > 0.0)
        };
        let mut flip = match rule.flip {
            Some(f) => f,
            None => along()?,
        };
        if bool_(p, "flip") == Some(true) {
            flip = !flip;
        }
        // A cylinder resting on a plane needs its round face's radius (refused when it has none).
        if c == Constraint::Tangent {
            s.doc.origin_radius(&st, &b).map_err(|e| bad(cmd, e.to_string()))?;
        }
        flip
    };
    // The joint's own params: the kind and facing the rule needs, the rest as given.
    let mut q = p.clone();
    if let Some(m) = q.as_object_mut() {
        m.remove("type");
        m.remove("flip");
        m.remove("values");
        m.remove("value");
        m.remove("limits");
        m.insert("flip".into(), Value::Bool(flip));
        // No name, `null` or blank text: the rule's own name (`Mate1`), not a joint's.
        if !m.get("name").and_then(Value::as_str).is_some_and(|n| !n.trim().is_empty()) {
            m.insert("name".into(), Value::String(default_name(s, c)));
        }
    }
    let mut j = new_joint(s, &q, rule.kind, a, b, cmd)?;
    if c == Constraint::Tangent {
        // The gap is kept as the offset's expression; the radius is added to it whenever the joints
        // are solved (`reexpress`, which `add_joint`'s solve runs), so the part follows its face.
        j.exprs.offset = Some(j.exprs.offset.take().unwrap_or_else(|| format!("{} mm", j.offset)));
    }
    if j.offset != 0.0 && !j.offset.is_finite() {
        return Err(bad(cmd, "`offset` must be finite"));
    }
    j.constraint = Some(c);
    add_joint(s, j, &q)
}
