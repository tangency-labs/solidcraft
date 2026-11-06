//! Arrange (`solid.arrange`, #12430): lay bodies out side by side on a rectangle of the XY plane,
//! as for a build plate or a sheet to cut from. Each body keeps its height and is only turned
//! about Z, unless it is laid down on a flat side first: the side picked for it, or with
//! `lay_flat` the one it rests on best ([`solidcraft_geom::rest`]). Its footprint is the outline
//! it covers seen from above, worked out from its exact edges and silhouettes
//! ([`solidcraft_geom::footprint`]); the footprints are packed by [`solidcraft_geom::arrange`],
//! and each body that fits is moved there by a Move feature, all in one undo step. Bodies that
//! don't fit stay where they are and are listed.

use serde_json::{Value, json};
use solidcore_record::brep::Brep;
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::arrange::{
    ArrangeItem, ArrangeMethod, ArrangeOptions, MAX_COPIES, MAX_IMPROVE, MAX_OBSTACLES, MAX_ROTATIONS, Placement2, Shape2, arrange_around,
};
use solidcraft_geom::footprint::footprint;
use solidcraft_geom::prelude::*;
use solidcraft_geom::rest::{FlatSide, flat_sides, level_face_at, side_at};

use solidcraft_doc::expr::Kind;

use super::CommandSpec;

use crate::params::{bad, expr, num, str_, string_list};
use crate::schema::arrange::{ArrangeParams, ArrangeReply, ArrangedBody, MAX_SIZE};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<ArrangeParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.arrange", "Arrange", crate::checked!("solid.arrange", arrange_bodies))
    .at("SOLID", "MODIFY")
    .icon("move")
    .enabled(super::not_in_sketch)
    .contract(&CONTRACT)
    .output(output_schema_for::<ArrangeReply>)
    .params(
        "bodies: [names] (solid bodies of the design, not of placed components); width, length: expr (the rectangle along X and Y); \
         origin?: [x, y] (its lower-left corner, default [0, 0]); sheet?: face (in place of the rectangle: a level face looking up, of a body \
         not arranged; its outline and holes, at its height); spacing?: mm between bodies (default 1); margin?: mm from the sheet's edge \
         (default 0); angle_step?: degrees a body may turn by about Z (default 90; 0 keeps each body's turn); method?: \"true_shape\" (default: \
         bodies tuck into each other's corners) | \"rectangular\" (each body's box, in rows); improve?: other orders to try (default 0, at most \
         10000); quantity?: how many of each body (default 1: the body itself; more are copies); sheets?: how many rectangles side by side \
         along +X (default 1); sheet_spacing?: mm between them (default 10); copy?: bool (default false: the bodies are moved; true leaves them \
         where they are, clear of the layout, and lays out copies); lay_flat?: bool (default false; true lays each body on the flat side it \
         rests on best, nothing hanging below it); bottoms?: [faces] (a flat face per body to lay it on) → placed: [{body, copy_of?, sheet, x, y, \
         angle_deg, bottom?}] (how far each body moved and turned, about the world Z axis through the origin, then moved; bottom: the side it \
         was laid on); unplaced: [names] (did not fit; not moved); copies_left (copies that did not fit); sheets_used; fill (the footprints \
         over the area of the sheets used); footprint_error_mm. Bodies stand on the sheet (z = 0 for the rectangle). Other bodies on it stay \
         and are kept clear of; so is a body that does not fit, which stays where it is; nothing fitting is an error. Footprints come \
         from the exact edges and silhouettes seen from above; a hole through a body is not used",
    )];

/// Largest spacing or margin, mm.
const MAX_GAP: f64 = 1e5;
/// Most bodies one call arranges.
const MAX_BODIES: usize = 2_000;
/// Most copies of each body.
const MAX_QUANTITY: usize = 1_000;
/// Most sheets.
const MAX_SHEETS: usize = 100;

fn arrange_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.arrange";
    let mut names = string_list(cmd, p, "bodies")?;
    let mut seen = std::collections::BTreeSet::new();
    names.retain(|n| seen.insert(n.clone()));
    if names.is_empty() {
        return Err(bad(cmd, "`bodies` must list the bodies to arrange"));
    }
    if names.len() > MAX_BODIES {
        return Err(bad(cmd, format!("at most {MAX_BODIES} bodies")));
    }
    // A number (mm or degrees) or an expression with units ("200 mm", "w / 2", "45 deg").
    let value = |k: &str, kind: Kind| -> Result<Option<f64>> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => {
                let e = expr(p, k).ok_or_else(|| bad(cmd, format!("`{k}` must be a number or an expression")))?;
                s.doc.eval(&e, kind).map(Some).map_err(|e| bad(cmd, format!("`{k}`: {e}")))
            }
        }
    };
    let size = |k: &str| -> Result<f64> {
        value(k, Kind::Length)?
            .filter(|v| *v > 0.0 && *v <= MAX_SIZE)
            .ok_or_else(|| bad(cmd, format!("`{k}` must be a length above 0 and at most {MAX_SIZE} mm")))
    };
    let on_face = !matches!(p.get("sheet"), None | Some(Value::Null));
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    let (width, length) = if on_face {
        if given("width") || given("length") || given("origin") {
            return Err(bad(cmd, "give `width`, `length` and `origin`, or a `sheet` face, not both"));
        }
        (0.0, 0.0)
    } else {
        (size("width")?, size("length")?)
    };
    let gap = |k: &str, default: f64| -> Result<f64> {
        match value(k, Kind::Length)? {
            None => Ok(default),
            Some(v) if (0.0..=MAX_GAP).contains(&v) => Ok(v),
            Some(_) => Err(bad(cmd, format!("`{k}` must be a length from 0 to {MAX_GAP} mm"))),
        }
    };
    let (spacing, margin, sheet_spacing) = (gap("spacing", 1.0)?, gap("margin", 0.0)?, gap("sheet_spacing", 10.0)?);
    let whole = |k: &str, max: usize| -> Result<usize> {
        match value(k, Kind::Unitless)? {
            None => Ok(1),
            Some(v) => Some(v)
                .filter(|v| (1.0..=max as f64).contains(v) && v.fract() == 0.0)
                .map(|v| v as usize)
                .ok_or_else(|| bad(cmd, format!("`{k}` must be a whole number from 1 to {max}"))),
        }
    };
    let (quantity, sheets) = (whole("quantity", MAX_QUANTITY)?, whole("sheets", MAX_SHEETS)?);
    if on_face && sheets > 1 {
        return Err(bad(cmd, "a `sheet` face is one sheet: `sheets` above 1 needs the rectangle"));
    }
    if names.len() * quantity > MAX_COPIES {
        return Err(bad(cmd, format!("at most {MAX_COPIES} bodies and copies in all")));
    }
    let origin = match p.get("origin") {
        None | Some(Value::Null) => Vec2::ZERO,
        Some(v) => crate::params::vec2(v)
            .filter(|o| o.x.abs() <= MAX_SIZE && o.y.abs() <= MAX_SIZE)
            .ok_or_else(|| bad(cmd, format!("`origin` must be [x, y] in mm, each at most {MAX_SIZE} in size")))?,
    };
    // Angles evaluate to radians; the step is in degrees from here on.
    let step = match value("angle_step", Kind::Angle)? {
        None => 90.0,
        Some(v) if (0.0..=std::f64::consts::TAU + 1e-12).contains(&v) => v.to_degrees(),
        Some(_) => return Err(bad(cmd, "`angle_step` must be an angle from 0 to 360 degrees")),
    };
    let turns: Vec<f64> = if step < 360.0 / MAX_ROTATIONS as f64 {
        if step == 0.0 {
            Vec::new()
        } else {
            return Err(bad(cmd, format!("`angle_step` must be 0, or at least {} degrees", 360.0 / MAX_ROTATIONS as f64)));
        }
    } else {
        (0..MAX_ROTATIONS).map(|k| k as f64 * step).take_while(|a| *a < 360.0 - 1e-9).collect()
    };
    let method = match str_(p, "method") {
        None => ArrangeMethod::TrueShape,
        Some("true_shape") => ArrangeMethod::TrueShape,
        Some("rectangular") => ArrangeMethod::Rectangular,
        Some(other) => return Err(bad(cmd, format!("unknown `method` `{other}` (true_shape or rectangular)"))),
    };
    let improve = match p.get("improve") {
        None | Some(Value::Null) => 0,
        Some(_) => num(p, "improve")
            .filter(|v| (0.0..=MAX_IMPROVE as f64).contains(v) && v.fract() == 0.0)
            .ok_or_else(|| bad(cmd, format!("`improve` must be a whole number from 0 to {MAX_IMPROVE}")))? as usize,
    };

    let flag = |k: &str| -> Result<bool> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(bad(cmd, format!("`{k}` must be true or false"))),
        }
    };
    let (lay_flat, keep) = (flag("lay_flat")?, flag("copy")?);
    // The sheet: the rectangle, or a level face of some other body.
    let (base, sheet_z, sheet_error) = match p.get("sheet") {
        None | Some(Value::Null) => {
            let outer =
                [(0.0, 0.0), (width, 0.0), (width, length), (0.0, length)].iter().map(|&(x, y)| Vec2::new(origin.x + x, origin.y + y)).collect();
            (Shape2 { outer, holes: Vec::new() }, 0.0, 0.0)
        }
        Some(v) => {
            let at = super::face::face_ref(s, v, None, cmd)?;
            let world = s.world_state()?;
            let mut found = None;
            for b in world.bodies.iter().filter(|b| !b.body.is_mesh() && !names.contains(&b.name)) {
                let Ok(record) = solidcraft_kernel::brep_record(&b.body) else { continue };
                if let Some(level) = level_face_at(&record, at, 1e-4) {
                    found = Some(level);
                    break;
                }
            }
            let level = found.ok_or_else(|| bad(cmd, "`sheet` must be a level flat face looking up, of a body that is not being arranged"))?;
            (level.shape, level.z, level.error)
        }
    };
    let picks: Vec<Vec3> = match p.get("bottoms") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.len() <= MAX_BODIES => a.iter().map(|v| super::face::face_ref(s, v, None, cmd)).collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, format!("`bottoms` must list at most {MAX_BODIES} faces"))),
    };

    // Long work stops when whoever runs it (a dialog's OK or preview) gives up on it.
    let watch = solidcraft_kernel::watch::current();
    let stop = move || watch.as_ref().is_some_and(|w| w.cancelled());
    // Each body's footprint once it is laid down: as it stands, or on a flat side.
    let world = s.world_state()?;
    // A body of a placed component moves in its component's frame, which the moves below do not
    // map into.
    let frames = s.body_frames();
    let own =
        |m: &[[f64; 4]; 4]| m.iter().enumerate().all(|(j, c)| c.iter().enumerate().all(|(i, v)| (v - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-12));
    for n in &names {
        if frames.iter().any(|(w, local, m)| w == n && (w != local || !own(m))) {
            return Err(bad(
                cmd,
                format!("`{n}` is in a placed component: only the design's own bodies can be arranged yet; move the component with Move instead"),
            ));
        }
    }
    let mut items = Vec::with_capacity(names.len());
    let mut lows = Vec::with_capacity(names.len());
    let mut records = Vec::with_capacity(names.len());
    let mut boxes = Vec::with_capacity(names.len());
    // What stands where parts may go: (outline from above, lowest z, highest z).
    let mut obstacles: Vec<(Shape2, f64, f64)> = Vec::new();
    let mut spans = Vec::with_capacity(names.len());
    let mut laid_down: Vec<Option<Down>> = Vec::with_capacity(names.len());
    let mut picked = vec![false; picks.len()];
    let mut error: f64 = 0.0;
    let outline = |record: &Brep, rotation: [Vec3; 3], n: &str| {
        footprint(record, rotation, &stop)
            .map_err(|e| if stop() { bad(cmd, "cancelled") } else { bad(cmd, format!("`{n}`: no outline from above: {e}")) })
    };
    for n in &names {
        let body = world.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))?;
        if body.body.is_mesh() {
            return Err(bad(cmd, format!("`{n}` is a mesh body: only solid bodies can be arranged yet")));
        }
        let record = solidcraft_kernel::brep_record(&body.body).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
        let bounds = body.body.exact_bounds().map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
        let tol = 1e-6 * (bounds.max - bounds.min).len().max(1.0);
        let mut chosen = None;
        for (k, at) in picks.iter().enumerate() {
            let Some(side) = side_at(&record, *at, tol) else { continue };
            if chosen.is_some() {
                return Err(bad(cmd, format!("two bottom faces are on `{n}`: pick one face for a body to lie on")));
            }
            match picked.get_mut(k) {
                Some(true) => return Err(bad(cmd, format!("bottom face {} is on more than one body", k + 1))),
                Some(m) => *m = true,
                None => {}
            }
            let (down, _, rests) = down_on(&body.body, &side).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
            if !rests {
                return Err(bad(cmd, format!("`{n}` can't lie on the picked face: some of it would hang below")));
            }
            chosen = Some(down);
        }
        if chosen.is_none() && lay_flat {
            chosen = best_side(&body.body, &flat_sides(&record)).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
        }
        if keep {
            // The body stays: where it stands is not free.
            obstacles.push((outline(&record, IDENTITY, n)?.shape, bounds.min.z, bounds.max.z));
        }
        lows.push(bounds.min.z);
        // Where it and its copies will be, in height: standing on the sheet.
        spans.push((sheet_z, sheet_z + (bounds.max - bounds.min).len()));
        let rotation = chosen.map_or(IDENTITY, |d| d.rotation);
        let fp = outline(&record, rotation, n)?;
        error = error.max(fp.error);
        items.push(ArrangeItem { shape: fp.shape, quantity, rotations_deg: turns.clone() });
        laid_down.push(chosen);
        records.push(record);
        boxes.push(bounds);
    }
    if let Some(k) = picked.iter().position(|m| !*m) {
        return Err(bad(cmd, format!("bottom face {} is not on a flat face of a body being arranged", k + 1)));
    }
    // Sheet after sheet along X, each taking what the ones before left.
    let xs: Vec<f64> = base.outer.iter().map(|v| v.x).collect();
    let ys: Vec<f64> = base.outer.iter().map(|v| v.y).collect();
    let low = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let high = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let step = high(&xs) - low(&xs) + sheet_spacing;
    // Where the sheets are, grown by what keeps parts apart: a body outside it is in no one's way.
    let grow = spacing + margin + 1e-6;
    let reach = (low(&xs) - grow, low(&ys) - grow, high(&xs) + step * (sheets - 1) as f64 + grow, high(&ys) + grow);
    let over = |lo: Vec3, hi: Vec3| hi.x >= reach.0 && lo.x <= reach.2 && hi.y >= reach.1 && lo.y <= reach.3;
    // Bodies are in the way only where the parts will be, in height; one just below the sheet
    // (the table under it) is not.
    let (zlo, zhi) = spans.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| (lo.min(*a), hi.max(*b)));
    let ztol = 1e-6 * (zhi - zlo).abs().max(1.0);
    let level = |lo: f64, hi: f64| hi > zlo + ztol && lo < zhi - ztol;
    // The other bodies on the sheets stand where they are.
    for b in world.bodies.iter().filter(|b| !names.contains(&b.name)) {
        let Ok(bb) = b.body.exact_bounds() else { continue };
        if !over(bb.min, bb.max) || !level(bb.min.z, bb.max.z) {
            continue;
        }
        let rect = || Shape2 {
            outer: vec![Vec2::new(bb.min.x, bb.min.y), Vec2::new(bb.max.x, bb.min.y), Vec2::new(bb.max.x, bb.max.y), Vec2::new(bb.min.x, bb.max.y)],
            holes: Vec::new(),
        };
        // A mesh, or a body with no outline, stands in for its box.
        let shape = match (b.body.is_mesh(), solidcraft_kernel::brep_record(&b.body)) {
            (false, Ok(r)) => footprint(&r, IDENTITY, &stop).map(|f| f.shape).unwrap_or_else(|_| rect()),
            _ => rect(),
        };
        if stop() {
            return Err(bad(cmd, "cancelled"));
        }
        obstacles.push((shape, bb.min.z, bb.max.z));
    }
    if obstacles.len() > MAX_OBSTACLES {
        return Err(bad(cmd, format!("more than {MAX_OBSTACLES} other bodies stand on the sheet")));
    }
    // Widened by the footprints' error, so the true outlines keep the spacing and the margin.
    let opts = ArrangeOptions { method, spacing: spacing + 2.0 * error, margin: margin + error + sheet_error, part_in_part: false, improve, seed: 1 };
    let sheet_area = shape_area(&base);
    // Lays out what is not standing, around the obstacles: per body, where each of its copies
    // goes (sheet, placement); how many are left; the sheets used and the area they cover.
    let pack = |standing: &[bool], blocking: &[Shape2]| -> Result<(Vec<Vec<(usize, Placement2)>>, Vec<usize>, usize, f64)> {
        let mut left: Vec<usize> = standing.iter().map(|st| if *st { 0 } else { quantity }).collect();
        let mut spots: Vec<Vec<(usize, Placement2)>> = vec![Vec::new(); items.len()];
        let (mut used, mut covered) = (0usize, 0.0);
        for k in 0..sheets {
            let wanted: Vec<usize> = (0..items.len()).filter(|i| left.get(*i).is_some_and(|n| *n > 0)).collect();
            if wanted.is_empty() {
                break;
            }
            let dx = step * k as f64;
            let moved = |r: &[Vec2]| r.iter().map(|v| Vec2::new(v.x + dx, v.y)).collect::<Vec<_>>();
            let sheet = Shape2 { outer: moved(&base.outer), holes: base.holes.iter().map(|h| moved(h)).collect() };
            let these: Vec<ArrangeItem> =
                wanted.iter().filter_map(|i| Some(ArrangeItem { quantity: *left.get(*i)?, ..items.get(*i)?.clone() })).collect();
            let laid = arrange_around(&these, &sheet, blocking, &opts, &stop)
                .map_err(|e| if stop() { bad(cmd, "cancelled") } else { bad(cmd, e.to_string()) })?;
            // With nothing in the way the next sheet is the same: if nothing fits here, nothing
            // will there.
            if laid.placed.is_empty() && blocking.is_empty() {
                break;
            }
            if !laid.placed.is_empty() {
                used = k + 1;
            }
            covered += laid.fill * sheet_area;
            for pl in laid.placed {
                let Some(&i) = wanted.get(pl.item) else { continue };
                if let (Some(n), Some(list)) = (left.get_mut(i), spots.get_mut(i)) {
                    *n = n.saturating_sub(1);
                    list.push((k, Placement2 { item: i, ..pl }));
                }
            }
        }
        Ok((spots, left, used, covered))
    };
    // A body that finds no place stays where it is: when that is on a sheet the others keep
    // clear of it, so lay out again until no new one is left standing in their way.
    let mut standing = vec![false; items.len()];
    let mut blocking: Vec<Shape2> = obstacles.iter().filter(|(_, lo, hi)| level(*lo, *hi)).map(|(o, _, _)| o.clone()).collect();
    let (spots, mut left, used, covered) = loop {
        let packed = pack(&standing, &blocking)?;
        let mut more = false;
        if !keep {
            for (i, list) in packed.0.iter().enumerate() {
                let (Some(bb), Some(record), Some(st)) = (boxes.get(i), records.get(i), standing.get_mut(i)) else { continue };
                if list.is_empty() && !*st && over(bb.min, bb.max) && level(bb.min.z, bb.max.z) && blocking.len() < MAX_OBSTACLES {
                    *st = true;
                    more = true;
                    blocking.push(outline(record, IDENTITY, names.get(i).map_or("", String::as_str))?.shape);
                }
            }
        }
        if !more {
            break packed;
        }
    };
    if spots.iter().all(Vec::is_empty) {
        let (w, l) = (high(&xs) - low(&xs), high(&ys) - low(&ys));
        return Err(bad(
            cmd,
            format!(
                "nothing fits on the sheet ({w:.1} × {l:.1} mm, less the margin, spacing and what stands on it): make it larger or the spacing smaller"
            ),
        ));
    }
    // One left standing is left over as it is.
    for (n, st) in left.iter_mut().zip(&standing) {
        if *st {
            *n = quantity;
        }
    }

    let mover = super::find_command("solid.move").ok_or_else(|| EngineError::UnknownCommand("solid.move".into()))?;
    let mut placed = Vec::new();
    for (i, list) in spots.iter().enumerate() {
        let Some(name) = names.get(i) else { continue };
        let down = laid_down.get(i).copied().flatten();
        // Copies first, from the body where it stands; the body itself takes the first spot.
        for (n, (sheet, pl)) in list.iter().enumerate().skip(1).chain(list.iter().enumerate().take(1)) {
            let copy = n > 0 || keep;
            // Laying down, then the layout's turn about Z: one turn about the origin.
            let (c, sn) = (pl.angle.cos(), pl.angle.sin());
            let spin = |v: Vec3| Vec3::new(c * v.x - sn * v.y, sn * v.x + c * v.y, v.z);
            let r = down.map_or(IDENTITY, |d| d.rotation);
            let (axis, turn) = axis_angle([spin(r[0]), spin(r[1]), spin(r[2])]);
            let dz = match down {
                Some(d) => d.lift + sheet_z,
                None => sheet_z - lows.get(i).copied().unwrap_or(sheet_z),
            };
            let label = if copy { format!("Arrange {name} copy {}", n + usize::from(keep)) } else { format!("Arrange {name}") };
            let mut q = json!({"bodies": [name], "translate": [pl.shift.x, pl.shift.y, dz], "name": label});
            if turn.abs() > 1e-12 {
                q["axis"] = json!([axis.x, axis.y, axis.z]);
                q["angle"] = json!(format!("{} deg", turn.to_degrees()));
            }
            if copy {
                q["copy"] = json!(true);
            }
            let mut body = name.clone();
            if copy || pl.shift.x != 0.0 || pl.shift.y != 0.0 || dz != 0.0 || turn.abs() > 1e-12 {
                let reply = mover.run_nested(s, &q)?;
                if copy {
                    body = reply["created_bodies"]
                        .get(0)
                        .and_then(Value::as_str)
                        .map(String::from)
                        .ok_or_else(|| EngineError::Other("the copy made no body".into()))?;
                }
            }
            let bottom = down.map(|d| [d.normal.x, d.normal.y, d.normal.z]);
            let copy_of = copy.then(|| name.clone());
            placed.push(ArrangedBody { body, copy_of, sheet: *sheet, x: pl.shift.x, y: pl.shift.y, angle_deg: pl.angle.to_degrees(), bottom });
        }
    }
    let unplaced = names.iter().zip(&spots).filter(|(_, l)| l.is_empty()).map(|(n, _)| n.clone()).collect();
    // A body that did not fit stays as one of its copies.
    let copies_left = left.iter().zip(&spots).map(|(n, l)| if l.is_empty() && !keep { n.saturating_sub(1) } else { *n }).sum();
    let fill = if used == 0 || sheet_area <= 0.0 { 0.0 } else { covered / (used as f64 * sheet_area) };
    let reply = ArrangeReply { placed, unplaced, copies_left, sheets_used: used, fill, footprint_error_mm: error, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The area of a region, mm².
fn shape_area(s: &Shape2) -> f64 {
    let ring = |r: &[Vec2]| (0..r.len()).map(|k| r[k].x * r[(k + 1) % r.len()].y - r[(k + 1) % r.len()].x * r[k].y).sum::<f64>().abs() / 2.0;
    ring(&s.outer) - s.holes.iter().map(|h| ring(h)).sum::<f64>()
}

const IDENTITY: [Vec3; 3] = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];

/// How a body is laid on one of its flat sides.
#[derive(Clone, Copy, Debug)]
struct Down {
    /// The turn about the origin that points the side down (its columns: where X, Y and Z go).
    rotation: [Vec3; 3],
    /// How far the turned body then rises to stand on z = 0.
    lift: f64,
    /// The side's outward normal before the turn.
    normal: Vec3,
}

/// The shortest turn that points `n` (unit) along -Z; half a turn about X when `n` points up.
fn pointing_down(n: Vec3) -> [Vec3; 3] {
    let down = Vec3::new(0.0, 0.0, -1.0);
    let cos = n.dot(down).clamp(-1.0, 1.0);
    if cos > 1.0 - 1e-15 {
        return IDENTITY;
    }
    let axis = n.cross(down).normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let angle = cos.acos();
    // Rodrigues' formula.
    let turn = |v: Vec3| v * angle.cos() + axis.cross(v) * angle.sin() + axis * (axis.dot(v) * (1.0 - angle.cos()));
    IDENTITY.map(turn)
}

fn matrix(r: [Vec3; 3]) -> [[f64; 4]; 4] {
    [[r[0].x, r[0].y, r[0].z, 0.0], [r[1].x, r[1].y, r[1].z, 0.0], [r[2].x, r[2].y, r[2].z, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// `body` laid on `side`, its height then, and whether all of it stays above the side's plane:
/// nothing hangs below it, on the exact body.
fn down_on(body: &solidcraft_kernel::Body, side: &FlatSide) -> std::result::Result<(Down, f64, bool), String> {
    let mut rotation = pointing_down(side.normal);
    // Squared up: the side's longest straight edge along X, so quarter turns line it up with the
    // sheet's edges.
    if let Some(d) = side.edge {
        let e = rotation[0] * d.x + rotation[1] * d.y + rotation[2] * d.z;
        if e.x.hypot(e.y) > 1e-9 {
            let a = -e.y.atan2(e.x);
            let (c, sn) = (a.cos(), a.sin());
            rotation = rotation.map(|v| Vec3::new(c * v.x - sn * v.y, sn * v.x + c * v.y, v.z));
        }
    }
    let turned = solidcraft_kernel::transform_matrix(body, matrix(rotation)).map_err(|e| e.to_string())?;
    let b = turned.exact_bounds().map_err(|e| e.to_string())?;
    // The side's plane is at z = -offset once turned.
    let tol = 1e-6 * (b.max - b.min).len().max(1.0);
    let rests = b.min.z >= -side.offset - tol;
    Ok((Down { rotation, lift: -b.min.z, normal: side.normal }, b.max.z - b.min.z, rests))
}

/// The flat side `body` rests on with the most of it touching, among those nothing hangs below;
/// of sides as large, the one that leaves it lowest. `None` when it rests on none.
fn best_side(body: &solidcraft_kernel::Body, sides: &[FlatSide]) -> std::result::Result<Option<Down>, String> {
    let mut best: Option<(Down, f64, f64)> = None;
    for side in sides {
        if best.as_ref().is_some_and(|(_, area, _)| side.area < area * (1.0 - 1e-9)) {
            break;
        }
        let (down, height, rests) = down_on(body, side)?;
        if rests && best.as_ref().is_none_or(|(_, _, h)| height < h - 1e-9) {
            let area = best.as_ref().map_or(side.area, |b| b.1);
            best = Some((down, area, height));
        }
    }
    Ok(best.map(|(d, _, _)| d))
}

/// The axis and angle (radians, 0 to π) of the turn whose columns are `r`.
fn axis_angle(r: [Vec3; 3]) -> (Vec3, f64) {
    let trace = r[0].x + r[1].y + r[2].z;
    // Entry (i, j) is r[j][i]; the skew part is 2 sin(angle) times the axis.
    let skew = Vec3::new(r[1].z - r[2].y, r[2].x - r[0].z, r[0].y - r[1].x);
    let (sin, cos) = (skew.len() / 2.0, (trace - 1.0) / 2.0);
    let angle = sin.atan2(cos);
    if angle < 1e-12 {
        return (Vec3::new(0.0, 0.0, 1.0), 0.0);
    }
    if cos > -0.5
        && let Some(axis) = skew.normalized()
    {
        return (axis, angle);
    }
    // Far round: the symmetric part less cos times I is (1 - cos) times the axis times itself;
    // its largest column, signed by the skew part where it says anything.
    let sym = |j: usize| {
        let e = |i: usize, j: usize| {
            let [a, b] = [r[j], r[i]];
            let (aij, aji) = ([a.x, a.y, a.z][i], [b.x, b.y, b.z][j]);
            (aij + aji) / 2.0 - if i == j { cos } else { 0.0 }
        };
        Vec3::new(e(0, j), e(1, j), e(2, j))
    };
    let k = (0..3).max_by(|a, b| sym(*a).len().total_cmp(&sym(*b).len())).unwrap_or(0);
    let mut axis = sym(k).normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    if axis.dot(skew) < 0.0 {
        axis = axis * -1.0;
    }
    (axis, angle)
}

#[cfg(test)]
#[path = "arrange_tests.rs"]
mod tests;
