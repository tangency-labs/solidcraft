//! Snap points on model geometry: the places a pick locks onto, as the viewport's hover offers
//! them. Two questions, both answered here: what lies within a distance of a place (`at` and
//! `radius`), and what one edge or face offers however far away it is (`edge` or `face` with
//! `body`) — the second is how hovering the rim of a hole picks its axis.
//!
//! The geometry is [`solidcraft_geom::snap`]; this adds the bodies, their visibility and the
//! JSON. The viewport ranks the same candidates in screen pixels instead
//! (`ui-egui/src/model_snap.rs`), so a script and a hover agree on what is there.

use serde_json::{Value, json};
use solidcraft_geom::snap::{self, SnapCandidate, SnapKinds};

use super::CommandSpec;
use crate::params::{bad, num, req_vec3, string_list_or_default};
use crate::{Result, Session};

/// Snap distance when none is given (mm), as for `sketch.snap`.
const DEFAULT_RADIUS: f64 = 1.0;
/// Most bodies one search looks at.
const MAX_BODIES: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("model.snap", "Snap Points", snap_points).icon("snap").noundo().params(
    "at: [x,y,z]; radius?: mm (snap distance, default 1); \
         kinds?: [vertex|center|midpoint|quadrant|face_center|on_edge|on_face] (default all); \
         bodies?: [names] (default every visible body); \
         edge?: index | face?: index, with body: what that one edge or face offers whatever the distance \
         (a circular edge offers its centre, which is how the axis of a hole is picked) \
         → snaps: [{type, at, body, edge?, face?, circle?: {center, axis, radius}}], the strongest pull first",
)];

fn kinds_of(p: &Value, cmd: &str) -> Result<SnapKinds> {
    let ids = string_list_or_default(p, "kinds");
    if ids.is_empty() {
        return Ok(snap::ALL);
    }
    snap::kinds_of(ids.iter().map(String::as_str))
        .map_err(|bad_id| bad(cmd, format!("unknown snap kind `{bad_id}`; one of vertex, center, midpoint, quadrant, face_center, on_edge, on_face")))
}

fn candidate_json(body: &str, c: &SnapCandidate) -> Value {
    let mut v = json!({"type": c.kind.id(), "at": c.at, "body": body});
    if let Some(o) = v.as_object_mut() {
        if let Some(e) = c.edge {
            o.insert("edge".into(), json!(e));
        }
        if let Some(f) = c.face {
            o.insert("face".into(), json!(f));
        }
        if let Some(circle) = c.circle {
            o.insert("circle".into(), json!({"center": circle.center, "axis": circle.axis, "radius": circle.radius}));
        }
    }
    v
}

fn snap_points(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "model.snap";
    let at = req_vec3(cmd, p, "at")?;
    let radius = match num(p, "radius") {
        Some(r) if r > 0.0 => r,
        Some(r) => return Err(bad(cmd, format!("`radius` must be more than zero, not {r}"))),
        None => DEFAULT_RADIUS,
    };
    let kinds = kinds_of(p, cmd)?;
    let only = string_list_or_default(p, "bodies");
    let edge = p.get("edge").and_then(Value::as_u64);
    let face = p.get("face").and_then(Value::as_u64);
    // One edge or face is asked about by name, so the body has to be named too.
    let named = p.get("body").and_then(Value::as_str);
    if (edge.is_some() || face.is_some()) && named.is_none() {
        return Err(bad(cmd, "`edge` and `face` need `body`: which body the index belongs to"));
    }
    let st = s.world_state()?;
    if let Some(name) = named
        && (edge.is_some() || face.is_some())
    {
        let b = st.body(name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
        let m = b.mesh();
        let mut out = Vec::new();
        if let Some(e) = edge {
            let e = usize::try_from(e).map_err(|_| bad(cmd, "`edge` is not an index"))?;
            if m.edges.get(e).is_none() {
                return Err(bad(cmd, format!("body `{name}` has no edge {e}")));
            }
            out.extend(snap::edge_candidates(&m, e, at, kinds));
        }
        if let Some(f) = face {
            let f = u32::try_from(f).map_err(|_| bad(cmd, "`face` is not an index"))?;
            if !m.tri_face.contains(&f) {
                return Err(bad(cmd, format!("body `{name}` has no face {f}")));
            }
            let on = snap::nearest_on_face(&m, f, at).unwrap_or(at);
            out.extend(snap::face_candidates(&m, f, on, kinds));
        }
        snap::rank(&mut out, at, 0.0);
        let snaps: Vec<Value> = out.iter().map(|c| candidate_json(&b.name, c)).collect();
        return Ok(json!({"snaps": snaps}));
    }
    let mut out: Vec<(String, SnapCandidate)> = Vec::new();
    for b in st.bodies.iter().take(MAX_BODIES) {
        if !s.body_visible(&b.name) || (!only.is_empty() && !only.contains(&b.name)) {
            continue;
        }
        out.extend(snap::candidates_near(&b.mesh(), at, radius, kinds).into_iter().map(|c| (b.name.clone(), c)));
    }
    if !only.is_empty()
        && let Some(missing) = only.iter().find(|n| st.body(n).is_none())
    {
        return Err(bad(cmd, format!("no body `{missing}`")));
    }
    // One ranking across the bodies, so the strongest place wins whichever body it is on.
    snap::rank_by(&mut out, at, (radius * 1e-3).max(1e-9), |(_, c)| (c.kind, c.at));
    let snaps: Vec<Value> = out.iter().map(|(name, c)| candidate_json(name, c)).collect();
    Ok(json!({"snaps": snaps}))
}

#[cfg(test)]
#[path = "model_snap_tests.rs"]
mod tests;
