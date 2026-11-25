//! Pure paper emission of resolved drawing-owned fills (W38d).
//! D6 clips hatch centre-lines and dot centres; explicit physical pen width
//! paints lines and filled circular dots. No zero becomes a degenerate stroke.

use solidcore_record::{Seg2, Vec2};
use solidcraft_doc::drawing::{Drawing, FillPaint, ResolvedSketchFill};
use solidcraft_doc::ids;
use solidcraft_geom::paper::{GridItem, Group, Item, MAX_RECORDS, ObjectKind, Piece, Scene};
use solidcraft_kernel::services::profile::Mark;

use crate::Result;

fn dot_item(at: Vec2, width: f64) -> std::result::Result<Item, String> {
    let region = solidcraft_kernel::services::profile::dot_region(at, width)
        .ok_or_else(|| "pen width cannot represent a finite positive filled disk at the dot centre".to_string())?;
    let item = Item::Fill { region };
    let scene = Scene::new(Vec2::new(1.0, 1.0), vec![Group { object: 1, kind: ObjectKind::Fill, items: vec![item.clone()] }]);
    let prepared = scene.prepare().map_err(|error| format!("dot disk: {error}"))?;
    // Every paper writer's grid, as io lists them. Refuse precision loss here
    // while the item is still known to be a hatch dot.
    let grids = solidcraft_io::paper::writer_grids().map_err(|error| format!("dot disk: {error}"))?;
    for grid in grids {
        let on = prepared.on_grid(grid).map_err(|error| format!("dot disk on {} steps/mm: {error}", grid.per_mm()))?;
        let survives = on.report.pieces_merged == 0
            && on.groups.first().is_some_and(|group| match group.items.as_slice() {
                [GridItem::Fill { loops }] => match loops.as_slice() {
                    [path] => {
                        path.closed
                            && path.pieces.len() == 4
                            && path.pieces.iter().all(|piece| matches!(piece, Piece::Arc { radius, .. } if *radius > 0))
                            && path.pieces.last().is_some_and(|piece| piece.to() == path.start)
                    }
                    _ => false,
                },
                _ => false,
            });
        if !survives {
            return Err(format!("dot disk cannot survive the {} steps/mm grid without merged or empty geometry", grid.per_mm()));
        }
    }
    Ok(item)
}
use crate::params::bad;

pub fn paper_group(drawing: &Drawing, resolved: &ResolvedSketchFill) -> Result<Group> {
    let error = |message: String| {
        bad(
            super::CMD,
            format!(
                "drawing {} sheet {} sketch {} fill {}: {message}",
                ids::encode(resolved.drawing),
                ids::encode(resolved.sheet),
                ids::encode(resolved.sketch),
                ids::encode(resolved.id)
            ),
        )
    };
    drawing.validate().map_err(|e| error(e.to_string()))?;
    if drawing.id != resolved.drawing
        || !drawing.sheet_fill(resolved.id).is_some_and(|fill| fill.sketch == resolved.sketch && fill.paint == resolved.paint)
        || !drawing.sheet_sketch(resolved.sketch).is_some_and(|sketch| drawing.sketch_sheet(sketch) == Some(resolved.sheet))
    {
        return Err(error("resolved fill owner or paint differs from its drawing record".into()));
    }
    let mut group = Group { object: resolved.id, kind: ObjectKind::Fill, items: Vec::new() };
    match &resolved.paint {
        FillPaint::Solid => group.items.push(Item::Fill { region: resolved.region.clone() }),
        FillPaint::Hatch { pattern, pen_width_mm } => {
            let pattern = drawing.hatch_pattern(*pattern).ok_or_else(|| error("pattern snapshot does not exist".into()))?;
            let marks = pattern.pattern.marks(&resolved.region).map_err(|e| error(e.to_string()))?;
            // Count the complete result before growing paper item storage. D6's
            // 40,000-mark cap does not by itself bound four-record filled dots.
            let records = marks.iter().try_fold(1usize, |count, mark| {
                let more = match mark {
                    Mark::Line { .. } => 2,
                    Mark::Dot { .. } => 4,
                };
                count
                    .checked_add(more)
                    .filter(|n| *n <= MAX_RECORDS)
                    .ok_or_else(|| error(format!("expanded hatch exceeds {MAX_RECORDS} paper records")))
            })?;
            if records > MAX_RECORDS {
                return Err(error(format!("expanded hatch exceeds {MAX_RECORDS} paper records")));
            }
            group.items.try_reserve_exact(marks.len()).map_err(|_| error("cannot allocate bounded hatch items".into()))?;
            for mark in marks {
                group.items.push(match mark {
                    Mark::Line { a, b } => Item::Stroke { path: vec![Seg2::Line { a, b }], closed: false, width_mm: *pen_width_mm, pattern: None },
                    Mark::Dot { at } => dot_item(at, *pen_width_mm).map_err(error)?,
                });
            }
        }
    }
    if group.records() > MAX_RECORDS {
        return Err(error(format!("fill exceeds {MAX_RECORDS} paper records")));
    }
    Scene::new(Vec2::new(1.0, 1.0), vec![group.clone()]).validate().map_err(|e| error(e.to_string()))?;
    Ok(group)
}
