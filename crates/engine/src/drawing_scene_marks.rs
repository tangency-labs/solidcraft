//! Manual centre marks and lines on a sheet. References are resolved from the current model; a
//! lost reference has no strokes and remains inspectable through its command.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec2;
use solidcraft_doc::drawing::Drawing;
use solidcraft_geom::drawing_centre_mark;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;

use super::CMD;
use crate::params::bad;
use crate::{Result, Session};

// Paper millimetres, independent of view scale. A future drawing style owns these choices.
const WIDTH_MM: f64 = 0.18;

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

pub(super) fn groups(s: &Session, drawing: &Drawing, frames: &BTreeMap<u64, (ViewFrame, Vec2)>) -> Vec<Mark> {
    let view_ids: BTreeSet<_> = frames.keys().copied().collect();
    let marks: Vec<_> = drawing.centre_marks.iter().filter(|mark| view_ids.contains(&mark.view)).collect();
    let lines: Vec<_> = drawing.centre_lines.iter().filter(|line| view_ids.contains(&line.view)).collect();
    if marks.is_empty() && lines.is_empty() {
        return Vec::new();
    }
    let wanted: Vec<_> = marks.iter().map(|mark| mark.id).collect();
    let resolved = s.doc.resolve_centre_marks(&s.model, drawing, &wanted);
    let wanted_lines: Vec<_> = lines.iter().map(|line| line.id).collect();
    let resolved_lines = s.doc.resolve_centre_lines(&s.model, drawing, &wanted_lines);
    let mut groups = Vec::new();
    for (i, mark) in marks.into_iter().enumerate() {
        let result = if let Some(apart) = crate::drawing_view::unsupported_centre_view(drawing, mark.view) {
            Err(format!("centre annotations on {apart} are not supported yet"))
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
        let result = if let Some(apart) = crate::drawing_view::unsupported_centre_view(drawing, line.view) {
            Err(format!("centre annotations on {apart} are not supported yet"))
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
    groups
}

/// Saved annotations on a view without a drawable frame are omitted with a reason.
pub(super) fn unframed(drawing: &Drawing, view: u64) -> Vec<Mark> {
    let why = crate::drawing_view::unsupported_centre_view(drawing, view)
        .map_or_else(|| "its view has no frame".to_string(), |apart| format!("centre annotations on {apart} are not supported yet"));
    drawing
        .centre_marks
        .iter()
        .filter(|mark| mark.view == view)
        .map(|mark| Mark { object: mark.id, view, kind: ObjectKind::CentreMark, group: None, why: Some(why.clone()) })
        .chain(drawing.centre_lines.iter().filter(|line| line.view == view).map(|line| Mark {
            object: line.id,
            view,
            kind: ObjectKind::CentreLine,
            group: None,
            why: Some(why.clone()),
        }))
        .collect()
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
