//! Arrange (`solid.arrange`, #12430): lay bodies out side by side on a rectangle of the XY plane,
//! as for a build plate or a sheet to cut from. Each body keeps its height and is only turned
//! about Z, unless it is laid down on a flat side first: the side picked for it, or with
//! `lay_flat` the one it rests on best ([`solidcraft_geom::rest`]). Its footprint is the outline
//! it covers seen from above, worked out from its exact edges and silhouettes
//! ([`solidcraft_geom::footprint`]); the footprints are packed by [`solidcraft_geom::arrange`],
//! and each body that fits is moved there by a Move feature, all in one undo step. Bodies that
//! don't fit stay where they are and are listed.
//!
//! Each body listed is laid out on its own, one in a component too: only that body moves, by a
//! Move feature in its component, and the component's placement and its joints stay as they
//! are. The sheet may also be sketch areas and flat faces in one plane, joined where they
//! touch, as Extrude takes areas ([`sheet_areas`]).

use serde_json::{Value, json};
use solidcore_record::brep::Brep;
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::arrange::Hurry;
use solidcraft_geom::arrange::{
    ArrangeItem, ArrangeMethod, ArrangeOptions, COARSEN_BUDGET, MAX_COPIES, MAX_IMPROVE, MAX_OBSTACLES, MAX_ROTATIONS, Placement2, Shape2, apart,
    arrange_hurried, coarsen,
};
use solidcraft_geom::footprint::{Footprint, Shadow, footprint, footprint_of};
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
        "bodies: [names] (solid bodies; each is laid out on its own: a body in a component moves alone, its component's placement, its other bodies and its joints stay as they are, and copies of it are bodies of the design's own); width, length: expr (the rectangle along X and Y); \
         plane?: plane (the rectangle lies on it, along its X and Y from its origin, the bodies on its normal side; default XY); origin?: [x, y] (its lower-left corner on the plane, default [0, 0]); sheet?: face | [{face: face} | {sketch: id|name, profile: index}] (in place of the rectangle: flat faces at any tilt, of bodies \
         not arranged, and sketch areas, all in one plane; their outlines and holes, joined where they touch, each piece apart a sheet of its own; the bodies lie on them, laid out in their plane, on the first one's side); spacing?: mm between bodies (default 1); margin?: mm from the sheet's edge \
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
    // Each body is laid out on its own, whatever component it is in or joints hold it.
    let pieces = pieces(s, &names);
    let in_piece = |name: &String| pieces.iter().any(|pc| pc.bodies.contains(name));
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
        return Err(bad(cmd, "a `sheet` of faces or areas is laid out as it is: `sheets` above 1 needs the rectangle"));
    }
    if pieces.len() * quantity > MAX_COPIES {
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
    // The sheets: the rectangle and those beside it, or flat faces of other bodies and sketch
    // areas in one plane at any tilt. The layout is worked out in the sheets' frame (rows: along
    // it, across it, out of it), where they are level.
    let Sheets { shapes: sheet_list, z: sheet_z, error: sheet_error, rows, owners: sheet_owners } = match p.get("sheet") {
        None | Some(Value::Null) => {
            // On a plane: its X and Y, from its origin; else the XY plane.
            let (rows, o) = match p.get("plane") {
                None | Some(Value::Null) => (IDENTITY, Vec3::ZERO),
                Some(v) => {
                    let plane = super::features::plane_param(s, Some(v), cmd)?;
                    let (vals, _) = s.doc.param_values();
                    let pl =
                        s.doc.resolve_plane_in(&vals, &plane, 0, s.active_component, Some(&s.model.state())).map_err(|e| bad(cmd, e.to_string()))?;
                    let (Some(e1), Some(e2)) = (pl.x.normalized(), pl.y.normalized()) else {
                        return Err(bad(cmd, "the plane is degenerate"));
                    };
                    let Some(n) = e1.cross(e2).normalized() else { return Err(bad(cmd, "the plane is degenerate")) };
                    ([e1, n.cross(e1), n], pl.origin)
                }
            };
            let at = Vec2::new(rows[0].dot(o), rows[1].dot(o));
            // Sheet after sheet along X.
            let shapes = (0..sheets)
                .map(|k| {
                    let dx = (width + sheet_spacing) * k as f64;
                    let outer = [(0.0, 0.0), (width, 0.0), (width, length), (0.0, length)]
                        .iter()
                        .map(|&(x, y)| Vec2::new(at.x + origin.x + x + dx, at.y + origin.y + y))
                        .collect();
                    Shape2 { outer, holes: Vec::new() }
                })
                .collect();
            Sheets { shapes, z: rows[2].dot(o), error: 0.0, rows, owners: Vec::new() }
        }
        // A list of faces and areas, or one face as it was given before areas were.
        Some(Value::Array(list)) if list.first().is_some_and(Value::is_object) => sheet_areas(s, list, &pieces, cmd)?,
        Some(v) => sheet_areas(s, &[json!({"face": v})], &pieces, cmd)?,
    };
    // The rectangles are all alike: what does not fit on one fits on none.
    let alike = !on_face;
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
    // Each piece's footprint once it is laid down: as it stands, or on a flat side.
    let world = s.world_state()?;
    let mut items = Vec::with_capacity(pieces.len());
    let mut lows = Vec::with_capacity(pieces.len());
    let mut shadows: Vec<Vec<Option<Shadow>>> = Vec::with_capacity(pieces.len());
    let mut boxes = Vec::with_capacity(pieces.len());
    // What stands where parts may go: (outline from above, lowest z, highest z).
    let mut obstacles: Vec<(Shape2, f64, f64)> = Vec::new();
    let mut spans = Vec::with_capacity(pieces.len());
    let mut laid_down: Vec<Option<Down>> = Vec::with_capacity(pieces.len());
    let mut picked = vec![false; picks.len()];
    let mut error: f64 = 0.0;
    let mut boxed: Vec<String> = Vec::new();
    // A body's outline from above, or none when it can't be found or takes too long.
    let give_up = || stop() || late(BUDGET.outlines);
    // Made ready once per body, for every way it is turned.
    let shadow = |record: &Brep| Shadow::new(record, &give_up).ok();
    // A piece's outline: all its bodies' together; none when one of them has none.
    let outline = |ready: &[Option<Shadow>], rotation: [Vec3; 3]| -> Result<Option<Footprint>> {
        let all: Option<Vec<&Shadow>> = ready.iter().map(Option::as_ref).collect();
        match all.map(|sh| footprint_of(&sh, rotation, &give_up)) {
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
    for pc in &pieces {
        let n = pc.name();
        let mut locals = Vec::with_capacity(pc.bodies.len());
        let mut records = Vec::with_capacity(pc.bodies.len());
        let mut bounds: Option<solidcraft_geom::Aabb3> = None;
        for m in &pc.bodies {
            let body = world.body(m).ok_or_else(|| bad(cmd, format!("no body `{m}`")))?;
            if body.body.is_mesh() {
                return Err(bad(cmd, format!("`{m}` is a mesh body: only solid bodies can be arranged yet")));
            }
            // On a tilted sheet, the body as seen in the sheet's frame.
            let local = if tilted {
                solidcraft_kernel::transform_matrix(&body.body, matrix(into_sheet)).map_err(|e| bad(cmd, format!("`{m}`: {e}")))?
            } else {
                body.body.clone()
            };
            let record =
                if local.is_mesh() { None } else { Some(solidcraft_kernel::brep_record(&local).map_err(|e| bad(cmd, format!("`{m}`: {e}")))?) };
            let bb = local.exact_bounds().map_err(|e| bad(cmd, format!("`{m}`: {e}")))?;
            bounds = Some(bounds.map_or(bb, |b| b.union(&bb)));
            locals.push(local);
            records.push(record);
        }
        let Some(bounds) = bounds else { return Err(bad(cmd, format!("`{n}` has no bodies"))) };
        let tol = 1e-6 * (bounds.max - bounds.min).len().max(1.0);
        let mut chosen = None;
        for (k, at) in picks.iter().enumerate() {
            for (j, record) in records.iter().enumerate() {
                let Some(side) = record.as_ref().and_then(|r| side_at(r, *at, tol)) else { continue };
                if chosen.is_some() {
                    return Err(bad(cmd, format!("two bottom faces are on `{n}`: pick one face for a part to lie on")));
                }
                match picked.get_mut(k) {
                    Some(true) => return Err(bad(cmd, format!("bottom face {} is on more than one body", k + 1))),
                    Some(m) => *m = true,
                    None => {}
                }
                let (down, _, rests) = down_on(&locals, j, &side).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
                if !rests {
                    return Err(bad(cmd, format!("`{n}` can't lie on the picked face: some of it would hang below")));
                }
                chosen = Some(down);
            }
        }
        // Laying flat looks at one body's sides: parts put together stand as they are.
        if chosen.is_none()
            && lay_flat
            && let ([_], [Some(record)]) = (locals.as_slice(), records.as_slice())
        {
            chosen = best_side(&locals, &flat_sides(record)).map_err(|e| bad(cmd, format!("`{n}`: {e}")))?;
        }
        let ready: Vec<Option<Shadow>> = records.iter().map(|r| r.as_ref().and_then(shadow)).collect();
        if keep && avoid {
            // The piece stays: where it stands is not free.
            let shape = match outline(&ready, IDENTITY)? {
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
        // A piece whose outline can't be found takes its box, turned as it will lie: never in
        // another's way, only less tightly packed.
        let shape = match outline(&ready, rotation)? {
            Some(fp) => {
                error = error.max(fp.error);
                fp.shape
            }
            None => {
                boxed.push(n.to_string());
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
    // Where the sheets are, grown by what keeps parts apart: a body outside it is in no one's way.
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for v in sheet_list.iter().flat_map(|sh| &sh.outer) {
        lo = Vec2::new(lo.x.min(v.x), lo.y.min(v.y));
        hi = Vec2::new(hi.x.max(v.x), hi.y.max(v.y));
    }
    let grow = spacing + margin + 1e-6;
    let reach = (lo.x - grow, lo.y - grow, hi.x + grow, hi.y + grow);
    let over = |lo: Vec3, hi: Vec3| hi.x >= reach.0 && lo.x <= reach.2 && hi.y >= reach.1 && lo.y <= reach.3;
    // Bodies are in the way only where the parts will be, in height; one just below the sheet
    // (the table under it) is not.
    let (zlo, zhi) = spans.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| (lo.min(*a), hi.max(*b)));
    let ztol = 1e-6 * (zhi - zlo).abs().max(1.0);
    let level = |lo: f64, hi: f64| hi > zlo + ztol && lo < zhi - ztol;
    // The other bodies shown on the sheets stand where they are (hidden ones are not in the way);
    // the sheet's own body (a tray's walls round its floor) is the sheet's edge already.
    for b in world.bodies.iter().filter(|b| avoid && !in_piece(&b.name) && !sheet_owners.contains(&b.name) && s.body_visible(&b.name)) {
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
    let sheet_areas: Vec<f64> = sheet_list.iter().map(shape_area).collect();
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
        for (k, sheet) in sheet_list.iter().enumerate() {
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
            let these: Vec<ArrangeItem> =
                wanted.iter().filter_map(|i| Some(ArrangeItem { quantity: *left.get(*i)?, ..items.get(*i)?.clone() })).collect();
            let laid = arrange_hurried(&these, sheet, &here, &opts, &stop, &hurry)
                .map_err(|e| if stop() { bad(cmd, "cancelled") } else { bad(cmd, e.to_string()) })?;
            if laid.hurried > Hurry::No {
                hurried.set(true);
                pack_hurried.set(true);
            }
            // With nothing in the way the next rectangle is the same: if nothing fits here,
            // nothing will there.
            if alike && laid.placed.is_empty() && here.is_empty() {
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
                    let shape = match outline(record, IDENTITY)? {
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
        let first = sheet_list.first().map(|sh| sh.outer.as_slice()).unwrap_or_default();
        let span = |f: fn(&Vec2) -> f64| {
            let v: Vec<f64> = first.iter().map(f).collect();
            v.iter().copied().fold(f64::NEG_INFINITY, f64::max) - v.iter().copied().fold(f64::INFINITY, f64::min)
        };
        let (w, l) = (span(|v| v.x), span(|v| v.y));
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
        let Some(pc) = pieces.get(i) else { continue };
        let name = pc.name().to_string();
        let down = laid_down.get(i).copied().flatten();
        // A body in a component is copied by way of a copy of it in the design's own bodies,
        // made where it stands (its shape only): Move copies a body in its own component's
        // frame, which is not where it stands under a moved placement.
        let copies = list.len().saturating_sub(usize::from(!keep));
        let source = if pc.placed && copies > 0 { copy_into_design(s, std::slice::from_ref(&name), cmd)?.into_iter().next() } else { None };
        let last_copy = if keep { 0 } else { list.len().saturating_sub(1) };
        // Copies first, from the body where it stands; the body itself takes the first spot.
        for (n, (sheet, pl)) in list.iter().enumerate().skip(1).chain(list.iter().enumerate().take(1)) {
            let copy = n > 0 || keep;
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
            let label = if copy { format!("Arrange {name} copy {}", n + usize::from(keep)) } else { format!("Arrange {name}") };
            let t = to_world(Vec3::new(pl.shift.x, pl.shift.y, dz));
            let turned = turn.abs() > 1e-12;
            let mut q = json!({"translate": [t.x, t.y, t.z], "name": label});
            if turned {
                q["axis"] = json!([axis.x, axis.y, axis.z]);
                q["angle"] = json!(format!("{} deg", turn.to_degrees()));
            }
            let mut body = name.clone();
            if copy {
                match &source {
                    // The copy made in the design takes the last place itself.
                    Some(src) if n == last_copy => {
                        if t.x != 0.0 || t.y != 0.0 || t.z != 0.0 || turned {
                            q["bodies"] = json!([src]);
                            mover.run_nested(s, &q)?;
                        }
                        body = src.clone();
                    }
                    _ => {
                        q["copy"] = json!(true);
                        q["bodies"] = json!([source.as_ref().unwrap_or(&name)]);
                        let reply = mover.run_nested(s, &q)?;
                        body = reply["created_bodies"]
                            .get(0)
                            .and_then(Value::as_str)
                            .map(String::from)
                            .ok_or_else(|| EngineError::Other("the copy made no body".into()))?;
                    }
                }
            } else if t.x != 0.0 || t.y != 0.0 || t.z != 0.0 || turned {
                q["bodies"] = json!([name]);
                // Move takes the motion in the frame the body's geometry is in: for a body in
                // a placed component, the world's motion as seen from that placement.
                if let Some(frame) = pc.frame.as_ref()
                    && let Some(back) = solidcraft_doc::mat_inverse(frame)
                {
                    let at = solidcraft_doc::apply_point(frame, Vec3::new(0.0, 0.0, 0.0));
                    let moved = if turned { about(axis, turn, at) } else { at };
                    let (a, tl) = (solidcraft_doc::apply_vector(&back, axis), solidcraft_doc::apply_vector(&back, moved + t - at));
                    q["translate"] = json!([tl.x, tl.y, tl.z]);
                    if turned {
                        q["axis"] = json!([a.x, a.y, a.z]);
                    }
                }
                mover.run_nested(s, &q)?;
            }
            let bottom = down.map(|d| to_world(d.normal)).map(|v| [v.x, v.y, v.z]);
            let copy_of = copy.then(|| name.clone());
            placed.push(ArrangedBody { body, copy_of, sheet: *sheet, x: pl.shift.x, y: pl.shift.y, angle_deg: pl.angle.to_degrees(), bottom });
        }
    }
    // The copies of bodies made go into a new group of their own in the browser ("Arrangement
    // 1", ...).
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
    let unplaced: Vec<String> = pieces.iter().zip(&spots).filter(|(_, l)| l.is_empty()).map(|(pc, _)| pc.name().to_string()).collect();
    // A piece that did not fit stays as one of its copies.
    let copies_left = left.iter().zip(&spots).map(|(n, l)| if l.is_empty() && !keep { n.saturating_sub(1) } else { *n }).sum();
    let area_used: f64 = sheet_areas.iter().take(used).sum();
    let fill = if used == 0 || area_used <= 0.0 { 0.0 } else { covered / area_used };
    let ring = |r: &[Vec2]| r.iter().map(|v| [v.x, v.y]).collect::<Vec<_>>();
    let sheet_outlines =
        sheet_list.iter().map(|sh| SheetOutline { outer: ring(&sh.outer), holes: sh.holes.iter().map(|h| ring(h)).collect() }).collect();
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

/// A body laid out: on its own, whatever component it is in.
#[derive(Debug)]
struct Piece {
    /// The body, by its world name (a list so an outline is worked out the same way for all).
    bodies: Vec<String>,
    /// Whether it is in a component placed in the design, not a body of the design's own.
    placed: bool,
    /// For one in a placed component: from the frame its geometry is in to the world.
    frame: Option<solidcraft_doc::Mat>,
}

impl Piece {
    fn name(&self) -> &str {
        self.bodies.first().map_or("", String::as_str)
    }
}

/// The pieces `names` make, in the order listed: each body alone. One in a component moves by
/// a Move feature in that component; its placement and joints are not touched.
fn pieces(s: &Session, names: &[String]) -> Vec<Piece> {
    let st = s.model.state();
    let frames = s.placed_document().body_frames_with_source(&st);
    drop(st);
    names
        .iter()
        .map(|n| {
            let frame = frames.iter().find(|f| &f.0 == n && !f.3.is_empty()).map(|f| f.4);
            Piece { bodies: vec![n.clone()], placed: frame.is_some(), frame }
        })
        .collect()
}

/// `v` turned by `angle` (radians) about `axis` through the origin.
fn about(axis: Vec3, angle: f64, v: Vec3) -> Vec3 {
    let Some(k) = axis.normalized() else { return v };
    let (sin, cos) = angle.sin_cos();
    v * cos + k.cross(v) * sin + k * (k.dot(v) * (1.0 - cos))
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

/// The sheets laid out on, in their frame.
struct Sheets {
    /// One per sheet, in order.
    shapes: Vec<Shape2>,
    /// The height they lie at, in the frame.
    z: f64,
    /// How far their outlines may be inside or outside the true ones (mm).
    error: f64,
    /// The frame: along the sheets, across them and out of them.
    rows: [Vec3; 3],
    /// The bodies whose faces they are: not in the way of the parts.
    owners: Vec<String>,
}

/// Most faces and areas one sheet is made of.
const MAX_AREAS: usize = 500;

/// The sheets made of flat faces of other bodies and sketch areas, all in one plane at any tilt
/// (`sheet: [{face} | {sketch, profile}]`): the frame is the first one's (the parts stand on its
/// side), a sketch's areas are joined exactly where they share an edge, as Extrude joins them,
/// then all are joined where their outlines share an edge, and each piece apart is a sheet of
/// its own.
fn sheet_areas(s: &Session, list: &[Value], pieces: &[Piece], cmd: &str) -> Result<Sheets> {
    use solidcore_record::{Loop2, Region2, Seg2};
    if list.is_empty() || list.len() > MAX_AREAS {
        return Err(bad(cmd, format!("`sheet` must list 1 to {MAX_AREAS} faces and sketch areas")));
    }
    let in_piece = |name: &String| pieces.iter().any(|pc| pc.bodies.contains(name));
    let world = s.world_state()?;
    let st = s.model.state();
    // Each pick as rings of world points (outer, then holes), with its frame.
    let mut picked: Vec<(Vec<Vec<Vec3>>, [Vec3; 3])> = Vec::new();
    let mut owners: Vec<String> = Vec::new();
    let mut error: f64 = 0.0;
    // A sketch's areas, by sketch, joined before anything else.
    let mut by_sketch: Vec<(u64, Vec<Region2>)> = Vec::new();
    let mut first_rows: Option<[Vec3; 3]> = None;
    for v in list {
        if let Some(face) = v.get("face") {
            let at = super::face::face_ref(s, face, None, cmd)?;
            let mut found = None;
            for b in world.bodies.iter().filter(|b| !b.body.is_mesh() && !in_piece(&b.name)) {
                let Ok(record) = solidcraft_kernel::brep_record(&b.body) else { continue };
                if let Some(face) = flat_face_at(&record, at, 1e-4) {
                    found = Some((face, b.name.clone()));
                    break;
                }
            }
            let Some((face, owner)) = found else { return Err(bad(cmd, sheet_face_trouble(&world, pieces, at))) };
            let rows = [face.e1, face.e2, face.normal];
            first_rows.get_or_insert(rows);
            let lift = |r: &[Vec2]| r.iter().map(|p| face.e1 * p.x + face.e2 * p.y + face.normal * face.height).collect::<Vec<_>>();
            let mut rings = vec![lift(&face.shape.outer)];
            rings.extend(face.shape.holes.iter().map(|h| lift(h)));
            picked.push((rings, rows));
            error = error.max(face.error);
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        } else if let Some(sketch) = v.get("sketch") {
            let id = super::features::sketch_id(s, Some(sketch), cmd, "sheet")?;
            let ss = st.sketch(id).ok_or_else(|| bad(cmd, format!("no sketch {id}")))?;
            let index = v.get("profile").and_then(Value::as_u64).and_then(|k| usize::try_from(k).ok());
            let pr = index.and_then(|k| ss.profiles.get(k)).ok_or_else(|| bad(cmd, "`profile` must be the index of an area of the sketch"))?;
            if first_rows.is_none() {
                first_rows = Some(sketch_rows(ss, cmd)?);
            }
            match by_sketch.iter_mut().find(|(k, _)| *k == id) {
                Some((_, regions)) => regions.push(pr.region.clone()),
                None => by_sketch.push((id, vec![pr.region.clone()])),
            }
        } else {
            return Err(bad(cmd, "each `sheet` entry is {face} or {sketch, profile}"));
        }
    }
    for (id, regions) in &by_sketch {
        let Some(ss) = st.sketch(*id) else { continue };
        let rows = sketch_rows(ss, cmd)?;
        for r in solidcraft_sketch::merge_regions(regions) {
            // Curves as chords within a ten-thousandth of the area's size, at least 1 µm.
            let tol = (1e-4 * r.area().abs().sqrt()).max(1e-3);
            error = error.max(tol);
            let lift = |l: &Loop2| l.polyline(tol).into_iter().map(|p| ss.plane.to_world(p)).collect::<Vec<_>>();
            let mut rings = vec![lift(&r.outer)];
            rings.extend(r.holes.iter().map(lift));
            picked.push((rings, rows));
        }
    }
    drop(st);
    // The first pick's frame, in the order given.
    let rows = first_rows.unwrap_or(IDENTITY);
    let flat = |p: Vec3| (Vec2::new(rows[0].dot(p), rows[1].dot(p)), rows[2].dot(p));
    let mut z: Option<f64> = None;
    let mut size: f64 = 1.0;
    let mut regions: Vec<Region2> = Vec::new();
    for (rings, frame) in &picked {
        if frame[2].cross(rows[2]).len() > 1e-6 {
            return Err(bad(cmd, "the sheet's faces and areas must all lie in one plane"));
        }
        let mut loops = Vec::with_capacity(rings.len());
        for ring in rings {
            let mut pts = Vec::with_capacity(ring.len());
            for p in ring {
                let (q, h) = flat(*p);
                size = size.max(q.x.abs()).max(q.y.abs()).max(h.abs());
                match z {
                    None => z = Some(h),
                    Some(z0) if (h - z0).abs() <= 1e-6 * size => {}
                    Some(_) => return Err(bad(cmd, "the sheet's faces and areas must all lie in one plane")),
                }
                pts.push(q);
            }
            if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-12 * size) {
                pts.pop();
            }
            let segs: Vec<Seg2> = (0..pts.len()).filter_map(|k| Some(Seg2::Line { a: *pts.get(k)?, b: *pts.get((k + 1) % pts.len())? })).collect();
            loops.push(Loop2 { segs });
        }
        let mut loops = loops.into_iter();
        if let Some(outer) = loops.next() {
            regions.push(Region2 { outer: outer.ccw(), holes: loops.map(|h| h.ccw()).collect() });
        }
    }
    let shapes: Vec<Shape2> = solidcraft_sketch::merge_regions(&regions)
        .iter()
        .map(|r| {
            let ring = |l: &Loop2| l.segs.iter().map(Seg2::start).collect::<Vec<_>>();
            Shape2 { outer: ring(&r.outer), holes: r.holes.iter().map(ring).collect() }
        })
        .filter(|sh| sh.outer.len() >= 3)
        .collect();
    if shapes.is_empty() {
        return Err(bad(cmd, "the sheet's faces and areas have no area"));
    }
    Ok(Sheets { shapes, z: z.unwrap_or(0.0), error, rows, owners })
}

/// A sketch's frame: along its X, across it and out of it.
fn sketch_rows(ss: &solidcraft_doc::SolvedSketch, cmd: &str) -> Result<[Vec3; 3]> {
    let (Some(e1), Some(y)) = (ss.plane.x.normalized(), ss.plane.y.normalized()) else {
        return Err(bad(cmd, "a sketch's plane is degenerate"));
    };
    let Some(normal) = e1.cross(y).normalized() else { return Err(bad(cmd, "a sketch's plane is degenerate")) };
    Ok([e1, normal.cross(e1), normal])
}

/// Why no sheet was found at `at`: it is on a body being arranged, or on no flat face.
fn sheet_face_trouble(world: &solidcraft_doc::ModelState, pieces: &[Piece], at: Vec3) -> String {
    for b in world.bodies.iter().filter(|b| !b.body.is_mesh() && pieces.iter().any(|pc| pc.bodies.contains(&b.name))) {
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

/// The bodies moved as one laid on `side` of `bodies[lead]`, their height then, and whether all
/// of them stay above the side's plane: nothing hangs below it, on the exact bodies.
fn down_on(bodies: &[solidcraft_kernel::Body], lead: usize, side: &FlatSide) -> std::result::Result<(Down, f64, bool), String> {
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
    // Measured on the bodies' exact models, turned: no new body per side tried.
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut size: f64 = 1.0;
    if lead >= bodies.len() {
        return Err("no such body".into());
    }
    for body in bodies {
        let b = body.exact_bounds_moved(&matrix(rotation)).map_err(|e| e.to_string())?;
        lo = lo.min(b.min.z);
        hi = hi.max(b.max.z);
        size = size.max((b.max - b.min).len());
    }
    // The side's plane is at z = -offset once turned.
    let tol = 1e-6 * size;
    let rests = lo >= -side.offset - tol;
    Ok((Down { rotation, lift: -lo, normal: side.normal }, hi - lo, rests))
}

/// The flat side `body` rests on with the most of it touching, among those nothing hangs below;
/// of sides as large, the one that leaves it lowest. `None` when it rests on none.
fn best_side(body: &[solidcraft_kernel::Body], sides: &[FlatSide]) -> std::result::Result<Option<Down>, String> {
    let mut best: Option<(Down, f64, f64)> = None;
    for side in sides {
        if best.as_ref().is_some_and(|(_, area, _)| side.area < area * (1.0 - 1e-9)) {
            break;
        }
        let (down, height, rests) = down_on(body, 0, side)?;
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
