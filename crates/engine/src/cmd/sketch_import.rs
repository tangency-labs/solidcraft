//! Sketch drawing exchange: Insert DXF, Insert SVG and sketch DXF export.

use std::collections::HashMap;

use serde_json::{Value, json};
use solidcraft_geom::bspline::{self, Fit, HPoint, NurbsCurve};
use solidcraft_geom::{MAX_BEZIER_DEGREE, Vec2, Vec3};
use solidcraft_io::{Geom2, MAX_DRAWING_BYTES};
use solidcraft_sketch::{CurveKind, LinkKind, LinkSource, MAX_SPLINE_POINTS, Sketch};

use super::CommandSpec;
use super::sketch::{edit, ids_of};
use super::sketch_store;
use crate::params::{bad, num, str_, vec2};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.insert_dxf", "Insert DXF", insert_dxf)
        .at("SKETCH", "INSERT")
        .icon("sketch_dxf")
        .params(concat!("path | text: ASCII DXF (blocks expanded); at?: [x,y] offset, scale? | width? | height?: mm, the drawing's size before rotation, aspect kept (win over scale); angle?: degrees, about the drawing's origin (or its centre); align?: \"origin\" (default: `at` is where the file's origin goes) | \"center\" (`at` is where the drawing's middle goes and it turns and scales about it); tolerance?: mm within which ends join (default 0.0001); plane?: (when no sketch is active, a new sketch on it; default XY)", created_doc!())),
    CommandSpec::new("sketch.insert_svg", "Insert SVG", insert_svg)
        .at("SKETCH", "INSERT")
        .icon("svg")
        .params(concat!("path | text: SVG; at?: [x,y] offset, scale? | width? | height?: mm, the drawing's size before rotation, aspect kept (win over scale); angle?: degrees, about the drawing's origin (or its centre); align?: \"origin\" (default: `at` is where the file's origin goes) | \"center\" (`at` is where the drawing's middle goes and it turns and scales about it); tolerance?: mm within which ends join (default 0.0001); plane?: (when no sketch is active, a new sketch on it; default XY)", created_doc!())),
    CommandSpec::new("sketch.drawing_bounds", "Drawing Size", drawing_bounds)
        .icon("svg")
        .noundo()
        .params("path | text: SVG or DXF (by `format`: \"svg\" | \"dxf\", default by the file name, else SVG); returns min, max, width and height in the file's own size"),
    CommandSpec::new("sketch.export_dxf", "Save As DXF", export_dxf).icon("sketch_dxf")
        .noundo()
        .params("sketch?: id|name (default active); path?: file to write (else the DXF text is returned)"),
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
/// ends almost meet still make closed profiles. Also returns how far that moved the end of a
/// spline or conic, which otherwise keeps the shape it was given (mm).
pub(super) fn add_geometry(sk: &mut Sketch, geom: &[Geom2], f: &dyn Fn(Vec2) -> Vec2, k: f64, q: f64) -> Result<(Vec<usize>, Vec<Geom2>, f64)> {
    let mut map: HashMap<(i64, i64), Vec<(Vec2, usize)>> = HashMap::new();
    let mut pt = |sk: &mut Sketch, p: Vec2| -> Result<usize> {
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
    // How far the point an end joined is from where the end was.
    let shift = |sk: &Sketch, p: Vec2, i: usize| sk.point(i).map_or(0.0, |o| o.dist(f(p)));
    let mut moved = 0.0f64;
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
                // Only the ends join other curves: an inner point moved would reshape the curve. A
                // through-point spline keeps one of two equal points in a row; a control point
                // repeated shapes the curve (a Bézier [A, A, B, C] pulls towards A), so stays.
                let mut pts: Vec<Vec2> = pts.iter().take(MAX_SPLINE_POINTS).copied().collect();
                if !*control {
                    pts.dedup();
                }
                match (pts.first(), pts.last()) {
                    (Some(&first), Some(&last)) if pts.iter().any(|q| *q != first) => {
                        let (a, b) = (pt(sk, first)?, pt(sk, last)?);
                        moved = moved.max(shift(sk, first, a)).max(shift(sk, last, b));
                        let inner = pts.get(1..pts.len() - 1).unwrap_or_default();
                        if inner.is_empty() && a == b {
                            None
                        } else {
                            let mut idx = vec![a];
                            for q in inner {
                                idx.push(sk.add_point(f(*q), None)?);
                            }
                            idx.push(b);
                            Some(CurveKind::Spline { pts: idx, control: *control, degree: *degree, periodic: false })
                        }
                    }
                    _ => None,
                }
            }
            Geom2::Nurbs(_) => return Err(EngineError::Other("a B-spline must be brought in as sketch curves first (split_nurbs)".into())),
            Geom2::Conic { a, apex, b, rho } => {
                // The apex shapes the curve: it joins nothing.
                let (a0, b0) = (*a, *b);
                let (a, b) = (pt(sk, a0)?, pt(sk, b0)?);
                moved = moved.max(shift(sk, a0, a)).max(shift(sk, b0, b));
                if a == b { None } else { Some(CurveKind::Conic { a, b, apex: sk.add_point(f(*apex), None)?, rho: *rho }) }
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
    Ok((curves, texts, moved))
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
            // A B-spline lies within its control points' hull.
            Geom2::Nurbs(curve) => pts.extend(curve.ctrl().filter_map(|h| h.project()).map(|q| Vec2::new(q.x, q.y))),
            // Points along the curve, not its apex, which can lie well outside it (an elliptical
            // arc of a DXF comes in as conics, #8155).
            Geom2::Conic { a, apex, b, rho } if *rho > 0.0 && *rho < 1.0 => {
                let seg = solidcraft_geom::Seg2::Conic { a: *a, apex: *apex, b: *b, w: rho / (1.0 - rho) };
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
fn read_drawing(p: &Value, cmd: &str, svg: bool) -> Result<(Vec<Geom2>, (Vec2, Vec2))> {
    let text = source_text(p, cmd, svg)?;
    let geom = if svg { solidcraft_io::read_svg(&text) } else { solidcraft_io::read_dxf(&text) }.map_err(|e| drawing_error(p, svg, e.to_string()))?;
    let Some(bounds) = bounds_of(&geom) else {
        let why = if svg { solidcraft_io::svg_nothing_to_import(&text) } else { "the DXF has no entities SolidCraft reads".to_string() };
        return Err(drawing_error(p, svg, why));
    };
    Ok((geom, bounds))
}

/// Within this (mm, in the sketch) of the file's curve, a B-spline comes in: half of it for the
/// curves that follow it, half for joining their ends to other curves. An end moved further to
/// join another (a join tolerance above that) is reported.
const FIT_TOLERANCE: f64 = 1e-3;
/// Most sketch curves one Bézier piece of a B-spline may become.
const MAX_PIECE_CURVES: usize = 4096;
/// Most sketch curves a drawing's B-splines may become together: a bound on the work and memory
/// a hostile file can ask for.
const MAX_SPLINE_CURVES: usize = 200_000;

/// A drawing's B-splines as the curves a sketch holds. One that is the sketch's own spline
/// ([`sketch_own`]) comes in as that one editable spline. Any other is split into Bézier pieces,
/// each followed within `tolerance` (mm, in the drawing) of the file's curve with the bound proven
/// ([`bspline::follow`]): a polynomial piece of degree at most 9 as a control-point spline (a line
/// at degree 1), a rational quadratic one as a conic, any other as cubic control-point splines.
/// That is every [`Geom2::Nurbs`], and every control-point spline with more points than a sketch
/// spline holds, which would otherwise lose the rest. Also what the import should say about them,
/// its distances in the sketch's millimetres (`k` per drawing millimetre), and how many curves
/// they became. A B-spline that can't be followed within the tolerance refuses the drawing, with
/// the reason, rather than bringing in another curve.
fn split_nurbs(geom: Vec<Geom2>, tolerance: f64, k: f64) -> std::result::Result<(Vec<Geom2>, Vec<String>, usize), String> {
    let bad = |e: &dyn std::fmt::Display| format!("a spline can't be brought in within {FIT_TOLERANCE} mm: {e}");
    let mut out = Vec::with_capacity(geom.len());
    let (mut fitted, mut worst, mut made) = (0usize, 0.0f64, 0usize);
    for g in geom {
        let curve = match g {
            Geom2::Nurbs(curve) => match sketch_own(&curve) {
                Some(own) => {
                    made += 1;
                    out.push(own);
                    continue;
                }
                None => curve,
            },
            Geom2::Spline { pts, control: true, degree } if pts.len() > MAX_SPLINE_POINTS => sketch_spline(&pts, degree).map_err(|e| bad(&e))?,
            g => {
                out.push(g);
                continue;
            }
        };
        for piece in curve.pieces().map_err(|e| bad(&e))? {
            if made >= MAX_SPLINE_CURVES {
                return Err(format!("the drawing's splines make more than {MAX_SPLINE_CURVES} curves"));
            }
            let fits = bspline::follow(&piece, tolerance, MAX_BEZIER_DEGREE, MAX_PIECE_CURVES.min(MAX_SPLINE_CURVES - made)).map_err(|e| bad(&e))?;
            // The piece itself: over its own control points, or a conic.
            let own_points: Vec<Vec3> = piece.ctrl().filter_map(|h| h.project()).collect();
            let own = match &fits[..] {
                [c] => matches!(c.value(), Fit::Conic { .. }) || *c.value() == Fit::Bezier(own_points),
                _ => false,
            };
            if !own {
                fitted += 1;
                worst = fits.iter().fold(worst, |w, c| w.max(c.bound()));
            }
            made += fits.len();
            out.extend(fits.into_iter().map(|c| sketch_curve(c.into_value())));
        }
    }
    let mut warnings = Vec::new();
    if fitted > 0 {
        warnings.push(format!(
            "{fitted} spline piece(s) have no exact sketch curve: each comes in as cubic splines, at most {:.1e} mm from the file's curve",
            worst * k
        ));
    }
    Ok((out, warnings, made))
}

/// The knots of the sketch's spline of degree `p` over `n > p` points on `[lo, hi]`: each end
/// `p + 1` times, the inner ones evenly spaced.
fn sketch_knots(p: usize, n: usize, lo: f64, hi: f64) -> Vec<f64> {
    let spans = n.saturating_sub(p).max(1);
    (0..n + p + 1).map(|i| lo + (hi - lo) * (i.saturating_sub(p).min(spans) as f64 / spans as f64)).collect()
}

/// The sketch's own spline of `degree` over `pts` (clamped, its inner knots evenly spaced, at the
/// degree its points allow), as a B-spline.
fn sketch_spline(pts: &[Vec2], degree: u8) -> bspline::SplineResult<NurbsCurve> {
    let n = pts.len();
    let p = usize::from(degree).clamp(1, MAX_BEZIER_DEGREE).min(n.saturating_sub(1)).max(1);
    let ctrl: Vec<HPoint> = pts.iter().map(|q| HPoint::weighted(Vec3::new(q.x, q.y, 0.0), 1.0)).collect();
    NurbsCurve::from_homogeneous(p, sketch_knots(p, n, 0.0, 1.0), &ctrl)
}

/// `curve` as one sketch spline when it is the sketch's own: degree at most 9, at most 500
/// points, its weights all alike (which is no weighting), its knots clamped and evenly spaced up
/// to the rounding of a number (4ε of the domain). Knots written to fewer digits than that are a
/// different curve, which comes in as its exact pieces.
fn sketch_own(curve: &NurbsCurve) -> Option<Geom2> {
    let p = curve.degree();
    let degree = u8::try_from(p).ok().filter(|_| p <= MAX_BEZIER_DEGREE)?;
    let ctrl: Vec<HPoint> = curve.ctrl().collect();
    let n = ctrl.len();
    if n > MAX_SPLINE_POINTS || ctrl.windows(2).any(|w| w[0].w != w[1].w) {
        return None;
    }
    let k = curve.knots();
    let (lo, hi) = (*k.get(p)?, *k.get(n)?);
    if !sketch_knots(p, n, lo, hi).iter().zip(k).all(|(a, b)| (a - b).abs() <= 4.0 * f64::EPSILON * (hi - lo)) {
        return None;
    }
    let pts = ctrl.iter().map(|h| h.project().map(|q| Vec2::new(q.x, q.y))).collect::<Option<Vec<_>>>()?;
    Some(Geom2::Spline { pts, control: true, degree })
}

/// A curve a sketch holds for a fit (z dropped: the file's curves have z = 0).
fn sketch_curve(fit: Fit) -> Geom2 {
    let flat = |q: Vec3| Vec2::new(q.x, q.y);
    match fit {
        Fit::Bezier(p) => match &p[..] {
            [a, b] => Geom2::Line(flat(*a), flat(*b)),
            _ => Geom2::Spline {
                degree: u8::try_from(p.len().saturating_sub(1)).unwrap_or(u8::MAX),
                control: true,
                pts: p.into_iter().map(flat).collect(),
            },
        },
        Fit::Conic { a, apex, b, rho } => Geom2::Conic { a: flat(a), apex: flat(apex), b: flat(b), rho },
    }
}

/// How big a drawing is in its own file, to size it before it is inserted.
fn drawing_bounds(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.drawing_bounds";
    let name_is_dxf = str_(p, "path").is_some_and(|x| x.to_ascii_lowercase().ends_with(".dxf"));
    let svg = match str_(p, "format") {
        Some(f) => !f.eq_ignore_ascii_case("dxf"),
        None => !name_is_dxf,
    };
    let (_, (lo, hi)) = read_drawing(p, cmd, svg)?;
    Ok(json!({"min": [lo.x, lo.y], "max": [hi.x, hi.y], "width": hi.x - lo.x, "height": hi.y - lo.y}))
}

fn insert(s: &mut Session, p: &Value, cmd: &str, svg: bool) -> Result<Value> {
    let (geom, (lo, hi)) = read_drawing(p, cmd, svg)?;
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
    // The sketch's curves are the drawing's scaled by `k`: half of FIT_TOLERANCE there.
    let (geom, mut warnings, from_bsplines) = split_nurbs(geom, FIT_TOLERANCE / 2.0 / k, k).map_err(|e| drawing_error(p, svg, e))?;
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
    let ((curves, texts, moved), info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, texts, moved) = add_geometry(sk, &geom, &f, k, tol)?;
        Ok((ids_of(sk, &cs), texts, moved))
    })?;
    if from_bsplines > 0 && moved > FIT_TOLERANCE / 2.0 {
        warnings.push(format!(
            "curve ends moved up to {moved:.1e} mm to join others (the join tolerance is {tol} mm), so a spline from the file can be that far from it at an end"
        ));
    }
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
    Ok(json!({"sketch": id, "created_sketch": created, "curves": curves.len(), "texts": n_text, "warnings": warnings, "info": info}))
}

fn insert_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    insert(s, p, "sketch.insert_dxf", false)
}

fn insert_svg(s: &mut Session, p: &Value) -> Result<Value> {
    insert(s, p, "sketch.insert_svg", true)
}

fn export_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.export_dxf";
    let id = sketch_store::target(s, p, cmd)?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
    let sk = &ss.sketch;
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
    let text = solidcraft_io::write_dxf(&geom);
    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        return Ok(json!({"path": path, "entities": geom.len()}));
    }
    Ok(json!({"dxf": text, "entities": geom.len()}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::{CurveKind, Sketch};

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

    /// A degree-`p` SPLINE over `ctrl` with `knots` and, when given, `weights`.
    fn spline_dxf(p: usize, ctrl: &[[f64; 2]], knots: &[f64], weights: &[f64]) -> String {
        let mut body = format!("0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n{p}\n");
        body.extend(knots.iter().map(|k| format!("40\n{k}\n")));
        body.extend(weights.iter().map(|w| format!("41\n{w}\n")));
        body.extend(ctrl.iter().map(|[x, y]| format!("10\n{x}\n20\n{y}\n")));
        body + "0\nENDSEC\n0\nEOF\n"
    }

    /// De Boor's evaluation, in homogeneous coordinates when there are weights, both factors of
    /// each step from knot differences: the B-spline at `u`, independent of the importer.
    fn de_boor(ctrl: &[[f64; 2]], weights: &[f64], knots: &[f64], p: usize, u: f64) -> Vec2 {
        let k = (p..ctrl.len()).rev().find(|&k| knots[k] <= u && knots[k] < knots[k + 1]).unwrap();
        let w = |i: usize| if weights.is_empty() { 1.0 } else { weights[i] };
        let mut d: Vec<[f64; 3]> = (0..=p).map(|j| j + k - p).map(|i| [ctrl[i][0] * w(i), ctrl[i][1] * w(i), w(i)]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + k - p;
                let (lo, hi) = (knots[i], knots[i + p + 1 - r]);
                let (f, g) = ((hi - u) / (hi - lo), (u - lo) / (hi - lo));
                d[j] = [0, 1, 2].map(|c| d[j - 1][c] * f + d[j][c] * g);
            }
        }
        Vec2::new(d[p][0] / d[p][2], d[p][1] / d[p][2])
    }

    fn insert_scaled(dxf: &str, scale: f64) -> Result<(serde_json::Value, Sketch, usize), String> {
        insert_with(json!({"text": dxf, "scale": scale}))
    }

    fn insert_with(params: serde_json::Value) -> Result<(serde_json::Value, Sketch, usize), String> {
        let mut s = Session::default();
        let r = s.execute("sketch.insert_dxf", &params).map_err(|e| e.to_string())?;
        let ss = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
        Ok((r, ss.sketch, ss.profiles.len()))
    }

    fn insert_one(dxf: &str) -> (serde_json::Value, Sketch, usize) {
        insert_scaled(dxf, 1.0).unwrap()
    }

    /// Sketch curve `i` at `t ∈ [0, 1]`.
    fn curve_point(sk: &Sketch, i: usize, t: f64) -> Vec2 {
        let at = |j: usize| sk.point(j).unwrap();
        match &sk.curves[i].kind {
            CurveKind::Line { a, b } => at(*a) + (at(*b) - at(*a)) * t,
            CurveKind::Spline { pts, control, degree, .. } => {
                solidcraft_sketch::spline_point(&pts.iter().map(|j| at(*j)).collect::<Vec<_>>(), *control, *degree, t)
            }
            CurveKind::Conic { a, b, apex, rho } => solidcraft_sketch::conic_point(at(*a), at(*apex), at(*b), *rho, t),
            k => panic!("{k:?}"),
        }
    }

    /// The distance from `x` to the curve `f` over `[lo, hi]`: samples spread along the curve, not
    /// its parameter (a rational curve can run much of its length in a sliver of it), and closer
    /// together the nearer they come to `x` (a curve can loop tighter than the spacing), each one
    /// closer than its neighbours refined by golden-section search. Never less than the true
    /// distance (each is to a point on the curve), so a check against it can't pass wrongly.
    fn closest(f: &dyn Fn(f64) -> Vec2, lo: f64, hi: f64, x: Vec2) -> f64 {
        let mut s: Vec<f64> = (0..=64).map(|i| lo + (hi - lo) * f64::from(i) / 64.0).collect();
        let mut p: Vec<Vec2> = s.iter().map(|&t| f(t)).collect();
        let length: f64 = p.windows(2).map(|w| w[0].dist(w[1])).sum();
        for _ in 0..60 {
            let (mut ns, mut np) = (vec![s[0]], vec![p[0]]);
            for i in 1..s.len() {
                let near = 0.5 * p[i - 1].dist(x).min(p[i].dist(x));
                if p[i - 1].dist(p[i]) > near.min(length / 512.0) && s[i] - s[i - 1] > 1e-13 * (hi - lo) && ns.len() < 20_000 {
                    let mid = 0.5 * (s[i - 1] + s[i]);
                    ns.push(mid);
                    np.push(f(mid));
                }
                ns.push(s[i]);
                np.push(p[i]);
            }
            if ns.len() == s.len() {
                break;
            }
            (s, p) = (ns, np);
        }
        let d: Vec<f64> = p.iter().map(|q| q.dist(x)).collect();
        let last = d.len() - 1;
        let mut found = d.iter().copied().fold(f64::INFINITY, f64::min);
        for i in (0..=last).filter(|&i| d[i] <= d[i.saturating_sub(1)] && d[i] <= d[(i + 1).min(last)]) {
            let (mut a, mut b) = (s[i.saturating_sub(1)], s[(i + 1).min(last)]);
            for _ in 0..80 {
                let (m1, m2) = (a + (b - a) * 0.381_966, b - (b - a) * 0.381_966);
                let (d1, d2) = (f(m1).dist(x), f(m2).dist(x));
                found = found.min(d1).min(d2);
                if d1 < d2 { b = m2 } else { a = m1 }
            }
        }
        found
    }

    /// Every point of the file's curve (scaled by `k`) within `allowed` of the sketch's curves from
    /// `from` on, and the points of the first, middle and last of those within it of the file's
    /// curve.
    fn assert_within(sk: &Sketch, from: usize, ctrl: &[[f64; 2]], weights: &[f64], knots: &[f64], p: usize, k: f64, allowed: f64) {
        let (lo, hi) = (knots[p], knots[ctrl.len()]);
        let file = |u: f64| de_boor(ctrl, weights, knots, p, u) * k;
        for j in 0..=40 {
            let x = file(lo + (hi - lo) * f64::from(j) / 40.0);
            let off = (from..sk.curves.len()).map(|i| closest(&|t| curve_point(sk, i, t), 0.0, 1.0, x)).fold(f64::INFINITY, f64::min);
            assert!(off <= allowed, "file point {j}/40 is {off:e} from the sketch's curves");
        }
        let last = sk.curves.len() - 1;
        for i in [from, (from + last) / 2, last] {
            for j in 0..=4 {
                let y = curve_point(sk, i, f64::from(j) / 4.0);
                let off = closest(&file, lo, hi, y);
                assert!(off <= allowed, "curve {i} at {j}/4 is {off:e} from the file's curve");
            }
        }
    }

    /// #9355: a SPLINE with uneven knots, a double knot or unclamped closed knots comes in as the
    /// curve its own knots describe: one control-point spline per span of its domain, each on the
    /// file's curve (by de Boor) all along, not one spline over the points with even knots. A
    /// repeated control point stays repeated, in a plain Bézier and in an exact piece alike.
    #[test]
    fn a_spline_comes_in_as_its_own_knots_describe() {
        let open = [[0.0, 0.0], [10.0, 25.0], [30.0, 30.0], [45.0, 5.0], [60.0, 20.0], [80.0, 0.0]];
        let (a, b, c) = ([0.0, 0.0], [20.0, 0.0], [10.0, 20.0]);
        let closed = [a, b, c, a, b, c];
        let bezier = [a, a, b, c];
        let repeated = [a, a, b, c, [30.0, 5.0]];
        for (ctrl, knots, closes) in [
            (&open[..], vec![0.0, 0.0, 0.0, 0.0, 0.1, 0.15, 1.0, 1.0, 1.0, 1.0], false),
            (&open, vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 1.0, 1.0, 1.0, 1.0], false),
            (&closed, (0..10).map(f64::from).collect(), true),
            (&bezier, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
            (&repeated, vec![0.0, 0.0, 0.0, 0.0, 0.3, 1.0, 1.0, 1.0, 1.0], false),
        ] {
            let (r, sk, profiles) = insert_one(&spline_dxf(3, ctrl, &knots, &[]));
            assert_eq!(r["warnings"], json!([]), "{r}");
            let spans: Vec<(f64, f64)> = knots[3..=ctrl.len()].windows(2).map(|w| (w[0], w[1])).filter(|(a, b)| a < b).collect();
            assert_eq!(sk.curves.len(), spans.len(), "{knots:?}");
            for (i, (lo, hi)) in spans.into_iter().enumerate() {
                let CurveKind::Spline { pts, control: true, degree: 3, .. } = &sk.curves[i].kind else { panic!("{:?}", sk.curves[i].kind) };
                // A cubic's piece: four control points, repeated ones included.
                assert_eq!(pts.len(), 4, "{knots:?}: {pts:?}");
                for j in 0..=16 {
                    let t = f64::from(j) / 16.0;
                    let (got, want) = (curve_point(&sk, i, t), de_boor(ctrl, &[], &knots, 3, lo + (hi - lo) * t));
                    assert!(got.dist(want) < 1e-9, "{knots:?} at {t} of [{lo}, {hi}]: {got:?} vs {want:?}");
                }
            }
            assert_eq!(profiles, usize::from(closes), "{knots:?}");
        }
    }

    /// A SPLINE that is the sketch's own (clamped, evenly spaced knots, no weights or all alike)
    /// comes in as one editable spline over its points, as it did before; knots written to six
    /// digits are a different curve and come in as its exact pieces.
    #[test]
    fn the_sketchs_own_spline_comes_in_as_one_spline() {
        let five = [[0.0, 0.0], [10.0, 20.0], [30.0, 20.0], [40.0, 0.0], [60.0, 10.0]];
        let even = [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
        for weights in [&[][..], &[2.0; 5]] {
            let (r, sk, _) = insert_one(&spline_dxf(3, &five, &even, weights));
            let [curve] = &sk.curves[..] else { panic!("{r}") };
            let CurveKind::Spline { pts, control: true, degree: 3, .. } = &curve.kind else { panic!("{:?}", curve.kind) };
            let pts: Vec<Vec2> = pts.iter().map(|i| sk.point(*i).unwrap()).collect();
            assert!(pts.iter().zip(&five).all(|(p, q)| p.dist(Vec2::new(q[0], q[1])) < 1e-12), "{pts:?}");
        }
        let six = [[0.0, 0.0], [10.0, 20.0], [30.0, 20.0], [40.0, 0.0], [60.0, 10.0], [70.0, 5.0]];
        let thirds = [0.0, 0.0, 0.0, 0.0, 0.333333, 0.666667, 1.0, 1.0, 1.0, 1.0];
        let (_, sk, _) = insert_one(&spline_dxf(3, &six, &thirds, &[]));
        assert_eq!(sk.curves.len(), 3);
        assert_within(&sk, 0, &six, &[], &thirds, 3, 1.0, 1e-3);
    }

    /// #9355: a SPLINE with more control points than a sketch spline holds (500) comes in whole,
    /// as exact pieces on the file's curve from its first point to its last: with uneven knots,
    /// with even ones, and with none given (the sketch's even knots).
    #[test]
    fn a_spline_past_five_hundred_control_points_comes_in_whole() {
        let n = 520;
        let ctrl: Vec<[f64; 2]> = (0..n).map(|i| [i as f64, 10.0 * (i as f64 / 7.0).sin()]).collect();
        let even: Vec<f64> = (0..n + 4).map(|i| (i.clamp(3, n) - 3) as f64 / (n - 3) as f64).collect();
        let uneven: Vec<f64> = (0..n + 4).map(|i| ((i.clamp(3, n) - 3) as f64 / (n - 3) as f64).powi(2)).collect();
        for (given, knots) in [(&uneven[..], &uneven), (&even, &even), (&[][..], &even)] {
            let (r, sk, _) = insert_one(&spline_dxf(3, &ctrl, given, &[]));
            assert_eq!((r["curves"].as_u64(), &r["warnings"]), (Some(n as u64 - 3), &json!([])), "{} knots given", given.len());
            for i in 0..sk.curves.len() {
                assert!(matches!(sk.curves[i].kind, CurveKind::Spline { control: true, degree: 3, .. }), "{:?}", sk.curves[i].kind);
                let (lo, hi) = (knots[i + 3], knots[i + 4]);
                for j in 0..=4 {
                    let t = f64::from(j) / 4.0;
                    let (got, want) = (curve_point(&sk, i, t), de_boor(&ctrl, &[], knots, 3, lo + (hi - lo) * t));
                    assert!(got.dist(want) < 1e-9, "{} knots given, piece {i} at {t}: {got:?} vs {want:?}", given.len());
                }
            }
        }
    }

    /// #9355: weighted quadratics (how CAD programs write arcs and circles) come in as conics on
    /// the circle all along: a quarter, and a whole circle of four, closed.
    #[test]
    fn a_weighted_quadratic_spline_comes_in_as_conics_on_its_circle() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let quarter = spline_dxf(2, &[[20.0, 0.0], [20.0, 20.0], [0.0, 20.0]], &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[1.0, h, 1.0]);
        let ctrl = [[20.0, 0.0], [20.0, 20.0], [0.0, 20.0], [-20.0, 20.0], [-20.0, 0.0], [-20.0, -20.0], [0.0, -20.0], [20.0, -20.0], [20.0, 0.0]];
        let knots = [0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
        let circle = spline_dxf(2, &ctrl, &knots, &[1.0, h, 1.0, h, 1.0, h, 1.0, h, 1.0]);
        for (dxf, conics, profiles) in [(quarter, 1, 0), (circle, 4, 1)] {
            let (r, sk, found) = insert_one(&dxf);
            assert_eq!((r["curves"].as_u64(), found, &r["warnings"]), (Some(conics), profiles, &json!([])), "{r}");
            for i in 0..sk.curves.len() {
                assert!(matches!(sk.curves[i].kind, CurveKind::Conic { .. }), "{:?}", sk.curves[i].kind);
                let worst = (0..=64).map(|j| (curve_point(&sk, i, f64::from(j) / 64.0).len() - 20.0).abs()).fold(0.0, f64::max);
                assert!(worst < 1e-9, "{worst}");
            }
        }
    }

    /// #9355: a weighted cubic has no exact sketch curve: it comes in as cubic splines within
    /// 0.001 mm of it in the sketch, scaled up or not, and the import says how close.
    #[test]
    fn a_weighted_cubic_spline_comes_in_within_its_tolerance_and_the_import_says_so() {
        let (ctrl, weights, knots) =
            ([[0.0, 0.0], [5.0, 30.0], [40.0, 30.0], [50.0, 0.0]], [1.0, 3.0, 0.5, 1.0], [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        for scale in [1.0, 40.0] {
            let (r, sk, _) = insert_scaled(&spline_dxf(3, &ctrl, &knots, &weights), scale).unwrap();
            assert!(sk.curves.iter().all(|c| matches!(c.kind, CurveKind::Spline { control: true, degree: 3, .. })), "{r}");
            let warnings = r["warnings"].as_array().unwrap();
            assert!(warnings.len() == 1 && warnings[0].as_str().unwrap().starts_with("1 spline piece(s) have no exact sketch curve"), "{r}");
            assert_within(&sk, 0, &ctrl, &weights, &knots, 3, scale, 1e-3);
        }
    }

    /// A weighted quadratic so sharp (middle weight 2·10⁹, so 1 − rho below 10⁻⁹) that the sketch
    /// would hold its conic at another weight comes in as cubics within 0.001 mm, not as that conic.
    #[test]
    fn a_quadratic_sharper_than_a_sketch_conic_comes_in_as_cubics() {
        let (ctrl, weights, knots) = ([[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0]], [1.0, 2e9, 1.0], [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let (r, sk, _) = insert_one(&spline_dxf(2, &ctrl, &knots, &weights));
        assert!(sk.curves.iter().all(|c| matches!(c.kind, CurveKind::Spline { control: true, degree: 3, .. })), "{r}");
        assert_within(&sk, 0, &ctrl, &weights, &knots, 2, 1.0, 1e-3);
    }

    /// A spline's end joining a line's end 0.01 mm away, with a join tolerance of 0.05 mm, moves
    /// that far: the import says so, for a weighted cubic split into pieces and for the sketch's
    /// own spline alike. With the default tolerance it doesn't join, and the spline keeps within
    /// 0.001 mm.
    #[test]
    fn a_spline_end_moved_further_than_the_tolerance_to_join_is_reported() {
        let (ctrl, knots) = ([[0.0, 0.0], [5.0, 30.0], [40.0, 30.0], [50.0, 0.0]], [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        let line = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n-20\n20\n0\n11\n0.01\n21\n0\n";
        for weights in [&[1.0, 3.0, 0.5, 1.0][..], &[]] {
            let dxf = line.to_string() + spline_dxf(3, &ctrl, &knots, weights).trim_start_matches("0\nSECTION\n2\nENTITIES\n");
            let (r, sk, _) = insert_with(json!({"text": dxf, "tolerance": 0.05})).unwrap();
            let warned = |r: &serde_json::Value, start: &str| {
                r["warnings"].as_array().unwrap().iter().filter(|w| w.as_str().unwrap().starts_with(start)).count()
            };
            assert_eq!(warned(&r, "curve ends moved up to 1.0e-2 mm"), 1, "{weights:?}: {r}");
            let (r, sk2, _) = insert_with(json!({"text": dxf})).unwrap();
            assert_eq!(warned(&r, "curve ends moved"), 0, "{weights:?}: {r}");
            assert_within(&sk2, 1, &ctrl, weights, &knots, 3, 1.0, 1e-3);
            assert!(sk.curves.len() == sk2.curves.len());
        }
    }

    /// An inner control point of a spline joins no other curve, even one ending right on it: it
    /// shapes the curve, and moving it would move the curve. The ends still join.
    #[test]
    fn an_inner_spline_point_joins_no_other_curve() {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n100\n20\n100\n11\n10\n21\n25\n0\nLINE\n10\n45\n20\n5\n11\n90\n21\n5\n\
                   0\nSPLINE\n71\n3\n10\n0\n20\n0\n10\n10\n20\n25\n10\n30\n20\n30\n10\n45\n20\n5\n0\nENDSEC\n0\nEOF\n";
        let (_, sk, _) = insert_one(dxf);
        let (CurveKind::Line { b: inner_end, .. }, CurveKind::Line { a: end, .. }, CurveKind::Spline { pts, .. }) =
            (&sk.curves[0].kind, &sk.curves[1].kind, &sk.curves[2].kind)
        else {
            panic!("{:?}", sk.curves)
        };
        assert!(pts[1] != *inner_end && pts[3] == *end, "{pts:?}");
    }

    /// Random SPLINEs of degree 1 to 9 (now and then 11), clamped or not, inner knots repeated up
    /// to and past the degree, weights up to 100 apart, inserted at three scales: each comes in
    /// within 0.001 mm of the file's curve in the sketch, both ways, or is refused with the reason.
    #[test]
    fn random_splines_come_in_within_the_tolerance_or_are_refused() {
        let mut rng = 0x9355_u64;
        let mut unit = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 11) as f64 / (1u64 << 53) as f64
        };
        let (mut came, mut refused) = (0, 0);
        for _ in 0..150 {
            let p = if unit() < 0.1 { 11 } else { 1 + (unit() * 9.0) as usize };
            let n = p + 1 + (unit() * 8.0) as usize;
            let clamped = unit() < 0.6;
            let mut knots = Vec::with_capacity(n + p + 1);
            let mut u = 0.0;
            for i in 0..n + p + 1 {
                if i > 0 && !(clamped && (i <= p || i > n)) && !(i > p && i < n && unit() < 0.25) {
                    u += 0.05 + unit();
                }
                knots.push(u);
            }
            let ctrl: Vec<[f64; 2]> = (0..n).map(|_| [100.0 * unit() - 50.0, 100.0 * unit() - 50.0]).collect();
            let weights: Vec<f64> = if unit() < 0.5 { Vec::new() } else { (0..n).map(|_| 10f64.powf(2.0 * unit() - 1.0)).collect() };
            let scale = [1.0, 7.5, 0.02][(unit() * 3.0) as usize];
            match insert_scaled(&spline_dxf(p, &ctrl, &knots, &weights), scale) {
                Ok((_, sk, _)) => {
                    assert_within(&sk, 0, &ctrl, &weights, &knots, p, scale, 1e-3 * (1.0 + 1e-6) + 1e-12 * 100.0 * scale);
                    came += 1;
                }
                Err(e) => {
                    assert!(e.contains("spline") || e.contains("SPLINE"), "{e}");
                    refused += 1;
                }
            }
        }
        assert!(came > 100 && refused > 5, "came in {came}, refused {refused}");
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
}
