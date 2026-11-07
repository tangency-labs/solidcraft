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
use solidcraft_geom::arrange::Hurry;
use solidcraft_geom::arrange::{
    ArrangeItem, ArrangeMethod, ArrangeOptions, COARSEN_BUDGET, MAX_COPIES, MAX_IMPROVE, MAX_OBSTACLES, MAX_ROTATIONS, Placement2, Shape2, apart,
    arrange_hurried, coarsen,
};
use solidcraft_geom::footprint::{Footprint, Shadow, footprint};
use solidcraft_geom::prelude::*;
use solidcraft_geom::rest::{FlatSide, flat_face_at, flat_sides, side_at};

use solidcraft_doc::expr::Kind;

use super::CommandSpec;

use crate::params::{bad, expr, num, str_, string_list};
use crate::schema::arrange::{ArrangeParams, ArrangeReply, ArrangedBody, MAX_SIZE, SheetOutline};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<ArrangeParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.arrange", "Arrange", crate::checked!("solid.arrange", arrange_bodies))
    .at("SOLID", "MODIFY")
    .icon("arrange")
    .enabled(super::not_in_sketch)
    .contract(&CONTRACT)
    .output(output_schema_for::<ArrangeReply>)
    .params(
        "bodies: [names] (solid bodies; a body in a component needs copy: true: it is copied into the design's own bodies first, with no link or joint); width, length: expr (the rectangle along X and Y); \
         plane?: plane (the rectangle lies on it, along its X and Y from its origin, the bodies on its normal side; default XY); origin?: [x, y] (its lower-left corner on the plane, default [0, 0]); sheet?: face (in place of the rectangle: a flat face at any tilt, of a body \
         not arranged; its outline and holes; the bodies lie on it, laid out in its plane); spacing?: mm between bodies (default 1); margin?: mm from the sheet's edge \
         (default 0); angle_step?: degrees a body may turn by about Z (default 90; 0 keeps each body's turn); method?: \"true_shape\" (default: \
         bodies tuck into each other's corners) | \"rectangular\" (each body's box, in rows); improve?: other orders to try (default 0, at most \
         10000); quantity?: how many of each body (default 1: the body itself; more are copies); sheets?: how many rectangles side by side \
         along +X (default 1); sheet_spacing?: mm between them (default 10); copy?: bool (default false: the bodies are moved; true leaves them \
         where they are, clear of the layout, and lays out copies; copies always go into a new browser group \"Arrangement n\"); lay_flat?: bool (default true: each body lies on the flat side it \
         rests on best, nothing hanging below it; false keeps it as it stands); avoid?: bool (default true: keep clear of the \
         other bodies shown on the sheet and the originals; false lays out as if the sheet were empty); bottoms?: [faces] (a flat face per body to lay it on) → placed: [{body, copy_of?, sheet, x, y, \
         angle_deg, bottom?}] (how far each body moved and turned, about the world Z axis through the origin, then moved; bottom: the side it \
         was laid on); boxed?: [names] (no outline from above: placed by their box); hurried?: true (the layout ran long, about 30 s, \
         6 s on the web, and hurried to finish: some parts may be boxed or unplaced); unplaced: [names] (did not fit; not moved); copies_left (copies that did not fit); sheets_used; fill (the footprints \
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

/// When a layout gives up on work, in seconds from the call's start: outlines not found by
/// `outlines` are boxes; after `quick` each part takes the first good spot; after `boxes` the rest
/// are packed by their boxes; after `stop` the rest are left unplaced. Laying out again round a
/// body that could not move takes at most `repair` more. Only a layout that runs this long can
/// come out differently from one call to the next (the reply's `hurried`).
struct Budget {
    outlines: f64,
    quick: f64,
    boxes: f64,
    stop: f64,
    repair: f64,
}

/// On the desktop a layout runs off the window thread and Cancel stops it at once, so the
/// budget is only a backstop for a job far too big.
#[cfg(not(target_arch = "wasm32"))]
const BUDGET: Budget = Budget { outlines: 25.0, quick: 25.0, boxes: 28.0, stop: 30.0, repair: 5.0 };
/// On the web it runs on the page's only thread, which nothing can interrupt: a few seconds.
#[cfg(target_arch = "wasm32")]
const BUDGET: Budget = Budget { outlines: 3.0, quick: 5.0, boxes: 5.5, stop: 6.0, repair: 1.0 };

#[cfg(test)]
thread_local! {
    /// How much longer the budget is: tests that check a layout's quality stretch it, so a
    /// loaded machine can't hurry them.
    static BUDGET_SCALE: std::cell::Cell<f64> = const { std::cell::Cell::new(1.0) };
}

fn budget_scale() -> f64 {
    #[cfg(test)]
    {
        BUDGET_SCALE.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        1.0
    }
}

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

    let flag = |k: &str, default: bool| -> Result<bool> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(bad(cmd, format!("`{k}` must be true or false"))),
        }
    };
    let (lay_flat, keep, avoid) = (flag("lay_flat", true)?, flag("copy", false)?, flag("avoid", true)?);
    // The sheet: the rectangle, or a flat face of some other body at any tilt. The layout is
    // worked out in the sheet's frame (rows: along it, across it, out of it), where it is level.
    let (base, sheet_z, sheet_error, rows, sheet_body) = match p.get("sheet") {
        None | Some(Value::Null) => {
            // On a plane: its X and Y, from its origin; else the XY plane.
            let (rows, o) = match p.get("plane") {
                None | Some(Value::Null) => (IDENTITY, Vec3::ZERO),
                Some(v) => {
                    let plane = super::features::plane_param(s, Some(v), cmd)?;
                    let (vals, _) = s.doc.param_values();
                    let pl = s.doc.resolve_plane_in(&vals, &plane, 0, Some(&s.model.state())).map_err(|e| bad(cmd, e.to_string()))?;
                    let (Some(e1), Some(e2)) = (pl.x.normalized(), pl.y.normalized()) else {
                        return Err(bad(cmd, "the plane is degenerate"));
                    };
                    let Some(n) = e1.cross(e2).normalized() else { return Err(bad(cmd, "the plane is degenerate")) };
                    ([e1, n.cross(e1), n], pl.origin)
                }
            };
            let at = Vec2::new(rows[0].dot(o), rows[1].dot(o));
            let outer = [(0.0, 0.0), (width, 0.0), (width, length), (0.0, length)]
                .iter()
                .map(|&(x, y)| Vec2::new(at.x + origin.x + x, at.y + origin.y + y))
                .collect();
            let z = rows[2].dot(o);
            (Shape2 { outer, holes: Vec::new() }, z, 0.0, rows, None)
        }
        Some(v) => {
            let at = super::face::face_ref(s, v, None, cmd)?;
            let world = s.world_state()?;
            let mut found = None;
            for b in world.bodies.iter().filter(|b| !b.body.is_mesh() && !names.contains(&b.name)) {
                let Ok(record) = solidcraft_kernel::brep_record(&b.body) else { continue };
                if let Some(face) = flat_face_at(&record, at, 1e-4) {
                    found = Some((face, b.name.clone()));
                    break;
                }
            }
            let (face, owner) = match found {
                Some(found) => found,
                None => return Err(bad(cmd, sheet_face_trouble(&world, &names, at))),
            };
            (face.shape, face.height, face.error, [face.e1, face.e2, face.normal], Some(owner))
        }
    };
    let to_local = |v: Vec3| Vec3::new(rows[0].dot(v), rows[1].dot(v), rows[2].dot(v));
    let to_world = |v: Vec3| rows[0] * v.x + rows[1] * v.y + rows[2] * v.z;
    let tilted = rows.iter().zip(IDENTITY).any(|(a, b)| (*a - b).len() > 1e-12);
    // The turn that takes the world into the sheet's frame (its columns: where X, Y and Z go).
    let into_sheet = IDENTITY.map(to_local);
    let picks: Vec<Vec3> = match p.get("bottoms") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.len() <= MAX_BODIES => {
            a.iter().map(|v| super::face::face_ref(s, v, None, cmd).map(to_local)).collect::<Result<_>>()?
        }
        Some(_) => return Err(bad(cmd, format!("`bottoms` must list at most {MAX_BODIES} faces"))),
    };

    // Long work stops when whoever runs it (a dialog's OK or preview) gives up on it.
    let watch = solidcraft_kernel::watch::current();
    let stop = move || watch.as_ref().is_some_and(|w| w.cancelled());
    // The whole layout keeps to a time budget (see `BUDGET`): outlines that take too long are
    // given up for boxes, and a layout running late hurries.
    let started = web_time::Instant::now();
    let elapsed = || started.elapsed().as_secs_f64() / budget_scale();
    let late = |limit: f64| elapsed() > limit;
    let hurried = std::cell::Cell::new(false);
    // Whether the layout itself, not only an outline, ran out of time.
    let pack_hurried = std::cell::Cell::new(false);
    // A body in a component (an imported part) stays as it is: moving it would change the part.
    // With Keep Originals it is copied into the design's own bodies where it stands (its shape
    // only: no link, no joint) and the copy is laid out; without, the run is refused before
    // anything is done.
    let in_component = component_bodies(s, &names);
    if !in_component.is_empty() && !keep {
        let (list, verb) = (in_component.join("`, `"), if in_component.len() == 1 { "is" } else { "are" });
        return Err(bad(
            cmd,
            format!("`{list}` {verb} in a component: arranging would move the part itself; turn on Keep Originals to lay out copies"),
        ));
    }
    // For each body laid out, the body it is a copy of, when it was copied in.
    let mut source_of: Vec<Option<String>> = vec![None; names.len()];
    if !in_component.is_empty() {
        let made = copy_into_design(s, &in_component, cmd)?;
        for (original, copy) in in_component.iter().zip(made) {
            if let Some(i) = names.iter().position(|n| n == original)
                && let (Some(n), Some(src)) = (names.get_mut(i), source_of.get_mut(i))
            {
                *n = copy;
                *src = Some(original.clone());
            }
        }
    }
    // Each body's footprint once it is laid down: as it stands, or on a flat side.
    let world = s.world_state()?;
    let mut items = Vec::with_capacity(names.len());
    let mut lows = Vec::with_capacity(names.len());
    let mut shadows = Vec::with_capacity(names.len());
    let mut boxes = Vec::with_capacity(names.len());
    // What stands where parts may go: (outline from above, lowest z, highest z).
    let mut obstacles: Vec<(Shape2, f64, f64)> = Vec::new();
    let mut spans = Vec::with_capacity(names.len());
    let mut laid_down: Vec<Option<Down>> = Vec::with_capacity(names.len());
    let mut picked = vec![false; picks.len()];
    let mut error: f64 = 0.0;
    let mut boxed: Vec<String> = Vec::new();
    // A body's outline from above, or none when it can't be found or takes too long.
    let give_up = || stop() || late(BUDGET.outlines);
    // Made ready once per body, for every way it is turned.
    let shadow = |record: &Brep| Shadow::new(record, &give_up).ok();
    let outline = |shadow: Option<&Shadow>, rotation: [Vec3; 3]| -> Result<Option<Footprint>> {
        match shadow.map(|sh| sh.footprint(rotation, &give_up)) {
            Some(Ok(fp)) => Ok(Some(fp)),
            _ if stop() => Err(bad(cmd, "cancelled")),
            _ => {
                if late(BUDGET.outlines) {
                    hurried.set(true);
                }
                Ok(None)
            }
        }
    };
    for n in &names {
        let body = world.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))?;
        if body.body.is_mesh() {
            return Err(bad(cmd, format!("`{n}` is a mesh body: only solid bodies can be arranged yet")));
        }
        // On a tilted sheet, the body as seen in the sheet's frame.
        let local = if tilted {
            solidcraft_kernel::transform_matrix(&body.body, matrix(into_sheet)).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?
        } else {
            body.body.clone()
        };
        let record = solidcraft_kernel::brep_record(&local).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
        let bounds = local.exact_bounds().map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
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
            let (down, _, rests) = down_on(&local, &side).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
            if !rests {
                return Err(bad(cmd, format!("`{n}` can't lie on the picked face: some of it would hang below")));
            }
            chosen = Some(down);
        }
        if chosen.is_none() && lay_flat {
            chosen = best_side(&local, &flat_sides(&record)).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
        }
        let ready = shadow(&record);
        if keep && avoid {
            // The body stays: where it stands is not free.
            let shape = match outline(ready.as_ref(), IDENTITY)? {
                Some(f) => {
                    error = error.max(f.error);
                    f.shape
                }
                None => turned_box(bounds.min, bounds.max, IDENTITY),
            };
            obstacles.push((shape, bounds.min.z, bounds.max.z));
        }
        lows.push(bounds.min.z);
        // Where it and its copies will be, in height: standing on the sheet.
        spans.push((sheet_z, sheet_z + (bounds.max - bounds.min).len()));
        let rotation = chosen.map_or(IDENTITY, |d| d.rotation);
        // A body whose outline can't be found takes its box, turned as it will lie: never in
        // another's way, only less tightly packed.
        let shape = match outline(ready.as_ref(), rotation)? {
            Some(fp) => {
                error = error.max(fp.error);
                fp.shape
            }
            None => {
                boxed.push(n.clone());
                turned_box(bounds.min, bounds.max, rotation)
            }
        };
        items.push(ArrangeItem { shape, quantity, rotations_deg: turns.clone() });
        laid_down.push(chosen);
        shadows.push(ready);
        boxes.push(bounds);
    }
    if let Some(k) = picked.iter().position(|m| !*m) {
        return Err(bad(
            cmd,
            format!(
                "bottom face {} is not on a flat face of a body being arranged: a bottom face is the side a part lies on; to lay the parts on that face, give it as the sheet",
                k + 1
            ),
        ));
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
    // The other bodies shown on the sheets stand where they are (hidden ones are not in the way);
    // the sheet's own body (a tray's walls round its floor) is the sheet's edge already. A part
    // copied in from a component is in the way as its copy, which stands just where it does.
    let copied = |name: &String| source_of.iter().flatten().any(|o| o == name);
    for b in world
        .bodies
        .iter()
        .filter(|b| avoid && !names.contains(&b.name) && !copied(&b.name) && sheet_body.as_ref() != Some(&b.name) && s.body_visible(&b.name))
    {
        let turned;
        let local = if tilted {
            match solidcraft_kernel::transform_matrix(&b.body, matrix(into_sheet)) {
                Ok(t) => {
                    turned = t;
                    &turned
                }
                Err(_) => continue,
            }
        } else {
            &b.body
        };
        let Ok(bb) = local.exact_bounds() else { continue };
        if !over(bb.min, bb.max) || !level(bb.min.z, bb.max.z) {
            continue;
        }
        let rect = || Shape2 {
            outer: vec![Vec2::new(bb.min.x, bb.min.y), Vec2::new(bb.max.x, bb.min.y), Vec2::new(bb.max.x, bb.max.y), Vec2::new(bb.min.x, bb.max.y)],
            holes: Vec::new(),
        };
        // A mesh, or a body with no outline, stands in for its box.
        let shape = match (local.is_mesh(), solidcraft_kernel::brep_record(local)) {
            (false, Ok(r)) => match footprint(&r, IDENTITY, &give_up) {
                Ok(f) => {
                    error = error.max(f.error);
                    f.shape
                }
                Err(_) => rect(),
            },
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
    // Widened by the footprints' error (the parts' and what stands in their way), so the true
    // outlines keep the spacing and the margin.
    let opts = ArrangeOptions { method, spacing: spacing + 2.0 * error, margin: margin + error + sheet_error, part_in_part: false, improve, seed: 1 };
    // Each outline coarsened for packing, grown only so it covers its body still: dents the
    // spacing keeps other parts out of are filled for nothing, other detail only as the budget
    // allows. Fewer corners, much less to try.
    let packed_shape = |s: &Shape2| coarsen(s, opts.spacing, COARSEN_BUDGET);
    // The fill counts the true outlines.
    let true_area: Vec<f64> = items.iter().map(|it| shape_area(&it.shape)).collect();
    // Boxes in rows gain nothing from fewer corners.
    if method == ArrangeMethod::TrueShape {
        for it in &mut items {
            it.shape = packed_shape(&it.shape);
        }
    }
    for o in &mut obstacles {
        o.0 = packed_shape(&o.0);
    }
    let sheet_area = shape_area(&base);
    // How far past the budget laying out again round a body that could not move may run.
    let grace = std::cell::Cell::new(0.0f64);
    let hurry = || {
        let t = elapsed() - grace.get();
        if t > BUDGET.stop {
            Hurry::Stop
        } else if t > BUDGET.boxes {
            Hurry::Boxes
        } else if t > BUDGET.quick {
            Hurry::Quick
        } else {
            Hurry::No
        }
    };
    // Where a copy of item `i` placed at `pl` stands, as it was packed.
    let placed_shape = |i: usize, pl: &Placement2| -> Option<Shape2> {
        let (sn, c) = pl.angle.sin_cos();
        let at = |r: &[Vec2]| r.iter().map(|v| Vec2::new(c * v.x - sn * v.y + pl.shift.x, sn * v.x + c * v.y + pl.shift.y)).collect::<Vec<_>>();
        let it = items.get(i)?;
        Some(Shape2 { outer: at(&it.shape.outer), holes: it.shape.holes.iter().map(|h| at(h)).collect() })
    };
    // Lays out the copies `spots` does not hold yet of what is not standing, around the
    // obstacles and the copies already placed: per body, where each new copy goes (sheet,
    // placement).
    let pack = |standing: &[bool], spots: &[Vec<(usize, Placement2)>], blocking: &[Shape2]| -> Result<Vec<Vec<(usize, Placement2)>>> {
        let mut left: Vec<usize> = standing.iter().zip(spots).map(|(st, l)| if *st { 0 } else { quantity.saturating_sub(l.len()) }).collect();
        let mut fresh: Vec<Vec<(usize, Placement2)>> = vec![Vec::new(); items.len()];
        for k in 0..sheets {
            let wanted: Vec<usize> = (0..items.len()).filter(|i| left.get(*i).is_some_and(|n| *n > 0)).collect();
            if wanted.is_empty() {
                break;
            }
            // What stands on this sheet already: the obstacles and the copies placed on it.
            let mut here = blocking.to_vec();
            for (i, list) in spots.iter().enumerate() {
                here.extend(list.iter().filter(|(sk, _)| *sk == k).filter_map(|(_, pl)| placed_shape(i, pl)));
            }
            if here.len() > MAX_OBSTACLES {
                continue;
            }
            let dx = step * k as f64;
            let moved = |r: &[Vec2]| r.iter().map(|v| Vec2::new(v.x + dx, v.y)).collect::<Vec<_>>();
            let sheet = Shape2 { outer: moved(&base.outer), holes: base.holes.iter().map(|h| moved(h)).collect() };
            let these: Vec<ArrangeItem> =
                wanted.iter().filter_map(|i| Some(ArrangeItem { quantity: *left.get(*i)?, ..items.get(*i)?.clone() })).collect();
            let laid = arrange_hurried(&these, &sheet, &here, &opts, &stop, &hurry)
                .map_err(|e| if stop() { bad(cmd, "cancelled") } else { bad(cmd, e.to_string()) })?;
            if laid.hurried > Hurry::No {
                hurried.set(true);
                pack_hurried.set(true);
            }
            // With nothing in the way the next sheet is the same: if nothing fits here, nothing
            // will there.
            if laid.placed.is_empty() && here.is_empty() {
                break;
            }
            for pl in laid.placed {
                let Some(&i) = wanted.get(pl.item) else { continue };
                if let (Some(n), Some(list)) = (left.get_mut(i), fresh.get_mut(i)) {
                    *n = n.saturating_sub(1);
                    list.push((k, Placement2 { item: i, ..pl }));
                }
            }
        }
        Ok(fresh)
    };
    // A body that finds no place stays where it is: when that is on a sheet the others keep
    // clear of it. The copies placed too near it give up their spots, and those copies alone
    // are laid out again, round it and the rest, until no new body is left standing.
    let mut standing = vec![false; items.len()];
    let mut blocking: Vec<Shape2> = obstacles.iter().filter(|(_, lo, hi)| level(*lo, *hi)).map(|(o, _, _)| o.clone()).collect();
    let mut spots: Vec<Vec<(usize, Placement2)>> = vec![Vec::new(); items.len()];
    loop {
        let fresh = pack(&standing, &spots, &blocking)?;
        for (list, more) in spots.iter_mut().zip(fresh) {
            list.extend(more);
        }
        let mut new: Vec<Shape2> = Vec::new();
        if !keep && avoid {
            for (i, list) in spots.iter().enumerate() {
                let (Some(bb), Some(record), Some(st)) = (boxes.get(i), shadows.get(i), standing.get_mut(i)) else { continue };
                if list.is_empty() && !*st && over(bb.min, bb.max) && level(bb.min.z, bb.max.z) {
                    if blocking.len() + new.len() >= MAX_OBSTACLES {
                        return Err(bad(cmd, format!("more than {MAX_OBSTACLES} bodies would stand on the sheet: arrange fewer at a time")));
                    }
                    *st = true;
                    // Its outline where it stands, or its box when that outline's error is more
                    // than the spacing was widened by.
                    let shape = match outline(record.as_ref(), IDENTITY)? {
                        Some(f) if f.error <= error => packed_shape(&f.shape),
                        _ => turned_box(bb.min, bb.max, IDENTITY),
                    };
                    new.push(shape);
                }
            }
        }
        if new.is_empty() {
            break;
        }
        for (i, list) in spots.iter_mut().enumerate() {
            list.retain(|(_, pl)| placed_shape(i, pl).is_some_and(|sh| new.iter().all(|b| apart(&sh, b, opts.spacing))));
        }
        blocking.extend(new);
        // Laying out again has a little time of its own, even when the budget is spent.
        let behind = elapsed() - grace.get() - BUDGET.quick;
        if behind > 0.0 {
            grace.set((grace.get() + behind).min(BUDGET.repair));
        }
    }
    let left: Vec<usize> = spots.iter().map(|l| quantity.saturating_sub(l.len())).collect();
    let used = spots.iter().flatten().map(|(k, _)| k + 1).max().unwrap_or(0);
    let covered: f64 = spots.iter().enumerate().map(|(i, l)| l.len() as f64 * true_area.get(i).copied().unwrap_or(0.0)).sum();
    if spots.iter().all(Vec::is_empty) && pack_hurried.get() {
        return Err(bad(cmd, "the layout ran out of time before any part was placed: arrange fewer parts at a time"));
    }
    if spots.iter().all(Vec::is_empty) {
        let (w, l) = (high(&xs) - low(&xs), high(&ys) - low(&ys));
        return Err(bad(
            cmd,
            format!(
                "nothing fits on the sheet ({w:.1} × {l:.1} mm, less the margin, spacing and what stands on it): make it larger or the spacing smaller"
            ),
        ));
    }

    let mover = super::find_command("solid.move").ok_or_else(|| EngineError::UnknownCommand("solid.move".into()))?;
    let mut placed = Vec::new();
    for (i, list) in spots.iter().enumerate() {
        let Some(name) = names.get(i) else { continue };
        let source = source_of.get(i).cloned().flatten();
        // A body copied in is already a copy: it moves to the first spot itself.
        let keep_here = keep && source.is_none();
        let down = laid_down.get(i).copied().flatten();
        // Copies first, from the body where it stands; the body itself takes the first spot.
        for (n, (sheet, pl)) in list.iter().enumerate().skip(1).chain(list.iter().enumerate().take(1)) {
            let copy = n > 0 || keep_here;
            // Laying down, then the layout's turn about Z: one turn about the origin.
            let (c, sn) = (pl.angle.cos(), pl.angle.sin());
            let spin = |v: Vec3| Vec3::new(c * v.x - sn * v.y, sn * v.x + c * v.y, v.z);
            let r = down.map_or(IDENTITY, |d| d.rotation).map(spin);
            // In the sheet's frame; in the world, into that frame, turned, and back.
            let turn_in_sheet = |v: Vec3| r[0] * v.x + r[1] * v.y + r[2] * v.z;
            let (axis, turn) = axis_angle(IDENTITY.map(|e| to_world(turn_in_sheet(to_local(e)))));
            let dz = match down {
                Some(d) => d.lift + sheet_z,
                None => sheet_z - lows.get(i).copied().unwrap_or(sheet_z),
            };
            let label = if copy { format!("Arrange {name} copy {}", n + usize::from(keep_here)) } else { format!("Arrange {name}") };
            let t = to_world(Vec3::new(pl.shift.x, pl.shift.y, dz));
            let mut q = json!({"bodies": [name], "translate": [t.x, t.y, t.z], "name": label});
            if turn.abs() > 1e-12 {
                q["axis"] = json!([axis.x, axis.y, axis.z]);
                q["angle"] = json!(format!("{} deg", turn.to_degrees()));
            }
            if copy {
                q["copy"] = json!(true);
            }
            let mut body = name.clone();
            if copy || t.x != 0.0 || t.y != 0.0 || t.z != 0.0 || turn.abs() > 1e-12 {
                let reply = mover.run_nested(s, &q)?;
                if copy {
                    body = reply["created_bodies"]
                        .get(0)
                        .and_then(Value::as_str)
                        .map(String::from)
                        .ok_or_else(|| EngineError::Other("the copy made no body".into()))?;
                }
            }
            let bottom = down.map(|d| to_world(d.normal)).map(|v| [v.x, v.y, v.z]);
            let copy_of = source.clone().or_else(|| copy.then(|| name.clone()));
            placed.push(ArrangedBody { body, copy_of, sheet: *sheet, x: pl.shift.x, y: pl.shift.y, angle_deg: pl.angle.to_degrees(), bottom });
        }
    }
    // The copies made go into a new group of their own in the browser ("Arrangement 1", ...).
    let mut copy_groups = Vec::new();
    let copies: Vec<&str> = placed.iter().filter(|b| b.copy_of.is_some()).map(|b| b.body.as_str()).collect();
    if !copies.is_empty() {
        let st = s.model.state();
        let mut by_component: Vec<(u64, Vec<String>)> = Vec::new();
        for (world, own, component, _, _) in s.placed_document().body_frames_with_source(&st) {
            if !copies.contains(&world.as_str()) {
                continue;
            }
            match by_component.iter_mut().find(|(c, _)| *c == component) {
                Some((_, list)) => {
                    if !list.contains(&own) {
                        list.push(own);
                    }
                }
                None => by_component.push((component, vec![own])),
            }
        }
        let grouper = super::find_command("browser.group").ok_or_else(|| EngineError::UnknownCommand("browser.group".into()))?;
        for (component, items) in by_component {
            let taken = |k: usize| s.doc.browser_groups.iter().any(|g| g.name == format!("Arrangement {k}"));
            let k = (1..).find(|k| !taken(*k)).unwrap_or(1);
            let name = format!("Arrangement {k}");
            grouper.run_nested(s, &json!({"folder": "bodies", "component": component, "items": items, "name": name}))?;
            copy_groups.push(name);
        }
    }
    let mut unplaced = Vec::new();
    let mut stray = Vec::new();
    for ((n, l), src) in names.iter().zip(&spots).zip(&source_of) {
        if l.is_empty() {
            unplaced.push(src.clone().unwrap_or_else(|| n.clone()));
            if src.is_some() {
                stray.push(n.clone());
            }
        }
    }
    if !stray.is_empty() {
        let remover = super::find_command("solid.remove").ok_or_else(|| EngineError::UnknownCommand("solid.remove".into()))?;
        remover.run_nested(s, &json!({"bodies": stray}))?;
    }
    // A body that did not fit stays as one of its copies.
    let copies_left = left.iter().zip(&spots).map(|(n, l)| if l.is_empty() && !keep { n.saturating_sub(1) } else { *n }).sum();
    let fill = if used == 0 || sheet_area <= 0.0 { 0.0 } else { covered / (used as f64 * sheet_area) };
    let ring = |r: &[Vec2], dx: f64| r.iter().map(|v| [v.x + dx, v.y]).collect::<Vec<_>>();
    let sheet_outlines = (0..sheets)
        .map(|k| SheetOutline { outer: ring(&base.outer, step * k as f64), holes: base.holes.iter().map(|h| ring(h, step * k as f64)).collect() })
        .collect();
    let reply = ArrangeReply {
        placed,
        unplaced,
        copies_left,
        sheets_used: used,
        fill,
        footprint_error_mm: error,
        sheet_outlines,
        sheet_z,
        sheet_frame: tilted.then(|| rows.map(|v| [v.x, v.y, v.z])),
        margin_mm: margin,
        boxed,
        copy_groups,
        hurried: hurried.get(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The bodies of `names` that are in a component (an imported part's), in the order given.
fn component_bodies(s: &Session, names: &[String]) -> Vec<String> {
    let st = s.model.state();
    let placed: Vec<String> =
        s.placed_document().body_frames_with_source(&st).into_iter().filter(|(_, _, _, path, _)| !path.is_empty()).map(|(w, ..)| w).collect();
    names.iter().filter(|n| placed.contains(n)).cloned().collect()
}

/// Copies of `bodies` (in components) in the design's own bodies, each where it stands in the
/// world: its shape only, kept in the design as a fixed import, with no link to its part and no
/// joint. Named as the app names copies; the new names, in order.
fn copy_into_design(s: &mut Session, bodies: &[String], cmd: &str) -> Result<Vec<String>> {
    let world = s.world_state()?;
    let st = s.model.state();
    let own: Vec<(String, String)> = s.placed_document().body_frames_with_source(&st).into_iter().map(|(w, o, ..)| (w, o)).collect();
    let mut names: Vec<String> = Vec::with_capacity(bodies.len());
    let mut shapes = Vec::with_capacity(bodies.len());
    for n in bodies {
        let body = world.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))?;
        let base = own.iter().find(|(w, _)| w == n).map_or(n.as_str(), |(_, o)| o.as_str());
        let taken = |x: &str| st.body(x).is_some() || world.body(x).is_some() || names.iter().any(|m| m == x);
        names.push(solidcraft_doc::copy_name(base, taken));
        shapes.push(body.body.clone());
    }
    drop(st);
    let list: Vec<solidcraft_kernel::ExportBody> =
        names.iter().zip(&shapes).map(|(name, body)| solidcraft_kernel::ExportBody { name: name.clone(), body, color: body.color() }).collect();
    let header = solidcraft_kernel::StepHeader { organization: "SolidCraft".into(), ..Default::default() };
    let step = solidcraft_kernel::step_export_bodies(&list, &header).map_err(|e| bad(cmd, format!("copying the bodies in: {e}")))?;
    // The design itself (its root component, at the world's frame).
    let root = 0;
    let mut scratch = s.scratch();
    let kind = solidcraft_doc::FeatureKind::Import { file: "Arrange copies".into(), step: step.into(), components: Vec::new(), skip: Vec::new() };
    let id = scratch.doc_mut().add_feature(kind, None)?;
    if let Some(f) = scratch.doc_mut().feature_mut(id) {
        f.component = root;
        f.body_names = names.clone();
    }
    scratch.refresh();
    if let Some(error) = scratch.model.result(id).and_then(|r| r.error.as_ref()) {
        return Err(bad(cmd, format!("the copies of the bodies in components didn't come in: {error}")));
    }
    s.accept_computed(scratch);
    Ok(names)
}

/// Why no sheet was found at `at`: it is on a body being arranged, or on no flat face.
fn sheet_face_trouble(world: &solidcraft_doc::ModelState, names: &[String], at: Vec3) -> String {
    for b in world.bodies.iter().filter(|b| !b.body.is_mesh() && names.contains(&b.name)) {
        let Ok(record) = solidcraft_kernel::brep_record(&b.body) else { continue };
        if flat_face_at(&record, at, 1e-4).is_some() {
            return format!("the sheet face is on `{}`, which is being arranged: take it out of the bodies or pick another face", b.name);
        }
    }
    "the sheet must be a flat face of a body".into()
}

/// The box from `lo` to `hi` turned by `r` (its columns: where X, Y and Z go), seen from above:
/// the rectangle round its eight corners.
fn turned_box(lo: Vec3, hi: Vec3, r: [Vec3; 3]) -> Shape2 {
    let (mut a, mut b) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for k in 0..8 {
        let c = Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
        let t = r[0] * c.x + r[1] * c.y + r[2] * c.z;
        a = Vec2::new(a.x.min(t.x), a.y.min(t.y));
        b = Vec2::new(b.x.max(t.x), b.y.max(t.y));
    }
    Shape2 { outer: vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)], holes: Vec::new() }
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
    // Measured on the body's one exact model, turned: no new body per side tried.
    let b = body.exact_bounds_moved(&matrix(rotation)).map_err(|e| e.to_string())?;
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
