//! Assembly constraints (`assembly.constraint.create`, `assembly.constraint.edit`): a geometric rule
//! between a reference on each of two parts (a mate, a flush, concentric holes…), in the words of the
//! Constrain dialog.
//! Each rule is kept as a joint of the kind it needs, made where the parts are and tagged with the
//! rule (`Joint::constraint`), so the one joint solver places, counts and checks it with the
//! others: several constraints between one pair add up, and b keeps every motion they leave free.

use serde_json::{Map, Value, json};
use solidcraft_doc::joints::{Constraint, Joint, JointKind, JointOrigin, Snap};

use super::{CommandSpec, add_joint, joint_id, new_joint, origin_param, value};
use crate::params::{bad, bool_, str_};
use crate::schema::joints::{CreateConstraintParams, EditConstraintParams, EditJointResult, JointAddedResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CREATE_CONTRACT: TypedContract<CreateConstraintParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<EditConstraintParams> = TypedContract::new();

const TYPE_MESSAGE: &str = "`type` must be mate, flush, distance, concentric, insert, parallel, perpendicular, angle, tangent or point_on_axis";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("assembly.constraint.create", "Constrain", crate::checked!("assembly.constraint.create", create))
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
        ),
    CommandSpec::new("assembly.constraint.edit", "Edit Constraint", crate::checked!("assembly.constraint.edit", edit))
        .icon("edit")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditJointResult>)
        .params(
            "constraint: id|name (a joint made with assembly.constraint.create); type?: a rule as assembly.constraint.create takes it \
             (the references it has must fit it; the gap carries over); offset?: the gap (mm or an expression); angle?: an angle \
             rule's angle; flip?: true turns b over (from the new rule's way after a type change, else from how it faces now); \
             name?; suppressed?; quiet?. Changed as joint.edit changes a joint: its values start again where its parts are \
             → conflicts, errors, dof, fixed as joint.edit",
        ),
];

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

/// Whether b's reference faces along a's now (true) or against it.
fn along(s: &Session, a: &JointOrigin, b: &JointOrigin, cmd: &str) -> Result<bool> {
    let st = s.model.state();
    let za = s.doc.origin_world(&st, a).map_err(|e| bad(cmd, e.to_string()))?;
    let zb = s.doc.origin_world(&st, b).map_err(|e| bad(cmd, e.to_string()))?;
    Ok((0..3).map(|i| za[2][i] * zb[2][i]).sum::<f64>() > 0.0)
}

/// The joint's `flip` for a rule: the way the rule fixes (a mate against, a flush along), or for a
/// rule that fixes neither, the way the parts face now, so b is not turned over; `turn` turns it.
fn facing(s: &Session, rule: &Kept, a: &JointOrigin, b: &JointOrigin, turn: bool, cmd: &str) -> Result<bool> {
    let f = match rule.flip {
        Some(f) => f,
        None => along(s, a, b, cmd)?,
    };
    Ok(f != turn)
}

/// A cylinder resting on a plane needs its round face's radius: refused when b's reference has none.
fn check_round(s: &Session, c: Constraint, b: &JointOrigin, cmd: &str) -> Result<()> {
    if c == Constraint::Tangent {
        s.doc.origin_radius(&s.model.state(), b).map_err(|e| bad(cmd, e.to_string()))?;
    }
    Ok(())
}

/// The gap a constraint keeps when its rule changes: its offset's expression where it has one (a
/// tangent's gap always is: its offset adds the radius), else its offset.
fn gap(j: &Joint) -> Value {
    j.exprs.offset.clone().map(Value::String).unwrap_or_else(|| json!(j.offset))
}

/// Change a constraint in place: its rule, gap, angle, facing, name or suppression. The new rule is
/// mapped onto a joint kind as `create` maps it, and the change goes through `joint.edit`, so the
/// values start again where the parts are and the joint is solved as any edited joint is.
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "assembly.constraint.edit";
    // Its key read as the contract reads it, then found as joint.edit finds a joint.
    let id = match p.get("constraint") {
        None => return Err(bad(cmd, "`constraint` is required")),
        Some(v @ (Value::Number(_) | Value::String(_))) => joint_id(s, Some(v), cmd)?,
        Some(_) => return Err(bad(cmd, "`constraint` must be an id or name")),
    };
    let j = s.doc.assembly.joints.iter().find(|j| j.id == id).cloned().ok_or_else(|| bad(cmd, "constraint"))?;
    let Some(was) = j.constraint else {
        return Err(bad(cmd, format!("{} was made as a joint, not as a constraint: change it with joint.edit", j.name)));
    };
    let c = match str_(p, "type") {
        Some(t) => Some(Constraint::parse(t).ok_or_else(|| bad(cmd, TYPE_MESSAGE))?),
        None => None,
    };
    for (key, angle) in [("offset", false), ("angle", true)] {
        if let Some(v) = p.get(key) {
            value(s, v, angle, cmd)?;
        }
    }
    // joint.edit's params: what is given, and what the rule means.
    let mut q = Map::new();
    q.insert("joint".into(), json!(id));
    for key in ["offset", "angle", "name", "suppressed", "quiet"] {
        if let Some(v) = p.get(key) {
            q.insert(key.into(), v.clone());
        }
    }
    let turn = bool_(p, "flip") == Some(true);
    let mut swap = false;
    if let Some(c) = c {
        let rule = kept(c, shape(&j.a), shape(&j.b), cmd)?;
        let (a, b) = if rule.swap { (&j.b, &j.a) } else { (&j.a, &j.b) };
        if c == Constraint::Angle && p.get("angle").is_none() && j.kind != JointKind::Angle {
            return Err(bad(cmd, "angle needs `angle`"));
        }
        check_round(s, c, b, cmd)?;
        let flip = facing(s, &rule, a, b, turn, cmd)?;
        q.insert("type".into(), serde_json::to_value(rule.kind).map_err(|e| bad(cmd, e.to_string()))?);
        q.insert("flip".into(), json!(flip));
        // The gap carries over to the new rule.
        if p.get("offset").is_none() {
            q.insert("offset".into(), gap(&j));
        }
        swap = rule.swap;
    } else if turn {
        q.insert("flip".into(), json!(!j.flip));
    }
    // The rule (and the side order it wants) is written first, so joint.edit restarts and keeps the
    // gap as this rule's joint. A refusal after this leaves the design as it was (the session's undo).
    if let Some(jj) = s.doc_mut().assembly.joints.iter_mut().find(|j| j.id == id) {
        if swap {
            std::mem::swap(&mut jj.a, &mut jj.b);
        }
        jj.constraint = Some(c.unwrap_or(was));
    }
    // As joint.edit changes a joint, keeping the rule just set through a change of kind.
    super::edit_joint(s, &Value::Object(q), false)
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
    let c = str_(p, "type").and_then(Constraint::parse).ok_or_else(|| bad(cmd, TYPE_MESSAGE))?;
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
    let flip = facing(s, &rule, &a, &b, bool_(p, "flip") == Some(true), cmd)?;
    check_round(s, c, &b, cmd)?;
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
