//! Inspect Joint (`joint.inspect`): what each joint is defined by and attached to — its two
//! origins (the occurrence, the snap as saved, and the feature, face, edge or sketch curve each
//! persistent name refers to), whether each origin is found today and why not, its kind, offset,
//! angle, flip, limits, values, expressions and the motion links it takes part in. Reads only.

use serde_json::{Value, json};
use solidcraft_doc::joints::{self, Joint, JointOrigin, Snap, XRef};
use solidcraft_doc::naming::{legacy_name, strip_piece};
use solidcraft_doc::{Document, MAX_PLACEMENT_DEPTH, ModelState};

use super::{component_of, joint_id, link_json, named_values, occurrence_name};
use crate::{Result, Session};

pub(super) fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.inspect";
    let ids: Vec<u64> = match p.get("joint") {
        None | Some(Value::Null) => s.doc.assembly.joints.iter().map(|j| j.id).collect(),
        v => vec![joint_id(s, v, cmd)?],
    };
    let st = s.model.state();
    let sol = joints::solve(&s.doc, &st);
    let vals = joints::joint_values(&s.doc.assembly);
    let out: Vec<Value> = ids
        .iter()
        .filter_map(|id| s.doc.assembly.joints.iter().find(|j| j.id == *id))
        .map(|j| {
            let q = vals.get(&j.id).map(Vec::as_slice).unwrap_or(&j.values);
            joint_json(s, &st, &sol, j, q)
        })
        .collect();
    Ok(json!({ "joints": out }))
}

fn joint_json(s: &Session, st: &ModelState, sol: &joints::Solution, j: &Joint, q: &[f64]) -> Value {
    let links: Vec<Value> = s
        .doc
        .assembly
        .links
        .iter()
        .enumerate()
        .filter(|(_, l)| l.a == j.id || l.b == j.id)
        .map(|(i, l)| {
            let mut v = link_json(s, st, l);
            v["index"] = json!(i);
            v["role"] = json!(if l.b == j.id { "driven" } else { "driver" });
            v
        })
        .collect();
    let why = |list: &[(u64, String, String)]| list.iter().filter(|(id, ..)| *id == j.id).map(|(.., why)| why.clone()).collect::<Vec<_>>();
    let mut v = json!({
        "id": j.id,
        "name": j.name,
        "type": j.kind,
        "suppressed": j.suppressed,
        "visible": s.joint_visible(j.id),
        "flip": j.flip,
        "offset": j.offset,
        "angle": j.angle.to_degrees(),
        "values": named_values(j, q),
        "dofs": j.kind.dofs(),
        "a": side_json(s, st, &j.a),
        "b": side_json(s, st, &j.b),
        "links": links,
        "errors": why(&sol.errors),
        "conflicts": why(&sol.conflicts),
    });
    if !j.exprs.is_empty() {
        v["expressions"] = json!(j.exprs);
    }
    v
}

/// One of a joint's origins: what it sits on, what it refers to, and where it is now.
fn side_json(s: &Session, st: &ModelState, o: &JointOrigin) -> Value {
    let component = component_of(s, o.occurrence);
    let component_name = if component == 0 {
        "root".to_string()
    } else {
        s.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default()
    };
    let occurrence_name = if o.occurrence == 0 { "root".to_string() } else { occurrence_name(s, o.occurrence) };
    let mut v = json!({
        "occurrence": o.occurrence,
        "occurrence_name": occurrence_name,
        "component": component,
        "component_name": component_name,
        "path": occurrence_path(&s.doc, o.occurrence),
        "snap": o.snap,
        "references": references(&s.doc, &o.snap),
    });
    match s.doc.origin_world(st, o) {
        Ok(m) => {
            v["resolves"] = json!(true);
            v["frame"] = json!({"origin": [m[3][0], m[3][1], m[3][2]], "z": [m[2][0], m[2][1], m[2][2]], "x": [m[0][0], m[0][1], m[0][2]]});
        }
        Err(e) => {
            v["resolves"] = json!(false);
            v["error"] = json!(e.to_string());
        }
    }
    v
}

/// The occurrence names from the top of the assembly down to `occurrence` (`/` between); a
/// nested component's parent is taken at its first placement. Empty for the root.
fn occurrence_path(doc: &Document, occurrence: u64) -> String {
    let mut names = Vec::new();
    let mut id = occurrence;
    for _ in 0..=MAX_PLACEMENT_DEPTH {
        let Some(o) = doc.occurrences.iter().find(|o| o.id == id) else { break };
        names.push(o.name.as_str());
        let Some(parent) = (o.parent != 0).then(|| doc.occurrence_of(o.parent)).flatten() else { break };
        id = parent.id;
    }
    names.reverse();
    names.join("/")
}

/// Every persistent name the snap holds, with what it refers to.
fn references(doc: &Document, snap: &Snap) -> Vec<Value> {
    let mut out = Vec::new();
    let mut face = |role: &str, name: &Option<String>| {
        if let Some(n) = name {
            out.push(with_role(role, face_ref(doc, n)));
        }
    };
    match snap {
        Snap::FaceCenter { face: f, x, .. } => {
            face("face", f);
            if let Some(XRef::Face { face: n, .. }) = x {
                face("x", &Some(n.clone()));
            }
        }
        Snap::CircleCenter { face: f, end, x, .. } => {
            face("face", f);
            face("end", end);
            if let Some(XRef::Face { face: n, .. }) = x {
                face("x", &Some(n.clone()));
            }
        }
        Snap::Edge { edge, face: f, corner, .. } => {
            if let Some(e) = edge {
                out.push(with_role("edge", edge_ref(doc, e)));
            }
            if let Some(n) = f {
                out.push(with_role("face", face_ref(doc, n)));
            }
            if let Some(e) = corner {
                out.push(with_role("corner", edge_ref(doc, e)));
            }
        }
        Snap::Frame { .. } | Snap::Point { .. } => {}
    }
    out
}

fn with_role(role: &str, mut v: Value) -> Value {
    v["role"] = json!(role);
    v
}

/// What a face's persistent name refers to: `F<feature>:<role>` names the feature that made the
/// face, and a side `side:<curve>` (or a later profile's `side:s<k>.<curve>`) the sketch curve
/// it was swept from. Names of another form are given as they are.
fn face_ref(doc: &Document, name: &str) -> Value {
    let mut v = json!({ "name": name });
    let bare = match legacy_name(name) {
        Some(n) => {
            v["unverified"] = json!(true);
            n
        }
        None => name,
    };
    let stem = strip_piece(bare);
    let Some((fid, role)) = stem.strip_prefix('F').and_then(|r| r.split_once(':')).and_then(|(id, role)| Some((id.parse::<u64>().ok()?, role)))
    else {
        return v;
    };
    v["role_in_feature"] = json!(role);
    let Some(f) = doc.feature(fid) else {
        v["feature"] = json!({"id": fid, "error": format!("feature {fid} is gone")});
        return v;
    };
    v["feature"] = json!({"id": f.id, "name": f.name, "type": f.kind.type_name()});
    if let Some(curve) = role.strip_prefix("side:") {
        // A later profile's side is `s<k>.<curve>`.
        let curve = curve
            .split_once('.')
            .filter(|(k, _)| k.strip_prefix('s').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
            .map_or(curve, |(_, c)| c);
        let found = f.kind.input_sketches().into_iter().find_map(|id| {
            // design-only: a joint names a curve of one of the design's sketches.
            let sk = doc.sketch(id).ok()?;
            let c = sk.curves.iter().find(|c| c.id == curve)?;
            let curve_type = serde_json::to_value(&c.kind).ok().and_then(|k| k.get("type").cloned()).unwrap_or(Value::Null);
            let name = doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
            Some(json!({"id": id, "name": name, "curve": curve, "curve_type": curve_type}))
        });
        v["sketch"] = found.unwrap_or_else(|| json!({"curve": curve, "error": format!("no sketch of {} has a curve `{curve}`", f.name)}));
    }
    v
}

/// An edge's persistent name (`<face A>|<face B>`, `~k` for one of several between them) with
/// what each of its faces refers to.
fn edge_ref(doc: &Document, name: &str) -> Value {
    let bare = legacy_name(name).unwrap_or(name);
    let bare = bare.rsplit_once('~').filter(|(_, k)| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())).map_or(bare, |(e, _)| e);
    // Split at the `|` outside a piece's `#[…]`.
    let mut depth = 0i32;
    let split = bare.char_indices().find(|(_, c)| {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        *c == '|' && depth == 0
    });
    let faces: Vec<Value> = match split {
        Some((i, _)) => [bare.get(..i), bare.get(i + 1..)].into_iter().flatten().map(|f| face_ref(doc, f)).collect(),
        None => Vec::new(),
    };
    let mut v = json!({"name": name, "faces": faces});
    if legacy_name(name).is_some() {
        v["unverified"] = json!(true);
    }
    v
}
