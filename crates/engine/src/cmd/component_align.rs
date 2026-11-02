//! Aligning component occurrences (`occurrence.align`): moves only the chosen occurrence
//! placements in world coordinates using the same `from` / `to` / `flip` rules as `solid.align`,
//! without modifying shared component geometry across other instances.

use serde_json::{Value, json};
use solidcraft_doc::{IDENTITY, mat_inverse, mat_mul, rigid};
use solidcraft_geom::Vec3;

use super::{occurrence, occurrence_path, valid_position};
use crate::params::{bad, vec3};
use crate::{Result, Session};

const MAX_ALIGNED: usize = 64;

fn opt_bool(p: &Value, key: &str, default: bool, cmd: &str) -> Result<bool> {
    match p.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(bad(cmd, format!("`{key}` must be a boolean"))),
    }
}

fn parse_path(v: &Value, key: &str, cmd: &str) -> Result<Vec<u64>> {
    v.as_array()
        .filter(|a| !a.is_empty() && a.len() <= 64)
        .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
        .ok_or_else(|| bad(cmd, format!("`{key}` must list 1..64 occurrence ids")))
}

fn resolve_paths(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Vec<u64>>> {
    let raw: Vec<Vec<u64>> = if let Some(v) = p.get("paths") {
        if ["occurrences", "occurrence", "path"].into_iter().any(|k| p.get(k).is_some()) {
            return Err(bad(cmd, "give `paths`, `occurrences`, or `occurrence`/`path`, not more than one"));
        }
        let items = v
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= MAX_ALIGNED)
            .ok_or_else(|| bad(cmd, format!("`paths` must list 1..{MAX_ALIGNED} occurrence paths")))?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let path = parse_path(item, "paths", cmd)?;
            s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
            out.push(path);
        }
        out
    } else if let Some(v) = p.get("occurrences") {
        if p.get("occurrence").is_some() || p.get("path").is_some() {
            return Err(bad(cmd, "give `occurrences` or `occurrence`/`path`, not both"));
        }
        let items: Vec<&Value> = match v {
            Value::Number(_) | Value::String(_) => vec![v],
            Value::Array(a) if !a.is_empty() && a.len() <= MAX_ALIGNED => a.iter().collect(),
            _ => return Err(bad(cmd, format!("`occurrences` must be an occurrence id/name or a list of 1..{MAX_ALIGNED} ids/names"))),
        };
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let q = json!({ "occurrence": item });
            let id = occurrence(s, &q, cmd)?;
            out.push(occurrence_path(s, &json!({}), id, cmd)?);
        }
        out
    } else if p.get("occurrence").is_some() {
        let id = occurrence(s, p, cmd)?;
        vec![occurrence_path(s, p, id, cmd)?]
    } else if let Some(v) = p.get("path") {
        let path = parse_path(v, "path", cmd)?;
        s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
        vec![path]
    } else {
        return Err(bad(cmd, "give `occurrence`, `occurrences`, `path` or `paths`"));
    };

    let mut deduped: Vec<Vec<u64>> = Vec::with_capacity(raw.len());
    for path in raw {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }

    // If both an ancestor placement and a placement under it were selected, moving the ancestor
    // already carries the descendant along; skip the descendant so it moves once. Only a path
    // prefix means "under it": another placement of the same component does not carry it.
    let all = deduped.clone();
    deduped.retain(|cand| !all.iter().any(|other| other.len() < cand.len() && cand.starts_with(other)));

    // Two distinct paths ending in the same nested occurrence id share one definition transform.
    for (i, a) in deduped.iter().enumerate() {
        if deduped[..i].iter().any(|b| b.last() == a.last()) {
            return Err(bad(cmd, "cannot align two placements of the same shared child occurrence at once; align their parent occurrences instead"));
        }
    }
    Ok(deduped)
}

fn world_pick(
    s: &Session,
    p: &Value,
    pk: &str,
    fk: &str,
    plane: &Option<solidcraft_doc::PlaneRef>,
    vals: &std::collections::BTreeMap<String, solidcraft_doc::expr::Value>,
    cmd: &str,
) -> Result<(Vec3, Option<Vec3>)> {
    if let Some(pl_ref) = plane {
        if p.get(pk).is_some() || p.get(fk).is_some() {
            return Err(bad(cmd, format!("give `{pk}_plane` or `{pk}`/`{fk}`, not both")));
        }
        let world = s.world_state();
        let pl = s.placed_document().resolve_plane_in(vals, pl_ref, 0, Some(&*world)).map_err(|e| bad(cmd, e.to_string()))?;
        return Ok((pl.origin, Some(pl.normal())));
    }
    if let Some(fv) = p.get(fk) {
        if p.get(pk).is_some() {
            return Err(bad(cmd, format!("give `{pk}` or `{fk}`, not both")));
        }
        let fp = vec3(fv).ok_or_else(|| bad(cmd, format!("`{fk}` must be [x, y, z]")))?;
        let n = super::super::face::face_normal_world(s, fp).ok_or_else(|| bad(cmd, format!("`{fk}` must be on a planar face")))?;
        return Ok((fp, Some(n)));
    }
    if p.get(pk).and_then(Value::as_str).is_some_and(|o| o.eq_ignore_ascii_case("origin")) {
        return Ok((Vec3::ZERO, None));
    }
    if let Some(o) = p.get(pk).and_then(Value::as_object)
        && (o.contains_key("at") || o.contains_key("snap"))
    {
        let at = o.get("at").and_then(vec3).ok_or_else(|| bad(cmd, format!("`{pk}.at` must be [x, y, z]")))?;
        let kind = match o.get("snap") {
            None => "vertex",
            Some(Value::String(k)) if matches!(k.as_str(), "vertex" | "edge_mid" | "face_center" | "circle_center") => k.as_str(),
            _ => return Err(bad(cmd, format!("`{pk}.snap` must be vertex, edge_mid, face_center or circle_center"))),
        };
        return super::super::features_more::snap_point(s, kind, at)
            .map(|q| (q, None))
            .ok_or_else(|| bad(cmd, format!("no {kind} to snap to near `{pk}`")));
    }
    p.get(pk)
        .and_then(vec3)
        .map(|v| (v, None))
        .ok_or_else(|| bad(cmd, format!("give `{pk}`, `{fk}` or `{pk}_plane` ([x, y, z], \"origin\", or a plane)")))
}

pub(super) fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.align";
    let paths = resolve_paths(s, p, cmd)?;
    let flip = opt_bool(p, "flip", false, cmd)?;
    let capture = opt_bool(p, "capture", true, cmd)?;
    let move_grounded = opt_bool(p, "move_grounded", false, cmd)?;

    for path in &paths {
        let Some(&id) = path.last() else {
            return Err(bad(cmd, "occurrence path is empty"));
        };
        let o = s.doc.occurrences.iter().find(|o| o.id == id).ok_or_else(|| bad(cmd, "no occurrence"))?;
        if o.grounded && !move_grounded {
            return Err(bad(
                cmd,
                format!("{} is grounded: unground it first (occurrence.ground; a design's first part is grounded when it is made)", o.name),
            ));
        }
        if let Some(mut group) = s.held_by_rigid_group(id) {
            for _ in 0..64 {
                match s.doc.occurrence_of(group).and_then(|occ| s.held_by_rigid_group(occ.id)) {
                    Some(up) => group = up,
                    None => break,
                }
            }
            let name = s.doc.components.iter().find(|c| c.id == group).map(|c| c.name.clone()).unwrap_or_default();
            return Err(bad(cmd, format!("{} is held in {name} by a rigid group: move {name}, or Unlock Contents (component.unlock) first", o.name)));
        }
    }

    let plane = |key: &str| -> Result<Option<solidcraft_doc::PlaneRef>> {
        p.get(key).map(|v| super::super::features::plane_param(s, Some(v), cmd)).transpose()
    };
    let (from_plane, to_plane) = (plane("from_plane")?, plane("to_plane")?);
    let (vals, _) = s.doc.param_values();
    let (from, from_normal) = world_pick(s, p, "from", "from_face", &from_plane, &vals, cmd)?;
    let (to, to_normal) = world_pick(s, p, "to", "to_face", &to_plane, &vals, cmd)?;

    let (mut axis, mut ang) = (Vec3::Z, 0.0);
    if let (Some(a), Some(b)) = (from_normal.and_then(Vec3::normalized), to_normal.and_then(Vec3::normalized)) {
        let want = if flip { b } else { b * -1.0 };
        let cross = a.cross(want);
        let theta = cross.len().atan2(a.dot(want));
        if theta.abs() > 1e-12 {
            axis = cross.normalized().unwrap_or_else(|| a.any_perp());
            ang = theta;
        }
    }
    let d = match (&to_plane, to_normal.and_then(Vec3::normalized)) {
        (Some(_), Some(n)) => n * (to - from).dot(n),
        _ => to - from,
    };
    if !(d.is_finite() && from.is_finite() && axis.is_finite() && ang.is_finite()) {
        return Err(bad(cmd, "non-finite alignment transform"));
    }
    let world_move = rigid(d, from, axis, ang);

    // Compute every new local placement from the current placements before mutating anything.
    let mut updates = Vec::with_capacity(paths.len());
    for path in &paths {
        let placement = s.occurrence_placement(path).map_err(|e| bad(cmd, e.to_string()))?;
        let inverse = mat_inverse(&placement.parent_world).ok_or_else(|| bad(cmd, "parent frame is singular"))?;
        let local_delta = mat_mul(&mat_mul(&inverse, &world_move), &placement.parent_world);
        let next = mat_mul(&local_delta, &placement.local);
        if !valid_position(&next) {
            return Err(bad(cmd, "alignment transform exceeds supported numeric range"));
        }
        updates.push((placement.occurrence, next));
    }

    let moved: Vec<u64> = updates.iter().map(|(id, _)| *id).collect();
    // Placements inside a component placed more than once move in every placement of it: the
    // command still runs (as occurrence.move does), and says so for the dialog's warning.
    let shared: Vec<&Vec<u64>> = paths.iter().filter(|path| s.occurrence_shared(path)).collect();
    if capture {
        if world_move != IDENTITY || updates.iter().any(|(id, _)| s.pending_moves.contains_key(id)) {
            for (id, next) in updates {
                s.pending_moves.remove(&id);
                if let Some(x) = s.doc_mut().occurrences.iter_mut().find(|x| x.id == id) {
                    x.transform = next;
                }
            }
        }
    } else {
        for (id, next) in updates {
            s.pending_moves.insert(id, next);
        }
        s.revision += 1;
    }

    let mut reply = json!({
        "occurrences": moved,
        "paths": paths,
        "world_transform": world_move,
        "pending": !capture,
    });
    if !shared.is_empty()
        && let Some(o) = reply.as_object_mut()
    {
        o.insert("shared".into(), json!(shared));
    }
    Ok(reply)
}
