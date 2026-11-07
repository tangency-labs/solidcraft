//! Insert PCB (Gerber): a printed circuit board built from its fabrication files (#12797).;
//!
//! The board goes into a new component named after it, made in the active component, as the
//! STEP and mesh imports do. Inside it, the board outline layer (Gerber) and the drill files
//! (Excellon) become one sketch `Outline` on XY with the outline and every hole and slot that
//! lies wholly inside the board, and one extrude of the board's region by its thickness, making
//! the body `Board`: an ordinary sketch and extrude, edited like any other (the thickness stays
//! editable in the history). The whole insert runs inside one command, so it is one undo step
//! and a failure leaves the design as it was.
//!
//! The component's `properties` say where the board came from, as plain text:
//! - `pcb.outline`: the outline layer's file name (`outline text` when it was given as text);
//! - `pcb.drills`: the drill files' names, joined by `, ` (absent without drill files);
//! - `pcb.thickness`: the board's thickness as inserted, in mm.
//!
//! Further layers (copper, solder mask, silkscreen) are meant to join the same component.
//!
//! Which holes are inside the board is decided on the exact outline (its lines and arcs), never
//! on a polygon or mesh made from it.

use std::collections::{HashMap, HashSet};
use std::f64::consts::{FRAC_PI_2, TAU};

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use solidcraft_io::{Drills, Geom2, MAX_DRAWING_BYTES};

use super::CommandSpec;
use super::board_layers::{self, Finish, Layer};
use super::sketch::{edit, ids_of};
use super::sketch_import::{add_geometry, bounds_of};
use crate::params::{bad, string_list};
use crate::schema::board_import::{BoardImportResult, InsertGerberParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

pub(super) const CMD: &str = "file.insert_gerber";
const CONTRACT: TypedContract<InsertGerberParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("file.insert_gerber", "Insert PCB (Gerber)", crate::checked!("file.insert_gerber", insert_gerber))
    .at("SOLID", "INSERT")
    .icon("import")
    .contract(&CONTRACT)
    .output(output_schema_for::<BoardImportResult>)
    .params("outline: path of the board outline Gerber (RS-274X) layer | outline_text: its text; drills?: path or [paths] of Excellon drill files (up to 16) and/or drills_text?: text or [texts]; thickness?: mm (default 1.6, more than 0, at most 20); name?: the new component's (default: the outline file's name without its extension, or Board); origin?: file (default: keep the files' coordinates) | corner (the lower-left corner of the box around the outline at the origin); layers?: {top_copper?, bottom_copper?, top_mask?, bottom_mask?, top_silk?, bottom_silk?: path of that Gerber layer} and/or layers_text?: the same keys with the layers' text; finish?: hasl (default) | enig, how bare copper looks → {component, component_name, sketch, feature, body, bodies, holes, slots, skipped, thickness, offset: [x, y] added to the files' coordinates, layer_sketches?: ids, prints?: names, warnings}: a new component in the active one, named after the board, holding a sketch Outline on XY with the outline (closed loops inside it are cut-outs) and every hole and slot wholly inside the board, and one extrude of the board by its thickness as the body Board; its properties pcb.outline, pcb.drills and pcb.thickness name the files and the thickness; a hole or slot outside the board or across its edge is skipped with a warning; gaps in the outline up to 0.05 mm are closed with a line and a warning, a wider gap is refused, naming it; each layer given is kept exactly as hidden sketches in the component (Top copper, Bottom mask...) and the board gets each side's printed look (solder mask, copper, finish, silkscreen) as an image on its face, Top print and Bottom print; component properties pcb.<layer> name the layer files. A sketch being edited is finished first. One undo step")];

/// End points closer than this (mm) are one point. Fabrication files give coordinates to a
/// micrometre or finer, and a drawn outline's ends meet exactly.
const JOIN: f64 = 1e-3;
const DEFAULT_THICKNESS: f64 = 1.6;
const MAX_THICKNESS: f64 = 20.0;
const MAX_DRILL_FILES: usize = 16;
/// Board a hole must keep between itself and the outline to count as inside (mm): a hole that
/// touches the edge would leave a sliver the kernel can't build.
const CLEAR: f64 = 1e-4;
/// Gaps in an outline up to this wide (mm) are closed with a line: drafting slips and rounding
/// in the files leave them, and board makers close them too.
const BRIDGE: f64 = 0.05;
const MAX_BRIDGED_ENDS: usize = 64;
/// Lines of the outline this short or shorter (mm) may be steps of a curve: some writers draw
/// a rounded corner as hundreds of tiny lines rather than an arc.
const STEP: f64 = 0.5;
/// How far a run of steps may lie from the arc or line it is read as (mm).
const TRACE: f64 = 0.05;
/// A plated hole the drill file doesn't mark as a via or not is taken as one when this small or
/// smaller (mm). This only spares the body hundreds of tiny cuts, which make it slow to build and
/// to show: every via is still a hole to drill, at its own size, in the `Vias` sketch.
const VIA_MAX: f64 = 0.5;
/// The longest board name taken.
const MAX_NAME: usize = 200;

/// Where the board goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Origin {
    /// The coordinates the files give.
    File,
    /// The lower-left corner of the box around the outline at the origin.
    Corner,
}

/// A board to build, from parsed fabrication data.
pub(super) struct Board<'a> {
    /// The outline layer's centre lines: lines, arcs and circles.
    pub outline: &'a [Geom2],
    pub drills: &'a Drills,
    /// mm, already checked.
    pub thickness: f64,
    /// The new component's name.
    pub name: &'a str,
    pub origin: Origin,
    /// Where the outline came from, for the component's properties.
    pub outline_file: &'a str,
    /// Where the drills came from, in order.
    pub drill_files: &'a [String],
    /// Copper, mask and silkscreen layers, read.
    pub layers: &'a [Layer],
    pub finish: Finish,
}

/// The board's sketch and body, inside its component.
const SKETCH_NAME: &str = "Outline";
const BODY_NAME: &str = "Board";

fn insert_gerber(s: &mut Session, p: &Value) -> Result<Value> {
    let thickness = match p.get("thickness") {
        None | Some(Value::Null) => DEFAULT_THICKNESS,
        Some(v) => v
            .as_f64()
            .filter(|t| t.is_finite() && *t > 0.0 && *t <= MAX_THICKNESS)
            .ok_or_else(|| bad(CMD, "`thickness` must be a number of mm, more than 0 and at most 20"))?,
    };
    let origin = match p.get("origin") {
        None | Some(Value::Null) => Origin::File,
        Some(Value::String(o)) if o == "file" => Origin::File,
        Some(Value::String(o)) if o == "corner" => Origin::Corner,
        Some(_) => return Err(bad(CMD, "`origin` must be \"file\" or \"corner\"")),
    };
    let name = match p.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) if n.chars().count() <= MAX_NAME => Some(n.trim()).filter(|n| !n.is_empty()),
        Some(_) => return Err(bad(CMD, format!("`name` must be a name of at most {MAX_NAME} characters"))),
    };
    let outline_path = text_key(p, "outline")?;
    let outline_text = text_key(p, "outline_text")?;
    let drill_paths = string_list(CMD, p, "drills")?;
    let drill_texts = string_list(CMD, p, "drills_text")?;
    if drill_paths.len() + drill_texts.len() > MAX_DRILL_FILES {
        return Err(bad(CMD, format!("give at most {MAX_DRILL_FILES} drill files")));
    }
    let (layer_sources, finish) = board_layers::params(p)?;
    // Every parameter is checked before any file is read.
    let (outline_name, outline_text) = match (outline_path, outline_text) {
        (Some(_), Some(_)) => return Err(bad(CMD, "give `outline` or `outline_text`, not both")),
        (Some(path), None) => (file_name(path), read_text(path)?),
        (None, Some(text)) if text.len() <= MAX_DRAWING_BYTES => ("outline text".to_string(), text.to_string()),
        (None, Some(_)) => return Err(bad(CMD, "the outline is too large")),
        (None, None) => return Err(bad(CMD, "give `outline` (the board outline Gerber file) or `outline_text`")),
    };
    let mut warnings = Vec::new();
    let read = solidcraft_io::read_gerber_outline(&outline_text).map_err(|e| EngineError::Other(format!("{outline_name}: {e}")))?;
    warnings.extend(read.warnings.iter().map(|w| format!("{outline_name}: {w}")));
    let mut drills = Drills::default();
    let mut sources: Vec<(String, String)> = Vec::new();
    for path in &drill_paths {
        sources.push((file_name(path), read_text(path)?));
    }
    for (i, text) in drill_texts.iter().enumerate() {
        if text.len() > MAX_DRAWING_BYTES {
            return Err(bad(CMD, "a drill file is too large"));
        }
        sources.push((format!("drill text {}", i + 1), text.clone()));
    }
    let outline_box = bounds_of(&read.geom);
    for (label, text) in &sources {
        let mut d = solidcraft_io::read_excellon(text).map_err(|e| EngineError::Other(format!("{label}: {e}")))?;
        if let (Some(dec), Some(bounds)) = (d.guessed_decimals, outline_box) {
            fit_guessed_places(&mut d, dec, bounds, label, &mut warnings);
        }
        drills.hits.extend(d.hits);
        drills.slots.extend(d.slots);
        drills.arcs.extend(d.arcs);
        warnings.extend(d.warnings.iter().map(|w| format!("{label}: {w}")));
    }
    let default_name = outline_path
        .and_then(|path| std::path::Path::new(path).file_stem())
        .map(|stem| stem.to_string_lossy().trim().chars().take(MAX_NAME).collect::<String>())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "Board".to_string());
    let drill_files: Vec<String> = sources.into_iter().map(|(label, _)| label).collect();
    let layers = board_layers::read(&layer_sources, &mut warnings)?;
    let board = Board {
        outline: &read.geom,
        drills: &drills,
        thickness,
        name: name.unwrap_or(default_name.as_str()),
        origin,
        outline_file: &outline_name,
        drill_files: &drill_files,
        layers: &layers,
        finish,
    };
    let reply = build_board(s, &board, warnings).map_err(|e| match e {
        EngineError::Other(msg) if msg.starts_with("the board outline") => EngineError::Other(format!("{outline_name}: {msg}")),
        e => e,
    })?;
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// A drill file that doesn't say how many decimal places its coordinates have was read with
/// the usual number; a file written with one more or one fewer is ten times out, its holes off
/// the board. Of the three readings, the one that puts the most holes in the box round the
/// outline is kept (the usual one when they tie).
fn fit_guessed_places(d: &mut Drills, dec: usize, (lo, hi): (Vec2, Vec2), label: &str, warnings: &mut Vec<String>) {
    let inside = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    let count =
        |k: f64| d.hits.iter().map(|h| h.at).chain(d.slots.iter().map(|s| s.a)).chain(d.arcs.iter().map(|a| a.a)).filter(|p| inside(*p * k)).count();
    let usual = count(1.0);
    let (more, fewer) = (count(0.1), count(10.0));
    let (k, places) = if more > usual && more >= fewer {
        (0.1, dec.saturating_add(1))
    } else if fewer > usual {
        (10.0, dec.saturating_sub(1))
    } else {
        return;
    };
    for h in &mut d.hits {
        h.at = h.at * k;
    }
    for s in &mut d.slots {
        (s.a, s.b) = (s.a * k, s.b * k);
    }
    for a in &mut d.arcs {
        (a.c, a.a, a.b) = (a.c * k, a.a * k, a.b * k);
    }
    warnings.push(format!(
        "{label}: the file doesn't say how many decimal places its coordinates have; they were read with {places}, not the usual {dec}, which puts the holes on the board"
    ));
}

/// A text parameter: absent, `null` or a string.
fn text_key<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t)),
        Some(_) => Err(bad(CMD, format!("`{key}` must be text"))),
    }
}

pub(super) fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

pub(super) fn read_text(path: &str) -> Result<String> {
    if path.trim().is_empty() || path.len() >= 4096 {
        return Err(bad(CMD, "a file path must be a path"));
    }
    let bytes = solidcraft_io::vfs::read(path, MAX_DRAWING_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Build the board from parsed data: its component, sketch and extrude, in the open command's
/// undo step. `warnings` are the readers' own, passed on in front of the import's.
pub(super) fn build_board(s: &mut Session, board: &Board, mut warnings: Vec<String>) -> Result<BoardImportResult> {
    let traced = trace_steps(board.outline, &mut warnings);
    let outline = Outline::new(&traced, &mut warnings)?;
    let offset = match board.origin {
        Origin::File => Vec2::ZERO,
        Origin::Corner => {
            let geom: Vec<Geom2> = outline.edges.iter().map(Edge::geom).collect();
            let (lo, _) = bounds_of(&geom).ok_or_else(|| EngineError::Other("the board outline has no size".into()))?;
            Vec2::ZERO - lo
        }
    };
    let n_holes = board.drills.hits.len().saturating_add(board.drills.slots.len()).saturating_add(board.drills.arcs.len());
    if outline.edges.len().saturating_add(n_holes.saturating_mul(4)) > solidcraft_sketch::MAX_POINTS {
        return Err(solidcraft_sketch::SketchError::TooLarge.into());
    }
    let holes = Holes::pick(&outline, board.drills, &mut warnings);
    // A sketch being edited is finished first, as a person would before inserting a board.
    if s.active_sketch.is_some() {
        run(s, "sketch.finish", &json!({}))?;
    }
    // The board's own component, in the active one; it is active while the board is built, so
    // the sketch and the extrude are made in it, and the active component is put back after,
    // whether the build works or not (a failing command puts the design back, not that).
    let parent = s.active_component;
    let component = s.doc_mut().add_component(Some(board.name), parent)?;
    let active = (s.active_component, s.active_occurrence_path.take());
    s.active_component = component;
    s.revision += 1;
    let built = build_in(s, &outline, &holes, offset, board.thickness);
    let mut layer_sketches = Vec::new();
    let mut prints = Vec::new();
    if built.is_ok() && !holes.vias.is_empty() {
        match board_layers::via_sketch(s, &holes.vias, offset, board.thickness) {
            Ok(id) => layer_sketches.push(id),
            Err(e) => warnings.push(format!("The vias could not be kept as a sketch: {e}")),
        }
    }
    if built.is_ok() && !board.layers.is_empty() {
        match board_layers::sketches(s, board.layers, offset, board.thickness) {
            Ok(ids) => layer_sketches.extend(ids),
            Err(e) => warnings.push(format!("The layers could not be kept as sketches: {e}")),
        }
        let mut board_loops: Vec<Vec<Geom2>> =
            outline.loops().iter().map(|(edges, _)| edges.iter().filter_map(|i| outline.edges.get(*i)).map(Edge::geom).collect()).collect();
        if let Ok(cut) = Outline::new(&holes.geom, &mut Vec::new()) {
            board_loops
                .extend(cut.loops().iter().map(|(edges, _)| edges.iter().filter_map(|i| cut.edges.get(*i)).map(Edge::geom).collect::<Vec<_>>()));
        }
        prints = board_layers::decals(s, &board_loops, &holes.vias, board.layers, offset, board.thickness, board.finish, &mut warnings);
    }
    (s.active_component, s.active_occurrence_path) = active;
    s.revision += 1;
    let (sketch, feature, bodies, warning) = built?;
    warnings.extend(warning);
    if let Some(body) = bodies.first() {
        paint(s, body, &outline, &holes, offset, board.thickness, &mut warnings);
    }
    let mut component_name = String::new();
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == component) {
        c.properties.insert("pcb.outline".into(), board.outline_file.to_string());
        if !board.drill_files.is_empty() {
            c.properties.insert("pcb.drills".into(), board.drill_files.join(", "));
        }
        c.properties.insert("pcb.thickness".into(), mm(board.thickness));
        for layer in board.layers {
            c.properties.insert(format!("pcb.{}", layer.role.key), layer.file.clone());
        }
        component_name = c.name.clone();
    }
    Ok(BoardImportResult {
        component,
        component_name,
        sketch,
        feature,
        body: bodies.first().cloned().unwrap_or_else(|| BODY_NAME.to_string()),
        bodies,
        holes: holes.hits,
        vias: holes.vias.len(),
        slots: holes.slots,
        skipped: holes.skipped,
        thickness: board.thickness,
        offset: [offset.x, offset.y],
        layer_sketches,
        prints,
        warnings,
        param_warnings: Vec::new(),
    })
}

/// The board's sketch and extrude, made in the active component: the sketch's and the
/// extrude's ids, the bodies made and the extrude's warning.
fn build_in(s: &mut Session, outline: &Outline, holes: &Holes, offset: Vec2, thickness: f64) -> Result<(u64, u64, Vec<String>, Option<String>)> {
    let made = run(s, "sketch.create", &json!({"plane": "XY", "name": SKETCH_NAME, "project_edges": false}))?;
    let sketch = made.get("sketch").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("the board's sketch was not made".into()))?;
    let shift = move |q: Vec2| q + offset;
    let loops = outline.loops();
    let (material, _) = edit(s, &json!({"sketch": sketch}), CMD, false, |sk, _| {
        // Each loop on its own, so its curve ids are known; the material loops are the profiles.
        let mut material = Vec::new();
        for (edges, depth) in &loops {
            let geom: Vec<Geom2> = edges.iter().filter_map(|i| outline.edges.get(*i)).map(Edge::geom).collect();
            let (curves, _) = add_geometry(sk, &geom, &shift, 1.0, JOIN)?;
            if depth % 2 == 0 {
                material.push(ids_of(sk, &curves));
            }
        }
        add_geometry(sk, &holes.geom, &shift, 1.0, JOIN)?;
        Ok(material)
    })?;
    run(s, "sketch.finish", &json!({}))?;
    let profiles: Vec<Value> = material.iter().map(|ids| json!({"curves": ids})).collect();
    // A unit, so a design in inches still gets a board as thick as asked.
    let distance = format!("{thickness} mm");
    let r =
        run(s, "solid.extrude", &json!({"sketch": sketch, "profiles": profiles, "distance": distance, "operation": "new", "body_name": BODY_NAME}))?;
    let feature = r.get("feature").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("the board's extrude was not made".into()))?;
    let bodies: Vec<String> = r
        .get("created_bodies")
        .and_then(Value::as_array)
        .map(|b| b.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let warning = r.get("warning").and_then(Value::as_str).map(str::to_string);
    Ok((sketch, feature, bodies, warning))
}

/// FR-4 glass-epoxy laminate, the usual board material: its name, density (g/cm³) and the
/// colour seen on the board's edge.
const LAMINATE_NAME: &str = "FR-4";
const LAMINATE_DENSITY: f64 = 1.85;
const LAMINATE: &str = "#cdbb8b";
/// Green solder mask on the top and bottom.
const SOLDER_MASK: &str = "#1d6b40";
/// Copper plating on the walls of plated holes.
const PLATING: &str = "#c8823f";

/// Give the board a manufactured board's material and colours: FR-4 for the body, solder mask on its
/// top and bottom, copper on the walls of plated holes. Only the look: a look that can't be
/// set is a warning, never a failed import.
fn paint(s: &mut Session, body: &str, outline: &Outline, holes: &Holes, offset: Vec2, thickness: f64, warnings: &mut Vec<String>) {
    let mut failed = 0;
    let mut first: Option<String> = None;
    // The laminate is the body's physical material, so its mass is a real board's; a design that
    // has it already (an earlier board) keeps its own.
    let created = run(
        s,
        "material.create",
        &json!({"name": LAMINATE_NAME, "density_g_cm3": LAMINATE_DENSITY, "color": LAMINATE, "pbr": {"metallic": 0.0, "roughness": 0.8, "dielectric_ior": 1.5}}),
    );
    if let Err(e) = run(s, "material.assign", &json!({"bodies": [body], "material": LAMINATE_NAME})) {
        failed += 1;
        first.get_or_insert(match created {
            Err(c) => c.to_string(),
            Ok(_) => e.to_string(),
        });
    }
    let mut assign = |s: &mut Session, p: Value| {
        if let Err(e) = run(s, "appearance.assign", &p) {
            failed += 1;
            first.get_or_insert(e.to_string());
        }
    };
    if let Some(p) = material_point(outline, holes) {
        let p = p + offset;
        let faces = json!([[p.x, p.y, thickness], [p.x, p.y, 0.0]]);
        assign(s, json!({"faces": faces, "body": body, "color": SOLDER_MASK, "pbr": {"metallic": 0.0, "roughness": 0.3, "dielectric_ior": 1.5}}));
    }
    let copper = |pts: &[Vec2]| {
        let faces: Vec<[f64; 3]> = pts.iter().map(|q| [q.x + offset.x, q.y + offset.y, thickness / 2.0]).collect();
        json!({"faces": faces, "body": body, "color": PLATING, "pbr": {"metallic": 1.0, "roughness": 0.35, "dielectric_ior": 1.5}})
    };
    if !holes.plated.is_empty() && run(s, "appearance.assign", &copper(&holes.plated)).is_err() {
        // One wall that isn't there (two holes overlapping) fails the lot: colour them one by one.
        for q in &holes.plated {
            assign(s, copper(std::slice::from_ref(q)));
        }
    }
    if failed > 0 {
        warnings.push(format!("{failed} parts of the board's look could not be set: {}", first.unwrap_or_default()));
    }
}

/// A point of the board's material, clear of its edges and holes: where the top and bottom
/// faces are picked for their colour.
fn material_point(outline: &Outline, holes: &Holes) -> Option<Vec2> {
    const CLEARANCE: f64 = 0.2;
    let geom: Vec<Geom2> = outline.edges.iter().map(Edge::geom).collect();
    let (lo, hi) = bounds_of(&geom)?;
    let cut = Outline::new(&holes.geom, &mut Vec::new()).ok();
    let clear = |p: Vec2| {
        outline.fits_hole(p, CLEARANCE)
            && holes.geom.iter().all(|g| !matches!(*g, Geom2::Circle(c, r) if p.dist(c) < r + CLEARANCE))
            && cut.as_ref().is_none_or(|h| !h.inside(p) && h.edges.iter().all(|e| e.dist_to_point(p) > CLEARANCE))
    };
    const STEPS: usize = 24;
    (1..STEPS)
        .flat_map(|i| (1..STEPS).map(move |j| (i, j)))
        .map(|(i, j)| {
            let (u, v) = (i as f64 / STEPS as f64, j as f64 / STEPS as f64);
            Vec2::new(lo.x + (hi.x - lo.x) * u, lo.y + (hi.y - lo.y) * v)
        })
        .find(|p| clear(*p))
}

pub(super) fn run(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let spec = super::find_command(id).ok_or_else(|| EngineError::Other(format!("`{id}` is missing")))?;
    (spec.run)(s, p)
}

/// A length for a message: at most three decimals, without trailing zeros.
fn mm(v: f64) -> String {
    let t = format!("{v:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" { "0".to_string() } else { t.to_string() }
}

fn at(p: Vec2) -> String {
    format!("({}, {})", mm(p.x), mm(p.y))
}

/// One curve of the outline, exactly as drawn.
#[derive(Clone, Copy, Debug)]
enum Edge {
    Line(Vec2, Vec2),
    /// Counter-clockwise about `c` from `a` to `b`; `from` is `a`'s angle, `sweep` the turn (0, 2π).
    Arc {
        c: Vec2,
        r: f64,
        a: Vec2,
        b: Vec2,
        from: f64,
        sweep: f64,
    },
    Circle(Vec2, f64),
}

impl Edge {
    fn geom(&self) -> Geom2 {
        match *self {
            Edge::Line(a, b) => Geom2::Line(a, b),
            Edge::Arc { c, a, b, .. } => Geom2::Arc { c, a, b },
            Edge::Circle(c, r) => Geom2::Circle(c, r),
        }
    }

    fn ends(&self) -> Option<(Vec2, Vec2)> {
        match *self {
            Edge::Line(a, b) | Edge::Arc { a, b, .. } => Some((a, b)),
            Edge::Circle(..) => None,
        }
    }

    /// Whether the direction `d` from the centre points at the curve.
    fn spans(&self, d: Vec2) -> bool {
        match *self {
            Edge::Arc { from, sweep, .. } => (d.angle() - from).rem_euclid(TAU) <= sweep,
            _ => true,
        }
    }

    /// The distance from `p` to the curve.
    fn dist_to_point(&self, p: Vec2) -> f64 {
        match *self {
            Edge::Line(a, b) => p.dist(closest_on_segment(p, a, b)),
            Edge::Arc { c, r, .. } | Edge::Circle(c, r) => self.round_to_point(p, c, r),
        }
    }

    fn round_to_point(&self, p: Vec2, c: Vec2, r: f64) -> f64 {
        let d = p - c;
        let l = d.len();
        if l > 1e-12 && self.spans(d) {
            return (l - r).abs();
        }
        match self.ends() {
            Some((a, b)) => p.dist(a).min(p.dist(b)),
            None => r,
        }
    }

    /// The distance from the segment `u`–`v` to the curve: the least of distances between
    /// actual points of the two, one of which is the closest pair.
    fn dist_to_segment(&self, u: Vec2, v: Vec2) -> f64 {
        match *self {
            Edge::Line(a, b) => segment_distance(u, v, a, b),
            Edge::Arc { c, r, .. } | Edge::Circle(c, r) => {
                let mut best = self.round_to_point(u, c, r).min(self.round_to_point(v, c, r));
                if let Some((a, b)) = self.ends() {
                    best = best.min(a.dist(closest_on_segment(a, u, v))).min(b.dist(closest_on_segment(b, u, v)));
                }
                // The segment's point nearest the centre, against the curve's point in its direction.
                let d = closest_on_segment(c, u, v) - c;
                let l = d.len();
                if l > 1e-12 && self.spans(d) {
                    best = best.min((l - r).abs());
                }
                // Where the segment crosses the circle on the curve, they meet.
                let w = v - u;
                let (qa, qb, qc) = (w.len2(), 2.0 * (u - c).dot(w), (u - c).len2() - r * r);
                let disc = qb * qb - 4.0 * qa * qc;
                if qa > 1e-24 && disc >= 0.0 {
                    let root = disc.sqrt();
                    for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
                        if (0.0..=1.0).contains(&t) && self.spans(u + w * t - c) {
                            return 0.0;
                        }
                    }
                }
                best
            }
        }
    }

    /// The centre and radius of an arc or a circle.
    fn round(&self) -> Option<(Vec2, f64)> {
        match *self {
            Edge::Arc { c, r, .. } | Edge::Circle(c, r) => Some((c, r)),
            Edge::Line(..) => None,
        }
    }

    /// The distance between two round curves (arcs or circles): the least of the distances from
    /// each one's ends to the other and between their points on the line through both centres,
    /// or none where they cross. The closest pair of points is always one of these.
    fn dist_to_round(&self, other: &Edge) -> f64 {
        let (Some((c1, r1)), Some((c2, r2))) = (self.round(), other.round()) else { return f64::INFINITY };
        let mut best = f64::INFINITY;
        for (curve, to) in [(self, other), (other, self)] {
            if let Some((a, b)) = curve.ends() {
                best = best.min(to.dist_to_point(a)).min(to.dist_to_point(b));
            }
        }
        let w = c2 - c1;
        let d = w.len();
        if d > 1e-12 {
            let u = w * (1.0 / d);
            for (s1, s2) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                let (d1, d2) = (u * s1, u * s2);
                if self.spans(d1) && other.spans(d2) {
                    best = best.min((c1 + d1 * r1).dist(c2 + d2 * r2));
                }
            }
            if d <= r1 + r2 && d >= (r1 - r2).abs() {
                let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let (base, n) = (c1 + u * x, u.perp() * h);
                if [base + n, base - n].iter().any(|p| self.spans(*p - c1) && other.spans(*p - c2)) {
                    return 0.0;
                }
            }
        }
        best
    }

    /// The tool path of a routed arc slot: counter-clockwise about `c` from `a` to `b`, a whole
    /// circle when the ends meet. None when it has no radius.
    fn path(c: Vec2, a: Vec2, b: Vec2) -> Option<Edge> {
        let r = c.dist(a);
        if !(r > JOIN) {
            return None;
        }
        let from = (a - c).angle();
        let sweep = ((b - c).angle() - from).rem_euclid(TAU);
        Some(if a.dist(b) <= JOIN || sweep <= 0.0 { Edge::Circle(c, r) } else { Edge::Arc { c, r, a, b, from, sweep } })
    }

    /// How many times the ray from `p` towards +x crosses the curve. An end on the ray's line
    /// counts as below it, the same for every curve, so a corner is crossed once or not at all.
    fn crossings(&self, p: Vec2) -> usize {
        match *self {
            Edge::Line(a, b) => usize::from(crosses(p, a, b, |_| a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y))),
            Edge::Circle(c, r) => {
                let dy = p.y - c.y;
                if dy.abs() >= r {
                    return 0;
                }
                let h = (r * r - dy * dy).sqrt();
                usize::from(c.x - h > p.x) + usize::from(c.x + h > p.x)
            }
            Edge::Arc { c, r, a, b, from, sweep } => {
                // Pieces that rise or fall all the way, split where the arc turns at its top or
                // bottom; each lies on one side of the centre, which says where it meets the ray.
                let mut cuts: Vec<f64> =
                    [FRAC_PI_2, 3.0 * FRAC_PI_2].iter().map(|k| (k - from).rem_euclid(TAU)).filter(|t| *t > 0.0 && *t < sweep).collect();
                cuts.sort_by(f64::total_cmp);
                let mut stops = vec![0.0];
                stops.extend(cuts);
                stops.push(sweep);
                let point = |t: f64| {
                    if t <= 0.0 {
                        a
                    } else if t >= sweep {
                        b
                    } else {
                        c + Vec2::from_angle(from + t) * r
                    }
                };
                stops
                    .windows(2)
                    .filter(|w| {
                        let right = (from + (w[0] + w[1]) / 2.0).cos() > 0.0;
                        crosses(p, point(w[0]), point(w[1]), |p| {
                            let dy = p.y - c.y;
                            let h = (r * r - dy * dy).max(0.0).sqrt();
                            if right { c.x + h } else { c.x - h }
                        })
                    })
                    .count()
            }
        }
    }
}

/// Whether the piece from `s` to `e`, which rises or falls all the way, crosses the ray from `p`
/// towards +x; `x_at` gives where it meets the ray's line.
fn crosses(p: Vec2, s: Vec2, e: Vec2, x_at: impl Fn(Vec2) -> f64) -> bool {
    (s.y > p.y) != (e.y > p.y) && x_at(p) > p.x
}

fn closest_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let w = b - a;
    let l2 = w.len2();
    if l2 <= 1e-24 {
        return a;
    }
    a + w * ((p - a).dot(w) / l2).clamp(0.0, 1.0)
}

fn segment_distance(u: Vec2, v: Vec2, a: Vec2, b: Vec2) -> f64 {
    let (d1, d2) = ((v - u).cross(a - u), (v - u).cross(b - u));
    let (d3, d4) = ((b - a).cross(u - a), (b - a).cross(v - a));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    let near = |p: Vec2, s: Vec2, e: Vec2| p.dist(closest_on_segment(p, s, e));
    near(u, a, b).min(near(v, a, b)).min(near(a, u, v)).min(near(b, u, v))
}

/// The outline with each run of tiny lines read as the arcs and lines it traces (see [`STEP`]),
/// each within [`TRACE`] of every point of the run. Each run keeps its own end points exactly, so
/// it still joins its neighbours. A thousand steps make a slow board; a few arcs don't.
fn trace_steps(geom: &[Geom2], warnings: &mut Vec<String>) -> Vec<Geom2> {
    let mut out = Vec::with_capacity(geom.len());
    let mut run: Vec<Vec2> = Vec::new();
    let (mut steps, mut made) = (0usize, 0usize);
    let mut flush = |run: &mut Vec<Vec2>, out: &mut Vec<Geom2>| {
        if run.len() > 1 {
            let before = out.len();
            trace(run, out);
            steps += run.len() - 1;
            made += out.len() - before;
        }
        run.clear();
    };
    for g in geom {
        match *g {
            Geom2::Line(a, b) if a.dist(b) <= STEP => {
                if a.dist(b) <= 1e-9 {
                    continue;
                }
                if run.last().is_some_and(|&l| l.dist(a) <= 1e-9) {
                    run.push(b);
                } else {
                    flush(&mut run, &mut out);
                    run.extend([a, b]);
                }
            }
            _ => {
                flush(&mut run, &mut out);
                out.push(g.clone());
            }
        }
    }
    flush(&mut run, &mut out);
    if made + 100 <= steps {
        warnings.push(format!(
            "the board outline draws its curves in {steps} short lines; they were read as the {made} arcs and lines they trace, within {} mm",
            mm(TRACE)
        ));
    }
    out
}

/// Greedily, the longest arc (three steps or more) or line from each point of `pts` on.
fn trace(pts: &[Vec2], out: &mut Vec<Geom2>) {
    let mut i = 0;
    while i + 1 < pts.len() {
        let mut line = i + 1;
        while line + 1 < pts.len() && on_line(&pts[i..=line + 1]) {
            line += 1;
        }
        let mut arc = None;
        let mut end = i + 3;
        while end < pts.len() {
            match on_arc(&pts[i..=end]) {
                Some(g) => arc = Some((end, g)),
                None => break,
            }
            end += 1;
        }
        match arc.filter(|(end, _)| *end > line) {
            Some((end, g)) => {
                out.push(g);
                i = end;
            }
            None => {
                out.push(Geom2::Line(pts[i], pts[line]));
                i = line;
            }
        }
    }
}

/// Every point of `pts` within [`TRACE`] of the line from its first to its last.
fn on_line(pts: &[Vec2]) -> bool {
    let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return false };
    pts.iter().all(|&p| p.dist(closest_on_segment(p, a, b)) <= TRACE)
}

/// The arc through the first, middle and last of `pts`, when every point and every step's middle
/// is within [`TRACE`] of it and the steps turn one way, less than a whole turn.
fn on_arc(pts: &[Vec2]) -> Option<Geom2> {
    let (a, m, b) = (*pts.first()?, *pts.get(pts.len() / 2)?, *pts.last()?);
    let d = 2.0 * (m - a).cross(b - a);
    if d.abs() < 1e-12 {
        return None;
    }
    // The circle through a, m and b, from a.
    let (u, w) = (m - a, b - a);
    let c = a + Vec2::new(w.y * u.len2() - u.y * w.len2(), u.x * w.len2() - w.x * u.len2()) * (1.0 / d);
    let r = c.dist(a);
    if !(r.is_finite() && r < 1e4) {
        return None;
    }
    let near = |p: Vec2| (p.dist(c) - r).abs() <= TRACE;
    let mut sweep = 0.0;
    for s in pts.windows(2) {
        let (p, q) = (s[0], s[1]);
        if !near(q) || !near((p + q) * 0.5) {
            return None;
        }
        let turn = (p - c).cross(q - c).atan2((p - c).dot(q - c));
        if sweep != 0.0 && turn * sweep < 0.0 {
            return None;
        }
        sweep += turn;
    }
    if sweep.abs() >= std::f64::consts::TAU - 1e-6 {
        return None;
    }
    Some(if sweep > 0.0 { Geom2::Arc { c, a, b } } else { Geom2::Arc { c, a: b, b: a } })
}

/// End points, joined when closer than [`JOIN`].
#[derive(Default)]
struct Ends {
    cells: HashMap<(i64, i64), Vec<usize>>,
    at: Vec<Vec2>,
}

impl Ends {
    fn id(&mut self, p: Vec2) -> usize {
        let key = ((p.x / JOIN).floor() as i64, (p.y / JOIN).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let cell = (key.0.saturating_add(dx), key.1.saturating_add(dy));
                if let Some(found) = self.cells.get(&cell).and_then(|ids| ids.iter().find(|i| self.at.get(**i).is_some_and(|o| o.dist(p) <= JOIN))) {
                    return *found;
                }
            }
        }
        let id = self.at.len();
        self.at.push(p);
        self.cells.entry(key).or_default().push(id);
        id
    }
}

/// The board outline: its curves, closed, with the end points they share.
struct Outline {
    edges: Vec<Edge>,
    /// Each edge's two end points' ids (a circle has none).
    ends: Vec<Option<(usize, usize)>>,
}

impl Outline {
    fn new(geom: &[Geom2], warnings: &mut Vec<String>) -> Result<Outline> {
        let finite = |ps: &[Vec2]| ps.iter().all(|p| p.x.is_finite() && p.y.is_finite());
        let mut points = Ends::default();
        let mut edges = Vec::new();
        let mut ends = Vec::new();
        let mut lines = HashSet::new();
        let (mut repeated, mut ignored) = (0, 0);
        for g in geom {
            let edge = match *g {
                Geom2::Line(a, b) if finite(&[a, b]) => {
                    let (i, j) = (points.id(a), points.id(b));
                    // A line drawn twice (often, where two outlines meet) is one edge of the board.
                    if i == j {
                        continue;
                    }
                    if !lines.insert((i.min(j), i.max(j))) {
                        repeated += 1;
                        continue;
                    }
                    edges.push(Edge::Line(a, b));
                    ends.push(Some((i, j)));
                    continue;
                }
                Geom2::Arc { c, a, b } if finite(&[c, a, b]) => {
                    let r = c.dist(a);
                    if r <= JOIN {
                        continue;
                    }
                    let from = (a - c).angle();
                    let sweep = ((b - c).angle() - from).rem_euclid(TAU);
                    if a.dist(b) <= JOIN || sweep <= 0.0 { Edge::Circle(c, r) } else { Edge::Arc { c, r, a, b, from, sweep } }
                }
                Geom2::Circle(c, r) if finite(&[c]) && r.is_finite() => {
                    if r <= JOIN {
                        continue;
                    }
                    Edge::Circle(c, r)
                }
                Geom2::Point(_) | Geom2::Text { .. } => {
                    ignored += 1;
                    continue;
                }
                Geom2::Line(..) | Geom2::Arc { .. } | Geom2::Circle(..) => {
                    return Err(EngineError::Other("the board outline has a curve with a coordinate that is not a number".into()));
                }
                Geom2::Ellipse { .. } | Geom2::Spline { .. } | Geom2::Conic { .. } => {
                    return Err(EngineError::Other("the board outline has a curve that is not a line or an arc".into()));
                }
            };
            let e = edge.ends().map(|(a, b)| (points.id(a), points.id(b)));
            edges.push(edge);
            ends.push(e);
        }
        if repeated > 0 {
            warnings.push(format!("{repeated} outline lines drawn twice were used once"));
        }
        if ignored > 0 {
            warnings.push(format!("{ignored} points or texts on the outline layer were left out"));
        }
        if edges.is_empty() {
            return Err(EngineError::Other("the board outline draws nothing: the file has no lines or arcs".into()));
        }
        // Every end must meet another: an end met by no other curve is a gap in the outline.
        let mut degree = vec![0usize; points.at.len()];
        for (i, j) in ends.iter().flatten() {
            for k in [*i, *j] {
                if let Some(d) = degree.get_mut(k) {
                    *d += 1;
                }
            }
        }
        let mut open: Vec<usize> = degree.iter().enumerate().filter(|(_, d)| *d % 2 == 1).map(|(k, _)| k).collect();
        // Tiny gaps, as drafting slips and rounding leave, are closed with a short line, as a
        // board maker would.
        let mut bridged: Vec<f64> = Vec::new();
        // Only a few open ends are slips; many are a drawing that isn't an outline.
        while open.len() >= 2 && open.len() <= MAX_BRIDGED_ENDS {
            let mut best: Option<(f64, usize, usize)> = None;
            for (x, i) in open.iter().enumerate() {
                for (y, j) in open.iter().enumerate().skip(x + 1) {
                    let (Some(a), Some(b)) = (points.at.get(*i), points.at.get(*j)) else { continue };
                    let d = a.dist(*b);
                    if d <= BRIDGE && best.is_none_or(|(e, ..)| d < e) {
                        best = Some((d, x, y));
                    }
                }
            }
            let Some((d, x, y)) = best else { break };
            let (Some(&i), Some(&j)) = (open.get(x), open.get(y)) else { break };
            if let (Some(&a), Some(&b)) = (points.at.get(i), points.at.get(j)) {
                edges.push(Edge::Line(a, b));
                ends.push(Some((i, j)));
                bridged.push(d);
            }
            open.remove(y);
            open.remove(x);
        }
        match bridged[..] {
            [] => {}
            [one] => warnings.push(format!("closed a gap of {} mm in the board outline", mm(one))),
            _ => {
                let widest = bridged.iter().copied().fold(0.0, f64::max);
                warnings.push(format!("closed {} gaps in the board outline, the widest {} mm", bridged.len(), mm(widest)));
            }
        }
        let open: Vec<Vec2> = open.iter().filter_map(|k| points.at.get(*k).copied()).collect();
        if let Some(first) = open.first() {
            let others = || open.iter().skip(1);
            let nearest = others().min_by(|a, b| a.dist(*first).total_cmp(&b.dist(*first)));
            let more = if open.len() > 2 { format!(" ({} open ends in all)", open.len()) } else { String::new() };
            return Err(EngineError::Other(match nearest {
                Some(q) => format!(
                    "the board outline is not closed: there is a gap of {} mm between {} and {}{more}",
                    mm(first.dist(*q)),
                    at(*first),
                    at(*q)
                ),
                None => format!("the board outline is not closed: it has an open end at {}", at(*first)),
            }));
        }
        Ok(Outline { edges, ends })
    }

    /// The closed loops, as edge indices, each with how many other loops are around it: an
    /// even count is board, an odd one a cut-out.
    fn loops(&self) -> Vec<(Vec<usize>, usize)> {
        // Edges that share an end are in one loop.
        let mut parent: Vec<usize> = (0..self.edges.len()).collect();
        fn root(parent: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while let Some(&p) = parent.get(r) {
                if p == r {
                    break;
                }
                r = p;
            }
            // Point the path straight at the root, so later lookups are short.
            let mut k = i;
            while let Some(&p) = parent.get(k) {
                if p == r || p == k {
                    break;
                }
                if let Some(slot) = parent.get_mut(k) {
                    *slot = r;
                }
                k = p;
            }
            r
        }
        let mut by_point: HashMap<usize, usize> = HashMap::new();
        for (e, ends) in self.ends.iter().enumerate() {
            for k in ends.iter().flat_map(|(i, j)| [*i, *j]) {
                match by_point.get(&k) {
                    Some(&other) => {
                        let (a, b) = (root(&mut parent, e), root(&mut parent, other));
                        if let Some(slot) = parent.get_mut(a) {
                            *slot = b;
                        }
                    }
                    None => {
                        by_point.insert(k, e);
                    }
                }
            }
        }
        let mut index: HashMap<usize, usize> = HashMap::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for e in 0..self.edges.len() {
            let r = root(&mut parent, e);
            let g = *index.entry(r).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            if let Some(members) = groups.get_mut(g) {
                members.push(e);
            }
        }
        let sample = |members: &[usize]| {
            members.first().and_then(|e| self.edges.get(*e)).map(|e| match *e {
                Edge::Line(a, _) | Edge::Arc { a, .. } => a,
                Edge::Circle(c, r) => c + Vec2::new(r, 0.0),
            })
        };
        // The box round each loop: a ray that misses it can't cross the loop.
        let boxes: Vec<Option<(Vec2, Vec2)>> =
            groups.iter().map(|members| bounds_of(&members.iter().filter_map(|e| self.edges.get(*e)).map(Edge::geom).collect::<Vec<_>>())).collect();
        let around = |p: Vec2, j: usize| {
            let (Some(members), Some(Some((lo, hi)))) = (groups.get(j), boxes.get(j)) else { return false };
            p.y >= lo.y
                && p.y <= hi.y
                && p.x <= hi.x
                && members.iter().filter_map(|e| self.edges.get(*e)).map(|e| e.crossings(p)).sum::<usize>() % 2 == 1
        };
        groups
            .iter()
            .enumerate()
            .map(|(i, members)| {
                let depth = sample(members).map_or(0, |p| (0..groups.len()).filter(|j| *j != i && around(p, *j)).count());
                (members.clone(), depth)
            })
            .collect()
    }

    /// Whether `p` is in the board's material (inside the outline, outside its cut-outs).
    fn inside(&self, p: Vec2) -> bool {
        self.edges.iter().map(|e| e.crossings(p)).sum::<usize>() % 2 == 1
    }

    /// Whether a hole of radius `r` about `c` lies wholly in the board.
    fn fits_hole(&self, c: Vec2, r: f64) -> bool {
        self.inside(c) && self.edges.iter().all(|e| e.dist_to_point(c) > r + CLEAR)
    }

    /// Whether a slot of radius `r` along `a`–`b` lies wholly in the board.
    fn fits_slot(&self, a: Vec2, b: Vec2, r: f64) -> bool {
        self.inside(a) && self.edges.iter().all(|e| e.dist_to_segment(a, b) > r + CLEAR)
    }

    /// Whether a slot of radius `r` along the round tool path `path` lies wholly in the board.
    fn fits_arc(&self, path: &Edge, r: f64) -> bool {
        let start = match (path.ends(), path.round()) {
            (Some((a, _)), _) => a,
            (None, Some((c, radius))) => c + Vec2::new(radius, 0.0),
            (None, None) => return false,
        };
        self.inside(start)
            && self.edges.iter().all(|e| {
                let gap = match *e {
                    Edge::Line(a, b) => path.dist_to_segment(a, b),
                    Edge::Arc { .. } | Edge::Circle(..) => path.dist_to_round(e),
                };
                gap > r + CLEAR
            })
    }
}

/// The holes and slots that go into the board, as sketch geometry.
struct Holes {
    geom: Vec<Geom2>,
    hits: usize,
    /// Vias: holes the drill file marks as vias, or small plated ones (see [`VIA_MAX`]), drawn on
    /// the board's look rather than cut.
    vias: Vec<Geom2>,
    slots: usize,
    skipped: usize,
    /// A point on the wall of each plated hole and straight slot, for its copper colour.
    plated: Vec<Vec2>,
}

impl Holes {
    fn pick(outline: &Outline, drills: &Drills, warnings: &mut Vec<String>) -> Holes {
        let mut out = Holes { geom: Vec::new(), hits: 0, vias: Vec::new(), slots: 0, skipped: 0, plated: Vec::new() };
        let mut seen = HashSet::new();
        let mut repeated = 0;
        let usable = |d: f64, ps: &[Vec2]| d.is_finite() && d > JOIN && ps.iter().all(|p| p.x.is_finite() && p.y.is_finite());
        // A key that is the same for a hole drilled twice at one place.
        let key = |p: Vec2, d: f64| ((p.x / JOIN).round() as i64, (p.y / JOIN).round() as i64, (d / JOIN).round() as i64);
        for h in &drills.hits {
            let r = h.diameter / 2.0;
            if !usable(h.diameter, &[h.at]) {
                warnings.push(format!("skipped a hole at {} whose diameter is not a usable size", at(h.at)));
                out.skipped += 1;
            } else if !seen.insert((key(h.at, h.diameter), None)) {
                repeated += 1;
                out.skipped += 1;
            } else if !outline.fits_hole(h.at, r) {
                warnings.push(format!("skipped the {} mm hole at {}: it is not wholly inside the board", mm(h.diameter), at(h.at)));
                out.skipped += 1;
            } else if h.plated != Some(false) && h.via.unwrap_or(h.diameter <= VIA_MAX) {
                out.vias.push(Geom2::Circle(h.at, r));
            } else {
                out.geom.push(Geom2::Circle(h.at, r));
                out.hits += 1;
                if h.plated == Some(true) {
                    out.plated.push(h.at + Vec2::new(r, 0.0));
                }
            }
        }
        for sl in &drills.slots {
            let r = sl.diameter / 2.0;
            let Some(dir) = (sl.b - sl.a).normalized().filter(|_| sl.a.dist(sl.b) > JOIN) else {
                // No length: a hole.
                if !usable(sl.diameter, &[sl.a]) {
                    warnings.push(format!("skipped a slot at {} whose width is not a usable size", at(sl.a)));
                    out.skipped += 1;
                } else if !seen.insert((key(sl.a, sl.diameter), None)) {
                    repeated += 1;
                    out.skipped += 1;
                } else if !outline.fits_hole(sl.a, r) {
                    warnings.push(format!("skipped the {} mm slot at {}: it is not wholly inside the board", mm(sl.diameter), at(sl.a)));
                    out.skipped += 1;
                } else {
                    out.geom.push(Geom2::Circle(sl.a, r));
                    out.hits += 1;
                    if sl.plated == Some(true) {
                        out.plated.push(sl.a + Vec2::new(r, 0.0));
                    }
                }
                continue;
            };
            if !usable(sl.diameter, &[sl.a, sl.b]) {
                warnings.push(format!("skipped a slot from {} to {} whose width is not a usable size", at(sl.a), at(sl.b)));
                out.skipped += 1;
            } else if !seen.insert((key(sl.a, sl.diameter), Some(key(sl.b, sl.diameter)))) {
                repeated += 1;
                out.skipped += 1;
            } else if !outline.fits_slot(sl.a, sl.b, r) {
                warnings.push(format!(
                    "skipped the {} mm slot from {} to {}: it is not wholly inside the board",
                    mm(sl.diameter),
                    at(sl.a),
                    at(sl.b)
                ));
                out.skipped += 1;
            } else {
                // Two lines along the sides and a half circle round each end, sharing their ends.
                let n = dir.perp() * r;
                out.geom.push(Geom2::Line(sl.a + n, sl.b + n));
                out.geom.push(Geom2::Arc { c: sl.b, a: sl.b - n, b: sl.b + n });
                out.geom.push(Geom2::Line(sl.b - n, sl.a - n));
                out.geom.push(Geom2::Arc { c: sl.a, a: sl.a + n, b: sl.a - n });
                out.slots += 1;
                if sl.plated == Some(true) {
                    out.plated.push((sl.a + sl.b) * 0.5 + n);
                }
            }
        }
        for arc in &drills.arcs {
            let r = arc.diameter / 2.0;
            let outline_geom = if usable(arc.diameter, &[arc.c, arc.a, arc.b]) { arc.outline() } else { Vec::new() };
            match Edge::path(arc.c, arc.a, arc.b).filter(|_| !outline_geom.is_empty()) {
                None => {
                    warnings.push(format!("skipped a curved slot from {} to {} that can't be a slot", at(arc.a), at(arc.b)));
                    out.skipped += 1;
                }
                Some(path) if !outline.fits_arc(&path, r) => {
                    warnings.push(format!(
                        "skipped the {} mm curved slot from {} to {}: it is not wholly inside the board",
                        mm(arc.diameter),
                        at(arc.a),
                        at(arc.b)
                    ));
                    out.skipped += 1;
                }
                Some(_) => {
                    out.geom.extend(outline_geom);
                    out.slots += 1;
                }
            }
        }
        if repeated > 0 {
            warnings.push(format!("{repeated} holes or slots drilled twice at the same place were made once"));
        }
        out
    }
}

#[cfg(test)]
#[path = "board_import_tests.rs"]
mod tests;
