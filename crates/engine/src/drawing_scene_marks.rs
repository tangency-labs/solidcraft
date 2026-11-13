//! Manual centre marks and lines on a sheet. References are resolved from the current model; a
//! lost reference has no strokes and remains inspectable through its command.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec2;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::auto_marks::{CircularEdge, MAX_GROUP_EDGES, MarkCandidate, circular_edges, group_candidates, resolve_exclusions};
use solidcraft_doc::drawing::view::ResolvedView;
use solidcraft_geom::drawing_centre_mark;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;

use super::CMD;
use crate::drawing_view::{self, ShownSet, Unmeasured};
use crate::params::bad;
use crate::{Result, Session};

// Paper millimetres, independent of view scale. A future drawing style owns these choices.
const WIDTH_MM: f64 = 0.18;
const AUTO_OVERSHOOT_MM: f64 = 2.0;
fn apart_why(apart: crate::drawing_view::Apart) -> String {
    format!("not supported yet: automatic centre marks on {}", apart.view())
}

/// One annotation and either its view-local paper group or the reason it has no group.
#[derive(Clone, Debug)]
pub(super) struct Mark {
    pub object: u64,
    pub view: u64,
    pub kind: ObjectKind,
    pub group: Option<Group>,
    pub why: Option<String>,
}

fn missing(status: solidcraft_doc::drawing::dimension::Status) -> String {
    use solidcraft_doc::drawing::dimension::Status;
    match status {
        Status::Failed(why) | Status::Lost(why, _) | Status::Unmeasurable(why) => why,
        Status::Ambiguous(_) => "its reference is ambiguous".into(),
        Status::Suppressed => "its reference is suppressed".into(),
        _ => "its reference has no drawable geometry".into(),
    }
}

pub(super) fn groups(
    s: &Session,
    drawing: &Drawing,
    frames: &BTreeMap<u64, (ViewFrame, Vec2)>,
    shown: Option<&std::result::Result<ShownSet, Unmeasured>>,
) -> Vec<Mark> {
    let view_ids: BTreeSet<_> = frames.keys().copied().collect();
    let marks: Vec<_> = drawing.centre_marks.iter().filter(|mark| view_ids.contains(&mark.view)).collect();
    let lines: Vec<_> = drawing.centre_lines.iter().filter(|line| view_ids.contains(&line.view)).collect();
    if marks.is_empty() && lines.is_empty() && !view_ids.iter().any(|id| drawing.view(*id).is_some_and(|view| view.centre_marks.is_some())) {
        return Vec::new();
    }
    let wanted: Vec<_> = marks.iter().map(|mark| mark.id).collect();
    let resolved = s.doc.resolve_centre_marks(&s.model, drawing, &wanted);
    let wanted_lines: Vec<_> = lines.iter().map(|line| line.id).collect();
    let resolved_lines = s.doc.resolve_centre_lines(&s.model, drawing, &wanted_lines);
    let mut groups = Vec::new();
    for (i, mark) in marks.into_iter().enumerate() {
        let result = if let Some(apart) = crate::drawing_view::draws_apart(drawing, mark.view) {
            Err(format!("centre annotations on {} are not supported yet", apart.view()))
        } else {
            match (frames.get(&mark.view), &resolved) {
                (Some((frame, at)), Ok(found)) => match found.get(i) {
                    Some(found) => match (found.centre, found.radius_mm) {
                        (Some(centre), Some(radius)) => {
                            group(mark.id, frame.to_paper(centre) + *at, radius * frame.scale().factor(), mark.overshoot_mm)
                                .map_err(|e| e.to_string())
                        }
                        _ => Err(missing(found.status.clone())),
                    },
                    None => Err("centre mark resolution returned no result".into()),
                },
                (_, Err(e)) => Err(e.to_string()),
                _ => Err("its view has no frame".into()),
            }
        };
        let (group, why) = match result {
            Ok(group) => (Some(group), None),
            Err(why) => (None, Some(why)),
        };
        groups.push(Mark { object: mark.id, view: mark.view, kind: ObjectKind::CentreMark, group, why });
    }
    for (i, line) in lines.into_iter().enumerate() {
        let result = if let Some(apart) = crate::drawing_view::draws_apart(drawing, line.view) {
            Err(format!("centre annotations on {} are not supported yet", apart.view()))
        } else {
            match (frames.get(&line.view), &resolved_lines) {
                (Some((frame, at)), Ok(found)) => match found.get(i) {
                    Some(found) => match found.points {
                        Some(points) => {
                            drawing_centre_mark::centre_line(frame.to_paper(points[0]) + *at, frame.to_paper(points[1]) + *at, line.overshoot_mm)
                                .map(|segment| Group { object: line.id, kind: ObjectKind::CentreLine, items: vec![stroke(segment)] })
                                .map_err(|e| e.to_string())
                        }
                        None => Err(missing(found.status.clone())),
                    },
                    None => Err("centre line resolution returned no result".into()),
                },
                (_, Err(e)) => Err(e.to_string()),
                _ => Err("its view has no frame".into()),
            }
        };
        let (group, why) = match result {
            Ok(group) => (Some(group), None),
            Err(why) => (None, Some(why)),
        };
        groups.push(Mark { object: line.id, view: line.view, kind: ObjectKind::CentreLine, group, why });
    }
    for (id, (frame, at)) in frames {
        let Some(rule) = drawing.view(*id).and_then(|view| view.centre_marks.as_ref()) else { continue };
        let result = (|| -> Result<Vec<Mark>> {
            let view = drawing.resolve_view(*id).map_err(|e| bad(CMD, e.to_string()))?;
            // A saved rule cannot scan a view that draws its parts in another placement.
            if let Some(apart) = crate::drawing_view::Apart::of(&view) {
                return Err(bad(CMD, apart_why(apart)));
            }
            let shown = match shown {
                None | Some(Err(Unmeasured::NoBodies)) => return Ok(Vec::new()),
                Some(Ok(shown)) => shown,
                Some(Err(why)) => return Err(bad(CMD, format!("cannot scan shown bodies: {}", why.name()))),
            };
            let (edges, candidates) = scan(&shown.of_view(&view), &view)?;
            let resolved = resolve_exclusions(&s.doc, &s.model, &rule.exclude).map_err(|e| bad(CMD, e.to_string()))?;
            let by_index: BTreeMap<_, _> = edges
                .iter()
                .map(|edge| ((edge.edge.body.clone(), edge.edge.origin_feature, edge.edge.path.clone(), edge.index), &edge.edge))
                .collect();
            let mut excluded = BTreeSet::new();
            for (saved, found) in rule.exclude.iter().zip(resolved) {
                if let Some(index) = found.edge_index
                    && let Some(edge) = by_index.get(&(saved.body.clone(), saved.origin_feature, saved.path.clone(), index))
                {
                    excluded.insert((edge.body.clone(), edge.origin_feature, edge.path.clone(), edge.name.clone()));
                }
            }
            Ok(candidates
                .into_iter()
                .filter(|mark| {
                    !mark
                        .sources
                        .iter()
                        .any(|edge| excluded.contains(&(edge.body.clone(), edge.origin_feature, edge.path.clone(), edge.name.clone())))
                })
                .map(|mark| {
                    let centre = frame.to_paper(view.basis.to_world(mark.center)) + *at;
                    let result = group(*id, centre, mark.radius * frame.scale().factor(), AUTO_OVERSHOOT_MM);
                    automatic(*id, result.map_err(|e| e.to_string()))
                })
                .collect())
        })();
        match result {
            Ok(marks) => groups.extend(marks),
            Err(err) => {
                let msg = match &err {
                    crate::EngineError::BadParams { msg, .. } => msg.clone(),
                    _ => err.to_string(),
                };
                groups.push(automatic(*id, Err(msg)));
            }
        }
    }
    groups
}

fn automatic(view: u64, result: std::result::Result<Group, String>) -> Mark {
    let (group, why) = match result {
        Ok(group) => (Some(group), None),
        Err(why) => (None, Some(why)),
    };
    Mark { object: view, view, kind: ObjectKind::CentreMark, group, why }
}

/// Explain annotations whose view has no drawable frame.
pub(super) fn unframed(drawing: &Drawing, id: u64, status: &super::Status) -> Vec<Mark> {
    let apart = crate::drawing_view::draws_apart(drawing, id);
    let manual_why =
        apart.map_or_else(|| "its view has no frame".to_string(), |apart| format!("centre annotations on {} are not supported yet", apart.view()));
    let mut marks: Vec<_> = drawing
        .centre_marks
        .iter()
        .filter(|mark| mark.view == id)
        .map(|mark| Mark { object: mark.id, view: id, kind: ObjectKind::CentreMark, group: None, why: Some(manual_why.clone()) })
        .collect();
    marks.extend(drawing.centre_lines.iter().filter(|line| line.view == id).map(|line| Mark {
        object: line.id,
        view: id,
        kind: ObjectKind::CentreLine,
        group: None,
        why: Some(manual_why.clone()),
    }));
    if drawing.view(id).is_some_and(|view| view.centre_marks.is_some()) {
        let reason = if let Some(apart) = apart {
            apart_why(apart)
        } else {
            format!(
                "cannot draw automatic centre marks without a drawable view frame: {}",
                status.reason().unwrap_or_else(|| "view was left out".into())
            )
        };
        marks.push(automatic(id, Err(reason)));
    }
    marks
}

/// Eligible sources for a saved view, independent of exclusions.
pub(super) fn candidates(s: &Session, id: u64) -> Result<Vec<MarkCandidate>> {
    let drawing = s.drawing(CMD)?;
    let view = drawing.resolve_view(id).map_err(|e| bad(CMD, e.to_string()))?;
    if let Some(apart) = crate::drawing_view::Apart::of(&view) {
        return Err(bad(CMD, apart_why(apart)));
    }
    let copy = (!s.pending_moves.is_empty()).then(|| super::designed(s));
    let shown = match drawing_view::shown(copy.as_ref().unwrap_or(s)) {
        Ok(shown) => shown,
        Err(Unmeasured::NoBodies) => return Ok(Vec::new()),
        Err(why) => return Err(bad(CMD, format!("cannot scan shown bodies: {}", why.name()))),
    };
    scan(&shown.of_view(&view), &view).map(|(_, marks)| marks)
}

fn scan(shown: &ShownSet, view: &ResolvedView) -> Result<(Vec<CircularEdge>, Vec<MarkCandidate>)> {
    let mut edges = Vec::new();
    for item in &shown.bodies {
        let Some(body) = shown.body(item) else { return Err(bad(CMD, "a shown body no longer exists")) };
        if body.body.is_mesh() {
            continue;
        }
        edges.extend(circular_edges(body, &item.path, &view.basis).map_err(|e| bad(CMD, e.to_string()))?);
        if edges.len() > MAX_GROUP_EDGES {
            return Err(bad(CMD, format!("more than {MAX_GROUP_EDGES} automatic-mark edges to group")));
        }
    }
    let marks = group_candidates(&edges).map_err(|e| bad(CMD, e.to_string()))?;
    Ok((edges, marks))
}

/// The common paper construction for manual marks and later automatic marks.
pub(super) fn group(id: u64, centre: Vec2, radius: f64, overshoot: f64) -> Result<Group> {
    let lines = drawing_centre_mark::strokes(centre, radius, overshoot).map_err(|e| bad(CMD, e.to_string()))?;
    let items = lines.into_iter().map(stroke).collect();
    Ok(Group { object: id, kind: ObjectKind::CentreMark, items })
}

fn stroke(line: solidcore_record::Seg2) -> Item {
    let long = 3.6;
    let lengths_mm = vec![long, 0.54, 0.18, 0.54];
    let period = lengths_mm.iter().sum();
    let phase_mm = (long / 2.0 - line.length() / 2.0).rem_euclid(period);
    Item::Stroke { path: vec![line], closed: false, width_mm: WIDTH_MM, pattern: Some(Dashes { lengths_mm, phase_mm }) }
}
