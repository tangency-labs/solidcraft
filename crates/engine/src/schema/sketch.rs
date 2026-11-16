//! Contracts and result types for sketch editing and completion in `cmd/sketch.rs`.
//!
//! The geometric constraints (`sketch.constraint.coincident`, `collinear`, `concentric`, `equal`,
//! `fix`, `horizontal_vertical`, `midpoint`, `parallel`, `perpendicular`, `symmetry` and
//! `tangent`, #12549) read their parameters from the raw JSON, in the forms and cases their
//! handlers do, so a key is read here only where its handler reads it (`line` of a Horizontal /
//! Vertical call that gives `points`, `a` and `b` of an Equal call that gives `curves`, `line` of
//! a Symmetry call that gives `point`, and so on): a call that works today gets no warning. Each
//! one is therefore read by hand, in the handler's order, and fails with the handler's message.
//! A key the handler refuses when it is left out is optional in the type and required in the
//! schema. Whether a curve or point exists, whether a model edge can be projected and whether
//! the constraint can be solved stay the handler's to check.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::check::whole;
use super::motion::require;
use super::types::{IdOrName, Point3, present};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub sketch: IdOrName,
    /// A position (or null, the end of the timeline): temporarily roll to this sketch and
    /// restore that position when the edit ends. Left out, a sketch with features after it is
    /// rolled to and the current position restored, so those features rebuild once, when the
    /// edit ends (#14010). `false` leaves the timeline in place, rebuilding them after every
    /// sketch command.
    #[serde(default, deserialize_with = "present")]
    pub restore_marker: Option<EditMarker>,
}

/// `sketch.edit`'s `restore_marker`: where the timeline goes back to when the edit ends, or
/// `false` to edit with the timeline left in place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditMarker {
    /// Roll to the sketch; restore this position (`None`: the end) when the edit ends.
    Restore(Option<usize>),
    /// Leave the timeline in place.
    InPlace,
}

impl EditMarker {
    pub(crate) fn read(v: &Value) -> Option<Self> {
        match v {
            Value::Null => Some(Self::Restore(None)),
            Value::Bool(false) => Some(Self::InPlace),
            Value::Number(n) => n.as_u64().and_then(|m| usize::try_from(m).ok()).map(|m| Self::Restore(Some(m))),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for EditMarker {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Self::read(&v).ok_or_else(|| D::Error::custom("`restore_marker` must be a nonnegative timeline position, null or false"))
    }
}

impl JsonSchema for EditMarker {
    fn schema_name() -> Cow<'static, str> {
        "EditMarker".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A timeline position to restore when the edit ends (null: the end), or false to leave the timeline in place.",
            "anyOf": [{"type": "integer", "minimum": 0}, {"type": "null"}, {"const": false}]
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExitParams {
    /// Restore the temporary editor marker (default true). False transfers it directly to
    /// another editor without rebuilding the intermediate timeline.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub restore_marker: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub sketch: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FinishResult {
    pub sketch: Option<u64>,
    pub profiles: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CancelResult {
    pub sketch: u64,
    pub removed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// The geometric constraints

type Fields = Map<String, Value>;
type Read<T> = Result<T, String>;

/// The handler's message, whole (the check puts no key in front of it).
fn refuse<T>(message: impl std::fmt::Display) -> Read<T> {
    Err(whole(message))
}

/// The sketch a command works in, as `sketch_store::target` reads `sketch`: a feature id (a whole
/// number) or a name. `null` is read as left out.
#[derive(Clone, Debug, PartialEq)]
pub enum SketchKey {
    Id(u64),
    Name(String),
}

impl JsonSchema for SketchKey {
    fn schema_name() -> Cow<'static, str> {
        "SketchKey".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A sketch's feature id (a whole number, 0 or more) or its name.",
            "anyOf": [{"type": "integer", "minimum": 0}, {"type": "string"}]
        })
    }
}

fn sketch_of(m: &Fields) -> Read<Option<SketchKey>> {
    match m.get("sketch") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => Ok(Some(SketchKey::Name(name.clone()))),
        Some(Value::Number(n)) => match n.as_u64() {
            Some(id) => Ok(Some(SketchKey::Id(id))),
            None => refuse("`sketch` must be an id or a name"),
        },
        Some(_) => refuse("`sketch` must be an id or a name"),
    }
}

/// An optional text, as `str_` reads it (`null` is left out); anything else is refused, where
/// the handler would read it as left out.
fn text_of(m: &Fields, key: &str) -> Read<Option<String>> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => refuse(format!("`{key}` must be text")),
    }
}

/// An optional flag, as `bool_` reads it (`null` is left out); anything else is refused, where
/// the handler would read it as left out.
fn flag_of(m: &Fields, key: &str) -> Read<Option<bool>> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => refuse(format!("`{key}` must be true or false")),
    }
}

/// `if_redundant`: `refuse` (in any case) turns a redundant constraint away instead of keeping
/// it; any other text keeps it.
fn if_redundant_of(m: &Fields) -> Read<Option<String>> {
    text_of(m, "if_redundant")
}

/// A model edge `{edge: [x, y, z], body?}` or vertex `{vertex: [x, y, z], body?}`, which
/// `project_operands` projects into the sketch before the handler reads its operands.
fn model_pick(v: &Value) -> bool {
    v.get("edge").is_some() || v.get("vertex").is_some()
}

fn model_pick_schema(generator: &mut SchemaGenerator) -> Value {
    let point = generator.subschema_for::<Point3>();
    json!({
        "type": "object",
        "properties": {"edge": point, "vertex": point, "body": {"type": "string"}},
        "anyOf": [{"required": ["edge"]}, {"required": ["vertex"]}]
    })
}

/// A sketch entity a constraint is applied to: its id, or a model edge or vertex, which is
/// projected into the sketch first (the projected curve or point is then the operand, and is
/// reported in `created`). Whether the id names something and whether the model geometry can
/// be projected is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Id(String),
    Model(Value),
}

impl JsonSchema for Operand {
    fn schema_name() -> Cow<'static, str> {
        "SketchOperand".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let model = model_pick_schema(generator);
        json_schema!({
            "description": "A sketch point or curve by its id, or a model edge {\"edge\": [x, y, z], \"body\"?} or vertex {\"vertex\": [x, y, z], \"body\"?}, projected into the sketch first.",
            "anyOf": [{"type": "string"}, model]
        })
    }
}

/// `key` as an operand; anything else is refused with `message`, the handler's. Left out is
/// the handler's to refuse.
fn operand_of(m: &Fields, key: &str, message: &str) -> Read<Option<Operand>> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::String(id)) => Ok(Some(Operand::Id(id.clone()))),
        Some(v) if model_pick(v) => Ok(Some(Operand::Model(v.clone()))),
        Some(_) => refuse(message),
    }
}

/// A line's middle, as a point of a constraint: `{midpoint: line_id}`.
fn midpoint_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"midpoint": {"type": "string"}},
        "required": ["midpoint"]
    })
}

/// A point of `sketch.constraint.coincident`: an id, a model edge or vertex, or a line's middle
/// as `{midpoint: line_id}`, which becomes a point tied there.
#[derive(Clone, Debug, PartialEq)]
pub enum PointOperand {
    Id(String),
    Model(Value),
    Midpoint(String),
}

impl JsonSchema for PointOperand {
    fn schema_name() -> Cow<'static, str> {
        "CoincidentOperand".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let model = model_pick_schema(generator);
        json_schema!({
            "description": "A sketch point, curve or axis by its id, a model edge or vertex (projected into the sketch first), or a line's middle {\"midpoint\": line_id}.",
            "anyOf": [{"type": "string"}, model, midpoint_schema()]
        })
    }
}

/// Why a Coincident operand was refused: its `midpoint` is not a curve id, or it has no form.
enum Wrong {
    Midpoint,
    Form,
}

fn point_operand(v: Option<&Value>) -> Result<Option<PointOperand>, Wrong> {
    match v {
        None => Ok(None),
        Some(Value::String(id)) => Ok(Some(PointOperand::Id(id.clone()))),
        Some(v) if model_pick(v) => Ok(Some(PointOperand::Model(v.clone()))),
        Some(v) => match v.get("midpoint") {
            Some(Value::String(line)) => Ok(Some(PointOperand::Midpoint(line.clone()))),
            Some(_) => Err(Wrong::Midpoint),
            None => Err(Wrong::Form),
        },
    }
}

/// One of the two points of `sketch.constraint.horizontal_vertical`'s `points`: a point id or a
/// line's middle as `{midpoint: line_id}`.
#[derive(Clone, Debug, PartialEq)]
pub enum AlignPoint {
    Id(String),
    Midpoint(String),
}

impl JsonSchema for AlignPoint {
    fn schema_name() -> Cow<'static, str> {
        "AlignPoint".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A sketch point by its id, or a line's middle {\"midpoint\": line_id} (it reuses the point a Midpoint constraint ties there, or makes one).",
            "anyOf": [{"type": "string"}, midpoint_schema()]
        })
    }
}

fn align_point(v: &Value) -> Read<AlignPoint> {
    match v.get("midpoint") {
        Some(Value::String(line)) => Ok(AlignPoint::Midpoint(line.clone())),
        Some(_) => refuse("`midpoint` must be a curve id"),
        None => match v {
            Value::String(id) => Ok(AlignPoint::Id(id.clone())),
            _ => refuse("`points` must be a point reference"),
        },
    }
}

/// Names, as `params::string_list` reads them: one name or a list of names, `null` for none,
/// with its messages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names(pub Vec<String>);

impl JsonSchema for Names {
    fn schema_name() -> Cow<'static, str> {
        "SketchNames".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An id, or a list of ids.",
            "anyOf": [
                {"type": "string"},
                {"type": "array", "items": {"type": "string"}, "maxItems": crate::params::MAX_STRING_LIST}
            ]
        })
    }
}

/// `key` as the handler's `string_list` reads it, with its message for anything else.
fn names_of(m: &Fields, key: &str) -> Read<Option<Names>> {
    if m.get(key).is_none_or(Value::is_null) {
        return Ok(None);
    }
    let one: Fields = m.iter().filter(|(k, _)| *k == key).map(|(k, v)| (k.clone(), v.clone())).collect();
    match crate::params::string_list("", &Value::Object(one), key) {
        Ok(names) => Ok(Some(Names(names))),
        Err(crate::EngineError::BadParams { msg, .. }) => refuse(msg),
        Err(e) => refuse(e),
    }
}

/// The fields of a call.
fn fields<'de, D: Deserializer<'de>>(d: D) -> Result<Fields, D::Error> {
    Fields::deserialize(d)
}

/// Adds a key the type has optional so that the handler refuses it when it is left out, as
/// `require`, for every one of `keys`.
fn require_all(schema: &mut Schema, keys: &[&str]) {
    for key in keys {
        require(schema, key);
    }
}

/// Requires one of two sets of keys, for the commands that take two forms.
fn require_either(schema: &mut Schema, first: &[&str], second: &[&str]) {
    schema.insert("anyOf".into(), json!([{"required": first}, {"required": second}]));
}

fn a_and_b_required(schema: &mut Schema) {
    require_all(schema, &["a", "b"]);
}

/// What a command that adds one constraint returns: the constraint, the sketch's solve and, when
/// the sketch already determined the constraint, what determines it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Constrained {
    /// The new constraint's id.
    pub constraint: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// `true` when other constraints already determined it: it is kept, shown as redundant (not
    /// when `if_redundant` is `refuse`, which refuses the call instead). Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redundant: Option<bool>,
    /// With `redundant`: the constraints that determine it, as "id (Name)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub determined_by: Option<Vec<String>>,
}

/// What `sketch.constraint.horizontal_vertical`, `parallel`, `perpendicular`, `collinear`,
/// `concentric` and `midpoint` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConstraintResult {
    #[serde(flatten)]
    pub made: Constrained,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.constraint.tangent` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TangentResult {
    #[serde(flatten)]
    pub made: TangentMade,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What a Tangent call added.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TangentMade {
    /// Two curves: one Tangent constraint.
    Curves(Constrained),
    /// A curve's end point for `a` or `b` (#12092): the end now lies on the other curve (a
    /// point-on-curve or, for two end points, a coincident constraint, left out when the end was
    /// already there) and its curve is tangent there.
    End {
        /// The Tangent constraint.
        constraint: String,
        /// Every constraint added, the Tangent last.
        constraints: Vec<String>,
        /// The sketch's solve: `{solved, dof}`.
        sketch: Value,
        /// `true` when they already held and other constraints already determined them: they are
        /// kept, shown as redundant (not when `if_redundant` is `refuse`, which refuses the call
        /// instead). Absent otherwise.
        #[serde(skip_serializing_if = "Option::is_none")]
        redundant: Option<bool>,
        /// With `redundant`: the constraints that determine the Tangent, as "id (Name)".
        #[serde(skip_serializing_if = "Option::is_none")]
        determined_by: Option<Vec<String>>,
    },
}

/// What `sketch.constraint.coincident` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CoincidentResult {
    #[serde(flatten)]
    pub made: CoincidentMade,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What a Coincident call did.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CoincidentMade {
    /// A coincident (or point-on-curve) constraint was added.
    Added(Constrained),
    /// The two points (or two lines' closest ends) were already coincident: nothing was added.
    Existing {
        /// The constraint that already joins them.
        constraint: String,
        /// Always `true`.
        existing: bool,
        /// The sketch's solve: `{solved, dof}`.
        sketch: Value,
    },
    /// Two circles or arcs: the second now lies on the first (a Concentric and an Equal
    /// constraint; one of them is left out when it was already there).
    Circles {
        /// The first constraint added.
        constraint: String,
        /// Every constraint added.
        constraints: Vec<String>,
        /// The sketch's solve: `{solved, dof}`.
        sketch: Value,
    },
}

/// What `sketch.constraint.equal` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EqualResult {
    #[serde(flatten)]
    pub made: EqualMade,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What an Equal call added.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum EqualMade {
    /// `a` and `b`: one Equal constraint.
    Pair(Constrained),
    /// `curves`: an Equal constraint from the first curve to each other one (a pair that was
    /// already equal is left out).
    Curves {
        /// The constraints added.
        constraints: Vec<String>,
        /// The sketch's solve: `{solved, dof}`.
        sketch: Value,
    },
}

/// What `sketch.constraint.symmetry` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SymmetryResult {
    /// The constraints added: one Symmetric for two points, two for two lines, a Symmetric and an
    /// Equal for two circles or arcs; about a point, one Midpoint constraint for each pair, on
    /// a new construction line (reported in `created`).
    pub constraints: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What a Fix call did to the entities.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FixOutcome {
    /// Whether they are fixed now (`false`: released).
    pub fixed: bool,
    /// How many points and curves changed; those already as asked, the origin and projected
    /// geometry are not counted. A point placed by parameter expressions is released as plain
    /// numbers at its current place.
    pub changed: usize,
}

/// What `sketch.constraint.fix` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FixResult {
    /// What the call did.
    pub result: FixOutcome,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A command that takes two operands, `a` and `b`, in one form: the same keys, read with the
/// handler's message for each.
macro_rules! two_operands {
    (
        $(#[$meta:meta])* $name:ident,
        a: $a_doc:literal, b: $b_doc:literal, what: $what:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, JsonSchema)]
        #[schemars(deny_unknown_fields, transform = a_and_b_required)]
        pub struct $name {
            #[doc = $a_doc]
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[schemars(with = "Operand")]
            pub a: Option<Operand>,
            #[doc = $b_doc]
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[schemars(with = "Operand")]
            pub b: Option<Operand>,
            /// `refuse` (in any case) refuses the call when the sketch already determines the new
            /// constraint, instead of keeping it as a redundant one (any other text keeps it).
            /// Read only when the constraint turns out redundant.
            #[serde(default)]
            pub if_redundant: Option<String>,
            /// The sketch (default: the one being edited).
            #[serde(default)]
            pub sketch: Option<SketchKey>,
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let m = fields(d)?;
                let read = || -> Read<Self> {
                    let sketch = sketch_of(&m)?;
                    let a = operand_of(&m, "a", &format!("`a` must be {}", $what))?;
                    let b = operand_of(&m, "b", &format!("`b` must be {}", $what))?;
                    Ok(Self { a, b, if_redundant: if_redundant_of(&m)?, sketch })
                };
                read().map_err(D::Error::custom)
            }
        }
    };
}

two_operands! {
    /// Make two curves, a curve and a sketch axis, or a curve's end point and another curve,
    /// tangent. Two different curves are needed.
    TangentParams,
    a: "A curve's id, or `x` / `y` for the sketch's own axis (a curve with that id wins; an axis becomes a linked construction line, reported in `created`), or a model edge, or the end point of a line or an arc (such as `l1.end`): that end then lies on the other curve, and its curve is tangent there (`constraints` lists both).",
    b: "The other curve or end point, as `a`.",
    what: "a curve id, or x / y for a sketch axis"
}

two_operands! {
    /// Make two lines, or a line and a sketch axis, parallel. Two different lines are needed.
    ParallelParams,
    a: "A line's id, or `x` / `y` for the sketch's own axis (a curve with that id wins; an axis becomes a linked construction line, reported in `created`), or a model edge.",
    b: "The other line, as `a`.",
    what: "a curve id, or x / y for a sketch axis"
}

two_operands! {
    /// Make two lines, or a line and a sketch axis, perpendicular. Two different lines are
    /// needed.
    PerpendicularParams,
    a: "A line's id, or `x` / `y` for the sketch's own axis (a curve with that id wins; an axis becomes a linked construction line, reported in `created`), or a model edge.",
    b: "The other line, as `a`.",
    what: "a curve id, or x / y for a sketch axis"
}

two_operands! {
    /// Make two lines, or a line and a sketch axis, lie on one line. Two different lines are
    /// needed.
    CollinearParams,
    a: "A line's id, or `x` / `y` for the sketch's own axis (a curve with that id wins; an axis becomes a linked construction line, reported in `created`), or a model edge.",
    b: "The other line, as `a`.",
    what: "a curve id, or x / y for a sketch axis"
}

two_operands! {
    /// Give two circles or arcs one centre, or put a circle's or an arc's centre on a sketch point
    /// (the origin too), in either order.
    ConcentricParams,
    a: "A circle's or arc's id, or a sketch point's id, or a model edge or vertex.",
    b: "The other circle or arc, or the sketch point, as `a`.",
    what: "a curve id"
}

/// Make two curves equal (two lines the same length, two circles or arcs the same radius), or
/// each of a list of curves equal to the first, in one undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = equal_required)]
pub struct EqualParams {
    /// A line, circle or arc, or a model edge. Read when `curves` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub a: Option<Operand>,
    /// The other line, circle or arc, as `a`. Read when `curves` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub b: Option<Operand>,
    /// 2 to 200 lines, circles or arcs (ids), each made equal to the first. When the key is given
    /// (`null` too), `a` and `b` are ignored, and so is `if_redundant`.
    #[serde(default)]
    pub curves: Option<Names>,
    /// `refuse` (in any case) refuses the call when the sketch already determines the new
    /// constraint, instead of keeping it as a redundant one (any other text keeps it). Read
    /// only with `a` and `b`, and when the constraint turns out redundant.
    #[serde(default)]
    pub if_redundant: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchKey>,
}

fn equal_required(schema: &mut Schema) {
    require_either(schema, &["a", "b"], &["curves"]);
}

impl<'de> Deserialize<'de> for EqualParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = fields(d)?;
        let read = || -> Read<Self> {
            if m.contains_key("curves") {
                let curves = names_of(&m, "curves")?;
                if !(2..=200).contains(&curves.as_ref().map_or(0, |c| c.0.len())) {
                    return refuse("`curves` must list 2 to 200 lines or circles/arcs");
                }
                return Ok(Self { a: None, b: None, curves, if_redundant: None, sketch: sketch_of(&m)? });
            }
            let sketch = sketch_of(&m)?;
            let a = operand_of(&m, "a", "`a` must be a curve id")?;
            let b = operand_of(&m, "b", "`b` must be a curve id")?;
            Ok(Self { a, b, curves: None, if_redundant: if_redundant_of(&m)?, sketch })
        };
        read().map_err(D::Error::custom)
    }
}

/// Make two points coincident, put a point on a curve or a sketch axis, join the closest ends of
/// two lines, or put one circle or arc on another (concentric and equal).
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = a_and_b_required)]
pub struct CoincidentParams {
    /// A point, a curve or axis (`x` / `y`: the sketch's own axes, unless a curve has that id; an
    /// axis becomes a linked construction line on it, reported in `created`), a line's middle
    /// `{midpoint: line_id}` or a model edge or vertex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointOperand")]
    pub a: Option<PointOperand>,
    /// The other one, as `a`. Two lines join their closest ends; two circles or arcs make the
    /// second lie on the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointOperand")]
    pub b: Option<PointOperand>,
    /// `refuse` (in any case) refuses the call when the sketch already determines the new
    /// constraint, instead of keeping it as a redundant one (any other text keeps it). Read
    /// only when a constraint is added for a point and a point, a point and a curve or two
    /// lines, and it turns out redundant.
    #[serde(default)]
    pub if_redundant: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchKey>,
}

impl<'de> Deserialize<'de> for CoincidentParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = fields(d)?;
        let read = || -> Read<Self> {
            let sketch = sketch_of(&m)?;
            let (a, b) = (point_operand(m.get("a")), point_operand(m.get("b")));
            // The handler looks at the middles first, then `b`, then `a`.
            if matches!(a, Err(Wrong::Midpoint)) || matches!(b, Err(Wrong::Midpoint)) {
                return refuse("`midpoint` must be a curve id");
            }
            let b = b.or_else(|_| refuse("`b` must be a point or curve"))?;
            // With no `b` the handler stops at `b` first, whatever `a` is.
            let a = a.or_else(|_| if b.is_none() { refuse("`b` must be a point or curve") } else { refuse("`a` must be a point reference") })?;
            Ok(Self { a, b, if_redundant: if_redundant_of(&m)?, sketch })
        };
        read().map_err(D::Error::custom)
    }
}

/// Make a line horizontal or vertical, or two points level (same height) or plumb (one above the
/// other), by `line` or by `points`.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = horizontal_vertical_required)]
pub struct HorizontalVerticalParams {
    /// A line's id. Read when `points` is not a list; ignored when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub line: Option<String>,
    /// Exactly two points: ids, or a line's middle as `{midpoint: line_id}`. When it is a list,
    /// `line` is ignored; anything else is ignored, as if the key were left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "[AlignPoint; 2]")]
    pub points: Option<[AlignPoint; 2]>,
    /// `horizontal` or `vertical` (in any case). Default, and for any other text: horizontal when
    /// the line or the two points are further apart across than up, else vertical.
    #[serde(default)]
    pub mode: Option<String>,
    /// `refuse` (in any case) refuses the call when the sketch already determines the new
    /// constraint, instead of keeping it as a redundant one (any other text keeps it). Read
    /// only when the constraint turns out redundant.
    #[serde(default)]
    pub if_redundant: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchKey>,
}

fn horizontal_vertical_required(schema: &mut Schema) {
    require_either(schema, &["line"], &["points"]);
}

impl<'de> Deserialize<'de> for HorizontalVerticalParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = fields(d)?;
        let read = || -> Read<Self> {
            let mode = text_of(&m, "mode")?;
            let sketch = sketch_of(&m)?;
            let if_redundant = if_redundant_of(&m)?;
            if let Some(list) = m.get("points").and_then(Value::as_array) {
                let [a, b] = list.as_slice() else { return refuse("`points` needs exactly two point references") };
                let points = Some([align_point(a)?, align_point(b)?]);
                return Ok(Self { line: None, points, mode, if_redundant, sketch });
            }
            let line = match m.get("line") {
                None => None,
                Some(Value::String(id)) => Some(id.clone()),
                Some(_) => return refuse("`line` must be a curve id"),
            };
            Ok(Self { line, points: None, mode, if_redundant, sketch })
        };
        read().map_err(D::Error::custom)
    }
}

/// Tie a point to the middle of a line, or make two lines' middles meet (the first stays, the
/// second moves), by `point` and `line` or by `a` and `b`.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = midpoint_required)]
pub struct MidpointParams {
    /// A point or a line, in pick order (a point and a line either way round, or two lines). When
    /// `a` or `b` is given (`null` too), `point` and `line` are ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub a: Option<Operand>,
    /// The other one, as `a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub b: Option<Operand>,
    /// The point. Read when neither `a` nor `b` is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub point: Option<Operand>,
    /// The line it goes to the middle of. Read when neither `a` nor `b` is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub line: Option<Operand>,
    /// `refuse` (in any case) refuses the call when the sketch already determines the new
    /// constraint, instead of keeping it as a redundant one (any other text keeps it). Read
    /// only when the constraint turns out redundant.
    #[serde(default)]
    pub if_redundant: Option<String>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchKey>,
}

fn midpoint_required(schema: &mut Schema) {
    require_either(schema, &["a", "b"], &["point", "line"]);
}

impl<'de> Deserialize<'de> for MidpointParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = fields(d)?;
        let read = || -> Read<Self> {
            let sketch = sketch_of(&m)?;
            let none = Self { a: None, b: None, point: None, line: None, if_redundant: None, sketch };
            if m.contains_key("a") || m.contains_key("b") {
                // A `null` is a value here, which the handler refuses.
                let a = operand_of(&m, "a", "`a` must be a point or line id")?;
                let b = operand_of(&m, "b", "`b` must be a point or line id")?;
                return Ok(Self { a, b, if_redundant: if_redundant_of(&m)?, ..none });
            }
            let point = operand_of(&m, "point", "`point` must be a point reference")?;
            let line = operand_of(&m, "line", "`line` must be a curve id")?;
            Ok(Self { point, line, if_redundant: if_redundant_of(&m)?, ..none })
        };
        read().map_err(D::Error::custom)
    }
}

/// Make two points, two lines or two circles or arcs mirror images about a line or a sketch axis
/// (`line`), or about a point (`point`): each pair then joined by a construction line that the
/// point is the middle of.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = symmetry_required)]
pub struct SymmetryParams {
    /// The first point or curve (lines mirror their ends, circles and arcs their centres). It
    /// stays; the second moves onto its mirror image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub a: Option<Operand>,
    /// The second point or curve, as `a`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub b: Option<Operand>,
    /// The symmetry line's id, or `x` / `y` for the sketch's own axis (a curve with that id wins;
    /// an axis becomes a linked construction line, reported in `created`). A point's id (or
    /// `origin`) instead makes the symmetry about that point. Read when `point` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub line: Option<Operand>,
    /// The point the symmetry is about. When the key is given (`null` too), `line` is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Operand")]
    pub point: Option<Operand>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchKey>,
}

fn symmetry_required(schema: &mut Schema) {
    require_all(schema, &["a", "b"]);
    require_either(schema, &["line"], &["point"]);
}

impl<'de> Deserialize<'de> for SymmetryParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = fields(d)?;
        let read = || -> Read<Self> {
            let sketch = sketch_of(&m)?;
            let (mut point, mut line) = (None, None);
            if m.contains_key("point") {
                // A `null` is a value here, which the handler refuses.
                point = operand_of(&m, "point", "`point` must be a point reference")?;
            } else {
                line = operand_of(&m, "line", "`line` must be a curve id, or x / y for a sketch axis")?;
            }
            let a = operand_of(&m, "a", "`a` must be a point or curve")?;
            let b = operand_of(&m, "b", "`b` must be a point or curve")?;
            Ok(Self { a, b, line, point, sketch })
        };
        read().map_err(D::Error::custom)
    }
}

/// Fix points and curves where they are, or release them (a curve keeps its radius too; the
/// origin and projected geometry are left as they are; a point placed by parameter expressions
/// is released where they come to now, as plain numbers).
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = fix_required)]
pub struct FixParams {
    /// One point or curve (an id). Goes first among `entities`.
    #[serde(default)]
    pub entity: Option<String>,
    /// Points and curves (ids): one, or a list of up to 10000. `entity` or `entities` must name
    /// at least one.
    #[serde(default)]
    pub entities: Option<Names>,
    /// `true` fixes them all, `false` releases them all. Default: fix them, unless all are
    /// fixed already, then release them.
    #[serde(default)]
    pub fixed: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchKey>,
}

fn fix_required(schema: &mut Schema) {
    require_either(schema, &["entity"], &["entities"]);
}

impl<'de> Deserialize<'de> for FixParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = fields(d)?;
        let read = || -> Read<Self> {
            let entities = names_of(&m, "entities")?;
            let fixed = flag_of(&m, "fixed")?;
            // The handler reads `entity` as text only; the refusal for none at all comes first.
            if entities.as_ref().is_none_or(|e| e.0.is_empty()) && !m.get("entity").is_some_and(Value::is_string) {
                return refuse("`entity` or `entities` must name sketch points or curves");
            }
            let entity = text_of(&m, "entity")?;
            Ok(Self { entity, entities, fixed, sketch: sketch_of(&m)? })
        };
        read().map_err(D::Error::custom)
    }
}

#[cfg(test)]
#[path = "sketch_tests.rs"]
mod tests;
