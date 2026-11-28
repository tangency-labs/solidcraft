//! Sketch drawing exchange: Insert DXF, Insert SVG, Insert Airfoil and sketch DXF export.

use solidcraft_geom::prelude::*;
use std::collections::HashMap;

use serde_json::{Value, json};
use solidcore_record::{Seg2, Vec2};
use solidcraft_io::{AirfoilFormat, DxfEntity, DxfLayer, Geom2, MAX_AIRFOIL_BYTES, MAX_DRAWING_BYTES, SvgShape};
use solidcraft_sketch::{Analysis, ConstraintKind, CurveKind, LinkKind, LinkSource, Sketch};

use super::CommandSpec;
use super::sketch::{Left, edit_solved, ids_of};
use super::sketch_store;
use crate::params::{bad, bool_, num, str_, vec2};
use crate::schema::sketch_import::{
    AirfoilInserted, DrawingBoundsParams, DrawingBoundsResult, DrawingBox, DrawingInserted, DxfExported, ExportDxfParams, ExportDxfResult,
    ExportSvgParams, ExportSvgResult, InsertAirfoilParams, InsertDxfParams, InsertSvgParams, SketchSolve, SvgExported,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INSERT_DXF_CONTRACT: TypedContract<InsertDxfParams> = TypedContract::new();
const INSERT_SVG_CONTRACT: TypedContract<InsertSvgParams> = TypedContract::new();
const INSERT_AIRFOIL_CONTRACT: TypedContract<InsertAirfoilParams> = TypedContract::new();
const DRAWING_BOUNDS_CONTRACT: TypedContract<DrawingBoundsParams> = TypedContract::new();
const EXPORT_DXF_CONTRACT: TypedContract<ExportDxfParams> = TypedContract::new();
const EXPORT_SVG_CONTRACT: TypedContract<ExportSvgParams> = TypedContract::new();

/// A command's reply as JSON, built from the type its output schema describes so the two can't
/// drift apart.
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.insert_dxf", "Insert DXF", crate::checked!("sketch.insert_dxf", insert_dxf))
        .at("SKETCH", "INSERT")
        .icon("sketch_dxf")
        .contract(&INSERT_DXF_CONTRACT)
        .output(output_schema_for::<DrawingInserted>)
        .params(concat!("path | text: DXF, ASCII or (by `path`) binary (blocks expanded); at?: [x,y] offset, scale? | width? | height?: mm, the size of the drawn geometry's bounds before rotation, aspect kept (win over scale); angle?: degrees, about the drawing's origin (or its centre); align?: \"origin\" (default: `at` is where the file's origin goes) | \"center\" (`at` is where the drawing's middle goes and it turns and scales about it); tolerance?: mm within which ends join (default 0.0001); simplify?: mm on the drawing as it is drawn, at any scale, 0 to 100 (default 0: every edge as drawn) — runs of lines that meet end to end are replaced by fewer lines that follow them within this distance, so a dense drawing comes in with fewer curves (arcs, circles, splines and text stay as drawn); fixed?: bool (a Fix constraint on every point the drawing adds, as sketch.constraint.fix puts one; a circle's or ellipse's radius stays free; default false); plane?: (when no sketch is active, a new sketch on it; default XY)", created_doc!())),
    CommandSpec::new("sketch.insert_svg", "Insert SVG", crate::checked!("sketch.insert_svg", insert_svg))
        .at("SKETCH", "INSERT")
        .icon("svg")
        .contract(&INSERT_SVG_CONTRACT)
        .output(output_schema_for::<DrawingInserted>)
        .params(concat!("path | text: SVG; at?: [x,y] offset, scale? | width? | height?: mm, the drawing's size before rotation, aspect kept (win over scale); dpi?: the dots per inch the file's pixels are read at, 1 to 10000 (default 96; a file drawn at 72 dpi comes in at its drawn size with 72; lengths with a unit such as mm keep it); size_from?: \"geometry\" (default: width and height are the bounds of the drawn geometry) | \"viewbox\" (the SVG sheet: its viewBox, else its width and height; a drawing smaller than its sheet then comes out smaller than width or height say); angle?: degrees, about the drawing's origin (or its centre); align?: \"origin\" (default: `at` is where the file's origin goes) | \"center\" (`at` is where the drawing's (or sheet's) middle goes and it turns and scales about it); tolerance?: mm within which ends join (default 0.0001); simplify?: mm on the drawing as it is drawn, at any scale, 0 to 100 (default 0: every edge as drawn) — runs of lines that meet end to end are replaced by fewer lines that follow them within this distance, so a dense drawing comes in with fewer curves (arcs, circles, splines and text stay as drawn); fixed?: bool (every point it adds gets a Fix constraint, so the drawing stays where it is put); plane?: (when no sketch is active, a new sketch on it; default XY)", created_doc!())),
    CommandSpec::new("sketch.insert_airfoil", "Insert Airfoil", crate::checked!("sketch.insert_airfoil", insert_airfoil))
        .at("SKETCH", "INSERT")
        .icon("airfoil")
        .contract(&INSERT_AIRFOIL_CONTRACT)
        .output(output_schema_for::<AirfoilInserted>)
        .params(concat!("path | text: airfoil coordinates (.dat, Selig or Lednicer, told apart by content; format?: \"selig\" | \"lednicer\" overrides); chord: mm (> 0); pivot?: share of the chord that `at` places and `angle` turns about (0 leading edge, 0.25 quarter chord, 1 trailing edge; default 0); at?: [x,y] mm (default [0,0]); angle?: degrees, counter-clockwise about the pivot; flip?: bool, mirror across the chord line; tolerance?: mm within which points join (default 0.0001; a trailing edge within it is closed, else a line closes it); plane?: (when no sketch is active, a new sketch on it; default XY); name?: the new sketch's name. One fit point spline through every point; replies with the point count, the trailing edge's gap, the largest deviation of merged points and the largest bulge of the spline between points", created_doc!())),
    CommandSpec::new("sketch.drawing_bounds", "Drawing Size", crate::checked!("sketch.drawing_bounds", drawing_bounds))
        .icon("svg")
        .noundo().read_only()
        .contract(&DRAWING_BOUNDS_CONTRACT)
        .output(output_schema_for::<DrawingBoundsResult>)
        .params("path | text: SVG or DXF (by `format`: \"svg\" | \"dxf\", default by the file name, else SVG); dpi?: SVG only, the dots per inch its pixels are read at, 1 to 10000 (default 96); fit?: bool (also work out the smallest Simplify distance, mm on the drawing to 0.01, at which `sketch.insert_svg`/`_dxf` would add it to a sketch without running out of points or curves — `auto_simplify`: 0 when it fits as drawn, left out when no distance up to the cap does; the window's Auto figures); returns min, max, width and height of the drawn geometry in the file's own size, how many drawn elements it has (`edges`, about what the insert adds curves for), and for an SVG with a viewBox (or width and height) its sheet: {min, max, width, height}"),
    CommandSpec::new("sketch.export_dxf", "Save As DXF", crate::checked!("sketch.export_dxf", export_dxf))
        .icon("sketch_dxf")
        .noundo()
        .contract(&EXPORT_DXF_CONTRACT)
        .output(output_schema_for::<ExportDxfResult>)
        .params("sketch?: id|name (default active); path?: file to write (else the DXF text is returned); units?: \"design\" | \"in\" | \"inch\" | \"mm\" | \"cm\" | \"m\" | \"ft\" (default mm); layer?: profile layer name (default \"0\"); layers?: \"single\" | \"roles\" (default \"single\": all on `layer`; \"roles\": outer on \"OUTLINE\", holes on \"CUT\"); join?: bool (default false: R12 LINE/ARC; true: closed loops as LWPOLYLINE with arc bulges)"),
    CommandSpec::new("sketch.export_svg", "Save As SVG", crate::checked!("sketch.export_svg", export_svg))
        .icon("svg")
        .noundo()
        .contract(&EXPORT_SVG_CONTRACT)
        .output(output_schema_for::<ExportSvgResult>)
        .params("sketch?: id|name (default active); path?: file to write (else the SVG text is returned); units?: \"mm\" | \"cm\" | \"in\" | \"inch\" (default mm). The file's width and height carry the unit and its viewBox is the same size without one, so it opens 1:1; the Y axis points down as SVG has it, so the picture is the sketch as seen. Each closed profile is one closed path with its holes as sub-paths; circles, arcs and lines stay themselves and other curves are line segments; construction curves are left out"),
];

/// The sketch import command for a drawing file name, regardless of extension case.
pub fn drawing_import_command(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("dxf") {
        Some("sketch.insert_dxf")
    } else if ext.eq_ignore_ascii_case("svg") {
        Some("sketch.insert_svg")
    } else {
        None
    }
}

fn source_text(p: &Value, cmd: &str, svg: bool) -> Result<String> {
    if let Some(t) = str_(p, "text") {
        if t.len() > MAX_DRAWING_BYTES {
            return Err(bad(cmd, "the drawing is too large"));
        }
        return Ok(t.to_string());
    }
    let path = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "give `path` or `text`"))?;
    let bytes = solidcraft_io::vfs::read(path, MAX_DRAWING_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if svg {
        return solidcraft_io::decode_svg(&bytes).map_err(|e| drawing_error(p, svg, e.to_string()));
    }
    solidcraft_io::dxf_text_from_bytes(&bytes).map_err(|e| drawing_error(p, svg, e.to_string()))
}

/// A problem with the drawing file's contents (not with the command's parameters), named after
/// the file so the message says which drawing it is about (#9641).
fn drawing_error(p: &Value, svg: bool, msg: String) -> EngineError {
    let name = str_(p, "path")
        .and_then(|x| std::path::Path::new(x).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| if svg { "SVG" } else { "DXF" }.to_string());
    EngineError::Other(format!("{name}: {msg}"))
}

/// Add imported geometry to a sketch, sharing end points that meet.
/// Adds the geometry; end points closer than `q` (mm) become one point, so drawings whose
/// ends almost meet still make closed profiles.
pub(super) fn add_geometry(sk: &mut Sketch, geom: &[Geom2], f: &dyn Fn(Vec2) -> Vec2, k: f64, q: f64) -> Result<(Vec<usize>, Vec<Geom2>)> {
    // A dense drawing adds tens of thousands of points and curves: a batch finds each new id
    // without searching the whole sketch, so the insert takes time in proportion to its size.
    let sk = &mut sk.batch();
    let mut map: HashMap<(i64, i64), Vec<(Vec2, usize)>> = HashMap::new();
    let mut pt = |sk: &mut solidcraft_sketch::SketchBatch, p: Vec2| -> Result<usize> {
        let p = f(p);
        let key = ((p.x / q).floor() as i64, (p.y / q).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = map.get(&(key.0 + dx, key.1 + dy))
                    && let Some((_, i)) = v.iter().find(|(o, _)| o.dist(p) <= q)
                {
                    return Ok(*i);
                }
            }
        }
        let i = sk.add_point(p, None)?;
        map.entry(key).or_default().push((p, i));
        Ok(i)
    };
    let mut curves = Vec::new();
    let mut texts = Vec::new();
    for g in geom {
        let made = match g {
            Geom2::Point(p) => {
                pt(sk, *p)?;
                None
            }
            Geom2::Line(a, b) => {
                let (a, b) = (pt(sk, *a)?, pt(sk, *b)?);
                (a != b).then_some(CurveKind::Line { a, b })
            }
            Geom2::Circle(c, r) => Some(CurveKind::Circle { c: pt(sk, *c)?, r: r * k }),
            Geom2::Arc { c, a, b } => {
                let (c, a, b) = (pt(sk, *c)?, pt(sk, *a)?, pt(sk, *b)?);
                (a != b && c != a).then_some(CurveKind::Arc { c, a, b })
            }
            Geom2::Ellipse { c, major, minor } => Some(CurveKind::Ellipse { c: pt(sk, *c)?, m: pt(sk, *major)?, r: minor * k }),
            Geom2::Spline { pts, control, degree } => {
                let mut idx = Vec::new();
                // A spline past the cap keeps every n-th point and always its last, so a loop stays
                // closed (taking the first ones only cut its tail off, #11119).
                let cap = solidcraft_sketch::MAX_SPLINE_POINTS;
                let keep: Vec<&Vec2> = if pts.len() > cap {
                    (0..cap).map(|k| &pts[(k * (pts.len() - 1) + (cap - 1) / 2) / (cap - 1)]).collect()
                } else {
                    pts.iter().collect()
                };
                for p in keep {
                    let i = pt(sk, *p)?;
                    if idx.last() != Some(&i) {
                        idx.push(i);
                    }
                }
                (idx.len() >= 2).then_some(CurveKind::Spline {
                    pts: idx,
                    control: *control,
                    degree: *degree,
                    periodic: false,
                    nurbs: None,
                    handles: Vec::new(),
                })
            }
            // A spline with its own knots keeps every control point, as the knots count them
            // (#9355): only its two ends join other curves' ends; the points between are its own,
            // even where two of them coincide. One that is no valid sketch spline (too many
            // points for one) is drawn as before, evenly spaced over a share of its points.
            Geom2::Nurbs { pts, degree, knots, weights } => {
                let nurbs = solidcraft_sketch::Nurbs { knots: knots.clone(), weights: weights.clone() };
                match (pts.first(), pts.last()) {
                    (Some(first), Some(last)) if nurbs.check(pts.len(), *degree).is_ok() => {
                        let mut idx = Vec::with_capacity(pts.len());
                        idx.push(pt(sk, *first)?);
                        for p in pts.iter().take(pts.len() - 1).skip(1) {
                            idx.push(sk.add_point(f(*p), None)?);
                        }
                        idx.push(pt(sk, *last)?);
                        Some(CurveKind::Spline { pts: idx, control: true, degree: *degree, periodic: false, nurbs: Some(nurbs), handles: Vec::new() })
                    }
                    _ => {
                        let cap = solidcraft_sketch::MAX_SPLINE_POINTS;
                        let keep: Vec<&Vec2> = if pts.len() > cap {
                            (0..cap).map(|k| &pts[(k * (pts.len() - 1) + (cap - 1) / 2) / (cap - 1)]).collect()
                        } else {
                            pts.iter().collect()
                        };
                        let mut idx = Vec::new();
                        for p in keep {
                            let i = pt(sk, *p)?;
                            if idx.last() != Some(&i) {
                                idx.push(i);
                            }
                        }
                        (idx.len() >= 2).then_some(CurveKind::Spline {
                            pts: idx,
                            control: true,
                            degree: *degree,
                            periodic: false,
                            nurbs: None,
                            handles: Vec::new(),
                        })
                    }
                }
            }
            Geom2::Conic { a, apex, b, rho } => {
                let (a, x, b) = (pt(sk, *a)?, pt(sk, *apex)?, pt(sk, *b)?);
                (a != b && x != a && x != b).then_some(CurveKind::Conic { a, b, apex: x, rho: *rho })
            }
            Geom2::Text { text, at, height, angle } => {
                texts.push(Geom2::Text { text: text.clone(), at: f(*at), height: height * k, angle: *angle });
                None
            }
        };
        if let Some(kind) = made
            && let Ok(c) = sk.add_curve(kind, None)
        {
            curves.push(c);
        }
    }
    Ok((curves, texts))
}

/// The box around a drawing's geometry (arcs, circles and curves by their extent).
pub(super) fn bounds_of(geom: &[Geom2]) -> Option<(Vec2, Vec2)> {
    let mut pts: Vec<Vec2> = Vec::new();
    for g in geom {
        match g {
            Geom2::Point(p) => pts.push(*p),
            Geom2::Line(a, b) => pts.extend([*a, *b]),
            Geom2::Circle(c, r) => pts.extend([*c + Vec2::new(*r, *r), *c - Vec2::new(*r, *r)]),
            Geom2::Arc { c, a, b } => {
                let r = c.dist(*a);
                let (a0, mut a1) = ((*a - *c).angle(), (*b - *c).angle());
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                pts.extend([*a, *b]);
                // The four extreme directions that lie on the arc.
                for q in 0..4 {
                    let mut d = f64::from(q) * std::f64::consts::FRAC_PI_2;
                    while d < a0 {
                        d += std::f64::consts::TAU;
                    }
                    if d < a1 {
                        pts.push(*c + Vec2::from_angle(d) * r);
                    }
                }
            }
            Geom2::Ellipse { c, major, minor } => {
                let r = c.dist(*major).max(*minor);
                pts.extend([*c + Vec2::new(r, r), *c - Vec2::new(r, r)]);
            }
            // A spline lies inside its control points' hull (weights are positive).
            Geom2::Spline { pts: sp, .. } | Geom2::Nurbs { pts: sp, .. } => pts.extend(sp.iter().copied()),
            // Points along the curve, not its apex, which can lie well outside it (an elliptical
            // arc of a DXF comes in as conics, #8155).
            Geom2::Conic { a, apex, b, rho } if *rho > 0.0 && *rho < 1.0 => {
                let seg = solidcore_record::Seg2::Conic { a: *a, apex: *apex, b: *b, w: rho / (1.0 - rho) };
                pts.extend((0..=64).map(|i| seg.point_at(f64::from(i) / 64.0)));
            }
            Geom2::Conic { a, apex, b, .. } => pts.extend([*a, *apex, *b]),
            Geom2::Text { at, .. } => pts.push(*at),
        }
    }
    let first = *pts.first()?;
    let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))));
    (lo.x.is_finite() && lo.y.is_finite() && hi.x.is_finite() && hi.y.is_finite()).then_some((lo, hi))
}

/// A drawing as read: its geometry, the box around it, its text, and what the reading warns
/// about (an SVG cubic kept as drawn that the curve check refuses, #16190).
type Drawing = (Vec<Geom2>, (Vec2, Vec2), String, Vec<String>);

/// The drawing's geometry and the box around it; a drawing with none is an error that says why
/// (#9641).
/// The dots per inch an SVG's pixels are read at: `dpi`, else CSS's 96 (#20240).
fn dpi_of(p: &Value, cmd: &str, svg: bool) -> Result<f64> {
    let Some(v) = p.get("dpi").filter(|v| !v.is_null()) else { return Ok(solidcraft_io::SVG_DEFAULT_DPI) };
    if !svg {
        return Err(bad(cmd, "`dpi` is for SVG drawings; a DXF has no pixels"));
    }
    v.as_f64().filter(|d| (1.0..=10000.0).contains(d)).ok_or_else(|| bad(cmd, "`dpi` must be a number from 1 to 10000 (dots per inch)"))
}

fn read_drawing(p: &Value, cmd: &str, svg: bool) -> Result<Drawing> {
    let text = source_text(p, cmd, svg)?;
    let dpi = dpi_of(p, cmd, svg)?;
    let (geom, warnings) = if svg {
        solidcraft_io::read_svg_drawing_at(&text, dpi).map(|d| (d.geometry, d.warnings.iter().map(ToString::to_string).collect()))
    } else {
        solidcraft_io::read_dxf(&text).map(|g| (g, Vec::new()))
    }
    .map_err(|e| drawing_error(p, svg, e.to_string()))?;
    let Some(bounds) = bounds_of(&geom) else {
        let why = if svg { solidcraft_io::svg_nothing_to_import(&text) } else { "the DXF has no entities SolidCraft reads".to_string() };
        return Err(drawing_error(p, svg, why));
    };
    Ok((geom, bounds, text, warnings))
}

/// The box around a drawing, as the reply describes it: `min`, `max` and the span between them.
fn drawing_box((lo, hi): (Vec2, Vec2)) -> DrawingBox {
    DrawingBox { min: [lo.x, lo.y], max: [hi.x, hi.y], width: hi.x - lo.x, height: hi.y - lo.y }
}

/// How big a drawing is in its own file, to size it before it is inserted.
fn drawing_bounds(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.drawing_bounds";
    let name_is_dxf = str_(p, "path").is_some_and(|x| x.to_ascii_lowercase().ends_with(".dxf"));
    let svg = match str_(p, "format") {
        Some(f) => !f.eq_ignore_ascii_case("dxf"),
        None => !name_is_dxf,
    };
    let (geom, bounds, text, _) = read_drawing(p, cmd, svg)?;
    // Only an SVG names a sheet, and only when it says how big its sheet is.
    let dpi = dpi_of(p, cmd, svg)?;
    let sheet = svg.then(|| solidcraft_io::svg_sheet_at(&text, dpi)).flatten().map(drawing_box);
    // The window's Auto figure: what it takes to fit a sketch, worked out here on the same read.
    let auto_simplify = if bool_(p, "fit").unwrap_or(false) {
        let span = (bounds.1.x - bounds.0.x).max(bounds.1.y - bounds.0.y);
        fit_simplify(&geom, span)
    } else {
        None
    };
    reply(cmd, DrawingBoundsResult { drawing: drawing_box(bounds), sheet, edges: geom.len(), auto_simplify, param_warnings: Vec::new() })
}

/// The most a drawing may be simplified by, in mm: `simplify` is brought inside this.
const MAX_SIMPLIFY_MM: f64 = 100.0;

/// The join tolerance the insert commands use when the caller gives none, in mm of the drawing's
/// place.
const DEFAULT_TOLERANCE_MM: f64 = 1e-4;

/// The smallest Simplify distance (mm on the drawing, to 0.01) at which the insert would add
/// `geom` to a sketch on a scratch model: `Some(0.0)` when it fits as drawn, `None` when no
/// distance up to the cap (a hundredth of its span, and [`MAX_SIMPLIFY_MM`]) does. Counted as
/// `sketch.insert_svg` adds it, so a drawing whose curves the sketch cannot take as drawn (a
/// field of splines the simplify distance leaves as drawn) says none fits rather than guessing.
fn fit_simplify(geom: &[Geom2], span: f64) -> Option<f64> {
    let cap = MAX_SIMPLIFY_MM.min(span * 0.01);
    if !(cap > 0.0 && cap.is_finite()) {
        return None;
    }
    let fits = |d: f64| {
        // As drawn is a borrow: a drawing too large for the sketch is refused before it is
        // copied, which is the common case for a window that asks.
        let cut: std::borrow::Cow<[Geom2]> =
            if d > 0.0 { std::borrow::Cow::Owned(simplify_lines(geom, d, DEFAULT_TOLERANCE_MM).0) } else { std::borrow::Cow::Borrowed(geom) };
        if cut.len() > solidcraft_sketch::limits::max_points() {
            return false;
        }
        let mut sk = Sketch::new();
        add_geometry(&mut sk, cut.as_ref(), &|q: Vec2| q, 1.0, DEFAULT_TOLERANCE_MM).is_ok()
    };
    if fits(0.0) {
        return Some(0.0);
    }
    if !fits(cap) {
        return None;
    }
    // Bisect for the smallest distance that fits, on the 0.01 mm the window's box steps in.
    let (mut no, mut yes) = (0.0_f64, cap);
    for _ in 0..40 {
        if yes - no <= 0.005 {
            break;
        }
        let mid = ((no + yes) / 2.0 * 100.0).ceil() / 100.0;
        if mid <= no || mid >= yes {
            break;
        }
        if fits(mid) {
            yes = mid;
        } else {
            no = mid;
        }
    }
    Some(if cap >= 0.01 { (yes * 100.0).ceil() / 100.0 } else { yes })
}

/// Why a drawing was not simplified as far as asked (a run too complex for the work budget):
/// it is inserted as drawn, never coarser than the tolerance allows.
fn simplify_short_note() -> &'static str {
    "part of the drawing has too many edges to simplify within the time allowed; that part was inserted as drawn"
}

/// `geom` with runs of consecutive lines that meet end to end replaced by fewer lines following
/// the same outline within `tol` (file units): no removed vertex lies further than `tol` from the
/// lines kept, so no point of the outline moves by more than that. Every other entity is left
/// as it is, and ends a run. The second value is false when the work budget ran out, when the
/// run is kept as drawn.
fn simplify_lines(geom: &[Geom2], tol: f64, join: f64) -> (Vec<Geom2>, bool) {
    let mut out: Vec<Geom2> = Vec::with_capacity(geom.len());
    if !(tol > 0.0 && tol.is_finite()) {
        out.extend_from_slice(geom);
        return (out, true);
    }
    let mut complete = true;
    let mut run: Vec<Vec2> = Vec::new();
    let flush = |run: &mut Vec<Vec2>, out: &mut Vec<Geom2>, complete: &mut bool| {
        if run.len() >= 2 {
            let (kept, done) = douglas_peucker(run, tol);
            *complete &= done;
            for pair in kept.windows(2) {
                out.push(Geom2::Line(pair[0], pair[1]));
            }
        }
        run.clear();
    };
    for item in geom {
        match item {
            Geom2::Line(a, b) => {
                match run.last() {
                    // The run goes on through the vertex shared with the line before it.
                    Some(end) if end.dist(*a) <= join => {}
                    // A new run: the line before it ended elsewhere (or there is none yet).
                    None => {}
                    Some(_) => flush(&mut run, &mut out, &mut complete),
                }
                if run.is_empty() {
                    run.push(*a);
                }
                run.push(*b);
            }
            other => {
                flush(&mut run, &mut out, &mut complete);
                out.push(other.clone());
            }
        }
    }
    flush(&mut run, &mut out, &mut complete);
    (out, complete)
}

/// The points of `points` that keep their outline within `tol`: Douglas-Peucker on a stack (no
/// recursion, so a drawing of a million vertices cannot overflow it), with a work budget so one
/// cannot hold an import for long. Its ends are always kept. When the budget runs out, the run
/// is kept as drawn (never a coarser outline than asked for) and the second value is false.
fn douglas_peucker(points: &[Vec2], tol: f64) -> (Vec<Vec2>, bool) {
    let n = points.len();
    if n <= 2 {
        return (points.to_vec(), true);
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    let budget = n.saturating_mul(64).saturating_add(100_000);
    let mut work = 0usize;
    let mut complete = true;
    while let Some((i, j)) = stack.pop() {
        if j <= i + 1 {
            continue;
        }
        let (a, b) = (points[i], points[j]);
        let mut farthest = 0.0;
        let mut at = i;
        for (k, q) in points.iter().enumerate().take(j).skip(i + 1) {
            work += 1;
            let d = point_segment_distance(*q, a, b);
            if d > farthest {
                farthest = d;
                at = k;
            }
        }
        if farthest > tol {
            keep[at] = true;
            stack.push((i, at));
            stack.push((at, j));
        }
        if work > budget {
            keep.iter_mut().for_each(|k| *k = true);
            complete = false;
            break;
        }
    }
    let kept: Vec<Vec2> = points.iter().zip(&keep).filter(|(_, kept)| **kept).map(|(p, _)| *p).collect();
    (kept, complete)
}

/// The distance from `p` to the segment `a`–`b` (its nearest point, not its line's): the
/// projection and the nearest point are the geometry layer's ([`Seg2Geom::closest_param`]).
fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let segment = Seg2::Line { a, b };
    segment.point_at(segment.closest_param(p)).dist(p)
}

fn insert(s: &mut Session, p: &Value, cmd: &str, svg: bool) -> Result<Value> {
    let (geom, (lo, hi), text, mut warnings) = read_drawing(p, cmd, svg)?;
    // The drawing's own span, for `simplify`'s cap: `size_from` below may replace `lo` and `hi`
    // with the SVG's sheet, which is not what is simplified.
    let drawing_span = (hi.x - lo.x).max(hi.y - lo.y);
    // What `width`, `height` and `align: center` refer to: the drawn geometry, or the SVG sheet.
    let (lo, hi) = match str_(p, "size_from") {
        None | Some("geometry") => (lo, hi),
        Some("viewbox") if !svg => return Err(bad(cmd, "`size_from: viewbox` is for SVG drawings; a DXF has no sheet")),
        Some("viewbox") => solidcraft_io::svg_sheet_at(&text, dpi_of(p, cmd, svg)?)
            .ok_or_else(|| bad(cmd, "the SVG has no viewBox or width and height to size by"))?,
        Some(_) => return Err(bad(cmd, "`size_from` must be \"geometry\" or \"viewbox\"")),
    };
    // A size in millimetres wins over a factor; the aspect ratio is always kept.
    let natural = (hi.x - lo.x, hi.y - lo.y);
    let sized = |key: &str, of: fn(&(f64, f64)) -> f64| -> Result<Option<f64>> {
        let Some(v) = p.get(key) else { return Ok(None) };
        let want = v.as_f64().filter(|w| w.is_finite() && *w > 0.0).ok_or_else(|| bad(cmd, format!("`{key}` must be a positive number (mm)")))?;
        let have = Some(of(&natural)).filter(|h| *h > 1e-9).ok_or_else(|| bad(cmd, format!("the drawing has no {key} to size")))?;
        Ok(Some(want / have))
    };
    let from_size = match sized("width", |n| n.0)? {
        Some(k) => Some(k),
        None => sized("height", |n| n.1)?,
    };
    let k = from_size.or_else(|| num(p, "scale")).filter(|k| *k > 1e-9 && *k < 1e6).unwrap_or(1.0);
    let angle = match p.get("angle") {
        Some(v) => v.as_f64().filter(|a| a.is_finite()).ok_or_else(|| bad(cmd, "`angle` must be a number (degrees)"))?.to_radians(),
        None => 0.0,
    };
    let pivot = match str_(p, "align") {
        None | Some("origin") => Vec2::ZERO,
        Some("center") => (lo + hi) * 0.5,
        Some(_) => return Err(bad(cmd, "`align` must be \"origin\" or \"center\"")),
    };
    let off = p.get("at").and_then(vec2).unwrap_or(Vec2::ZERO);
    let mut created = None;
    if s.active_sketch.is_none() {
        let mut cp = json!({"plane": p.get("plane").cloned().unwrap_or(json!("XY")), "project_edges": false});
        if let Some(n) = str_(p, "name") {
            cp["name"] = json!(n);
        }
        let r = (super::find_command("sketch.create").ok_or_else(|| bad(cmd, "sketch.create"))?.run)(s, &cp)?;
        created = r.get("sketch").and_then(Value::as_u64);
    }
    let tol = num(p, "tolerance").filter(|t| t.is_finite() && *t > 0.0).unwrap_or(1e-4).clamp(1e-9, 10.0);
    let f = placement(pivot, Vec2::new(k, k), angle, off);
    // `simplify` is measured on the drawing as it is drawn, so it means the same at any size
    // factor: the preview, imported at scale 1, then simplifies exactly as the confirmed command
    // does. It is never more than a hundredth of the drawing's own span, so a drawing cannot be
    // simplified away altogether.
    let simplify = num(p, "simplify").filter(|s| *s > 0.0).map(|s| s.clamp(0.0, MAX_SIMPLIFY_MM)).unwrap_or(0.0);
    let geom = if simplify > 0.0 {
        let tol_file = simplify.min(drawing_span * 0.01);
        let (simplified, complete) = simplify_lines(&geom, tol_file, tol / k);
        if !complete {
            warnings.push(simplify_short_note().into());
        }
        simplified
    } else {
        geom
    };
    // A dense SVG/DXF (a floorplan) can yield far more entities than a sketch takes; refuse
    // before adding so the message names the cap and what reduces it (#10874). Scale alone does
    // not shrink the count; `simplify` does.
    let limit = solidcraft_sketch::limits::max_points();
    if geom.len() > limit {
        // Not a parameter's fault: the drawing itself is too dense (and `simplify`, a parameter,
        // is what brings it under the cap), so the message stands on its own.
        return Err(EngineError::Other(format!(
            "the drawing has {} curves, and a sketch takes at most {limit} points or curves: insert it with `simplify` (mm, the Simplify field) so the drawing is followed with fewer edges, split it across sketches or raise Preferences > Sketch > Largest sketch (up to {}); changing scale does not reduce how many curves it has",
            geom.len(),
            solidcraft_sketch::limits::CEILING_POINTS
        )));
    }
    let fixed = bool_(p, "fixed").unwrap_or(false);
    // `sketch::edit`, but keeping the solve as a typed value for the reply.
    let target = sketch_store::target(s, p, cmd)?;
    let ((curves, texts, n_fixed), report) = edit_solved(s, target, false, Analysis::Freedom, |sk, _| {
        let first = sk.points.len();
        let (cs, texts) = add_geometry(sk, &geom, &f, k, tol)?;
        // Fixed where it is put: a Fix constraint on every point the drawing added (a circle's
        // or ellipse's radius stays free). Refused as a whole when the constraints would not fit.
        let mut n_fixed = 0;
        if fixed {
            let limit = solidcraft_sketch::limits::max_constraints();
            if sk.constraints.len() + (sk.points.len() - first) > limit {
                return Err(bad(cmd, format!("fixing the drawing's points needs more than the sketch's {limit} constraints")));
            }
            for q in first..sk.points.len() {
                sk.add_constraint(ConstraintKind::Fix { p: q }, None)?;
                n_fixed += 1;
            }
        }
        Ok(((ids_of(sk, &cs), texts, n_fixed), Left::Unsolved))
    })?;
    let info = report.map(|rep| SketchSolve { solved: rep.keeps_orientation(), dof: rep.dof });
    // Text becomes sketch text.
    let id = sketch_store::active(s).ok_or_else(|| bad(cmd, "no sketch"))?;
    let mut n_text = 0;
    for t in texts {
        if let Geom2::Text { text, at, height, angle: own } = t {
            // Not `sketch_store::update`: a text that can't be linked is skipped, not refused.
            let mut doc = (*s.doc).clone();
            let mut sk = sketch_store::read(&doc, id)?.clone();
            if super::sketch_project::add_link(
                s,
                &doc,
                id,
                &mut sk,
                LinkKind::Text,
                LinkSource::Text { text, at, height, angle: own + angle, font: None, style: Default::default(), exprs: Default::default() },
                None,
            )
            .is_ok()
            {
                sketch_store::write(&mut doc, id, sk)?;
                *s.doc_mut() = doc;
                n_text += 1;
            }
        }
    }
    reply(
        cmd,
        DrawingInserted {
            sketch: id,
            created_sketch: created,
            curves: curves.len(),
            texts: n_text,
            fixed: fixed.then_some(n_fixed),
            info,
            created: None,
            warnings,
            param_warnings: Vec::new(),
        },
    )
}

fn insert_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    insert(s, p, "sketch.insert_dxf", false)
}

fn insert_svg(s: &mut Session, p: &Value) -> Result<Value> {
    insert(s, p, "sketch.insert_svg", true)
}

/// The airfoil file's text, from `text` or the file at `path`.
fn airfoil_text(p: &Value, cmd: &str) -> Result<String> {
    if let Some(t) = str_(p, "text") {
        if t.len() > MAX_AIRFOIL_BYTES {
            return Err(bad(cmd, "the airfoil file is too large"));
        }
        return Ok(t.to_string());
    }
    let path = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "give `path` or `text`"))?;
    let bytes = solidcraft_io::vfs::read(path, MAX_AIRFOIL_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// A problem with the airfoil file's contents, named after the file (or "airfoil" for text).
fn airfoil_error(p: &Value, msg: String) -> EngineError {
    let name = str_(p, "path")
        .and_then(|x| std::path::Path::new(x).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "airfoil".to_string());
    EngineError::Other(format!("{name}: {msg}"))
}

/// Where a point of an inserted file lands in the sketch: moved so `pivot` is at the origin,
/// scaled per axis by `scale` (a negative `scale.y` mirrors), turned by `angle` (radians,
/// counter-clockwise) and placed at `offset`. The one placement of every insert here.
fn placement(pivot: Vec2, scale: Vec2, angle: f64, offset: Vec2) -> impl Fn(Vec2) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    move |q: Vec2| {
        let v = Vec2::new((q.x - pivot.x) * scale.x, (q.y - pivot.y) * scale.y);
        Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos) + offset
    }
}

/// Insert an airfoil coordinate file as one fit point spline through its points, at the chord
/// given, with a line across an open trailing edge (#14202).
fn insert_airfoil(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.insert_airfoil";
    let text = airfoil_text(p, cmd)?;
    let chord = p
        .get("chord")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c > 0.0)
        .ok_or_else(|| bad(cmd, "`chord` must be a positive number (mm)"))?;
    let pivot = match p.get("pivot") {
        Some(v) => v.as_f64().filter(|a| a.is_finite()).ok_or_else(|| bad(cmd, "`pivot` must be a number (a share of the chord)"))?,
        None => 0.0,
    };
    let angle = match p.get("angle") {
        Some(v) => v.as_f64().filter(|a| a.is_finite()).ok_or_else(|| bad(cmd, "`angle` must be a number (degrees)"))?.to_radians(),
        None => 0.0,
    };
    let flip = match p.get("flip").filter(|v| !v.is_null()) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`flip` must be true or false")),
    };
    let format = match p.get("format").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => Some(v.as_str().and_then(AirfoilFormat::parse).ok_or_else(|| bad(cmd, "`format` must be \"selig\" or \"lednicer\""))?),
    };
    let tol = num(p, "tolerance").filter(|t| t.is_finite() && *t > 0.0).unwrap_or(1e-4).clamp(1e-9, 10.0);
    let off = p.get("at").and_then(vec2).unwrap_or(Vec2::ZERO);
    let airfoil = solidcraft_io::read_airfoil(&text, format).map_err(|e| airfoil_error(p, e.to_string()))?;
    if airfoil.points.len() > solidcraft_sketch::MAX_SPLINE_POINTS {
        return Err(airfoil_error(p, format!("{} points; a spline takes at most {}", airfoil.points.len(), solidcraft_sketch::MAX_SPLINE_POINTS)));
    }
    // Chord units to the sketch: scaled to the chord, mirrored when asked, turned about the pivot
    // and placed.
    let place = placement(Vec2::new(pivot, 0.0), Vec2::new(chord, if flip { -chord } else { chord }), angle, off);
    let file_points: Vec<Vec2> = airfoil.points.iter().map(|q| place(*q)).collect();
    // Points within the tolerance of the one before become one sketch point, as `add_geometry`
    // would make them; doing it here says how many, and leaves the spline's points known.
    let mut spline: Vec<Vec2> = Vec::with_capacity(file_points.len());
    // Each merged point, with the index of the spline point that stands for it.
    let mut near: Vec<(usize, [f64; 3])> = Vec::new();
    for q in &file_points {
        if spline.last().is_some_and(|k| k.dist(*q) <= tol) {
            near.push((spline.len() - 1, [q.x, q.y, 0.0]));
        } else {
            spline.push(*q);
        }
    }
    let merged = near.len();
    let (Some(first), Some(last)) = (spline.first().copied(), spline.last().copied()) else {
        return Err(airfoil_error(p, "no points".into()));
    };
    let te_gap = first.dist(last);
    let closed = te_gap <= tol;
    if closed {
        // The spline ends where it starts: a sharp trailing edge, not a periodic (rounded) one.
        spline.pop();
        spline.push(first);
        if last != first {
            near.push((spline.len() - 1, [last.x, last.y, 0.0]));
        }
    }
    if spline.len() < 4 {
        return Err(airfoil_error(p, format!("{} points left within a tolerance of {tol} mm; an airfoil needs at least four", spline.len())));
    }
    let mut geom = vec![Geom2::Spline { pts: spline.clone(), control: false, degree: 3 }];
    if !closed {
        geom.push(Geom2::Line(last, first));
    }
    // The merged points against the spline points that stand for them, and the spline against
    // the straight lines between its points: the kernel bounds both.
    let pieces: Vec<[[f64; 3]; 4]> = solidcraft_sketch::fit_beziers(&spline).iter().map(|piece| piece.map(|c| [c.x, c.y, 0.0])).collect();
    let fit = solidcraft_kernel::curves::fit_spline_bounds(&pieces, &near).map_err(|e| airfoil_error(p, e.to_string()))?;

    let mut created = None;
    if s.active_sketch.is_none() {
        let mut cp = json!({"plane": p.get("plane").cloned().unwrap_or(json!("XY")), "project_edges": false});
        if let Some(n) = str_(p, "name") {
            cp["name"] = json!(n);
        }
        let r = (super::find_command("sketch.create").ok_or_else(|| bad(cmd, "sketch.create"))?.run)(s, &cp)?;
        created = r.get("sketch").and_then(Value::as_u64);
    }
    let target = sketch_store::target(s, p, cmd)?;
    let (curves, report) = edit_solved(s, target, false, Analysis::Freedom, |sk, _| {
        let (cs, _) = add_geometry(sk, &geom, &|q| q, 1.0, tol)?;
        Ok((ids_of(sk, &cs), Left::Unsolved))
    })?;
    let info = report.map(|rep| SketchSolve { solved: rep.keeps_orientation(), dof: rep.dof });
    reply(
        cmd,
        AirfoilInserted {
            sketch: target,
            created_sketch: created,
            curves: curves.len(),
            name: airfoil.name,
            format: airfoil.format.name().to_string(),
            points: airfoil.points.len(),
            merged,
            trailing_edge: if closed { "closed" } else { "open" }.to_string(),
            te_gap,
            deviation: fit.deviation,
            bulge: fit.bulge,
            info,
            created: None,
            param_warnings: Vec::new(),
        },
    )
}

fn sorted_curve_ids<'a>(ids: &'a [String]) -> Vec<&'a str> {
    let mut v: Vec<&'a str> = ids.iter().map(String::as_str).collect();
    v.sort_unstable();
    v
}

fn export_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.export_dxf";
    let id = sketch_store::target(s, p, cmd)?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
    let sk = &ss.sketch;

    let units_val = p.get("units").filter(|v| !v.is_null());
    if let Some(v) = units_val
        && !v.is_string()
    {
        return Err(bad(cmd, "`units` must be a string"));
    }
    let units_param = str_(p, "units").map(str::to_ascii_lowercase);
    let (insunits, scale) = match units_param.as_deref() {
        None => (4, 1.0),
        Some("design") => {
            let doc_units = solidcraft_doc::expr::length_unit_or_mm(&s.doc.units);
            match doc_units.to_ascii_lowercase().as_str() {
                "in" | "inch" => (1, 1.0 / 25.4),
                "ft" => (2, 1.0 / 304.8),
                "cm" => (5, 0.1),
                "m" => (6, 0.001),
                _ => (4, 1.0),
            }
        }
        Some("in" | "inch") => (1, 1.0 / 25.4),
        Some("ft") => (2, 1.0 / 304.8),
        Some("mm") => (4, 1.0),
        Some("cm") => (5, 0.1),
        Some("m") => (6, 0.001),
        Some(other) => {
            return Err(bad(cmd, format!("unsupported unit `{other}` (use \"design\", \"mm\", \"in\", \"cm\", \"m\", or \"ft\")")));
        }
    };

    let layer_val = p.get("layer").filter(|v| !v.is_null());
    if let Some(v) = layer_val
        && !v.is_string()
    {
        return Err(bad(cmd, "`layer` must be a string"));
    }
    let custom_layer = str_(p, "layer").map(str::trim).filter(|s| !s.is_empty());

    let layers_val = p.get("layers").filter(|v| !v.is_null());
    if let Some(v) = layers_val
        && !v.is_string()
    {
        return Err(bad(cmd, "`layers` must be \"single\" or \"roles\""));
    }
    let roles_layers = match str_(p, "layers").map(str::to_ascii_lowercase).as_deref() {
        None | Some("single") => false,
        Some("roles") => true,
        Some(_) => return Err(bad(cmd, "`layers` must be \"single\" or \"roles\"")),
    };

    let join = match p.get("join").filter(|v| !v.is_null()) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`join` must be true or false")),
    };

    let path_val = p.get("path").filter(|v| !v.is_null());
    if let Some(v) = path_val
        && !v.is_string()
    {
        return Err(bad(cmd, "`path` must be a string"));
    }

    let is_default = units_val.is_none() && layer_val.is_none() && layers_val.is_none() && !join;

    let (text, entity_count) = if is_default {
        let mut geom = Vec::new();
        for (i, c) in sk.curves.iter().enumerate() {
            if c.construction {
                continue;
            }
            let pt = |k: usize| sk.point(k).unwrap_or_default();
            match &c.kind {
                CurveKind::Line { a, b } => geom.push(Geom2::Line(pt(*a), pt(*b))),
                CurveKind::Circle { c, r } => geom.push(Geom2::Circle(pt(*c), *r)),
                CurveKind::Arc { c, a, b } => geom.push(Geom2::Arc { c: pt(*c), a: pt(*a), b: pt(*b) }),
                _ => {
                    let poly = sk.polyline(i);
                    geom.extend(poly.windows(2).map(|w| Geom2::Line(w[0], w[1])));
                }
            }
        }
        let count = geom.len();
        (solidcraft_io::write_dxf(&geom), count)
    } else if !join {
        let hole_keys: std::collections::HashSet<Vec<&str>> =
            ss.profiles.iter().flat_map(|p| p.hole_curves.iter().map(|h| sorted_curve_ids(h))).collect();

        let mut outline_ids = std::collections::HashSet::new();
        let mut cut_ids = std::collections::HashSet::new();
        if roles_layers {
            for prof in &ss.profiles {
                for holes in &prof.hole_curves {
                    for id in holes {
                        cut_ids.insert(id.as_str());
                    }
                }
                if !hole_keys.contains(&sorted_curve_ids(&prof.outer_curves)) {
                    for id in &prof.outer_curves {
                        outline_ids.insert(id.as_str());
                    }
                }
            }
        }
        let base_layer = custom_layer.unwrap_or("0");
        let mut entities = Vec::new();
        for (i, c) in sk.curves.iter().enumerate() {
            if c.construction {
                continue;
            }
            let layer = if roles_layers {
                if cut_ids.contains(c.id.as_str()) {
                    "CUT"
                } else if outline_ids.contains(c.id.as_str()) {
                    "OUTLINE"
                } else {
                    base_layer
                }
            } else {
                base_layer
            };
            let pt = |k: usize| sk.point(k).unwrap_or_default() * scale;
            match &c.kind {
                CurveKind::Line { a, b } => entities.push((layer, Geom2::Line(pt(*a), pt(*b)))),
                CurveKind::Circle { c, r } => entities.push((layer, Geom2::Circle(pt(*c), *r * scale))),
                CurveKind::Arc { c, a, b } => entities.push((layer, Geom2::Arc { c: pt(*c), a: pt(*a), b: pt(*b) })),
                _ => {
                    let poly = sk.polyline(i);
                    for w in poly.windows(2) {
                        entities.push((layer, Geom2::Line(w[0] * scale, w[1] * scale)));
                    }
                }
            }
        }
        let count = entities.len();
        (solidcraft_io::write_dxf_entities(&entities, insunits), count)
    } else {
        // join: true -> emit closed loops as LWPOLYLINE with bulges for arcs
        let hole_keys: std::collections::HashSet<Vec<&str>> =
            ss.profiles.iter().flat_map(|p| p.hole_curves.iter().map(|h| sorted_curve_ids(h))).collect();

        let mut outline_ids = std::collections::HashSet::new();
        let mut cut_ids = std::collections::HashSet::new();
        for prof in &ss.profiles {
            for id in &prof.outer_curves {
                outline_ids.insert(id.as_str());
            }
            for holes in &prof.hole_curves {
                for id in holes {
                    cut_ids.insert(id.as_str());
                }
            }
        }
        let base_layer = custom_layer.unwrap_or("0");
        let mut entities = Vec::new();
        for prof in &ss.profiles {
            if hole_keys.contains(&sorted_curve_ids(&prof.outer_curves)) {
                continue;
            }
            let outer_layer = if roles_layers { "OUTLINE" } else { base_layer };
            let shape = solidcraft_io::loop_lwpolyline_scaled(&prof.region.outer, scale);
            entities.push(DxfEntity { layer: outer_layer.into(), shape });

            for hole in &prof.region.holes {
                let hole_layer = if roles_layers { "CUT" } else { base_layer };
                let shape = solidcraft_io::loop_lwpolyline_scaled(hole, scale);
                entities.push(DxfEntity { layer: hole_layer.into(), shape });
            }
        }
        // Preserve any non-profile open curves as disconnected entities so nothing is lost
        for (i, c) in sk.curves.iter().enumerate() {
            if c.construction || outline_ids.contains(c.id.as_str()) || cut_ids.contains(c.id.as_str()) {
                continue;
            }
            let pt = |k: usize| sk.point(k).unwrap_or_default() * scale;
            match &c.kind {
                CurveKind::Line { a, b } => {
                    entities.push(DxfEntity { layer: base_layer.into(), shape: solidcraft_io::DxfShape::Line(pt(*a), pt(*b)) })
                }
                CurveKind::Circle { c, r } => {
                    entities.push(DxfEntity { layer: base_layer.into(), shape: solidcraft_io::DxfShape::Circle { c: pt(*c), r: *r * scale } })
                }
                CurveKind::Arc { c, a, b } => {
                    let center = pt(*c);
                    let pa = pt(*a);
                    let pb = pt(*b);
                    let r = center.dist(pa);
                    let a0 = (pa - center).angle().to_degrees().rem_euclid(360.0);
                    let a1 = (pb - center).angle().to_degrees().rem_euclid(360.0);
                    entities.push(DxfEntity { layer: base_layer.into(), shape: solidcraft_io::DxfShape::Arc { c: center, r, a0, a1 } });
                }
                _ => {
                    let poly = sk.polyline(i);
                    let points: Vec<Vec2> = poly.into_iter().map(|p| p * scale).collect();
                    entities.push(DxfEntity { layer: base_layer.into(), shape: solidcraft_io::DxfShape::Polyline { points } });
                }
            }
        }
        let mut layer_names = std::collections::BTreeSet::new();
        if roles_layers {
            layer_names.insert("OUTLINE".to_string());
            layer_names.insert("CUT".to_string());
        }
        for e in &entities {
            layer_names.insert(e.layer.clone());
        }
        let layers: Vec<DxfLayer> = layer_names
            .iter()
            .map(|name| match name.as_str() {
                "OUTLINE" => DxfLayer::new(name, 7),
                "CUT" => DxfLayer::new(name, 4),
                _ => DxfLayer::new(name, 7),
            })
            .collect();
        let count = entities.len();
        (solidcraft_io::write_dxf_drawing_units(&layers, &entities, insunits)?, count)
    };

    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        let exported = DxfExported::Written { path: path.to_string(), entities: entity_count };
        return reply(cmd, ExportDxfResult { exported, param_warnings: Vec::new() });
    }
    reply(cmd, ExportDxfResult { exported: DxfExported::Text { dxf: text, entities: entity_count }, param_warnings: Vec::new() })
}

fn export_svg(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.export_svg";
    let id = sketch_store::target(s, p, cmd)?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
    let sk = &ss.sketch;

    if let Some(v) = p.get("units").filter(|v| !v.is_null())
        && !v.is_string()
    {
        return Err(bad(cmd, "`units` must be a string"));
    }
    let (unit, per_mm) = match str_(p, "units") {
        None => ("mm", 1.0),
        Some(name) => solidcraft_io::svg_unit(name).ok_or_else(|| bad(cmd, format!("unsupported unit `{name}` (use \"mm\", \"cm\" or \"in\")")))?,
    };
    if let Some(v) = p.get("path").filter(|v| !v.is_null())
        && !v.is_string()
    {
        return Err(bad(cmd, "`path` must be a string"));
    }

    // A profile is drawn as one closed contour with its holes. A profile that fills another's hole
    // is that hole's inside, not a second outline, so it is left out as `sketch.export_dxf` leaves it.
    let hole_keys: std::collections::HashSet<Vec<&str>> =
        ss.profiles.iter().flat_map(|p| p.hole_curves.iter().map(|h| sorted_curve_ids(h))).collect();
    let mut in_profile = std::collections::HashSet::new();
    let mut shapes = Vec::new();
    for prof in &ss.profiles {
        for id in prof.outer_curves.iter().chain(prof.hole_curves.iter().flatten()) {
            in_profile.insert(id.as_str());
        }
        if !hole_keys.contains(&sorted_curve_ids(&prof.outer_curves)) {
            shapes.push(SvgShape::Region { outer: &prof.region.outer, holes: prof.region.holes.iter().collect() });
        }
    }
    // Whatever is not part of a profile is drawn on its own, so nothing visible is lost.
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction || in_profile.contains(c.id.as_str()) {
            continue;
        }
        let pt = |k: usize| sk.point(k).unwrap_or_default();
        shapes.push(match &c.kind {
            CurveKind::Line { a, b } => SvgShape::Line(pt(*a), pt(*b)),
            CurveKind::Circle { c, r } => SvgShape::Circle(pt(*c), *r),
            CurveKind::Arc { c, a, b } => SvgShape::Arc { c: pt(*c), a: pt(*a), b: pt(*b) },
            _ => SvgShape::Polyline(sk.polyline(i)),
        });
    }
    let elements = shapes.len();
    let text = solidcraft_io::write_svg(&shapes, unit, per_mm).map_err(|e| bad(cmd, e.to_string()))?;

    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        let exported = SvgExported::Written { path: path.to_string(), elements };
        return reply(cmd, ExportSvgResult { exported, param_warnings: Vec::new() });
    }
    reply(cmd, ExportSvgResult { exported: SvgExported::Text { svg: text, elements }, param_warnings: Vec::new() })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcore_record::Vec2;
    use solidcraft_geom::prelude::*;
    use solidcraft_io::Geom2;

    use crate::Session;

    #[test]
    fn dxf_and_svg_round_trip_into_sketches() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [30, 20]})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [10, 10], "radius": 4})).unwrap();
        let out = s.execute("sketch.export_dxf", &json!({})).unwrap();
        let dxf = out["dxf"].as_str().unwrap().to_string();
        s.execute("sketch.finish", &json!({})).unwrap();
        // Insert it again as a new sketch, offset: same profiles.
        let r = s.execute("sketch.insert_dxf", &json!({"text": dxf, "at": [100, 0]})).unwrap();
        assert_eq!(r["curves"], 5, "{r}");
        let id = r["sketch"].as_u64().unwrap();
        let ss = s.model.state().sketch(id).unwrap().clone();
        let mut areas: Vec<f64> = ss.profiles.iter().map(|p| p.area).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert!((areas[1] - (600.0 - std::f64::consts::PI * 16.0)).abs() < 1e-6, "{areas:?}");
        // SVG with a path and text, into the active sketch.
        let svg =
            r#"<svg width="40mm" height="40mm" viewBox="0 0 40 40"><path d="M0 0 H40 V40 H0 Z"/><text x="5" y="20" font-size="10">Hi</text></svg>"#;
        let r = s.execute("sketch.insert_svg", &json!({"text": svg})).unwrap();
        assert_eq!(r["curves"], 4, "{r}");
        assert_eq!(r["texts"], 1, "{r}");
        assert!(s.execute("sketch.insert_svg", &json!({"text": "<svg></svg>"})).is_err());
        assert!(s.execute("sketch.insert_dxf", &json!({"path": "/nonexistent.dxf"})).is_err());
        s.execute("sketch.finish", &json!({})).unwrap();
        // Ends that miss by 0.00005 still close the square; with a tighter tolerance they don't.
        let gappy = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n10\n21\n0\n0\nLINE\n10\n10.00005\n20\n0\n11\n10\n21\n10\n0\nLINE\n10\n10\n20\n10\n11\n0\n21\n10\n0\nLINE\n10\n0\n20\n10.00003\n11\n0\n21\n0.00004\n0\nENDSEC\n0\nEOF\n";
        for (tol, n) in [(1e-4, 1), (1e-6, 0)] {
            let r = s.execute("sketch.insert_dxf", &json!({"text": gappy, "tolerance": tol})).unwrap();
            let ss = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
            assert_eq!(ss.profiles.len(), n, "tolerance {tol}");
            s.execute("sketch.finish", &json!({})).unwrap();
        }
    }

    /// #15408: an SVG can repeat the same cubic outline backwards without losing its profile.
    #[test]
    fn repeated_svg_cubic_outlines_keep_their_curves_and_extrude() {
        let mut s = Session::default();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <path d="M0 0 C0 5 12 4 11 -1 C10 -6 -1 -5 0 0 Z"/>
            <path d="M0 0 C-1 -5 10 -6 11 -1 C12 4 0 5 0 0 Z"/>
        </svg>"#;
        let imported = s.execute("sketch.insert_svg", &json!({"text": svg})).unwrap();
        assert_eq!(imported["curves"], 4, "all source curves remain editable");
        let id = imported["sketch"].as_u64().unwrap();
        let areas = profile_areas(&s, id);
        assert_eq!(areas.len(), 1, "{areas:?}");
        // 1/2 ∮ (x dy - y dx) for the cubics is 1353/20 square pixels; SVG uses 96 dpi.
        let area = 1353.0 / 20.0 * (25.4_f64 / 96.0).powi(2);
        assert!((areas[0] - area).abs() < 1e-12, "{areas:?}");
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 2})).unwrap();
        assert_eq!(s.model.state().bodies.len(), 1);
        let volume = total_volume(&s);
        assert!((volume - area * 2.0).abs() < 1e-6, "volume {volume}");
    }

    /// A degree-9 SPLINE (solidcraft#5269) with fewer control points than its degree needs still
    /// comes in as one spline over all its points, which the sketch draws at the degree they allow.
    #[test]
    fn a_degree_nine_spline_with_few_control_points_comes_in_whole() {
        let mut s = Session::default();
        for n in 2..=10 {
            let points: String = (0..n).map(|i| format!("10\n{i}\n20\n{}\n", i % 2)).collect();
            let dxf = format!("0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n9\n{points}0\nENDSEC\n0\nEOF\n");
            let r = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap();
            assert_eq!(r["curves"], 1, "{n} points: {r}");
            s.execute("sketch.finish", &json!({})).unwrap();
        }
    }

    /// #11119: a closed SPLINE with more control points than a sketch spline holds keeps its
    /// whole loop, ends and all, so it still makes a profile; the tail was cut off before, which
    /// left an outline of open splines that cannot be extruded.
    #[test]
    fn a_closed_spline_with_too_many_control_points_still_closes() {
        let n = solidcraft_sketch::MAX_SPLINE_POINTS * 3;
        let mut points = String::new();
        for i in 0..=n {
            let a = std::f64::consts::TAU * (i % n) as f64 / n as f64;
            points.push_str(&format!(
                "10
{}
20
{}
",
                30.0 * a.cos(),
                30.0 * a.sin()
            ));
        }
        let dxf = format!(
            "0
SECTION
2
ENTITIES
0
SPLINE
70
11
71
3
{points}0
ENDSEC
0
EOF
"
        );
        let mut s = Session::default();
        let r = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap();
        let sk = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
        assert_eq!(sk.profiles.len(), 1, "the loop closes into one profile: {r}");
        let want = std::f64::consts::PI * 900.0;
        assert!((sk.profiles[0].area - want).abs() < want * 0.01, "{} vs {want}", sk.profiles[0].area);
    }

    /// A DXF SPLINE of `degree` over `ctrl` with `knots` and `weights` (none: no 41 codes).
    fn spline_dxf(flags: u32, degree: u8, ctrl: &[(f64, f64)], knots: &[f64], weights: &[f64]) -> String {
        let mut e = format!("0\nSPLINE\n70\n{flags}\n71\n{degree}\n72\n{}\n73\n{}\n", knots.len(), ctrl.len());
        for k in knots {
            e.push_str(&format!("40\n{k}\n"));
        }
        for w in weights {
            e.push_str(&format!("41\n{w}\n"));
        }
        for (x, y) in ctrl {
            e.push_str(&format!("10\n{x}\n20\n{y}\n30\n0\n"));
        }
        e
    }

    /// The splines `sketch.insert_dxf` makes of `entities`: points, degree and knots and weights.
    fn imported_splines(entities: &str) -> Vec<(Vec<Vec2>, u8, Option<solidcraft_sketch::Nurbs>)> {
        let mut s = Session::default();
        let dxf = format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n");
        let r = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap();
        let sk = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
        sk.sketch
            .curves
            .iter()
            .filter_map(|c| match &c.kind {
                solidcraft_sketch::CurveKind::Spline { pts, control: true, degree, nurbs, .. } => {
                    Some((pts.iter().map(|i| sk.sketch.point(*i).unwrap()).collect(), *degree, nurbs.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The point at `t` in [0, 1] of an imported spline, as the sketch draws it.
    fn at(spline: &(Vec<Vec2>, u8, Option<solidcraft_sketch::Nurbs>), t: f64) -> Vec2 {
        solidcraft_sketch::spline_point_mode(&spline.0, true, spline.1, false, spline.2.as_ref(), t)
    }

    /// #9355: a SPLINE keeps its own knots and weights, so it comes in as the file's curve, not
    /// the evenly spaced, unweighted spline over its points it used to become.
    #[test]
    fn a_spline_keeps_its_knots_and_weights() {
        // A rational quadratic quarter circle of radius 10: every point lies on the circle.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let quarter = imported_splines(&spline_dxf(8, 2, &[(10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[1.0, w, 1.0]));
        let [arc] = &quarter[..] else { panic!("{quarter:?}") };
        assert_eq!(arc.2.as_ref().map(|n| n.weights.clone()), Some(vec![1.0, w, 1.0]));
        for i in 0..=20 {
            let r = at(arc, f64::from(i) / 20.0).len();
            assert!((r - 10.0).abs() < 1e-9, "t {i}/20: radius {r}");
        }
        // A cubic with a triple knot in the middle passes through its fourth control point there,
        // a sharp corner (a font's, in a logo); two of its control points coincide, and all seven
        // stay. Evenly spaced knots would round the corner off.
        let ctrl = [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (20.0, 0.0), (20.0, 10.0), (20.0, 20.0), (10.0, 30.0)];
        let knots = [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
        let cornered = imported_splines(&spline_dxf(8, 3, &ctrl, &knots, &[]));
        let [corner] = &cornered[..] else { panic!("{cornered:?}") };
        assert_eq!(corner.0.len(), 7, "every control point is kept");
        assert_eq!(corner.2.as_ref().map(|n| n.knots.clone()), Some(knots.to_vec()));
        assert!(at(corner, 0.5).dist(Vec2::new(20.0, 0.0)) < 1e-9, "{:?}", at(corner, 0.5));
        // Evenly spaced, unweighted knots are the spline the sketch draws anyway: stored as before.
        let plain = imported_splines(&spline_dxf(8, 3, &ctrl[3..], &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], &[]));
        assert!(matches!(&plain[..], [(pts, 3, None)] if pts.len() == 4), "{plain:?}");
    }

    /// #9355: a periodic SPLINE (open knots, its first control points repeated at the end) comes
    /// in as the same closed curve with clamped knots, and closes into a profile.
    #[test]
    fn a_periodic_spline_comes_in_closed_with_clamped_knots() {
        let ring: Vec<(f64, f64)> = (0..6)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 6.0;
                (20.0 * a.cos(), 20.0 * a.sin())
            })
            .collect();
        let ctrl: Vec<(f64, f64)> = ring.iter().chain(&ring[..3]).copied().collect();
        let knots: Vec<f64> = (0..13).map(f64::from).collect();
        let mut s = Session::default();
        let dxf = format!("0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", spline_dxf(11, 3, &ctrl, &knots, &[]));
        let r = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap();
        let sk = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
        assert_eq!(sk.profiles.len(), 1, "{r}");
        let spline = &imported_splines(&spline_dxf(11, 3, &ctrl, &knots, &[]))[0];
        let nurbs = spline.2.as_ref().unwrap();
        assert!(nurbs.knots[..4].iter().all(|k| *k == 3.0) && nurbs.knots[nurbs.knots.len() - 4..].iter().all(|k| *k == 9.0), "{:?}", nurbs.knots);
        // The same curve: the uniform cubic B-spline over the ring, at its knots.
        for i in 0..6 {
            let (p0, p1, p2) = (ring[i], ring[(i + 1) % 6], ring[(i + 2) % 6]);
            let want = Vec2::new((p0.0 + 4.0 * p1.0 + p2.0) / 6.0, (p0.1 + 4.0 * p1.1 + p2.1) / 6.0);
            let got = at(spline, f64::from(i as u32) / 6.0);
            assert!(got.dist(want) < 1e-9, "knot {i}: {got:?} vs {want:?}");
        }
    }

    /// #9355: a knot repeated more than the degree breaks the curve (here it jumps from the third
    /// control point to the fourth), so it comes in as two splines;
    /// knots that aren't a spline (decreasing) come in as before, evenly spaced over the points.
    #[test]
    fn a_broken_or_invalid_knotted_spline_still_comes_in() {
        let ctrl = [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (30.0, 10.0), (40.0, 0.0), (50.0, 5.0)];
        let broken = imported_splines(&spline_dxf(8, 2, &ctrl, &[0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], &[]));
        assert_eq!(broken.len(), 2, "{broken:?}");
        assert!(broken.iter().all(|(pts, degree, nurbs)| pts.len() == 3 && *degree == 2 && nurbs.is_some()), "{broken:?}");
        assert!(broken[0].0[2].dist(Vec2::new(20.0, 0.0)) < 1e-9 && broken[1].0[0].dist(Vec2::new(30.0, 10.0)) < 1e-9, "{broken:?}");
        let decreasing = imported_splines(&spline_dxf(8, 2, &ctrl, &[0.0, 0.0, 0.0, 0.7, 0.3, 0.5, 1.0, 1.0, 1.0], &[]));
        assert!(matches!(&decreasing[..], [(pts, 2, None)] if pts.len() == 6), "{decreasing:?}");
    }

    /// The sketch limit is process-wide: the tests that depend on it take turns.
    static SKETCH_LIMIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// #10874: a drawing with more entities than a sketch can hold is refused with the point
    /// cap and that scale alone does not help (a floorplan SVG often hits this).
    #[test]
    fn a_drawing_with_too_many_entities_names_the_sketch_cap() {
        let _turn = SKETCH_LIMIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let n = solidcraft_sketch::MAX_POINTS + 1;
        let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
        for i in 0..n {
            dxf.push_str(&format!("0\nLINE\n10\n{i}\n20\n0\n11\n{}\n21\n1\n", i + 1));
        }
        dxf.push_str("0\nENDSEC\n0\nEOF\n");
        let mut s = Session::default();
        let e = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap_err().to_string();
        assert!(e.contains("20000") && e.contains("scale") && e.contains("Largest sketch"), "{e}");
        assert!(s.active_sketch.is_none(), "a refused insert leaves no empty sketch behind");
    }

    /// #10874: with Preferences › Sketch › Largest sketch raised, a drawing past the default
    /// limit goes into one sketch; back at the default it is refused again.
    #[test]
    fn a_raised_sketch_limit_takes_a_denser_drawing() {
        use solidcraft_sketch::limits;
        let _turn = SKETCH_LIMIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let n = limits::DEFAULT_POINTS + 1;
        let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
        for i in 0..n {
            dxf.push_str(&format!("0\nPOINT\n10\n{}\n20\n{}\n", i % 200, i / 200));
        }
        dxf.push_str("0\nENDSEC\n0\nEOF\n");
        let mut s = Session::default();
        limits::set_max_points(limits::DEFAULT_POINTS + 1000).unwrap();
        let raised = s.execute("sketch.insert_dxf", &json!({"text": dxf}));
        limits::set_max_points(limits::DEFAULT_POINTS).unwrap();
        raised.unwrap();
        let id = s.active_sketch.unwrap();
        assert!(super::sketch_store::read(&s.doc, id).unwrap().points.len() > limits::DEFAULT_POINTS, "every point went in");
        s.execute("sketch.finish", &json!({})).unwrap();
        let e = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap_err().to_string();
        assert!(e.contains("at most 20000"), "{e}");
    }

    /// A dense outline is simplified by the distance asked: the curves kept follow the drawing
    /// within it, no removed vertex lying further away.
    #[test]
    fn simplify_follows_a_dense_outline_within_its_distance() {
        let n = 2000;
        let mut path = String::from("M 20 0");
        // The file's own coordinates, and where they land: the root maps the viewBox "-20 0 40 20"
        // to 40 x 20 mm with the y axis turned over.
        let mut original = vec![Vec2::new(40.0, 0.0)];
        for k in 1..=n {
            let a = std::f64::consts::PI * k as f64 / n as f64;
            let p = Vec2::new(20.0 * a.cos(), 20.0 * a.sin());
            path.push_str(&format!(" L {:.6} {:.6}", p.x, p.y));
            original.push(Vec2::new(p.x + 20.0, -p.y));
        }
        let svg =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40mm\" height=\"20mm\" viewBox=\"-20 0 40 20\"><path d=\"{path}\"/></svg>");
        let mut s = Session::default();
        let r = s.execute("sketch.insert_svg", &json!({"text": svg, "simplify": 0.05})).unwrap();
        let curves = r["curves"].as_u64().unwrap() as usize;
        assert!(curves > 1 && curves < n / 4, "most of the dense half circle's edges go: {curves} of {n}");
        let id = s.active_sketch.unwrap();
        let sk = super::sketch_store::read(&s.doc, id).unwrap();
        let lines: Vec<(Vec2, Vec2)> = (0..sk.curves.len())
            .filter_map(|i| match sk.shape(i)? {
                solidcraft_sketch::Shape::Line { a, b } => Some((a, b)),
                _ => None,
            })
            .collect();
        let worst = original
            .iter()
            .map(|p| lines.iter().map(|(a, b)| super::point_segment_distance(*p, *a, *b)).fold(f64::INFINITY, f64::min))
            .fold(0.0_f64, f64::max);
        assert!(worst <= 0.05 + 1e-9, "no point of the drawing is further than 0.05 mm from the curves kept: {worst}");
    }

    /// A Simplify distance past a hundredth of the drawing is brought down to it, in the file's
    /// own units however the drawing is scaled: 100 mm on a 40 mm drawing simplifies exactly as
    /// 0.4 mm does.
    #[test]
    fn simplify_never_takes_more_than_a_hundredth_of_the_drawing() {
        let n = 400;
        let mut path = String::from("M 20 0");
        for k in 1..=n {
            let a = std::f64::consts::PI * k as f64 / n as f64;
            path.push_str(&format!(" L {:.5} {:.5}", 20.0 * a.cos(), 20.0 * a.sin()));
        }
        let svg =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40mm\" height=\"20mm\" viewBox=\"-20 0 40 20\"><path d=\"{path}\"/></svg>");
        let curves = |simplify: f64| {
            let mut s = Session::default();
            s.execute("sketch.insert_svg", &json!({"text": svg, "simplify": simplify})).unwrap()["curves"].as_u64().unwrap()
        };
        // 1% of the 40 mm span, and the same at any size factor: the window's preview imports at
        // scale 1 and the confirmed command at the window's scale, and both must keep the same
        // edges.
        let at_clamp = curves(0.4);
        assert_eq!(curves(100.0), at_clamp, "a distance past the clamp is the clamp");
        assert_eq!(curves(500.0), at_clamp, "a distance past the command's own clamp is the clamp");
        assert!(at_clamp > 1, "most of the outline still comes in: {at_clamp}");
    }

    /// A drawing the sketch takes as drawn needs no Simplify distance; a dense one gets the
    /// smallest distance at which the insert fits, and nothing below it does; more circles than a
    /// sketch takes, which Simplify leaves as drawn, get none.
    #[test]
    fn bounds_fit_finds_the_smallest_distance_that_fits() {
        let mut s = Session::default();
        // As drawn: a rectangle.
        let square = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        let b = s.execute("sketch.drawing_bounds", &json!({"text": square, "fit": true})).unwrap();
        assert_eq!(b["auto_simplify"], json!(0.0), "{b}");
        // A dense half circle: at the auto distance the insert fits, below it the sketch refuses.
        let n = 40_000;
        let mut path = String::from("M 10 0");
        for k in 1..=n {
            let a = std::f64::consts::PI * k as f64 / n as f64;
            path.push_str(&format!(" L {:.6} {:.6}", 10.0 * a.cos(), 10.0 * a.sin()));
        }
        let dense =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20mm\" height=\"10mm\" viewBox=\"-10 0 20 10\"><path d=\"{path}\"/></svg>");
        let b = s.execute("sketch.drawing_bounds", &json!({"text": dense, "fit": true})).unwrap();
        let auto = b["auto_simplify"].as_f64().unwrap_or_else(|| panic!("a distance fits: {b}"));
        assert!(auto > 0.0 && auto <= super::MAX_SIMPLIFY_MM && (auto * 100.0).fract().abs() < 1e-9, "on the 0.01 grid: {auto}");
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let fit = s.execute("sketch.insert_svg", &json!({"text": dense, "simplify": auto}));
        assert!(fit.unwrap()["curves"].as_u64().unwrap() > 0, "the auto distance fits");
        let mut below = Session::default();
        below.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let too_dense = below.execute("sketch.insert_svg", &json!({"text": dense, "simplify": (auto - 0.02).max(0.0)}));
        assert!(too_dense.is_err(), "nothing below it fits: {too_dense:?}");
        // Circles stay as drawn, so no distance fits more of them than a sketch takes.
        let mut circles = String::new();
        for k in 0..25_000 {
            circles.push_str(&format!("<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"0.1\"/>", (k % 100) as f64 * 0.2, (k / 100) as f64 * 0.2));
        }
        let many = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20mm\" height=\"10mm\" viewBox=\"0 0 20 10\">{circles}</svg>");
        let b = s.execute("sketch.drawing_bounds", &json!({"text": many, "fit": true})).unwrap();
        assert!(b.get("auto_simplify").is_none(), "no distance fits circles that stay as drawn: {b}");
    }

    /// `simplify` is measured on the drawing as drawn, so the window's preview (imported at
    /// scale 1) and the confirmed command (at the window's scale) keep the same edges: the
    /// confirmed geometry is exactly the preview scaled by that factor.
    #[test]
    fn simplify_keeps_the_same_edges_at_any_scale() {
        let n = 1000;
        let mut path = String::from("M 20 0");
        for k in 1..=n {
            let a = std::f64::consts::PI * k as f64 / n as f64;
            path.push_str(&format!(" L {:.6} {:.6}", 20.0 * a.cos(), 20.0 * a.sin()));
        }
        let svg =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40mm\" height=\"20mm\" viewBox=\"-20 0 40 20\"><path d=\"{path}\"/></svg>");
        // The kept outline at `scale`: how many curves, its box, and the first line's start point.
        let kept = |scale: f64| {
            let mut s = Session::default();
            let r = s.execute("sketch.insert_svg", &json!({"text": svg, "simplify": 0.05, "scale": scale})).unwrap();
            let id = s.active_sketch.unwrap();
            let sk = super::sketch_store::read(&s.doc, id).unwrap();
            let (mut lo, mut hi, mut first) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN), None);
            for i in 0..sk.curves.len() {
                if let Some(solidcraft_sketch::Shape::Line { a, b }) = sk.shape(i) {
                    for q in [a, b] {
                        lo = Vec2::new(lo.x.min(q.x), lo.y.min(q.y));
                        hi = Vec2::new(hi.x.max(q.x), hi.y.max(q.y));
                        first.get_or_insert(q);
                    }
                }
            }
            (r["curves"].as_u64().unwrap(), lo, hi, first.unwrap_or_default())
        };
        let (one, lo1, hi1, start1) = kept(1.0);
        let (two, lo2, hi2, start2) = kept(2.0);
        assert!(one > 1, "the dense half circle keeps some edges: {one}");
        assert_eq!(one, two, "the same edges are kept at twice the size");
        let close = |a: Vec2, b: Vec2| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
        assert!(close(start2, start1 * 2.0), "the same first point, scaled: {start1:?} vs {start2:?}");
        assert!(close(lo2, lo1 * 2.0) && close(hi2, hi1 * 2.0), "the kept outline is twice the size: {lo1:?}..{hi1:?} vs {lo2:?}..{hi2:?}");
    }

    /// An SVG with far more edges than the largest sketch takes is refused with its count and
    /// what reduces it; the same drawing at a Simplify distance goes in, with few enough curves.
    #[test]
    fn simplify_takes_a_drawing_past_the_sketch_limit() {
        let _turn = SKETCH_LIMIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let n = 60_000;
        let mut path = String::from("M 20 0");
        for k in 1..=n {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            path.push_str(&format!(" L {:.6} {:.6}", 20.0 * a.cos(), 20.0 * a.sin()));
        }
        let svg =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40mm\" height=\"40mm\" viewBox=\"-20 -20 40 40\"><path d=\"{path}\"/></svg>");
        let mut s = Session::default();
        let refused = s.execute("sketch.insert_svg", &json!({"text": svg})).unwrap_err().to_string();
        assert!(refused.contains(&format!("{n} curves")) && refused.contains("simplify"), "{refused}");
        assert!(refused.contains("at most 20000") && refused.contains("Largest sketch") && refused.contains("scale"), "{refused}");
        let r = s.execute("sketch.insert_svg", &json!({"text": svg, "simplify": 0.05})).unwrap();
        let curves = r["curves"].as_u64().unwrap();
        assert!(curves > 0 && curves <= 20_000, "the closed outline comes in with {curves} curves");
    }

    /// Simplifying moves lines only: an arc, a circle or a spline is drawn as it was, and lines
    /// meeting it are not followed through it.
    #[test]
    fn simplify_leaves_arcs_circles_and_splines_as_drawn() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="20mm" viewBox="0 0 40 20">
            <path d="M 0 10 L 5 10 L 5.01 10 L 10 10"/>
            <path d="M 10 10 A 5 5 0 0 1 20 10"/>
            <circle cx="30" cy="10" r="4"/>
        </svg>"#;
        let mut s = Session::default();
        let r = s.execute("sketch.insert_svg", &json!({"text": svg, "simplify": 0.05})).unwrap();
        assert_eq!(r["curves"], 3, "one line, one arc and one circle: {r}");
        let kinds = s.execute("sketch.inspect", &json!({})).unwrap();
        let types: Vec<&str> = kinds["curves"].as_array().unwrap().iter().filter_map(|c| c["type"].as_str()).collect();
        assert_eq!(types.iter().filter(|t| **t == "line").count(), 1, "{types:?}");
        assert_eq!(types.iter().filter(|t| **t == "arc").count(), 1, "{types:?}");
        assert_eq!(types.iter().filter(|t| **t == "circle").count(), 1, "{types:?}");
    }

    /// The simplifier keeps runs of lines that meet end to end within the tolerance asked, and
    /// leaves every other entity where it is.
    #[test]
    fn straight_runs_become_one_line_and_gaps_end_a_run() {
        let line = |a: (f64, f64), b: (f64, f64)| Geom2::Line(Vec2::new(a.0, a.1), Vec2::new(b.0, b.1));
        // Three lines along one straight run: one line from its first end to its last.
        let (geom, complete) =
            super::simplify_lines(&[line((0.0, 0.0), (1.0, 0.0)), line((1.0, 0.0), (2.0, 0.0)), line((2.0, 0.0), (3.0, 0.0))], 0.01, 1e-9);
        assert!(complete, "a run of three lines is no work at all");
        assert_eq!(geom, [line((0.0, 0.0), (3.0, 0.0))]);
        // A gap ends the run: two lines that do not meet stay two.
        let gap = [line((0.0, 0.0), (1.0, 0.0)), line((2.0, 0.0), (3.0, 0.0))];
        assert_eq!(super::simplify_lines(&gap, 0.01, 1e-9).0, gap);
        // Other geometry stays, and the lines before and after it are runs of their own.
        let arc = Geom2::Arc { c: Vec2::new(5.0, 0.0), a: Vec2::new(4.0, 0.0), b: Vec2::new(6.0, 0.0) };
        let mixed = [line((0.0, 0.0), (1.0, 0.0)), line((1.0, 0.0), (2.0, 0.0)), arc.clone()];
        assert_eq!(super::simplify_lines(&mixed, 0.01, 1e-9).0, [line((0.0, 0.0), (2.0, 0.0)), arc]);
    }

    /// Douglas-Peucker drops a zigzag's middle only where the tolerance reaches past it, and
    /// always keeps the run's ends.
    #[test]
    fn douglas_peucker_keeps_the_ends_and_what_the_tolerance_cannot_reach() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.1), Vec2::new(2.0, 0.0), Vec2::new(3.0, 0.1), Vec2::new(4.0, 0.0)];
        let (kept, complete) = super::douglas_peucker(&points, 0.05);
        assert!(complete);
        assert_eq!(kept, points, "the zigzag is further than 0.05 mm from its chords");
        let (kept, complete) = super::douglas_peucker(&points, 0.2);
        assert!(complete);
        assert_eq!(kept, [points[0], points[4]], "within 0.2 mm the ends are the whole line");
        let (kept, _) = super::douglas_peucker(&points[..2], 0.2);
        assert_eq!(kept, &points[..2], "a two-point run keeps both ends");
    }

    /// #9641: an SVG with nothing SolidCraft can draw (here only an embedded picture) is refused
    /// with a message naming the file and saying why, not "invalid parameters … no geometry"; a
    /// UTF-16 SVG file inserts.
    #[test]
    fn an_svg_with_nothing_to_import_says_why() {
        let dir = std::env::temp_dir().join(format!("solidcraft-9641-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let picture = dir.join("logo.svg");
        std::fs::write(
            &picture,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><image width="64" height="64" href="data:image/png;base64,iVBORw0KGgo="/></svg>"#,
        )
        .unwrap();
        let mut s = Session::default();
        for cmd in ["sketch.insert_svg", "sketch.drawing_bounds"] {
            let e = s.execute(cmd, &json!({"path": picture.to_string_lossy(), "align": "center"})).unwrap_err().to_string();
            assert!(e.starts_with("logo.svg: ") && e.contains("embedded picture") && !e.contains("invalid parameters"), "{cmd}: {e}");
        }
        assert!(s.active_sketch.is_none(), "a refused insert leaves no empty sketch behind");
        let square = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        let utf16 = dir.join("square.svg");
        std::fs::write(&utf16, [0xFF, 0xFE].into_iter().chain(square.encode_utf16().flat_map(u16::to_le_bytes)).collect::<Vec<u8>>()).unwrap();
        let r = s.execute("sketch.insert_svg", &json!({"path": utf16.to_string_lossy()})).unwrap();
        assert_eq!(r["curves"], 4, "{r}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A binary DXF file inserts like its ASCII twin: here one 10 mm line, written with the
    /// two-byte group codes of release 13 and later (DXF reference, "Binary DXF Files").
    #[test]
    fn a_binary_dxf_file_inserts_its_drawing() {
        let mut bytes = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
        let text = |b: &mut Vec<u8>, code: i16, v: &str| {
            b.extend(code.to_le_bytes());
            b.extend(v.as_bytes());
            b.push(0);
        };
        let double = |b: &mut Vec<u8>, code: i16, v: f64| {
            b.extend(code.to_le_bytes());
            b.extend(v.to_le_bytes());
        };
        for (code, v) in [(0, "SECTION"), (2, "ENTITIES"), (0, "LINE"), (8, "0")] {
            text(&mut bytes, code, v);
        }
        for (code, v) in [(10, 0.0), (20, 0.0), (11, 10.0), (21, 0.0)] {
            double(&mut bytes, code, v);
        }
        for (code, v) in [(0, "ENDSEC"), (0, "EOF")] {
            text(&mut bytes, code, v);
        }
        let file = std::env::temp_dir().join(format!("solidcraft-binary-dxf-{}.dxf", std::process::id()));
        std::fs::write(&file, &bytes).unwrap();
        let mut s = Session::default();
        let r = s.execute("sketch.insert_dxf", &json!({"path": file.to_string_lossy()}));
        let _ = std::fs::remove_file(&file);
        assert_eq!(r.unwrap()["curves"], 1);
    }

    /// #8155: an elliptical arc of a DXF (as Fusion writes one) comes into the sketch as exact
    /// conics, and the drawing's size is the arc's own, not the box round the conics' apexes.
    #[test]
    fn an_elliptical_arc_comes_in_as_conics_at_its_own_size() {
        // Centre (0, 0), major radius 10 along X, minor 5, from -45° to 45°: one quarter turn,
        // whose apex is at x = 10 / cos 45° = 14.1, past the arc's end at x = 10.
        let dxf = format!(
            "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n0\n20\n0\n11\n10\n21\n0\n40\n0.5\n41\n{}\n42\n{}\n0\nENDSEC\n0\nEOF\n",
            -std::f64::consts::FRAC_PI_4,
            std::f64::consts::FRAC_PI_4
        );
        let mut s = Session::default();
        let b = s.execute("sketch.drawing_bounds", &json!({"text": dxf, "format": "dxf"})).unwrap();
        let (w, h) = (b["width"].as_f64().unwrap(), b["height"].as_f64().unwrap());
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!((w - 10.0 * (1.0 - half)).abs() < 1e-3 && (h - 10.0 * half).abs() < 1e-3, "{b}");
        let r = s.execute("sketch.insert_dxf", &json!({"text": dxf})).unwrap();
        assert_eq!(r["curves"], 1, "{r}");
        let st = s.model.state();
        let sk = &st.sketch(r["sketch"].as_u64().unwrap()).unwrap().sketch;
        assert!(matches!(sk.curves[0].kind, solidcraft_sketch::CurveKind::Conic { .. }), "{:?}", sk.curves[0].kind);
        // Every point the sketch draws it with lies on the ellipse.
        let worst = sk.polyline(0).iter().map(|p| ((p.x / 10.0).powi(2) + (p.y / 5.0).powi(2) - 1.0).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-9, "{worst}");
    }

    const PLATE: &str =
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><rect x="0" y="0" width="20" height="10"/></svg>"#;

    fn span(s: &Session) -> (f64, f64, f64, f64) {
        let st = s.model.state();
        let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for p in (0..sk.curves.len()).flat_map(|i| sk.polyline(i)) {
            lo = (lo.0.min(p.x), lo.1.min(p.y));
            hi = (hi.0.max(p.x), hi.1.max(p.y));
        }
        (lo.0, lo.1, hi.0, hi.1)
    }

    fn inserted(params: serde_json::Value) -> (f64, f64, f64, f64) {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let mut p = json!({"text": PLATE});
        p.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
        s.execute("sketch.insert_svg", &p).unwrap();
        span(&s)
    }

    /// An SVG's pixels are read at 96 dpi unless `dpi` says otherwise (#20240): a file drawn at 72
    /// dpi comes in at the size it was drawn at. A length with its own unit keeps it, and a DXF has
    /// no pixels to read.
    #[test]
    fn an_svg_is_read_at_the_dpi_it_was_drawn_at() {
        let size = |svg: &str, params: serde_json::Value| {
            let mut s = Session::default();
            s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
            let mut p = json!({"text": svg});
            p.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
            s.execute("sketch.insert_svg", &p).unwrap();
            let b = span(&s);
            (b.2 - b.0, b.3 - b.1)
        };
        let near = |a: (f64, f64), e: (f64, f64)| assert!((a.0 - e.0).abs() < 1e-6 && (a.1 - e.1).abs() < 1e-6, "{a:?} vs {e:?}");
        let px = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 144 72"><rect x="0" y="0" width="72" height="36"/></svg>"#;
        near(size(px, json!({})), (72.0 * 25.4 / 96.0, 36.0 * 25.4 / 96.0));
        near(size(px, json!({"dpi": 96})), (72.0 * 25.4 / 96.0, 36.0 * 25.4 / 96.0));
        near(size(px, json!({"dpi": 72})), (25.4, 12.7));
        near(size(px, json!({"dpi": 72, "scale": 2})), (50.8, 25.4));
        near(size(px, json!({"dpi": 72, "width": 10})), (10.0, 5.0));
        // The sheet is read at the same dpi.
        near(size(px, json!({"dpi": 72, "size_from": "viewbox"})), (25.4, 12.7));
        // Measuring takes the same `dpi`, for the drawing and for its sheet.
        let b = Session::default().execute("sketch.drawing_bounds", &json!({"text": px, "dpi": 72})).unwrap();
        near((b["width"].as_f64().unwrap(), b["height"].as_f64().unwrap()), (25.4, 12.7));
        near((b["sheet"]["width"].as_f64().unwrap(), b["sheet"]["height"].as_f64().unwrap()), (50.8, 25.4));
        // A file with its own size in millimetres is not a matter of pixels.
        let mm =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 200 100"><rect width="200" height="100"/></svg>"#;
        near(size(mm, json!({"dpi": 72})), (100.0, 50.0));
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        for bad in [json!(0), json!(0.5), json!(20000), json!("72")] {
            let e = s.execute("sketch.insert_svg", &json!({"text": px, "dpi": bad})).unwrap_err().to_string();
            assert!(e.contains("dpi"), "{bad}: {e}");
        }
        let dxf = json!({"text": "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n0\nENDSEC\n0\nEOF\n", "dpi": 72});
        assert!(s.execute("sketch.insert_dxf", &dxf).unwrap_err().to_string().contains("DXF"));
    }

    /// A drawing can be given a size in millimetres (its aspect kept), turned about its origin and
    /// placed; the size of the file itself is read without inserting anything.
    #[test]
    fn an_inserted_drawing_takes_a_width_a_height_and_an_angle() {
        let mut s = Session::default();
        let b = s.execute("sketch.drawing_bounds", &json!({"text": PLATE, "format": "svg"})).unwrap();
        assert_eq!((b["width"].as_f64(), b["height"].as_f64()), (Some(20.0), Some(10.0)));
        let near = |a: (f64, f64, f64, f64), e: (f64, f64, f64, f64)| {
            assert!((a.0 - e.0).abs() < 1e-6 && (a.1 - e.1).abs() < 1e-6 && (a.2 - e.2).abs() < 1e-6 && (a.3 - e.3).abs() < 1e-6, "{a:?} vs {e:?}")
        };
        near(inserted(json!({"width": 40})), (0.0, -20.0, 40.0, 0.0));
        near(inserted(json!({"height": 5, "at": [3, 4]})), (3.0, -1.0, 13.0, 4.0));
        near(inserted(json!({"width": 40, "scale": 9})), (0.0, -20.0, 40.0, 0.0));
        // Turned 90 degrees about the origin, then placed.
        near(inserted(json!({"angle": 90, "at": [100, 0]})), (100.0, 0.0, 110.0, 20.0));
        // About its centre: the middle lands on `at` whatever the size and angle.
        near(inserted(json!({"align": "center", "at": [50, 50]})), (40.0, 45.0, 60.0, 55.0));
        near(inserted(json!({"align": "center", "at": [50, 50], "width": 40})), (30.0, 40.0, 70.0, 60.0));
        near(inserted(json!({"align": "center", "at": [50, 50], "angle": 90})), (45.0, 40.0, 55.0, 60.0));
        for bad in [json!({"width": 0}), json!({"height": -1}), json!({"angle": "x"}), json!({"align": "left"})] {
            s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
            let mut p = json!({"text": PLATE});
            p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(s.execute("sketch.insert_svg", &p).is_err(), "{bad}");
        }
    }

    /// A 20 × 10 rectangle drawn at (10, 10) on a 100 × 50 sheet.
    const ON_SHEET: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50"><rect x="10" y="10" width="20" height="10"/></svg>"#;

    /// `width` and `height` size the drawn geometry unless `size_from: viewbox` makes them the
    /// sheet's, so a drawing that does not fill its sheet keeps its place and size on it.
    #[test]
    fn an_svg_can_be_sized_by_its_sheet() {
        let near = |a: (f64, f64, f64, f64), e: (f64, f64, f64, f64)| {
            assert!((a.0 - e.0).abs() < 1e-6 && (a.1 - e.1).abs() < 1e-6 && (a.2 - e.2).abs() < 1e-6 && (a.3 - e.3).abs() < 1e-6, "{a:?} vs {e:?}")
        };
        let inserted = |params: serde_json::Value| {
            let mut s = Session::default();
            s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
            let mut p = json!({"text": ON_SHEET});
            p.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
            s.execute("sketch.insert_svg", &p).unwrap();
            span(&s)
        };
        // By the geometry (as before): the rectangle itself becomes 200 wide, placed at the origin.
        near(inserted(json!({"width": 200})), (100.0, -200.0, 300.0, -100.0));
        // By the sheet: the sheet becomes 200 wide, so the rectangle is 40 × 20 at (20, −40).
        near(inserted(json!({"width": 200, "size_from": "viewbox"})), (20.0, -40.0, 60.0, -20.0));
        near(inserted(json!({"size_from": "viewbox"})), (10.0, -20.0, 30.0, -10.0));
        // Centred on `at` by the sheet's middle (50, −25): the rectangle sits above and left of it.
        near(inserted(json!({"size_from": "viewbox", "align": "center", "at": [50, 50]})), (10.0, 55.0, 30.0, 65.0));
        let mut s = Session::default();
        let b = s.execute("sketch.drawing_bounds", &json!({"text": ON_SHEET})).unwrap();
        assert_eq!((b["width"].as_f64(), b["sheet"]["width"].as_f64(), b["sheet"]["height"].as_f64()), (Some(20.0), Some(100.0), Some(50.0)), "{b}");
        let b = s.execute("sketch.drawing_bounds", &json!({"text": r#"<svg><rect width="20" height="10"/></svg>"#})).unwrap();
        assert!(b.get("sheet").is_none(), "no viewBox, no width: {b}");
        // No sheet to size by, a DXF, and a wrong word are refused.
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let bare = json!({"text": r#"<svg><rect width="20" height="10"/></svg>"#, "size_from": "viewbox", "width": 50});
        assert!(s.execute("sketch.insert_svg", &bare).unwrap_err().to_string().contains("viewBox"));
        let dxf = json!({"text": "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n0\nENDSEC\n0\nEOF\n", "size_from": "viewbox"});
        assert!(s.execute("sketch.insert_dxf", &dxf).unwrap_err().to_string().contains("DXF"));
        let word = json!({"text": ON_SHEET, "size_from": "sheet"});
        assert!(s.execute("sketch.insert_svg", &word).unwrap_err().to_string().contains("size_from"));
    }

    /// `fixed: true` fixes every point the drawing adds, so the sketch has nothing left to move.
    #[test]
    fn an_inserted_drawing_can_come_in_fixed() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let r = s.execute("sketch.insert_svg", &json!({"text": PLATE, "fixed": true})).unwrap();
        assert_eq!((r["curves"].as_u64(), r["fixed"].as_u64()), (Some(4), Some(4)), "{r}");
        assert_eq!(r["created"]["constraints"].as_array().map(Vec::len), Some(4), "{r}");
        let solved = s.execute("sketch.solve", &json!({})).unwrap();
        assert_eq!(solved["dof"], json!(0), "{solved}");
        // Without it, as before: nothing fixed, four free points.
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let r = s.execute("sketch.insert_svg", &json!({"text": PLATE})).unwrap();
        assert!(r.get("fixed").is_none() && r["created"]["constraints"].as_array().is_some_and(Vec::is_empty), "{r}");
        // A circle: its centre is fixed, its radius stays free.
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let circle = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><circle cx="10" cy="10" r="5"/></svg>"#;
        let r = s.execute("sketch.insert_svg", &json!({"text": circle, "fixed": true})).unwrap();
        assert_eq!(r["fixed"], json!(1), "{r}");
        assert_eq!(s.execute("sketch.solve", &json!({})).unwrap()["dof"], json!(1), "the radius");
    }

    /// A NACA 00xx section (the public four-digit formula), `n` points per side on a cosine
    /// spacing, as a Selig file. The formula's trailing edge is open; `closed` uses the -0.1036
    /// last term that brings the sides together at x = 1.
    fn naca_00(thickness: f64, n: usize, closed: bool) -> String {
        let k4 = if closed { 0.1036 } else { 0.1015 };
        let yt = |x: f64| 5.0 * thickness * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x.powi(3) - k4 * x.powi(4));
        let xs: Vec<f64> = (0..=n).map(|i| 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / n as f64).cos())).collect();
        let mut text = format!("NACA 00{:02}\n", (thickness * 100.0).round() as usize);
        for &x in xs.iter().rev() {
            text.push_str(&format!("{x:.7} {:.7}\n", yt(x)));
        }
        for &x in xs.iter().skip(1) {
            text.push_str(&format!("{x:.7} {:.7}\n", -yt(x)));
        }
        text
    }

    /// The area of that section at chord `c`, in closed form: 2 ∫ y_t dx.
    fn naca_00_area(thickness: f64, c: f64, closed: bool) -> f64 {
        let k4 = if closed { 0.1036 } else { 0.1015 };
        10.0 * thickness * c * c * (0.2969 * 2.0 / 3.0 - 0.1260 / 2.0 - 0.3516 / 3.0 + 0.2843 / 4.0 - k4 / 5.0)
    }

    fn total_volume(s: &Session) -> f64 {
        s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum()
    }

    fn profile_areas(s: &Session, id: u64) -> Vec<f64> {
        s.model.state().sketch(id).unwrap().profiles.iter().map(|p| p.area).collect()
    }

    /// #14202: a NACA 0012 at a 100 mm chord comes in as one spline through its 121 points and a
    /// line across the open trailing edge, one profile whose area is the formula's (0.685083 t c²
    /// = 822.1 mm²), and extruded 10 mm it is 8221 mm³. The spline bows between the file's points
    /// (most at the nose), so the area is within a quarter of a percent, not exact.
    #[test]
    fn a_naca_0012_section_extrudes_to_its_area_times_the_depth() {
        let mut s = Session::default();
        let r = s.execute("sketch.insert_airfoil", &json!({"text": naca_00(0.12, 60, false), "chord": 100})).unwrap();
        assert_eq!(r["format"], "selig", "{r}");
        assert_eq!(r["name"], "NACA 0012", "{r}");
        assert_eq!(r["points"], 121, "{r}");
        assert_eq!(r["merged"], 0, "{r}");
        assert_eq!(r["curves"], 2, "{r}");
        assert_eq!(r["trailing_edge"], "open", "{r}");
        assert_eq!(r["created_sketch"], r["sketch"], "{r}");
        // y_t(1) = 5 t · 0.0021 = 0.00126 each side.
        assert!((r["te_gap"].as_f64().unwrap() - 0.252).abs() < 1e-6, "{r}");
        assert!(r["deviation"].as_f64().unwrap() < 1e-9, "{r}");
        let bulge = r["bulge"].as_f64().unwrap();
        assert!(bulge > 0.0 && bulge < 0.05, "{r}");
        let id = r["sketch"].as_u64().unwrap();
        let areas = profile_areas(&s, id);
        let expected = naca_00_area(0.12, 100.0, false);
        assert!((expected - 822.1).abs() < 1e-3, "{expected}");
        assert_eq!(areas.len(), 1, "{areas:?}");
        assert!((areas[0] - expected).abs() < 2.5e-3 * expected, "area {} vs {expected}", areas[0]);
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 10})).unwrap();
        let v = total_volume(&s);
        assert!((v - 8221.0).abs() < 2.5e-3 * 8221.0, "volume {v} vs 8221.0");
    }

    /// #14202: two sections, 100 mm and 60 mm chords 300 mm apart, loft to the volume of the
    /// linear taper: 0.685083 t · 300 · (100² + 100·60 + 60²) / 3 = 161 131.6 mm³.
    #[test]
    fn two_sections_loft_to_the_volume_of_the_linear_taper() {
        let mut s = Session::default();
        for (name, chord, z) in [("Root", 100.0, 0.0), ("Tip", 60.0, 300.0)] {
            let plane = json!({"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]});
            let r =
                s.execute("sketch.insert_airfoil", &json!({"text": naca_00(0.12, 60, false), "chord": chord, "plane": plane, "name": name})).unwrap();
            assert_eq!(r["curves"], 2, "{r}");
            s.execute("sketch.finish", &json!({})).unwrap();
        }
        s.execute("solid.loft", &json!({"sections": [{"sketch": "Root"}, {"sketch": "Tip"}], "body_name": "Wing"})).unwrap();
        let expected = naca_00_area(0.12, 1.0, false) * 300.0 * (100.0f64.powi(2) + 100.0 * 60.0 + 60.0f64.powi(2)) / 3.0;
        assert!((expected - 161_131.6).abs() < 0.1, "{expected}");
        let v = total_volume(&s);
        assert!((v - expected).abs() < 3e-3 * expected, "volume {v} vs {expected}");
    }

    /// #14202: a Lednicer file as the databases write it (CRLF, a comment, counts as `61.0` and
    /// `61.`, the nose on both sides) with a closed trailing edge comes in as one spline that ends
    /// where it starts, placed by its quarter-chord point, turned and mirrored.
    #[test]
    fn a_lednicer_section_is_placed_by_its_pivot_turned_and_flipped() {
        let selig = naca_00(0.12, 60, true);
        let pts: Vec<(f64, f64)> =
            selig.lines().skip(1).map(|l| l.split_once(' ').map(|(x, y)| (x.parse().unwrap(), y.parse().unwrap())).unwrap()).collect();
        assert_eq!(pts.len(), 121);
        let mut text = String::from("# a comment\r\nNACA 0012 lednicer\r\n      61.0      61.\r\n   \r\n");
        for (x, y) in pts[..=60].iter().rev() {
            text.push_str(&format!("  {x:.7}  {y:.7}\r\n"));
        }
        text.push_str("\r\n");
        for (x, y) in &pts[60..] {
            text.push_str(&format!("  {x:.7}  {y:.7}\r\n"));
        }
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let r = s
            .execute("sketch.insert_airfoil", &json!({"text": text, "chord": 100, "pivot": 0.25, "at": [10, 5], "angle": 90, "flip": true}))
            .unwrap();
        assert_eq!(r["format"], "lednicer", "{r}");
        assert_eq!(r["name"], "NACA 0012 lednicer", "{r}");
        assert_eq!(r["points"], 121, "{r}");
        assert_eq!(r["trailing_edge"], "closed", "{r}");
        assert_eq!(r["te_gap"], 0.0, "{r}");
        assert_eq!(r["curves"], 1, "{r}");
        assert_eq!(r["created_sketch"], serde_json::Value::Null, "{r}");
        let id = r["sketch"].as_u64().unwrap();
        let areas = profile_areas(&s, id);
        let expected = naca_00_area(0.12, 100.0, true);
        assert_eq!(areas.len(), 1, "{areas:?}");
        assert!((areas[0] - expected).abs() < 2.5e-3 * expected, "area {} vs {expected}", areas[0]);
        // The quarter-chord point sits at `at`; turned 90°, the chord runs up the sketch's y: the
        // nose 25 mm below `at`, the trailing edge 75 mm above it. Mirrored, the upper side is on
        // the +x side. The sketch's points are the file's, so the nose and tail are exact.
        let st = s.model.state();
        let sk = &st.sketch(id).unwrap().sketch;
        let has = |x: f64, y: f64| sk.points.iter().any(|p| (p.pos.x - x).abs() < 1e-9 && (p.pos.y - y).abs() < 1e-9);
        assert!(has(10.0, -20.0) && has(10.0, 80.0), "{:?}", sk.points);
        let (lo, hi) = (span(&s).0, span(&s).2);
        assert!((lo - 4.0).abs() < 0.02 && (hi - 16.0).abs() < 0.02, "{lo} {hi}");
        // The thickest point of the upper side (0.06 c at 0.3 c; the nearest file point is at
        // 0.2966 c) lands 6 mm to the +x side of `at`, 5 mm up the chord from it.
        let widest = sk.points.iter().map(|p| p.pos).max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
        assert!((widest.x - 16.0).abs() < 0.005 && (widest.y - 9.66).abs() < 0.05, "{widest:?}");
    }

    /// #14202: errors name the file and the line, a placeholder is never read as 0, more points
    /// than a spline takes is said with both numbers, and a refused insert leaves no sketch.
    #[test]
    fn airfoil_errors_name_the_file_and_the_line() {
        let dir = std::env::temp_dir().join(format!("solidcraft-14202-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.dat");
        std::fs::write(&junk, "NACA 23015\n  ......\n0.5 0.1\n").unwrap();
        let mut s = Session::default();
        let e = s.execute("sketch.insert_airfoil", &json!({"path": junk.to_string_lossy(), "chord": 100})).unwrap_err().to_string();
        assert_eq!(e, "junk.dat: line 2: expected two numbers, found 1: `......`");
        let e = s.execute("sketch.insert_airfoil", &json!({"text": "Name\n1 0\n0.5 ......\n", "chord": 100})).unwrap_err().to_string();
        assert_eq!(e, "airfoil: line 3: not a number: `......`");
        let e = s.execute("sketch.insert_airfoil", &json!({"text": naca_00(0.12, 300, false), "chord": 100})).unwrap_err().to_string();
        assert_eq!(e, "airfoil: 601 points; a spline takes at most 500");
        let good = naca_00(0.12, 20, false);
        for (p, part) in [
            (json!({"text": good, "chord": 0}), "`chord` must be a positive number (mm)"),
            (json!({"text": good}), "`chord` must be a positive number (mm)"),
            (json!({"text": good, "chord": 100, "pivot": "x"}), "`pivot` must be a number"),
            (json!({"text": good, "chord": 100, "angle": "x"}), "`angle` must be a number (degrees)"),
            (json!({"text": good, "chord": 100, "flip": "yes"}), "`flip` must be true or false"),
            (json!({"text": good, "chord": 100, "format": "csv"}), "`format` must be \"selig\" or \"lednicer\""),
            (json!({"chord": 100}), "give `path` or `text`"),
            (json!({"text": "hello", "chord": 100}), "airfoil: no coordinates"),
            (json!({"text": good, "chord": 10, "tolerance": 1000}), "points left within a tolerance of 10 mm; an airfoil needs at least four"),
            (json!({"path": dir.join("missing.dat").to_string_lossy(), "chord": 100}), "missing.dat"),
        ] {
            let e = s.execute("sketch.insert_airfoil", &p).unwrap_err().to_string();
            assert!(e.contains(part), "{p}: {e}");
        }
        assert!(s.active_sketch.is_none(), "a refused insert leaves no empty sketch behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #14202: two points of the file within the tolerance become one sketch point, counted and
    /// measured; a trailing edge within it is closed; with a tighter tolerance both stay apart.
    #[test]
    fn points_within_the_tolerance_merge_and_the_reply_measures_it() {
        // The upper side has a point 7e-7 chords (7e-5 mm at 100 mm) after another; the trailing
        // edge is 0.002 chords (0.2 mm) open.
        let text = "Tiny\n1 0.001\n0.5 0.05\n0.5000005 0.0500005\n0 0\n0.5 -0.05\n1 -0.001\n";
        let mut s = Session::default();
        let r = s.execute("sketch.insert_airfoil", &json!({"text": text, "chord": 100})).unwrap();
        assert_eq!((r["points"].as_u64(), r["merged"].as_u64(), r["curves"].as_u64()), (Some(6), Some(1), Some(2)), "{r}");
        assert_eq!(r["trailing_edge"], "open", "{r}");
        assert!((r["te_gap"].as_f64().unwrap() - 0.2).abs() < 1e-9, "{r}");
        let d = r["deviation"].as_f64().unwrap();
        assert!(d > 0.0 && d <= 1e-4, "{r}");
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("sketch.insert_airfoil", &json!({"text": text, "chord": 100, "tolerance": 1e-6})).unwrap();
        assert_eq!((r["merged"].as_u64(), r["deviation"].as_f64()), (Some(0), Some(0.0)), "{r}");
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("sketch.insert_airfoil", &json!({"text": text, "chord": 100, "tolerance": 0.5})).unwrap();
        assert_eq!((r["trailing_edge"].as_str(), r["curves"].as_u64()), (Some("closed"), Some(1)), "{r}");
        assert!(r["deviation"].as_f64().unwrap() > 0.05, "the last point is 0.2 mm from where the spline ends: {r}");
        assert_eq!(profile_areas(&s, r["sketch"].as_u64().unwrap()).len(), 1);
    }

    #[test]
    fn export_dxf_default_is_byte_identical_to_before() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [15, 15], "radius": 5})).unwrap();

        let out = s.execute("sketch.export_dxf", &json!({})).unwrap();
        let text = out["dxf"].as_str().unwrap();

        // Build expected R12 DXF exactly as before
        let st = s.model.state();
        let ss = st.sketch(1).unwrap();
        let sk = &ss.sketch;
        let mut geom = Vec::new();
        for (i, c) in sk.curves.iter().enumerate() {
            if c.construction {
                continue;
            }
            let pt = |k: usize| sk.point(k).unwrap_or_default();
            match &c.kind {
                solidcraft_sketch::CurveKind::Line { a, b } => geom.push(Geom2::Line(pt(*a), pt(*b))),
                solidcraft_sketch::CurveKind::Circle { c, r } => geom.push(Geom2::Circle(pt(*c), *r)),
                solidcraft_sketch::CurveKind::Arc { c, a, b } => geom.push(Geom2::Arc { c: pt(*c), a: pt(*a), b: pt(*b) }),
                _ => {
                    let poly = sk.polyline(i);
                    geom.extend(poly.windows(2).map(|w| Geom2::Line(w[0], w[1])));
                }
            }
        }
        let expected = solidcraft_io::write_dxf(&geom);
        assert_eq!(text, expected, "default output must be byte-identical to previous write_dxf output");

        // Null options must be treated as omitted (default behavior)
        for null_params in [
            json!({"units": null}),
            json!({"layer": null}),
            json!({"layers": null}),
            json!({"join": null}),
            json!({"path": null}),
            json!({"units": null, "layer": null, "layers": null, "join": null, "path": null}),
        ] {
            let out_null = s.execute("sketch.export_dxf", &null_params).unwrap();
            assert_eq!(out_null["dxf"].as_str().unwrap(), expected, "null options must behave identically to default options");
            assert_eq!(out_null["entities"], out["entities"]);
        }

        // Hostile inputs
        assert!(s.execute("sketch.export_dxf", &json!({"units": 123})).is_err());
        assert!(s.execute("sketch.export_dxf", &json!({"units": "unknown_unit"})).is_err());
        assert!(s.execute("sketch.export_dxf", &json!({"layer": 456})).is_err());
        assert!(s.execute("sketch.export_dxf", &json!({"layers": 123})).is_err());
        assert!(s.execute("sketch.export_dxf", &json!({"layers": "all"})).is_err());
        assert!(s.execute("sketch.export_dxf", &json!({"join": "true"})).is_err());
        assert!(s.execute("sketch.export_dxf", &json!({"path": 123})).is_err());
    }

    #[test]
    fn export_dxf_layers_present() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [50, 50]})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [25, 25], "radius": 10})).unwrap();

        // 1. Single custom layer
        let out_cut = s.execute("sketch.export_dxf", &json!({"layer": "CUT"})).unwrap();
        let dxf_cut = out_cut["dxf"].as_str().unwrap();
        assert!(dxf_cut.contains("\n8\nCUT\n"));
        assert!(!dxf_cut.contains("\n8\n0\n"));

        // 2. Roles layers (OUTLINE for outer profile, CUT for holes)
        let out_roles = s.execute("sketch.export_dxf", &json!({"layers": "roles"})).unwrap();
        let dxf_roles = out_roles["dxf"].as_str().unwrap();
        assert!(dxf_roles.contains("\n8\nOUTLINE\n"));
        assert!(dxf_roles.contains("\n8\nCUT\n"));

        // 3. Roles layers with join: true
        let out_joined_roles = s.execute("sketch.export_dxf", &json!({"join": true, "layers": "roles"})).unwrap();
        assert_eq!(out_joined_roles["entities"], 2);
        let dxf_joined = out_joined_roles["dxf"].as_str().unwrap();
        assert!(dxf_joined.contains("AcDbPolyline"));
        assert!(dxf_joined.contains("\n  8\nOUTLINE\n") || dxf_joined.contains("\n8\nOUTLINE\n"));
        assert!(dxf_joined.contains("\n  8\nCUT\n") || dxf_joined.contains("\n8\nCUT\n"));
    }

    #[test]
    fn export_dxf_join_does_not_duplicate_holes() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [100, 80]})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [30, 40], "radius": 5})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [70, 40], "radius": 8})).unwrap();

        // With join: true, layers: "roles": exactly 3 entities (1 outer outline, 2 hole cuts)
        let out_roles = s.execute("sketch.export_dxf", &json!({"join": true, "layers": "roles"})).unwrap();
        assert_eq!(out_roles["entities"], 3, "expected 1 outer outline and 2 hole cuts, got {}", out_roles["entities"]);
        let dxf_roles = out_roles["dxf"].as_str().unwrap();
        let lwpolyline_count = dxf_roles.lines().filter(|l| l.trim() == "LWPOLYLINE").count();
        assert_eq!(lwpolyline_count, 3, "expected exactly 3 LWPOLYLINE entities in DXF");

        // Without roles (single layer): still exactly 3 entities
        let out_single = s.execute("sketch.export_dxf", &json!({"join": true})).unwrap();
        assert_eq!(out_single["entities"], 3, "entities without roles must also be exactly 3");
    }

    #[test]
    fn export_dxf_join_closed_lwpolyline_with_bulge() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        // Quarter-pie shape: center at (0, 0), arc from (10, 0) to (0, 10), and lines to (0, 0)
        s.execute("sketch.line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
        s.execute("sketch.arc.center_point", &json!({"center": [0, 0], "start": [10, 0], "end": [0, 10]})).unwrap();
        s.execute("sketch.line", &json!({"points": [[0, 10], [0, 0]]})).unwrap();

        let out = s.execute("sketch.export_dxf", &json!({"join": true})).unwrap();
        let dxf = out["dxf"].as_str().unwrap();
        assert!(dxf.contains("LWPOLYLINE"));
        // 70: bit 1 (closed) + bit 128 (plinegen) = 129
        assert!(dxf.contains("\n 70\n129\n") || dxf.contains("\n70\n129\n"));
        // Bulge: 90° arc has sweep = pi/2, bulge = tan(sweep / 4) = tan(pi / 8) = sqrt(2) - 1 ≈ 0.414213562373
        let expected_bulge = (std::f64::consts::FRAC_PI_2 / 4.0).tan();
        let has_bulge = dxf
            .lines()
            .zip(dxf.lines().skip(1))
            .any(|(k, v)| k.trim() == "42" && (v.trim().parse::<f64>().unwrap_or(0.0).abs() - expected_bulge).abs() < 1e-6);
        assert!(has_bulge, "DXF must contain group 42 bulge approx {expected_bulge}");

        // Re-import the joined DXF into a new sketch and verify round-trip profile area
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("sketch.insert_dxf", &json!({"text": dxf, "at": [100, 0]})).unwrap();
        let id = r["sketch"].as_u64().unwrap();
        let ss = s.model.state().sketch(id).unwrap().clone();
        assert_eq!(ss.profiles.len(), 1);
        let expected_area = std::f64::consts::PI * 100.0 / 4.0; // quarter circle of radius 10
        assert!((ss.profiles[0].area - expected_area).abs() < 1e-4, "area was {}", ss.profiles[0].area);
    }

    #[test]
    fn export_dxf_inch_design_round_trip() {
        let mut s = Session::default();
        s.doc_mut().units = "in".to_string();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        // A 2 in by 3 in rectangle in an inch design (stored internally as 50.8 mm by 76.2 mm)
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [50.8, 76.2]})).unwrap();

        // 1. Export with units: "design"
        let out = s.execute("sketch.export_dxf", &json!({"units": "design"})).unwrap();
        let dxf = out["dxf"].as_str().unwrap();
        // $INSUNITS must be 1 (inches)
        assert!(dxf.contains("\n$INSUNITS\n70\n1\n"));
        // Sizes in inches: 2.0 and 3.0
        assert!(dxf.contains("2.000000000"));
        assert!(dxf.contains("3.000000000"));

        // Round-trip through read_dxf: reads $INSUNITS=1, scales by 25.4 to mm
        let geom = solidcraft_io::read_dxf(dxf).unwrap();
        let mut widths = Vec::new();
        let mut heights = Vec::new();
        for g in &geom {
            if let Geom2::Line(a, b) = g {
                let dx = (b.x - a.x).abs() / 25.4;
                let dy = (b.y - a.y).abs() / 25.4;
                if dx > 0.1 {
                    widths.push(dx);
                }
                if dy > 0.1 {
                    heights.push(dy);
                }
            }
        }
        for w in widths {
            assert!((w - 2.0).abs() < 1e-8, "width in inches was {w}");
        }
        for h in heights {
            assert!((h - 3.0).abs() < 1e-8, "height in inches was {h}");
        }

        // 2. Export with join: true and explicit units: "in"
        let out_join = s.execute("sketch.export_dxf", &json!({"join": true, "units": "in"})).unwrap();
        let dxf_join = out_join["dxf"].as_str().unwrap();
        assert!(dxf_join.contains("LWPOLYLINE"));
        assert!(dxf_join.contains("\n$INSUNITS\n 70\n1\n") || dxf_join.contains("\n$INSUNITS\n70\n1\n"));

        let geom_join = solidcraft_io::read_dxf(dxf_join).unwrap();
        assert_eq!(geom_join.len(), 4);
        for g in &geom_join {
            if let Geom2::Line(a, b) = g {
                let len_in = a.dist(*b) / 25.4;
                assert!((len_in - 2.0).abs() < 1e-8 || (len_in - 3.0).abs() < 1e-8, "edge length {len_in}");
            }
        }
    }

    /// A 30 x 20 rectangle with a 4 mm circle in it, on the sketch being edited.
    fn rectangle_with_a_round_hole() -> Session {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [30, 20]})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [10, 10], "radius": 4})).unwrap();
        s
    }

    #[test]
    fn export_svg_is_one_closed_path_with_the_hole_at_its_real_size() {
        let mut s = rectangle_with_a_round_hole();
        let out = s.execute("sketch.export_svg", &json!({})).unwrap();
        let svg = out["svg"].as_str().unwrap();
        assert!(svg.contains("width=\"30mm\" height=\"20mm\" viewBox=\"0 0 30 20\""), "{svg}");
        // The profile with its hole is one element: the outline and the hole are sub-paths of it.
        assert_eq!(out["elements"], 1, "{out}");
        let path = svg.lines().find(|l| l.starts_with("<path")).unwrap();
        assert_eq!((path.matches('M').count(), path.matches('Z').count()), (2, 2), "{path}");
    }

    #[test]
    fn export_svg_reads_back_into_a_sketch_with_the_same_profiles() {
        let mut s = rectangle_with_a_round_hole();
        let svg = s.execute("sketch.export_svg", &json!({})).unwrap()["svg"].as_str().unwrap().to_string();
        s.execute("sketch.finish", &json!({})).unwrap();
        let r = s.execute("sketch.insert_svg", &json!({"text": svg, "at": [100, 0]})).unwrap();
        let ss = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
        let mut areas: Vec<f64> = ss.profiles.iter().map(|p| p.area).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert!((areas[1] - (600.0 - std::f64::consts::PI * 16.0)).abs() < 1e-3, "{areas:?}");
    }

    #[test]
    fn export_svg_units_change_the_size_and_a_bad_one_is_refused() {
        let mut s = rectangle_with_a_round_hole();
        let cm = s.execute("sketch.export_svg", &json!({"units": "CM"})).unwrap();
        assert!(cm["svg"].as_str().unwrap().contains("width=\"3cm\" height=\"2cm\" viewBox=\"0 0 3 2\""), "{cm}");
        let inch = s.execute("sketch.export_svg", &json!({"units": "in"})).unwrap();
        assert!(inch["svg"].as_str().unwrap().contains("width=\"1.1811in\""), "{inch}");
        assert!(s.execute("sketch.export_svg", &json!({"units": "furlong"})).is_err());
        assert!(s.execute("sketch.export_svg", &json!({"units": 5})).is_err());
        assert!(s.execute("sketch.export_svg", &json!({"path": 5})).is_err());
    }

    #[test]
    fn export_svg_draws_loose_curves_and_leaves_construction_ones_out() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.line", &json!({"points": [[0, 0], [40, 10]]})).unwrap();
        s.execute("sketch.line", &json!({"points": [[0, 0], [100, 50]], "construction": true})).unwrap();
        let out = s.execute("sketch.export_svg", &json!({})).unwrap();
        let svg = out["svg"].as_str().unwrap();
        assert_eq!(out["elements"], 1, "{out}");
        assert!(svg.contains("width=\"40mm\" height=\"10mm\""), "the construction line is not in the bounds: {svg}");
        assert!(svg.contains("<path d=\"M0 10L40 0\"/>"), "{svg}");
    }

    #[test]
    fn export_svg_of_an_empty_sketch_is_refused_and_a_path_writes_the_file() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        assert!(s.execute("sketch.export_svg", &json!({})).is_err());
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [12, 8]})).unwrap();
        let file = std::env::temp_dir().join(format!("solidcraft-export-svg-{}.svg", std::process::id()));
        let out = s.execute("sketch.export_svg", &json!({"path": file.to_str().unwrap()})).unwrap();
        assert_eq!(out["elements"], 1, "{out}");
        let written = std::fs::read_to_string(&file).unwrap();
        let _ = std::fs::remove_file(&file);
        assert!(written.contains("width=\"12mm\" height=\"8mm\""), "{written}");
    }
}
