//! SOLID ▸ CONSTRUCT: Plane, Axis and Point. They have no methods of their own. Given
//! references, they try their family's CONSTRUCT commands (`construct.plane.*`,
//! `construct.axis.*`, `construct.point.*`, in the order the commands are listed) with the
//! references in each command's reference parameters, and build with the first that takes them.
//! A method added to a family is tried as well, with nothing to change here.

use serde_json::{Map, Value, json};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::construct::{ConstructAutoParams, ConstructAutoResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<ConstructAutoParams> = TypedContract::new();

const PARAMS: &str = "refs: 1-3 refs in any order; with?: {the method's other parameters: offset, angle, t, name…}. Builds with the first of the family's CONSTRUCT commands that takes the refs in its reference parameters (and every key of `with`). Returns method: the command used";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("construct.plane", "Plane", crate::checked!("construct.plane", plane))
        .at("SOLID", "CONSTRUCT")
        .icon("plane_auto")
        .contract(&CONTRACT)
        .output(output_schema_for::<ConstructAutoResult>)
        .params(PARAMS),
    CommandSpec::new("construct.axis", "Axis", crate::checked!("construct.axis", axis))
        .at("SOLID", "CONSTRUCT")
        .icon("axis")
        .contract(&CONTRACT)
        .output(output_schema_for::<ConstructAutoResult>)
        .params(PARAMS),
    CommandSpec::new("construct.point", "Point", crate::checked!("construct.point", point))
        .at("SOLID", "CONSTRUCT")
        .icon("point")
        .contract(&CONTRACT)
        .output(output_schema_for::<ConstructAutoResult>)
        .params(PARAMS),
];

/// Whether `id` is one of the tools that choose their method (`construct.plane`…).
pub fn is_construct_tool(id: &str) -> bool {
    COMMANDS.iter().any(|c| c.id == id)
}

/// The methods of `tool` (`construct.plane` → `construct.plane.offset`, `construct.plane.midplane`
/// …), in the order the commands are listed: the order they are tried in. Planned commands are
/// left out.
pub fn construct_methods(tool: &str) -> Vec<&'static CommandSpec> {
    let prefix = format!("{tool}.");
    super::command_specs().into_iter().filter(|c| c.id.starts_with(&prefix) && c.planned.is_none() && c.contract.is_some()).collect()
}

/// How a reference parameter reads a reference (its schema type).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    /// Any `GeoRef` form, as given.
    Geo,
    /// `PlaneOrFace`: a plane as such (`{plane: p}` given as `p`) or `{face}`.
    Plane,
    /// `AxisLine`: an axis as such (`{axis: a}` given as `a`), `{edge}` or `{face}`.
    Axis,
}

/// A parameter of a method that takes a reference: its key, how it reads one, and whether the
/// method needs it.
struct Slot {
    key: String,
    form: Form,
    required: bool,
}

/// The schema type a property refers to, directly or as one of its `anyOf`/`oneOf` (an
/// optional parameter's).
fn form_of(property: &Value) -> Option<Form> {
    let named = |v: &Value| match v.get("$ref").and_then(Value::as_str).and_then(|r| r.rsplit('/').next()) {
        Some("GeoRef") => Some(Form::Geo),
        Some("PlaneOrFace") => Some(Form::Plane),
        Some("AxisLine") => Some(Form::Axis),
        _ => None,
    };
    named(property).or_else(|| ["anyOf", "oneOf"].iter().filter_map(|k| property.get(*k).and_then(Value::as_array)).flatten().find_map(named))
}

/// The reference parameters of `spec`, read from its input schema.
fn slots(spec: &CommandSpec) -> Vec<Slot> {
    let Some(contract) = spec.contract else { return Vec::new() };
    let schema = contract.input_schema();
    let root = schema.as_value();
    let required: Vec<&str> =
        root.get("required").and_then(Value::as_array).map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let Some(properties) = root.get("properties").and_then(Value::as_object) else { return Vec::new() };
    properties
        .iter()
        .filter_map(|(key, p)| form_of(p).map(|form| Slot { key: key.clone(), form, required: required.contains(&key.as_str()) }))
        .collect()
}

/// `r` as a parameter of `form` reads it.
fn as_form(r: &Value, form: Form) -> Value {
    let key = match form {
        Form::Geo => return r.clone(),
        Form::Plane => "plane",
        Form::Axis => "axis",
    };
    match r.as_object() {
        Some(o) if o.len() == 1 => o.get(key).cloned().unwrap_or_else(|| r.clone()),
        _ => r.clone(),
    }
}

/// Every way to put each of `refs` in its own slot of `slots`, with every required slot filled:
/// the slot index of each reference. The first ways keep the references in the slots' order.
fn placements(refs: usize, slots: &[Slot]) -> Vec<Vec<usize>> {
    fn go(refs: usize, slots: &[Slot], at: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if at.len() == refs {
            if slots.iter().enumerate().all(|(i, s)| !s.required || at.contains(&i)) {
                out.push(at.clone());
            }
            return;
        }
        for i in 0..slots.len() {
            if !at.contains(&i) {
                at.push(i);
                go(refs, slots, at, out);
                at.pop();
            }
        }
    }
    let mut out = Vec::new();
    if refs <= slots.len() {
        go(refs, slots, &mut Vec::with_capacity(refs), &mut out);
    }
    out
}

/// The parameters of `spec` with `refs` placed as `at` says, and `with`.
fn params(refs: &[Value], slots: &[Slot], at: &[usize], with: &Map<String, Value>) -> Value {
    let mut p = with.clone();
    for (r, i) in refs.iter().zip(at) {
        if let Some(slot) = slots.get(*i) {
            p.insert(slot.key.clone(), as_form(r, slot.form));
        }
    }
    Value::Object(p)
}

/// The first method of `tool` that builds from `refs` with `with`, tried on a scratch copy of
/// `s`: its command and parameters. The error, when none does, is the first a method gave whose
/// parameters fitted, else a list of what can be built from.
pub fn construct_method(s: &Session, tool: &str, refs: &[Value], with: &Map<String, Value>) -> Result<(&'static str, Value)> {
    let mut first_error = None;
    for spec in construct_methods(tool) {
        let Some(contract) = spec.contract else { continue };
        let slots = slots(spec);
        for at in placements(refs.len(), &slots) {
            let p = params(refs, &slots, &at, with);
            if !contract.check(&p).is_empty() {
                continue;
            }
            let mut trial = s.scratch();
            match spec.run_nested(&mut trial, &p) {
                Ok(_) => return Ok((spec.id, p)),
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }
    }
    Err(first_error.unwrap_or_else(|| {
        let names: Vec<&str> = construct_methods(tool).iter().map(|c| c.label).collect();
        bad(tool, format!("no method takes these references; the methods are: {}", names.join(", ")))
    }))
}

fn build(s: &mut Session, tool: &str, p: &Value) -> Result<Value> {
    // The contract has checked `refs` and `with`; this reads them as given.
    let refs = p.get("refs").and_then(Value::as_array).cloned().unwrap_or_default();
    let with = p.get("with").and_then(Value::as_object).cloned().unwrap_or_default();
    let (method, params) = construct_method(s, tool, &refs, &with)?;
    let spec = super::find_command(method).ok_or_else(|| bad(tool, format!("no command `{method}`")))?;
    let mut reply = spec.run_nested(s, &params)?;
    if let Value::Object(m) = &mut reply {
        m.insert("method".into(), json!(method));
    }
    Ok(reply)
}

fn plane(s: &mut Session, p: &Value) -> Result<Value> {
    build(s, "construct.plane", p)
}

fn axis(s: &mut Session, p: &Value) -> Result<Value> {
    build(s, "construct.axis", p)
}

fn point(s: &mut Session, p: &Value) -> Result<Value> {
    build(s, "construct.point", p)
}
