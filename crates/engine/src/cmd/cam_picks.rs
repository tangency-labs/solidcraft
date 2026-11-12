//! Picks (a child of `cmd/cam.rs`): an operation's geometry stored as persistent face and edge
//! names (`schema/cam_picks.rs`), found again each time the job is planned and read as the keys
//! the planners take (a pocket's or contour's `feature`).
//!
//! A pick is found by its name first (`naming::locate_face_named`, `locate_edge_named`), else by
//! the exact face or edge its stored point lies on (the B-rep's surfaces and exact extents, never
//! the display mesh), and is `lost` when neither finds it: the operation is then refused. One
//! found by its point, or by a name worked out from geometry, or whose face has moved since it was
//! picked, is a note to check. Nothing here changes the job as stored: the keys picks give go into
//! the copy that is planned.

use serde_json::{Map, Value, json};
use solidcore_record::Vec3;
use solidcraft_cam::Frame;
use solidcraft_cam::kinematics::Machine;
use solidcraft_doc::ModelState;
use solidcraft_geom::prelude::*;
use solidcraft_kernel::FaceSurface;

use super::features::{BodyFaces, FeatureSet, Lazy};
use super::num;
use crate::params::bad;
use crate::schema::cam::CamJob;
use crate::schema::cam_picks::{OpRef, Pick, PickResolved};
use crate::{Result, Session};

/// Most picks one operation holds.
pub(crate) const MAX_PICKS: usize = 1000;
/// The keys of an operation that picks take the place of.
const GEOMETRY_KEYS: &[&str] = &["sketch", "profiles", "polygon", "feature"];

/// A face or edge found again.
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub body: usize,
    pub body_name: String,
    pub face: Option<usize>,
    pub edge: Option<usize>,
    /// `name` or `point`.
    pub by: &'static str,
    pub note: Option<String>,
}

/// How far from an exact surface a stored point may be and still be on it: the display mesh a
/// click lands on is within a small share of the body's size of the exact surface.
fn point_tol(st: &ModelState, body: usize, tol: f64) -> f64 {
    let size = st.bodies.get(body).map_or(1.0, |b| b.body.size());
    (tol * 10.0).max(size * 2e-3).max(1e-4)
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn finite3(p: &[f64; 3]) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= super::MAX_COORD)
}

/// Checks a pick's shape: a face or an edge of a body (with a finite point), or a point alone.
pub(crate) fn check_pick(p: &Pick) -> std::result::Result<(), String> {
    let text_ok =
        |t: &Option<String>| t.as_deref().is_none_or(|t| !t.trim().is_empty() && t.chars().count() <= 400 && !t.chars().any(char::is_control));
    if ![&p.body, &p.face, &p.edge].iter().all(|t| text_ok(t)) {
        return Err("a pick's names must be 1 to 400 characters, without control characters".into());
    }
    if p.at.as_ref().is_some_and(|a| !finite3(a)) {
        return Err("a pick's `at` must be a finite point [x, y, z] within 1000 m".into());
    }
    match (p.face.is_some(), p.edge.is_some()) {
        (true, true) => Err("a pick names a `face` or an `edge`, not both".into()),
        (false, false) if p.at.is_none() => Err("a pick needs a `face` or an `edge` (with `at`)".into()),
        _ => Ok(()),
    }
}

/// The part's bodies: the job's `bodies`, else every body with exact faces.
fn part_bodies(st: &ModelState, bodies: Option<&[String]>) -> Vec<usize> {
    st.bodies
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.body.is_mesh())
        .filter(|(_, b)| bodies.is_none_or(|list| list.contains(&b.name)))
        .map(|(i, _)| i)
        .collect()
}

/// Finds face or edge pick `p` again: by name, else by its point. `Err`: lost, and why.
pub(crate) fn find(st: &ModelState, part: &[usize], p: &Pick, tol: f64) -> std::result::Result<Hit, String> {
    let named_body = p.body.as_deref().and_then(|n| st.bodies.iter().position(|b| b.name == n));
    if p.body.is_some() && named_body.is_none() {
        // The body itself is gone or renamed: only its point can find it on another body.
        if p.at.is_none() {
            return Err(format!("no body `{}`", p.body.as_deref().unwrap_or_default()));
        }
    }
    let at = p.at.map(v3);
    let hit = |body: usize, face: Option<usize>, edge: Option<usize>, by: &'static str, note: Option<String>| Hit {
        body,
        body_name: st.bodies.get(body).map(|b| b.name.clone()).unwrap_or_default(),
        face,
        edge,
        by,
        note,
    };
    if let Some(name) = &p.face {
        let near = at.unwrap_or(Vec3::ZERO);
        if let Some((b, f, verified, on_point)) = solidcraft_doc::naming::locate_face_named(st, p.body.as_deref(), name, near) {
            let note = (!verified && !on_point).then(|| "found by a name worked out from the shape, not exact history; check it".to_string());
            // The face its name finds, away from where it was picked: the model moved it (a
            // pocket made deeper, a step raised). Said, never followed silently.
            // Off its surface (a pocket made deeper), or on it but outside the face (the face
            // shrunk or slid along its surface), by exact geometry.
            let off = match (at, verified && !on_point, st.bodies.get(b)) {
                (Some(a), true, Some(mb)) => {
                    let faces = BodyFaces::new(&mb.name, &mb.body);
                    let ptol = point_tol(st, b, tol);
                    let across = faces.distance(f, a).unwrap_or(0.0);
                    let beside = faces.extent(f, Vec3::ZERO, &Frame::IDENTITY).map_or(0.0, |[lo, hi]| {
                        let q = [a.x, a.y, a.z];
                        (0..3).map(|k| (lo[k] - q[k]).max(q[k] - hi[k]).max(0.0)).map(|d| d * d).sum::<f64>().sqrt()
                    });
                    let d = across.max(beside);
                    (d > ptol).then_some(d)
                }
                _ => None,
            };
            if let (Some(d), Some(a)) = (off, at) {
                let note = format!(
                    "has moved since it was picked: its face is now {} mm from where it was picked ({}, {}, {}); check the operation still cuts what was meant",
                    num(d),
                    num(a.x),
                    num(a.y),
                    num(a.z)
                );
                return Ok(hit(b, Some(f), None, "name", Some(note)));
            }
            return Ok(hit(b, Some(f), None, "name", note));
        }
        let Some(at) = at else { return Err(format!("no face is named `{name}` any more")) };
        let bodies: Vec<usize> = named_body.into_iter().chain(part.iter().copied()).collect();
        for b in bodies {
            let Some(mb) = st.bodies.get(b) else { continue };
            if mb.body.is_mesh() {
                continue;
            }
            let faces = BodyFaces::new(&mb.name, &mb.body);
            if let Some(f) = faces.faces_at(at, point_tol(st, b, tol)).first() {
                let note = format!("found by its position, not its name (`{name}` names no face now); check it");
                return Ok(hit(b, Some(*f), None, "point", Some(note)));
            }
        }
        return Err(format!("face `{name}` is gone, and no face is at ({}, {}, {})", num(at.x), num(at.y), num(at.z)));
    }
    if let Some(name) = &p.edge {
        let near = at.unwrap_or(Vec3::ZERO);
        let bodies: Vec<usize> = named_body.into_iter().chain(part.iter().copied()).collect();
        for b in &bodies {
            let Some(mb) = st.bodies.get(*b) else { continue };
            if let Ok((e, verified, on_point)) = solidcraft_doc::naming::locate_edge_named(mb, name, near) {
                let note = (!verified && !on_point).then(|| "found by a name worked out from the shape, not exact history; check it".to_string());
                return Ok(hit(*b, None, Some(e), "name", note));
            }
        }
        let Some(at) = at else { return Err(format!("no edge is named `{name}` any more")) };
        for b in bodies {
            let Some(mb) = st.bodies.get(b) else { continue };
            if let Ok(Some((e, _))) = mb.body.nearest_edge(at, point_tol(st, b, tol)) {
                let note = format!("found by its position, not its name (`{name}` names no edge now); check it");
                return Ok(hit(b, None, Some(e), "point", Some(note)));
            }
        }
        return Err(format!("edge `{name}` is gone, and no edge is at ({}, {}, {})", num(at.x), num(at.y), num(at.z)));
    }
    // A point alone: the face it is on.
    let Some(at) = at else { return Err("the pick names nothing to find".into()) };
    let bodies: Vec<usize> = named_body.into_iter().chain(part.iter().copied()).collect();
    for b in bodies {
        let Some(mb) = st.bodies.get(b) else { continue };
        if mb.body.is_mesh() {
            continue;
        }
        if let Some(f) = BodyFaces::new(&mb.name, &mb.body).faces_at(at, point_tol(st, b, tol)).first() {
            return Ok(hit(b, Some(*f), None, "point", None));
        }
    }
    Err(format!("no face is at ({}, {}, {})", num(at.x), num(at.y), num(at.z)))
}

/// The `cam.features` ids face `f` of body `name` belongs to, most specific first: a hole's
/// wall, a pocket's floor or wall, a face, a profile's face or side, a round, a chamfer, a
/// surface, a turned diameter.
pub(crate) fn features_of_face(set: &FeatureSet, faces: &BodyFaces, name: &str, f: usize, origin: Vec3, tol: f64) -> Vec<String> {
    let r = &set.reply;
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    };
    for h in r.holes.iter().filter(|h| h.body == name && h.faces.contains(&f)) {
        push(&h.id);
    }
    // A countersink's cone: the hole on its axis.
    if let Some(FaceSurface::Cone { apex, axis, .. }) = faces.surface(f) {
        for h in r.holes.iter().filter(|h| h.body == name && h.countersink.is_some()) {
            let Ok(g) = set.get(&h.id) else { continue };
            let Some(su) = set.setup(g) else { continue };
            let local = su.frame.to_local(apex - origin);
            let along = axis.vec().dot(su.frame.z).abs() >= 1.0 - 1e-6;
            if along && (local.x - h.center[0]).abs() <= tol * 10.0 && (local.y - h.center[1]).abs() <= tol * 10.0 {
                push(&h.id);
            }
        }
    }
    for p in r.pockets.iter().filter(|p| p.body == name && p.floor_face == f) {
        push(&p.id);
    }
    // A wall: square to the tool axis of the feature's setup, within its outline and depth.
    let within = |id: &str, bounds: [[f64; 2]; 2], lo_z: f64, hi_z: f64| -> bool {
        let Ok(g) = set.get(id) else { return false };
        let Some(su) = set.setup(g) else { return false };
        let upright = match faces.surface(f) {
            Some(FaceSurface::Plane { normal, .. }) => normal.vec().dot(su.frame.z).abs() <= 1e-6,
            Some(FaceSurface::Cylinder { axis, .. }) => axis.vec().dot(su.frame.z).abs() >= 1.0 - 1e-9,
            _ => false,
        };
        if !upright {
            return false;
        }
        let Some([lo, hi]) = faces.extent(f, origin, &su.frame) else { return false };
        lo[0] >= bounds[0][0] - tol
            && hi[0] <= bounds[1][0] + tol
            && lo[1] >= bounds[0][1] - tol
            && hi[1] <= bounds[1][1] + tol
            && lo[2] >= lo_z - tol
            && hi[2] <= hi_z + tol
    };
    // And on the feature's outline (or an island's), seen along that tool axis: on a 4th-axis
    // part a floor on one side is square to another side's axis and inside its outline, but no
    // wall of it.
    let on_outline = |id: &str, outlines: &[&[[f64; 2]]]| -> bool {
        let Ok(g) = set.get(id) else { return false };
        let Some(su) = set.setup(g) else { return false };
        let Some(p) = faces.point_on(f) else { return false };
        let q = su.frame.to_local(p - origin);
        outlines.iter().any(|o| outline_distance(o, [q.x, q.y]).is_some_and(|d| d <= OUTLINE_NEAR.max(10.0 * tol)))
    };
    for p in r.pockets.iter().filter(|p| p.body == name) {
        let mut outlines: Vec<&[[f64; 2]]> = vec![&p.outline];
        outlines.extend(p.islands.iter().map(|i| i.outline.as_slice()));
        if within(&p.id, p.bounds, p.floor_z, p.top_z) && on_outline(&p.id, &outlines) {
            push(&p.id);
        }
    }
    for x in r.faces.iter().filter(|x| x.body == name && x.face == f) {
        if let Some(p) = &x.pocket {
            push(p);
        }
        push(&x.id);
        for pr in r.profiles.iter().filter(|pr| pr.face_id == x.id) {
            push(&pr.id);
        }
    }
    for pr in r.profiles.iter().filter(|p| p.body == name) {
        if within(&pr.id, pr.bounds, pr.bottom_z, pr.top_z) && on_outline(&pr.id, &[&pr.outline]) {
            push(&pr.id);
        }
    }
    for x in r.fillets.iter().chain(&r.chamfers).filter(|x| x.body == name && x.faces.contains(&f)) {
        push(&x.id);
    }
    for x in r.surfaces.iter().filter(|x| x.body == name && x.faces.contains(&f)) {
        push(&x.id);
    }
    for x in r.turned.iter().filter(|x| x.body == name && x.faces.contains(&f)) {
        push(&x.id);
    }
    out
}

/// How near a wall's point must come to a feature's outline to be one of its walls (mm): the
/// outline is sampled from the exact edges within the job's tolerance.
const OUTLINE_NEAR: f64 = 0.05;

/// How far point `q` is from closed outline `pts` (`None` for an empty outline).
fn outline_distance(pts: &[[f64; 2]], q: [f64; 2]) -> Option<f64> {
    let n = pts.len();
    (0..n)
        .filter_map(|i| {
            let (a, b) = (pts.get(i)?, pts.get((i + 1) % n)?);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            Some((q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy))
        })
        .reduce(f64::min)
}

/// The features face `f` (or the faces either side of edge `e`) belongs to.
fn features_of_hit(set: &FeatureSet, st: &ModelState, h: &Hit, origin: Vec3, tol: f64) -> Vec<String> {
    let Some(mb) = st.bodies.get(h.body) else { return Vec::new() };
    let faces = BodyFaces::new(&mb.name, &mb.body);
    match (h.face, h.edge) {
        (Some(f), _) => features_of_face(set, &faces, &mb.name, f, origin, tol),
        (None, Some(e)) => {
            let mut out: Vec<String> = Vec::new();
            for f in faces.edge_faces(e) {
                for id in features_of_face(set, &faces, &mb.name, f, origin, tol) {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
            // An edge's own features (a pocket's rim) before the top face it also bounds.
            out.sort_by_key(|id| id.starts_with("face:"));
            out
        }
        _ => Vec::new(),
    }
}

/// The kind of a feature id (`pocket`, `hole`...).
fn kind_of(id: &str) -> &str {
    id.split(':').next().unwrap_or_default()
}

/// What feature kinds an operation type reads its picks as, best first.
fn wanted(ty: &str) -> &'static [&'static str] {
    match ty {
        "pocket" => &["pocket", "hole", "face"],
        "contour" => &["profile", "pocket", "hole", "face"],
        _ => &["surface", "fillet", "chamfer", "pocket", "face", "profile", "hole", "turned"],
    }
}

/// Picks of an operation found again: the hits, their features, and what each reports.
struct Found {
    hits: Vec<(usize, Hit, Option<String>)>,
    report: Vec<PickResolved>,
    lost: Vec<(usize, String)>,
}

/// Everything resolution needs about the job and the model.
pub(crate) struct Model<'a> {
    pub s: &'a Session,
    pub st: std::sync::Arc<ModelState>,
    pub part: Vec<usize>,
    pub origin: Vec3,
    pub tol: f64,
    pub features: Lazy,
    pub cmd: &'a str,
}

impl<'a> Model<'a> {
    pub fn new(cmd: &'a str, s: &'a Session, job: &CamJob, machine: &Machine) -> Result<Model<'a>> {
        let st = s.world_state()?;
        let part = part_bodies(&st, job.bodies.as_deref());
        let origin = v3(job.origin.unwrap_or([0.0; 3]));
        let tol = job.tolerance.unwrap_or(0.01).clamp(super::TOLERANCE_RANGE.0, super::TOLERANCE_RANGE.1);
        let features = Lazy::new(job.bodies.clone(), origin, tol, machine);
        Ok(Model { s, st, part, origin, tol, features, cmd })
    }

    fn set(&self) -> Option<&FeatureSet> {
        self.features.get(self.cmd, self.s).ok()
    }

    /// Finds pick `p` again on the part.
    pub(crate) fn find(&self, p: &Pick) -> std::result::Result<Hit, String> {
        find(&self.st, &self.part, p, self.tol)
    }
}

/// Finds an operation's picks again.
fn find_all(m: &Model<'_>, op: Option<&OpRef>, picks: &[Pick], ty: &str) -> Found {
    let mut found = Found { hits: Vec::new(), report: Vec::new(), lost: Vec::new() };
    let set = m.set();
    for (k, p) in picks.iter().enumerate() {
        match m.find(p) {
            Ok(h) => {
                let ids = set.map(|set| features_of_hit(set, &m.st, &h, m.origin, m.tol)).unwrap_or_default();
                let want = wanted(ty);
                let feature = want.iter().find_map(|w| ids.iter().find(|id| kind_of(id) == *w)).cloned().or_else(|| ids.first().cloned());
                found.report.push(PickResolved {
                    op: op.cloned(),
                    pick: k,
                    by: h.by.into(),
                    body: Some(h.body_name.clone()),
                    face: h.face,
                    edge: h.edge,
                    feature: feature.clone(),
                    note: h.note.clone(),
                });
                found.hits.push((k, h, feature));
            }
            Err(why) => {
                found.report.push(PickResolved {
                    op: op.cloned(),
                    pick: k,
                    by: "lost".into(),
                    body: p.body.clone(),
                    note: Some(why.clone()),
                    ..PickResolved::default()
                });
                found.lost.push((k, why));
            }
        }
    }
    found
}

/// The A angle of feature `id`'s setup (0 on top).
fn a_of(set: &FeatureSet, id: &str) -> f64 {
    set.get(id).ok().and_then(|g| set.setup(g)).map_or(0.0, |su| su.a_deg)
}

/// The geometry the hits give an operation of type `ty`: keys written into `op`.
fn apply(m: &Model<'_>, ty: &str, op: &mut Map<String, Value>, found: &Found) -> std::result::Result<(), String> {
    if found.hits.is_empty() {
        return Ok(());
    }
    let set = m.set().ok_or_else(|| "the part's features could not be found".to_string())?;
    let ids: Vec<&str> = {
        let mut v: Vec<&str> = Vec::new();
        for (_, _, f) in &found.hits {
            if let Some(f) = f.as_deref()
                && !v.contains(&f)
            {
                v.push(f);
            }
        }
        v
    };
    let side = ids.first().map_or(0.0, |id| a_of(set, id));
    if ids.iter().any(|id| (a_of(set, id) - side).abs() > 1e-6) {
        return Err("the picks are on different sides of the part (different A angles): give one operation each".into());
    }
    let one = |kinds: &[&str]| -> std::result::Result<&str, String> {
        let fit: Vec<&str> = ids.iter().copied().filter(|id| kinds.contains(&kind_of(id))).collect();
        match fit.as_slice() {
            [one] => Ok(one),
            [] => Err(format!(
                "the picks are not {} (they are {})",
                kinds.join(" or "),
                if ids.is_empty() { "on no feature this operation reads".to_string() } else { ids.join(", ") }
            )),
            many => Err(format!("the picks are {} features ({}): give one operation each", many.len(), many.join(", "))),
        }
    };
    match ty {
        "pocket" | "contour" => {
            let id = one(wanted(ty))?;
            op.insert("feature".into(), json!(id));
            Ok(())
        }
        _ => Err(format!("a `{ty}` operation takes no picks")),
    }
}

/// Finds the picks of every operation of `job` again and writes the geometry they give into it
/// (a pocket's or contour's `feature`), in place of `picks`. An operation turned off keeps its
/// picks unread. A pick that is lost, or that is no feature the operation reads, refuses the job
/// with every operation that has one named.
pub(crate) fn resolve_job(m: &Model<'_>, job: &mut Map<String, Value>) -> Result<Vec<PickResolved>> {
    let mut report: Vec<PickResolved> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    let Some(Value::Array(ops)) = job.get_mut("operations") else { return Ok(report) };
    for (i, op) in ops.iter_mut().enumerate() {
        let Some(inner) = op.as_object_mut() else { continue };
        let name = inner.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        if inner.get("enabled").and_then(Value::as_bool) == Some(false) {
            // Turned off: neither planned nor written, so its picks need not be found (a lost one
            // must not stop the rest).
            continue;
        }
        let ty = inner.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
        let picks: Vec<Pick> = match inner.remove("picks") {
            None | Some(Value::Null) => continue,
            Some(v) => serde_json::from_value(v).map_err(|e| bad(m.cmd, format!("operation {i} `{name}`: `picks`: {e}")))?,
        };
        if picks.is_empty() {
            continue;
        }
        let refuse = |why: String| format!("operation {i} `{name}`: {why}");
        if picks.len() > MAX_PICKS {
            refused.push(refuse(format!("at most {MAX_PICKS} picks")));
            continue;
        }
        if let Some(e) = picks.iter().find_map(|p| check_pick(p).err()) {
            refused.push(refuse(e));
            continue;
        }
        if let Some(k) = GEOMETRY_KEYS.iter().find(|k| inner.contains_key(**k)) {
            refused.push(refuse(format!("give `picks` or `{k}`, not both")));
            continue;
        }
        let r = OpRef { index: i, name: name.clone() };
        let found = find_all(m, Some(&r), &picks, &ty);
        report.extend(found.report.iter().cloned());
        if !found.lost.is_empty() {
            let what: Vec<String> = found.lost.iter().map(|(k, why)| format!("pick {} was not found: {why}", k + 1)).collect();
            refused.push(refuse(what.join("; ")));
            continue;
        }
        if let Err(e) = apply(m, &ty, inner, &found) {
            refused.push(refuse(e));
        }
    }
    if !refused.is_empty() {
        return Err(bad(m.cmd, format!("the job was not planned: {}. Nothing was written.", refused.join("; "))));
    }
    Ok(report)
}

#[cfg(test)]
#[path = "cam_picks_tests.rs"]
mod tests;
