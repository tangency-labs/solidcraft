//! Sketch drawing exchange: Insert DXF, Insert SVG and sketch DXF export.

use std::collections::HashMap;

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_io::{Geom2, MAX_DRAWING_BYTES};
use solidcraft_sketch::{CurveKind, LinkKind, LinkSource, Sketch};

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
/// ends almost meet still make closed profiles.
pub(super) fn add_geometry(sk: &mut Sketch, geom: &[Geom2], f: &dyn Fn(Vec2) -> Vec2, k: f64, q: f64) -> Result<(Vec<usize>, Vec<Geom2>)> {
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
                for p in pts.iter().take(solidcraft_sketch::MAX_SPLINE_POINTS) {
                    let i = pt(sk, *p)?;
                    if idx.last() != Some(&i) {
                        idx.push(i);
                    }
                }
                (idx.len() >= 2).then_some(CurveKind::Spline { pts: idx, control: *control, degree: *degree, periodic: false })
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
    let ((curves, texts), info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, texts) = add_geometry(sk, &geom, &f, k, tol)?;
        Ok((ids_of(sk, &cs), texts))
    })?;
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
    Ok(json!({"sketch": id, "created_sketch": created, "curves": curves.len(), "texts": n_text, "info": info}))
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
