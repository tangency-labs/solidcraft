//! Heat-Set Insert (#12378): a blind hole sized for a heat-set threaded insert, drilled after the
//! wall around the insert and the floor below it are measured on the body's exact geometry.
//!
//! The material is measured against the air around it: the part of a box around the insert that
//! is not the body, cut at the face the hole starts on. The wall is thick enough when a cylinder
//! `min_wall` wider than the insert, over its length, shares no volume with the air between the
//! face and the hole's bottom; the floor, when a disc `min_floor` thick under the insert shares
//! none with the air below the bottom. Both are exact Booleans; a thin one is measured by halving.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, PointClass};

use super::CommandSpec;
use crate::params::{bad, num, str_, string_list, vec3};
use crate::schema::heat_insert::{
    HeatInsertParams, HeatInsertPartsParams, HeatInsertPartsResult, HeatInsertResult, HeatInsertSizesParams, HeatInsertSizesResult, InsertHole,
    InsertSizeInfo, MAX_INSERTS,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const HEAT_INSERT_CONTRACT: TypedContract<HeatInsertParams> = TypedContract::new();
const HEAT_INSERT_SIZES_CONTRACT: TypedContract<HeatInsertSizesParams> = TypedContract::new();
const HEAT_INSERT_PARTS_CONTRACT: TypedContract<HeatInsertPartsParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("plastic.heat_insert", "Heat-Set Insert", crate::checked!("plastic.heat_insert", heat_insert))
        .at("PLASTIC", "CREATE")
        .icon("heat_insert")
        .contract(&HEAT_INSERT_CONTRACT)
        .output(output_schema_for::<HeatInsertResult>)
        .params(
            "position: [x,y,z] on a flat face, or a list of them | sketch + points? (the sketch's points; one Hole feature that follows them); size?: \"M3x5.7\", \"#4-40x5.7\", \"M3\" (see plastic.heat_insert_sizes); body?; \
             outer_diameter?, length?, hole_diameter?, min_wall?, min_floor? (default min_wall); extra_depth? (default 1) | depth?; \
             part?: bool (also place a brass insert part in each hole: one component the BOM counts, placed once per hole; needs size); \
             chamfer?: width in mm | true (a 45° lead-in at the mouth, at most half the hole's depth; true: out to the insert's outer diameter; one wider than the insert is part of the wall check); \
             check?: stop|warn|off (default stop: refuse a thin wall or floor; every point is checked before any hole is drilled); name? → holes: [{position, body, feature, name, wall?, floor?}]",
        ),
    CommandSpec::new("plastic.heat_insert_parts", "Update Heat-Set Insert Parts", crate::checked!("plastic.heat_insert_parts", heat_insert_parts))
        .icon("heat_insert")
        .contract(&HEAT_INSERT_PARTS_CONTRACT)
        .output(output_schema_for::<HeatInsertPartsResult>)
        .params("feature: id|name of a Heat-Set Insert → component?, occurrences, joints: replaces the insert parts it placed to match it (size, holes); none with its `part` off"),
    CommandSpec::new("plastic.heat_insert_sizes", "Heat-Set Insert Sizes", crate::checked!("plastic.heat_insert_sizes", sizes))
        .icon("inspect")
        .noundo()
        .contract(&HEAT_INSERT_SIZES_CONTRACT)
        .output(output_schema_for::<HeatInsertSizesResult>)
        .params("thread?: \"M3\" → the built-in insert sizes: id, thread, outer_diameter, length, hole_diameter, min_wall (mm)"),
];

/// One built-in insert size, in mm.
pub struct InsertSize {
    /// The size's id, which `plastic.heat_insert`'s `size` takes.
    pub id: &'static str,
    /// The thread the insert carries.
    pub thread: &'static str,
    /// The insert's outer diameter.
    pub outer: f64,
    /// The insert's length.
    pub length: f64,
    /// The least wall around the insert.
    pub min_wall: f64,
    /// The recommended hole diameter.
    pub hole: f64,
    /// The size a bare thread name means.
    standard: bool,
}

const fn size(id: &'static str, thread: &'static str, outer: f64, length: f64, min_wall: f64, hole: f64, standard: bool) -> InsertSize {
    InsertSize { id, thread, outer, length, min_wall, hole, standard }
}

/// Starting points from manufacturers' published dimensions (see ATTRIBUTION); every value can be
/// changed in the call.
const SIZES: &[InsertSize] = &[
    size("M2x4", "M2", 3.6, 4.0, 1.3, 3.2, true),
    size("M2.5x5.7", "M2.5", 4.6, 5.7, 1.6, 4.0, true),
    size("M3x4", "M3", 4.6, 4.0, 1.6, 4.0, false),
    size("M3x4-5", "M3", 5.0, 4.0, 1.3, 4.4, false),
    size("M3x5.7", "M3", 4.6, 5.7, 1.6, 4.0, true),
    size("M4x4", "M4", 6.3, 4.0, 2.1, 5.6, false),
    size("M4x8.1", "M4", 6.3, 8.1, 2.1, 5.6, true),
    size("M5x5.8", "M5", 7.1, 5.8, 2.6, 6.4, false),
    size("M5x9.5", "M5", 7.1, 9.5, 2.6, 6.4, true),
    size("M6x12.7", "M6", 8.7, 12.7, 3.3, 8.0, true),
    size("M8x12.7", "M8", 10.1, 12.7, 4.5, 9.6, true),
    size("#2-56x4", "#2-56", 3.6, 4.0, 1.3, 3.2, true),
    size("#4-40x5.7", "#4-40", 4.6, 5.7, 1.6, 4.0, true),
    size("#8-32x8.1", "#8-32", 6.3, 8.1, 2.1, 5.6, true),
    size("#10-24x9.5", "#10-24", 7.1, 9.5, 2.6, 6.4, true),
    size("1/4-20x12.7", "1/4-20", 8.7, 12.7, 3.3, 8.0, true),
    size("3/8-16x12.7", "3/8-16", 12.6, 12.7, 6.0, 11.9, true),
];

/// A size or thread name as the tables compare it: lower case, no spaces or quotes, `x` for `×`
/// and `.` for `,`.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '"')
        .map(|c| match c {
            '×' => 'x',
            ',' => '.',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// Every built-in size, in the table's order.
pub fn all_sizes() -> impl Iterator<Item = &'static InsertSize> {
    SIZES.iter()
}

/// A built-in size by its id, or a thread's standard size by the thread's name.
pub(super) fn find_size(name: &str) -> Option<&'static InsertSize> {
    let k = key(name);
    SIZES.iter().find(|s| key(s.id) == k).or_else(|| SIZES.iter().find(|s| s.standard && key(s.thread) == k))
}

fn sizes(_s: &mut Session, p: &Value) -> Result<Value> {
    let only = str_(p, "thread").map(key);
    let list = SIZES
        .iter()
        .filter(|s| only.as_deref().is_none_or(|t| key(s.thread) == t))
        .map(|s| InsertSizeInfo {
            id: s.id.into(),
            thread: s.thread.into(),
            outer_diameter: s.outer,
            length: s.length,
            hole_diameter: s.hole,
            min_wall: s.min_wall,
            standard: s.standard,
        })
        .collect();
    serde_json::to_value(HeatInsertSizesResult { sizes: list, param_warnings: Vec::new() }).map_err(|e| EngineError::Other(e.to_string()))
}

/// A length the call gives or the size holds: finite, positive and at most a metre.
fn length(p: &Value, k: &str, from_size: Option<f64>, cmd: &str) -> Result<f64> {
    let v = match p.get(k) {
        None | Some(Value::Null) => from_size.ok_or_else(|| bad(cmd, format!("give `{k}` or a `size`")))?,
        Some(_) => num(p, k).ok_or_else(|| bad(cmd, format!("`{k}` must be a number of mm")))?,
    };
    if v.is_finite() && v > 0.0 && v <= 1000.0 { Ok(v) } else { Err(bad(cmd, format!("`{k}` must be more than 0 and at most 1000 mm"))) }
}

fn heat_insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "plastic.heat_insert";
    let (points, sketch) = positions(s, p, cmd)?;
    let size = match str_(p, "size") {
        None => None,
        Some(n) => Some(find_size(n).ok_or_else(|| bad(cmd, format!("no insert size `{n}` (plastic.heat_insert_sizes lists them)")))?),
    };
    let outer = length(p, "outer_diameter", size.map(|z| z.outer), cmd)?;
    let insert_length = length(p, "length", size.map(|z| z.length), cmd)?;
    let hole = length(p, "hole_diameter", size.map(|z| z.hole), cmd)?;
    let min_wall = length(p, "min_wall", size.map(|z| z.min_wall), cmd)?;
    let min_floor = length(p, "min_floor", Some(min_wall), cmd)?;
    let depth = match p.get("depth") {
        None | Some(Value::Null) => insert_length + length(p, "extra_depth", Some(1.0), cmd)?,
        Some(_) => length(p, "depth", None, cmd)?,
    };
    if hole > outer {
        return Err(bad(cmd, format!("the hole ({hole} mm) is wider than the insert ({outer} mm)")));
    }
    if depth < insert_length {
        return Err(bad(cmd, format!("the hole ({depth} mm deep) is shorter than the insert ({insert_length} mm)")));
    }
    // A 45° lead-in at the hole's mouth, as wide across as given; `true`: out to the insert's
    // outer diameter. It takes at most half the hole's depth; where it is wider than the insert,
    // the wall check measures from its rim too.
    let to_outer = (outer - hole) / 2.0;
    let deepest = depth / 2.0;
    let chamfer = match p.get("chamfer") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) if to_outer > 1e-6 => Some(to_outer.min(deepest)),
        Some(Value::Bool(true)) => return Err(bad(cmd, "`chamfer: true` needs a hole narrower than the insert (or give a width)")),
        Some(_) => match num(p, "chamfer") {
            Some(c) if c.is_finite() && c > 0.0 && c <= deepest + 1e-9 => Some(c),
            Some(c) if c.is_finite() && c > 0.0 => {
                return Err(bad(cmd, format!("the chamfer ({c} mm) is deeper than half the hole (at most {deepest:.2} mm)")));
            }
            _ => return Err(bad(cmd, "`chamfer` must be true or a width in mm, more than 0")),
        },
    };
    // The inserts themselves, placed in the holes as one part of its own (a component the bill
    // of materials counts, placed once per hole): from the size table, so it needs a size.
    let part = match p.get("part") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(size.ok_or_else(|| bad(cmd, "`part` needs a `size` (the part is built from the size table)"))?),
        Some(_) => return Err(bad(cmd, "`part` must be true or false")),
    };
    let check = match str_(p, "check").map(str::to_ascii_lowercase).as_deref() {
        None | Some("stop") => Some(true),
        Some("warn") => Some(false),
        Some("off") => None,
        Some(o) => return Err(bad(cmd, format!("unknown check `{o}` (stop, warn or off)"))),
    };

    // Every point is placed and checked before anything changes.
    let st = s.model.state();
    let mut placed = Vec::with_capacity(points.len());
    for at in &points {
        let (body_name, normal) = entry_face(&st, *at, str_(p, "body"), cmd).map_err(|e| at_point(e, *at, points.len()))?;
        let body =
            st.bodies.iter().find(|b| b.name == body_name).map(|b| b.body.clone()).ok_or_else(|| bad(cmd, format!("no body `{body_name}`")))?;
        placed.push((*at, body_name, -normal, body));
    }
    drop(st);

    // The chamfer's rim, where it is wider than the insert: its radius and depth.
    let mouth = chamfer.map(|c| (hole / 2.0 + c, c)).filter(|(r, _)| *r > outer / 2.0);
    let mut warnings = Vec::new();
    let mut measured = vec![(None, None); placed.len()];
    if let Some(stop) = check {
        let mut thin = Vec::new();
        for ((at, _, into, body), m) in placed.iter().zip(measured.iter_mut()) {
            *m = measure(body, *at, *into, (outer / 2.0, insert_length), mouth, depth, min_wall, min_floor)?;
            let whose = if points.len() > 1 { format!("at [{}, {}, {}], ", at.x, at.y, at.z) } else { String::new() };
            if let Some(w) = m.0 {
                thin.push(format!("{whose}the wall around the insert is {w:.2} mm; this insert needs at least {min_wall} mm"));
            }
            if let Some(f) = m.1 {
                thin.push(format!("{whose}the floor below the hole is {f:.2} mm; this insert needs at least {min_floor} mm"));
            }
        }
        if stop && !thin.is_empty() {
            return Err(bad(cmd, format!("{} (check: warn drills it anyway)", thin.join("; "))));
        }
        warnings = thin;
    }

    let hole_params = |extra: Value| -> Value {
        let mut hp = json!({"diameter": hole, "depth": depth});
        if let (Some(o), Value::Object(e)) = (hp.as_object_mut(), extra) {
            o.extend(e);
            // A 90° countersink is a 45° chamfer.
            if let Some(c) = chamfer {
                o.insert("type".into(), json!("countersink"));
                o.insert("cs_diameter".into(), json!(hole + 2.0 * c));
            }
        }
        hp
    };
    // What the feature keeps of the insert: the check it runs whenever it is built, and the parts.
    let spec = solidcraft_doc::HeatInsert {
        size: size.map(|z| z.id.to_string()).unwrap_or_default(),
        outer_diameter: outer,
        length: insert_length,
        min_wall,
        min_floor,
        part: part.is_some(),
        component: None,
    };
    let made = |r: &Value| {
        (r.get("feature").and_then(Value::as_u64).unwrap_or_default(), r.get("name").and_then(Value::as_str).unwrap_or_default().to_string())
    };
    let mut features: Vec<(u64, String)> = vec![(0, String::new()); placed.len()];
    match &sketch {
        // At the points of a sketch: one feature that follows them.
        Some((sk, ids)) => {
            let mut bodies: Vec<&str> = placed.iter().map(|(_, b, _, _)| b.as_str()).collect();
            bodies.sort_unstable();
            bodies.dedup();
            let mut extra = json!({"sketch": sk, "points": ids, "targets": bodies});
            if let (Some(n), Some(o)) = (str_(p, "name"), extra.as_object_mut()) {
                o.insert("name".into(), json!(n));
            }
            let one = made(&super::features::hole_with(s, &hole_params(extra), Vec::new(), Some(spec.clone()))?);
            features.fill(one);
        }
        // At points on faces: one feature for the points of each face (a body and a direction).
        None => {
            let mut groups: Vec<Vec<usize>> = Vec::new();
            for (i, (_, body, into, _)) in placed.iter().enumerate() {
                match groups
                    .iter_mut()
                    .find(|g| g.first().and_then(|&k| placed.get(k)).is_some_and(|(_, b, d, _)| b == body && d.dot(*into) > 1.0 - 1e-9))
                {
                    Some(g) => g.push(i),
                    None => groups.push(vec![i]),
                }
            }
            for (n, group) in groups.iter().enumerate() {
                let Some((at, body_name, into, _)) = group.first().and_then(|&k| placed.get(k)) else { continue };
                let mut extra = json!({"position": [at.x, at.y, at.z], "direction": [into.x, into.y, into.z], "targets": [body_name]});
                if let (Some(name), Some(o)) = (str_(p, "name"), extra.as_object_mut()) {
                    o.insert("name".into(), json!(if n == 0 { name.to_string() } else { format!("{name} {}", n + 1) }));
                }
                let more: Vec<Vec3> = group.iter().skip(1).filter_map(|&k| placed.get(k).map(|(a, _, _, _)| *a)).collect();
                let one = made(&super::features::hole_with(s, &hole_params(extra), more, Some(spec.clone()))?);
                for &k in group {
                    if let Some(f) = features.get_mut(k) {
                        *f = one.clone();
                    }
                }
            }
        }
    }
    // The insert parts, placed by each feature; not while Edit Feature tries the new definition
    // on a copy of the design (`timeline.redefine`, which `plastic.heat_insert_parts` follows),
    // nor in a live preview.
    let mut inserted = (None, Vec::new(), Vec::new());
    if part.is_some() && !s.features_only {
        let mut done: Vec<u64> = Vec::new();
        for (f, _) in &features {
            if done.contains(f) {
                continue;
            }
            done.push(*f);
            let (c, o, j) = sync_parts(s, *f)?;
            inserted.0 = inserted.0.or(c);
            inserted.1.extend(o);
            inserted.2.extend(j);
        }
    }
    let first_feature = features.first().map(|f| f.0).unwrap_or_default();
    let holes = placed
        .into_iter()
        .zip(measured)
        .zip(features)
        .map(|(((at, body, _, _), (wall, floor)), (feature, name))| InsertHole { position: [at.x, at.y, at.z], body, feature, name, wall, floor })
        .collect();
    let reply = HeatInsertResult {
        holes,
        size: size.map(|z| z.id.to_string()),
        outer_diameter: outer,
        length: insert_length,
        hole_diameter: hole,
        depth,
        chamfer,
        min_wall,
        min_floor,
        checked: check.is_some(),
        feature: first_feature,
        component: inserted.0,
        occurrences: (!inserted.1.is_empty()).then_some(inserted.1),
        joints: (!inserted.2.is_empty()).then_some(inserted.2),
        warnings,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Make the insert parts of Heat-Set Insert `feature` match it: the component it placed before
/// goes, and with `part` on, one component of its size is placed once in each of its holes, seated
/// on a point of each hole's wall. The feature keeps the new component. Returns it, its
/// occurrences and its joints.
fn sync_parts(s: &mut Session, feature: u64) -> Result<(Option<u64>, Vec<u64>, Vec<u64>)> {
    let cmd = "plastic.heat_insert_parts";
    let f = s.doc.feature(feature).cloned().ok_or_else(|| bad(cmd, format!("no feature {feature}")))?;
    let FeatureKind::Hole { insert: Some(spec), position, more, points, direction, diameter, depth, hole: kind, .. } = &f.kind else {
        return Err(bad(cmd, format!("`{}` is not a Heat-Set Insert", f.name)));
    };
    if let Some(old) = spec.component.filter(|c| s.doc.components.iter().any(|x| x.id == *c)) {
        super::component::delete(s, &json!({"component": old}))?;
    }
    let set = |s: &mut Session, component: Option<u64>| {
        if let Some(FeatureKind::Hole { insert: Some(spec), .. }) = s.doc_mut().feature_mut(feature).map(|f| &mut f.kind) {
            spec.component = component;
        }
    };
    if !spec.part {
        set(s, None);
        return Ok((None, Vec::new(), Vec::new()));
    }
    if spec.size.is_empty() {
        return Err(bad(cmd, "the insert parts need a size from the table"));
    }
    let hole = s.doc.eval(diameter, Kind::Length).map_err(|e| bad(cmd, e.to_string()))?;
    let depth = match depth {
        Some(d) => s.doc.eval(d, Kind::Length).map_err(|e| bad(cmd, e.to_string()))?,
        None => spec.length + 1.0,
    };
    let chamfer = match kind {
        solidcraft_doc::HoleKind::Countersink { cs_diameter, .. } => s.doc.eval(cs_diameter, Kind::Length).ok().map(|c| (c - hole) / 2.0),
        _ => None,
    };
    let into = direction.0;
    let at: Vec<Vec3> = match points {
        Some(sp) if !sp.single => {
            let st = s.model.state();
            let ss = st.sketch(sp.sketch.0).ok_or_else(|| bad(cmd, "the feature's sketch is not evaluated"))?;
            sp.ids.iter().filter_map(|id| ss.sketch.points.iter().find(|q| &q.id == id)).map(|q| ss.plane.to_world(q.pos)).collect()
        }
        _ => std::iter::once(position.0).chain(more.iter().map(|m| m.0)).collect(),
    };
    // A point on each hole's wall, below a chamfer, as Insert Part seats a screw.
    let across = into.cross(if into.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }).normalized().unwrap_or(Vec3::X);
    let walls: Vec<[f64; 3]> =
        at.iter().map(|a| *a + into * (chamfer.unwrap_or(0.0) + (depth / 2.0).min(0.5)) + across * (hole / 2.0)).map(|w| [w.x, w.y, w.z]).collect();
    let r = super::parts::insert_atomic(s, &json!({"family": "heat_set_insert", "size": spec.size, "at": walls}))?;
    let component = r.get("component").and_then(Value::as_u64);
    let ids = |k: &str| r.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect::<Vec<_>>()).unwrap_or_default();
    // The feature is the one entry in the History: how the brass part is built stays out of it.
    if let Some(c) = component.and_then(|c| s.doc_mut().components.iter_mut().find(|x| x.id == c)) {
        c.history_hidden = true;
    }
    set(s, component);
    Ok((component, ids("occurrences"), ids("joints")))
}

fn heat_insert_parts(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "plastic.heat_insert_parts";
    let key = match p.get("feature") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(t)) => t.clone(),
        _ => return Err(bad(cmd, "`feature` must be the Heat-Set Insert's id or name")),
    };
    let feature = s.doc.find_feature(&key).map(|f| f.id).ok_or_else(|| bad(cmd, format!("no feature `{key}`")))?;
    // A live preview shows the feature alone.
    let (component, occurrences, joints) = if s.features_only { (None, Vec::new(), Vec::new()) } else { sync_parts(s, feature)? };
    serde_json::to_value(HeatInsertPartsResult { component, occurrences, joints, param_warnings: Vec::new() })
        .map_err(|e| EngineError::Other(e.to_string()))
}

/// Where the inserts go, in world coordinates: the points of `position`, or of `sketch` (its
/// `points`, or every standalone point and circle centre), with the sketch and its point ids.
#[allow(clippy::type_complexity)]
fn positions(s: &Session, p: &Value, cmd: &str) -> Result<(Vec<Vec3>, Option<(Value, Vec<String>)>)> {
    let points = match (p.get("position"), p.get("sketch")) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `position` or `sketch`, not both")),
        (None, None) => return Err(bad(cmd, "give `position` (a point on a flat face, or a list of them) or `sketch`")),
        (Some(v), None) => {
            let one = |v: &Value| vec3(v).ok_or_else(|| bad(cmd, "`position` must be a point [x, y, z] on a flat face, or a list of them"));
            return match v {
                Value::Array(list) if list.first().is_some_and(Value::is_array) => {
                    if list.len() > MAX_INSERTS {
                        return Err(bad(cmd, format!("`position` takes at most {MAX_INSERTS} points")));
                    }
                    Ok((list.iter().map(one).collect::<Result<_>>()?, None))
                }
                v => Ok((vec![one(v)?], None)),
            };
        }
        (None, Some(v)) => {
            let sketch = super::features::feature_sketch(s, p, cmd)?;
            let ids = if p.get("points").is_some() { string_list(cmd, p, "points")? } else { super::features::sketch_hole_points(s, sketch) };
            if ids.is_empty() || ids.len() > MAX_INSERTS {
                return Err(bad(cmd, format!("the sketch must give 1…{MAX_INSERTS} points (standalone points or circle centres, or `points`)")));
            }
            let st = s.model.state();
            let ss = st.sketch(sketch).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
            let at = ids
                .iter()
                .map(|id| {
                    ss.sketch
                        .points
                        .iter()
                        .find(|q| &q.id == id)
                        .map(|q| ss.plane.to_world(q.pos))
                        .ok_or_else(|| bad(cmd, format!("no point `{id}` in the sketch")))
                })
                .collect::<Result<Vec<_>>>()?;
            (at, Some((v.clone(), ids)))
        }
    };
    Ok(points)
}

/// An error about one of several points, saying which.
fn at_point(e: EngineError, at: Vec3, of: usize) -> EngineError {
    if of > 1 { bad("plastic.heat_insert", format!("at [{}, {}, {}]: {e}", at.x, at.y, at.z)) } else { e }
}

/// The solid body whose flat face holds `at`, and that face's outward normal, decided exactly: a
/// planar face through the point with material just behind it and none just in front.
fn entry_face(st: &solidcraft_doc::ModelState, at: Vec3, only: Option<&str>, cmd: &str) -> Result<(String, Vec3)> {
    let mut found: Vec<(String, Vec3)> = Vec::new();
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh() && only.is_none_or(|n| b.name == n)) {
        if b.body.classify_point(at)? != PointClass::OnBoundary {
            continue;
        }
        let step = (b.body.record().tolerance * 100.0).max(1e-4);
        for i in 0..b.body.face_count() {
            let Some(plane) = b.body.planar_face(i) else { continue };
            let Some(n) = plane.normal().normalized() else { continue };
            if (at - plane.origin).dot(n).abs() > step
                || b.body.classify_point(at + n * step)? != PointClass::Outside
                || b.body.classify_point(at - n * step)? != PointClass::Inside
            {
                continue;
            }
            if !found.iter().any(|(name, m)| *name == b.name && m.dot(n) > 1.0 - 1e-9) {
                found.push((b.name.clone(), n));
            }
        }
    }
    match found.len() {
        1 => found.pop().ok_or_else(|| bad(cmd, "no flat face at `position`")),
        0 => Err(bad(cmd, "`position` is not on a flat face of a solid body")),
        _ => Err(bad(cmd, "`position` is on an edge or on several bodies (move it onto one flat face, or give `body`)")),
    }
}

/// The wall around the insert (`(outer radius, length)`) and around a chamfer wider than it
/// (`mouth`: `(rim radius, depth)`), and the floor below the hole, in mm, where they are thinner
/// than `min_wall` and `min_floor` (`None`: at least that thick): the check the feature runs
/// whenever it is built ([`solidcraft_doc::heat_insert::wall_and_floor`]).
#[allow(clippy::too_many_arguments)]
fn measure(
    body: &Body,
    at: Vec3,
    into: Vec3,
    (outer_radius, length): (f64, f64),
    mouth: Option<(f64, f64)>,
    depth: f64,
    min_wall: f64,
    min_floor: f64,
) -> Result<(Option<f64>, Option<f64>)> {
    let place = solidcraft_doc::heat_insert::InsertPlace { at, into, outer_radius, length, mouth, depth, min_wall, min_floor };
    Ok(solidcraft_doc::heat_insert::wall_and_floor(body, &place)?)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcore_record::Vec3;

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    }

    #[test]
    fn an_m3_insert_in_a_thick_block_drills_its_hole_and_measures_the_wall() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
        let before = volume(&mut s);
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [10, 10, 10], "size": "M3"}));
        assert_eq!(r["size"], "M3x5.7");
        assert_eq!(r["depth"], 6.7);
        // The sides are 10 - 2.3 = 7.7 mm from the insert and the floor 10 - 6.7 = 3.3 mm below
        // the hole, both more than the 1.6 mm asked for.
        assert!(r["holes"][0].get("wall").is_none() && r["holes"][0].get("floor").is_none(), "{r}");
        assert_eq!(r["checked"], true);
        let removed = before - volume(&mut s);
        let expected = std::f64::consts::PI * 2.0 * 2.0 * 6.7;
        assert!((removed - expected).abs() / expected < 1e-3, "{removed} vs {expected}");
    }

    #[test]
    fn a_thin_wall_stops_the_insert_and_leaves_the_design_alone() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
        let before = volume(&mut s);
        // 3.1 mm from the side: 3.1 - 2.3 = 0.8 mm of wall, less than 1.6.
        let e = s.execute("plastic.heat_insert", &json!({"position": [3.1, 10, 10], "size": "M3x5.7"})).unwrap_err().to_string();
        assert!(e.contains("0.80 mm") && e.contains("1.6"), "{e}");
        assert!((volume(&mut s) - before).abs() < 1e-9);
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [3.1, 10, 10], "size": "M3x5.7", "check": "warn"}));
        assert!((r["holes"][0]["wall"].as_f64().unwrap_or(f64::NAN) - 0.8).abs() < 2e-3, "{r}");
        // The floor is 10 - 6.7 = 3.3 mm, more than 1.6 mm, though the side is near.
        assert!(r["holes"][0].get("floor").is_none(), "{r}");
        assert_eq!(r["warnings"].as_array().map(Vec::len), Some(1), "{r}");
    }

    #[test]
    fn a_thin_floor_is_found_below_the_hole() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 7.5, "body_name": "Plate"}));
        // 7.5 - 6.7 = 0.8 mm of floor.
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [10, 10, 7.5], "size": "M3", "check": "warn"}));
        assert!((r["holes"][0]["floor"].as_f64().unwrap_or(f64::NAN) - 0.8).abs() < 2e-3, "{r}");
    }

    #[test]
    fn an_insert_at_the_edge_of_a_boss_top_meets_the_air_beside_the_boss() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 3, "body_name": "Base"}));
        run(&mut s, "plastic.boss", json!({"position": [15, 15, 3], "diameter": 8, "height": 10}));
        // The boss is 4 mm in radius around (15, 15): an M3 insert 0.6 mm off its centre leaves
        // 4 - 0.6 - 2.3 = 1.1 mm of wall at the top, where the air is beside the boss, not above.
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [15.6, 15, 13], "size": "M3", "check": "warn"}));
        assert!((r["holes"][0]["wall"].as_f64().unwrap_or(f64::NAN) - 1.1).abs() < 2e-3, "{r}");
        // Centred, the wall is 4 - 2.3 = 1.7 mm, enough for 1.6.
        let mut c = Session::default();
        run(&mut c, "solid.box", json!({"length": 30, "width": 30, "height": 3, "body_name": "Base"}));
        run(&mut c, "plastic.boss", json!({"position": [15, 15, 3], "diameter": 8, "height": 10}));
        let r = run(&mut c, "plastic.heat_insert", json!({"position": [15, 15, 13], "size": "M3"}));
        assert!(r["holes"][0].get("wall").is_none(), "{r}");
    }

    #[test]
    fn a_chamfer_leads_the_insert_in_and_a_wide_one_counts_in_the_wall_check() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
        let before = volume(&mut s);
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [10, 10, 10], "size": "M3", "chamfer": 0.3}));
        assert_eq!(r["chamfer"], 0.3);
        // The Ø4 hole, 6.7 deep, and a 45° cone from Ø4.6 at the face to Ø4 0.3 mm down.
        let (hole, outer, c) = (2.0_f64, 2.3_f64, 0.3_f64);
        let cone = PI * c / 3.0 * (outer * outer + outer * hole + hole * hole) - PI * hole * hole * c;
        let expected = PI * hole * hole * 6.7 + cone;
        let removed = before - volume(&mut s);
        assert!((removed - expected).abs() / expected < 1e-3, "{removed} vs {expected}");
        // `true`: out to the outer diameter, 0.3 mm for an M3.
        let mut t = Session::default();
        run(&mut t, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
        let r = run(&mut t, "plastic.heat_insert", json!({"position": [10, 10, 10], "size": "M3", "chamfer": true}));
        assert!((r["chamfer"].as_f64().unwrap_or(f64::NAN) - 0.3).abs() < 1e-9, "{r}");
        // A 1.2 mm chamfer 4.6 mm from the side: its rim (r 3.2) leaves 1.4 mm of wall, though
        // the insert (r 2.3) leaves 2.3 mm.
        let mut t = Session::default();
        run(&mut t, "solid.box", json!({"length": 20, "width": 30, "height": 10, "body_name": "Block"}));
        let r = run(&mut t, "plastic.heat_insert", json!({"position": [4.6, 10, 10], "size": "M3", "chamfer": 1.2, "check": "warn"}));
        assert!((r["holes"][0]["wall"].as_f64().unwrap_or(f64::NAN) - 1.4).abs() < 2e-3, "{r}");
        let e = t.execute("plastic.heat_insert", &json!({"position": [4.6, 22, 10], "size": "M3", "chamfer": 1.2})).unwrap_err().to_string();
        assert!(e.contains("1.40 mm"), "{e}");
        for (chamfer, word) in [(json!(4), "half the hole"), (json!(-1), "chamfer"), (json!("big"), "chamfer")] {
            let e = t.execute("plastic.heat_insert", &json!({"position": [5, 5, 10], "size": "M3", "chamfer": chamfer})).unwrap_err().to_string();
            assert!(e.contains(word), "{chamfer}: {e}");
        }
    }

    #[test]
    fn several_points_are_all_checked_before_any_hole_is_drilled() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
        let before = volume(&mut s);
        // The second point is 3.1 mm from the side: 0.8 mm of wall. Nothing is drilled.
        let e = s.execute("plastic.heat_insert", &json!({"position": [[10, 10, 10], [3.1, 10, 10]], "size": "M3"})).unwrap_err().to_string();
        assert!(e.contains("at [3.1, 10, 10]") && e.contains("0.80 mm"), "{e}");
        assert!((volume(&mut s) - before).abs() < 1e-9);
        // Three good points on one face: one Heat-Set Insert feature drills them all.
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [[10, 10, 10], [20, 10, 10], [30, 10, 10]], "size": "M3", "name": "Insert"}));
        let holes = r["holes"].as_array().cloned().unwrap_or_default();
        assert_eq!(holes.len(), 3, "{r}");
        assert!(holes.iter().all(|h| h["feature"] == r["feature"] && h["name"] == "Insert"), "{r}");
        let f = s.doc.feature(r["feature"].as_u64().unwrap_or_default()).unwrap();
        assert!(
            matches!(&f.kind, solidcraft_doc::FeatureKind::Hole { insert: Some(i), more, .. } if i.size == "M3x5.7" && more.len() == 2),
            "{:?}",
            f.kind
        );
        assert_eq!(f.kind.base_name(), "HeatInsert");
        let removed = before - volume(&mut s);
        let expected = 3.0 * PI * 2.0 * 2.0 * 6.7;
        assert!((removed - expected).abs() / expected < 1e-3, "{removed} vs {expected}");
    }

    #[test]
    fn the_points_of_a_sketch_on_the_face_get_one_hole_feature_that_follows_them() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
        let before = volume(&mut s);
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Spots", "offset": 10}));
        run(&mut s, "sketch.point", json!({"point": [10, 10], "id": "a"}));
        run(&mut s, "sketch.point", json!({"point": [30, 10], "id": "b"}));
        run(&mut s, "sketch.point", json!({"point": [3.1, 10], "id": "thin"}));
        run(&mut s, "sketch.finish", json!({}));
        // Every point of the sketch: the thin one stops them all.
        let e = s.execute("plastic.heat_insert", &json!({"sketch": "Spots", "size": "M3"})).unwrap_err().to_string();
        assert!(e.contains("0.80 mm"), "{e}");
        let r = run(&mut s, "plastic.heat_insert", json!({"sketch": "Spots", "points": ["a", "b"], "size": "M3"}));
        let holes = r["holes"].as_array().cloned().unwrap_or_default();
        assert_eq!(holes.len(), 2, "{r}");
        assert!(holes.iter().all(|h| h["feature"] == holes[0]["feature"]), "one feature for the sketch: {r}");
        let removed = before - volume(&mut s);
        let expected = 2.0 * PI * 2.0 * 2.0 * 6.7;
        assert!((removed - expected).abs() / expected < 1e-3, "{removed} vs {expected}");
        let e = s.execute("plastic.heat_insert", &json!({"sketch": "Spots", "position": [10, 10, 10], "size": "M3"})).unwrap_err().to_string();
        assert!(e.contains("not both"), "{e}");
        // Move point `a` 5 mm along X: its hole goes with it, and the old spot is filled again.
        let solid_at = |s: &Session, x: f64, y: f64| {
            s.model.state().bodies.iter().any(|b| b.body.classify_point(Vec3::new(x, y, 7.0)).ok() == Some(solidcraft_kernel::PointClass::Inside))
        };
        assert!(!solid_at(&s, 10.0, 10.0) && solid_at(&s, 15.0, 10.0));
        run(&mut s, "sketch.edit", json!({"sketch": "Spots"}));
        run(&mut s, "sketch.move_point", json!({"point": "a", "to": [15, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        assert!(solid_at(&s, 10.0, 10.0) && !solid_at(&s, 15.0, 10.0), "the hole followed its sketch point");
    }

    #[test]
    fn with_part_each_hole_gets_a_brass_insert_the_bill_of_materials_counts() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
        let undo = s.undo.len();
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [[10, 10, 10], [30, 10, 10]], "size": "M3", "part": true}));
        assert_eq!(s.undo.len(), undo + 1, "the holes and their parts are one undo step");
        assert!(r["component"].as_u64().is_some(), "{r}");
        assert_eq!(r["occurrences"].as_array().map(Vec::len), Some(2), "one placement per hole: {r}");
        assert_eq!(r["joints"].as_array().map(Vec::len), Some(2), "{r}");
        // The feature is the History's one entry: the part's sketch and revolve stay out of it.
        let part = r["component"].as_u64().and_then(|c| s.doc.components.iter().find(|x| x.id == c));
        assert!(part.is_some_and(|c| c.history_hidden), "{r}");
        // One row for the two inserts, counted twice.
        let bom = run(&mut s, "assembly.bom", json!({}));
        let rows = bom["rows"].as_array().cloned().unwrap_or_default();
        let insert: Vec<&serde_json::Value> = rows.iter().filter(|r| r["part_number"] == "Heat-set insert M3x5.7").collect();
        assert_eq!(insert.len(), 1, "{bom}");
        assert_eq!(insert.first().map(|r| r["count"].clone()), Some(json!(2)), "{bom}");
        assert_eq!(insert.first().map(|r| r["description"].clone()), Some(json!("Heat-set threaded insert M3x5.7")), "{bom}");
        // Each insert is a brass sleeve, Ø4.6 outside and Ø3 bore, 5.7 long.
        let m = run(&mut s, "inspect.measure", json!({}));
        let sleeves: Vec<&serde_json::Value> =
            m["bodies"].as_array().into_iter().flatten().filter(|b| b["material"].as_str().is_some_and(|x| x == "Brass")).collect();
        assert!(!sleeves.is_empty(), "{m}");
        let expected = PI * (2.3 * 2.3 - 1.5 * 1.5) * 5.7;
        for b in sleeves {
            let v = b["volume_mm3"].as_f64().unwrap_or(f64::NAN);
            assert!((v - expected).abs() / expected < 1e-3, "{v} vs {expected}: {b}");
        }
        // With a chamfer, the insert is seated below it.
        run(&mut s, "plastic.heat_insert", json!({"position": [20, 10, 10], "size": "M3", "part": true, "chamfer": true}));
        let e = s.execute(
            "plastic.heat_insert",
            &json!({"position": [20, 5, 10], "outer_diameter": 5, "length": 5, "hole_diameter": 4.2, "min_wall": 1, "part": true}),
        );
        assert!(e.is_err_and(|e| e.to_string().contains("size")));
    }

    #[test]
    fn edit_feature_changes_the_size_of_every_hole() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 12, "body_name": "Block"}));
        let before = volume(&mut s);
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [[10, 10, 12], [30, 10, 12]], "size": "M3"}));
        let feature = r["feature"].as_u64().unwrap();
        // Edit Feature: M4 instead of M3, as the dialog's OK sends it.
        let params = json!({"position": [[10, 10, 12], [30, 10, 12]], "size": "M4"});
        run(&mut s, "timeline.redefine", json!({"feature": feature, "command": "plastic.heat_insert", "params": params}));
        // Still one feature, now drilling two Ø5.6 holes 9.1 deep.
        let holes: Vec<_> = s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Hole { .. })).collect();
        assert_eq!(holes.len(), 1);
        assert!(matches!(&holes[0].kind, solidcraft_doc::FeatureKind::Hole { insert: Some(i), .. } if i.size == "M4x8.1"));
        let removed = before - volume(&mut s);
        let expected = 2.0 * PI * 2.8 * 2.8 * 9.1;
        assert!((removed - expected).abs() / expected < 1e-3, "{removed} vs {expected}");
    }

    #[test]
    fn edit_feature_changes_the_size_and_the_parts_follow() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 12, "body_name": "Block"}));
        let block = |s: &mut Session| run(s, "inspect.measure", json!({"bodies": ["Block"]}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN);
        let before = block(&mut s);
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [[10, 10, 12], [30, 10, 12]], "size": "M3", "part": true}));
        let feature = r["feature"].as_u64().unwrap();
        let first = r["component"].as_u64().unwrap();
        // Edit Feature: M4 instead of M3, as the dialog's OK sends it.
        let params = json!({"position": [[10, 10, 12], [30, 10, 12]], "size": "M4", "part": true});
        run(&mut s, "timeline.redefine", json!({"feature": feature, "command": "plastic.heat_insert", "params": params}));
        let parts = run(&mut s, "plastic.heat_insert_parts", json!({"feature": feature}));
        // Still one feature, now drilling two Ø5.6 holes 9.1 deep.
        let holes = s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Hole { .. })).count();
        assert_eq!(holes, 1);
        let removed = before - block(&mut s);
        let expected = 2.0 * PI * 2.8 * 2.8 * 9.1;
        assert!((removed - expected).abs() / expected < 1e-3, "{removed} vs {expected}");
        // The M3 parts are gone; one M4 part is placed in both holes, and the feature keeps it.
        let second = parts["component"].as_u64().unwrap();
        assert!(second != first && !s.doc.components.iter().any(|c| c.id == first));
        assert_eq!(parts["occurrences"].as_array().map(Vec::len), Some(2), "{parts}");
        let bom = run(&mut s, "assembly.bom", json!({}));
        assert!(bom["rows"].as_array().into_iter().flatten().any(|r| r["part_number"] == "Heat-set insert M4x8.1" && r["count"] == 2), "{bom}");
        assert!(
            matches!(&s.doc.feature(feature).unwrap().kind, solidcraft_doc::FeatureKind::Hole { insert: Some(i), .. } if i.component == Some(second))
        );
        // Parts off: the feature takes them away.
        let params = json!({"position": [[10, 10, 12], [30, 10, 12]], "size": "M4"});
        run(&mut s, "timeline.redefine", json!({"feature": feature, "command": "plastic.heat_insert", "params": params}));
        run(&mut s, "plastic.heat_insert_parts", json!({"feature": feature}));
        assert!(!s.doc.components.iter().any(|c| c.id == second));
    }

    #[test]
    fn a_sketch_point_moved_too_near_the_side_warns_when_the_feature_is_rebuilt() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Spots", "offset": 10}));
        run(&mut s, "sketch.point", json!({"point": [10, 10], "id": "a"}));
        run(&mut s, "sketch.finish", json!({}));
        let r = run(&mut s, "plastic.heat_insert", json!({"sketch": "Spots", "size": "M3"}));
        let feature = r["feature"].as_u64().unwrap();
        let warning = |s: &Session| s.model.result(feature).and_then(|r| r.warning.clone());
        assert!(warning(&s).is_none());
        // 3.1 mm from the side: 0.8 mm of wall around the insert.
        run(&mut s, "sketch.edit", json!({"sketch": "Spots"}));
        run(&mut s, "sketch.move_point", json!({"point": "a", "to": [3.1, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        let w = warning(&s).unwrap_or_default();
        assert!(w.contains("0.80 mm") && w.contains("1.6"), "{w}");
    }

    #[test]
    fn check_off_drills_without_measuring() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
        let r = run(&mut s, "plastic.heat_insert", json!({"position": [3.1, 10, 10], "size": "M3", "check": "off"}));
        assert_eq!(r["checked"], false);
        assert!(r["holes"][0].get("wall").is_none() && r.get("warnings").is_none(), "{r}");
    }

    #[test]
    fn bad_calls_say_what_is_wrong() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10}));
        for (p, word) in [
            (json!({"position": [10, 10, 10], "size": "M7"}), "M7"),
            (json!({"position": [10, 10, 10]}), "size"),
            (json!({"position": [10, 10, 5], "size": "M3"}), "flat face"),
            (json!({"position": [10, 10, 10], "size": "M3", "hole_diameter": 9}), "wider"),
            (json!({"position": [10, 10, 10], "size": "M3", "depth": 2}), "shorter"),
            (json!({"position": [10, 10, 10], "size": "M3", "check": "maybe"}), "check"),
        ] {
            let e = s.execute("plastic.heat_insert", &p).unwrap_err().to_string();
            assert!(e.contains(word), "{p}: {e}");
        }
    }

    #[test]
    fn the_size_list_has_every_thread_and_one_standard_each() {
        let mut s = Session::default();
        let r = run(&mut s, "plastic.heat_insert_sizes", json!({}));
        assert_eq!(r["sizes"].as_array().map(Vec::len), Some(17));
        let m3 = run(&mut s, "plastic.heat_insert_sizes", json!({"thread": "m3"}));
        let standard: Vec<&Value> = m3["sizes"].as_array().into_iter().flatten().filter(|z| z["standard"] == true).collect();
        assert_eq!(standard.len(), 1);
    }
}
