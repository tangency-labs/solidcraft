//! Section curves: the cut of bodies by a plane as 2D lines, arcs and splines in the plane's
//! frame, returned as JSON. It changes nothing in the design.

use serde_json::{Value, json};
use solidcraft_doc::project::{SectionLoop, SectionSeg, section_loops};
use solidcraft_geom::Vec2;

use crate::params::bad;
use crate::{Result, Session};

pub(super) const CMD: &str = "inspect.section_curves";
pub(super) const PARAMS: &str = "plane: XY|XZ|YZ | construction plane name or id | {origin, normal}; offset?, at?, flip?: as inspect.section; \
     bodies?: name | [name] (1–10000 names; default every visible body); tolerance?: 0.0001..1 mm (mesh chord tolerance, default 0.01); \
     Returns {frame: {origin, x, y, normal}, loops: [{body, closed, area (mm², closed loops), segments}]}: points [x, y] in the frame, \
     segments {type: line, start, end} | {type: arc, center, radius, start, end, sweep_deg (counter-clockwise +)} | \
     {type: spline, closed, points (fit points), beziers ([[p0, p1, p2, p3]], the same curve as cubic pieces)}. \
     Lines and arcs are exact; other curves are fit splines through points on the surface. Outer loops run counter-clockwise, holes clockwise";

const MAX_BODIES: usize = 10_000;

fn pt(p: Vec2) -> Value {
    json!([p.x, p.y])
}

/// The bodies the request names (none: every visible body). Read whole rather than with
/// `string_list`, which keeps the first 10,000 entries and drops any that is not text: a list past
/// the limit, or with an entry that is not a name, is refused instead of cut short.
fn body_names(p: &Value) -> Result<Vec<String>> {
    match p.get("bodies") {
        None => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(a)) if a.len() > MAX_BODIES => Err(bad(CMD, format!("`bodies` lists at most {MAX_BODIES} names"))),
        Some(Value::Array(a)) if !a.is_empty() => {
            a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad(CMD, "`bodies` must list body names (text)"))).collect()
        }
        Some(_) => Err(bad(CMD, "`bodies` must list body names")),
    }
}

pub(super) fn section_curves(s: &mut Session, p: &Value) -> Result<Value> {
    let (plane, _) = super::cut_plane(s, p, CMD)?;
    let tolerance = match p.get("tolerance") {
        None => 0.01,
        Some(v) => v.as_f64().filter(|t| (0.0001..=1.0).contains(t)).ok_or_else(|| bad(CMD, "tolerance must be 0.0001..1 mm"))?,
    };
    let world = s.world_state();
    let names = body_names(p)?;
    if let Some(n) = names.iter().find(|n| world.bodies.iter().all(|b| &b.name != *n)) {
        return Err(bad(CMD, format!("no body `{n}`")));
    }
    let mut cut: Vec<(String, Vec<SectionLoop>)> = Vec::new();
    for b in world.bodies.iter().filter(|b| if names.is_empty() { s.body_visible(&b.name) } else { names.contains(&b.name) }) {
        // Bodies the plane misses need no finer mesh.
        let bounds = b.mesh().bounds();
        let heights = [bounds.min.x, bounds.max.x]
            .iter()
            .flat_map(|x| [bounds.min.y, bounds.max.y].map(|y| (*x, y)))
            .flat_map(|(x, y)| [bounds.min.z, bounds.max.z].map(|z| plane.height(solidcraft_geom::Vec3::new(x, y, z))))
            .collect::<Vec<_>>();
        // A plane on a face (within rounding) still cuts the body there.
        let on = bounds.diagonal() * 1e-9;
        if bounds.is_empty() || heights.iter().all(|h| *h > on) || heights.iter().all(|h| *h < -on) {
            continue;
        }
        let mesh = b.body.reference_display_mesh_angular(tolerance, solidcraft_kernel::DISPLAY_ANGLE)?;
        let loops = section_loops(&plane, &mesh)?;
        if !loops.is_empty() {
            cut.push((b.name.clone(), loops));
        }
    }
    let loops: Vec<Value> = cut
        .iter()
        .flat_map(|(body, loops)| {
            loops.iter().map(move |l| {
                let mut v = json!({"body": body, "closed": l.closed, "segments": l.segments.iter().map(seg_json).collect::<Vec<_>>()});
                if let Some(a) = l.area {
                    v["area"] = json!(a);
                }
                v
            })
        })
        .collect();
    Ok(json!({
        "frame": {"origin": plane.origin, "x": plane.x, "y": plane.y, "normal": plane.normal()},
        "units": "mm",
        "loops": loops,
    }))
}

fn seg_json(s: &SectionSeg) -> Value {
    match s {
        SectionSeg::Line { a, b } => json!({"type": "line", "start": pt(*a), "end": pt(*b)}),
        SectionSeg::Arc { c, r, sweep, .. } => {
            json!({"type": "arc", "center": pt(*c), "radius": r, "start": pt(s.start()), "end": pt(s.end()), "sweep_deg": sweep.to_degrees()})
        }
        SectionSeg::Spline { pts, periodic, .. } => json!({
            "type": "spline",
            "closed": periodic,
            "points": pts.iter().map(|p| pt(*p)).collect::<Vec<_>>(),
            "beziers": s.beziers().iter().map(|b| b.iter().map(|p| pt(*p)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        }),
    }
}

#[cfg(test)]
#[path = "section_curves_tests.rs"]
mod tests;
