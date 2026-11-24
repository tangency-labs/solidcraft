//! `shaft_spline.insert`: a spline on a picked shaft, in a picked bore, or both, or with nothing picked a new
//! splined shaft, its hub, or both, from the kernel's exact tooth outlines (`solidcore::teeth`), with a
//! master spline when asked for.
//!
//! The command adds ONE feature, `FeatureKind::ShaftSpline` (#17693): it stores the picked faces and every number
//! as an expression, so the numbers stay parameters, and the rebuild cuts the spline again each time
//! (`crates/doc`). The checks and the outlines are the kernel's, asked through the doc crate
//! ([`solidcraft_doc::shaft_spline::plan`]): the command works the expressions out to numbers and asks the same
//! plan first, so it refuses a bad value with the same words the rebuild would give, before anything is added.
//! It is edited through `timeline.redefine`.

use std::collections::HashMap;

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::expr::{Kind, is_bare_number};
use solidcraft_doc::shaft_spline::{
    MasterArc, SplineCorner, SplineForm, SplineMake, SplineMaster as MasterKind, SplineNumbers, SplinePlan, ToothOutline, outline_points, plan,
};
use solidcraft_doc::{Expr, FacePick, FeatureKind};

use super::CommandSpec;
use super::features::add_feature;
use crate::params::{bad, req_expr, vec3};
use crate::schema::shaft_spline::{
    MasterSplineMade, MasterSplineParam, ShaftSplineAdded, ShaftSplineNumbers, ShaftSplineParams, SplineKindParam, SplineMakeParam, TipCornerParam,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

/// Why the numbers make nothing: the parameter the refusal names, when it names one, and the words. The doc
/// crate's type, which the rebuild gives too.
pub use solidcraft_doc::shaft_spline::SplineRefusal;

const CMD: &str = "shaft_spline.insert";
const CONTRACT: TypedContract<ShaftSplineParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new("shaft_spline.insert", "Shaft Spline", crate::checked!("shaft_spline.insert", shaft_spline))
        .at("SOLID", "CREATE")
        .icon("shaft_spline")
        .contract(&CONTRACT)
        .output(output_schema_for::<ShaftSplineAdded>)
        .params(
            "Adds one Shaft Spline feature. Each number is a number or an expression (lengths in the design's unit, angles in \
         degrees). shaft_face?: [x,y,z] a point on the outside of a shaft to cut the spline into; bore_face?: [x,y,z] a \
         point on a bore to cut the hub's spline into (with a face picked, make is not needed and hub_diameter is refused); \
         shaft_start?, bore_start?: [x,y,z] a point on the face the teeth start from (the part's end, a shoulder, a \
         chamfer): they start at the end of the round face nearest it and run into the face for length (left out: \
         the end nearest shaft_face or bore_face; each only with its face); \
         type: involute|straight|serration; teeth (3..2000 involute, 2..2000 otherwise); length: at least 0.001 mm; \
         make?: shaft|hub|both (default shaft; with no face picked); hub_diameter?: the outside of a new hub (required \
         with a new hub, refused otherwise); with shaft_face, the size may be left out (module for involute, \
         major_diameter otherwise): the spline then takes the shaft's own diameter, and with the size given the shaft \
         is turned down or built up to it; involute: module, pressure_angle? (deg, default 30), addendum?, dedendum?, \
         root_fillet?, profile_shift?, hub_addendum? (modules; defaults 0.5, 0.85, 0.4, 0, 0.5), thinning? (default 0), \
         side_clearance (both sides on the pitch circle), tip_clearance, form_clearance? (default 0), hub_root_round? \
         (default 0); straight and serration: major_diameter, minor_diameter, tooth_width (straight) | flank_angle? \
         (serration, deg, default 90), root_round? (default 0), tip_corner?: plain|chamfered|rounded (default plain) with \
         tip_size, side_clearance (each side, square to the flanks), major_clearance, minor_clearance, hub_root_round?, \
         hub_tip_round? (default 0); master_spline?: none|tooth_out|wide_tooth (default none) with master_angle? (deg \
         from angle 0, -360..360, default 0, refused without a master spline; the whole spline turns to put the master \
         spline exactly there); cutter_diameter?: each tooth space cut into a picked shaft ends in the cutter's curve \
         (without it the spaces end square); end_chamfer?: a chamfer on the ends of the shaft's teeth, this far \
         along the shaft (alone: at 45 degrees, as far in from the major diameter), with end_chamfer_distance2? \
         (how far in from the major diameter) or end_chamfer_angle? (deg to the shaft's surface, above 0 and below \
         90), not both; entry_chamfer?, entry_chamfer_distance2?, entry_chamfer_angle?: the same where the teeth run \
         out of the hub's bore, out from the tops of the hub's teeth (on a picked face: on the end the teeth start \
         from, and on the far end where the teeth run the whole face; on a new part: on both ends; each cut by the \
         spline, so it is made again when the size or the teeth change; default none; a second distance or an \
         angle only with its chamfer); name? → the feature, the bodies, the diameters, clearances, turning play \
         (deg) and positions",
        )];

/// The keys only involute teeth read, and those only straight-sided and serrated teeth read.
const INVOLUTE_KEYS: &[&str] = &[
    "module",
    "pressure_angle",
    "addendum",
    "dedendum",
    "root_fillet",
    "profile_shift",
    "thinning",
    "hub_addendum",
    "tip_clearance",
    "form_clearance",
];
const STRAIGHT_KEYS: &[&str] = &[
    "major_diameter",
    "minor_diameter",
    "tooth_width",
    "flank_angle",
    "root_round",
    "tip_corner",
    "tip_size",
    "major_clearance",
    "minor_clearance",
    "hub_tip_round",
];

/// A number the call may give as a JSON number: absent or `null` is none; anything else must be a finite number.
fn number(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| bad(CMD, format!("`{key}` must be a number"))),
    }
}

/// A number the call may give as a number or an expression, with the number it works out to in the design: a
/// length in mm (a bare number in the design's unit), an angle in degrees, anything else as it is. Absent or
/// `null` is none.
fn worked_out(s: &Session, p: &Value, key: &str, kind: Kind) -> Result<Option<(Expr, f64)>> {
    if p.get(key).is_none_or(Value::is_null) {
        return Ok(None);
    }
    let e = req_expr(CMD, p, key)?;
    let v = if kind == Kind::Angle && is_bare_number(&e) {
        // A bare angle is degrees as it stands.
        s.doc.eval(&e, Kind::Unitless)
    } else {
        let typed = s.doc.with_design_unit(&e, kind);
        s.doc.eval(&typed, kind).map(|v| if kind == Kind::Angle { v.to_degrees() } else { v })
    };
    let v = v.map_err(|err| bad(CMD, format!("{key}: {err}")))?;
    Ok(Some((e, v)))
}

/// A point the call may give: absent or `null` is none.
fn face_point(p: &Value, key: &str) -> Result<Option<Vec3>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => vec3(v).map(Some).ok_or_else(|| bad(CMD, format!("`{key}` must be a point [x, y, z]"))),
    }
}

/// A keyword the call may give, as its contract's type reads it.
fn keyword<T: serde::de::DeserializeOwned>(p: &Value, key: &str, allowed: &str) -> Result<Option<T>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|_| bad(CMD, format!("`{key}` must be {allowed}"))),
    }
}

/// What a key of another kind of teeth is for.
fn kind_of_key(key: &str) -> &'static str {
    match key {
        "tooth_width" => "straight-sided teeth",
        "flank_angle" => "serrations",
        k if INVOLUTE_KEYS.contains(&k) => "involute teeth",
        _ => "straight-sided teeth and serrations",
    }
}

/// Whether the call gives `key` a value (not `null`).
fn given(p: &Value, key: &str) -> bool {
    p.get(key).is_some_and(|v| !v.is_null())
}

fn form_of(k: SplineKindParam) -> SplineForm {
    match k {
        SplineKindParam::Involute => SplineForm::Involute,
        SplineKindParam::Straight => SplineForm::Straight,
        SplineKindParam::Serration => SplineForm::Serration,
    }
}

fn make_of(m: SplineMakeParam) -> SplineMake {
    match m {
        SplineMakeParam::Shaft => SplineMake::Shaft,
        SplineMakeParam::Hub => SplineMake::Hub,
        SplineMakeParam::Both => SplineMake::Both,
    }
}

fn corner_of(c: TipCornerParam) -> SplineCorner {
    match c {
        TipCornerParam::Plain => SplineCorner::Plain,
        TipCornerParam::Chamfered => SplineCorner::Chamfered,
        TipCornerParam::Rounded => SplineCorner::Rounded,
    }
}

fn master_of(m: MasterSplineParam) -> MasterKind {
    match m {
        MasterSplineParam::None => MasterKind::None,
        MasterSplineParam::ToothOut => MasterKind::ToothOut,
        MasterSplineParam::WideTooth => MasterKind::WideTooth,
    }
}

/// What a call asks for, read and checked as far as the command sees it (the keys, the keywords, which parts),
/// with its numbers for [`plan`].
struct Asked {
    kind: SplineKindParam,
    /// `make` as given (default shaft): which new part is made when no face is picked.
    make: SplineMakeParam,
    corner: TipCornerParam,
    master: MasterSplineParam,
    numbers: SplineNumbers,
}

/// Reads a call's keys and numbers. `shaft` and `bore` say which faces are picked; `read` gives each number in
/// mm, degrees or as it is (a JSON number for a preview, an expression worked out for the command).
fn asked(p: &Value, shaft: bool, bore: bool, read: &mut dyn FnMut(&str, Kind) -> Result<Option<f64>>) -> Result<Asked> {
    if !p.is_object() {
        return Err(bad(CMD, "the parameters must be an object"));
    }
    let kind = keyword::<SplineKindParam>(p, "type", "involute, straight or serration")?
        .ok_or_else(|| bad(CMD, "`type` is required: involute, straight or serration"))?;
    let teeth = match read("teeth", Kind::Unitless)? {
        Some(t) if t >= 0.0 && t.fract() == 0.0 && t <= f64::from(u32::MAX) => t as u32,
        _ => return Err(bad(CMD, "`teeth` must be a whole number")),
    };
    let length = read("length", Kind::Length)?.ok_or_else(|| bad(CMD, "`length` is required (mm)"))?;
    let make = keyword::<SplineMakeParam>(p, "make", "shaft, hub or both")?.unwrap_or(SplineMakeParam::Shaft);
    // A key of the other kinds of teeth would be ignored: say so instead.
    let (others, own) = match kind {
        SplineKindParam::Involute => (STRAIGHT_KEYS, "involute teeth"),
        SplineKindParam::Straight => (INVOLUTE_KEYS, "straight-sided teeth"),
        SplineKindParam::Serration => (INVOLUTE_KEYS, "serrations"),
    };
    let foreign = match kind {
        SplineKindParam::Straight => Some("flank_angle"),
        SplineKindParam::Serration => Some("tooth_width"),
        SplineKindParam::Involute => None,
    };
    if let Some(key) = others.iter().copied().chain(foreign).find(|k| given(p, k)) {
        return Err(bad(CMD, format!("`{key}` does not apply to {own} (it is for {})", kind_of_key(key))));
    }
    let picked = shaft || bore;
    // A new hub is made only with no face picked; a picked bore is cut instead.
    let new_hub = !picked && make != SplineMakeParam::Shaft;
    if !new_hub && given(p, "hub_diameter") {
        return Err(if picked {
            bad(CMD, "`hub_diameter` is the outside diameter of a new hub: leave it out when a shaft or bore face is picked")
        } else {
            bad(CMD, "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub or both")
        });
    }
    let hub_diameter = read("hub_diameter", Kind::Length)?;
    if new_hub && hub_diameter.is_none() {
        return Err(bad(CMD, "`hub_diameter` is required to make the hub (mm, its outside diameter)"));
    }
    // The cutter and the two chamfers, each with its second distance or angle, belong to a part: a key for a part
    // that is neither cut nor made would be ignored, so it is refused.
    let (with_shaft, with_hub) = if picked { (shaft, bore) } else { (make != SplineMakeParam::Hub, make != SplineMakeParam::Shaft) };
    if !shaft && given(p, "shaft_start") {
        return Err(bad(CMD, "`shaft_start` is the face the teeth of a picked shaft start from: give it only with `shaft_face`"));
    }
    if !bore && given(p, "bore_start") {
        return Err(bad(CMD, "`bore_start` is the face the teeth of a picked bore start from: give it only with `bore_face`"));
    }
    if !shaft && given(p, "cutter_diameter") {
        return Err(bad(CMD, "`cutter_diameter` is the cutter that ends the tooth spaces cut into a picked shaft: give it only with `shaft_face`"));
    }
    if !with_shaft && given(p, "end_chamfer") {
        return Err(bad(CMD, "`end_chamfer` is the chamfer on the ends of the shaft's teeth: give it only when a shaft is cut or made"));
    }
    if !with_hub && given(p, "entry_chamfer") {
        return Err(bad(CMD, "`entry_chamfer` is the chamfer where the teeth run out of the hub's bore: give it only when a hub is cut or made"));
    }
    for (key, what) in [("end_chamfer_distance2", "the distance in from the major diameter"), ("end_chamfer_angle", "the angle")] {
        if !with_shaft && given(p, key) {
            return Err(bad(
                CMD,
                format!("`{key}` is {what} of the chamfer on the ends of the shaft's teeth: give it only when a shaft is cut or made"),
            ));
        }
    }
    for (key, what) in [("entry_chamfer_distance2", "the distance out from the tops of the hub's teeth"), ("entry_chamfer_angle", "the angle")] {
        if !with_hub && given(p, key) {
            return Err(bad(
                CMD,
                format!("`{key}` is {what} of the chamfer where the teeth run out of the hub's bore: give it only when a hub is cut or made"),
            ));
        }
    }
    let master = keyword::<MasterSplineParam>(p, "master_spline", "none, tooth_out or wide_tooth")?.unwrap_or(MasterSplineParam::None);
    if master == MasterSplineParam::None && given(p, "master_angle") {
        return Err(bad(CMD, "`master_angle` is where the master spline sits: give it only with `master_spline` tooth_out or wide_tooth"));
    }
    let master_angle = match master {
        MasterSplineParam::None => None,
        _ => Some(read("master_angle", Kind::Angle)?.unwrap_or(0.0)),
    };
    let corner = match kind {
        SplineKindParam::Involute => TipCornerParam::Plain,
        _ => keyword::<TipCornerParam>(p, "tip_corner", "plain, chamfered or rounded")?.unwrap_or(TipCornerParam::Plain),
    };
    let numbers = SplineNumbers {
        form: form_of(kind),
        // With a face picked nothing new is made, so the plan's checks of a new hub do not apply: the same
        // numbers the rebuild gives it (make shaft, no hub diameter, the parts in with_shaft and with_hub).
        make: if picked { SplineMake::Shaft } else { make_of(make) },
        teeth,
        length,
        module: read("module", Kind::Length)?,
        pressure_angle: read("pressure_angle", Kind::Angle)?,
        addendum: read("addendum", Kind::Unitless)?,
        dedendum: read("dedendum", Kind::Unitless)?,
        root_fillet: read("root_fillet", Kind::Unitless)?,
        profile_shift: read("profile_shift", Kind::Unitless)?,
        thinning: read("thinning", Kind::Length)?,
        hub_addendum: read("hub_addendum", Kind::Unitless)?,
        tip_clearance: read("tip_clearance", Kind::Length)?,
        form_clearance: read("form_clearance", Kind::Length)?,
        major_diameter: read("major_diameter", Kind::Length)?,
        minor_diameter: read("minor_diameter", Kind::Length)?,
        tooth_width: read("tooth_width", Kind::Length)?,
        flank_angle: read("flank_angle", Kind::Angle)?,
        root_round: read("root_round", Kind::Length)?,
        tip_corner: corner_of(corner),
        tip_size: read("tip_size", Kind::Length)?,
        major_clearance: read("major_clearance", Kind::Length)?,
        minor_clearance: read("minor_clearance", Kind::Length)?,
        hub_tip_round: read("hub_tip_round", Kind::Length)?,
        side_clearance: read("side_clearance", Kind::Length)?,
        hub_root_round: read("hub_root_round", Kind::Length)?,
        hub_diameter: hub_diameter.filter(|_| new_hub),
        master: master_of(master),
        master_angle,
        cutter_diameter: read("cutter_diameter", Kind::Length)?,
        end_chamfer: read("end_chamfer", Kind::Length)?,
        entry_chamfer: read("entry_chamfer", Kind::Length)?,
        end_chamfer_distance2: read("end_chamfer_distance2", Kind::Length)?,
        end_chamfer_angle: read("end_chamfer_angle", Kind::Angle)?,
        entry_chamfer_distance2: read("entry_chamfer_distance2", Kind::Length)?,
        entry_chamfer_angle: read("entry_chamfer_angle", Kind::Angle)?,
        with_hub,
        with_shaft,
    };
    Ok(Asked { kind, make, corner, master, numbers })
}

/// Where the master spline is, as the reply says it: the angle asked for and the engine's placement.
fn master_made(asked: &Asked, planned: &SplinePlan) -> Option<MasterSplineMade> {
    planned.master.as_ref().filter(|_| asked.master != MasterSplineParam::None).map(|m| MasterSplineMade {
        form: asked.master,
        angle: asked.numbers.master_angle.unwrap_or(0.0),
        tooth: m.tooth,
        turn: m.turn.to_degrees(),
    })
}

/// The kernel's numbers for the feature's reply.
fn feature_numbers(asked: &Asked, planned: &SplinePlan) -> ShaftSplineNumbers {
    let m = &planned.made;
    ShaftSplineNumbers {
        kind: asked.kind,
        teeth: m.teeth,
        shaft_major_diameter: m.shaft_major_diameter,
        shaft_minor_diameter: m.shaft_minor_diameter,
        hub_major_diameter: m.hub_major_diameter,
        hub_minor_diameter: m.hub_minor_diameter,
        pitch_diameter: m.pitch_diameter,
        base_diameter: m.base_diameter,
        form_diameter: m.form_diameter,
        undercut: m.undercut,
        sharp_tip_diameter: m.sharp_tip_diameter,
        side_clearance_each: m.side_clearance_each,
        tip_gap: m.tip_gap,
        root_gap: m.root_gap,
        form_gap: m.form_gap,
        turning_play: m.turning_play,
        positions: m.positions,
        master_spline: master_made(asked, planned),
        flank_tolerance: m.flank_tolerance,
    }
}

/// A refusal of the plan as this command's.
fn refused(r: SplineRefusal) -> EngineError {
    bad(CMD, r.words)
}

/// The command: one Shaft Spline feature.
fn shaft_spline(s: &mut Session, p: &Value) -> Result<Value> {
    add_spline(s, p)
}

/// Adds the one Shaft Spline feature: the picks and every number as given (expressions stay expressions), after
/// the same plan the rebuild makes has accepted the numbers they work out to.
fn add_spline(s: &mut Session, p: &Value) -> Result<Value> {
    let shaft = face_point(p, "shaft_face")?;
    let bore = face_point(p, "bore_face")?;
    let shaft_start = face_point(p, "shaft_start")?;
    let bore_start = face_point(p, "bore_start")?;
    let mut exprs: HashMap<String, Expr> = HashMap::new();
    let mut asked = {
        let at: &Session = s;
        let mut read = |key: &str, kind: Kind| -> Result<Option<f64>> {
            Ok(worked_out(at, p, key, kind)?.map(|(e, v)| {
                exprs.insert(key.to_string(), e);
                v
            }))
        };
        asked(p, shaft.is_some(), bore.is_some(), &mut read)?
    };
    // A size left out with a shaft picked is the shaft's own, so the shaft keeps its diameter, and so is a size
    // given that is the shaft's own but for its last places; any other size given wins, and the shaft follows it.
    // The rebuild reads the shaft off the picked face, and so does this check where the face is found. Where it
    // is not, the rebuild says what is wrong with the pick.
    let fitting = match asked.kind {
        SplineKindParam::Involute => asked.numbers.module.is_none(),
        SplineKindParam::Straight | SplineKindParam::Serration => asked.numbers.major_diameter.is_none(),
    };
    let mut known = true;
    if let Some(at) = shaft {
        match solidcraft_doc::shaft_radius_at(&s.model.state(), at) {
            Some(radius) => solidcraft_doc::shaft_spline::fit_to_shaft_within(&mut asked.numbers, radius),
            None => known = !fitting,
        }
    }
    let planned = if known { Some(plan(&asked.numbers).map_err(refused)?) } else { None };
    if s.active_sketch.is_some() {
        return Err(bad(CMD, "a sketch is being edited: finish it first (sketch.finish)"));
    }
    // A start left out is the end of the round face nearest its pick. It is stored as a point at that end, so
    // the spline keeps its end when the part is made longer or shorter later.
    let (shaft_start, bore_start) = {
        let state = s.model.state();
        (
            shaft_start.or_else(|| shaft.and_then(|at| solidcraft_doc::spline_start_at(&state, at, false))),
            bore_start.or_else(|| bore.and_then(|at| solidcraft_doc::spline_start_at(&state, at, true))),
        )
    };
    let mut take = |key: &str| exprs.remove(key);
    let teeth = take("teeth").ok_or_else(|| bad(CMD, "`teeth` must be a whole number"))?;
    let length = take("length").ok_or_else(|| bad(CMD, "`length` is required (mm)"))?;
    let side_clearance = take("side_clearance").ok_or_else(|| bad(CMD, "`side_clearance` is required (mm)"))?;
    let kind = FeatureKind::ShaftSpline {
        shaft: shaft.map(FacePick),
        shaft_start: shaft_start.map(FacePick),
        bore: bore.map(FacePick),
        bore_start: bore_start.map(FacePick),
        form: form_of(asked.kind),
        make: make_of(asked.make),
        teeth,
        length,
        module: take("module"),
        pressure_angle: take("pressure_angle"),
        addendum: take("addendum"),
        dedendum: take("dedendum"),
        root_fillet: take("root_fillet"),
        profile_shift: take("profile_shift"),
        thinning: take("thinning"),
        hub_addendum: take("hub_addendum"),
        tip_clearance: take("tip_clearance"),
        form_clearance: take("form_clearance"),
        major_diameter: take("major_diameter"),
        minor_diameter: take("minor_diameter"),
        tooth_width: take("tooth_width"),
        flank_angle: take("flank_angle"),
        root_round: take("root_round"),
        tip_corner: corner_of(asked.corner),
        tip_size: take("tip_size"),
        major_clearance: take("major_clearance"),
        minor_clearance: take("minor_clearance"),
        hub_tip_round: take("hub_tip_round"),
        side_clearance,
        hub_root_round: take("hub_root_round"),
        hub_diameter: take("hub_diameter").filter(|_| asked.numbers.hub_diameter.is_some()),
        master: master_of(asked.master),
        master_angle: take("master_angle").filter(|_| asked.master != MasterSplineParam::None),
        cutter_diameter: take("cutter_diameter"),
        end_chamfer: take("end_chamfer"),
        entry_chamfer: take("entry_chamfer"),
        end_chamfer_distance2: take("end_chamfer_distance2"),
        end_chamfer_angle: take("end_chamfer_angle"),
        entry_chamfer_distance2: take("entry_chamfer_distance2"),
        entry_chamfer_angle: take("entry_chamfer_angle"),
    };
    let mut reply = add_feature(s, p, kind)?;
    if let Some(planned) = &planned {
        let numbers = serde_json::to_value(feature_numbers(&asked, planned)).map_err(|e| EngineError::Other(e.to_string()))?;
        if let (Some(out), Value::Object(more)) = (reply.as_object_mut(), numbers) {
            out.extend(more);
        }
    }
    Ok(reply)
}

/// Which part an outline of a preview is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplinePart {
    Shaft,
    Hub,
}

/// A circle of a spline worth showing in a drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplineCircle {
    ShaftMajor,
    ShaftMinor,
    /// The pitch circle (involute teeth only).
    Pitch,
    HubMajor,
    HubMinor,
    /// The outside of the hub's body.
    HubOutside,
}

/// A part's outline as a closed chain of points in millimetres, counter-clockwise, where the command builds it.
#[derive(Clone, Debug, PartialEq)]
pub struct SplineOutline {
    pub part: SplinePart,
    /// Whether the command makes or cuts this part. The other one is given too, so a drawing can show what it
    /// fits.
    pub made: bool,
    pub points: Vec<[f64; 2]>,
}

/// An arc about the axis: its radius in millimetres and its angles in radians, counter-clockwise from +X.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplineArc {
    pub radius: f64,
    pub start: f64,
    pub end: f64,
}

/// The master spline of a preview.
#[derive(Clone, Debug, PartialEq)]
pub struct SplineMaster {
    pub form: MasterSplineParam,
    /// Where it sits, in degrees counter-clockwise from +X: the angle asked for.
    pub angle: f64,
    /// How far the whole spline was turned to put it there, in degrees.
    pub turn: f64,
    /// The arc on the shaft that takes the place of a tooth, or fills a space.
    pub shaft_arc: SplineArc,
    /// The same on the hub, laid over the shaft.
    pub hub_arc: SplineArc,
}

/// What `shaft_spline.insert` would make from its parameters, for a drawing. Nothing is built.
#[derive(Debug)]
pub struct SplinePreview {
    pub outlines: Vec<SplineOutline>,
    /// The circles worth showing, each with its radius in millimetres.
    pub circles: Vec<(SplineCircle, f64)>,
    pub master: Option<SplineMaster>,
    /// The angle from one tooth to the next, in radians.
    pub pitch: f64,
    /// The angle of the middle of a tooth that is there, in radians counter-clockwise from +X: tooth 0, or with a
    /// master spline the tooth opposite it.
    pub tooth_at: f64,
    /// The numbers the command's reply would carry.
    pub numbers: ShaftSplineNumbers,
}

/// A refusal of this command as a dialog needs it. The parameter is the first word in backticks: every refusal
/// here starts with the parameter it is about.
fn refusal(e: EngineError) -> SplineRefusal {
    let words = match e {
        EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    };
    // A parameter's name is lowercase letters, digits and `_` (`end_chamfer_distance2`).
    let is_name = |k: &&str| !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    let key = words.split('`').nth(1).filter(is_name).map(str::to_string);
    SplineRefusal { key, words }
}

/// What `shaft_spline.insert` would make from `p`, or its refusal: the same checks and the same plan as the
/// command and the rebuild, without a session. Every number is a JSON number (the dialog works its expressions
/// out first). A dialog draws this, so it never shows what the command would refuse.
pub fn shaft_spline_preview(p: &Value) -> std::result::Result<SplinePreview, SplineRefusal> {
    let mut read = |key: &str, _: Kind| -> Result<Option<f64>> { number(p, key) };
    let asked = asked(p, given(p, "shaft_face"), given(p, "bore_face"), &mut read).map_err(refusal)?;
    let planned = plan(&asked.numbers)?;
    let reply = feature_numbers(&asked, &planned);
    let hub_diameter = asked.numbers.hub_diameter;
    let mut circles =
        vec![(SplineCircle::ShaftMajor, reply.shaft_major_diameter / 2.0), (SplineCircle::ShaftMinor, reply.shaft_minor_diameter / 2.0)];
    if let Some(d) = reply.pitch_diameter {
        circles.push((SplineCircle::Pitch, d / 2.0));
    }
    circles.push((SplineCircle::HubMajor, reply.hub_major_diameter / 2.0));
    circles.push((SplineCircle::HubMinor, reply.hub_minor_diameter / 2.0));
    if let Some(od) = hub_diameter {
        circles.push((SplineCircle::HubOutside, od / 2.0));
    }
    // The outlines as the kernel's points: the hub's turned to lie on the shaft.
    let points = |outline: &ToothOutline, turn: f64| outline_points(outline, turn).map_err(|words| SplineRefusal { key: None, words });
    let outlines = vec![
        SplineOutline { part: SplinePart::Shaft, made: asked.numbers.with_shaft, points: points(&planned.shaft, 0.0)? },
        SplineOutline { part: SplinePart::Hub, made: asked.numbers.with_hub, points: points(&planned.hub, planned.hub_start)? },
    ];
    // The pitch of the teeth as counted, the tooth the close-up shows and the master spline's arcs, as the kernel's
    // plan gives them.
    let (pitch, tooth_at) = (planned.tooth_pitch, planned.tooth_at);
    let master = planned.master.as_ref().filter(|_| asked.master != MasterSplineParam::None).map(|made| {
        let arc = |s: MasterArc| SplineArc { radius: s.radius, start: s.start, end: s.end };
        SplineMaster {
            form: asked.master,
            angle: made.middle.to_degrees(),
            turn: made.turn.to_degrees(),
            shaft_arc: arc(made.shaft_arc),
            hub_arc: arc(made.hub_arc_on_shaft),
        }
    });
    Ok(SplinePreview { outlines, circles, master, pitch, tooth_at, numbers: reply })
}

#[cfg(test)]
#[path = "shaft_spline_tests.rs"]
mod tests;
