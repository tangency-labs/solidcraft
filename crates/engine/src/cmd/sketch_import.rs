//! Sketch drawing exchange: Insert DXF, Insert SVG and sketch DXF export.

use solidcraft_geom::prelude::*;
use std::collections::HashMap;

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_io::{DxfEntity, DxfLayer, Geom2, MAX_DRAWING_BYTES};
use solidcraft_sketch::{Analysis, ConstraintKind, CurveKind, LinkKind, LinkSource, Sketch};

use super::CommandSpec;
use super::sketch::{Left, edit_solved, ids_of};
use super::sketch_store;
use crate::params::{bad, bool_, num, str_, vec2};
use crate::schema::sketch_import::{
    DrawingBoundsParams, DrawingBoundsResult, DrawingBox, DrawingInserted, DxfExported, ExportDxfParams, ExportDxfResult, InsertDxfParams,
    InsertSvgParams, SketchSolve,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INSERT_DXF_CONTRACT: TypedContract<InsertDxfParams> = TypedContract::new();
const INSERT_SVG_CONTRACT: TypedContract<InsertSvgParams> = TypedContract::new();
const DRAWING_BOUNDS_CONTRACT: TypedContract<DrawingBoundsParams> = TypedContract::new();
const EXPORT_DXF_CONTRACT: TypedContract<ExportDxfParams> = TypedContract::new();

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
        .params(concat!("path | text: ASCII DXF (blocks expanded); at?: [x,y] offset, scale? | width? | height?: mm, the size of the drawn geometry's bounds before rotation, aspect kept (win over scale); angle?: degrees, about the drawing's origin (or its centre); align?: \"origin\" (default: `at` is where the file's origin goes) | \"center\" (`at` is where the drawing's middle goes and it turns and scales about it); tolerance?: mm within which ends join (default 0.0001); fixed?: bool (a Fix constraint on every point the drawing adds, as sketch.constraint.fix puts one; a circle's or ellipse's radius stays free; default false); plane?: (when no sketch is active, a new sketch on it; default XY)", created_doc!())),
    CommandSpec::new("sketch.insert_svg", "Insert SVG", crate::checked!("sketch.insert_svg", insert_svg))
        .at("SKETCH", "INSERT")
        .icon("svg")
        .contract(&INSERT_SVG_CONTRACT)
        .output(output_schema_for::<DrawingInserted>)
        .params(concat!("path | text: SVG; at?: [x,y] offset, scale? | width? | height?: mm, the drawing's size before rotation, aspect kept (win over scale); size_from?: \"geometry\" (default: width and height are the bounds of the drawn geometry) | \"viewbox\" (the SVG sheet: its viewBox, else its width and height; a drawing smaller than its sheet then comes out smaller than width or height say); angle?: degrees, about the drawing's origin (or its centre); align?: \"origin\" (default: `at` is where the file's origin goes) | \"center\" (`at` is where the drawing's (or sheet's) middle goes and it turns and scales about it); tolerance?: mm within which ends join (default 0.0001); fixed?: bool (every point it adds gets a Fix constraint, so the drawing stays where it is put); plane?: (when no sketch is active, a new sketch on it; default XY)", created_doc!())),
    CommandSpec::new("sketch.drawing_bounds", "Drawing Size", crate::checked!("sketch.drawing_bounds", drawing_bounds))
        .icon("svg")
        .noundo().read_only()
        .contract(&DRAWING_BOUNDS_CONTRACT)
        .output(output_schema_for::<DrawingBoundsResult>)
        .params("path | text: SVG or DXF (by `format`: \"svg\" | \"dxf\", default by the file name, else SVG); returns min, max, width and height of the drawn geometry in the file's own size, and for an SVG with a viewBox (or width and height) its sheet: {min, max, width, height}"),
    CommandSpec::new("sketch.export_dxf", "Save As DXF", crate::checked!("sketch.export_dxf", export_dxf))
        .icon("sketch_dxf")
        .noundo()
        .contract(&EXPORT_DXF_CONTRACT)
        .output(output_schema_for::<ExportDxfResult>)
        .params("sketch?: id|name (default active); path?: file to write (else the DXF text is returned); units?: \"design\" | \"in\" | \"inch\" | \"mm\" | \"cm\" | \"m\" | \"ft\" (default mm); layer?: profile layer name (default \"0\"); layers?: \"single\" | \"roles\" (default \"single\": all on `layer`; \"roles\": outer on \"OUTLINE\", holes on \"CUT\"); join?: bool (default false: R12 LINE/ARC; true: closed loops as LWPOLYLINE with arc bulges)"),
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
    Ok(String::from_utf8_lossy(&bytes).to_string())
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
fn bounds_of(geom: &[Geom2]) -> Option<(Vec2, Vec2)> {
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
            Geom2::Spline { pts: sp, .. } => pts.extend(sp.iter().copied()),
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

/// The drawing's geometry and the box around it; a drawing with none is an error that says why
/// (#9641).
fn read_drawing(p: &Value, cmd: &str, svg: bool) -> Result<(Vec<Geom2>, (Vec2, Vec2), String)> {
    let text = source_text(p, cmd, svg)?;
    let geom = if svg { solidcraft_io::read_svg(&text) } else { solidcraft_io::read_dxf(&text) }.map_err(|e| drawing_error(p, svg, e.to_string()))?;
    let Some(bounds) = bounds_of(&geom) else {
        let why = if svg { solidcraft_io::svg_nothing_to_import(&text) } else { "the DXF has no entities SolidCraft reads".to_string() };
        return Err(drawing_error(p, svg, why));
    };
    Ok((geom, bounds, text))
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
    let (_, bounds, text) = read_drawing(p, cmd, svg)?;
    // Only an SVG names a sheet, and only when it says how big its sheet is.
    let sheet = svg.then(|| solidcraft_io::svg_sheet(&text)).flatten().map(drawing_box);
    reply(cmd, DrawingBoundsResult { drawing: drawing_box(bounds), sheet, param_warnings: Vec::new() })
}

fn insert(s: &mut Session, p: &Value, cmd: &str, svg: bool) -> Result<Value> {
    let (geom, (lo, hi), text) = read_drawing(p, cmd, svg)?;
    // What `width`, `height` and `align: center` refer to: the drawn geometry, or the SVG sheet.
    let (lo, hi) = match str_(p, "size_from") {
        None | Some("geometry") => (lo, hi),
        Some("viewbox") if !svg => return Err(bad(cmd, "`size_from: viewbox` is for SVG drawings; a DXF has no sheet")),
        Some("viewbox") => solidcraft_io::svg_sheet(&text).ok_or_else(|| bad(cmd, "the SVG has no viewBox or width and height to size by"))?,
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
    let (sin, cos) = angle.sin_cos();
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
    let f = move |q: Vec2| {
        let v = (q - pivot) * k;
        Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos) + off
    };
    // A dense SVG/DXF (a floorplan) can yield far more entities than a sketch takes; refuse
    // before adding so the message names the cap (#10874). Scale alone does not shrink the count.
    if geom.len() > solidcraft_sketch::limits::max_points() {
        return Err(solidcraft_sketch::SketchError::TooLarge.into());
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

#[cfg(test)]
mod tests {
    use serde_json::json;
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
}
