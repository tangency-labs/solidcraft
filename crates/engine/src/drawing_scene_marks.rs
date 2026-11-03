//! Manual centre marks and lines on a sheet. References are resolved from the current model; a
//! lost reference has no strokes and remains inspectable through its command.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::view::ResolvedView;
use solidcraft_geom::Vec2;
use solidcraft_geom::drawing_centre_mark;
use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind, Scene};

use super::CMD;
use crate::drawing_view;
use crate::params::bad;
use crate::{Result, Session};

// Paper millimetres, independent of view scale. A future drawing style owns these choices.
const WIDTH_MM: f64 = 0.18;

pub(super) fn groups(s: &Session, drawing: &Drawing, views: &[(u64, ResolvedView, Vec2)]) -> Result<Vec<Group>> {
    let view_ids: BTreeSet<_> = views.iter().map(|(id, _, _)| *id).collect();
    let marks: Vec<_> = drawing.centre_marks.iter().filter(|mark| view_ids.contains(&mark.view)).collect();
    let lines: Vec<_> = drawing.centre_lines.iter().filter(|line| view_ids.contains(&line.view)).collect();
    if marks.is_empty() && lines.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: Vec<_> = marks.iter().map(|mark| mark.id).collect();
    let resolved = s.doc.resolve_centre_marks(&s.model, drawing, &wanted).map_err(|e| bad(CMD, e.to_string()))?;
    let wanted_lines: Vec<_> = lines.iter().map(|line| line.id).collect();
    let resolved_lines = s.doc.resolve_centre_lines(&s.model, drawing, &wanted_lines).map_err(|e| bad(CMD, e.to_string()))?;
    let shown = match drawing_view::shown(s) {
        Ok(shown) => shown,
        Err(_) => return Ok(Vec::new()),
    };
    let mut frames = BTreeMap::new();
    for (id, view, centre) in views {
        if let Ok(extents) = drawing_view::measure(&shown, &view.basis) {
            let frame = drawing_view::frame(view.basis, view.scale.value, &extents.model, *centre, CMD)?;
            frames.insert(*id, frame);
        }
    }
    let mut groups = Vec::new();
    for (mark, found) in marks.into_iter().zip(resolved) {
        let (Some(frame), Some(centre), Some(radius)) = (frames.get(&mark.view), found.centre, found.radius_mm) else { continue };
        groups.push(group(mark.id, frame.to_paper(centre), radius * frame.scale().factor(), mark.overshoot_mm)?);
    }
    for (line, found) in lines.into_iter().zip(resolved_lines) {
        let (Some(frame), Some(points)) = (frames.get(&line.view), found.points) else { continue };
        let segment = drawing_centre_mark::centre_line(frame.to_paper(points[0]), frame.to_paper(points[1]), line.overshoot_mm)
            .map_err(|e| bad(CMD, e.to_string()))?;
        groups.push(Group { object: line.id, kind: ObjectKind::CentreLine, items: vec![stroke(segment)] });
    }
    Ok(groups)
}

/// The common paper construction for manual marks and later automatic marks.
pub(super) fn group(id: u64, centre: Vec2, radius: f64, overshoot: f64) -> Result<Group> {
    let lines = drawing_centre_mark::strokes(centre, radius, overshoot).map_err(|e| bad(CMD, e.to_string()))?;
    let items = lines.into_iter().map(stroke).collect();
    Ok(Group { object: id, kind: ObjectKind::CentreMark, items })
}

fn stroke(line: solidcraft_geom::Seg2) -> Item {
    let long = 3.6;
    let lengths_mm = vec![long, 0.54, 0.18, 0.54];
    let period = lengths_mm.iter().sum();
    let phase_mm = (long / 2.0 - line.length() / 2.0).rem_euclid(period);
    Item::Stroke { path: vec![line], closed: false, width_mm: WIDTH_MM, pattern: Some(Dashes { lengths_mm, phase_mm }) }
}

pub(super) fn append(scene: &mut Scene, marks: Vec<Group>) -> Result<()> {
    scene.groups.extend(marks);
    scene.validate().map_err(|e| bad(CMD, e.to_string()))
}
