//! Joints between occurrences (`solidcraft_doc::joints`): Joint Origin, Joint, As-built Joint,
//! Rigid Group, Drive Joints, Motion Link, limits, and Interference. Every change re-solves the
//! joint graph and moves the occurrences it places; the session re-solves after any edit too,
//! so joints follow the geometry their origins snap to.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::joints::cam::{self, Cam, CamProfile, Follower};
use solidcraft_doc::joints::{self, Coupling, Joint, JointExprs, JointKind, JointOrigin, MotionLink, NamedOrigin, Size, Snap, XRef};
use solidcraft_geom::{Aabb3, Vec3};

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, vec3};
use crate::schema::joints::EditJointResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const EDIT_CONTRACT: TypedContract<crate::schema::joints::EditJointParams> = TypedContract::new();

#[path = "joints_inspect.rs"]
mod inspect;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("joint.origin", "Joint Origin", joint_origin).at("SOLID", "CREATE").icon("joint_origin").params(
        "name?, occurrence?: id|name (default: from the pick, or the named face's component), face: [x,y,z] | \"<name>\" (centre of a planar face) | circle: [x,y,z] | \"<name>\" (centre of a round end: a point on its wall or rim, the end's centre, or the round face's name as model.faces lists it) | edge: [x,y,z] (a point on an edge of a planar face: the edge's middle, z out of the face, y into it; on?: [x,y,z] a point on the face (default: the larger planar face beside the edge); corner?: [x,y,z] near one end: the corner there, x and y along its two edges into the face; found again by name, so it follows the face as it changes size; z?: [x,y,z] the frame's z instead of the face's normal, along the edge when it runs that way, x chosen from z: an edge or corner picked as it is, kept as a point where it can't be found by name) | point: [x,y,z], z?: [x,y,z] | plane: \"XY\"|\"XZ\"|\"YZ\"|\"<name>\" | axis: \"X\"|\"Y\"|\"Z\"|\"<name>\"; a circle's z?: [x,y,z] (a round edge's centre with the way out of it: its end by name when that is the frame it shows, else a point there); body?: the named face's body; end?: \"<name>\" (a circle's end: the face across its rim there; default the end nearer the pick); x?: [x,y,z] (a direction, world) | \"<face name>\" | {face: [x,y,z]} (the frame's x: a direction, a plane's normal, or the way to another hole's axis; keeps a round origin's turn; a point's x is a direction, laid square to its z, default: chosen from z alone). Faces are kept by name: the origin follows its face, and a circle its end, when the design changes",
    ),
    CommandSpec::new("joint.create", "Joint", joint).at("SOLID", "ASSEMBLE").icon("joint").key("J").params(
        "type: rigid|revolute|slider|cylindrical|pin_slot|planar|ball, or a relation: parallel|perpendicular|angle|point_on_plane|point_on_line|line_on_plane \
         (ties only some directions or points, made where the parts are; several between two parts add up); a, b: joint origins ({occurrence, face|circle|edge|point: [x,y,z] in world, or face|circle: \"<face name>\", plane: \"XY\"|\"<name>\", axis: \"Z\"|\"<name>\"; z? for a point, end?, x? as joint.origin} | origin name); \
         values?: [angles in deg or expressions, distances]; limits?: [[min, max] | null…]; flip?: bool (default false: the origins are mated, \
         b's z against a's as two faces meet; true: b's z along a's, so two origins given at one point with the same z and x stay as they are; \
         a point origin without x takes its x from z, so give x on both, or use joint.as_built, to keep a turned part where it is); \
         offset?: mm or an expression; angle?: deg or an expression (an angle relation's tilt from mated; for other joints the default \
         is none, or a half turn where that turns b less, so faces that already meet stay as they are); name?; quiet?: bool (leave \
         out dof and fixed). An expression (text) is \
         kept with the joint and worked out again whenever the design changes, so the joint follows the parameters it names; a joint whose \
         expression no longer works out (a parameter deleted) is left out of the solve with the reason in its errors",
    ),
    CommandSpec::new("joint.as_built", "As-Built Joint", as_built).at("SOLID", "ASSEMBLE").icon("as_built").params(
        "type, a: occurrence, b: occurrence (stay where they are); at?: joint origin (default: b's origin) — the joint's frame; one on a or b is kept on its face or hole there, so the joint follows it as the design changes; \
         name?; limits?: [[min, max] | null…]; quiet?: bool",
    ),
    CommandSpec::new("joint.rigid_group", "Rigid Group", rigid_group).at("SOLID", "ASSEMBLE").icon("rigid_group").params("occurrences: [ids or names] (move together, as they are)"),
    CommandSpec::new("joint.drive", "Drive Joints", drive).at("SOLID", "ASSEMBLE").icon("drive").params(
        "joint: id|name; value | values: [angles in deg or expressions, distances] (values not given keep theirs; a value driven drops the expression joint.create gave it; clamped to the limits; the other joints of a \
         closed loop follow, and the motion stops where the loop can no longer close → stopped_by_loop); quiet?: bool (leave out dof \
         and fixed) → values: [{name, value, unit: deg|mm, limits}] as joint.list gives them; requested: the values asked for, in the same form; \
         held: [{name, requested, value, unit, why}] for each driven value that ended up elsewhere (a motion link sets it, as a screw's \
         travel follows its turn; a limit; contact; a closed loop); moved: whether any occurrence moved; \
         reason: why the joint can move nothing (suppressed: nothing is stored; its origins lost; its chain joined to nothing grounded → unconnected: \
         the occurrence named; or, when nothing moved, the first held value's why); index?: zero-based coordinate to drive with value (the other coordinates follow); continuous?: bool (drag frame, suppress UI echo)",
    ),
    CommandSpec::new("joint.reach", "Reach Point", reach).icon("reach").params(
        "occurrence: id|name; point: [x,y,z] (where its point goes, world); at?: [x,y,z] (its point, in the world where it is now; default: the centre of          its bodies' box); z?: [x,y,z] (the way its axis then points); axis?: [x,y,z] (that axis, in the world now; default: its own z); joints?: [ids          or names] (only these move toward the point; the other joints of a closed loop they are in follow); quiet?: bool; continuous?: bool (a frame          of a drag: the answer says continuous, and the app's command line doesn't echo it). Moves the values of the joints          on its chain from where they are to the nearest pose, within their limits, through motion links, keeping closed loops closed as joint.drive does          (contact sets are not checked) → reached; error_mm (and error_deg with z): how close it got; point: where its point is; values: [{joint, name,          values}] of the joints that moved, as joint.list gives them; moved; stopped_by_limit: [{joint, value, limit: min|max}]: the limits it rests on          short of the point; stopped_by_loop: {joint}: the closed loop that stopped it (its chain alone gets closer); reason: why nothing can move",
    ),
    CommandSpec::new("joint.motion_link", "Motion Link", motion_link)
        .at("SOLID", "ASSEMBLE")
        .icon("motion_link")
        .params("a, b: joints; ratio (b = ratio · a + offset): b's units per a's (mm per deg for an angle driving a distance); offset?: in b's units (deg for an angle, mm for a distance, or an expression); ia?, ib?: value indices (default 0)"),
    CommandSpec::new("joint.gear", "Gear", gear).at("SOLID", "ASSEMBLE").icon("gear").params(
        "a, b: joints that turn (revolute), b driven by a; ra, rb: the gears' pitch radii (or tooth counts): mm, an expression, or \
         {circle: [x,y,z], occurrence?}: the round face there, its radius (read again whenever the design changes, so the ratio follows \
         edits); internal?: bool (one is a ring gear: both turn the same way; by default b turns the other way); reverse?: bool; \
         link?: index (replace that motion link) → link, ratio (b's degrees per a's); b keeps where it is, also when the link replaces \
         another (at the current pose)",
    ),
    CommandSpec::new("joint.rack_pinion", "Rack and Pinion", rack_pinion).at("SOLID", "ASSEMBLE").icon("rack_pinion").params(
        "a, b: a joint that turns (revolute: the pinion) and one that slides (slider: the rack), in either order, b driven by a; \
         radius: the pinion's pitch radius (mm, an expression, or {circle: [x,y,z], occurrence?}): the rack travels radius × the turn in \
         radians; reverse?: bool; link?: index (replace that motion link) → link, ratio (mm per degree, or degrees per mm); b keeps \
         where it is, also when the link replaces another (at the current pose)",
    ),
    CommandSpec::new("joint.screw", "Screw", screw).at("SOLID", "ASSEMBLE").icon("screw").params(
        "joint: a cylindrical joint (its turn drives its travel) | a, b: a joint that turns and one that slides, b driven by a; pitch: \
         travel per turn (mm or an expression); reverse?: bool (a left-hand thread); link?: index (replace that motion link) → link, \
         ratio (mm per degree, or degrees per mm); b keeps where it is, also when the link replaces another (at the current \
         pose)",
    ),
    CommandSpec::new("joint.cam", "Cam", cam_cmd).at("SOLID", "ASSEMBLE").icon("cam").params(
        "a: the cam's joint (revolute); b: the follower's joint, a slider (a translating follower) or a revolute (an oscillating one),          b driven by a; profile: {sketch: id|name, index?: the profile (default: the largest)} | {edges: [[x,y,z]…] (points on edges of          the cam's bodies forming one closed loop, world), occurrence?}: the cam's outline, in a plane square to its axis, read again          whenever the design changes; follower?: knife|roller|flat (default knife; flat: a face square to a slider's line); radius?: the          roller's radius (mm, an expression or {circle: [x,y,z], occurrence?}: the roller's round face, which is its tip too); tip?: a          joint origin where the follower touches: its knife's point or its roller's centre, such as {circle: [x,y,z]} on the roller          (needed for an oscillating follower; a translating one's line runs through it, by default along its slider's axis); flip?: bool (ride the cam from the other side          than the one the follower is on now); link?: index (replace that motion link) → link, lift: {min, max, rise, unit} (b's travel          over a turn of the cam), reverse (the side it rides from), follower_offset (mm: how far a translating follower's line passes          from the cam's axis); b keeps where it is, also when the link replaces another (at the current pose)",
    ),
    CommandSpec::new("joint.limits", "Joint Limits", limits).icon("joint").params("joint; index?: value index (default 0); min?, max? (omit both to clear)"),
    CommandSpec::new("joint.edit", "Edit Joint", crate::checked!("joint.edit", edit))
        .icon("edit")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditJointResult>)
        .params(
            "joint; a?, b?: joint origins as joint.create takes them (a side left out keeps its place; the two must stay on different occurrences); \
             type?, flip?, offset?, angle? (a number, or an expression the joint then follows), suppressed?, name?; the joint re-solves, keeping \
             its limits and expressions (a relation's values start again where its parts are)",
        ),
    CommandSpec::new("joint.delete", "Delete Joint", delete).icon("delete").params("joint: id|name (or origin: name, link: index)"),
    CommandSpec::new("joint.list", "List Joints", list).icon("list").noundo().params(
        "→ joints (type, occurrences, values, limits; expressions: {offset?, angle?, values?} when they were typed as expressions), origins, links, degrees of freedom, conflicts, floating: [{occurrences, joints}] \
         (joint chains joined to nothing grounded: the solver places none of them, so driving them moves nothing)",
    ),
    CommandSpec::new("joint.inspect", "Inspect Joint", inspect::inspect).icon("inspect").noundo().params(
        "joint?: id|name (default: every joint) → joints: [{id, name, type, suppressed, visible, flip, offset (mm), angle (deg), values: as joint.list \
         gives them, dofs, expressions?: {offset?, angle?, values?}, a, b: {occurrence, occurrence_name, component, component_name, path \
         (occurrence names from the top, `/` between), snap (as saved: type, pick, face, end, edge, corner, x, point, z, in the component's \
         coordinates), references: [{role: face|end|x|edge|corner, name, role_in_feature?: (top, side:c1…), feature?: {id, name, type} (the feature that made the face; \
         error when it is gone), sketch?: {id, name, curve, curve_type} (a face swept from a sketch curve), faces?: [an edge's two faces], \
         unverified?: a name saved before exact names}], resolves, frame?: {origin, z, x} in the world, error?: why the origin is lost}, \
         links: [{index, role: driver|driven, …as joint.list}], errors, conflicts}] (reads only: nothing changes)",
    ),
    CommandSpec::new("joint.solve", "Solve Joints", solve_cmd).icon("compute").params("re-place the occurrences from the joints → conflicts, errors, dof"),
    CommandSpec::new("inspect.interference", "Interference", super::interference::interference).at("SOLID", "INSPECT").icon("interference").noundo().params(
        "bodies?: [names] (default: all, in world placement; a name that is no body is refused); budget_s?: seconds (default 240, at most 86400: pairs not finished by then are listed in unchecked as \"not checked: the time budget … ran out\" and the call returns) → interferences: pairs that overlap with the overlap volume, area, bounding box and centre (a pin a hair off the axis of a hole of its size also has near_coaxial: {offset_mm, length_mm}, its volume the crescent the walls share); unchecked: [{a, b, reason}], the pairs that could not be checked, with the reason, and each mesh body (STL, 3MF or OBJ import: not intersected) as {a: name, b: null, reason: \"mesh body: not checked\"}, so count: 0 is not read as no overlap (checked counts the solid bodies). Bodies are pruned by bounding box first, then by their display meshes (pairs whose meshes stay apart by more than their chord tolerances, neither inside the other, are clear without a boolean), so any number of bodies is fine; the pairs left are intersected exactly, in parallel on desktop, the slowest first; at most 20000 pairs whose boxes overlap are intersected per call (truncated: true and skipped_pairs: n say when more were left out). Also: pairs_candidate (boxes overlap), pairs_cleared (by the meshes), pairs_intersected, pairs_not_checked, budget_exceeded, threads (worker threads; 0: on the calling thread), boolean_ms and tessellate_ms (kernel time summed over the pairs), slowest: [{a, b, ms, boolean_ms, tessellate_ms}] and ms",
    ),
];

/// Re-solve the joints and move the occurrences they place (after any edit). Gears, racks and
/// screws first take their ratios from the radii and pitches they are defined by.
pub fn resolve(s: &mut Session) -> joints::Solution {
    reexpress(s);
    recouple(s);
    resolve_holding(s, &BTreeMap::new())
}

/// Update placements after an edit without computing a DOF report that the caller discards.
/// Expressions and geometry-dependent couplings are refreshed just as in [`resolve`].
pub fn place(s: &mut Session) -> joints::Solution {
    reexpress(s);
    recouple(s);
    place_holding(s, &BTreeMap::new())
}

/// Work out the offsets, angles and values joints were given as expressions again (a parameter
/// may have changed). One that can't be worked out (its parameter gone) keeps its numbers; the
/// solve leaves that joint out and says why.
fn reexpress(s: &mut Session) {
    let params = joints::Params::new(&s.doc);
    let changed: Vec<(usize, (f64, f64, Vec<f64>))> = s
        .doc
        .assembly
        .joints
        .iter()
        .enumerate()
        .filter(|(_, j)| !j.exprs.is_empty())
        .filter_map(|(i, j)| {
            let (o, a, mut q) = joints::expressed(&params, j).ok()?;
            joints::clamp(j, &mut q);
            ((o, a) != (j.offset, j.angle) || q != j.values).then_some((i, (o, a, q)))
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    let doc = s.doc_mut();
    for (i, (o, a, q)) in changed {
        if let Some(j) = doc.assembly.joints.get_mut(i) {
            (j.offset, j.angle, j.values) = (o, a, q);
        }
    }
}

/// Work out the ratios (and cam tables) of the coupled motion links again (a radius, a parameter
/// or a cam's profile may have changed). A coupling that can't be worked out (its face gone)
/// keeps its last ratio and table; `joint.list` says why.
fn recouple(s: &mut Session) {
    if s.doc.assembly.links.iter().all(|l| l.coupling.is_none()) {
        return;
    }
    let st = s.model.state();
    let changed: Vec<(usize, (f64, Vec<f64>))> = s
        .doc
        .assembly
        .links
        .iter()
        .enumerate()
        .filter_map(|(i, l)| joints::coupling_values(&s.doc, &st, l)?.ok().filter(|(r, t)| *r != l.ratio || *t != l.lift).map(|v| (i, v)))
        .collect();
    if changed.is_empty() {
        return;
    }
    let doc = s.doc_mut();
    for (i, (r, t)) in changed {
        if let Some(l) = doc.assembly.links.get_mut(i) {
            l.ratio = r;
            l.lift = t;
        }
    }
}

/// Re-solve the joints, keeping the `held` values (the ones being driven, mapped to the values
/// they move from) where loops are closed.
pub fn resolve_holding(s: &mut Session, held: &joints::Held) -> joints::Solution {
    resolve_with_analysis(s, held, joints::Analysis::DegreesOfFreedom)
}

/// Contact and loop-search trials need placements and conflicts, not a DOF report.
pub(super) fn place_holding(s: &mut Session, held: &joints::Held) -> joints::Solution {
    resolve_with_analysis(s, held, joints::Analysis::Placement)
}

fn solution(s: &Session, held: &joints::Held, analysis: joints::Analysis) -> joints::Solution {
    #[cfg(test)]
    solve_counts::record(analysis);
    joints::solve_holding_with_analysis(&s.doc, &s.model.state(), held, analysis)
}

fn resolve_with_analysis(s: &mut Session, held: &joints::Held, analysis: joints::Analysis) -> joints::Solution {
    let sol = solution(s, held, analysis);
    if !sol.world.is_empty() {
        let mut next = (*s.doc).clone();
        if next.apply_joint_solution(&sol) {
            *s.doc_mut() = next;
            s.revision += 1;
        }
    }
    sol
}

/// A quiet drive/reach does not publish DOF, so it needs only placement and constraint checks.
fn response_analysis(p: &Value) -> joints::Analysis {
    if bool_(p, "quiet").unwrap_or(false) { joints::Analysis::Placement } else { joints::Analysis::DegreesOfFreedom }
}

/// A solve's answer; `quiet: true` in the command's params leaves out the per-occurrence lists.
fn sol_json(sol: &joints::Solution, p: &Value) -> Value {
    let mut out = json!({
        "conflicts": sol.conflicts.iter().map(|(id, n, w)| json!({"joint": id, "name": n, "why": w})).collect::<Vec<_>>(),
        "errors": sol.errors.iter().map(|(id, n, w)| json!({"joint": id, "name": n, "why": w})).collect::<Vec<_>>(),
    });
    if !bool_(p, "quiet").unwrap_or(false) {
        out["dof"] = sol.dof.iter().map(|(o, d)| json!({"occurrence": o, "dof": d})).collect();
        out["fixed"] = json!(sol.fixed);
    }
    out
}

/// A joint's values as `joint.list` gives them: degrees for angles, mm for distances, each with
/// its name and limits.
pub(crate) fn named_values(j: &Joint, q: &[f64]) -> Vec<Value> {
    j.kind
        .dofs()
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let (v, angle) = (q.get(i).copied().unwrap_or(0.0), j.kind.is_angle(i));
            json!({"name": n, "value": if angle { v.to_degrees() } else { v }, "unit": if angle { "deg" } else { "mm" },
                "limits": j.limits.get(i).copied().flatten().map(|(a, b)| if angle { [a.to_degrees(), b.to_degrees()] } else { [a, b] })})
        })
        .collect()
}

/// A joint's values as the solver places it (after motion links, so a screw's travel follows its
/// turn), named as `joint.list` gives them.
pub(crate) fn effective_values(s: &Session, id: u64) -> Option<Vec<Value>> {
    let j = s.doc.assembly.joints.iter().find(|j| j.id == id)?;
    let vals = joints::joint_values(&s.doc.assembly);
    Some(named_values(j, vals.get(&id).map(Vec::as_slice).unwrap_or(&j.values)))
}

fn occurrence_name(s: &Session, id: u64) -> String {
    s.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.clone()).unwrap_or_else(|| id.to_string())
}

/// Why driving joint `id` moves nothing whatever its values (`None`: it places what it joins),
/// with the occurrence that is joined to nothing grounded when that is the reason.
pub(crate) fn cannot_move(s: &Session, sol: &joints::Solution, id: u64) -> Option<(String, Option<String>)> {
    let j = s.doc.assembly.joints.iter().find(|j| j.id == id)?;
    // Its value carries through motion links to joints that may move (a suppressed dial too).
    if s.doc.assembly.links.iter().any(|l| l.a == id || l.b == id) {
        return None;
    }
    if j.suppressed {
        return Some((format!("{} is suppressed", j.name), None));
    }
    if let Some((_, _, why)) = sol.errors.iter().find(|e| e.0 == id) {
        return Some((why.clone(), None));
    }
    // The solver places what it reaches from the root and grounded occurrences.
    let placed = |o: u64| o == 0 || sol.world.contains_key(&o);
    if placed(j.a.occurrence) || placed(j.b.occurrence) {
        return None;
    }
    let name = occurrence_name(s, j.a.occurrence);
    Some((format!("{name} is not connected to anything grounded (ground it, or join it to a grounded part)"), Some(name)))
}

/// Joint chains joined to nothing grounded, which the solver places none of: each chain's
/// occurrences and joints, by name.
fn floating_chains(s: &Session, sol: &joints::Solution) -> Vec<Value> {
    let placed = |o: u64| o == 0 || sol.world.contains_key(&o);
    let errors: BTreeSet<u64> = sol.errors.iter().map(|e| e.0).collect();
    let mut next: BTreeMap<u64, Vec<(u64, &Joint)>> = BTreeMap::new();
    for j in s.doc.assembly.joints.iter().filter(|j| !j.suppressed && !errors.contains(&j.id)) {
        if !placed(j.a.occurrence) && !placed(j.b.occurrence) {
            next.entry(j.a.occurrence).or_default().push((j.b.occurrence, j));
            next.entry(j.b.occurrence).or_default().push((j.a.occurrence, j));
        }
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for &start in next.keys() {
        if !seen.insert(start) {
            continue;
        }
        let (mut occs, mut js, mut todo) = (vec![start], BTreeSet::new(), vec![start]);
        while let Some(o) = todo.pop() {
            for (other, j) in next.get(&o).into_iter().flatten() {
                js.insert((j.id, j.name.as_str()));
                if seen.insert(*other) {
                    occs.push(*other);
                    todo.push(*other);
                }
            }
        }
        out.push(json!({"occurrences": occs.iter().map(|o| occurrence_name(s, *o)).collect::<Vec<_>>(),
            "joints": js.iter().map(|(_, n)| *n).collect::<Vec<_>>()}));
    }
    out
}

pub(crate) fn occurrence_id(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    let key = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "an occurrence must be an id or a name")),
    };
    if key == "0" || key.eq_ignore_ascii_case("root") {
        return Ok(0);
    }
    if let Some(o) = s.doc.occurrences.iter().find(|o| o.id.to_string() == key || o.name == key) {
        return Ok(o.id);
    }
    // A component name: its first occurrence.
    s.doc.find_component(&key).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).ok_or_else(|| bad(cmd, format!("no occurrence `{key}`")))
}

/// The occurrence whose bodies are nearest a world point (0 = root bodies).
pub(crate) fn occurrence_at(s: &Session, p: Vec3) -> Option<u64> {
    let st = s.model.state();
    let mut best: Option<(f64, u64)> = None;
    for b in &st.bodies {
        let comp = s.doc.model_body_component(b);
        let occs: Vec<u64> = if comp == 0 { vec![0] } else { s.doc.occurrences.iter().filter(|o| o.component == comp).map(|o| o.id).collect() };
        for occ in occs {
            let t = if occ == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occ) };
            let inv = solidcraft_doc::mat_inverse(&t)?;
            let q = solidcraft_doc::apply_point(&inv, p);
            let bb = b.mesh().bounds();
            let c = Vec3::new(q.x.clamp(bb.min.x, bb.max.x), q.y.clamp(bb.min.y, bb.max.y), q.z.clamp(bb.min.z, bb.max.z));
            let d = c.dist(q);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, occ));
            }
        }
    }
    best.map(|(_, o)| o)
}

/// Resolve an origin from a name: a named joint origin, an origin plane ("XY", "XZ", "YZ"),
/// an origin axis ("X", "Y", "Z"), or a construction plane/axis from `st.construct`.
fn origin_from_name(s: &Session, name: &str, occurrence: Option<u64>, cmd: &str) -> Result<Option<JointOrigin>> {
    let name_trim = name.trim();
    let upper = name_trim.to_ascii_uppercase();
    let st = s.model.state();
    let occ = occurrence.unwrap_or(0);
    if matches!(upper.as_str(), "XY" | "XZ" | "YZ") {
        let snap = Snap::Plane { plane: Box::new(solidcraft_doc::PlaneRef::Origin { name: upper }), pick: None, x: None };
        return settle(s, JointOrigin { occurrence: occ, snap }, &json!({}), cmd).map(Some);
    }
    if matches!(upper.as_str(), "X" | "Y" | "Z") {
        let snap = Snap::Axis { axis: Box::new(solidcraft_doc::joints::AxisRef::origin(upper)), pick: None, x: None };
        return settle(s, JointOrigin { occurrence: occ, snap }, &json!({}), cmd).map(Some);
    }
    if let Some(c) = st.construct.iter().find(|c| c.name == name_trim) {
        let occ = occurrence.unwrap_or_else(|| {
            let comp = s.doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
            if comp == 0 { 0 } else { s.doc.occurrence_of(comp).map(|o| o.id).unwrap_or(0) }
        });
        let snap = match &c.geom {
            solidcraft_doc::construct::ConstructGeom::Plane(_) => {
                Snap::Plane { plane: Box::new(solidcraft_doc::PlaneRef::Origin { name: name_trim.to_string() }), pick: None, x: None }
            }
            solidcraft_doc::construct::ConstructGeom::Axis { .. } => {
                Snap::Axis { axis: Box::new(solidcraft_doc::joints::AxisRef::construction(name_trim.to_string())), pick: None, x: None }
            }
            _ => return Ok(None),
        };
        return settle(s, JointOrigin { occurrence: occ, snap }, &json!({}), cmd).map(Some);
    }
    Ok(None)
}

/// A joint origin on a plane: origin plane ("XY", "XZ", "YZ"), construction plane by name, or a
/// plane definition object. Optional `pick: [x,y,z]` sets the origin along the plane; `x` sets the turn.
fn plane_origin(s: &Session, v: &Value, cmd: &str) -> Result<JointOrigin> {
    let p_val = v.get("plane").ok_or_else(|| bad(cmd, "`plane` parameter required"))?;
    let world_pick = ["pick", "at", "point"].iter().find_map(|k| v.get(*k).and_then(vec3));
    let occurrence = match v.get("occurrence") {
        Some(o) => occurrence_id(s, Some(o), cmd)?,
        None => match p_val.as_str() {
            Some(name) => {
                let upper = name.trim().to_ascii_uppercase();
                if matches!(upper.as_str(), "XY" | "XZ" | "YZ") {
                    0
                } else {
                    let st = s.model.state();
                    st.construct
                        .iter()
                        .find(|c| c.name == name.trim())
                        .and_then(|c| {
                            let comp = s.doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
                            if comp == 0 { Some(0) } else { s.doc.occurrence_of(comp).map(|o| o.id) }
                        })
                        .unwrap_or(0)
                }
            }
            None => match world_pick {
                Some(w) => occurrence_at(s, w).unwrap_or(0),
                None => 0,
            },
        },
    };
    let t = if occurrence == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occurrence) };
    let inv = solidcraft_doc::mat_inverse(&t).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let local = |p: Vec3| solidcraft_doc::apply_point(&inv, p);
    let component = component_of(s, occurrence);

    let plane_ref = match p_val {
        Value::String(name) => {
            let upper = name.trim().to_ascii_uppercase();
            if matches!(upper.as_str(), "XY" | "XZ" | "YZ") {
                solidcraft_doc::PlaneRef::Origin { name: upper }
            } else {
                solidcraft_doc::PlaneRef::Origin { name: name.trim().to_string() }
            }
        }
        Value::Object(_) => serde_json::from_value(p_val.clone()).map_err(|e| bad(cmd, format!("invalid plane definition: {e}")))?,
        _ => return Err(bad(cmd, "`plane` must be a plane name (e.g. \"XY\", \"Plane1\") or a plane definition object")),
    };

    let pick = world_pick.map(local);
    let x = x_param(s, v, component, &inv, cmd)?;
    let snap = Snap::Plane { plane: Box::new(plane_ref), pick, x };
    settle(s, JointOrigin { occurrence, snap }, v, cmd)
}

/// A joint origin on an axis: origin axis ("X", "Y", "Z"), construction axis by name, or an axis
/// definition object. Optional `pick: [x,y,z]` sets the origin along the line; `x` sets the turn.
fn axis_origin(s: &Session, v: &Value, cmd: &str) -> Result<JointOrigin> {
    let a_val = v.get("axis").ok_or_else(|| bad(cmd, "`axis` parameter required"))?;
    let world_pick = ["pick", "at", "point"].iter().find_map(|k| v.get(*k).and_then(vec3));
    let occurrence = match v.get("occurrence") {
        Some(o) => occurrence_id(s, Some(o), cmd)?,
        None => match a_val.as_str() {
            Some(name) => {
                let upper = name.trim().to_ascii_uppercase();
                if matches!(upper.as_str(), "X" | "Y" | "Z") {
                    0
                } else {
                    let st = s.model.state();
                    st.construct
                        .iter()
                        .find(|c| c.name == name.trim())
                        .and_then(|c| {
                            let comp = s.doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
                            if comp == 0 { Some(0) } else { s.doc.occurrence_of(comp).map(|o| o.id) }
                        })
                        .unwrap_or(0)
                }
            }
            None => match world_pick {
                Some(w) => occurrence_at(s, w).unwrap_or(0),
                None => 0,
            },
        },
    };
    let t = if occurrence == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occurrence) };
    let inv = solidcraft_doc::mat_inverse(&t).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let local = |p: Vec3| solidcraft_doc::apply_point(&inv, p);
    let component = component_of(s, occurrence);

    let axis_ref = match a_val {
        Value::String(name) => {
            let upper = name.trim().to_ascii_uppercase();
            if matches!(upper.as_str(), "X" | "Y" | "Z") {
                solidcraft_doc::joints::AxisRef::origin(upper)
            } else {
                solidcraft_doc::joints::AxisRef::construction(name.trim().to_string())
            }
        }
        Value::Object(_) => serde_json::from_value(a_val.clone()).map_err(|e| bad(cmd, format!("invalid axis definition: {e}")))?,
        _ => return Err(bad(cmd, "`axis` must be an axis name (e.g. \"Z\", \"Axis1\") or an axis definition object")),
    };

    let pick = world_pick.map(local);
    let x = x_param(s, v, component, &inv, cmd)?;
    let snap = Snap::Axis { axis: Box::new(axis_ref), pick, x };
    settle(s, JointOrigin { occurrence, snap }, v, cmd)
}

/// A joint origin from parameters: a named origin, or a snap given in world coordinates.
pub(crate) fn origin_param(s: &Session, v: Option<&Value>, cmd: &str) -> Result<JointOrigin> {
    let v = v.ok_or_else(|| bad(cmd, "joint origins `a` and `b` are required"))?;
    if let Value::String(name) = v {
        if let Some(o) = s.doc.assembly.origins.iter().find(|o| &o.name == name) {
            return Ok(o.origin.clone());
        }
        if let Some(o) = origin_from_name(s, name, None, cmd)? {
            return Ok(o);
        }
        return Err(bad(cmd, format!("no joint origin `{name}`")));
    }
    // A plane or axis origin given in object form.
    if v.get("plane").is_some() {
        return plane_origin(s, v, cmd);
    }
    if v.get("axis").is_some() && v.get("edge").is_none() && v.get("point").is_none() {
        return axis_origin(s, v, cmd);
    }
    // A face given by its name, as `model.faces` lists it.
    if let Some((what, name)) = ["face", "circle"].iter().find_map(|k| v.get(*k).and_then(Value::as_str).map(|n| (*k, n))) {
        return origin_by_name(s, v, what, name, cmd);
    }
    let pick = ["face", "circle", "edge", "point"].iter().find_map(|k| v.get(*k).and_then(vec3).map(|p| (*k, p)));
    let (what, world) = match (pick, v.get("frame")) {
        (Some(x), _) => x,
        (None, Some(f)) => ("frame", f.get("origin").and_then(vec3).ok_or_else(|| bad(cmd, "`frame` needs origin and z"))?),
        _ => return Err(bad(cmd, "a joint origin needs face, circle, edge, point, plane, axis or frame [x, y, z], or a face's name")),
    };
    let occurrence = match v.get("occurrence") {
        Some(o) => occurrence_id(s, Some(o), cmd)?,
        None => occurrence_at(s, world).ok_or_else(|| bad(cmd, "no body there"))?,
    };
    // World → the component's own coordinates.
    let t = if occurrence == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occurrence) };
    let inv = solidcraft_doc::mat_inverse(&t).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let local = |p: Vec3| solidcraft_doc::apply_point(&inv, p);
    let local_dir = |d: Vec3| solidcraft_doc::apply_vector(&inv, d);
    let component = component_of(s, occurrence);
    let st = s.model.state();
    let snap = match what {
        "face" | "circle" => {
            let pick = local(world);
            // The face's name, which it is found by from then on (a circle's is its wall's).
            let face = joints::face_name_at(&s.doc, &st, component, pick, what == "circle");
            let x = x_param(s, v, component, &inv, cmd)?;
            if what == "face" { Snap::FaceCenter { pick, face, x } } else { Snap::CircleCenter { pick, face, end: None, x } }
        }
        "edge" => {
            let (on, corner) = (v.get("on").and_then(vec3).map(local), v.get("corner").and_then(vec3).map(local));
            let snap = joints::edge_snap(&s.doc, &st, component, local(world), on, corner);
            match given_z(v, &local_dir, cmd)? {
                None => snap.map_err(|e| bad(cmd, e.to_string()))?,
                // An edge or a corner picked as it is (z given): it follows the edge by name with
                // that z, or, where it can't be (no planar face beside it, no names), stays a
                // point there as before.
                Some(z) => {
                    let at = corner.unwrap_or(local(world));
                    let point = Snap::Point { point: at, z: Some(z), x: None };
                    let followed = snap.ok().map(|snap| joints::edge_axis(&s.doc, &st, component, snap, z));
                    let o = followed.map(|snap| JointOrigin { occurrence, snap }).filter(|o| same_frame(s, o, at, z));
                    return Ok(o.unwrap_or(JointOrigin { occurrence, snap: point }));
                }
            }
        }
        "point" => {
            let z = v.get("z").and_then(vec3).map(local_dir);
            let x = match v.get("x") {
                None | Some(Value::Null) => None,
                Some(x) => Some(vec3(x).map(local_dir).ok_or_else(|| bad(cmd, "a point origin's `x` is a direction [x, y, z]"))?),
            };
            // An x along z (or no direction at all) would leave the frame's turn to chance.
            if let Some(x) = x
                && (!x.is_finite() || x.cross(z.unwrap_or(Vec3::Z)).len() <= 1e-9 * x.len().max(1.0))
            {
                return Err(bad(cmd, "a point origin's `x` must be a direction off its z axis"));
            }
            Snap::Point { point: local(world), z, x }
        }
        _ => {
            let f = v.get("frame").cloned().unwrap_or_default();
            let z = f.get("z").and_then(vec3).ok_or_else(|| bad(cmd, "`frame` needs a z axis"))?;
            Snap::Frame { origin: local(world), z: local_dir(z), x: f.get("x").and_then(vec3).map(local_dir) }
        }
    };
    let o = JointOrigin { occurrence, snap };
    // A round edge's centre with the z it was picked with: its end, by name, where that is the
    // frame it shows; else a point there as before.
    if what == "circle"
        && let Some(z) = given_z(v, &local_dir, cmd)?
    {
        let at = local(world);
        // The turn the point gave, kept.
        let point = Snap::Point { point: at, z: Some(z), x: None };
        let px =
            joints::resolve_origin(&s.doc, &st, &JointOrigin { occurrence, snap: point.clone() }).ok().map(|m| Vec3::new(m[0][0], m[0][1], m[0][2]));
        let turn = |snap: Snap| match (snap, px) {
            (Snap::CircleCenter { pick, face, end, x: None }, Some(x)) => Snap::CircleCenter { pick, face, end, x: Some(XRef::Dir(x)) },
            (snap, _) => snap,
        };
        let by_wall = settle(s, JointOrigin { occurrence, snap: turn(o.snap.clone()) }, v, cmd).ok();
        let by_centre = || {
            let snap = joints::round_end_at(&s.doc, &st, component, at)?;
            settle(s, JointOrigin { occurrence, snap: turn(snap) }, v, cmd).ok()
        };
        let found = by_wall.filter(|o| same_frame(s, o, at, z)).or_else(|| by_centre().filter(|o| same_frame(s, o, at, z)));
        return Ok(found.unwrap_or(JointOrigin { occurrence, snap: point }));
    }
    match settle(s, o, v, cmd) {
        Ok(o) => Ok(o),
        // A circle given by its centre, where no face lies (#393): that end of the cylinder, by
        // its face's name and its end's (a face without a name: the point and axis there).
        Err(e) if what == "circle" => {
            let snap = match joints::round_end_at(&s.doc, &st, component, local(world)) {
                Some(Snap::CircleCenter { pick, face, end, .. }) => Snap::CircleCenter { pick, face, end, x: x_param(s, v, component, &inv, cmd)? },
                _ => circle_centre(s, occurrence, local(world)).ok_or(e)?,
            };
            settle(s, JointOrigin { occurrence, snap }, v, cmd)
        }
        Err(e) => Err(e),
    }
}

/// An origin's `z` (world), in its component's coordinates.
fn given_z(v: &Value, local_dir: &dyn Fn(Vec3) -> Vec3, cmd: &str) -> Result<Option<Vec3>> {
    match v.get("z") {
        None | Some(Value::Null) => Ok(None),
        Some(z) => {
            vec3(z).filter(|z| z.is_finite() && z.len() > 1e-9).map(|z| Some(local_dir(z))).ok_or_else(|| bad(cmd, "`z` is a direction [x, y, z]"))
        }
    }
}

/// Whether origin `o` resolves, now, to the frame at `at` with z along `z` (component coordinates)
/// that its pick showed.
fn same_frame(s: &Session, o: &JointOrigin, at: Vec3, z: Vec3) -> bool {
    let Ok(m) = joints::resolve_origin(&s.doc, &s.model.state(), o) else { return false };
    let (o, oz) = (Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[2][0], m[2][1], m[2][2]));
    o.dist(at) <= 1e-6 * at.len().max(100.0) && z.normalized().is_some_and(|z| oz.dot(z) > 1.0 - 1e-9)
}

/// The longest face name a joint origin takes. A face that cuts split and merged keeps the names
/// of the faces around it (`<name>#[<name>,…]`): several hundred characters on a part a text or
/// a pattern of holes was cut through.
const MAX_FACE_NAME: usize = 16_384;

/// The component an occurrence places (0 for the root, or an unknown occurrence).
fn component_of(s: &Session, occurrence: u64) -> u64 {
    if occurrence == 0 { 0 } else { s.doc.occurrences.iter().find(|o| o.id == occurrence).map_or(0, |o| o.component) }
}

/// A joint origin on a face given by its name (`face: "<name>"`: its centre; `circle: "<name>"`:
/// a round end of it), on `body?`, of `occurrence?` (default: the first occurrence of the
/// face's component).
fn origin_by_name(s: &Session, v: &Value, what: &str, name: &str, cmd: &str) -> Result<JointOrigin> {
    if name.is_empty() || name.len() > MAX_FACE_NAME {
        return Err(bad(cmd, format!("a face name is 1…{MAX_FACE_NAME} characters")));
    }
    let st = s.model.state();
    let (component, pick) = joints::named_face_spot(&s.doc, &st, str_(v, "body"), name).map_err(|why| {
        if s.doc.assembly.origins.iter().any(|o| o.name == name) {
            bad(cmd, format!("`{name}` is a joint origin: pass it as the whole origin, not as a `{what}` name"))
        } else {
            bad(cmd, why)
        }
    })?;
    let occurrence = match v.get("occurrence") {
        Some(o) => occurrence_id(s, Some(o), cmd)?,
        None if component == 0 => 0,
        None => {
            s.doc.occurrence_of(component).map(|o| o.id).ok_or_else(|| bad(cmd, format!("face `{name}` is on a component with no occurrence")))?
        }
    };
    if component_of(s, occurrence) != component {
        return Err(bad(cmd, format!("face `{name}` is not on that occurrence's component")));
    }
    let t = if occurrence == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occurrence) };
    let inv = solidcraft_doc::mat_inverse(&t).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let x = x_param(s, v, component, &inv, cmd)?;
    let face = Some(name.to_string());
    let snap = if what == "face" { Snap::FaceCenter { pick, face, x } } else { Snap::CircleCenter { pick, face, end: None, x } };
    settle(s, JointOrigin { occurrence, snap }, v, cmd)
}

/// A new joint origin checked: `end?` (a circle's end, by the name of the face across its rim
/// there) set, and the end it takes named so it keeps it; refused when it doesn't resolve.
fn settle(s: &Session, mut o: JointOrigin, v: &Value, cmd: &str) -> Result<JointOrigin> {
    let st = s.model.state();
    let component = component_of(s, o.occurrence);
    if let Some(want) = v.get("end") {
        let want = want
            .as_str()
            .filter(|n| !n.is_empty() && n.len() <= MAX_FACE_NAME)
            .ok_or_else(|| bad(cmd, "`end` is the name of a face a round end lies on"))?;
        let Snap::CircleCenter { end, .. } = &mut o.snap else { return Err(bad(cmd, "`end` is for a round origin (`circle`)")) };
        *end = Some(want.to_string());
        let names = joints::end_faces(&s.doc, &st, component, &o.snap);
        if !names.iter().any(|n| n == want) {
            return Err(bad(cmd, format!("the round face doesn't end on face `{want}` (its ends lie on: {})", names.join(", "))));
        }
    }
    let m = joints::resolve_origin(&s.doc, &st, &o).map_err(|e| bad(cmd, e.to_string()))?;
    joints::pin_end(&s.doc, &st, component, &mut o.snap);
    // A face or circle given no `x` keeps the x it has now, so its frame doesn't turn as the face
    // tilts.
    o.snap = o.snap.with_frame_x(Vec3::new(m[0][0], m[0][1], m[0][2]));
    Ok(o)
}

/// A joint origin's x reference (its turn about z): `x: [x, y, z]` (a direction, world), `x:
/// "<face name>"`, or `x: {face: [x, y, z]}` (the face of the origin's component there, world).
fn x_param(s: &Session, v: &Value, component: u64, inv: &solidcraft_doc::Mat, cmd: &str) -> Result<Option<XRef>> {
    let st = s.model.state();
    let named = |name: &str| -> Result<Option<XRef>> {
        joints::named_face_spot(&s.doc, &st, None, name).map_err(|why| bad(cmd, format!("`x`: {why}")))?;
        Ok(Some(XRef::Face { face: name.to_string(), pick: None }))
    };
    let wrong = || bad(cmd, "`x` is a direction [x, y, z], a face's name, or {face: [x, y, z]}");
    match v.get("x") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) if !name.is_empty() && name.len() <= MAX_FACE_NAME => named(name),
        Some(o @ Value::Object(_)) => match o.get("face") {
            Some(Value::String(name)) if !name.is_empty() && name.len() <= MAX_FACE_NAME => named(name),
            Some(p) => {
                let pick = vec3(p).map(|p| solidcraft_doc::apply_point(inv, p)).ok_or_else(wrong)?;
                let face = joints::face_name_at(&s.doc, &st, component, pick, false).ok_or_else(|| bad(cmd, "`x`: no named face there"))?;
                Ok(Some(XRef::Face { face, pick: Some(pick) }))
            }
            None => Err(wrong()),
        },
        Some(d) => {
            let d = vec3(d).filter(|d| d.is_finite() && d.len() > 1e-9).ok_or_else(wrong)?;
            Ok(Some(XRef::Dir(solidcraft_doc::apply_vector(inv, d))))
        }
    }
}

/// The round end of a cylindrical face of the occurrence's bodies centred at `pick`: a point on
/// its axis there, pointing out of that end (as picking its circular edge gives).
fn circle_centre(s: &Session, occurrence: u64, pick: Vec3) -> Option<Snap> {
    let component = if occurrence == 0 { 0 } else { s.doc.occurrences.iter().find(|o| o.id == occurrence)?.component };
    let st = s.model.state();
    let c = st
        .bodies
        .iter()
        .filter(|b| s.doc.model_body_component(b) == component)
        .find_map(|b| solidcraft_doc::kernel::cylinder_end_at(&b.body, pick))?;
    let along = (pick - c.axis_point).dot(c.axis);
    let (at, dir) = if (along - c.start).abs() <= (c.end - along).abs() { (c.start, -1.0) } else { (c.end, 1.0) };
    Some(Snap::Point { point: c.axis_point + c.axis * at, z: Some(c.axis * dir), x: None })
}

pub(crate) fn joint_id(s: &Session, v: Option<&Value>, cmd: &str) -> Result<u64> {
    let key = match v {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`joint` must be an id or a name")),
    };
    let joint = if let Some(Value::Number(number)) = v {
        number.as_u64().and_then(|id| s.doc.assembly.joints.iter().find(|joint| joint.id == id))
    } else {
        s.doc.assembly.joints.iter().find(|joint| joint.id.to_string() == key || joint.name == key)
    };
    joint.map(|joint| joint.id).ok_or_else(|| bad(cmd, format!("no joint `{key}`")))
}

/// A joint value: numbers are degrees for angles and mm for distances; text is an expression.
pub(crate) fn value(s: &Session, v: &Value, angle: bool, cmd: &str) -> Result<f64> {
    let kind = if angle { Kind::Angle } else { Kind::Length };
    let e = match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite() && x.abs() < 1e9).map(|x| x.to_string()),
        Value::String(t) => Some(t.clone()),
        _ => None,
    }
    .ok_or_else(|| bad(cmd, "values must be numbers or expressions"))?;
    s.doc.eval(&e, kind).map_err(|e| bad(cmd, e.to_string()))
}

/// The expression a number was typed as, kept so the joint follows the parameters it names
/// (`None` for a plain number, or for text that names no parameter, such as `"5 mm"`).
fn typed(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(t)) if !solidcraft_doc::expr::references(t).is_empty() => Some(t.trim().to_string()),
        _ => None,
    }
}

/// The expressions among `values` (or `value`), by index.
fn value_exprs(p: &Value) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = match (p.get("values"), p.get("value")) {
        (Some(Value::Array(a)), _) => a.iter().map(|v| typed(Some(v))).collect(),
        (_, Some(v)) => vec![typed(Some(v))],
        _ => Vec::new(),
    };
    while out.last().is_some_and(Option::is_none) {
        out.pop();
    }
    out
}

fn values_param(s: &Session, p: &Value, kind: JointKind, cmd: &str) -> Result<Vec<f64>> {
    let n = kind.dofs().len();
    let list: Vec<Value> = match (p.get("values"), p.get("value")) {
        (Some(Value::Array(a)), _) => a.clone(),
        (_, Some(v)) => vec![v.clone()],
        _ => Vec::new(),
    };
    if list.len() > n {
        return Err(bad(cmd, format!("a {kind:?} joint has {n} values ({})", kind.dofs().join(", "))));
    }
    let mut out = vec![0.0; n];
    for (i, v) in list.iter().enumerate() {
        if let Some(slot) = out.get_mut(i) {
            *slot = value(s, v, kind.is_angle(i), cmd)?;
        }
    }
    Ok(out)
}

fn limits_param(s: &Session, p: &Value, kind: JointKind, cmd: &str) -> Result<Vec<Option<(f64, f64)>>> {
    let n = kind.dofs().len();
    let mut out = vec![None; n];
    if let Some(Value::Array(a)) = p.get("limits") {
        if a.len() > n {
            return Err(bad(cmd, "more limits than values"));
        }
        for (i, l) in a.iter().enumerate() {
            if let (Some(lo), Some(hi)) = (l.get(0), l.get(1))
                && let Some(slot) = out.get_mut(i)
            {
                *slot = Some((value(s, lo, kind.is_angle(i), cmd)?, value(s, hi, kind.is_angle(i), cmd)?));
            }
        }
    }
    Ok(out)
}

/// A joint's two origins on different occurrences (joint.create and joint.edit). Several parts as
/// bodies of one component (a multi-body STEP import, #1239) say how to get two components.
fn check_sides(a: &JointOrigin, b: &JointOrigin, cmd: &str) -> Result<()> {
    if a.occurrence == b.occurrence {
        return Err(bad(
            cmd,
            "a joint needs two different occurrences: both origins are on one component; give each part a component of its own first \
             (component.from_bodies)",
        ));
    }
    Ok(())
}

fn add_joint(s: &mut Session, mut j: Joint, p: &Value) -> Result<Value> {
    if s.doc.assembly.joints.len() >= 10_000 {
        return Err(EngineError::Other("too many joints".into()));
    }
    check_sides(&j.a, &j.b, "joint.create")?;
    // It starts at its values inside its limits. A value moved to a limit moves the part as it
    // is joined, which the answer says.
    let mut q = j.values.clone();
    q.resize(j.kind.dofs().len(), 0.0);
    joints::clamp(&j, &mut q);
    let round = |v: &Value| v.as_f64().map(|x| (x * 1e6).round() / 1e6).unwrap_or(f64::NAN);
    let warnings: Vec<String> = named_values(&j, &q)
        .iter()
        .zip(named_values(&j, &j.values))
        .filter(|(now, was)| now["value"] != was["value"])
        .map(|(now, was)| {
            let unit = now["unit"].as_str().unwrap_or("");
            format!(
                "{} {} starts at {} {unit}, inside its limits [{}, {}], not at {} {unit}: the part moved there as it was joined",
                j.name,
                now["name"].as_str().unwrap_or(""),
                round(&now["value"]),
                round(&now["limits"][0]),
                round(&now["limits"][1]),
                round(&was["value"])
            )
        })
        .collect();
    j.values = q;
    let id = j.id;
    let name = j.name.clone();
    // A new joint starts shown, even if a stale visibility entry came from an older design.
    s.visibility.hidden_joints.retain(|h| *h != id);
    s.doc_mut().assembly.joints.push(j);
    let sol = resolve(s);
    let mut out = sol_json(&sol, p);
    out["joint"] = json!(id);
    out["name"] = json!(name);
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(out)
}

fn joint_origin(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.origin";
    let origin = origin_param(s, Some(p), cmd)?;
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => (1..).map(|k| format!("Joint Origin{k}")).find(|n| !s.doc.assembly.origins.iter().any(|o| &o.name == n)).unwrap_or_default(),
    };
    if s.doc.assembly.origins.iter().any(|o| o.name == name) {
        return Err(bad(cmd, format!("there is already a joint origin `{name}`")));
    }
    s.doc_mut().assembly.origins.push(NamedOrigin { name: name.clone(), origin });
    Ok(json!({"origin": name}))
}

fn kind_param(p: &Value, cmd: &str) -> Result<JointKind> {
    str_(p, "type")
        .and_then(JointKind::parse)
        .ok_or_else(|| bad(cmd, "`type` must be rigid, revolute, slider, cylindrical, pin_slot, planar, ball, parallel, perpendicular, angle, point_on_plane, point_on_line or line_on_plane"))
}

fn new_joint_name(s: &Session, p: &Value) -> String {
    str_(p, "name").map(str::to_string).unwrap_or_else(|| {
        let names: std::collections::BTreeSet<_> = s.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect();
        let number = (1..=s.doc.assembly.joints.len().saturating_add(1)).find(|n| !names.contains(format!("Joint{n}").as_str())).unwrap_or(1);
        format!("Joint{number}")
    })
}

/// If a plane or axis origin was given with no pick point, anchor it where the other
/// side's origin currently sits in the world, projected onto the plane or axis.
fn anchor_datum_origin(doc: &solidcraft_doc::Document, st: &solidcraft_doc::ModelState, target: &mut JointOrigin, other: &JointOrigin) {
    if !matches!(&target.snap, Snap::Plane { pick: None, .. } | Snap::Axis { pick: None, .. }) {
        return;
    }
    let Ok(other_frame) = doc.origin_world(st, other) else { return };
    let other_world_pt = solidcraft_doc::apply_point(&other_frame, Vec3::ZERO);
    let target_world = if target.occurrence == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(doc, target.occurrence) };
    let Some(inv) = solidcraft_doc::mat_inverse(&target_world) else { return };
    let local_pt = solidcraft_doc::apply_point(&inv, other_world_pt);
    match &mut target.snap {
        Snap::Plane { pick, .. } | Snap::Axis { pick, .. } => {
            *pick = Some(local_pt);
        }
        _ => {}
    }
}

fn joint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.create";
    let kind = kind_param(p, cmd)?;
    let mut a = origin_param(s, p.get("a"), cmd)?;
    let mut b = origin_param(s, p.get("b"), cmd)?;
    let st = s.model.state();
    anchor_datum_origin(&s.doc, &st, &mut a, &b);
    anchor_datum_origin(&s.doc, &st, &mut b, &a);
    let id = s.doc_mut().allocate_id()?;
    let name = new_joint_name(s, p);
    let offset = match p.get("offset") {
        Some(v) => value(s, v, false, cmd)?,
        None => 0.0,
    };
    let angle = match p.get("angle") {
        Some(v) => Some(value(s, v, true, cmd)?),
        None => None,
    };
    let mut j = Joint {
        id,
        name,
        kind,
        a,
        b,
        values: values_param(s, p, kind, cmd)?,
        limits: limits_param(s, p, kind, cmd)?,
        flip: bool_(p, "flip").unwrap_or(false),
        offset,
        angle: angle.unwrap_or(0.0),
        suppressed: false,
        exprs: JointExprs { offset: typed(p.get("offset")), angle: typed(p.get("angle")), values: value_exprs(p) },
    };
    // A relation given no values starts from where its parts are: B moves no further than it must.
    if p.get("values").is_none()
        && p.get("value").is_none()
        && let Some(q) = joints::relation_values(&s.doc, &s.model.state(), &j)
    {
        j.values = q;
    }
    // No angle given: B keeps its turn about the joint's axis where a half turn would otherwise
    // come only from how its origin's x was chosen. (An angle relation's angle is its tilt.)
    if angle.is_none() && !j.kind.is_relation() {
        j.angle = joints::least_turn_angle(&s.doc, &s.model.state(), &j);
    }
    add_joint(s, j, p)
}

fn as_built(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.as_built";
    let kind = kind_param(p, cmd)?;
    let (oa, ob) = (occurrence_id(s, p.get("a"), cmd)?, occurrence_id(s, p.get("b"), cmd)?);
    // The joint's frame in the world: the given origin, or B's component origin.
    let at = p.get("at").map(|v| origin_param(s, Some(v), cmd)).transpose()?;
    let w = match &at {
        Some(o) => s.doc.origin_world(&s.model.state(), o).map_err(|e| bad(cmd, e.to_string()))?,
        None => joints::occurrence_world(&s.doc, ob),
    };
    // An origin on A or B is kept as it is on that side, so the joint follows its face or hole
    // as the design changes; the other side holds the frame where it is now.
    let side = |occ: u64| -> Result<Snap> {
        match &at {
            Some(o) if o.occurrence == occ => Ok(o.snap.clone()),
            _ => s.doc.frame_in_occurrence(occ, &w).ok_or_else(|| bad(cmd, "occurrence transform")),
        }
    };
    let fa = side(oa)?;
    let fb = side(ob)?;
    let id = s.doc_mut().allocate_id()?;
    let name = new_joint_name(s, p);
    let n = kind.dofs().len();
    let j = Joint {
        id,
        name,
        kind,
        a: JointOrigin { occurrence: oa, snap: fa },
        b: JointOrigin { occurrence: ob, snap: fb },
        values: vec![0.0; n],
        limits: limits_param(s, p, kind, cmd)?,
        flip: true, // as built: the origins coincide as they are, not mated
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
    };
    add_joint(s, j, p)
}

fn rigid_group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.rigid_group";
    let list = p.get("occurrences").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.len() < 2 || list.len() > 1000 {
        return Err(bad(cmd, "`occurrences` must list 2…1000 occurrences"));
    }
    let ids: Vec<u64> = list.iter().map(|v| occurrence_id(s, Some(v), cmd)).collect::<Result<_>>()?;
    let first = ids.first().copied().unwrap_or(0);
    let mut made = Vec::new();
    for other in ids.iter().skip(1) {
        let r = as_built(s, &json!({"type": "rigid", "a": first, "b": other, "name": format!("Rigid Group {first}:{other}")}))?;
        made.push(r["joint"].clone());
    }
    Ok(json!({"joints": made}))
}

fn drive(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.drive";
    let id = joint_id(s, p.get("joint"), cmd)?;
    let kind = s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind).ok_or_else(|| bad(cmd, "joint"))?;
    if kind == JointKind::Rigid {
        return Err(bad(cmd, "a rigid joint does not move: joint.edit can make it revolute (turns), slider (slides) or cylindrical (both)"));
    }
    let index = match p.get("index") {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < kind.dofs().len())
                .ok_or_else(|| bad(cmd, "index is outside this joint's coordinates"))?,
        ),
    };
    let mut q = if let Some(index) = index {
        if p.get("values").is_some() {
            return Err(bad(cmd, "index goes with value, not values"));
        }
        let mut q = joints::joint_values(&s.doc.assembly).remove(&id).unwrap_or_else(|| vec![0.0; kind.dofs().len()]);
        if let Some(slot) = q.get_mut(index) {
            *slot = value(s, p.get("value").ok_or_else(|| bad(cmd, "give value"))?, kind.is_angle(index), cmd)?;
        }
        q
    } else {
        values_param(s, p, kind, cmd)?
    };
    if p.get("values").is_none() && p.get("value").is_none() {
        return Err(bad(cmd, "give `value` or `values`"));
    }
    // Values not given (after `value`'s first, or past the end of `values`) keep theirs.
    let given = match p.get("values") {
        Some(Value::Array(a)) => a.len(),
        _ => 1,
    };
    let joint = s.doc.assembly.joints.iter().find(|j| j.id == id).ok_or_else(|| bad(cmd, "joint"))?;
    if index.is_none() {
        for (i, v) in q.iter_mut().enumerate().skip(given) {
            *v = joint.values.get(i).copied().unwrap_or(*v);
        }
    }
    // What was asked for, before limits and motion links have their say.
    let requested = q.clone();
    let driven: Vec<usize> = match index {
        Some(index) => vec![index],
        None => (0..given.min(kind.dofs().len())).collect(),
    };
    // A suppressed joint places nothing: a value stored now would move the part only when the
    // joint is unsuppressed, and leave an undo step for a drive whose reply says nothing moved
    // (#9742). A joint in a motion link still carries its value to the joints it drives.
    let hold = joint.suppressed && !s.doc.assembly.links.iter().any(|l| l.a == id || l.b == id);
    let before = super::motion::positions(s);
    let r = if hold {
        json!({"stopped_by_contact": null, "stopped_by_loop": null})
    } else {
        let doc = s.doc_mut();
        let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == id) else { return Err(bad(cmd, "joint")) };
        joints::clamp(j, &mut q);
        // Moved as far toward `q` as contact sets and closed loops allow. Values left out of
        // `values` are not driven: in a closed loop they follow.
        // Driving moves the joint by hand: the values driven drop their expressions (an expression
        // given here is worked out once, as contact or a loop may stop the motion short of it).
        for &i in &driven {
            j.exprs.set_value(i, None);
        }
        if let Some(index) = index { super::motion::drive_joint_index(s, id, q, index)? } else { super::motion::drive_joint(s, id, q, given)? }
    };
    let sol = solution(s, &BTreeMap::new(), response_analysis(p));
    let mut out = sol_json(&sol, p);
    out["joint"] = json!(id);
    out["values"] = json!(effective_values(s, id));
    if bool_(p, "continuous") == Some(true) {
        out["continuous"] = json!(true);
    }
    out["stopped_by_contact"] = r["stopped_by_contact"].clone();
    out["stopped_by_loop"] = r["stopped_by_loop"].clone();
    let moved = super::motion::moved(&before, &super::motion::positions(s)).next().is_some();
    out["moved"] = json!(moved);
    if let Some(j) = s.doc.assembly.joints.iter().find(|j| j.id == id) {
        out["requested"] = json!(named_values(j, &requested));
        let held = if hold { Vec::new() } else { held_values(s, j, &requested, &driven, &r) };
        if let Some(first) = held.first()
            && !moved
        {
            out["reason"] = first["why"].clone();
        }
        if !held.is_empty() {
            out["held"] = json!(held);
        }
    }
    if let Some((why, unconnected)) = cannot_move(s, &sol, id) {
        out["reason"] = json!(why);
        if let Some(o) = unconnected {
            out["unconnected"] = json!(o);
        }
    }
    Ok(out)
}

/// The driven values of `j` that ended up other than `requested`, each with why: a motion link
/// sets it from another coordinate (a screw's travel follows its turn), a limit holds it, or
/// contact or a closed loop stopped the motion short. Degrees and mm, as `values` (#9742).
fn held_values(s: &Session, j: &Joint, requested: &[f64], driven: &[usize], r: &Value) -> Vec<Value> {
    let now = joints::joint_values(&s.doc.assembly).remove(&j.id).unwrap_or_else(|| j.values.clone());
    let names = j.kind.dofs();
    let mut out = Vec::new();
    for &i in driven {
        let (Some(&want), Some(&got)) = (requested.get(i), now.get(i)) else { continue };
        if (want - got).abs() <= 1e-9 * want.abs().max(got.abs()).max(1.0) {
            continue;
        }
        let angle = j.kind.is_angle(i);
        let shown = |v: f64| if angle { v.to_degrees() } else { v };
        let name = names.get(i).copied().unwrap_or("value");
        let why = if let Some(l) = s.doc.assembly.links.iter().find(|l| l.b == j.id && l.ib == i) {
            let coupling = match &l.coupling {
                Some(joints::Coupling::Gear { .. }) => " (gears)",
                Some(joints::Coupling::RackPinion { .. }) => " (rack and pinion)",
                Some(joints::Coupling::Screw { .. }) => " (screw)",
                Some(joints::Coupling::Cam(_)) => " (cam)",
                None => "",
            };
            let from = s.doc.assembly.joints.iter().find(|o| o.id == l.a);
            let coordinate = from.and_then(|o| o.kind.dofs().get(l.ia).copied()).unwrap_or("value");
            if l.a == j.id {
                format!("{name} follows its {coordinate} through a motion link{coupling}")
            } else {
                format!("{name} follows {}'s {coordinate} through a motion link{coupling}", from.map_or("another joint", |o| o.name.as_str()))
            }
        } else if let Some(Some((lo, hi))) = j.limits.get(i)
            && (want < lo.min(*hi) || want > hi.max(*lo))
        {
            format!("{name} is held to its limits, {} to {} {}", shown(lo.min(*hi)), shown(hi.max(*lo)), if angle { "deg" } else { "mm" })
        } else if r["stopped_by_contact"].is_object() {
            format!("{name} stopped at contact")
        } else if r["stopped_by_loop"].is_object() {
            format!("{name} stopped where a closed loop can no longer close")
        } else {
            continue;
        };
        out.push(json!({"name": name, "requested": shown(want), "value": shown(got), "unit": if angle { "deg" } else { "mm" }, "why": why}));
    }
    out
}

/// Joints `joint.reach` takes at most in `joints`.
const MAX_REACH_JOINTS: usize = 1000;

fn reach(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.reach";
    let occ = occurrence_id(s, p.get("occurrence"), cmd)?;
    let given = |k: &str| p.get(k).and_then(vec3).filter(|v| v.is_finite() && v.len() < 1e9);
    let to = given("point").ok_or_else(|| bad(cmd, "`point` must be [x, y, z]"))?;
    let world = if occ == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occ) };
    let inv = solidcraft_doc::mat_inverse(&world).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let at = match p.get("at") {
        Some(_) => solidcraft_doc::apply_point(&inv, given("at").ok_or_else(|| bad(cmd, "`at` must be [x, y, z]"))?),
        None => body_centre(s, occ),
    };
    let unit = |k: &str| -> Result<Option<Vec3>> {
        match p.get(k) {
            None => Ok(None),
            Some(_) => given(k).and_then(Vec3::normalized).map(Some).ok_or_else(|| bad(cmd, format!("`{k}` must be a direction [x, y, z]"))),
        }
    };
    let (z, axis) = (unit("z")?, unit("axis")?);
    if axis.is_some() && z.is_none() {
        return Err(bad(cmd, "`axis` goes with `z`, the way it points"));
    }
    let axis = z.map(|z| (axis.map(|a| solidcraft_doc::apply_vector(&inv, a)).and_then(Vec3::normalized).unwrap_or(Vec3::Z), z));
    let only = match p.get("joints") {
        None => None,
        Some(Value::Array(a)) if a.len() <= MAX_REACH_JOINTS => Some(a.iter().map(|v| joint_id(s, Some(v), cmd)).collect::<Result<BTreeSet<u64>>>()?),
        Some(_) => return Err(bad(cmd, format!("`joints` must be a list of joints (ids or names), at most {MAX_REACH_JOINTS}"))),
    };
    let before = super::motion::positions(s);
    let st = s.model.state();
    let r = joints::reach(&s.doc, &st, &joints::Reach { occurrence: occ, point: at, to, axis, joints: only });
    let reached = match &r {
        Ok(x) => {
            // Only a reach that moves something touches the design (and so adds an undo step).
            if !x.values.is_empty() {
                let doc = s.doc_mut();
                for (id, q) in &x.values {
                    if let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == *id) {
                        j.values.clone_from(q);
                    }
                }
            }
            x.clone()
        }
        Err(_) => joints::Reached::default(),
    };
    // Loops closed again from there, and where it ended up.
    let sol = if response_analysis(p) == joints::Analysis::Placement { place(s) } else { resolve(s) };
    let t = joints::occurrence_world(&s.doc, occ);
    let got = solidcraft_doc::apply_point(&t, at);
    let mut out = sol_json(&sol, p);
    out["occurrence"] = json!(occurrence_name(s, occ));
    out["point"] = json!([got.x, got.y, got.z]);
    out["error_mm"] = json!(got.dist(to));
    let mut there = got.dist(to) <= joints::REACH_TOL;
    if let Some((a, z)) = axis {
        // From the sine too: the cosine alone loses small angles to rounding.
        let v = solidcraft_doc::apply_vector(&t, a);
        let deg = v.cross(z).len().atan2(v.dot(z)).to_degrees();
        out["error_deg"] = json!(deg);
        there &= deg <= joints::REACH_TOL;
    }
    out["reached"] = json!(there);
    let name = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id);
    // As joint.list gives them: with what motion links set.
    let effective = joints::joint_values(&s.doc.assembly);
    out["values"] = reached
        .values
        .keys()
        .filter_map(|id| name(*id))
        .map(|j| json!({"joint": j.id, "name": j.name, "values": named_values(j, effective.get(&j.id).unwrap_or(&j.values))}))
        .collect();
    out["moved"] = json!(super::motion::moved(&before, &super::motion::positions(s)).next().is_some());
    out["stopped_by_limit"] = if reached.limits.is_empty() {
        Value::Null
    } else {
        reached
            .limits
            .iter()
            .filter_map(|(id, i, upper)| {
                let j = name(*id)?;
                Some(json!({"joint": j.name, "value": j.kind.dofs().get(*i), "limit": if *upper { "max" } else { "min" }}))
            })
            .collect()
    };
    out["stopped_by_loop"] = json!(reached.stopped_by_loop.and_then(name).map(|j| json!({"joint": j.name})));
    if let Err(why) = r {
        out["reason"] = json!(why);
    }
    if bool_(p, "continuous") == Some(true) {
        out["continuous"] = json!(true);
    }
    Ok(out)
}

/// The centre of the box around an occurrence's bodies, in its own coordinates (its origin when
/// it has none).
fn body_centre(s: &Session, occ: u64) -> Vec3 {
    let comp = s.doc.occurrences.iter().find(|o| o.id == occ).map(|o| o.component).unwrap_or(0);
    let st = s.model.state();
    st.bodies.iter().filter(|b| s.doc.model_body_component(b) == comp).fold(Aabb3::EMPTY, |bb, b| bb.union(&b.mesh().bounds())).center()
}

fn motion_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.motion_link";
    let (a, b) = (joint_id(s, p.get("a"), cmd)?, joint_id(s, p.get("b"), cmd)?);
    if a == b {
        return Err(bad(cmd, "link two different joints"));
    }
    let ratio = num(p, "ratio").ok_or_else(|| bad(cmd, "`ratio` must be a number"))?;
    let (ia, ib) = (num(p, "ia").unwrap_or(0.0).max(0.0) as usize, num(p, "ib").unwrap_or(0.0).max(0.0) as usize);
    let dofs = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind.dofs().len()).unwrap_or(0);
    if ia >= dofs(a) || ib >= dofs(b) {
        return Err(bad(cmd, "the linked joints need moving values at those indices"));
    }
    // In b's units, like its values and limits (stored in radians for an angle).
    let offset = match p.get("offset") {
        Some(v) => value(s, v, is_angle(s, b, ib), cmd)?,
        None => 0.0,
    };
    let ratio = ratio * ratio_scale(s, a, ia, b, ib);
    s.doc_mut().assembly.links.push(MotionLink { a, ia, b, ib, ratio, offset, coupling: None, lift: Vec::new() });
    let sol = resolve(s);
    Ok(sol_json(&sol, p))
}

/// A coupling length: a number (mm), an expression (kept as typed, so it follows the
/// parameters), or `{circle: [x, y, z], occurrence?}`: the round face there, by its radius.
fn size_param(s: &Session, p: &Value, key: &str, cmd: &str) -> Result<Size> {
    match p.get(key) {
        Some(Value::Number(n)) => n.as_f64().map(Size::Length).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite length"))),
        Some(Value::String(t)) if t.len() <= 1000 => Ok(Size::Expr(t.trim().to_string())),
        Some(o @ Value::Object(m)) if m.contains_key("circle") => match origin_param(s, Some(o), cmd)? {
            o @ JointOrigin { snap: Snap::CircleCenter { .. }, .. } => Ok(Size::Circle(o)),
            _ => Err(bad(cmd, format!("`{key}`: give a point on the round face, not its centre"))),
        },
        _ => Err(bad(cmd, format!("`{key}` must be a length (mm or an expression) or a round face ({{circle: [x, y, z]}})"))),
    }
}

/// A joint's first value that turns (an angle), or that travels (a distance).
fn value_index(s: &Session, id: u64, turn: bool) -> Option<usize> {
    let j = s.doc.assembly.joints.iter().find(|j| j.id == id)?;
    (0..j.kind.dofs().len()).find(|i| j.kind.is_angle(*i) == turn)
}

/// The values a rack or a screw couples: a's turn and b's travel, or a's travel and b's turn.
fn turn_and_travel(s: &Session, a: u64, b: u64, cmd: &str) -> Result<(usize, usize)> {
    match (value_index(s, a, true), value_index(s, b, false), value_index(s, a, false), value_index(s, b, true)) {
        (Some(ia), Some(ib), ..) | (_, _, Some(ia), Some(ib)) => Ok((ia, ib)),
        _ => Err(bad(cmd, "couple a joint that turns (revolute) with one that slides (slider)")),
    }
}

/// Two different joints `a` and `b`.
fn joint_pair(s: &Session, p: &Value, cmd: &str) -> Result<(u64, u64)> {
    let (a, b) = (joint_id(s, p.get("a"), cmd)?, joint_id(s, p.get("b"), cmd)?);
    if a == b {
        return Err(bad(cmd, "couple two different joints"));
    }
    Ok((a, b))
}

fn reverse(p: &Value) -> bool {
    bool_(p, "reverse").unwrap_or(false)
}

fn gear(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.gear";
    let (a, b) = joint_pair(s, p, cmd)?;
    let turn = |id: u64| value_index(s, id, true).ok_or_else(|| bad(cmd, "gears need two joints that turn (revolute)"));
    let (ia, ib) = (turn(a)?, turn(b)?);
    let c = Coupling::Gear {
        ra: size_param(s, p, "ra", cmd)?,
        rb: size_param(s, p, "rb", cmd)?,
        internal: bool_(p, "internal").unwrap_or(false),
        reverse: reverse(p),
    };
    couple(s, p, cmd, MotionLink { a, ia, b, ib, ratio: 1.0, offset: 0.0, coupling: Some(c), lift: Vec::new() })
}

fn rack_pinion(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.rack_pinion";
    let (a, b) = joint_pair(s, p, cmd)?;
    let (ia, ib) = turn_and_travel(s, a, b, cmd)?;
    let c = Coupling::RackPinion { radius: size_param(s, p, "radius", cmd)?, reverse: reverse(p) };
    couple(s, p, cmd, MotionLink { a, ia, b, ib, ratio: 1.0, offset: 0.0, coupling: Some(c), lift: Vec::new() })
}

fn screw(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.screw";
    let (a, ia, b, ib) = match p.get("joint") {
        Some(v) => {
            let id = joint_id(s, Some(v), cmd)?;
            match (value_index(s, id, true), value_index(s, id, false)) {
                (Some(ia), Some(ib)) => (id, ia, id, ib),
                _ => return Err(bad(cmd, "a screw on one joint needs a joint that turns and slides (cylindrical); or give a and b")),
            }
        }
        None => {
            let (a, b) = joint_pair(s, p, cmd)?;
            let (ia, ib) = turn_and_travel(s, a, b, cmd)?;
            (a, ia, b, ib)
        }
    };
    let c = Coupling::Screw { pitch: size_param(s, p, "pitch", cmd)?, reverse: reverse(p) };
    couple(s, p, cmd, MotionLink { a, ia, b, ib, ratio: 1.0, offset: 0.0, coupling: Some(c), lift: Vec::new() })
}

fn cam_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.cam";
    let (a, b) = joint_pair(s, p, cmd)?;
    let kind = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind);
    if kind(a) != Some(JointKind::Revolute) {
        return Err(bad(cmd, "the cam (a) turns on a revolute joint"));
    }
    let oscillating = match kind(b) {
        Some(JointKind::Slider) => false,
        Some(JointKind::Revolute) => true,
        _ => return Err(bad(cmd, "the follower (b) is a slider or a revolute joint")),
    };
    let follower = match str_(p, "follower").unwrap_or("knife") {
        "knife" => Follower::Knife,
        "roller" => Follower::Roller { radius: size_param(s, p, "radius", cmd)? },
        "flat" => Follower::Flat,
        _ => return Err(bad(cmd, "`follower` is knife, roller or flat")),
    };
    if oscillating && p.get("tip").is_none() && !matches!(follower, Follower::Roller { radius: Size::Circle(_) }) {
        return Err(bad(cmd, "an oscillating follower needs `tip`: its knife's point or its roller's centre ({circle: [x, y, z]} on the roller)"));
    }
    let tip = match p.get("tip") {
        Some(v) => Some(origin_param(s, Some(v), cmd)?),
        None => None,
    };
    let mut c = Cam { profile: cam_profile(s, p, cmd)?, follower, tip, reverse: false };
    let probe = MotionLink { a, ia: 0, b, ib: 0, ratio: 1.0, offset: 0.0, coupling: None, lift: Vec::new() };
    let st = s.model.state();
    c.reverse = cam::default_reverse(&s.doc, &st, &probe, &c).map_err(|e| bad(cmd, e.to_string()))? != bool_(p, "flip").unwrap_or(false);
    let offset = cam::follower_offset(&s.doc, &st, &probe, &c).map_err(|e| bad(cmd, e.to_string()))?;
    let reverse = c.reverse;
    let mut out = couple(s, p, cmd, MotionLink { coupling: Some(Coupling::Cam(c)), ..probe })?;
    let index = out["link"].as_u64().and_then(|i| usize::try_from(i).ok()).unwrap_or(usize::MAX);
    if let (Some(o), Some(l)) = (out.as_object_mut(), s.doc.assembly.links.get(index)) {
        o.remove("ratio");
        o.insert("lift".into(), lift_json(l, oscillating));
        o.insert("reverse".into(), json!(reverse));
        if !oscillating {
            o.insert("follower_offset".into(), json!(offset));
        }
    }
    Ok(out)
}

/// A cam's follower travel over a turn of the cam: b's lowest and highest values and their
/// difference, in degrees (`angle`) or mm.
fn lift_json(l: &MotionLink, angle: bool) -> Value {
    // A table that is not whole (a hand-edited file) moves nothing, and says so.
    if l.lift.len() != cam::CAM_SAMPLES {
        return Value::Null;
    }
    let unit = |v: f64| if angle { v.to_degrees() } else { v };
    let (lo, hi) = l.lift.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let (lo, hi) = (unit(lo + l.offset), unit(hi + l.offset));
    json!({"min": lo, "max": hi, "rise": hi - lo, "unit": if angle { "deg" } else { "mm" }})
}

/// A cam's profile: a sketch's closed profile (kept by its outer loop's curve ids, so the sketch's
/// other profiles may change), or picked edges of the cam's bodies (kept in the component's
/// coordinates with the edges' names).
fn cam_profile(s: &Session, p: &Value, cmd: &str) -> Result<CamProfile> {
    let v = p.get("profile").ok_or_else(|| bad(cmd, "`profile` is required: {sketch: id|name} or {edges: [[x, y, z], …]}"))?;
    if let Some(sk) = v.get("sketch") {
        let sketch = super::features::sketch_id(s, Some(sk), cmd, "profile.sketch")?;
        let st = s.model.state();
        let solved =
            st.sketches.iter().find(|x| x.feature == sketch).ok_or_else(|| bad(cmd, "that sketch is not evaluated (it is after the marker?)"))?;
        let picked = match v.get("index") {
            None => cam::sketch_profile(solved, &[]),
            Some(i) => i.as_u64().and_then(|i| usize::try_from(i).ok()).and_then(|i| solved.profiles.get(i)),
        };
        let profile = picked.ok_or_else(|| bad(cmd, "the sketch has no such closed profile (an open curve makes none)"))?;
        return Ok(CamProfile::Sketch { sketch, curves: profile.outer_curves.clone() });
    }
    let Some(Value::Array(pts)) = v.get("edges") else {
        return Err(bad(cmd, "`profile` is {sketch: id|name} or {edges: [[x, y, z], …]}"));
    };
    if pts.is_empty() || pts.len() > cam::MAX_PROFILE_EDGES {
        return Err(bad(cmd, format!("pick 1 to {} edges", cam::MAX_PROFILE_EDGES)));
    }
    let world: Vec<Vec3> = pts.iter().map(vec3).collect::<Option<_>>().ok_or_else(|| bad(cmd, "`edges` are finite [x, y, z] points"))?;
    let occurrence = match v.get("occurrence") {
        Some(o) => occurrence_id(s, Some(o), cmd)?,
        None => world.first().and_then(|w| occurrence_at(s, *w)).ok_or_else(|| bad(cmd, "no body there"))?,
    };
    let t = if occurrence == 0 { solidcraft_doc::IDENTITY } else { joints::occurrence_world(&s.doc, occurrence) };
    let inv = solidcraft_doc::mat_inverse(&t).ok_or_else(|| bad(cmd, "occurrence transform"))?;
    let picks: Vec<Vec3> = world.iter().map(|w| solidcraft_doc::apply_point(&inv, *w)).collect();
    let comp = s.doc.occurrences.iter().find(|o| o.id == occurrence).map(|o| o.component).unwrap_or(0);
    let st = s.model.state();
    let bodies: Vec<&solidcraft_doc::ModelBody> = st.bodies.iter().filter(|b| s.doc.model_body_component(b) == comp).collect();
    // Each pick's edge name on the body whose edges pass nearest it.
    let names = picks
        .iter()
        .map(|q| {
            let near = |b: &&solidcraft_doc::ModelBody| {
                b.mesh()
                    .edges
                    .iter()
                    .flat_map(|e| e.windows(2))
                    .filter_map(|w| Some(q.dist_to_segment(*w.first()?, *w.get(1)?)))
                    .fold(f64::MAX, f64::min)
            };
            bodies
                .iter()
                .min_by(|x, y| near(x).total_cmp(&near(y)))
                .and_then(|b| solidcraft_doc::naming::edge_names_at(b, &[*q]).into_iter().next())
                .unwrap_or_default()
        })
        .collect();
    Ok(CamProfile::Edges { occurrence, picks, names })
}

/// Add a coupling's motion link, or put it in place of link `link`: its ratio worked out from its
/// sizes, its offset leaving b where it is (the teeth mesh as they are now), for a new link and
/// one put in place of another alike.
fn couple(s: &mut Session, p: &Value, cmd: &str, mut l: MotionLink) -> Result<Value> {
    let links = &s.doc.assembly.links;
    let replace = match p.get("link") {
        None => None,
        Some(v) => {
            Some(v.as_u64().and_then(|i| usize::try_from(i).ok()).filter(|i| *i < links.len()).ok_or_else(|| bad(cmd, "no such motion link"))?)
        }
    };
    if replace.is_none() && links.len() >= joints::MAX_JOINTS {
        return Err(bad(cmd, "too many motion links"));
    }
    let others: Vec<&MotionLink> = links.iter().enumerate().filter(|(i, _)| Some(*i) != replace).map(|(_, k)| k).collect();
    let name = |id: u64| s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.name.clone()).unwrap_or_default();
    if others.iter().any(|k| (k.b, k.ib) == (l.b, l.ib)) {
        return Err(bad(cmd, format!("{} already follows a motion link: couple it the other way round (as a)", name(l.b))));
    }
    // The values that set a's, back through the links: b among them would make a cycle.
    let mut at = (l.a, l.ia);
    for _ in 0..=others.len() {
        if at == (l.b, l.ib) {
            return Err(bad(cmd, format!("{} already drives {} through motion links", name(l.b), name(l.a))));
        }
        match others.iter().find(|k| (k.b, k.ib) == at) {
            Some(k) => at = (k.a, k.ia),
            None => break,
        }
    }
    (l.ratio, l.lift) =
        joints::coupling_values(&s.doc, &s.model.state(), &l).unwrap_or(Ok((l.ratio, Vec::new()))).map_err(|e| bad(cmd, e.to_string()))?;
    let vals = joints::joint_values(&s.doc.assembly);
    let value = |id: u64, i: usize| vals.get(&id).and_then(|q| q.get(i)).copied().unwrap_or(0.0);
    // New or put in place of another (turned the other way, a ring gear now), b keeps where it
    // is: the gears stay in mesh at the pose they are in. (A radius edited in the design keeps
    // the offset instead, so the gears turn from where they meshed when they were coupled.)
    l.offset = value(l.b, l.ib) - MotionLink { offset: 0.0, ..l.clone() }.follow(value(l.a, l.ia));
    if !l.offset.is_finite() {
        return Err(bad(cmd, "the coupling gives the driven joint no value where it is"));
    }
    let shown = l.ratio / ratio_scale(s, l.a, l.ia, l.b, l.ib);
    let doc = s.doc_mut();
    let index = match replace {
        Some(i) => {
            if let Some(slot) = doc.assembly.links.get_mut(i) {
                *slot = l;
            }
            i
        }
        None => {
            doc.assembly.links.push(l);
            doc.assembly.links.len() - 1
        }
    };
    let sol = resolve(s);
    let mut out = sol_json(&sol, p);
    out["link"] = json!(index);
    out["ratio"] = json!(shown);
    Ok(out)
}

fn is_angle(s: &Session, joint: u64, i: usize) -> bool {
    s.doc.assembly.joints.iter().find(|j| j.id == joint).is_some_and(|j| j.kind.is_angle(i))
}

/// A link's `ratio` is in b's units per a's (mm per degree, degrees per mm) and stored per
/// radian: the given one times this is the stored one.
fn ratio_scale(s: &Session, a: u64, ia: usize, b: u64, ib: usize) -> f64 {
    let unit = |angle: bool| if angle { 1f64.to_radians() } else { 1.0 };
    unit(is_angle(s, b, ib)) / unit(is_angle(s, a, ia))
}

fn limits(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.limits";
    let id = joint_id(s, p.get("joint"), cmd)?;
    let i = num(p, "index").unwrap_or(0.0).max(0.0) as usize;
    let kind = s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.kind).ok_or_else(|| bad(cmd, "joint"))?;
    if i >= kind.dofs().len() {
        return Err(bad(cmd, "no such value"));
    }
    let lim = match (p.get("min"), p.get("max")) {
        (Some(lo), Some(hi)) => Some((value(s, lo, kind.is_angle(i), cmd)?, value(s, hi, kind.is_angle(i), cmd)?)),
        (None, None) => None,
        _ => return Err(bad(cmd, "give both `min` and `max`, or neither to clear")),
    };
    let doc = s.doc_mut();
    let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == id) else { return Err(bad(cmd, "joint")) };
    j.limits.resize(kind.dofs().len(), None);
    if let Some(slot) = j.limits.get_mut(i) {
        *slot = lim;
    }
    let mut q = j.values.clone();
    q.resize(kind.dofs().len(), 0.0);
    joints::clamp(j, &mut q);
    j.values = q;
    let sol = resolve(s);
    Ok(sol_json(&sol, p))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.edit";
    let id = joint_id(s, p.get("joint"), cmd)?;
    let kind = match str_(p, "type") {
        Some(t) => Some(JointKind::parse(t).ok_or_else(|| bad(cmd, "unknown joint type"))?),
        None => None,
    };
    let offset = match p.get("offset") {
        Some(v) => Some(value(s, v, false, cmd)?),
        None => None,
    };
    let angle = match p.get("angle") {
        Some(v) => Some(value(s, v, true, cmd)?),
        None => None,
    };
    // The sides, read as joint.create reads them; a side left out keeps its place.
    let a = p.get("a").map(|v| origin_param(s, Some(v), cmd)).transpose()?;
    let b = p.get("b").map(|v| origin_param(s, Some(v), cmd)).transpose()?;
    let sides = a.is_some() || b.is_some();
    if sides {
        let j = s.doc.assembly.joints.iter().find(|j| j.id == id).ok_or_else(|| bad(cmd, "joint"))?;
        check_sides(a.as_ref().unwrap_or(&j.a), b.as_ref().unwrap_or(&j.b), cmd)?;
    }
    let doc = s.doc_mut();
    let Some(j) = doc.assembly.joints.iter_mut().find(|j| j.id == id) else { return Err(bad(cmd, "joint")) };
    if let Some(a) = a {
        j.a = a;
    }
    if let Some(b) = b {
        j.b = b;
    }
    if let Some(k) = kind {
        let was = j.kind;
        j.kind = k;
        j.values.resize(k.dofs().len(), 0.0);
        j.limits.resize(k.dofs().len(), None);
        j.exprs.values.truncate(k.dofs().len());
        // A value that changes unit with the type (a slide becoming a turn) drops its expression,
        // which would no longer work out and leave the joint out of the solve.
        for i in 0..j.exprs.values.len() {
            if was.is_angle(i) != k.is_angle(i) {
                j.exprs.set_value(i, None);
            }
        }
    }
    if let Some(o) = offset {
        j.offset = o;
        j.exprs.offset = typed(p.get("offset"));
    }
    if let Some(a) = angle {
        j.angle = a;
        j.exprs.angle = typed(p.get("angle"));
    }
    if let Some(f) = bool_(p, "flip") {
        j.flip = f;
    }
    if let Some(n) = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128) {
        j.name = n.to_string();
    }
    if let Some(x) = bool_(p, "suppressed") {
        suppress_joints(s.doc_mut(), &[id], x);
    }
    // A side moved without an angle turns B as joint.create would: the least turn from where it
    // is now (a relation's angle is its tilt, which stays; an angle expression keeps driving it).
    if sides
        && p.get("angle").is_none()
        && let Some(j) = s.doc.assembly.joints.iter().find(|j| j.id == id).filter(|j| !j.kind.is_relation() && j.exprs.angle.is_none()).cloned()
    {
        let turn = joints::least_turn_angle(&s.doc, &s.model.state(), &j);
        if let Some(j) = s.doc_mut().assembly.joints.iter_mut().find(|j| j.id == id) {
            j.angle = turn;
        }
    }
    // A relation whose type, offset, tilt or flip changes starts again from where its parts are.
    let reseat = kind.is_some() || offset.is_some() || angle.is_some() || p.get("flip").is_some() || sides;
    if reseat {
        let edited = s.doc.assembly.joints.iter().find(|j| j.id == id).cloned();
        if let Some(q) = edited.and_then(|j| joints::relation_values(&s.doc, &s.model.state(), &j))
            && let Some(j) = s.doc_mut().assembly.joints.iter_mut().find(|j| j.id == id)
        {
            j.values = q;
        }
    }
    let sol = resolve(s);
    Ok(sol_json(&sol, p))
}

/// Shared mutation for individual joints and timeline folders; the caller resolves once.
pub(super) fn suppress_joints(doc: &mut solidcraft_doc::Document, ids: &[u64], suppressed: bool) {
    let ids: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    for joint in &mut doc.assembly.joints {
        if ids.contains(&joint.id) {
            joint.suppressed = suppressed;
        }
    }
}

pub(super) fn delete_joints(doc: &mut solidcraft_doc::Document, ids: &[u64]) {
    let ids: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    doc.assembly.joints.retain(|joint| !ids.contains(&joint.id));
    doc.assembly.links.retain(|link| !ids.contains(&link.a) && !ids.contains(&link.b));
    for group in &mut doc.timeline_groups {
        group.joints.retain(|id| !ids.contains(id));
    }
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.delete";
    if let Some(name) = str_(p, "origin") {
        let used = s.doc.assembly.joints.len();
        let doc = s.doc_mut();
        let before = doc.assembly.origins.len();
        doc.assembly.origins.retain(|o| o.name != name);
        if doc.assembly.origins.len() == before {
            return Err(bad(cmd, format!("no joint origin `{name}`")));
        }
        return Ok(json!({"deleted_origin": name, "joints": used}));
    }
    if let Some(i) = num(p, "link") {
        let i = i.max(0.0) as usize;
        let doc = s.doc_mut();
        if i >= doc.assembly.links.len() {
            return Err(bad(cmd, "no such motion link"));
        }
        doc.assembly.links.remove(i);
        return Ok(json!({"deleted_link": i}));
    }
    let id = joint_id(s, p.get("joint"), cmd)?;
    delete_joints(s.doc_mut(), &[id]);
    let sol = resolve(s);
    let mut out = sol_json(&sol, p);
    out["deleted"] = json!(id);
    Ok(out)
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let sol = joints::solve(&s.doc, &st);
    let vals = joints::joint_values(&s.doc.assembly);
    let js: Vec<Value> = s
        .doc
        .assembly
        .joints
        .iter()
        .map(|j| {
            let named = named_values(j, vals.get(&j.id).map(Vec::as_slice).unwrap_or_default());
            let world = |o: &JointOrigin| s.doc.origin_world(&st, o).ok().map(|m| json!({"origin": [m[3][0], m[3][1], m[3][2]], "z": [m[2][0], m[2][1], m[2][2]]}));
            let mut out = json!({"id": j.id, "name": j.name, "type": j.kind, "a": j.a.occurrence, "b": j.b.occurrence, "values": named,
                "flip": j.flip, "offset": j.offset, "suppressed": j.suppressed, "visible": s.joint_visible(j.id), "frame_a": world(&j.a), "frame_b": world(&j.b)});
            if !j.exprs.is_empty() {
                out["expressions"] = json!(j.exprs);
            }
            out
        })
        .collect();
    let links: Vec<Value> = s.doc.assembly.links.iter().map(|l| link_json(s, &st, l)).collect();
    let origins: Vec<Value> = s.doc.assembly.origins.iter().map(|o| json!({"name": o.name, "occurrence": o.origin.occurrence})).collect();
    let mut out = sol_json(&sol, p);
    out["joints"] = json!(js);
    out["links"] = json!(links);
    out["origins"] = json!(origins);
    out["floating"] = json!(floating_chains(s, &sol));
    Ok(out)
}

/// A motion link as `joint.list` and `joint.inspect` give it: ratio per degree, offset in b's
/// units, a coupling's definition (and why it can't be worked out), a cam's lift in place of a ratio.
fn link_json(s: &Session, st: &solidcraft_doc::ModelState, l: &MotionLink) -> Value {
    let angle = is_angle(s, l.b, l.ib);
    let offset = if angle { l.offset.to_degrees() } else { l.offset };
    let ratio = l.ratio / ratio_scale(s, l.a, l.ia, l.b, l.ib);
    let mut v = json!({"a": l.a, "ia": l.ia, "b": l.b, "ib": l.ib, "ratio": ratio, "offset": offset, "unit": if angle { "deg" } else { "mm" }});
    if let Some(c) = &l.coupling {
        v["coupling"] = json!(c);
        if let Some(Err(e)) = joints::coupling_values(&s.doc, st, l) {
            v["error"] = json!(e.to_string());
        }
    }
    // A cam's follower follows its table, not a ratio: its travel instead.
    if matches!(l.coupling, Some(Coupling::Cam(_)))
        && let Some(o) = v.as_object_mut()
    {
        o.remove("ratio");
        o.insert("lift".into(), lift_json(l, angle));
    }
    v
}

fn solve_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let sol = resolve(s);
    Ok(sol_json(&sol, p))
}

/// Index pairs `(i, j)`, `i < j`, whose boxes overlap or touch (sort and sweep along X), at most
/// `limit` of them, plus how many more there were.
pub(crate) fn overlapping_pairs(boxes: &[Aabb3], limit: usize) -> (Vec<(usize, usize)>, usize) {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| match (boxes.get(a), boxes.get(b)) {
        (Some(a), Some(b)) => a.min.x.total_cmp(&b.min.x),
        _ => std::cmp::Ordering::Equal,
    });
    let (mut pairs, mut skipped) = (Vec::new(), 0);
    for (k, &i) in order.iter().enumerate() {
        let Some(a) = boxes.get(i) else { continue };
        for &j in order.iter().skip(k + 1) {
            let Some(b) = boxes.get(j) else { continue };
            // Sorted by min.x: once one starts past `a`, all later ones do.
            if b.min.x > a.max.x {
                break;
            }
            if b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z {
                continue;
            }
            if pairs.len() < limit {
                pairs.push((i.min(j), i.max(j)));
            } else {
                skipped += 1;
            }
        }
    }
    pairs.sort_unstable();
    (pairs, skipped)
}

#[cfg(test)]
#[path = "joints_as_built_tests.rs"]
mod as_built_tests;
#[cfg(test)]
#[path = "joint_edge_tests.rs"]
mod edge_tests;
#[cfg(test)]
#[path = "joints_exprs_tests.rs"]
mod exprs_tests;
#[cfg(test)]
#[path = "joints_face_tests.rs"]
pub(crate) mod face_tests;
#[cfg(test)]
#[path = "joint_frames_tests.rs"]
mod frames_tests;
#[cfg(test)]
#[path = "joints_point_x_tests.rs"]
mod point_x_tests;
#[cfg(test)]
#[path = "joints_reach_tests.rs"]
mod reach_tests;
#[cfg(test)]
#[path = "joints_relations_tests.rs"]
mod relations_tests;
#[cfg(test)]
#[path = "joints_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "joints_turn_tests.rs"]
mod turn_tests;

/// Per-thread work counts for ordinary engine regression tests; no production instrumentation.
#[cfg(test)]
pub(super) mod solve_counts {
    use solidcraft_doc::joints::Analysis;
    use std::cell::Cell;

    thread_local! {
        static COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    pub fn record(analysis: Analysis) {
        COUNTS.with(|c| {
            let (all, full) = c.get();
            c.set((all + 1, full + usize::from(analysis == Analysis::DegreesOfFreedom)));
        });
    }

    pub fn take() -> (usize, usize) {
        COUNTS.with(|c| c.replace((0, 0)))
    }
}
