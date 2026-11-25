//! Pipe routes. `route.create` builds a butt-welded pipe line from its centreline points and a
//! pipe spec (NPS, schedule, elbow type): an elbow at every square corner, a tee where three
//! segments meet, and the reducers, valves, caps and welds the route names, each piece a
//! component of the route's own sub-assembly, placed by its ports and held to its neighbour by a
//! rigid joint at the weld, the first pipe grounded. The route keeps its spec in its component's
//! metadata (`io.tangencylabs.solidcraft.route`), so `route.move_point` and `route.edit` change
//! the centreline or the spec and build the route again: pipes take their new cut lengths and the
//! fittings their new places. `route.get` and `route.list` read routes back (the pieces, the cut
//! list), `route.fittings` lists the sizes. The planner is `route_plan.rs`.

use serde_json::{Map, Value, json};
use solidcraft_doc::metadata::{DESCRIPTION, Owner};

use super::route_plan::{self as rp, Kind, M34, Piece, Spec};
use super::{CommandSpec, not_in_sketch};
use crate::params::{bad, bool_, str_, vec3};
use crate::schema::route::{
    CreateRouteParams, EditRouteParams, MoveRoutePointParams, RouteBuildResult, RouteDeleteResult, RouteFittingsResult, RouteGetResult,
    RouteListParams, RouteListResult, RouteMoveResult, RouteNameParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;

/// Where a route's component keeps its spec.
pub const KEY: &str = "io.tangencylabs.solidcraft.route";

/// The colours pieces get unless the spec's `colours` say otherwise.
const PIPE_COLOUR: &str = "#8a8d91";
const FITTING_COLOUR: &str = "#6f7378";
const VALVE_COLOUR: &str = "#c0392b";

macro_rules! spec_doc {
    () => {
        "name (1…64 letters, digits, - or _: the route's sub-assembly, its pieces named <name>_<kind>_<n>); nps?: the route's pipe size (0.5…4); \
         schedule?: \"10S\"|\"40\"|\"80\" (default 40); elbow?: long_radius|short_radius (default long_radius, B16.9); points?: [[x,y,z]…] (a polyline \
         at the route's nps) and/or segments?: [{a, b, nps?}] (branches meet at tees); fittings?: [{kind: reducer, large, small, nps?, \
         nps_small?} | {kind: valve, lib, a, b, up, overlap?} | {kind: cap, at} | {kind: weld, at} | {kind: open, at}]; valves?: {lib: \
         {step: STEP path, port_a, port_b, up, pre?: 3x4}} (ports and spindle in the valve's frame); start?: [x,y,z] (the end a point move \
         holds still; default the first point); colours?: {pipe, fitting, valve: #rrggbb}. Points in mm, world"
    };
}

const CREATE_CONTRACT: TypedContract<CreateRouteParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<EditRouteParams> = TypedContract::new();
const MOVE_CONTRACT: TypedContract<MoveRoutePointParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<RouteNameParams> = TypedContract::new();
const GET_CONTRACT: TypedContract<RouteNameParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<RouteListParams> = TypedContract::new();
const FITTINGS_CONTRACT: TypedContract<RouteListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("route.create", "Pipe Route", crate::checked!("route.create", create))
        .at("WELDMENTS", "CREATE")
        .icon("pipe")
        .enabled(not_in_sketch)
        .contract(&CREATE_CONTRACT)
        .output(output_schema_for::<RouteBuildResult>)
        .params(concat!(
            spec_doc!(),
            "; frame?: 12 numbers (a rigid row-major 3x4 matrix the whole route is taken through first). Builds the route's sub-assembly: \
         one component per pipe and fitting, valves inserted from their STEP files, every piece held by a rigid joint at its weld, the \
         first pipe grounded → route, component, pieces [{name, kind, lib?, nps, length?, ports, matrix?, occurrence, bodies}], cut_list \
         [{piece, nps, schedule, length}], welds, free_dof, unjoined. One undo step"
        )),
    CommandSpec::new("route.edit", "Edit Pipe Route", crate::checked!("route.edit", edit))
        .icon("edit")
        .enabled(not_in_sketch)
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<RouteBuildResult>)
        .params(concat!(
            "route: its name; then any spec field to change (",
            spec_doc!(),
            "; the name stays): the route is built again from the changed spec → as route.create"
        )),
    CommandSpec::new("route.move_point", "Move Route Point", crate::checked!("route.move_point", move_point))
        .icon("move")
        .enabled(not_in_sketch)
        .contract(&MOVE_CONTRACT)
        .output(output_schema_for::<RouteMoveResult>)
        .params(
            "route: its name; at: [x,y,z] (a route point: a corner, a branch, an end, a weld or a fitting port); delta: [dx,dy,dz] or to: [x,y,z]; \
         carry?: bool (default true: the points beyond it, away from the route's start, move with it, as a dragged leg; false: only that \
         point). The route is built again: pipes take their new cut lengths, fittings their new places, a new pipe where a fitting moved \
         off its neighbour → as route.create, with moved_points. Joints from outside the route to its pieces do not survive the rebuild",
        ),
    CommandSpec::new("route.delete", "Delete Pipe Route", crate::checked!("route.delete", delete))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<RouteDeleteResult>)
        .params("route: its name → deleted: the components removed"),
    CommandSpec::new("route.get", "Pipe Route Details", crate::checked!("route.get", get))
        .icon("list")
        .noundo()
        .read_only()
        .doc_only()
        .contract(&GET_CONTRACT)
        .output(output_schema_for::<RouteGetResult>)
        .params("route: its name → route, component, spec (as stored), points (the route's nodes), pieces, cut_list, bodies"),
    CommandSpec::new("route.list", "Pipe Routes", crate::checked!("route.list", list))
        .icon("list")
        .noundo()
        .read_only()
        .doc_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<RouteListResult>)
        .params("→ routes [{name, component, nps, schedule, elbow, pieces, pipes, pipe_length}]"),
    CommandSpec::new("route.fittings", "Pipe Fitting Library", crate::checked!("route.fittings", fittings))
        .icon("library")
        .noundo()
        .read_only()
        .doc_only()
        .contract(&FITTINGS_CONTRACT)
        .output(output_schema_for::<RouteFittingsResult>)
        .params(
            "→ pipes [{nps, od, od_b3610, od_en, wall {10S, 40, 80, ACR, EN}, elbow_long_radius, elbow_short_radius, tee, cap}], reducing_tees, \\
         reducers, schedules, elbows: the sizes routes are built from (mm). ACR is ASTM B280 copper tube (1-1/8 in, OD 28.575 mm, 1.0 mm wall); \\
         EN is EN 10253-4 stainless tube (1 in, OD 33.7 mm, 1.6 mm wall)",
        ),
];

fn nested(s: &mut Session, id: &str, p: Value) -> Result<Value> {
    super::find_command(id).ok_or_else(|| bad("route", format!("{id} is not available")))?.run_nested(s, &p)
}

fn spec_of(cmd: &str, v: &Value) -> Result<Spec> {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("frame");
        o.remove("route");
    }
    let spec: Spec = serde_json::from_value(v).map_err(|e| bad(cmd, format!("the route spec: {e}")))?;
    spec.check().map_err(|e| bad(cmd, e))?;
    Ok(spec)
}

fn matrix(cmd: &str, v: &Value) -> Result<M34> {
    let nums: Vec<f64> = match v {
        Value::Array(a) if a.len() == 12 => a.iter().filter_map(Value::as_f64).collect(),
        _ => Vec::new(),
    };
    let m: M34 = nums.try_into().map_err(|_| bad(cmd, "`frame` must be 12 numbers: a row-major 3x4 matrix"))?;
    if !rp::rigid(&m) {
        return Err(bad(cmd, "`frame` must be a rotation and translation (no scale, shear or mirror)"));
    }
    Ok(m)
}

/// The route component named `name` (one that carries a route spec).
fn route_component(s: &Session, name: &str) -> Option<u64> {
    s.doc.components.iter().find(|c| c.name == name && c.metadata.get(KEY).is_some()).map(|c| c.id)
}

fn stored(s: &Session, cmd: &str, name: &str) -> Result<(u64, Value)> {
    let id = route_component(s, name).ok_or_else(|| bad(cmd, format!("no pipe route `{name}` (route.list lists them)")))?;
    let rec = s.doc.components.iter().find(|c| c.id == id).and_then(|c| c.metadata.get(KEY)).cloned().unwrap_or(Value::Null);
    Ok((id, rec))
}

fn stored_spec(s: &Session, cmd: &str, name: &str) -> Result<(u64, Spec, Value)> {
    let (id, rec) = stored(s, cmd, name)?;
    let spec = rec.get("spec").ok_or_else(|| bad(cmd, format!("route `{name}` has no spec")))?;
    let mut spec = spec_of(cmd, spec)?;
    // The component's name is the route's identity (`route_component` looks it up); the name in
    // the stored spec goes stale when `component.rename` renames the sub-assembly. A name that
    // cannot name the route's pieces keeps the spec's own.
    if let Some(c) = s.doc.components.iter().find(|c| c.id == id)
        && rp::name_ok(&c.name)
    {
        spec.name = c.name.clone();
    }
    Ok((id, spec, rec))
}

fn route_name<'a>(cmd: &str, p: &'a Value) -> Result<&'a str> {
    str_(p, "route").ok_or_else(|| bad(cmd, "`route` (the route's name) is required"))
}

fn pieces_json(pieces: &[Piece], built: &Map<String, Value>) -> Vec<Value> {
    pieces
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
            if let (Some(o), Some(b)) = (v.as_object_mut(), built.get(&p.name)) {
                if let Some(occ) = b.get("occurrence") {
                    o.insert("occurrence".into(), occ.clone());
                }
                if let Some(bodies) = b.get("bodies") {
                    o.insert("bodies".into(), bodies.clone());
                }
            }
            v
        })
        .collect()
}

fn schedule_word(s: rp::Schedule) -> &'static str {
    match s {
        rp::Schedule::S40 => "40",
        rp::Schedule::S80 => "80",
        rp::Schedule::S40S => "40S",
        rp::Schedule::S10S => "10S",
        rp::Schedule::Acr => "ACR",
        rp::Schedule::En => "EN",
    }
}

fn cut_list(spec: &Spec, pieces: &[Piece]) -> Vec<Value> {
    pieces
        .iter()
        .filter(|p| p.kind == Kind::Pipe)
        .map(|p| json!({"piece": p.name, "nps": p.nps, "schedule": schedule_word(spec.schedule), "length": p.length}))
        .collect()
}

/// A piece's part description, as the bill of materials shows it.
fn description(spec: &Spec, p: &Piece) -> String {
    let sch = schedule_word(spec.schedule);
    let size = |n: f64| format!("{} in", rp::nps_s(n).replace('_', "."));
    match p.kind {
        Kind::Pipe => format!("Pipe {} Sch {sch}, L {:.1} mm", size(p.nps), p.length.unwrap_or(0.0)),
        Kind::Elbow => {
            let a = p.angle.unwrap_or(90.0);
            let kind = if (a - 90.0).abs() < 1e-6 { "90".to_string() } else { format!("{a:.2} deg") };
            format!("Elbow {kind} {} {} Sch {sch} BW", if spec.elbow == rp::Elbow::ShortRadius { "SR" } else { "LR" }, size(p.nps))
        }
        Kind::Tee => match p.nps_small {
            Some(b) => format!("Tee {} x {} Sch {sch} BW", size(p.nps), size(b)),
            None => format!("Tee {} Sch {sch} BW", size(p.nps)),
        },
        Kind::Reducer => format!("Reducer conc. {} x {} Sch {sch} BW", size(p.nps), size(p.nps_small.unwrap_or(0.0))),
        Kind::Cap => format!("Cap {} Sch {sch} BW", size(p.nps)),
        Kind::Valve => format!("Valve {}", p.lib.clone().unwrap_or_default()),
    }
}

fn set_component_metadata(s: &mut Session, cmd: &str, comp: u64, key: &str, value: Value) -> Result<()> {
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == comp) {
        c.metadata.set(Owner::Component, key, value).map_err(|e| bad(cmd, e.to_string()))?;
    }
    Ok(())
}

fn id_of(cmd: &str, v: &Value, what: &str) -> Result<u64> {
    v.get(what).and_then(Value::as_u64).ok_or_else(|| bad(cmd, format!("no {what} came back from building the route")))
}

/// Build `spec` as a new route sub-assembly under the root and keep the spec on it.
fn build(s: &mut Session, cmd: &str, spec: &Spec) -> Result<Value> {
    let pieces = rp::plan(spec).map_err(|e| bad(cmd, e))?;
    if pieces.is_empty() {
        return Err(bad(cmd, "the route has no pieces"));
    }
    if let Some(p) = pieces.iter().find(|p| s.doc.find_component(&p.name).is_some()) {
        return Err(bad(cmd, format!("a component `{}` is already in the design: name the route differently", p.name)));
    }
    let active = s.active_component;
    nested(s, "component.activate", json!({"component": 0}))?;
    let made = nested(s, "component.create", json!({"name": spec.name, "activate": false, "grounded": true}))?;
    let comp = id_of(cmd, &made, "component")?;
    let route_occ = id_of(cmd, &made, "occurrence")?;
    let colour = |k: &str, d: &str| spec.colours.as_ref().and_then(|c| c.get(k)).cloned().unwrap_or_else(|| d.to_string());
    let mut built: Map<String, Value> = Map::new();
    let mut first_pipe = true;
    // Pipes first (the first one grounded), then the fittings, then the valves.
    let order = |k: Kind| match k {
        Kind::Pipe => 0,
        Kind::Valve => 2,
        _ => 1,
    };
    let mut ordered: Vec<&Piece> = pieces.iter().collect();
    ordered.sort_by_key(|p| order(p.kind));
    for p in ordered {
        let (occ, bodies) = if p.kind == Kind::Valve {
            let (Some(step), Some(m)) = (&p.step, p.matrix) else { return Err(bad(cmd, format!("{}: the valve has no STEP file", p.name))) };
            nested(s, "component.activate", json!({"component": comp}))?;
            let r = nested(s, "file.insert_step", json!({"path": step, "name": p.name, "matrix": m}))?;
            (id_of(cmd, &r, "occurrence")?, r.get("created_bodies").cloned().unwrap_or(json!([])))
        } else {
            let grounded = p.kind == Kind::Pipe && first_pipe;
            first_pipe &= !grounded;
            let c = nested(s, "component.create", json!({"name": p.name, "parent": comp, "activate": true, "grounded": grounded}))?;
            let (pc, occ) = (id_of(cmd, &c, "component")?, id_of(cmd, &c, "occurrence")?);
            let cmds = if p.kind == Kind::Pipe {
                rp::pipe_commands(p, spec.schedule, &p.name)
            } else {
                rp::fitting_commands(p, spec.schedule, spec.elbow, &p.name)
            }
            .map_err(|e| bad(cmd, e))?;
            let mut bodies = Value::Null;
            for (id, q) in cmds {
                let r = nested(s, id, q)?;
                if bodies.is_null() {
                    bodies = r.get("created_bodies").cloned().unwrap_or(Value::Null);
                }
            }
            nested(s, "component.activate", json!({"component": comp}))?;
            if let Some(m) = p.matrix {
                nested(s, "occurrence.move", json!({"occurrence": occ, "matrix": m, "space": "world"}))?;
            }
            set_component_metadata(s, cmd, pc, DESCRIPTION, json!(description(spec, p)))?;
            (occ, if bodies.is_null() { json!([p.name]) } else { bodies })
        };
        built.insert(p.name.clone(), json!({"occurrence": occ, "bodies": bodies}));
    }
    let occ_of = |i: usize| pieces.get(i).and_then(|p| built.get(&p.name)).and_then(|b| b.get("occurrence")).cloned();
    let (welds, unjoined) = rp::welds(&pieces);
    for (h, i) in &welds {
        let (Some(a), Some(b), Some(p)) = (occ_of(*h), occ_of(*i), pieces.get(*i)) else { continue };
        nested(s, "joint.as_built", json!({"type": "rigid", "a": a, "b": b, "name": format!("weld {}", p.name), "quiet": true}))?;
    }
    for (kinds, c) in [
        (&[Kind::Pipe][..], colour("pipe", PIPE_COLOUR)),
        (&[Kind::Elbow, Kind::Tee, Kind::Reducer, Kind::Cap][..], colour("fitting", FITTING_COLOUR)),
        (&[Kind::Valve][..], colour("valve", VALVE_COLOUR)),
    ] {
        let names: Vec<&str> = pieces.iter().filter(|p| kinds.contains(&p.kind)).map(|p| p.name.as_str()).collect();
        if !names.is_empty() {
            nested(s, "appearance.assign", json!({"components": names, "color": c}))?;
        }
    }
    let joints = nested(s, "joint.list", json!({}))?;
    let mine: Vec<u64> = std::iter::once(route_occ).chain(built.values().filter_map(|b| b.get("occurrence").and_then(Value::as_u64))).collect();
    let free_dof: Vec<Value> = joints
        .get("dof")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|d| {
                    d.get("occurrence").and_then(Value::as_u64).is_some_and(|o| mine.contains(&o)) && d.get("dof").and_then(Value::as_u64) != Some(0)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let bodies: Map<String, Value> = built.iter().map(|(k, v)| (k.clone(), v.get("bodies").cloned().unwrap_or(Value::Null))).collect();
    let spec_v = serde_json::to_value(spec).map_err(|e| bad(cmd, e.to_string()))?;
    set_component_metadata(s, cmd, comp, KEY, json!({"spec": spec_v, "bodies": bodies}))?;
    set_component_metadata(s, cmd, comp, DESCRIPTION, json!(format!("Pipe route {}", spec.name)))?;
    nested(s, "component.activate", json!({"component": active})).or_else(|_| nested(s, "component.activate", json!({"component": 0})))?;
    Ok(json!({
        "route": spec.name,
        "component": comp,
        "occurrence": route_occ,
        "pieces": pieces_json(&pieces, &built),
        "cut_list": cut_list(spec, &pieces),
        "welds": welds.len(),
        "free_dof": free_dof,
        "unjoined": unjoined.iter().filter_map(|&i| pieces.get(i).map(|p| p.name.clone())).collect::<Vec<_>>(),
    }))
}

/// Delete the route's sub-assembly and build `spec` in its place.
fn rebuild(s: &mut Session, cmd: &str, comp: u64, spec: &Spec) -> Result<Value> {
    rp::plan(spec).map_err(|e| bad(cmd, e))?;
    nested(s, "component.delete", json!({"component": comp}))?;
    build(s, cmd, spec)
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "route.create";
    let mut spec = spec_of(cmd, p)?;
    if let Some(f) = p.get("frame").filter(|v| !v.is_null()) {
        spec = spec.transformed(&matrix(cmd, f)?);
    }
    if route_component(s, &spec.name).is_some() {
        return Err(bad(cmd, format!("pipe route `{}` is already in the design: route.edit or route.move_point change it", spec.name)));
    }
    if s.doc.find_component(&spec.name).is_some() {
        return Err(bad(cmd, format!("a component `{}` is already in the design: name the route differently", spec.name)));
    }
    build(s, cmd, &spec)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "route.edit";
    let name = route_name(cmd, p)?;
    let (comp, spec, _) = stored_spec(s, cmd, name)?;
    let mut v = serde_json::to_value(&spec).map_err(|e| bad(cmd, e.to_string()))?;
    let Some(o) = p.as_object() else { return Err(bad(cmd, "give the route and the spec fields to change")) };
    if let Some(n) = o.get("name").and_then(Value::as_str)
        && n != spec.name
    {
        return Err(bad(cmd, "a route keeps its name (component.rename renames its sub-assembly, not the route)"));
    }
    if let Some(t) = v.as_object_mut() {
        for (k, x) in o {
            if k != "route" && k != "frame" {
                if x.is_null() {
                    t.remove(k);
                } else {
                    t.insert(k.clone(), x.clone());
                }
            }
        }
    }
    let mut next = spec_of(cmd, &v)?;
    if let Some(f) = p.get("frame").filter(|v| !v.is_null()) {
        next = next.transformed(&matrix(cmd, f)?);
    }
    rebuild(s, cmd, comp, &next)
}

fn move_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "route.move_point";
    let name = route_name(cmd, p)?;
    let (comp, spec, _) = stored_spec(s, cmd, name)?;
    let at = p.get("at").and_then(vec3).ok_or_else(|| bad(cmd, "`at` must be a route point [x, y, z] in mm"))?;
    let at = [at.x, at.y, at.z];
    let delta = match (p.get("delta").and_then(vec3), p.get("to").and_then(vec3)) {
        (Some(d), None) => [d.x, d.y, d.z],
        (None, Some(t)) => [t.x - at[0], t.y - at[1], t.z - at[2]],
        _ => return Err(bad(cmd, "give `delta`: [dx, dy, dz] or `to`: [x, y, z], one of them")),
    };
    if !(rp::norm(delta) > 1e-6) {
        return Err(bad(cmd, "the move is no distance"));
    }
    let carry = match p.get("carry") {
        None | Some(Value::Null) => true,
        Some(_) => bool_(p, "carry").ok_or_else(|| bad(cmd, "`carry` must be true or false"))?,
    };
    let (next, moved) = rp::move_point(&spec, at, delta, carry).map_err(|e| bad(cmd, e))?;
    let mut out = rebuild(s, cmd, comp, &next)?;
    if let Some(o) = out.as_object_mut() {
        o.insert("moved_points".into(), json!(moved));
    }
    Ok(out)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "route.delete";
    let name = route_name(cmd, p)?;
    let (comp, _) = stored(s, cmd, name)?;
    let r = nested(s, "component.delete", json!({"component": comp}))?;
    Ok(json!({"route": name, "deleted": r.get("deleted").cloned().unwrap_or(Value::Null)}))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "route.get";
    let name = route_name(cmd, p)?;
    let (comp, spec, rec) = stored_spec(s, cmd, name)?;
    let pieces = rp::plan(&spec).map_err(|e| bad(cmd, e))?;
    let bodies = rec.get("bodies").cloned().unwrap_or(Value::Null);
    let built: Map<String, Value> =
        bodies.as_object().map(|b| b.iter().map(|(k, v)| (k.clone(), json!({"bodies": v}))).collect()).unwrap_or_default();
    let component_name = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_else(|| spec.name.clone());
    Ok(json!({
        "route": component_name,
        "component": comp,
        "spec": rec.get("spec"),
        "points": rp::nodes(&spec).map_err(|e| bad(cmd, e))?,
        "pieces": pieces_json(&pieces, &built),
        "cut_list": cut_list(&spec, &pieces),
        "bodies": bodies,
    }))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let mut routes = Vec::new();
    for c in &s.doc.components {
        let Some(spec) = c.metadata.get(KEY).and_then(|r| r.get("spec")).and_then(|v| serde_json::from_value::<Spec>(v.clone()).ok()) else {
            continue;
        };
        let pieces = rp::plan(&spec).unwrap_or_default();
        let pipes: Vec<&Piece> = pieces.iter().filter(|p| p.kind == Kind::Pipe).collect();
        let nps = spec.all_segments().ok().and_then(|v| v.first().map(|x| x.2));
        routes.push(json!({
            "name": c.name,
            "component": c.id,
            "nps": nps,
            "schedule": schedule_word(spec.schedule),
            "elbow": spec.elbow,
            "pieces": pieces.len(),
            "pipes": pipes.len(),
            "pipe_length": (pipes.iter().filter_map(|p| p.length).sum::<f64>() * 10.0).round() / 10.0,
        }));
    }
    Ok(json!({"routes": routes}))
}

fn fittings(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(rp::library())
}
