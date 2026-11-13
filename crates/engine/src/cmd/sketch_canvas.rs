//! Canvases (reference images on planes) and decals (images on planar faces).

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::canvas::{Canvas, MAX_CANVAS_BYTES, base64_decode, base64_encode, image_info};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, vec2};
use crate::schema::canvas::{
    CanvasCalibrateParams, CanvasCalibrated, CanvasDecalParams, CanvasDeleteParams, CanvasDeleted, CanvasEditParams, CanvasEdited,
    CanvasInsertParams, CanvasPlaced, CanvasStraightenParams, CanvasStraightened,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INSERT_CONTRACT: TypedContract<CanvasInsertParams> = TypedContract::new();
const DECAL_CONTRACT: TypedContract<CanvasDecalParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<CanvasEditParams> = TypedContract::new();
const CALIBRATE_CONTRACT: TypedContract<CanvasCalibrateParams> = TypedContract::new();
const STRAIGHTEN_CONTRACT: TypedContract<CanvasStraightenParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<CanvasDeleteParams> = TypedContract::new();

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
        .params("path | data (base64 PNG or JPEG); plane?: XY|XZ|YZ|{face: [x,y,z]}|… (default XY); center?: [x,y] on the plane, or at?: [x,y,z]; width?: mm (default 100); angle?: deg; opacity?: 0…1 (default 0.5); flip?; name?"),
    CommandSpec::new("canvas.decal", "Decal", crate::checked!("canvas.decal", add_decal))
        .at("SKETCH", "INSERT")
        .icon("decal")
        .contract(&DECAL_CONTRACT)
        .output(output_schema_for::<CanvasPlaced>)
        .params("path | data; face: [x,y,z] on a planar face (the image is centred there), or the picked face as sketch.create's plane takes it ({face, body, index, path?}); width?: mm; angle?: deg; opacity?: (default 1)"),
    CommandSpec::new("canvas.edit", "Edit Canvas", crate::checked!("canvas.edit", edit_canvas))
        .at("SKETCH", "INSERT")
        .icon("canvas")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<CanvasEdited>)
        .params("canvas: id or name; center?, width?, angle?: deg, opacity?, flip?, visible?, name?, locked? (a locked canvas refuses center, width, angle, flip, Calibrate and Delete)"),
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
    let bytes = image_bytes(p, cmd)?;
    let (format, pixels) = image_info(&bytes).ok_or_else(|| bad(cmd, "only PNG and JPEG images can be inserted"))?;
    let plane = super::sketch::plane_ref(s, p, cmd)?;
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
    });
    canvas_reply(cmd, CanvasPlaced { canvas: id, name, pixels, param_warnings: Vec::new() })
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

fn edit_canvas(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.edit";
    let i = find(s, p, cmd)?;
    let width = num(p, "width");
    if width.is_some_and(|w| !(w > 1e-6 && w < 1e7)) {
        return Err(bad(cmd, "`width` must be positive"));
    }
    // Unlocking in the same call lets the other changes through.
    let moves = ["center", "width", "angle", "flip"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()));
    if moves && bool_(p, "locked") != Some(false) {
        refuse_locked(s, i, cmd)?;
    }
    let c = s.doc_mut().canvases.get_mut(i).ok_or_else(|| bad(cmd, "canvas"))?;
    if let Some(l) = bool_(p, "locked") {
        c.locked = l;
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
}
