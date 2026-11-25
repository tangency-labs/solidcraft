//! Review markup commands: mark a face, an edge, a body, a feature, a component, a point or a
//! view of a design with a note, reply to it, attach pictures, and close it as done or not done,
//! with a reason ([review markups](../../../../docs/review-markups.md)).
//!
//! The markups are a set of their own beside the design (`solidcraft_io::markups`): every write
//! is one step on the set as it stands on the disk, under its lock. Only a markup's pictures are
//! the design's: `markup.image` puts each picture's bytes in the design as an entry of its own
//! (`Document::markup_images`, saved as a part of the container the way a canvas's image is), one
//! undo step, and the set names the entry. They need no window and open no dialog.
//!
//! A markup on a face keeps the face's lasting name (the name `model.faces` lists) and the point
//! that was marked. `markup.list` finds each face again by that name with exact geometry, never
//! the display mesh: the B-rep's own faces, `Body::nearest_face` and `Body::point_lies_on_face`.
//! So a rebuild that moves the face keeps the markup on it (`moved`), a face the history marker
//! or the active configuration leaves out is `hidden`, not gone, and only a face no feature makes
//! any more is `gone`.

use serde_json::{Value, json};

use solidcore_record::Vec3;
use solidcraft_doc::{ModelBody, ModelState, persistent_name};
use solidcraft_geom::prelude::*;
use solidcraft_io::markups::{self as set, Anchor, Image, Markup, Message, Outcome, Set, Status, View};

use super::CommandSpec;
use crate::params::{bad, str_, vec3};
use crate::schema::markups::{
    MarkupAddParams, MarkupChanged, MarkupCloseParams, MarkupDeleteParams, MarkupDeleted, MarkupEditParams, MarkupImageParams, MarkupList,
    MarkupListParams, MarkupMade, MarkupReopenParams, MarkupReplyParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LIST: TypedContract<MarkupListParams> = TypedContract::new();
const ADD: TypedContract<MarkupAddParams> = TypedContract::new();
const EDIT: TypedContract<MarkupEditParams> = TypedContract::new();
const REPLY: TypedContract<MarkupReplyParams> = TypedContract::new();
const IMAGE: TypedContract<MarkupImageParams> = TypedContract::new();
const CLOSE: TypedContract<MarkupCloseParams> = TypedContract::new();
const REOPEN: TypedContract<MarkupReopenParams> = TypedContract::new();
const DELETE: TypedContract<MarkupDeleteParams> = TypedContract::new();

/// Who wrote something when a call does not say: an agent or a script.
pub const AGENT: &str = "agent";

/// The keys that say what a markup is on.
const ANCHOR_KEYS: &[&str] = &["face", "edge", "body", "at", "feature", "occurrence"];

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("markup.list", "List Markups", crate::checked!("markup.list", markup_list))
        .icon("list")
        .contract(&LIST)
        .output(output_schema_for::<MarkupList>)
        .noundo()
        .read_only()
        .params("set?: path (default: beside the design); status?: open|resolved|rejected → set, counts, markups: [{id, guid, status, note, thread, anchor, found: {state: anchored|moved|gone|hidden|unchecked, at?, was?, moved_by?, why?}, view?, images, by, made, changed, outcome?}]"),
    CommandSpec::new("markup.add", "Add Markup", crate::checked!("markup.add", markup_add))
        .icon("text")
        .contract(&ADD)
        .output(output_schema_for::<MarkupMade>)
        .noundo()
        .params("text; face?: name (+ body?); edge?: name (+ body?); body?; at?: [x,y,z] (a point on a face, or in space); feature?; occurrence?: [ids]; view?: {target, yaw, pitch, distance, fov}; by?; set? → markup, guid, anchor, found"),
    CommandSpec::new("markup.edit", "Edit Markup", crate::checked!("markup.edit", markup_edit))
        .icon("edit")
        .contract(&EDIT)
        .output(output_schema_for::<MarkupChanged>)
        .noundo()
        .params("markup: id or guid; text? (the note, its maker only); face?/edge?/body?/at?/feature?/occurrence? (pin it again); view?; by?; set? → markup, status"),
    CommandSpec::new("markup.reply", "Reply to Markup", crate::checked!("markup.reply", markup_reply))
        .icon("text")
        .contract(&REPLY)
        .output(output_schema_for::<MarkupChanged>)
        .noundo()
        .doc_only()
        .params("markup: id or guid; text (at most 280 characters from anyone but its maker); by?; set? → markup, status"),
    CommandSpec::new("markup.image", "Markup Picture", crate::checked!("markup.image", markup_image))
        .icon("render_image")
        .contract(&IMAGE)
        .output(output_schema_for::<MarkupChanged>)
        .doc_only()
        .params("markup: id or guid; file?: a PNG or JPEG path (+ name?) | remove?: a picture's entry; by?; set? → markup, status, image?"),
    CommandSpec::new("markup.resolve", "Resolve Markup", crate::checked!("markup.resolve", markup_resolve))
        .icon("finish")
        .contract(&CLOSE)
        .output(output_schema_for::<MarkupChanged>)
        .noundo()
        .doc_only()
        .params("markup: id or guid; reason: what was done; by?; set? → markup, status"),
    CommandSpec::new("markup.reject", "Reject Markup", crate::checked!("markup.reject", markup_reject))
        .icon("close")
        .contract(&CLOSE)
        .output(output_schema_for::<MarkupChanged>)
        .noundo()
        .doc_only()
        .params("markup: id or guid; reason: why it was not done; by?; set? → markup, status"),
    CommandSpec::new("markup.reopen", "Reopen Markup", crate::checked!("markup.reopen", markup_reopen))
        .icon("undo")
        .contract(&REOPEN)
        .output(output_schema_for::<MarkupChanged>)
        .noundo()
        .doc_only()
        .params("markup: id or guid; reason?; by?; set? → markup, status"),
    CommandSpec::new("markup.delete", "Delete Markup", crate::checked!("markup.delete", markup_delete))
        .icon("delete")
        .contract(&DELETE)
        .output(output_schema_for::<MarkupDeleted>)
        .doc_only()
        .params("markup: id or guid; set? → deleted: id (its pictures leave the design too)"),
];

/// The set a call works on: `set`, or the one beside the design's file.
fn set_path(s: &Session, p: &Value, cmd: &str) -> Result<String> {
    if let Some(v) = p.get("set").filter(|v| !v.is_null()) {
        let path = v.as_str().filter(|t| !t.trim().is_empty()).ok_or_else(|| bad(cmd, "`set` must be a path"))?;
        set::folder(path).map_err(|e| bad(cmd, e))?;
        if path.chars().count() > set::MAX_NAME || path.contains('\0') {
            return Err(bad(cmd, "`set` is too long a path"));
        }
        return Ok(path.to_string());
    }
    match s.path.as_deref() {
        Some(design) if !design.is_empty() => Ok(set::beside(design)),
        _ => Err(bad(cmd, "the design has no file yet: save it first, or give `set`")),
    }
}

/// Who a call writes as: `by`, or [`AGENT`].
fn who(p: &Value, cmd: &str) -> Result<String> {
    match p.get("by").filter(|v| !v.is_null()) {
        None => Ok(AGENT.to_string()),
        Some(v) => {
            let by = v.as_str().ok_or_else(|| bad(cmd, "`by` must be a name"))?;
            set::check_person(by).map_err(|e| bad(cmd, format!("`by`: {e}")))?;
            Ok(by.to_string())
        }
    }
}

/// Unix seconds now.
fn now() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A new guid: 32 hexadecimal digits from the clock, a counter and this process's random keys.
fn new_guid() -> String {
    use std::hash::{BuildHasher, RandomState};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let a = RandomState::new().hash_one((nanos, n, 1u8));
    let b = RandomState::new().hash_one((n, nanos, 2u8));
    format!("{a:016x}{b:016x}")
}

/// The markup `markup` names in `set`: its number, or its guid.
fn which(s: &Set, p: &Value) -> std::result::Result<u64, String> {
    match p.get("markup") {
        Some(Value::Number(n)) => {
            let id = n.as_u64().ok_or("`markup` must be a markup's number or guid")?;
            s.get(id).map(|m| m.id).ok_or_else(|| format!("no markup {id}"))
        }
        Some(Value::String(g)) if set::is_guid(g) => s.by_guid(g).ok_or_else(|| "no markup has that guid".to_string()),
        Some(Value::String(t)) if t.parse::<u64>().is_ok() => {
            let id = t.parse::<u64>().unwrap_or(0);
            s.get(id).map(|m| m.id).ok_or_else(|| format!("no markup {id}"))
        }
        _ => Err("`markup` must be a markup's number or guid".into()),
    }
}

fn point(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn vec(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// How far a picked point may lie from a body's exact face and still be on it.
fn reach(b: &ModelBody) -> f64 {
    (b.body.size() * 2e-3).max(1e-3)
}

/// The body called `name`, else the one body the feature `maker` made (a body renamed since).
fn find_body<'a>(st: &'a ModelState, name: &str, maker: u64) -> Option<&'a ModelBody> {
    st.body(name).filter(|b| !b.body.is_mesh()).or_else(|| {
        let mut made = st.bodies.iter().filter(|b| b.feature == maker && maker != 0 && !b.body.is_mesh());
        match (made.next(), made.next()) {
            (Some(b), None) => Some(b),
            _ => None,
        }
    })
}

/// The faces of `b` with the lasting name `name`, by B-rep face index.
fn faces_named(b: &ModelBody, name: &str) -> Vec<usize> {
    solidcraft_doc::naming::face_names(b)
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.is_empty() && persistent_name::same_face(n, name))
        .map(|(i, _)| i)
        .collect()
}

/// The name of each B-rep edge of `b`: its faces' lasting names, sorted, joined by `|` (as
/// `model.edges` lists them, without the number that tells repeated ones apart).
fn edge_face_names(b: &ModelBody) -> Vec<String> {
    let faces = solidcraft_doc::naming::face_names(b);
    b.body
        .record()
        .edges
        .iter()
        .map(|e| {
            let mut n: Vec<&str> = e.uses.iter().filter_map(|u| faces.get(u.face as usize).map(String::as_str)).collect();
            n.sort_unstable();
            n.dedup();
            n.join("|")
        })
        .collect()
}

/// An edge name without the number that tells repeated edges between the same faces apart.
fn edge_stem(name: &str) -> &str {
    name.rsplit_once('~').filter(|(_, k)| !k.is_empty() && k.bytes().all(|c| c.is_ascii_digit())).map_or(name, |(stem, _)| stem)
}

/// Whether two edge names are the same pair of faces, in either text form.
fn same_edge_faces(a: &str, b: &str) -> bool {
    let (a, b) = (edge_stem(a), edge_stem(b));
    if a == b {
        return true;
    }
    let mut x: Vec<&str> = a.split('|').collect();
    let mut y: Vec<&str> = b.split('|').collect();
    x.sort_unstable();
    y.sort_unstable();
    x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| persistent_name::same_face(p, q))
}

/// The point of face `face` nearest `p`, exactly, and how far `p` is from it.
fn nearest_on_face(b: &ModelBody, face: usize, p: Vec3) -> Option<(Vec3, f64)> {
    let far = b.body.size().max(1.0) * 10.0 + p.dist(Vec3::ZERO);
    let q = b.body.point_lies_on_face(face, p, far).ok()??;
    Some((q, q.dist(p)))
}

/// A point on edge `edge` of `b` near `p`, exactly: `p` taken onto the edge's two faces in turn
/// until it lies on both (an edge is where they meet).
fn near_edge(b: &ModelBody, edge: usize, p: Vec3) -> Option<Vec3> {
    let faces: Vec<usize> = b.body.record().edges.get(edge)?.uses.iter().map(|u| u.face as usize).collect();
    let (first, second) = (*faces.first()?, *faces.get(1).or(faces.first())?);
    let mut q = p;
    for _ in 0..8 {
        q = nearest_on_face(b, first, q)?.0;
        q = nearest_on_face(b, second, q)?.0;
        if solidcraft_kernel::edge_distance(&b.body, edge, q).ok().flatten().is_some_and(|d| d <= reach(b)) {
            return Some(q);
        }
    }
    Some(q)
}

/// A point on face `face` of `b`: the one nearest the middle of the body.
fn point_on(b: &ModelBody, face: usize) -> Option<Vec3> {
    let middle = b.body.exact_bounds().ok()?.center();
    nearest_on_face(b, face, middle).map(|(q, _)| q)
}

/// The face of a body nearest `p` within a pick's reach, exactly: body, face, the point on it.
fn face_at<'a>(st: &'a ModelState, body: Option<&'a ModelBody>, p: Vec3) -> Option<(&'a ModelBody, usize, Vec3)> {
    let bodies: Vec<&ModelBody> = match body {
        Some(b) => vec![b],
        None => st.bodies.iter().filter(|b| !b.body.is_mesh()).collect(),
    };
    let mut best: Option<(f64, &ModelBody, usize)> = None;
    for b in bodies {
        let r = reach(b);
        if body.is_none() && !b.body.exact_bounds().is_ok_and(|bb| bb.inflated(r).contains(p)) {
            continue;
        }
        if let Some((d, f)) = b.body.nearest_face(p)
            && d <= r
            && best.is_none_or(|(bd, ..)| d < bd)
        {
            best = Some((d, b, f));
        }
    }
    let (_, b, f) = best?;
    let q = b.body.point_lies_on_face(f, p, reach(b)).ok().flatten().unwrap_or(p);
    Some((b, f, q))
}

/// The anchor a call names (`None`: it names none).
fn capture(s: &Session, p: &Value, cmd: &str) -> Result<Option<Anchor>> {
    if !ANCHOR_KEYS.iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null())) {
        return Ok(None);
    }
    let at = match p.get("at").filter(|v| !v.is_null()) {
        Some(v) => Some(
            vec3(v)
                .filter(|v| v.is_finite() && [v.x, v.y, v.z].iter().all(|c| c.abs() <= set::MAX_COORD))
                .ok_or_else(|| bad(cmd, "`at` must be a point [x, y, z] of finite numbers"))?,
        ),
        None => None,
    };
    let text = |k: &str| -> Result<Option<String>> {
        match p.get(k).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(v) => {
                let t = v.as_str().filter(|t| !t.trim().is_empty() && t.chars().count() <= set::MAX_NAME && !t.chars().any(char::is_control));
                t.map(|t| Some(t.to_string())).ok_or_else(|| bad(cmd, format!("`{k}` must be a name")))
            }
        }
    };
    let (face, edge, body) = (text("face")?, text("edge")?, text("body")?);
    let config = s.doc.configs.active.clone();
    let st = s.world_state()?;
    let body_ref = match &body {
        Some(n) => Some(st.body(n).filter(|b| !b.body.is_mesh()).ok_or_else(|| bad(cmd, format!("no solid body `{n}`")))?),
        None => None,
    };
    if let Some(face) = face {
        let candidates: Vec<(&ModelBody, Vec<usize>)> = match body_ref {
            Some(b) => vec![(b, faces_named(b, &face))],
            None => st.bodies.iter().filter(|b| !b.body.is_mesh()).map(|b| (b, faces_named(b, &face))).collect(),
        };
        let found: Vec<(&ModelBody, Vec<usize>)> = candidates.into_iter().filter(|(_, f)| !f.is_empty()).collect();
        let (b, faces) = match found.as_slice() {
            [] => return Err(bad(cmd, format!("no face is named `{face}` (`model.faces` lists the names)"))),
            [one] => one.clone(),
            _ => return Err(bad(cmd, format!("`{face}` names faces of several bodies: give `body`"))),
        };
        let on = match at {
            Some(a) => faces
                .iter()
                .find_map(|f| b.body.point_lies_on_face(*f, a, reach(b)).ok().flatten())
                .ok_or_else(|| bad(cmd, format!("`at` does not lie on face `{face}` of `{}`", b.name)))?,
            None => faces.iter().find_map(|f| point_on(b, *f)).ok_or_else(|| bad(cmd, format!("face `{face}` has no point to mark")))?,
        };
        return Ok(Some(Anchor::Face { body: b.name.clone(), maker: b.feature, face, at: point(on), config }));
    }
    if let Some(edge) = edge {
        let bodies: Vec<&ModelBody> = match body_ref {
            Some(b) => vec![b],
            None => st.bodies.iter().filter(|b| !b.body.is_mesh()).collect(),
        };
        let mut found: Vec<(&ModelBody, usize)> = Vec::new();
        for b in bodies {
            for (i, n) in edge_face_names(b).iter().enumerate() {
                if !n.is_empty() && same_edge_faces(n, &edge) {
                    found.push((b, i));
                }
            }
        }
        let mut names: Vec<&str> = found.iter().map(|(b, _)| b.name.as_str()).collect();
        names.dedup();
        if names.len() > 1 {
            return Err(bad(cmd, format!("`{edge}` names edges of several bodies: give `body`")));
        }
        // Of several edges between the same faces, the one the point lies on.
        let pick = match at {
            Some(a) => {
                found.iter().copied().find(|(b, i)| solidcraft_kernel::edge_distance(&b.body, *i, a).ok().flatten().is_some_and(|d| d <= reach(b)))
            }
            None if found.len() == 1 => found.first().copied(),
            None => return Err(bad(cmd, format!("`{edge}` names {} edges: give `at`, a point on the one you mean", found.len()))),
        };
        let Some((b, index)) = pick else {
            return Err(bad(
                cmd,
                format!("no edge is named `{edge}`{} (`model.edges` lists the names)", if at.is_some() { " through `at`" } else { "" }),
            ));
        };
        // No point given: the edge's own point nearest the body's middle, so the markup is found
        // where it was made.
        let at = match at {
            Some(a) => a,
            None => {
                let middle = b.body.exact_bounds().map(|bb| bb.center()).map_err(|e| bad(cmd, e.to_string()))?;
                solidcraft_kernel::edge_closest_point(&b.body, index, middle)
                    .map_err(|e| bad(cmd, e.to_string()))?
                    .ok_or_else(|| bad(cmd, format!("`{edge}` could not be measured: give `at`, a point on it")))?
            }
        };
        return Ok(Some(Anchor::Edge { body: b.name.clone(), maker: b.feature, edge, at: point(at), config }));
    }
    if let Some(feature) = p.get("feature").filter(|v| !v.is_null()) {
        let f = match feature {
            Value::Number(n) => n.as_u64().and_then(|id| s.doc.features.iter().find(|f| f.id == id)),
            Value::String(t) => s.doc.features.iter().find(|f| f.name == *t),
            _ => None,
        }
        .ok_or_else(|| bad(cmd, "`feature` must name a feature of the timeline, by id or name"))?;
        return Ok(Some(Anchor::Feature { feature: f.id, name: f.name.clone(), at: at.map(point) }));
    }
    if let Some(occ) = p.get("occurrence").filter(|v| !v.is_null()) {
        let path: Vec<u64> = occ
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= set::MAX_PATH)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<u64>>>())
            .ok_or_else(|| bad(cmd, format!("`occurrence` must be a path of 1 to {} occurrence ids", set::MAX_PATH)))?;
        let name = occurrence_name(s, &path).ok_or_else(|| bad(cmd, "`occurrence` names no placed component"))?;
        return Ok(Some(Anchor::Occurrence { path, name, at: at.map(point) }));
    }
    if let Some(b) = body_ref {
        // A point on the body marks the face it lies on; a body alone, the body.
        if let Some(a) = at {
            let (b, f, q) = face_at(&st, Some(b), a).ok_or_else(|| bad(cmd, format!("`at` does not lie on `{}`", b.name)))?;
            if let Some(face) = solidcraft_doc::naming::face_names(b).get(f).filter(|n| !n.is_empty()) {
                return Ok(Some(Anchor::Face { body: b.name.clone(), maker: b.feature, face: face.clone(), at: point(q), config }));
            }
            return Ok(Some(Anchor::Body { body: b.name.clone(), maker: b.feature, at: Some(point(q)), config }));
        }
        return Ok(Some(Anchor::Body { body: b.name.clone(), maker: b.feature, at: None, config }));
    }
    // A point alone: the face it lies on, or the point in space.
    if let Some(a) = at {
        if let Some((b, f, q)) = face_at(&st, None, a)
            && let Some(face) = solidcraft_doc::naming::face_names(b).get(f).filter(|n| !n.is_empty())
        {
            return Ok(Some(Anchor::Face { body: b.name.clone(), maker: b.feature, face: face.clone(), at: point(q), config }));
        }
        return Ok(Some(Anchor::Point { at: point(a) }));
    }
    Ok(None)
}

/// The anchor of the one thing selected, as `markup.add` takes it when it is given no anchor and no view.
fn from_selection(s: &Session) -> Option<Anchor> {
    let [one] = s.selection.as_slice() else { return None };
    let st = s.world_state().ok()?;
    let config = s.doc.configs.active.clone();
    match one {
        crate::Sel::Face { body, index, point: at } => {
            let b = st.body(body).filter(|b| !b.body.is_mesh())?;
            let face = solidcraft_doc::naming::face_names(b).get(*index).filter(|n| !n.is_empty())?.clone();
            let q = b.body.point_lies_on_face(*index, *at, reach(b)).ok().flatten().unwrap_or(*at);
            Some(Anchor::Face { body: b.name.clone(), maker: b.feature, face, at: point(q), config })
        }
        crate::Sel::Edge { body, index, point: at } => {
            let b = st.body(body).filter(|b| !b.body.is_mesh())?;
            let edge = edge_face_names(b).get(*index).filter(|n| !n.is_empty())?.clone();
            Some(Anchor::Edge { body: b.name.clone(), maker: b.feature, edge, at: point(*at), config })
        }
        crate::Sel::Body { name } => {
            let b = st.body(name)?;
            Some(Anchor::Body { body: b.name.clone(), maker: b.feature, at: None, config })
        }
        crate::Sel::Feature { id } => {
            s.doc.features.iter().find(|f| f.id == *id).map(|f| Anchor::Feature { feature: f.id, name: f.name.clone(), at: None })
        }
        crate::Sel::Occurrence { path } => occurrence_name(s, path).map(|name| Anchor::Occurrence { path: path.clone(), name, at: None }),
        _ => None,
    }
}

/// The name of the placed component at `path`, when each step is a child of the one before.
fn occurrence_name(s: &Session, path: &[u64]) -> Option<String> {
    let mut parent = 0;
    let mut name = None;
    for id in path {
        let o = s.doc.occurrences.iter().find(|o| o.id == *id && o.parent == parent)?;
        parent = o.id;
        name = Some(o.name.clone());
    }
    name
}

/// The camera a call gives (`None`: none).
fn view_of(p: &Value, cmd: &str) -> Result<Option<View>> {
    match p.get("view").filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => {
            let view: View = serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "`view` is {target: [x, y, z], yaw, pitch, distance, fov?}"))?;
            set::check_view(&view).map_err(|e| bad(cmd, format!("`view`: {e}")))?;
            Ok(Some(view))
        }
    }
}

/// Where an anchor stands now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Found {
    /// There, at `at` (the point it was marked at, or none).
    Anchored { at: Option<[f64; 3]> },
    /// There, but no longer where it was marked: now at `at`, `moved_by` mm from `was`.
    Moved { at: [f64; 3], was: [f64; 3], moved_by: f64 },
    /// Not in the model now, but still in the design: the history marker or the active
    /// configuration leaves it out.
    Hidden { why: String, was: Option<[f64; 3]> },
    /// Gone from the design, or never given.
    Gone { why: String, was: Option<[f64; 3]> },
    /// Could not be checked.
    Unchecked { why: String },
}

impl Found {
    fn word(&self) -> &'static str {
        match self {
            Found::Anchored { .. } => "anchored",
            Found::Moved { .. } => "moved",
            Found::Hidden { .. } => "hidden",
            Found::Gone { .. } => "gone",
            Found::Unchecked { .. } => "unchecked",
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut v = json!({ "state": self.word() });
        match self {
            Found::Anchored { at } => {
                if let Some(a) = at {
                    v["at"] = json!(a);
                }
            }
            Found::Moved { at, was, moved_by } => {
                v["at"] = json!(at);
                v["was"] = json!(was);
                v["moved_by"] = json!(moved_by);
            }
            Found::Hidden { why, was } | Found::Gone { why, was } => {
                v["why"] = json!(why);
                if let Some(w) = was {
                    v["was"] = json!(w);
                }
            }
            Found::Unchecked { why } => v["why"] = json!(why),
        }
        v
    }
}

/// Why a face, edge or body that is not in the model is hidden rather than gone: the features
/// that make it are all still in the design, and the history marker, a suppression or another
/// configuration leaves one of them out. `None`: it is gone.
fn hidden_why(s: &Session, features: &[u64], config: Option<&str>, what: &str) -> Option<String> {
    let doc = &s.doc;
    let mut out = None;
    for id in features.iter().filter(|id| **id != 0) {
        let i = doc.feature_index(*id)?;
        let f = doc.features.get(i)?;
        if out.is_none() {
            if doc.marker.is_some_and(|m| i >= m) {
                out = Some(format!("{what} is past the history marker (made by `{}`)", f.name));
            } else if f.suppressed {
                out = Some(format!("{what} is made by `{}`, which is suppressed", f.name));
            }
        }
    }
    if out.is_none() && config != doc.configs.active.as_deref() {
        let marked = config.map_or_else(|| "with no configuration".to_string(), |c| format!("in configuration `{c}`"));
        let now = doc.configs.active.as_deref().map_or_else(|| "none is active now".to_string(), |c| format!("`{c}` is active now"));
        out = Some(format!("{what} was marked {marked}; {now}"));
    }
    out
}

/// The features a face name says made the face.
fn face_features(name: &str) -> Vec<u64> {
    persistent_name::FaceName::parse(name).map(|f| f.features().into_iter().map(|id| id.0).collect()).unwrap_or_default()
}

/// Where `anchor` stands in the session's model now.
pub(crate) fn found(s: &Session, st: &ModelState, anchor: &Anchor) -> Found {
    match anchor {
        Anchor::Face { body, maker, face, at, config } => {
            let gone_or_hidden = |what: &str| {
                let mut features = face_features(face);
                features.push(*maker);
                let was = Some(*at);
                match hidden_why(s, &features, config.as_deref(), what) {
                    Some(why) => Found::Hidden { why, was },
                    None => Found::Gone { why: format!("{what} was deleted or replaced"), was },
                }
            };
            let Some(b) = find_body(st, body, *maker) else { return gone_or_hidden(&format!("the body `{body}` this face was on")) };
            let faces = faces_named(b, face);
            if faces.is_empty() {
                return gone_or_hidden(&format!("the face of `{}` it was on", b.name));
            }
            let p = vec(*at);
            let r = reach(b);
            let mut best: Option<(Vec3, f64)> = None;
            for f in &faces {
                if let Some((q, d)) = nearest_on_face(b, *f, p)
                    && best.is_none_or(|(_, bd)| d < bd)
                {
                    best = Some((q, d));
                }
            }
            match best {
                Some((_, d)) if d <= r => Found::Anchored { at: Some(*at) },
                Some((q, d)) => Found::Moved { at: point(q), was: *at, moved_by: d },
                None => Found::Unchecked { why: format!("the face of `{}` could not be measured", b.name) },
            }
        }
        Anchor::Edge { body, maker, edge, at, config } => {
            let gone_or_hidden = |what: &str| {
                let mut features: Vec<u64> = edge_stem(edge).split('|').flat_map(face_features).collect();
                features.push(*maker);
                let was = Some(*at);
                match hidden_why(s, &features, config.as_deref(), what) {
                    Some(why) => Found::Hidden { why, was },
                    None => Found::Gone { why: format!("{what} was deleted or replaced"), was },
                }
            };
            let Some(b) = find_body(st, body, *maker) else { return gone_or_hidden(&format!("the body `{body}` this edge was on")) };
            let p = vec(*at);
            let edges: Vec<usize> =
                edge_face_names(b).iter().enumerate().filter(|(_, n)| !n.is_empty() && same_edge_faces(n, edge)).map(|(i, _)| i).collect();
            if edges.is_empty() {
                return gone_or_hidden(&format!("the edge of `{}` it was on", b.name));
            }
            let nearest = edges
                .iter()
                .filter_map(|i| solidcraft_kernel::edge_distance(&b.body, *i, p).ok().flatten().map(|d| (d, *i)))
                .min_by(|x, y| x.0.total_cmp(&y.0));
            match nearest {
                Some((d, _)) if d <= reach(b) => Found::Anchored { at: Some(*at) },
                Some((_, i)) => match near_edge(b, i, p) {
                    Some(q) => Found::Moved { at: point(q), was: *at, moved_by: q.dist(p) },
                    None => Found::Unchecked { why: format!("the edge of `{}` could not be measured", b.name) },
                },
                None => Found::Unchecked { why: format!("the edge of `{}` could not be measured", b.name) },
            }
        }
        Anchor::Body { body, maker, at, config } => match find_body(st, body, *maker) {
            Some(_) => Found::Anchored { at: *at },
            None => {
                let what = format!("the body `{body}`");
                match hidden_why(s, &[*maker], config.as_deref(), &what) {
                    Some(why) => Found::Hidden { why, was: *at },
                    None => Found::Gone { why: format!("{what} was deleted or replaced"), was: *at },
                }
            }
        },
        Anchor::Feature { feature, name, at } => match s.doc.features.iter().find(|f| f.id == *feature) {
            Some(_) => Found::Anchored { at: *at },
            None => Found::Gone { why: format!("the feature `{name}` was deleted"), was: *at },
        },
        Anchor::Occurrence { path, name, at } => match occurrence_name(s, path) {
            Some(_) => Found::Anchored { at: *at },
            None => Found::Gone { why: format!("the component `{name}` was removed from the assembly"), was: *at },
        },
        Anchor::Point { at } => Found::Anchored { at: Some(*at) },
        Anchor::View {} => Found::Anchored { at: None },
        Anchor::Unanchored { why, .. } => Found::Gone { why: why.clone(), was: None },
    }
}

/// One markup as `markup.list` gives it.
fn row(s: &Session, st: &ModelState, m: &Markup) -> Value {
    let thread: Vec<Value> = m.thread.iter().map(|t| json!({"by": t.by, "at": t.at, "text": t.text, "reviewer": t.by == m.by})).collect();
    let images: Vec<Value> = m
        .images
        .iter()
        .map(|i| json!({"file": i.file, "name": i.name, "by": i.by, "at": i.at, "bytes": i.bytes, "in_design": s.doc.markup_images.contains_key(&i.file)}))
        .collect();
    let mut v = json!({
        "id": m.id,
        "guid": m.guid,
        "status": m.status.word(),
        "note": m.note(),
        "thread": thread,
        "anchor": serde_json::to_value(&m.anchor).unwrap_or(Value::Null),
        "found": found(s, st, &m.anchor).to_value(),
        "images": images,
        "by": m.by,
        "made": m.made,
        "changed": m.changed,
    });
    if let Some(view) = &m.view {
        v["view"] = serde_json::to_value(view).unwrap_or(Value::Null);
    }
    if let Some(o) = &m.outcome {
        v["outcome"] = json!({"by": o.by, "reason": o.reason, "at": o.at});
    }
    v
}

fn markup_list(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.list";
    let path = set_path(s, p, CMD)?;
    let status = match str_(p, "status") {
        Some(w) => Some(Status::from_word(w).ok_or_else(|| bad(CMD, "`status` is open, resolved or rejected"))?),
        None => None,
    };
    let the_set = set::read(&path).map_err(|e| bad(CMD, e))?;
    let st = s.world_state()?;
    let count = |w: Status| the_set.markups.iter().filter(|m| m.status == w).count();
    let counts = json!({"open": count(Status::Open), "resolved": count(Status::Resolved), "rejected": count(Status::Rejected)});
    let markups: Vec<Value> = the_set.markups.iter().filter(|m| status.is_none_or(|w| m.status == w)).map(|m| row(s, &st, m)).collect();
    Ok(json!({ "set": path, "format": the_set.format, "counts": counts, "markups": markups }))
}

fn markup_add(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.add";
    let path = set_path(s, p, CMD)?;
    let by = who(p, CMD)?;
    let text = str_(p, "text").ok_or_else(|| bad(CMD, "`text` is required: the note"))?.to_string();
    set::check_text(&text, set::MAX_NOTE).map_err(|e| bad(CMD, e))?;
    let view = view_of(p, CMD)?;
    // A view given with nothing else named is the markup's place: Mark this on empty space means
    // the view, whatever happens to be selected.
    let anchor = match capture(s, p, CMD)? {
        Some(a) => a,
        None if view.is_some() => Anchor::View {},
        None => match from_selection(s) {
            Some(a) => a,
            None => {
                return Err(bad(
                    CMD,
                    "say what the markup is on: `face`, `edge`, `body`, `at`, `feature`, `occurrence` or `view` (or select one thing)",
                ));
            }
        },
    };
    let guid = new_guid();
    let at = now();
    let made = Markup {
        id: 0,
        guid: guid.clone(),
        status: Status::Open,
        thread: vec![Message { by: by.clone(), at, text }],
        anchor: anchor.clone(),
        view,
        images: Vec::new(),
        by,
        made: at,
        changed: at,
        outcome: None,
    };
    let id = set::update(&path, move |the_set| {
        if the_set.markups.len() >= set::MAX_MARKUPS {
            return Err(format!("a set holds at most {} markups", set::MAX_MARKUPS));
        }
        let id = the_set.next_id;
        the_set.next_id = id.checked_add(1).ok_or("the set has no numbers left")?;
        the_set.markups.push(Markup { id, ..made });
        Ok(id)
    })
    .map_err(|e| bad(CMD, e))?;
    let st = s.world_state()?;
    Ok(json!({
        "markup": id,
        "guid": guid,
        "set": path,
        "anchor": serde_json::to_value(&anchor).unwrap_or(Value::Null),
        "found": found(s, &st, &anchor).to_value(),
    }))
}

/// One change to markup `markup`, under the set's lock: `step` gets it and the time.
fn change(s: &Session, p: &Value, cmd: &str, step: impl FnOnce(&mut Markup, u64) -> std::result::Result<Value, String>) -> Result<Value> {
    let path = set_path(s, p, cmd)?;
    let at = now();
    let (id, status, extra) = set::update(&path, |the_set| {
        let id = which(the_set, p)?;
        let m = the_set.get_mut(id).ok_or_else(|| format!("no markup {id}"))?;
        let extra = step(m, at)?;
        m.changed = at;
        Ok((id, m.status, extra))
    })
    .map_err(|e| bad(cmd, e))?;
    let mut v = json!({ "markup": id, "status": status.word() });
    if let Value::Object(more) = extra {
        for (k, x) in more {
            v[k] = x;
        }
    }
    Ok(v)
}

/// Add a message to a markup's thread, under the limit for who wrote it.
fn say(m: &mut Markup, by: &str, text: &str, at: u64) -> std::result::Result<(), String> {
    set::check_text(text, m.limit_for(by))?;
    if m.thread.len() >= set::MAX_MESSAGES {
        return Err(format!("a thread holds at most {} messages", set::MAX_MESSAGES));
    }
    m.thread.push(Message { by: by.to_string(), at, text: text.to_string() });
    Ok(())
}

fn markup_edit(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.edit";
    let by = who(p, CMD)?;
    let text = str_(p, "text").map(str::to_string);
    if let Some(t) = &text {
        set::check_text(t, set::MAX_NOTE).map_err(|e| bad(CMD, e))?;
    }
    let anchor = capture(s, p, CMD)?;
    let view = view_of(p, CMD)?;
    if text.is_none() && anchor.is_none() && view.is_none() {
        return Err(bad(CMD, "give the note's new `text`, what it is on now, or its `view`"));
    }
    change(s, p, CMD, |m, _| {
        if let Some(t) = text {
            if by != m.by {
                return Err(format!("only the markup's maker ({}) changes its note: reply instead", m.by));
            }
            if let Some(first) = m.thread.first_mut() {
                first.text = t;
            }
        }
        if let Some(a) = anchor {
            m.anchor = a;
        }
        if let Some(v) = view {
            m.view = Some(v);
        }
        Ok(Value::Null)
    })
}

fn markup_reply(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.reply";
    let by = who(p, CMD)?;
    let text = str_(p, "text").ok_or_else(|| bad(CMD, "`text` is required: the reply"))?.to_string();
    change(s, p, CMD, |m, at| {
        say(m, &by, &text, at)?;
        Ok(Value::Null)
    })
}

fn close(s: &mut Session, p: &Value, cmd: &str, to: Status) -> Result<Value> {
    let by = who(p, cmd)?;
    let reason = str_(p, "reason").ok_or_else(|| bad(cmd, "`reason` is required: what was done, or why not"))?.to_string();
    change(s, p, cmd, |m, at| {
        if m.status != Status::Open {
            return Err(format!("markup {} is {} already: reopen it first", m.id, m.status.word()));
        }
        say(m, &by, &reason, at)?;
        m.status = to;
        m.outcome = Some(Outcome { by: by.clone(), reason: reason.clone(), at });
        Ok(Value::Null)
    })
}

fn markup_resolve(s: &mut Session, p: &Value) -> Result<Value> {
    close(s, p, "markup.resolve", Status::Resolved)
}

fn markup_reject(s: &mut Session, p: &Value) -> Result<Value> {
    close(s, p, "markup.reject", Status::Rejected)
}

fn markup_reopen(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.reopen";
    let by = who(p, CMD)?;
    let reason = str_(p, "reason").map(str::to_string);
    change(s, p, CMD, |m, at| {
        if m.status == Status::Open {
            return Err(format!("markup {} is open already", m.id));
        }
        if let Some(r) = &reason {
            say(m, &by, r, at)?;
        }
        m.status = Status::Open;
        m.outcome = None;
        Ok(Value::Null)
    })
}

fn markup_image(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.image";
    set_path(s, p, CMD)?;
    let by = who(p, CMD)?;
    let given = ["file", "remove"].iter().filter(|k| p.get(**k).is_some_and(|v| !v.is_null())).count();
    if given != 1 {
        return Err(bad(CMD, "give one of `file` (a PNG or JPEG) or `remove` (a picture's entry)"));
    }
    if let Some(file) = str_(p, "remove") {
        let f = file.to_string();
        let out = change(s, p, CMD, move |m, _| {
            let i = m.images.iter().position(|i| i.file == f).ok_or_else(|| format!("markup {} has no picture `{f}`", m.id))?;
            m.images.remove(i);
            Ok(json!({ "removed": f }))
        })?;
        // Out of the design once the set no longer names it.
        if s.doc.markup_images.contains_key(file) {
            s.doc_mut().markup_images.remove(file);
        }
        return Ok(out);
    }
    let file = str_(p, "file").filter(|f| !f.trim().is_empty()).ok_or_else(|| bad(CMD, "`file` must be a path"))?;
    let bytes = solidcraft_io::vfs::read(file, set::MAX_IMAGE + 1).map_err(|e| bad(CMD, format!("{file}: {e}")))?;
    if bytes.len() > set::MAX_IMAGE {
        return Err(bad(CMD, format!("a picture is at most {} bytes", set::MAX_IMAGE)));
    }
    let kind = set::image_kind(&bytes).ok_or_else(|| bad(CMD, "a picture must be a PNG or a JPEG"))?;
    let came = std::path::Path::new(file).file_name().map_or_else(|| "picture".to_string(), |n| n.to_string_lossy().to_string());
    let name = match str_(p, "name") {
        Some(n) if !n.trim().is_empty() && n.chars().count() <= 200 && !n.chars().any(char::is_control) => n.to_string(),
        Some(_) => return Err(bad(CMD, "`name` is 1 to 200 characters, with no control characters")),
        None => came.chars().filter(|c| !c.is_control()).take(200).collect(),
    };
    let images = &s.doc.markup_images;
    if images.len() >= solidcraft_doc::markup_image::MAX_FILES {
        return Err(bad(CMD, format!("a design holds at most {} review pictures", solidcraft_doc::markup_image::MAX_FILES)));
    }
    let total: u64 = images.values().map(|b| b.len() as u64).sum();
    if total.saturating_add(bytes.len() as u64) > solidcraft_doc::markup_image::MAX_TOTAL_BYTES {
        return Err(bad(CMD, format!("a design's review pictures take at most {} bytes together", solidcraft_doc::markup_image::MAX_TOTAL_BYTES)));
    }
    let entry = format!("{}.{kind}", new_guid());
    let image = Image { file: entry.clone(), name, by, at: now(), bytes: bytes.len() as u64 };
    // The set names it first: a set that refuses leaves the design as it was.
    let added = change(s, p, CMD, move |m, _| {
        if m.images.len() >= set::MAX_IMAGES {
            return Err(format!("a markup holds at most {} pictures", set::MAX_IMAGES));
        }
        let f = image.file.clone();
        m.images.push(image);
        Ok(json!({ "image": f }))
    })?;
    s.doc_mut().markup_images.insert(entry, bytes.into());
    Ok(added)
}

fn markup_delete(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "markup.delete";
    let path = set_path(s, p, CMD)?;
    let (id, files) = set::update(&path, |the_set| {
        let id = which(the_set, p)?;
        let i = the_set.markups.iter().position(|m| m.id == id).ok_or_else(|| format!("no markup {id}"))?;
        let m = the_set.markups.remove(i);
        Ok((id, m.images.into_iter().map(|i| i.file).collect::<Vec<_>>()))
    })
    .map_err(|e| bad(CMD, e))?;
    for f in files {
        if s.doc.markup_images.contains_key(&f) {
            s.doc_mut().markup_images.remove(&f);
        }
    }
    Ok(json!({ "deleted": id }))
}

#[cfg(test)]
#[path = "markups_tests.rs"]
mod tests;
