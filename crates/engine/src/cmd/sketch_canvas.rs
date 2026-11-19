//! Canvases (reference images on planes) and decals (images on planar faces).

use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_doc::canvas::{Canvas, CanvasAnchor, MAX_CANVAS_BYTES, base64_decode, base64_encode, image_info};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, vec2};
use crate::schema::canvas::{
    CanvasCalibrateParams, CanvasCalibrated, CanvasDecalParams, CanvasDeleteParams, CanvasDeleted, CanvasEditParams, CanvasEdited, CanvasFrame,
    CanvasFrameSketchParams, CanvasFramed, CanvasInsertParams, CanvasPlaced, CanvasStraightenParams, CanvasStraightened,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INSERT_CONTRACT: TypedContract<CanvasInsertParams> = TypedContract::new();
const DECAL_CONTRACT: TypedContract<CanvasDecalParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<CanvasEditParams> = TypedContract::new();
const CALIBRATE_CONTRACT: TypedContract<CanvasCalibrateParams> = TypedContract::new();
const STRAIGHTEN_CONTRACT: TypedContract<CanvasStraightenParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<CanvasDeleteParams> = TypedContract::new();
const FRAME_SKETCH_CONTRACT: TypedContract<CanvasFrameSketchParams> = TypedContract::new();

/// A canvas command's reply, built from its schema's type so the two can't drift apart.
pub(super) fn canvas_reply(cmd: &str, r: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(r).map_err(|e| bad(cmd, e.to_string()))
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("canvas.insert", "Canvas", crate::checked!("canvas.insert", add_canvas))
        .at("SKETCH", "INSERT")
        .icon("canvas")
        .contract(&INSERT_CONTRACT)
        .output(output_schema_for::<CanvasPlaced>)
        .params("path | data (base64 PNG, JPEG or SVG; an SVG is drawn into a PNG); resolution?: pixels (an SVG's longest side, default 1024, at most 4096); plane?: XY|XZ|YZ|{face: [x,y,z]}|… (default XY); center?: [x,y] on the plane, or at?: [x,y,z]; width?: mm (default 100); angle?: deg; opacity?: 0…1 (default 0.5); flip?; name?; frame?: true ties the image to a construction rectangle around it (in the sketch being edited, which it must lie on, else in a new sketch), as canvas.frame_sketch does; \"auto\" frames it in the sketch being edited only when it lies on its plane"),
    CommandSpec::new("canvas.decal", "Decal", crate::checked!("canvas.decal", add_decal))
        .at("SKETCH", "INSERT")
        .icon("decal")
        .contract(&DECAL_CONTRACT)
        .output(output_schema_for::<CanvasPlaced>)
        .params("path | data (PNG, JPEG or SVG); resolution?: pixels (an SVG's longest side, default 1024); face: [x,y,z] on a planar face (the image is centred there), or the picked face as sketch.create's plane takes it ({face, body, index, path?}); width?: mm; angle?: deg; opacity?: (default 1); frame?: true ties the image to a construction rectangle around it, as canvas.frame_sketch does; \"auto\" only in the sketch being edited, when the face is on its plane. A decal is part of the model's look: URDF export writes it as a textured visual of its component's link"),
    CommandSpec::new("canvas.edit", "Edit Canvas", crate::checked!("canvas.edit", edit_canvas))
        .at("SKETCH", "INSERT")
        .icon("canvas")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<CanvasEdited>)
        .params("canvas: id or name; center?, width?, angle?: deg, opacity?, flip?, visible?, name?, locked? (a locked canvas refuses center, width, angle, flip, Calibrate and Delete); anchor?: {sketch: id, bottom_left, bottom_right: point ids, top_left?: point id} | null (the sketch points the image's corners follow; without top_left the height follows the image; an anchored canvas refuses center, width, angle, Calibrate and Straighten)"),
    CommandSpec::new("canvas.frame_sketch", "Frame Sketch", crate::checked!("canvas.frame_sketch", frame_sketch))
        .icon("canvas")
        .contract(&FRAME_SKETCH_CONTRACT)
        .output(output_schema_for::<CanvasFramed>)
        .params("canvas: id or name; name?: the new sketch's name. Draws a construction rectangle exactly around the image, in the sketch being edited (the image must lie on its plane) or else in a new sketch on the canvas's plane, and anchors the image's corners to it: dimension and constrain that rectangle to place and size the image → sketch, bottom_left, bottom_right, top_left (point ids)"),
    CommandSpec::new("canvas.calibrate", "Calibrate Canvas", crate::checked!("canvas.calibrate", calibrate))
        .icon("measure")
        .contract(&CALIBRATE_CONTRACT)
        .output(output_schema_for::<CanvasCalibrated>)
        .params("canvas: id or name; a, b: two points on the canvas plane [x,y]; distance: their true distance (mm): scales the image about a"),
    CommandSpec::new("canvas.straighten", "Straighten Canvas", crate::checked!("canvas.straighten", straighten))
        .icon("canvas")
        .contract(&STRAIGHTEN_CONTRACT)
        .output(output_schema_for::<CanvasStraightened>)
        .params("canvas: id or name; a, b: two points on the canvas plane [x,y] along a line of the image; angle?: deg, the direction that line should have (0 horizontal, the default; 90 vertical): turns the image about a, the shorter way"),
    CommandSpec::new("canvas.delete", "Delete Canvas", crate::checked!("canvas.delete", delete_canvas))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<CanvasDeleted>)
        .params("canvas: id or name"),
];

fn image_bytes(p: &Value, cmd: &str) -> Result<Vec<u8>> {
    let b = if let Some(d) = str_(p, "data") {
        if d.len() > MAX_CANVAS_BYTES * 4 / 3 + 16 {
            return Err(bad(cmd, "the image is too large"));
        }
        base64_decode(d).ok_or_else(|| bad(cmd, "`data` is not base64"))?
    } else {
        let path = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "give `path` or `data`"))?;
        solidcraft_io::vfs::read(path, MAX_CANVAS_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?
    };
    if b.len() > MAX_CANVAS_BYTES {
        return Err(bad(cmd, "the image is too large"));
    }
    Ok(b)
}

fn make(s: &mut Session, p: &Value, cmd: &str, decal: bool) -> Result<Value> {
    let mut bytes = image_bytes(p, cmd)?;
    if super::canvas_svg::is_svg(&bytes) {
        let side = match p.get("resolution") {
            None | Some(Value::Null) => super::canvas_svg::DEFAULT_SIDE,
            Some(_) => num(p, "resolution")
                .filter(|r| (1.0..=f64::from(super::canvas_svg::MAX_SIDE)).contains(r))
                .map(|r| r.round() as u32)
                .ok_or_else(|| bad(cmd, format!("`resolution` must be a number of pixels from 1 to {}", super::canvas_svg::MAX_SIDE)))?,
        };
        bytes = super::canvas_svg::rasterize(&bytes, side).map_err(|e| bad(cmd, e))?;
    }
    let (format, pixels) = image_info(&bytes).ok_or_else(|| bad(cmd, "only PNG, JPEG and SVG images can be inserted"))?;
    let plane = super::sketch::plane_ref_numbers(s, p, cmd)?;
    let (vals, _) = s.doc.param_values();
    let pl = s.doc.resolve_plane_in(&vals, &plane, 0, s.active_component, Some(&s.model.state()))?;
    // A decal sits where the face was picked.
    let at = p.get("at").or_else(|| p.get("plane").and_then(|v| v.get("face"))).and_then(crate::params::vec3).map(|q| pl.to_local(q));
    let center = p.get("center").and_then(vec2).or(at).unwrap_or(Vec2::ZERO);
    let width = num(p, "width").unwrap_or(100.0);
    if !(width > 1e-6 && width < 1e7) {
        return Err(bad(cmd, "`width` must be positive"));
    }
    let opacity = num(p, "opacity").unwrap_or(if decal { 1.0 } else { 0.5 }).clamp(0.0, 1.0);
    // `"auto"`: framed in the sketch being edited only when the image lies on its plane, as the
    // image tools place it; otherwise a free image.
    let (framed, auto) = match p.get("frame") {
        None | Some(Value::Null) => (false, false),
        Some(Value::String(v)) if v == "auto" => (true, true),
        Some(v) => (v.as_bool().ok_or_else(|| bad(cmd, "`frame` must be true, false or \"auto\""))?, false),
    };
    // It belongs to the active component (its plane is in that frame) and moves with it.
    let component = s.active_component;
    let doc = s.doc_mut();
    let id = doc.allocate_id()?;
    let base = if decal { "Decal" } else { "Canvas" };
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| {
        let names: std::collections::BTreeSet<_> = doc.canvases.iter().map(|c| c.name.as_str()).collect();
        let number = (1..=doc.canvases.len().saturating_add(1)).find(|n| !names.contains(format!("{base}{n}").as_str())).unwrap_or(1);
        format!("{base}{number}")
    });
    doc.canvases.push(Canvas {
        id,
        name: name.clone(),
        format: format.into(),
        data: base64_encode(&bytes).into(),
        pixels,
        plane,
        center,
        width,
        angle: num(p, "angle").unwrap_or(0.0).to_radians(),
        opacity,
        flip: bool_(p, "flip").unwrap_or(false),
        visible: true,
        locked: false,
        component,
        anchor: None,
    });
    // A frame that cannot be drawn fails the command, and the engine puts the design back as it
    // was: no image is left behind unframed.
    let frame = if framed {
        let i = s.doc.canvases.iter().position(|c| c.id == id).ok_or_else(|| bad(cmd, "canvas"))?;
        let on_open = || -> Result<bool> {
            let c = s.doc.canvases.get(i).ok_or_else(|| bad(cmd, "canvas"))?;
            Ok(match super::sketch_store::active(s) {
                Some(open) => corners_in_sketch(s, c, open, cmd)?.is_some(),
                None => false,
            })
        };
        if !auto || on_open()? { Some(frame_canvas(s, i, None, cmd)?) } else { None }
    } else {
        None
    };
    canvas_reply(cmd, CanvasPlaced { canvas: id, name, pixels, frame, param_warnings: Vec::new() })
}

fn add_canvas(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, "canvas.insert", false)
}

fn add_decal(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.decal";
    let face = p.get("face").cloned().ok_or_else(|| bad(cmd, "`face` must be a point [x,y,z] on a planar face"))?;
    // A picked face as `sketch.create` takes it ({face, body, index, path?}) is used as it is.
    let plane = if face.get("face").is_some() { face } else { json!({"face": face}) };
    let mut q = p.clone();
    if let Some(o) = q.as_object_mut() {
        o.insert("plane".into(), plane);
    }
    make(s, &q, cmd, true)
}

fn find(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let key = match p.get("canvas") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`canvas` must be an id or a name")),
    };
    s.doc.canvases.iter().position(|c| c.id.to_string() == key || c.name == key).ok_or_else(|| bad(cmd, format!("no canvas `{key}`")))
}

/// A locked canvas (#12310) keeps its place and is not deleted: the refusal names it.
fn refuse_locked(s: &Session, i: usize, cmd: &str) -> Result<()> {
    match s.doc.canvases.get(i) {
        Some(c) if c.locked => Err(bad(cmd, locked_reason(c))),
        _ => Ok(()),
    }
}

/// Why a locked canvas is left as it is.
pub(crate) fn locked_reason(c: &Canvas) -> String {
    format!("{} is locked; unlock it first", c.name)
}

/// An anchored canvas's sketch places and sizes it: the refusal of a change that would.
fn refuse_anchored(s: &Session, i: usize, cmd: &str) -> Result<()> {
    match s.doc.canvases.get(i) {
        Some(c) if c.anchor.is_some() => Err(bad(cmd, format!("{} follows points of a sketch: change that sketch, or clear `anchor` first", c.name))),
        _ => Ok(()),
    }
}

/// `anchor` as `canvas.edit` takes it: left out (`None`), `null` to clear it, or the sketch and
/// its point ids.
fn anchor_param(p: &Value, cmd: &str) -> Result<Option<Option<CanvasAnchor>>> {
    let v = match p.get("anchor") {
        None => return Ok(None),
        Some(Value::Null) => return Ok(Some(None)),
        Some(v) => v,
    };
    let sketch = v.get("sketch").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`anchor.sketch` must be a sketch's id"))?;
    let id = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .filter(|x| !x.is_empty() && x.len() <= 256)
            .map(str::to_string)
            .ok_or_else(|| bad(cmd, format!("`anchor.{k}` must be a point id in that sketch")))
    };
    let top_left = match v.get("top_left") {
        None | Some(Value::Null) => None,
        Some(_) => Some(id("top_left")?),
    };
    Ok(Some(Some(CanvasAnchor { sketch, bottom_left: id("bottom_left")?, bottom_right: id("bottom_right")?, top_left })))
}

fn edit_canvas(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.edit";
    let i = find(s, p, cmd)?;
    let width = num(p, "width");
    if width.is_some_and(|w| !(w > 1e-6 && w < 1e7)) {
        return Err(bad(cmd, "`width` must be positive"));
    }
    let anchor = anchor_param(p, cmd)?;
    // Unlocking in the same call lets the other changes through.
    let moves = ["center", "width", "angle", "flip"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()));
    if (moves || anchor.is_some()) && bool_(p, "locked") != Some(false) {
        refuse_locked(s, i, cmd)?;
    }
    // Clearing the anchor in the same call lets its own placement through.
    let places = ["center", "width", "angle"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()));
    if places && anchor != Some(None) {
        if anchor.is_some() {
            return Err(bad(cmd, "give either `anchor` or `center`, `width` and `angle`: the anchor's sketch places the image"));
        }
        refuse_anchored(s, i, cmd)?;
    }
    if let Some(Some(a)) = &anchor {
        let c = s.doc.canvases.get(i).ok_or_else(|| bad(cmd, "canvas"))?;
        let state = s.model.state();
        s.doc.canvas_anchor_corners(c, a, &state).map_err(|e| bad(cmd, e.to_string()))?;
    }
    let c = s.doc_mut().canvases.get_mut(i).ok_or_else(|| bad(cmd, "canvas"))?;
    if let Some(l) = bool_(p, "locked") {
        c.locked = l;
    }
    if let Some(a) = anchor {
        c.anchor = a;
    }
    if let Some(v) = p.get("center").and_then(vec2) {
        c.center = v;
    }
    if let Some(w) = width {
        c.width = w;
    }
    if let Some(a) = num(p, "angle") {
        c.angle = a.to_radians();
    }
    if let Some(o) = num(p, "opacity") {
        c.opacity = o.clamp(0.0, 1.0);
    }
    if let Some(f) = bool_(p, "flip") {
        c.flip = f;
    }
    if let Some(v) = bool_(p, "visible") {
        c.visible = v;
    }
    if let Some(n) = str_(p, "name").filter(|n| !n.trim().is_empty()) {
        c.name = n.to_string();
    }
    canvas_reply(cmd, CanvasEdited { canvas: c.id, param_warnings: Vec::new() })
}

fn calibrate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.calibrate";
    let i = find(s, p, cmd)?;
    let (a, b) = (p.get("a").and_then(vec2), p.get("b").and_then(vec2));
    let (Some(a), Some(b)) = (a, b) else { return Err(bad(cmd, "`a` and `b` must be points [x,y]")) };
    let d = num(p, "distance").filter(|d| *d > 1e-9).ok_or_else(|| bad(cmd, "`distance` must be positive"))?;
    let now = a.dist(b);
    if now < 1e-9 {
        return Err(bad(cmd, "the points must differ"));
    }
    let k = d / now;
    refuse_locked(s, i, cmd)?;
    refuse_anchored(s, i, cmd)?;
    let c = s.doc_mut().canvases.get_mut(i).ok_or_else(|| bad(cmd, "canvas"))?;
    c.width *= k;
    c.center = a + (c.center - a) * k;
    canvas_reply(cmd, CanvasCalibrated { canvas: c.id, width: c.width, param_warnings: Vec::new() })
}

/// The turn, in radians, that brings a line in direction `now` to direction `target` (both in
/// radians), the shorter way: a line has no arrow, so the turn is in (-90°, 90°].
pub fn straighten_turn(now: f64, target: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    let d = (target - now).rem_euclid(PI);
    if d > FRAC_PI_2 { d - PI } else { d }
}

/// An angle in radians brought into (-180°, 180°].
fn wrap_angle(a: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    let w = a.rem_euclid(TAU);
    if w > PI { w - TAU } else { w }
}

fn straighten(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.straighten";
    let i = find(s, p, cmd)?;
    refuse_locked(s, i, cmd)?;
    refuse_anchored(s, i, cmd)?;
    let (a, b) = (p.get("a").and_then(vec2), p.get("b").and_then(vec2));
    let (Some(a), Some(b)) = (a, b) else { return Err(bad(cmd, "`a` and `b` must be points [x,y]")) };
    let target = match p.get("angle") {
        None | Some(Value::Null) => 0.0,
        Some(v) => v.as_f64().and_then(crate::params::finite).ok_or_else(|| bad(cmd, "`angle` must be a number of degrees"))?,
    };
    if a.dist(b) < 1e-9 {
        return Err(bad(cmd, "the points must differ"));
    }
    let line = b - a;
    let turn = straighten_turn(line.y.atan2(line.x), target.to_radians());
    let c = s.doc_mut().canvases.get_mut(i).ok_or_else(|| bad(cmd, "canvas"))?;
    // The whole image turns about `a`: its angle and its centre.
    let (sn, cs) = turn.sin_cos();
    let r = c.center - a;
    c.center = a + Vec2::new(r.x * cs - r.y * sn, r.x * sn + r.y * cs);
    c.angle = wrap_angle(c.angle + turn);
    canvas_reply(cmd, CanvasStraightened { canvas: c.id, angle: c.angle.to_degrees(), turned: turn.to_degrees(), param_warnings: Vec::new() })
}

fn delete_canvas(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.delete";
    let i = find(s, p, cmd)?;
    refuse_locked(s, i, cmd)?;
    let c = s.doc_mut().canvases.remove(i);
    canvas_reply(cmd, CanvasDeleted { deleted: c.id, param_warnings: Vec::new() })
}

fn frame_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.frame_sketch";
    let i = find(s, p, cmd)?;
    let f = frame_canvas(s, i, str_(p, "name"), cmd)?;
    let canvas = s.doc.canvases.get(i).map(|c| c.id).ok_or_else(|| bad(cmd, "canvas"))?;
    canvas_reply(
        cmd,
        CanvasFramed {
            canvas,
            sketch: f.sketch,
            name: f.name,
            bottom_left: f.bottom_left,
            bottom_right: f.bottom_right,
            top_left: f.top_left,
            param_warnings: Vec::new(),
        },
    )
}

/// A construction rectangle exactly around canvas `i`'s image, and the canvas anchored to its
/// bottom-left, bottom-right and top-left corners: the sketch's dimensions and constraints then
/// place, turn and size the image. The rectangle goes into the sketch being edited, so it can be
/// tied to that sketch's own geometry; with none, into a new sketch on the canvas's plane, named
/// `name` (default `<canvas> frame`).
fn frame_canvas(s: &mut Session, i: usize, name: Option<&str>, cmd: &str) -> Result<CanvasFrame> {
    refuse_locked(s, i, cmd)?;
    let c = s.doc.canvases.get(i).cloned().ok_or_else(|| bad(cmd, "canvas"))?;
    if c.anchor.is_some() {
        return Err(bad(cmd, format!("{} already follows a sketch; clear its `anchor` with canvas.edit first", c.name)));
    }
    // The sketch is in the active component, as the canvas's plane is.
    if c.component != s.active_component {
        return Err(bad(cmd, format!("{} belongs to another component: activate that component first", c.name)));
    }
    let finish = super::find_command("sketch.finish").ok_or_else(|| bad(cmd, "sketch.finish"))?.run;
    let rectangle = super::find_command("sketch.rectangle.three_point").ok_or_else(|| bad(cmd, "sketch.rectangle.three_point"))?.run;
    let (sketch, name, [bl, br, tl], made) = match super::sketch_store::active(s) {
        Some(open) => {
            let corners = frame_in_sketch(s, &c, open, cmd)?;
            let name = s.doc.feature(open).map(|f| f.name.clone()).unwrap_or_default();
            // Placed inside a closed outline of four lines in the sketch (a rectangle drawn there,
            // or a face's edges projected into it): the image fills that outline, its corners tied
            // to the outline's corners, rather than getting a rectangle of its own.
            if let Some((bottom_left, bottom_right, top_left)) = fill_outline(s, open, corners) {
                let anchor = CanvasAnchor {
                    sketch: open,
                    bottom_left: bottom_left.clone(),
                    bottom_right: bottom_right.clone(),
                    top_left: Some(top_left.clone()),
                };
                s.doc_mut().canvases.get_mut(i).ok_or_else(|| bad(cmd, "canvas"))?.anchor = Some(anchor);
                return Ok(CanvasFrame { sketch: open, name, bottom_left, bottom_right, top_left });
            }
            (open, name, corners, false)
        }
        None => {
            // The frame unmirrored: a mirrored image's bottom-left corner is the frame's bottom-right.
            let k = c.corners();
            let corners = if c.flip { [k[1], k[0], k[2]] } else { [k[0], k[1], k[3]] };
            let name =
                name.filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or_else(|| s.doc.free_sketch_name(&format!("{} frame", c.name)));
            let made = super::sketch::create_sketch_with_plane(s, &json!({ "name": name }), c.plane.clone(), None)?;
            let sketch = made.get("sketch").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "could not start a sketch on the canvas"))?;
            (sketch, name, corners, true)
        }
    };
    // Its own points: a corner landing on a point of the sketch is tied to it only when asked.
    let drawn = rectangle(s, &json!({"p0": [bl.x, bl.y], "p1": [br.x, br.y], "p2": [tl.x, tl.y], "construction": true, "connect": false}));
    let drawn = match drawn {
        Ok(v) => v,
        Err(e) => {
            if made {
                let _ = finish(s, &json!({}));
            }
            return Err(e);
        }
    };
    if made {
        finish(s, &json!({}))?;
    }
    // Its sides run bottom-left to bottom-right, …, top-left to bottom-left.
    let side = |k: usize| drawn.get("curves").and_then(|c| c.get(k)).and_then(Value::as_str).map(str::to_string);
    let corners = (|| {
        let sk = super::sketch_store::read(&s.doc, sketch).ok()?;
        let line = |id: &str| match sk.curves.iter().find(|c| c.id == id).map(|c| &c.kind) {
            Some(solidcraft_sketch::CurveKind::Line { a, b }) => Some((*a, *b)),
            _ => None,
        };
        let ((b0, b1), (t0, _)) = (line(&side(0)?)?, line(&side(3)?)?);
        let id = |i: usize| sk.points.get(i).map(|p| p.id.clone());
        Some((id(b0)?, id(b1)?, id(t0)?))
    })();
    let (bottom_left, bottom_right, top_left) = corners.ok_or_else(|| bad(cmd, "could not read the corners of the frame drawn"))?;
    let anchor = CanvasAnchor { sketch, bottom_left: bottom_left.clone(), bottom_right: bottom_right.clone(), top_left: Some(top_left.clone()) };
    let canvas = s.doc_mut().canvases.get_mut(i).ok_or_else(|| bad(cmd, "canvas"))?;
    canvas.anchor = Some(anchor);
    Ok(CanvasFrame { sketch, name, bottom_left, bottom_right, top_left })
}

/// The smallest closed outline of four lines in sketch `sketch` that holds the centre of an image
/// whose frame has corners `bl`, `br`, `tl` (sketch coordinates), as the ids of the outline's
/// points to tie the image's bottom-left, bottom-right and top-left corners to: the outline's
/// corner nearest the image's bottom-left, its neighbour along the image's bottom edge, and its
/// other neighbour. Line ends at one place count as one corner, so a face's projected edges with
/// points of their own close an outline too. `None` when no convex outline of four lines holds the
/// centre.
fn fill_outline(s: &Session, sketch: u64, [bl, br, tl]: [Vec2; 3]) -> Option<(String, String, String)> {
    const MOST_LINES: usize = 2000;
    let state = s.model.state();
    let solved = state.sketch(sketch)?;
    let sk = &solved.sketch;
    let dist = |a: Vec2, b: Vec2| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let centre = Vec2::new((br.x + tl.x) / 2.0, (br.y + tl.y) / 2.0);
    let tolerance = 1e-6 * dist(bl, br).max(dist(bl, tl)).max(1.0);
    // Corners (a place and the first point there) and the lines between them.
    let mut corners: Vec<(Vec2, usize)> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for curve in sk.curves.iter().take(MOST_LINES) {
        let solidcraft_sketch::CurveKind::Line { a, b } = curve.kind else { continue };
        let (Some(pa), Some(pb)) = (sk.point(a), sk.point(b)) else { continue };
        let mut ends = [0usize; 2];
        for (slot, (point, at)) in ends.iter_mut().zip([(a, pa), (b, pb)]) {
            *slot = match corners.iter().position(|(q, _)| dist(*q, at) <= tolerance) {
                Some(k) => k,
                None => {
                    corners.push((at, point));
                    corners.len() - 1
                }
            };
        }
        let [a, b] = ends;
        if a != b && !edges.contains(&(a.min(b), a.max(b))) {
            edges.push((a.min(b), a.max(b)));
        }
    }
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); corners.len()];
    for &(a, b) in &edges {
        next.get_mut(a)?.push(b);
        next.get_mut(b)?.push(a);
    }
    let place = |k: usize| corners.get(k).map(|(at, _)| *at);
    // Every cycle c0-c1-c2-c3 of four lines that is convex and holds the centre; the smallest wins.
    let mut best: Option<([usize; 4], f64)> = None;
    for (c0, around) in next.iter().enumerate() {
        for (j, &c1) in around.iter().enumerate() {
            for &c3 in around.iter().skip(j + 1) {
                for &c2 in next.get(c1)?.iter().filter(|&&c2| c2 != c0 && c2 != c3) {
                    if !next.get(c3)?.contains(&c2) || c2 == c1 {
                        continue;
                    }
                    let q = [place(c0)?, place(c1)?, place(c2)?, place(c3)?];
                    let turns: [f64; 4] = std::array::from_fn(|k| cross(q[k], q[(k + 1) % 4], q[(k + 2) % 4]));
                    let holds: [f64; 4] = std::array::from_fn(|k| cross(q[k], q[(k + 1) % 4], centre));
                    let one_way = |v: [f64; 4]| v.iter().all(|t| *t > 0.0) || v.iter().all(|t| *t < 0.0);
                    if !(one_way(turns) && one_way(holds)) {
                        continue;
                    }
                    let area = (cross(q[0], q[1], q[2]) + cross(q[0], q[2], q[3])).abs() / 2.0;
                    if best.is_none_or(|(_, smallest)| area < smallest) {
                        best = Some(([c0, c1, c2, c3], area));
                    }
                }
            }
        }
    }
    let (quad, _) = best?;
    let q = quad.map(|k| place(k).unwrap_or(centre));
    let k = (0..4).min_by(|&a, &b| dist(q[a], bl).total_cmp(&dist(q[b], bl)))?;
    let (before, after) = ((k + 3) % 4, (k + 1) % 4);
    let along = |n: usize| (q[n].x - q[k].x) * (br.x - bl.x) + (q[n].y - q[k].y) * (br.y - bl.y);
    let (right, up) = if along(after) >= along(before) { (after, before) } else { (before, after) };
    let id = |n: usize| quad.get(n).and_then(|&c| corners.get(c)).and_then(|&(_, point)| sk.points.get(point)).map(|p| p.id.clone());
    Some((id(k)?, id(right)?, id(up)?))
}

/// The bottom-left, bottom-right and top-left corners of canvas `c`'s frame (unmirrored) in the
/// coordinates of sketch `sketch`, or why the image does not lie on that sketch's plane.
fn frame_in_sketch(s: &Session, c: &Canvas, sketch: u64, cmd: &str) -> Result<[Vec2; 3]> {
    corners_in_sketch(s, c, sketch, cmd)?.ok_or_else(|| {
        let name = s.doc.feature(sketch).map(|f| f.name.clone()).unwrap_or_default();
        bad(
            cmd,
            format!("{} does not lie on the plane of sketch `{name}`: put it on that plane, or finish the sketch to frame it in a new one", c.name),
        )
    })
}

/// [`frame_in_sketch`]'s corners, or `None` when the image does not lie on the sketch's plane.
fn corners_in_sketch(s: &Session, c: &Canvas, sketch: u64, cmd: &str) -> Result<Option<[Vec2; 3]>> {
    use solidcore_ops::affine;
    let (vals, _) = s.doc.param_values();
    let state = s.model.state();
    let ss = state.sketch(sketch).ok_or_else(|| bad(cmd, "the sketch being edited has not been solved; finish or repair it first"))?;
    // In the world, where the component's occurrence puts the canvas…
    let world = s.doc.canvas_corners(&vals, c, &state).map_err(|e| bad(cmd, e.to_string()))?;
    // …and back into the sketch's component's frame, which its plane is in.
    let m = s.doc.component_transform(s.doc.feature(sketch).map_or(c.component, |f| f.component));
    let back = if solidcraft_doc::is_identity(&m) {
        None
    } else {
        Some(affine::inverse(&m).map_err(|_| bad(cmd, "the sketch's component has no inverse placement"))?)
    };
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let (n, o) = (arr(ss.plane.normal()), arr(ss.plane.origin));
    let tolerance = 1e-6 * (1.0 + c.width.max(c.height()));
    let mut k = [Vec2::ZERO; 4];
    for (slot, w) in k.iter_mut().zip(world) {
        let [x, y, z] = back.as_ref().map_or(arr(w), |b| affine::apply_point(b, arr(w)));
        if !(affine::distance_along(o, n, [x, y, z]).abs() <= tolerance) {
            return Ok(None);
        }
        *slot = ss.plane.to_local(Vec3::new(x, y, z));
    }
    // A mirrored image's bottom-left corner is the frame's bottom-right.
    Ok(Some(if c.flip { [k[1], k[0], k[2]] } else { [k[0], k[1], k[3]] }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_doc::canvas::base64_encode;

    use crate::Session;

    fn png(w: u32, h: u32) -> String {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        base64_encode(&b)
    }

    #[test]
    fn canvases_on_faces_follow_geometry_edits_and_reopen() {
        let mut s = Session::default();
        for (command, params) in [
            ("parameters.add", json!({"name": "height", "expression": "10 mm"})),
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance": "height", "name": "Block"})),
            ("canvas.insert", json!({"data": png(40, 20), "plane": {"face": [20, 15, 10]}, "name": "Face", "width": 20})),
            ("canvas.decal", json!({"data": png(40, 20), "face": [20, 15, 10], "name": "Decal", "width": 20})),
            ("canvas.insert", json!({"data": png(40, 20), "plane": {"face": [20, 15, 10]}, "offset": 2, "name": "Offset", "width": 20})),
            ("canvas.insert", json!({"data": png(40, 20), "plane": {"face": [20, 15, 10], "fixed": true}, "name": "Fixed", "width": 20})),
        ] {
            s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
        }
        let check = |s: &Session, height: f64| {
            let (vals, _) = s.doc.param_values();
            let state = s.model.state();
            for canvas in &s.doc.canvases {
                let expected = match canvas.name.as_str() {
                    "Offset" => height + 2.0,
                    "Fixed" => 10.0,
                    _ => height,
                };
                let plane = s.doc.canvas_plane(&vals, canvas, &state).unwrap();
                for corner in canvas.world_corners(&plane) {
                    assert!((corner.z - expected).abs() < 1e-9, "{}: expected z={expected}, got {corner:?}", canvas.name);
                }
            }
        };
        check(&s, 10.0);
        s.execute("parameters.change", &json!({"name": "height", "expression": "14 mm"})).unwrap();
        check(&s, 14.0);
        s.execute("edit.undo", &json!({})).unwrap();
        check(&s, 10.0);
        s.execute("edit.redo", &json!({})).unwrap();
        check(&s, 14.0);
        check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()), 14.0);
        s.execute("timeline.suppress", &json!({"feature": "Block", "suppressed": true})).unwrap();
        let (vals, _) = s.doc.param_values();
        let state = s.model.state();
        for canvas in &s.doc.canvases {
            let plane = s.doc.canvas_plane(&vals, canvas, &state);
            assert_eq!(plane.is_ok(), canvas.name == "Fixed", "{}: {plane:?}", canvas.name);
        }
        s.execute("timeline.suppress", &json!({"feature": "Block", "suppressed": false})).unwrap();
        check(&s, 14.0);
    }

    #[test]
    fn canvases_on_model_planes_follow_rebuilds_and_reopen() {
        use solidcore_record::Vec3;
        use solidcraft_geom::PlaneGeom as _;

        let mut s = Session::default();
        for (command, params) in [
            ("parameters.add", json!({"name": "station", "expression": "0.25", "unit": ""})),
            ("sketch.create", json!({"plane": "XY", "name": "Path"})),
            ("sketch.line", json!({"points": [[0, 0], [20, 0], [20, 10]], "ids": ["a", "b"]})),
            ("sketch.finish", json!({})),
            (
                "construct.plane.three_points",
                json!({"a": {"point": [0, 0, 5]}, "b": {"point": [1, 0, 5]}, "c": {"point": [0, 1, 5]}, "name": "Numeric"}),
            ),
            (
                "construct.plane.three_points",
                json!({"a": {"sketch": "Path", "point": "a.start"}, "b": {"sketch": "Path", "point": "a.end"},
                    "c": {"sketch": "Path", "point": "b.end"}, "name": "Points"}),
            ),
            ("construct.plane.along_path", json!({"path": {"sketch": "Path", "curve": "a"}, "t": "station", "name": "Along"})),
            ("construct.plane.offset", json!({"base": "Along", "offset": 2, "name": "AlongOffset"})),
            ("construct.plane.at_angle", json!({"base": "Numeric", "axis": "X", "angle": 90, "name": "Angled"})),
            ("construct.plane.offset", json!({"base": "Angled", "offset": 2, "name": "Derived"})),
        ] {
            s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
        }
        for plane in ["XY", "Numeric", "Points", "Along", "AlongOffset", "Angled", "Derived"] {
            s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": plane, "name": plane, "width": 40})).unwrap();
        }
        let check = |s: &Session, station: f64| {
            let (vals, _) = s.doc.param_values();
            let state = s.model.state();
            for (name, origin, normal, expected_corners) in [
                ("XY", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [[-20.0, -10.0, 0.0], [20.0, -10.0, 0.0], [20.0, 10.0, 0.0], [-20.0, 10.0, 0.0]]),
                ("Numeric", [0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [[-20.0, -10.0, 5.0], [20.0, -10.0, 5.0], [20.0, 10.0, 5.0], [-20.0, 10.0, 5.0]]),
                ("Points", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [[-20.0, -10.0, 0.0], [20.0, -10.0, 0.0], [20.0, 10.0, 0.0], [-20.0, 10.0, 0.0]]),
                (
                    "Along",
                    [station, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [[station, -20.0, -10.0], [station, 20.0, -10.0], [station, 20.0, 10.0], [station, -20.0, 10.0]],
                ),
                (
                    "AlongOffset",
                    [station + 2.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [[station + 2.0, -20.0, -10.0], [station + 2.0, 20.0, -10.0], [station + 2.0, 20.0, 10.0], [station + 2.0, -20.0, 10.0]],
                ),
                ("Angled", [0.0, -5.0, 0.0], [0.0, -1.0, 0.0], [[-20.0, -5.0, -10.0], [20.0, -5.0, -10.0], [20.0, -5.0, 10.0], [-20.0, -5.0, 10.0]]),
                ("Derived", [0.0, -7.0, 0.0], [0.0, -1.0, 0.0], [[-20.0, -7.0, -10.0], [20.0, -7.0, -10.0], [20.0, -7.0, 10.0], [-20.0, -7.0, 10.0]]),
            ] {
                let c = s.doc.canvases.iter().find(|c| c.name == name).unwrap();
                let plane = s.doc.canvas_plane(&vals, c, &state).unwrap_or_else(|e| panic!("{name}: {e}"));
                let assert_components = |actual: Vec3, expected: [f64; 3], context: &str| {
                    for ((axis, actual), expected) in ["x", "y", "z"].into_iter().zip([actual.x, actual.y, actual.z]).zip(expected) {
                        assert!((actual - expected).abs() < 1e-9, "{name} {context}.{axis}: expected {expected}, got {actual}");
                    }
                };
                assert_components(plane.origin, origin, "origin");
                assert_components(plane.normal(), normal, "normal");
                for (index, (actual, expected)) in c.world_corners(&plane).into_iter().zip(expected_corners).enumerate() {
                    assert_components(actual, expected, &format!("corner {index}"));
                }
            }
        };
        check(&s, 5.0);
        s.execute("parameters.change", &json!({"name": "station", "expression": "0.75"})).unwrap();
        check(&s, 15.0);
        check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()), 15.0);
    }

    #[test]
    fn deleted_canvas_ids_are_not_reused_after_reopening() {
        let mut s = Session::default();
        let params = json!({"data": png(10, 10)});
        let first = s.execute("canvas.insert", &params).unwrap()["canvas"].as_u64().unwrap();
        s.execute("canvas.delete", &json!({"canvas": first})).unwrap();
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        *s.doc_mut() = doc;
        let second = s.execute("canvas.insert", &params).unwrap()["canvas"].as_u64().unwrap();
        assert!(second > first, "deleted canvas {first} was reused as {second}");
        assert!(s.execute("canvas.edit", &json!({"canvas": first, "width": 500})).is_err());
    }

    /// A locked canvas (#12310) refuses a move, a resize, a turn, a flip, Calibrate and Delete,
    /// keeps taking opacity, visibility and a new name, is saved locked, and moves again once
    /// unlocked. Delete of a selection with it deletes the rest and says why it stayed.
    #[test]
    fn a_locked_canvas_keeps_its_place_and_is_not_deleted() {
        let mut s = Session::default();
        let id = s.execute("canvas.insert", &json!({"data": png(10, 10), "width": 50})).unwrap()["canvas"].as_u64().unwrap();
        let other = s.execute("canvas.insert", &json!({"data": png(10, 10)})).unwrap()["canvas"].as_u64().unwrap();
        s.execute("canvas.edit", &json!({"canvas": id, "locked": true})).unwrap();
        let locked = s.doc.canvases[0].clone();
        assert!(locked.locked);
        for p in [
            json!({"canvas": id, "center": [5, 5]}),
            json!({"canvas": id, "width": 80}),
            json!({"canvas": id, "angle": 30}),
            json!({"canvas": id, "flip": true}),
            json!({"canvas": id, "locked": true, "width": 80}),
        ] {
            let e = s.execute("canvas.edit", &p).unwrap_err().to_string();
            assert!(e.contains("Canvas1 is locked"), "{p}: {e}");
        }
        assert!(s.execute("canvas.calibrate", &json!({"canvas": id, "a": [0, 0], "b": [10, 0], "distance": 20})).is_err());
        assert!(s.execute("canvas.straighten", &json!({"canvas": id, "a": [0, 0], "b": [10, 0], "angle": 45})).is_err());
        assert!(s.execute("canvas.delete", &json!({"canvas": id})).is_err());
        assert_eq!(s.doc.canvases[0], locked, "nothing changed");
        s.execute("canvas.edit", &json!({"canvas": id, "opacity": 0.9, "visible": false, "name": "Blueprint"})).unwrap();
        assert!(s.doc.canvases[0].locked && !s.doc.canvases[0].visible && s.doc.canvases[0].name == "Blueprint");
        // Saved locked; a design without the field opens unlocked.
        let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert!(back.canvases[0].locked && !back.canvases[1].locked);
        assert!(serde_json::to_value(&s.doc.canvases[1]).unwrap().get("locked").is_none(), "an unlocked canvas saves as before");
        // Delete of both deletes the unlocked one and names the locked one.
        let r = s.execute("selection.delete", &json!({"canvases": [id, other]})).unwrap();
        assert_eq!(r["skipped"], json!(["Blueprint is locked; unlock it first"]));
        assert_eq!(s.doc.canvases.iter().map(|c| c.id).collect::<Vec<_>>(), [id]);
        assert!(s.execute("selection.delete", &json!({"canvases": [id]})).is_err());
        // Unlocking in the same call lets the move through.
        s.execute("canvas.edit", &json!({"canvas": id, "locked": false, "width": 80})).unwrap();
        assert!(!s.doc.canvases[0].locked && (s.doc.canvases[0].width - 80.0).abs() < 1e-12);
        s.execute("canvas.delete", &json!({"canvas": id})).unwrap();
        assert!(s.doc.canvases.is_empty());
    }

    #[test]
    fn canvas_insert_edit_calibrate_save() {
        let mut s = Session::default();
        let r = s.execute("canvas.insert", &json!({"data": png(400, 200), "plane": "XZ", "width": 80})).unwrap();
        let id = r["canvas"].as_u64().unwrap();
        let c = s.doc.canvases[0].clone();
        assert_eq!(c.pixels, [400, 200]);
        assert!((c.height() - 40.0).abs() < 1e-12);
        // Two points 20 apart on the image are really 50 apart.
        s.execute("canvas.calibrate", &json!({"canvas": id, "a": [0, 0], "b": [20, 0], "distance": 50})).unwrap();
        assert!((s.doc.canvases[0].width - 200.0).abs() < 1e-9);
        s.execute("canvas.edit", &json!({"canvas": "Canvas1", "opacity": 0.8, "angle": 90})).unwrap();
        // Undo-able and saved with the design.
        let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(back.canvases, s.doc.canvases);
        s.execute("canvas.delete", &json!({"canvas": id})).unwrap();
        assert!(s.doc.canvases.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.canvases.len(), 1);
        assert!(s.execute("canvas.insert", &json!({"data": "bm9wZQ=="})).is_err());
        // A decal on the top face of a box.
        s.execute("solid.box", &json!({"length": 30, "width": 30, "height": 10})).unwrap();
        let r = s.execute("canvas.decal", &json!({"data": png(10, 10), "face": [15, 15, 10], "width": 20})).unwrap();
        assert!(r["canvas"].is_u64());
    }

    /// #18422: a decal on a curved face says the face is curved, not that it no longer exists;
    /// a point that is on no face still gets the old words.
    #[test]
    fn a_decal_on_a_curved_face_says_the_face_is_curved() {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 10})).unwrap();
        let curved = s.execute("canvas.decal", &json!({"data": png(10, 10), "face": [10, 0, 5], "width": 5})).unwrap_err().to_string();
        assert!(curved.contains("curved or is not an exact plane"), "{curved}");
        assert!(!curved.contains("no longer exists"), "{curved}");
        let nowhere = s.execute("canvas.decal", &json!({"data": png(10, 10), "face": [100, 100, 100], "width": 5})).unwrap_err().to_string();
        assert!(nowhere.contains("no longer exists"), "{nowhere}");
        // The flat end of the same cylinder still takes a decal.
        assert!(s.execute("canvas.decal", &json!({"data": png(10, 10), "face": [0, 0, 10], "width": 5})).is_ok());
    }

    /// Where a point of the plane is on the image, as fractions (s, t) of its bottom and left
    /// edges, and back: straightening must keep every image point's place relative to the others.
    fn on_image(c: &solidcraft_doc::canvas::Canvas, p: Vec2) -> (f64, f64) {
        let [bl, br, _, tl] = c.corners();
        let (u, v, w) = (br - bl, tl - bl, p - bl);
        (w.dot(u) / u.dot(u), w.dot(v) / v.dot(v))
    }

    fn at_image(c: &solidcraft_doc::canvas::Canvas, (s, t): (f64, f64)) -> Vec2 {
        let [bl, br, _, tl] = c.corners();
        bl + (br - bl) * s + (tl - bl) * t
    }

    use solidcore_record::Vec2;
    use solidcraft_geom::Vec2Geom as _;

    /// A line drawn on the image ends up at the asked direction, turned about its first point,
    /// the shorter way, with the image's size kept (#11989).
    #[test]
    fn canvas_straighten_turns_a_drawn_line_to_the_asked_direction() {
        let tilt = 5f64.to_radians();
        let cases = [
            // (flip, a, direction of a->b in radians, target in degrees or None, expected turn in degrees)
            (false, [3.0, -4.0], tilt, None, -5.0),
            // Drawn right to left: the same line, so the same small turn, not 175 degrees.
            (false, [3.0, -4.0], std::f64::consts::PI + tilt, None, -5.0),
            (false, [-10.0, 6.0], 80f64.to_radians(), Some(90.0), 10.0),
            (false, [-10.0, 6.0], -80f64.to_radians(), Some(90.0), -10.0),
            (false, [0.0, 0.0], tilt, Some(30.0), 25.0),
            (false, [0.0, 0.0], tilt, Some(-150.0), 25.0),
            (true, [7.0, 2.0], -tilt, Some(0.0), 5.0),
        ];
        for (flip, a, dir, target, turned) in cases {
            let mut s = Session::default();
            s.execute("canvas.insert", &json!({"data": png(400, 200), "width": 80, "center": [10, 5], "angle": 12, "flip": flip})).unwrap();
            let before = s.doc.canvases[0].clone();
            let a = Vec2::new(a[0], a[1]);
            let b = a + Vec2::new(dir.cos(), dir.sin()) * 25.0;
            let (ia, ib) = (on_image(&before, a), on_image(&before, b));
            let mut p = json!({"canvas": "Canvas1", "a": [a.x, a.y], "b": [b.x, b.y]});
            if let Some(t) = target {
                p["angle"] = json!(t);
            }
            let r = s.execute("canvas.straighten", &p).unwrap();
            let after = s.doc.canvases[0].clone();
            let case = format!("flip {flip}, a {a:?}, dir {}, target {target:?}: {r}", dir.to_degrees());
            assert!((r["turned"].as_f64().unwrap() - turned).abs() < 1e-9, "{case}");
            assert!((r["angle"].as_f64().unwrap() - (12.0 + turned)).abs() < 1e-9, "{case}");
            assert!((after.angle.to_degrees() - (12.0 + turned)).abs() < 1e-9, "{case}");
            assert_eq!((after.width, after.flip), (before.width, before.flip), "{case}");
            // The image point under `a` stays there; the one under `b` is now in the asked direction.
            let (na, nb) = (at_image(&after, ia), at_image(&after, ib));
            assert!(na.dist(a) < 1e-9, "{case}: a moved to {na:?}");
            let line = nb - na;
            assert!((na.dist(nb) - 25.0).abs() < 1e-9, "{case}");
            let want = target.unwrap_or(0.0_f64).to_radians();
            // Parallel to the asked direction (either way along it).
            assert!((line.x * want.sin() - line.y * want.cos()).abs() < 1e-9, "{case}: {line:?}");
        }
    }

    #[test]
    fn canvas_straighten_refuses_bad_input_and_is_one_undo_step() {
        let mut s = Session::default();
        s.execute("canvas.insert", &json!({"data": png(400, 200), "width": 80})).unwrap();
        let before = s.doc.canvases.clone();
        for p in [
            json!({"canvas": "Canvas1", "a": [1, 1], "b": [1, 1]}),
            json!({"canvas": "Canvas1", "a": [0, 0], "b": [1, 1], "angle": "level"}),
            json!({"canvas": "Canvas1", "a": [0, 0], "b": [1, 1], "angle": 1e300}),
            json!({"canvas": "Canvas1", "a": [0, 0]}),
            json!({"canvas": "Canvas9", "a": [0, 0], "b": [1, 1]}),
            json!({"a": [0, 0], "b": [1, 1]}),
        ] {
            assert!(s.execute("canvas.straighten", &p).is_err(), "{p}");
            assert_eq!(s.doc.canvases, before, "{p}");
        }
        s.execute("canvas.straighten", &json!({"canvas": "Canvas1", "a": [0, 0], "b": [10, 1], "angle": null})).unwrap();
        assert_ne!(s.doc.canvases, before);
        // The angle stays in (-180, 180] however often it turns.
        for _ in 0..40 {
            s.execute("canvas.straighten", &json!({"canvas": "Canvas1", "a": [0, 0], "b": [1, 0], "angle": 89})).unwrap();
            assert!(s.doc.canvases[0].angle.abs() <= std::f64::consts::PI + 1e-12);
        }
        for _ in 0..41 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(s.doc.canvases, before);
    }

    #[test]
    fn straighten_turn_is_the_shorter_way() {
        use super::straighten_turn;
        let deg = |a: f64, b: f64| straighten_turn(a.to_radians(), b.to_radians()).to_degrees();
        for (now, target, want) in [
            (0.0, 0.0, 0.0),
            (5.0, 0.0, -5.0),
            (185.0, 0.0, -5.0),
            (-175.0, 0.0, -5.0),
            (0.0, 90.0, 90.0),
            (0.0, -90.0, 90.0),
            (10.0, 100.0, 90.0),
            (1.0, 92.0, 91.0 - 180.0),
        ] {
            assert!((deg(now, target) - want).abs() < 1e-9, "{now} -> {target}: {}", deg(now, target));
        }
    }

    /// A canvas's world corners as the viewport and URDF export take them.
    fn world_corners(s: &Session, id: u64) -> [solidcore_record::Vec3; 4] {
        let (vals, _) = s.doc.param_values();
        let c = s.doc.canvases.iter().find(|c| c.id == id).unwrap();
        s.doc.canvas_corners(&vals, c, &s.model.state()).unwrap()
    }

    /// An anchored canvas's corners are its sketch's points, so a parameter that moves a point
    /// moves and stretches the image; without the top-left point its height follows the image.
    /// While anchored, its own placement is refused; clearing the anchor gives it back.
    #[test]
    fn an_anchored_canvas_follows_its_sketch_points_and_their_parameters() {
        use solidcore_record::{Vec2, Vec3};
        use solidcraft_geom::prelude::*;
        let mut s = Session::default();
        s.execute("parameters.add", &json!({"name": "w", "expression": "40 mm"})).unwrap();
        let sketch = s.execute("sketch.create", &json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
        let rect = s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": ["w", 20]})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        let corner = |k: &str| rect["corners"][k].as_str().unwrap().to_string();
        let id =
            s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": "XY", "width": 10, "name": "Pic"})).unwrap()["canvas"].as_u64().unwrap();
        let anchor =
            json!({"sketch": sketch, "bottom_left": corner("bottom_left"), "bottom_right": corner("bottom_right"), "top_left": corner("top_left")});
        s.execute("canvas.edit", &json!({"canvas": id, "anchor": anchor})).unwrap();
        let near = |a: Vec3, b: [f64; 3]| (a - Vec3::new(b[0], b[1], b[2])).len() < 1e-6;
        let k = world_corners(&s, id);
        assert!(
            near(k[0], [0.0, 0.0, 0.0]) && near(k[1], [40.0, 0.0, 0.0]) && near(k[2], [40.0, 20.0, 0.0]) && near(k[3], [0.0, 20.0, 0.0]),
            "{k:?}"
        );
        s.execute("parameters.change", &json!({"name": "w", "expression": "60 mm"})).unwrap();
        let k = world_corners(&s, id);
        assert!(near(k[1], [60.0, 0.0, 0.0]) && near(k[2], [60.0, 20.0, 0.0]), "the image stretches with the sketch: {k:?}");
        for (cmd, p) in [
            ("canvas.edit", json!({"canvas": id, "center": [1, 1]})),
            ("canvas.calibrate", json!({"canvas": id, "a": [0, 0], "b": [1, 0], "distance": 2})),
            ("canvas.straighten", json!({"canvas": id, "a": [0, 0], "b": [1, 1]})),
        ] {
            let e = s.execute(cmd, &p).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(e.contains("follows points of a sketch"), "{cmd}: {e}");
        }
        for bad in [
            json!({"sketch": 999_999, "bottom_left": "a", "bottom_right": "b"}),
            json!({"sketch": sketch, "bottom_left": corner("bottom_left"), "bottom_right": "nope"}),
            json!({"sketch": sketch, "bottom_left": corner("bottom_left"), "bottom_right": corner("bottom_left")}),
            json!({"sketch": "x"}),
            json!(7),
        ] {
            assert!(s.execute("canvas.edit", &json!({"canvas": id, "anchor": bad})).is_err(), "{bad}");
        }
        // The image is 2:1, so 30 mm high for 60 mm wide.
        s.execute(
            "canvas.edit",
            &json!({"canvas": id, "anchor": {"sketch": sketch, "bottom_left": corner("bottom_left"), "bottom_right": corner("bottom_right")}}),
        )
        .unwrap();
        assert!(near(world_corners(&s, id)[3], [0.0, 30.0, 0.0]), "{:?}", world_corners(&s, id));
        s.execute("canvas.edit", &json!({"canvas": id, "anchor": null, "center": [1, 1]})).unwrap();
        let c = s.doc.canvases.iter().find(|c| c.id == id).unwrap();
        assert!(c.anchor.is_none() && c.center == Vec2::new(1.0, 1.0));
    }

    /// Frame Sketch draws a construction rectangle exactly around the image and anchors the image
    /// to it, so the image stays where it was, turned and mirrored or not.
    #[test]
    fn frame_sketch_anchors_an_image_where_it_lies() {
        use solidcraft_geom::prelude::*;
        for flip in [false, true] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "corner": [0, 0, 0]})).unwrap();
            let r = s.execute("canvas.decal", &json!({"data": png(40, 20), "face": [20, 15, 10], "width": 20, "angle": 30, "flip": flip})).unwrap();
            let id = r["canvas"].as_u64().unwrap();
            let before = world_corners(&s, id);
            let framed = s.execute("canvas.frame_sketch", &json!({"canvas": id})).unwrap();
            let c = s.doc.canvases.iter().find(|c| c.id == id).unwrap();
            let anchor = c.anchor.clone().unwrap();
            assert_eq!(Some(anchor.sketch), framed["sketch"].as_u64());
            assert_eq!(anchor.top_left.as_deref(), framed["top_left"].as_str());
            let after = world_corners(&s, id);
            assert!(before.iter().zip(&after).all(|(a, b)| (*a - *b).len() < 1e-6), "flip {flip}: {before:?} vs {after:?}");
            let sketch = s.doc.sketch(anchor.sketch).unwrap();
            assert_eq!(sketch.curves.iter().filter(|c| c.construction).count(), 4, "four construction sides");
            assert!(s.execute("canvas.frame_sketch", &json!({"canvas": id})).is_err(), "already anchored");
        }
    }

    /// An image put into the sketch being edited with `frame` gets its rectangle in that sketch,
    /// so a constraint to the sketch's own geometry and a dimension place and size it, while the
    /// sketch stays open; image and rectangle are one undo step. Frame Sketch does the same for an
    /// image already there. Off the sketch's plane, framing is refused and nothing is left behind.
    #[test]
    fn an_image_framed_in_the_open_sketch_follows_its_constraints_and_dimensions() {
        use solidcore_record::Vec3;
        use solidcraft_geom::prelude::*;
        let mut s = Session::default();
        let sketch = s.execute("sketch.create", &json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
        s.execute("sketch.line", &json!({"points": [[-50, -50], [-40, -45]]})).unwrap();
        let curves = s.doc.sketch(sketch).unwrap().curves.len();
        let r = s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": "XY", "center": [30, 20], "width": 20, "frame": true})).unwrap();
        let id = r["canvas"].as_u64().unwrap();
        assert_eq!(r["frame"]["sketch"].as_u64(), Some(sketch), "{r}");
        assert_eq!(s.active_sketch, Some(sketch), "the sketch stays open");
        assert_eq!(s.doc.sketch(sketch).unwrap().curves.len(), curves + 4, "four construction sides in the open sketch");
        let near = |a: Vec3, b: [f64; 3]| (a - Vec3::new(b[0], b[1], b[2])).len() < 1e-6;
        let k = world_corners(&s, id);
        assert!(near(k[0], [20.0, 15.0, 0.0]) && near(k[2], [40.0, 25.0, 0.0]), "the image stays where it was put: {k:?}");
        let (bl, br) = (r["frame"]["bottom_left"].as_str().unwrap(), r["frame"]["bottom_right"].as_str().unwrap());
        s.execute("sketch.constraint.coincident", &json!({"a": bl, "b": "origin"})).unwrap();
        let k = world_corners(&s, id);
        assert!(near(k[0], [0.0, 0.0, 0.0]), "its corner is on the sketch's origin: {k:?}");
        let sk = s.doc.sketch(sketch).unwrap();
        let point = |id: &str| sk.points.iter().position(|p| p.id == id).unwrap();
        let ends = (point(bl), point(br));
        let bottom = sk
            .curves
            .iter()
            .find(|c| matches!(c.kind, solidcraft_sketch::CurveKind::Line { a, b } if (a, b) == ends || (b, a) == ends))
            .map(|c| c.id.clone())
            .unwrap();
        s.execute("sketch.dimension", &json!({"entities": [bottom], "value": 30})).unwrap();
        let k = world_corners(&s, id);
        assert!(near(k[0], [0.0, 0.0, 0.0]) && ((k[1] - k[0]).len() - 30.0).abs() < 1e-6, "the dimension stretches it: {k:?}");
        assert_eq!(s.active_sketch, Some(sketch));
        // One undo step for each command since the image: the dimension, the coincidence, then
        // the image and its rectangle together.
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert!(s.doc.canvases.is_empty());
        assert_eq!(s.doc.sketch(sketch).unwrap().curves.len(), curves);
        // An image already on the open sketch's plane: Frame Sketch draws into that sketch.
        let id = s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": "XY", "width": 20})).unwrap()["canvas"].as_u64().unwrap();
        let framed = s.execute("canvas.frame_sketch", &json!({"canvas": id})).unwrap();
        assert_eq!(framed["sketch"].as_u64(), Some(sketch), "{framed}");
        assert_eq!(s.doc.sketch(sketch).unwrap().curves.len(), curves + 4);
        // Off its plane: refused, with no image left behind.
        let size = |s: &Session| {
            let sk = s.doc.sketch(sketch).unwrap();
            (s.doc.canvases.len(), sk.points.len(), sk.curves.len(), sk.constraints.len())
        };
        let before = size(&s);
        let e = s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": "XZ", "width": 20, "frame": true})).unwrap_err().to_string();
        assert!(e.contains("does not lie on the plane"), "{e}");
        assert_eq!(size(&s), before);
        assert!(s.execute("canvas.insert", &json!({"data": png(40, 20), "frame": "yes"})).is_err());
        // "auto", as the image tools send it: off the open sketch's plane the image is placed free,
        // on it the image is framed there.
        let free = s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": "XZ", "width": 20, "frame": "auto"})).unwrap();
        assert!(free.get("frame").is_none(), "{free}");
        let framed =
            s.execute("canvas.insert", &json!({"data": png(40, 20), "plane": "XY", "center": [80, 20], "width": 20, "frame": "auto"})).unwrap();
        assert_eq!(framed["frame"]["sketch"].as_u64(), Some(sketch), "{framed}");
    }

    /// An image framed inside a closed outline of the open sketch (here a 50 mm square) fills that
    /// outline: its corners are the square's, whatever size it was placed at, and no rectangle of
    /// its own is drawn.
    #[test]
    fn an_image_framed_inside_an_outline_fills_it() {
        use solidcore_record::Vec3;
        use solidcraft_geom::prelude::*;
        let mut s = Session::default();
        let sketch = s.execute("sketch.create", &json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [50, 50]})).unwrap();
        let curves = s.doc.sketch(sketch).unwrap().curves.len();
        let r = s.execute("canvas.insert", &json!({"data": png(40, 40), "plane": "XY", "center": [20, 30], "width": 100, "frame": true})).unwrap();
        let id = r["canvas"].as_u64().unwrap();
        assert_eq!(r["frame"]["sketch"].as_u64(), Some(sketch), "{r}");
        assert_eq!(s.doc.sketch(sketch).unwrap().curves.len(), curves, "no rectangle of its own");
        let near = |a: Vec3, b: [f64; 3]| (a - Vec3::new(b[0], b[1], b[2])).len() < 1e-6;
        let k = world_corners(&s, id);
        assert!(
            near(k[0], [0.0, 0.0, 0.0]) && near(k[1], [50.0, 0.0, 0.0]) && near(k[2], [50.0, 50.0, 0.0]) && near(k[3], [0.0, 50.0, 0.0]),
            "the image fills the square: {k:?}"
        );
    }

    /// An SVG decal is kept as the PNG it draws, at the resolution asked for.
    #[test]
    fn an_svg_decal_is_kept_as_the_png_it_draws() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "corner": [0, 0, 0]})).unwrap();
        let svg = base64_encode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10"><rect width="30" height="10" fill="red"/></svg>"#);
        let r = s.execute("canvas.decal", &json!({"data": svg, "face": [20, 15, 10], "width": 30, "resolution": 300})).unwrap();
        assert_eq!(r["pixels"], json!([300, 100]));
        let c = s.doc.canvases.last().unwrap();
        assert_eq!(c.format, "png");
        assert!(c.bytes().unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
        for resolution in [json!(0), json!(5000), json!("big")] {
            assert!(s.execute("canvas.decal", &json!({"data": svg, "face": [20, 15, 10], "resolution": resolution})).is_err(), "{resolution}");
        }
    }
}
