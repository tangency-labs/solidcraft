//! Copper, solder-mask and silkscreen layers of an inserted board (#12797).
//!
//! Each layer's area is read exactly (`solidcraft_io::read_gerber_layer`) into hidden sketches in
//! the board's component, where later work (machining) can use it. Each side's layers are also
//! drawn into one image, laid on that side of the board, so the board looks like a made one:
//! green mask, lighter where copper runs under it, exposed copper tinned or gold, white print.
//! The image is display only; nothing is decided from it.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_io::{Geom2, MAX_DRAWING_BYTES};
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Transform};

use super::board_import::{CMD, file_name, read_text, run};
use super::sketch::edit;
use super::sketch_import::add_geometry;
use crate::params::bad;
use crate::{EngineError, Result, Session};

/// What a layer is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Copper,
    Mask,
    Silk,
}

/// A layer the import takes: its parameter key, its sketch's name, its side and kind.
#[derive(Clone, Copy, Debug)]
pub(super) struct Role {
    pub key: &'static str,
    pub name: &'static str,
    pub top: bool,
    pub kind: Kind,
}

pub(super) const ROLES: [Role; 6] = [
    Role { key: "top_copper", name: "Top copper", top: true, kind: Kind::Copper },
    Role { key: "bottom_copper", name: "Bottom copper", top: false, kind: Kind::Copper },
    Role { key: "top_mask", name: "Top mask", top: true, kind: Kind::Mask },
    Role { key: "bottom_mask", name: "Bottom mask", top: false, kind: Kind::Mask },
    Role { key: "top_silk", name: "Top silkscreen", top: true, kind: Kind::Silk },
    Role { key: "bottom_silk", name: "Bottom silkscreen", top: false, kind: Kind::Silk },
];

/// A layer read from its file.
pub(super) struct Layer {
    pub role: Role,
    /// Where it came from, for the component's properties.
    pub file: String,
    pub loops: Vec<Vec<Geom2>>,
}

/// The finish on copper the mask leaves bare.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Finish {
    /// Hot-air solder levelling: tin, silver-grey.
    #[default]
    Hasl,
    /// Electroless nickel, immersion gold.
    Enig,
}

/// The `layers` (paths) and `layers_text` objects and `finish`, checked; no file is read yet.
pub(super) fn params(p: &Value) -> Result<(Vec<(Role, Source<'_>)>, Finish)> {
    let finish = match p.get("finish") {
        None | Some(Value::Null) => Finish::Hasl,
        Some(Value::String(f)) if f == "hasl" => Finish::Hasl,
        Some(Value::String(f)) if f == "enig" => Finish::Enig,
        Some(_) => return Err(bad(CMD, "`finish` must be \"hasl\" or \"enig\"")),
    };
    let mut out = Vec::new();
    for (key, text) in [("layers", false), ("layers_text", true)] {
        let obj = match p.get(key) {
            None | Some(Value::Null) => continue,
            Some(Value::Object(o)) => o,
            Some(_) => return Err(bad(CMD, format!("`{key}` must be an object of layers ({})", keys()))),
        };
        for (k, v) in obj {
            let role = ROLES
                .iter()
                .find(|r| r.key == k)
                .copied()
                .ok_or_else(|| bad(CMD, format!("`{key}` has no layer `{k}`; the layers are {}", keys())))?;
            if out.iter().any(|(r, _): &(Role, Source)| r.key == role.key) {
                return Err(bad(CMD, format!("the layer `{k}` is given twice")));
            }
            let s = v.as_str().filter(|s| !s.trim().is_empty()).ok_or_else(|| bad(CMD, format!("`{key}.{k}` must be text")))?;
            if text && s.len() > MAX_DRAWING_BYTES {
                return Err(bad(CMD, format!("the layer `{k}` is too large")));
            }
            out.push((role, if text { Source::Text(s) } else { Source::Path(s) }));
        }
    }
    Ok((out, finish))
}

fn keys() -> String {
    ROLES.iter().map(|r| r.key).collect::<Vec<_>>().join(", ")
}

/// Where a layer's Gerber text comes from.
pub(super) enum Source<'a> {
    Path(&'a str),
    Text(&'a str),
}

/// Read the layers' files; their warnings go to `warnings`, named by file.
pub(super) fn read(given: &[(Role, Source)], warnings: &mut Vec<String>) -> Result<Vec<Layer>> {
    let mut out = Vec::with_capacity(given.len());
    for (role, src) in given {
        let (label, text) = match src {
            Source::Path(path) => (file_name(path), read_text(path)?),
            Source::Text(t) => (format!("{} text", role.name.to_lowercase()), (*t).to_string()),
        };
        let layer = solidcraft_io::read_gerber_layer(&text).map_err(|e| EngineError::Other(format!("{label}: {e}")))?;
        warnings.extend(layer.warnings.iter().map(|w| format!("{label}: {w}")));
        out.push(Layer { role: *role, file: label, loops: layer.loops });
    }
    Ok(out)
}

/// Most sketch points one layer sketch holds; a layer with more is split over several sketches.
const POINTS_PER_SKETCH: usize = 15_000;

/// The layers' hidden sketches, top layers on the top face and bottom ones on the bottom face,
/// made in the active component (the board's). Returns their ids.
pub(super) fn sketches(s: &mut Session, layers: &[Layer], offset: Vec2, thickness: f64) -> Result<Vec<u64>> {
    let shift = move |q: Vec2| Vec2::new(q.x + offset.x, q.y + offset.y);
    let mut ids = Vec::new();
    for layer in layers {
        // Loops in sketches of a size the sketch can take.
        let mut chunks: Vec<Vec<Geom2>> = vec![Vec::new()];
        for l in &layer.loops {
            let full = chunks.last().is_some_and(|c| c.len() + l.len() > POINTS_PER_SKETCH);
            if full {
                chunks.push(Vec::new());
            }
            if let Some(c) = chunks.last_mut() {
                c.extend(l.iter().cloned());
            }
        }
        chunks.retain(|c| !c.is_empty());
        let many = chunks.len() > 1;
        for (i, chunk) in chunks.iter().enumerate() {
            let name = if many { format!("{} {}", layer.role.name, i + 1) } else { layer.role.name.to_string() };
            let z = if layer.role.top { thickness } else { 0.0 };
            let made = run(s, "sketch.create", &json!({"plane": "XY", "offset": format!("{z} mm"), "name": name, "project_edges": false}))?;
            let sketch = made.get("sketch").and_then(Value::as_u64).ok_or_else(|| EngineError::Other(format!("the sketch `{name}` was not made")))?;
            edit(s, &json!({"sketch": sketch}), CMD, false, |sk, _| {
                add_geometry(sk, chunk, &shift, 1.0, 1e-6)?;
                Ok(())
            })?;
            run(s, "sketch.finish", &json!({}))?;
            if !s.visibility.hidden_sketches.contains(&sketch) {
                s.visibility.hidden_sketches.push(sketch);
            }
            ids.push(sketch);
        }
    }
    Ok(ids)
}

/// The vias' hidden sketch, `Vias`, on the top face, made in the active component.
pub(super) fn via_sketch(s: &mut Session, vias: &[Geom2], offset: Vec2, thickness: f64) -> Result<u64> {
    let shift = move |q: Vec2| Vec2::new(q.x + offset.x, q.y + offset.y);
    let made = run(s, "sketch.create", &json!({"plane": "XY", "offset": format!("{thickness} mm"), "name": "Vias", "project_edges": false}))?;
    let sketch = made.get("sketch").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("the sketch `Vias` was not made".into()))?;
    edit(s, &json!({"sketch": sketch}), CMD, false, |sk, _| {
        add_geometry(sk, vias, &shift, 1.0, 1e-6)?;
        Ok(())
    })?;
    run(s, "sketch.finish", &json!({}))?;
    if !s.visibility.hidden_sketches.contains(&sketch) {
        s.visibility.hidden_sketches.push(sketch);
    }
    Ok(sketch)
}

// ---- the look ----

/// Finest the image gets, in pixels per mm (0.05 mm a pixel), and its largest side.
const MAX_PX_PER_MM: f64 = 20.0;
const MAX_SIDE: f64 = 4096.0;

const MASK: [f64; 3] = [29.0, 107.0, 64.0];
const MASK_ON_COPPER: [f64; 3] = [62.0, 142.0, 74.0];
const BARE_LAMINATE: [f64; 3] = [190.0, 172.0, 118.0];
const TIN: [f64; 3] = [201.0, 204.0, 208.0];
const GOLD: [f64; 3] = [216.0, 178.0, 92.0];
const PRINT: [f64; 3] = [240.0, 240.0, 232.0];
/// A via's hole, seen into: dark, its plated wall barely lit.
const VIA_HOLE: [f64; 3] = [38.0, 30.0, 22.0];

/// One side's image and where it goes, in the files' coordinates.
pub(super) struct Look {
    pub png: Vec<u8>,
    /// Its width and centre, mm.
    pub width: f64,
    pub centre: Vec2,
}

/// Draw one side: `board` is the outline and the holes (closed contours; even-odd is the
/// material). Top images are seen from above (rows run down world y); bottom ones from below,
/// where the face's x is world x and its y is world -y (rows run up world y).
pub(super) fn look(board: &[Vec<Geom2>], vias: &[Geom2], layers: &[Layer], top: bool, finish: Finish) -> Option<Look> {
    let side = |k: Kind| layers.iter().find(|l| l.role.top == top && l.role.kind == k).map(|l| l.loops.as_slice());
    let (copper, mask, silk) = (side(Kind::Copper), side(Kind::Mask), side(Kind::Silk));
    if copper.is_none() && mask.is_none() && silk.is_none() {
        return None;
    }
    let board_pts: Vec<Vec<Vec2>> = board.iter().map(|l| flatten(l, 0.01)).collect();
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in board_pts.iter().flatten() {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let ppm = MAX_PX_PER_MM.min(MAX_SIDE / w.max(h));
    let (pw, ph) = ((w * ppm).ceil().max(1.0) as u32, (h * ppm).ceil().max(1.0) as u32);
    let (wmm, hmm) = (f64::from(pw) / ppm, f64::from(ph) / ppm);
    let to_px = |p: Vec2| -> (f32, f32) {
        let x = (p.x - lo.x) * ppm;
        let y = if top { (hi.y - p.y) * ppm } else { (p.y - lo.y) * ppm };
        (x as f32, y as f32)
    };
    let tol = 0.25 / ppm;
    let cover = |loops: Option<&[Vec<Geom2>]>| -> Option<Mask> {
        let mut m = Mask::new(pw, ph)?;
        if let Some(loops) = loops {
            let pts: Vec<Vec<Vec2>> = loops.iter().map(|l| flatten(l, tol)).collect();
            if let Some(path) = path(&pts, &to_px) {
                m.fill_path(&path, FillRule::EvenOdd, true, Transform::identity());
            }
        }
        Some(m)
    };
    let material = cover(None).and_then(|mut m| {
        let pts: Vec<Vec<Vec2>> = board.iter().map(|l| flatten(l, tol)).collect();
        path(&pts, &to_px).map(|p| {
            m.fill_path(&p, FillRule::EvenOdd, true, Transform::identity());
            m
        })
    })?;
    let (cu, open, print) = (cover(copper)?, cover(mask)?, cover(silk)?);
    let via_loops: Vec<Vec<Geom2>> = vias.iter().map(|g| vec![g.clone()]).collect();
    let holes = cover(Some(&via_loops))?;
    let bare = match finish {
        Finish::Hasl => TIN,
        Finish::Enig => GOLD,
    };
    let mut rgba = Vec::with_capacity(pw as usize * ph as usize * 4);
    for ((((&b, &c), &o), &s), &v) in material.data().iter().zip(cu.data()).zip(open.data()).zip(print.data()).zip(holes.data()) {
        let (c, o, s, v) = (f64::from(c) / 255.0, f64::from(o) / 255.0, f64::from(s) / 255.0, f64::from(v) / 255.0);
        let covered = mix(MASK, MASK_ON_COPPER, c);
        let opened = mix(BARE_LAMINATE, bare, c);
        let base = mix(covered, opened, o);
        // The print is not put on bare pads.
        let px = mix(mix(base, PRINT, s * (1.0 - o)), VIA_HOLE, v);
        rgba.extend(px.iter().map(|v| v.round().clamp(0.0, 255.0) as u8));
        rgba.push(b);
    }
    let png = encode(&rgba, pw, ph)?;
    let centre = Vec2::new(lo.x + wmm / 2.0, if top { hi.y - hmm / 2.0 } else { lo.y + hmm / 2.0 });
    Some(Look { png, width: wmm, centre })
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn encode(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut writer = e.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

/// Closed contours as one path (each its own sub-path).
fn path(loops: &[Vec<Vec2>], to_px: &dyn Fn(Vec2) -> (f32, f32)) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for l in loops {
        let mut pts = l.iter().map(|p| to_px(*p)).filter(|(x, y)| x.is_finite() && y.is_finite());
        let Some((x, y)) = pts.next() else { continue };
        pb.move_to(x, y);
        for (x, y) in pts {
            pb.line_to(x, y);
        }
        pb.close();
    }
    pb.finish()
}

/// A closed contour's points in order, arcs within `tol` mm. Its pieces may run either way.
fn flatten(l: &[Geom2], tol: f64) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for g in l {
        let mut piece = match *g {
            Geom2::Line(a, b) => vec![a, b],
            Geom2::Circle(c, r) => arc_points(c, r, 0.0, std::f64::consts::TAU, tol),
            Geom2::Arc { c, a, b } => {
                let r = (a.x - c.x).hypot(a.y - c.y);
                let t0 = (a.y - c.y).atan2(a.x - c.x);
                let t1 = (b.y - c.y).atan2(b.x - c.x);
                let sweep = (t1 - t0).rem_euclid(std::f64::consts::TAU);
                let mut pts = arc_points(c, r, t0, if sweep <= 0.0 { std::f64::consts::TAU } else { sweep }, tol);
                if let Some(first) = pts.first_mut() {
                    *first = a;
                }
                if let Some(last) = pts.last_mut() {
                    *last = b;
                }
                pts
            }
            _ => Vec::new(),
        };
        if let (Some(&end), Some(&first), Some(&last)) = (out.last(), piece.first(), piece.last()) {
            let d = |p: Vec2| (p.x - end.x).hypot(p.y - end.y);
            if d(last) < d(first) {
                piece.reverse();
            }
        }
        out.extend(piece);
    }
    out
}

fn arc_points(c: Vec2, r: f64, t0: f64, sweep: f64, tol: f64) -> Vec<Vec2> {
    // Chord error r(1 - cos(θ/2)) at most `tol`.
    let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { sweep };
    let n = (sweep / step.max(1e-3)).ceil().clamp(1.0, 4096.0) as usize;
    (0..=n)
        .map(|k| {
            let t = t0 + sweep * k as f64 / n as f64;
            Vec2::new(c.x + r * t.cos(), c.y + r * t.sin())
        })
        .collect()
}

/// Lay each side's image on the board: a canvas on the top face's plane and one on the bottom
/// face's (facing down), in the active component (the board's). Returns the canvases' names.
pub(super) fn decals(
    s: &mut Session,
    board: &[Vec<Geom2>],
    vias: &[Geom2],
    layers: &[Layer],
    offset: Vec2,
    thickness: f64,
    finish: Finish,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut names = Vec::new();
    for top in [true, false] {
        let Some(look) = look(board, vias, layers, top, finish) else { continue };
        let name = if top { "Top print" } else { "Bottom print" };
        let (z, y_dir) = if top { (thickness, [0.0, 1.0, 0.0]) } else { (0.0, [0.0, -1.0, 0.0]) };
        let c = Vec2::new(look.centre.x + offset.x, look.centre.y + offset.y);
        let p = json!({
            "data": solidcraft_doc::canvas::base64_encode(&look.png),
            "plane": {"origin": [0.0, 0.0, z], "x_dir": [1.0, 0.0, 0.0], "y_dir": y_dir},
            "at": [c.x, c.y, z],
            "width": look.width,
            "opacity": 1.0,
            "name": name,
        });
        match run(s, "canvas.insert", &p) {
            Ok(_) => names.push(name.to_string()),
            Err(e) => warnings.push(format!("The board's {} look could not be laid on it: {e}", if top { "top" } else { "bottom" })),
        }
    }
    names
}
