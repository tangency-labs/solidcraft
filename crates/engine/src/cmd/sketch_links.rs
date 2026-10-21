//! Managing a sketch's links (projected, intersected and included reference geometry): listing
//! them with their source and status, updating them where their sources are now, freezing and
//! resuming them, pointing one at a new source, hiding them and removing them.
//!
//! A link to another component reads an exact placement (`A:2`) and follows that placement's
//! *captured* position: a move made with `capture: false` (or a drag not yet captured) is
//! pending, and its links report `pending` until it is captured (Capture Position, or Update
//! References with `capture: true`). Every recompute shows links where their sources are; Update
//! References also stores that geometry in the sketch, so a source lost later leaves the last
//! geometry it had.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{LinkGeom, LinkKind, Sketch};

use super::sketch::{edit, target_sketch};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, str_, string_list};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.references", "Sketch References", references).icon("list").noundo().params(
        "sketch?: id|name (default: the active sketch, else every sketch with links). Each link: its source, the occurrence placement it \
         reads (path and name) and the placement of the sketch's component it was made in, its curves and points, hidden, frozen and \
         status: current; stale (the model shows it moved since it was stored: Update References stores it); pending (a placement it \
         reads has an uncaptured move, which it follows once captured); frozen; lost (with the reason; it keeps its last geometry)",
    ),
    CommandSpec::new("sketch.update_references", "Update References", update_references).icon("update").params(
        "sketch?: id|name (default: every sketch with links); capture?: bool (first capture the uncaptured moves of the placements \
         the links read); files?: bool (first update linked designs, one component at a time: a component whose file cannot be read \
         keeps its bodies and is reported unavailable). Reads each link where its source is now, solves and stores the sketch, \
         rebuilds what follows. Reports each link changed, unchanged, frozen or lost, and the features that failed (ok: false)",
    ),
    CommandSpec::new("sketch.freeze_link", "Freeze Link", freeze_link)
        .icon("ground")
        .params("link?: id | links?: [ids] | entities?: [curve or point ids]; sketch?: id|name (default: the active sketch). The links keep the geometry they show and stop following their sources until resumed"),
    CommandSpec::new("sketch.resume_link", "Resume Link", resume_link)
        .icon("play")
        .params("link?: id | links?: [ids] | entities?: [curve or point ids]; sketch?: id|name (default: the active sketch). Frozen links follow their sources again, from now"),
    CommandSpec::new("sketch.repair_link", "Repair Link", repair_link).icon("project").enabled(in_sketch).params(
        "link: id; ref: a reference as sketch.project takes it (with path?: [occurrence ids]). The link reads the new source; its \
         curves and points keep their ids, and the constraints on them, where the new geometry has the same shape",
    ),
    CommandSpec::new("sketch.remove_link", "Remove Projection", remove_link).icon("delete").params(
        "link?: id | links?: [ids] | entities?: [curve or point ids]; sketch?: id|name (default: the active sketch). Removes whole \
         links: their curves, points and 3D curves, and the constraints and dimensions on them (Break Link keeps the geometry instead)",
    ),
    CommandSpec::new("sketch.link_visibility", "Show/Hide Projection", link_visibility).icon("eye").params(
        "link?: id | links?: [ids] | entities?: [curve or point ids]; sketch?: id|name (default: the active sketch); visible?: bool \
         (default toggles). Hidden links are not drawn or picked; they still bound profiles, keep their constraints and follow their sources",
    ),
];

/// The links a command names: `link`, `links`, or the links that own `entities`. Text is not a
/// reference and is left to its own commands.
fn named_links(sk: &Sketch, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let add = |id: String, out: &mut Vec<String>| {
        if !out.contains(&id) {
            out.push(id);
        }
    };
    for key in ["links", "entities"] {
        if p.get(key).is_some_and(|v| !v.as_array().is_some_and(|a| a.iter().all(Value::is_string))) {
            return Err(bad(cmd, format!("`{key}` must list ids")));
        }
    }
    let mut given: Vec<String> = string_list(cmd, p, "links")?;
    given.extend(str_(p, "link").map(str::to_string));
    if given.len() > 10_000 {
        return Err(bad(cmd, "too many links"));
    }
    for l in given {
        if sk.link(&l).is_none() {
            return Err(bad(cmd, format!("no link `{l}`")));
        }
        add(l, &mut out);
    }
    for e in string_list(cmd, p, "entities")?.into_iter().take(10_000) {
        let l = sk
            .curves
            .iter()
            .find(|c| c.id == e)
            .and_then(|c| c.link.clone())
            .or_else(|| sk.points.iter().find(|q| q.id == e).and_then(|q| q.link.clone()))
            .or_else(|| sk.wires.iter().find(|w| w.id == e).and_then(|w| w.link.clone()))
            .ok_or_else(|| bad(cmd, format!("`{e}` is not linked geometry")))?;
        add(l, &mut out);
    }
    if out.is_empty() {
        return Err(bad(cmd, "give `link`, `links` or `entities`"));
    }
    if out.iter().any(|l| sk.link(l).is_some_and(|l| l.kind == LinkKind::Text)) {
        return Err(bad(cmd, "a text is not a reference (edit or explode the text instead)"));
    }
    Ok(out)
}

/// The sketches a command works on: `sketch`, else the active one, else every sketch with
/// references (when `all`), in timeline order.
fn target_sketches(s: &Session, p: &Value, cmd: &str, all: bool) -> Result<Vec<u64>> {
    if p.get("sketch").is_some() || s.active_sketch.is_some() || !all {
        return Ok(vec![target_sketch(s, p, cmd)?]);
    }
    Ok(s.doc.features.iter().filter(|f| s.doc.sketch(f.id).is_ok_and(|sk| sk.links.iter().any(|l| l.kind != LinkKind::Text))).map(|f| f.id).collect())
}

/// Where each of a link's points is, by id.
fn link_spots(sk: &Sketch, link: &str) -> Vec<(String, solidcraft_geom::Vec2)> {
    sk.link_points(link).iter().filter_map(|p| sk.points.get(*p)).map(|p| (p.id.clone(), p.pos)).collect()
}

fn moved(a: &[(String, solidcraft_geom::Vec2)], b: &[(String, solidcraft_geom::Vec2)]) -> bool {
    a.len() != b.len() || a.iter().zip(b).any(|((ia, pa), (ib, pb))| ia != ib || pa.dist(*pb) > 1e-9)
}

fn placement_json(s: &Session, path: &[u64]) -> Value {
    json!({"path": path, "name": solidcraft_doc::project::path_label(&s.doc, path)})
}

/// The links a deletion of `ids` (curve and point ids of `sk`) takes members from: the links
/// owning them, and those owning curves on a point that goes.
pub(super) fn touched_links(sk: &Sketch, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |link: Option<&String>| {
        if let Some(link) = link
            && !out.contains(link)
        {
            out.push(link.clone());
        }
    };
    for id in ids.iter().take(10_000) {
        if let Some(c) = sk.curves.iter().find(|c| c.id == *id) {
            add(c.link.as_ref());
        } else if let Some(q) = sk.point_index(id) {
            add(sk.points.get(q).and_then(|p| p.link.as_ref()));
            for c in sk.curves.iter().filter(|c| c.kind.point_ids().contains(&q)) {
                add(c.link.as_ref());
            }
        }
    }
    out
}

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let t = if ab.len2() > 0.0 { ((p - a).dot(ab) / ab.len2()).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + ab * t)
}

/// Is `p` on curve `ci` of `sk`, within `tol`: on a line's segment, on an arc between its
/// ends (not elsewhere on its circle), on a circle; on the drawn path of any other curve.
fn on_curve(sk: &Sketch, ci: usize, p: Vec2, tol: f64) -> bool {
    match sk.curve_link_geom(ci) {
        Some(LinkGeom::Line(a, b)) => seg_dist(p, a, b) <= tol,
        Some(LinkGeom::Circle(c, r)) => (p.dist(c) - r).abs() <= tol,
        Some(LinkGeom::Arc { c, a, b }) => {
            let r = c.dist(a);
            if (p.dist(c) - r).abs() > tol || r <= tol {
                return false;
            }
            let tau = std::f64::consts::TAU;
            let start = (a - c).angle();
            let sweep = ((b - c).angle() - start).rem_euclid(tau);
            let at = ((p - c).angle() - start).rem_euclid(tau);
            let slack = tol / r;
            at <= sweep + slack || at >= tau - slack
        }
        Some(_) => {
            let path = sk.polyline(ci);
            let size = path.iter().zip(path.iter().skip(1)).map(|(a, b)| a.dist(*b)).sum::<f64>();
            path.windows(2).any(|w| matches!(w, [a, b] if seg_dist(p, *a, *b) <= tol.max(size * 1e-3)))
        }
        None => false,
    }
}

/// Does piece `g` lie on curve `ci` of `sk` (its ends and middle within `tol` of it)?
fn piece_on(sk: &Sketch, ci: usize, g: &LinkGeom, tol: f64) -> bool {
    // Points on the piece itself: a conic's or a control-point spline's `spot` is a control
    // point, off the curve (a solid's exact Bézier pieces are such splines).
    let probe = match g {
        LinkGeom::Point(p) => vec![*p],
        LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } => vec![g.spot(), *a, *b],
        LinkGeom::Conic { a, apex, b, rho } => vec![solidcraft_sketch::conic_point(*a, *apex, *b, *rho, 0.5), *a, *b],
        LinkGeom::Circle(c, r) => vec![g.spot(), *c - Vec2::X * *r, *c + Vec2::Y * *r],
        LinkGeom::Ellipse { major, .. } => vec![*major],
        LinkGeom::Spline { pts, control, degree, periodic } => {
            [0.0, 0.5, 1.0].map(|t| solidcraft_sketch::spline_point_mode(pts, *control, *degree, *periodic, t)).to_vec()
        }
    };
    probe.iter().all(|p| on_curve(sk, ci, *p, tol))
}

/// Before curves and points of a sketch are deleted: each link that keeps some of its members
/// records the source edges of the members going ([`solidcraft_sketch::Link::omit_edges`], by
/// their persistent names), so exactly those stay deleted when it is next updated, however
/// its source moves or is resized. A curve on a point that goes, goes too.
///
/// Only whole source edges of a face's boundary can be told apart that way. Deleting part of a
/// projection that is not, or cannot be (part of one source edge, one of several edges between
/// the same two faces that are numbered by where they lie, an edge whose faces' names are not
/// exact, a silhouette, a section, a mesh body), is refused: Remove Projection removes it
/// whole, Break Link makes its geometry editable.
pub(super) fn omit_removed(
    sk: &mut Sketch,
    pieces: &std::collections::BTreeMap<String, std::result::Result<Vec<solidcraft_doc::project::EdgePieces>, String>>,
    curves: &[usize],
    points: &[usize],
    cmd: &str,
) -> Result<()> {
    let going: std::collections::BTreeSet<usize> = sk
        .curves
        .iter()
        .enumerate()
        .filter(|(i, c)| curves.contains(i) || c.kind.point_ids().iter().any(|q| points.contains(q)))
        .map(|(i, _)| i)
        .collect();
    let mut add: Vec<(String, String)> = Vec::new();
    for l in sk.links.iter().filter(|l| l.kind != LinkKind::Text) {
        let link_curves = sk.link_curves(&l.id);
        let on_curves: std::collections::BTreeSet<usize> =
            link_curves.iter().filter_map(|c| sk.curves.get(*c)).flat_map(|c| c.kind.point_ids()).collect();
        let lone: Vec<usize> = sk.link_points(&l.id).into_iter().filter(|q| !on_curves.contains(q)).collect();
        let deleted: Vec<usize> = link_curves.iter().copied().filter(|c| going.contains(c)).collect();
        let lone_gone = lone.iter().any(|q| points.contains(q));
        let survives = link_curves.iter().any(|c| !going.contains(c)) || lone.iter().any(|q| !points.contains(q));
        if !survives || (deleted.is_empty() && !lone_gone) {
            continue;
        }
        let whole = |why: &str| {
            bad(cmd, format!("the {} is kept whole ({why}): Remove Projection removes it, Break Link makes its curves editable", l.source.describe()))
        };
        if lone_gone {
            return Err(whole("its points belong to it"));
        }
        let edges = match pieces.get(&l.id) {
            Some(Ok(edges)) => edges,
            Some(Err(why)) => return Err(whole(why)),
            None => return Err(whole("its source edges are not known")),
        };
        let scale = sk.points.iter().fold(1.0_f64, |m, p| m.max(p.pos.x.abs()).max(p.pos.y.abs()));
        let tol = scale * 1e-7;
        // Which deleted curve each piece of each source edge lies on, if any.
        let on: Vec<Vec<Option<usize>>> =
            edges.iter().map(|e| e.geom.iter().map(|g| deleted.iter().copied().find(|c| piece_on(sk, *c, g, tol))).collect()).collect();
        let covered = |i: usize| on.get(i).is_some_and(|pieces| !pieces.is_empty() && pieces.iter().all(Option::is_some));
        for (i, pieces) in on.iter().enumerate() {
            if pieces.iter().any(Option::is_some) && !covered(i) {
                return Err(whole("one source edge gives more curves than the ones deleted: delete them together"));
            }
        }
        for c in &deleted {
            if !on.iter().enumerate().any(|(i, pieces)| covered(i) && pieces.contains(&Some(*c))) {
                return Err(whole("a deleted curve is not one of its face's source edges"));
            }
        }
        for (i, e) in edges.iter().enumerate().filter(|(i, _)| covered(*i)) {
            if !e.exact {
                return Err(whole("its source edge's name is not exact"));
            }
            let family = solidcraft_doc::project::omit_name(&e.name);
            // One of a family of edges told apart only by numbers (several edges between the same
            // two faces, or the pieces of a split face, such as a hole's two halves): the numbers
            // may pass between them, so only the whole family is kept out.
            if edges.iter().enumerate().any(|(j, other)| j != i && solidcraft_doc::project::omit_name(&other.name) == family && !covered(j)) {
                return Err(whole("its source edge is one of several told apart only by number: delete them together"));
            }
            add.push((l.id.clone(), family));
        }
    }
    for (id, name) in add {
        if let Some(l) = sk.links.iter_mut().find(|l| l.id == id)
            && !l.omit_edges.contains(&name)
        {
            if l.omit_edges.len() >= solidcraft_sketch::MAX_LINK_OMIT {
                return Err(bad(cmd, "too many deleted members of one projection"));
            }
            l.omit_edges.push(name);
        }
    }
    Ok(())
}

fn references(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.references";
    let mut out = Vec::new();
    for id in target_sketches(s, p, cmd, true)? {
        let stored = s.doc.sketch(id)?;
        let result = s.model.result(id);
        let evaluated = result.and_then(|r| r.state.sketch(id)).map(|ss| &ss.sketch);
        let name = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
        for l in stored.links.iter().filter(|l| l.kind != LinkKind::Text) {
            let shown = evaluated.and_then(|sk| sk.link(&l.id));
            let lost = shown.map_or(l.lost, |x| x.lost);
            let paths = l.placement.iter().flat_map(|p| p.source.iter().chain(&p.author));
            let pending = paths.clone().any(|o| s.pending_moves.contains_key(o));
            let stale = evaluated.is_some_and(|sk| moved(&link_spots(stored, &l.id), &link_spots(sk, &l.id)));
            let status = if l.frozen {
                "frozen"
            } else if lost {
                "lost"
            } else if pending {
                "pending"
            } else if stale {
                "stale"
            } else {
                "current"
            };
            let curves: Vec<&str> = stored.link_curves(&l.id).iter().filter_map(|c| stored.curves.get(*c)).map(|c| c.id.as_str()).collect();
            let points: Vec<&str> = stored.link_points(&l.id).iter().filter_map(|q| stored.points.get(*q)).map(|q| q.id.as_str()).collect();
            let construction = !curves.is_empty() && stored.link_curves(&l.id).iter().all(|c| stored.curves.get(*c).is_some_and(|c| c.construction));
            let mut entry = json!({
                "sketch": id,
                "sketch_name": name,
                "link": l.id,
                "kind": l.kind,
                "source": l.source.describe(),
                "bodies": l.source.bodies(),
                "occurrence": l.placement.as_ref().map(|p| placement_json(s, &p.source)),
                "author": l.placement.as_ref().map(|p| placement_json(s, &p.author)),
                "status": status,
                "hidden": l.hidden,
                "frozen": l.frozen,
                "construction": construction,
                "omitted": l.omit_edges,
                "curves": curves,
                "points": points,
            });
            if lost && let Some(w) = result.and_then(|r| r.warning.clone().or_else(|| r.error.clone())) {
                // The sketch's warnings, each once.
                let mut parts: Vec<&str> = Vec::new();
                for part in w.split("; ") {
                    if !parts.contains(&part) {
                        parts.push(part);
                    }
                }
                entry["reason"] = json!(parts.join("; "));
            }
            out.push(entry);
        }
    }
    Ok(json!({"references": out}))
}

fn update_references(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.update_references";
    let targets = target_sketches(s, p, cmd, true)?;
    let mut files = Value::Null;
    if bool_(p, "files").unwrap_or(false) {
        let update = super::find_command("component.update_links").ok_or_else(|| bad(cmd, "component.update_links is missing"))?;
        let (mut updated, mut unchanged, mut unavailable) = (Vec::new(), Vec::new(), Vec::new());
        let linked: Vec<u64> = s.doc.links.keys().copied().collect();
        for comp in linked {
            match update.run_nested(s, &json!({"component": comp})) {
                Ok(v) => {
                    updated.extend(v["updated"].as_array().cloned().unwrap_or_default());
                    unchanged.extend(v["unchanged"].as_array().cloned().unwrap_or_default());
                }
                // Its bodies stay as they were: usable, but not known to be current.
                Err(e) => unavailable.push(json!({"component": comp, "error": e.to_string()})),
            }
        }
        files = json!({"updated": updated, "unchanged": unchanged, "unavailable": unavailable});
    }
    let mut captured: Vec<u64> = Vec::new();
    if bool_(p, "capture").unwrap_or(false) {
        let read: std::collections::BTreeSet<u64> = targets
            .iter()
            .filter_map(|id| s.doc.sketch(*id).ok())
            .flat_map(|sk| sk.links.iter().filter_map(|l| l.placement.clone()))
            .flat_map(|p| p.source.into_iter().chain(p.author))
            .collect();
        let moves: Vec<(u64, solidcraft_doc::Mat)> = s.pending_moves.iter().filter(|(o, _)| read.contains(o)).map(|(o, m)| (*o, *m)).collect();
        for (o, _) in &moves {
            s.pending_moves.remove(o);
        }
        let doc = s.doc_mut();
        for (o, m) in &moves {
            if let Some(x) = doc.occurrences.iter_mut().find(|x| x.id == *o) {
                x.transform = *m;
            }
        }
        captured = moves.into_iter().map(|(o, _)| o).collect();
    }
    s.refresh();
    let mut links = Vec::new();
    let mut lost = false;
    for id in &targets {
        let mut doc = (*s.doc).clone();
        let before = doc.sketch(*id)?.clone();
        let mut sk = before.clone();
        let warnings = super::sketch_project::refresh(s, &doc, *id, &mut sk);
        for l in sk.links.iter().filter(|l| l.kind != LinkKind::Text) {
            let status = if l.frozen {
                "frozen"
            } else if l.lost {
                lost = true;
                "lost"
            } else if moved(&link_spots(&before, &l.id), &link_spots(&sk, &l.id)) || before.link(&l.id).map(|b| &b.source) != Some(&l.source) {
                "changed"
            } else {
                "unchanged"
            };
            links.push(json!({"sketch": id, "link": l.id, "source": l.source.describe(), "status": status}));
        }
        if sk != before {
            *doc.sketch_mut(*id)? = sk;
            *s.doc_mut() = doc;
            s.refresh();
        }
        if !warnings.is_empty() {
            links.push(json!({"sketch": id, "warnings": warnings}));
        }
    }
    // What follows the first updated sketch is rebuilt by the refresh; report what failed.
    let first = targets.iter().filter_map(|id| s.doc.features.iter().position(|f| f.id == *id)).min();
    let failed: Vec<Value> = match first {
        Some(at) => s
            .doc
            .features
            .iter()
            .skip(at)
            .filter_map(|f| s.model.result(f.id).and_then(|r| r.error.clone()).map(|e| json!({"feature": f.id, "name": f.name, "error": e})))
            .collect(),
        None => Vec::new(),
    };
    let unavailable = files.get("unavailable").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    Ok(json!({
        "links": links,
        "captured": captured,
        "files": files,
        "failed": failed,
        "ok": failed.is_empty() && !lost && !unavailable,
    }))
}

fn freeze_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.freeze_link";
    // Frozen where the model shows it now: `edit` reads the links first.
    let (links, info) = edit(s, p, cmd, false, |sk, _| {
        let links = named_links(sk, p, cmd)?;
        for l in &links {
            sk.set_link_frozen(l, true)?;
        }
        Ok(links)
    })?;
    Ok(json!({"frozen": links, "sketch": info}))
}

fn resume_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.resume_link";
    let id = target_sketch(s, p, cmd)?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    let links = named_links(&sk, p, cmd)?;
    for l in &links {
        sk.set_link_frozen(l, false)?;
    }
    let warnings = super::sketch_project::refresh(s, &doc, id, &mut sk);
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    Ok(json!({"resumed": links, "warnings": warnings}))
}

fn repair_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.repair_link";
    let id = s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let link = str_(p, "link").ok_or_else(|| bad(cmd, "`link` must name the link to repair"))?.to_string();
    let reference = p.get("ref").ok_or_else(|| bad(cmd, "`ref` must give the new source"))?;
    let mut doc = (*s.doc).clone();
    let mut sk = doc.sketch(id)?.clone();
    let old = sk.link(&link).cloned().ok_or_else(|| bad(cmd, format!("no link `{link}`")))?;
    if old.kind == LinkKind::Text {
        return Err(bad(cmd, "a text is not a reference"));
    }
    // The new source, resolved as a new link of its own in a scratch copy of the sketch.
    let mut others = sk.clone();
    others.links.retain(|l| l.id != link);
    let (src, placement) = super::sketch_project::pick(s, &doc, id, &others, reference, cmd)?;
    super::sketch_project::refresh(s, &doc, id, &mut sk);
    let mut scratch = sk.clone();
    let what = src.describe();
    let fresh = super::sketch_project::add_link(s, &doc, id, &mut scratch, old.kind, src, placement).map_err(|e| bad(cmd, format!("{what}: {e}")))?;
    let new = scratch.link(&fresh).cloned().ok_or_else(|| bad(cmd, "the new source gave nothing"))?;
    let curves = scratch.link_curves(&fresh);
    let used: std::collections::BTreeSet<usize> = curves.iter().filter_map(|c| scratch.curves.get(*c)).flat_map(|c| c.kind.point_ids()).collect();
    let mut geom: Vec<LinkGeom> = curves.iter().filter_map(|c| scratch.curve_link_geom(*c)).collect();
    geom.extend(scratch.link_points(&fresh).into_iter().filter(|q| !used.contains(q)).filter_map(|q| scratch.point(q)).map(LinkGeom::Point));
    let wires: Vec<Vec<solidcraft_geom::Vec3>> =
        scratch.link_wires(&fresh).iter().filter_map(|w| scratch.wires.get(*w).map(|w| w.pts.clone())).collect();
    if let Some(l) = sk.links.iter_mut().find(|l| l.id == link) {
        l.source = new.source;
        l.face_name = new.face_name;
        l.edge_name = new.edge_name;
        l.placement = new.placement;
        l.lost = false;
        l.frozen = false;
        // Deleted members were edges of the old source.
        l.omit_edges.clear();
    }
    sk.set_link_wires(&link, &wires)?;
    let rebuilt = sk.update_link(&link, &geom)?;
    *doc.sketch_mut(id)? = sk;
    *s.doc_mut() = doc;
    let sk = s.doc.sketch(id)?;
    let curves: Vec<&str> = sk.link_curves(&link).iter().filter_map(|c| sk.curves.get(*c)).map(|c| c.id.as_str()).collect();
    let points: Vec<&str> = sk.link_points(&link).iter().filter_map(|q| sk.points.get(*q)).map(|q| q.id.as_str()).collect();
    Ok(json!({"link": link, "rebuilt": rebuilt, "curves": curves, "points": points}))
}

fn remove_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.remove_link";
    let (removed, info) = edit(s, p, cmd, false, |sk, doc| {
        let links = named_links(sk, p, cmd)?;
        for l in &links {
            let wires: Vec<String> = sk.link_wires(l).iter().filter_map(|w| sk.wires.get(*w).map(|w| w.id.clone())).collect();
            sk.remove_wires(&wires);
            let points: Vec<String> = sk.link_points(l).iter().filter_map(|q| sk.points.get(*q).map(|q| q.id.clone())).collect();
            let curves = sk.link_curves(l);
            sk.remove_curves(&curves);
            let points: Vec<usize> = points.iter().filter_map(|q| sk.point_index(q)).collect();
            sk.remove_points(&points);
            // A link left without entities (only 3D curves, say) goes too.
            sk.links.retain(|x| x.id != *l);
        }
        doc.prune_model_params();
        Ok(links)
    })?;
    s.doc_mut().prune_model_params();
    Ok(json!({"removed": removed, "sketch": info}))
}

fn link_visibility(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.link_visibility";
    let want = bool_(p, "visible");
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let links = named_links(sk, p, cmd)?;
        let visible = want.unwrap_or_else(|| links.iter().any(|l| sk.link(l).is_some_and(|l| l.hidden)));
        for l in sk.links.iter_mut().filter(|l| links.contains(&l.id)) {
            l.hidden = !visible;
        }
        Ok(json!({"links": links, "visible": visible}))
    })?;
    Ok(json!({"links": out["links"], "visible": out["visible"], "sketch": info}))
}

#[cfg(test)]
#[path = "sketch_placed_link_tests.rs"]
mod placed_tests;

#[cfg(test)]
#[path = "sketch_link_manage_tests.rs"]
mod manage_tests;
