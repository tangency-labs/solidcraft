//! `drawing.dimension.from_model` (#13054): the model's own dimensions in a view of the open
//! drawing. A sketch's dimension is found on the model at its sketch's solved points, lifted onto
//! the sketch's plane and picked as `drawing.dimension.add` picks a point
//! ([`solidcraft_doc::Document::edge_reference`], [`solidcraft_doc::Document::corner_reference`]); an
//! extrude's distance is the distance between its two caps, by name. Each is placed
//! as `add` places one ([`placed`]) and kept only when it reads the model dimension's own value, so
//! an edge the sketch didn't make is never taken for one it did. A dimension saves which model
//! dimension it shows, and one a dimension of the drawing already shows is skipped, so each shows
//! once. Everything added is one undo step; nothing is laid out beyond `add`'s default place.

use std::collections::HashSet;
use std::f64::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::coverage::{is_cap, made_by};
use solidcraft_doc::drawing::dimension::{CornerPick, Dimension, DimensionKind, EdgePick, ModelDimension, ModelFace, Status};
use solidcraft_doc::{Feature, FeatureKind, ModelState, SolvedSketch, ids, naming};
use solidcraft_geom::drawing_measure::{LinearDirection, Reading};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{Constraint, ConstraintKind, CurveKind};

use super::CommandSpec;
use super::drawing_dimension::{NewDimension, Refused, placed};
use crate::params::bad;
use crate::schema::drawing_model_dimensions::{Added, FromModelParams, FromModelResult, ModelSource, SkipReason, Skipped, SourceKind};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const FROM_MODEL: &str = "drawing.dimension.from_model";
const CONTRACT: TypedContract<FromModelParams> = TypedContract::new();
/// How far a reading may be from its model dimension's value, for each unit of the value (model
/// millimetres, or radians for an angle), and at least: both come from exact geometry.
const AGREES: f64 = 1e-6;
/// How near the sketch's normal is to the view's for the view to see the sketch face on.
const FACE_ON: f64 = 1e-9;
/// Where a whole circle is picked, in radians from its sketch's x: off the quarter turns, where a
/// circle's side face may have its seam.
const ON_CIRCLE: f64 = 1.0;
/// An extrude's parameter this command brings in, by its label.
const EXTRUDE_DISTANCE: &str = "Distance";

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new(FROM_MODEL, "Model Dimensions", crate::checked!("drawing.dimension.from_model", from_model))
        .icon("dimension")
        .in_drawing()
        .doc_only()
        .contract(&CONTRACT)
        .output(output_schema_for::<FromModelResult>)
        .params(
            "view: a saved view's id; sketch?: a sketch's id or name (only its dimensions); feature?: a feature's id or name (only its \
             own dimensions and its sketch's); body?: a body's name (only dimensions of features that made a face of it); the filters \
             narrow together → {added: [{dimension, from}], skipped: [{from, reason, why, shown_as?}]}, in timeline order, then the \
             sketch's own order: the model's own dimensions brought into the view as dimensions of the drawing, each placed where \
             drawing.dimension.add places one (they can overlap: nothing is laid out). from is {kind: sketch, sketch, constraint} or \
             {kind: feature, feature, param}. A sketch's Length, Distance, DistanceX, DistanceY, Radius, Diameter and Angle come in on a \
             view that sees the sketch face on, found at the sketch's points on the model; an extrude's Distance comes in as the \
             distance between its two caps. Each is kept only when it reads the model dimension's value. reason is one \
             of a closed list: already_shown (a dimension of the drawing shows it: shown_as), not_supported, driven (a driven sketch \
             dimension), not_face_on, not_found (no edge, corner or face, or several, at its points), not_drawn (the view doesn't \
             draw its body), unmeasurable, value_differs (what was found reads another value), in_component (a sketch or feature \
             of a placed component, not supported yet). Each model dimension shows once on the drawing: deleting its dimension \
             frees it. One undo step; none when nothing is added.",
        )];

/// What one model dimension is, and how it becomes a drawing dimension, or why it doesn't.
struct Candidate {
    from: ModelSource,
    model: ModelDimension,
    plan: std::result::Result<Measure, (SkipReason, String)>,
}

/// A drawing dimension to place, and the value it must read.
struct Measure {
    kind: DimensionKind,
    expected: f64,
    angle: bool,
}

fn from_model(s: &mut Session, p: &Value) -> Result<Value> {
    let request: FromModelParams = crate::schema::read(FROM_MODEL, p)?;
    let record = s.drawing(FROM_MODEL)?.clone();
    let view = request.view.0;
    record.view(view).ok_or_else(|| bad(FROM_MODEL, format!("the drawing has no view {}", ids::encode(view))))?;
    let basis = record.resolve_view(view).map_err(|e| bad(FROM_MODEL, e.to_string()))?.basis;
    let candidates = candidates(s, &request, &basis)?;
    let mut staged = Drawing::clone(&record);
    let (mut added, mut skipped, mut made) = (Vec::new(), Vec::new(), Vec::new());
    for candidate in candidates {
        let skip = |reason, why: String| Skipped { from: candidate.from.clone(), reason, why, shown_as: None };
        if let Some(shown) = staged.showing(&candidate.model) {
            let why = format!("dimension {} shows it", ids::encode(shown));
            skipped.push(Skipped { shown_as: Some(ids::encode(shown)), ..skip(SkipReason::AlreadyShown, why) });
            continue;
        }
        let measure = match candidate.plan {
            Ok(measure) => measure,
            Err((reason, why)) => {
                skipped.push(skip(reason, why));
                continue;
            }
        };
        match place(s, &mut staged, view, measure)? {
            Ok(mut dimension) => {
                dimension.from_model = Some(candidate.model);
                let dimension = Arc::new(dimension);
                staged.add_dimensions([dimension.clone()]);
                added.push(Added { dimension: ids::encode(dimension.id), from: candidate.from });
                made.push(dimension);
            }
            Err((reason, why)) => skipped.push(skip(reason, why)),
        }
    }
    if !made.is_empty() {
        s.edit_drawing(FROM_MODEL, |d| {
            d.add_dimensions(made);
            Ok(())
        })?;
    }
    let result = FromModelResult { added, skipped, param_warnings: Vec::new() };
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}

/// `measure` placed on `view` as `drawing.dimension.add` places a dimension, when its view draws
/// and measures it and it reads its model dimension's value; else why it is skipped.
fn place(s: &mut Session, staged: &mut Drawing, view: u64, measure: Measure) -> Result<std::result::Result<Dimension, (SkipReason, String)>> {
    let new = NewDimension { view, kind: measure.kind, offset_mm: None, text: None, decimals: None, override_text: None, choose_diameter: false };
    let (dimension, read) = match placed(s, staged, FROM_MODEL, new)? {
        Ok(placed) => placed,
        Err(Refused::NotDrawn(e)) => return Ok(Err((SkipReason::NotDrawn, e.to_string()))),
        Err(Refused::Unmeasurable(e)) => return Ok(Err((SkipReason::Unmeasurable, e.to_string()))),
    };
    let Some(read) = read.filter(|r| r.status == Status::Ok) else {
        return Ok(Err((SkipReason::NotFound, "what it measures isn't found as one thing on the model".into())));
    };
    let value = read.measured.map_or(f64::NAN, |m| m.value);
    if (value - measure.expected).abs() > AGREES * measure.expected.abs().max(1.0) {
        let shown = |v: f64| if measure.angle { format!("{:.2}°", v.to_degrees()) } else { format!("{v:.2} mm") };
        let why = format!("what it found reads {}, the model dimension is {}", shown(value), shown(measure.expected));
        return Ok(Err((SkipReason::ValueDiffers, why)));
    }
    Ok(Ok(dimension))
}

/// The model dimensions `request` asks for, in timeline order, then each sketch's own order, each
/// with how it becomes a drawing dimension on a view looking along `basis`, or why it doesn't.
fn candidates(s: &Session, request: &FromModelParams, basis: &Plane) -> Result<Vec<Candidate>> {
    let find = |named: &Option<String>| -> Result<Option<&Feature>> {
        let Some(named) = named else { return Ok(None) };
        s.doc.find_feature(named).map(Some).ok_or_else(|| bad(FROM_MODEL, format!("the design has no feature `{}`", short(named))))
    };
    let sketch = find(&request.sketch)?;
    if let Some(f) = sketch
        && !matches!(f.kind, FeatureKind::Sketch { .. })
    {
        return Err(bad(FROM_MODEL, format!("`{}` is not a sketch", short(&f.name))));
    }
    let feature = find(&request.feature)?;
    let state = s.model.state();
    let made = match &request.body {
        Some(name) => {
            let body = state.body(name).ok_or_else(|| bad(FROM_MODEL, format!("the design has no body `{}`", short(name))))?;
            let faces = naming::face_names(body);
            Some(faces.iter().filter_map(|name| made_by(name)).chain([body.feature]).collect::<HashSet<u64>>())
        }
        None => None,
    };
    // A sketch is wanted through a feature that uses it, so a feature's filter and a body's take
    // the sketches of the features they keep.
    let users = |sketch: u64| s.doc.features.iter().filter(move |g| g.kind.input_sketches().contains(&sketch));
    let mut out = Vec::new();
    for f in &s.doc.features {
        if let FeatureKind::Sketch { sketch: drawn, .. } = &f.kind {
            let wanted = sketch.is_none_or(|x| x.id == f.id)
                && feature.is_none_or(|x| x.id == f.id || x.kind.input_sketches().contains(&f.id))
                && made.as_ref().is_none_or(|m| users(f.id).any(|g| m.contains(&g.id)));
            if !wanted {
                continue;
            }
            let solved = state.sketch(f.id);
            let constraints = solved.map_or(&drawn.constraints, |ss| &ss.sketch.constraints);
            for c in constraints.iter().filter(|c| c.kind.is_dimension()) {
                let from = ModelSource { kind: SourceKind::Sketch, sketch: Some(f.id), constraint: Some(c.id.clone()), feature: None, param: None };
                let plan = match solved {
                    _ if f.component != 0 => Err(in_component()),
                    Some(ss) => sketch_measure(s, ss, c, basis),
                    None => Err((SkipReason::NotFound, "the sketch isn't in the model now: it is suppressed, rolled back or failed".into())),
                };
                out.push(Candidate { from, model: ModelDimension::Sketch { sketch: f.id, constraint: c.id.clone() }, plan });
            }
        } else {
            let wanted = sketch.is_none() && feature.is_none_or(|x| x.id == f.id) && made.as_ref().is_none_or(|m| m.contains(&f.id));
            if !wanted {
                continue;
            }
            let lengths = f
                .kind
                .inputs()
                .into_iter()
                .filter(|(_, _, kind)| matches!(kind, solidcraft_doc::expr::Kind::Length | solidcraft_doc::expr::Kind::Angle));
            for (label, _, _) in lengths {
                let from = ModelSource { kind: SourceKind::Feature, sketch: None, constraint: None, feature: Some(f.id), param: Some(label.into()) };
                let plan = match &f.kind {
                    _ if f.component != 0 => Err(in_component()),
                    FeatureKind::Extrude { .. } if label == EXTRUDE_DISTANCE => extrude_distance(s, &state, f),
                    _ => Err((SkipReason::NotSupported, format!("not supported yet: a feature's {label}"))),
                };
                out.push(Candidate { from, model: ModelDimension::Feature { feature: f.id, param: label.into() }, plan });
            }
        }
    }
    Ok(out)
}

fn in_component() -> (SkipReason, String) {
    (SkipReason::InComponent, "not supported yet: a sketch or feature of a component placed through an occurrence".into())
}

/// At most 64 characters of `text`, for a message that quotes what the caller sent.
fn short(text: &str) -> &str {
    text.char_indices().nth(64).map_or(text, |(at, _)| text.get(..at).unwrap_or(text))
}

/// How sketch dimension `c` of `ss` becomes a drawing dimension on a view looking along `basis`:
/// its points lifted onto the sketch's plane and picked on the model.
fn sketch_measure(s: &Session, ss: &SolvedSketch, c: &Constraint, basis: &Plane) -> std::result::Result<Measure, (SkipReason, String)> {
    let not_supported = || (SkipReason::NotSupported, "not supported yet: this kind of sketch dimension".to_string());
    if !matches!(
        c.kind,
        ConstraintKind::Length { .. }
            | ConstraintKind::Distance { .. }
            | ConstraintKind::DistanceX { .. }
            | ConstraintKind::DistanceY { .. }
            | ConstraintKind::Radius { .. }
            | ConstraintKind::Diameter { .. }
            | ConstraintKind::Angle { .. }
    ) {
        return Err(not_supported());
    }
    if c.driven {
        return Err((SkipReason::Driven, "a driven sketch dimension measures the sketch; it doesn't define the part".into()));
    }
    if ss.plane.normal().dot(basis.normal()).abs() < 1.0 - FACE_ON {
        return Err((SkipReason::NotFaceOn, "the view doesn't look along the sketch's normal".into()));
    }
    let sketch = &ss.sketch;
    let lost = |what: String| (SkipReason::NotFound, what);
    let point = |i: usize| sketch.points.get(i).map(|p| p.pos).ok_or_else(|| lost(format!("the sketch has no point {i}")));
    let line = |i: usize| match sketch.curves.get(i).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Ok((point(*a)?, point(*b)?)),
        _ => Err(lost(format!("the sketch's curve {i} is not a line"))),
    };
    let (doc, model) = (&s.doc, &s.model);
    let world = |p: Vec2| ss.plane.to_world(p);
    let edge = |p: Vec2| doc.edge_reference(model, &EdgePick::Point(world(p)), &[]).map_err(|e| lost(e.to_string()));
    let corner = |p: Vec2| doc.corner_reference(model, &CornerPick::Point(world(p)), &[]).map_err(|e| lost(e.to_string()));
    let middle = |(a, b): (Vec2, Vec2)| (a + b) * 0.5;
    let length = |kind, expected: f64| Ok(Measure { kind, expected: expected.abs(), angle: false });
    match &c.kind {
        ConstraintKind::Length { l, value } => {
            length(DimensionKind::Length { edge: edge(middle(line(*l)?))?, direction: LinearDirection::Aligned, reading: Reading::Projected }, *value)
        }
        ConstraintKind::Distance { p, q, value } => {
            let (a, b) = (corner(point(*p)?)?, corner(point(*q)?)?);
            length(DimensionKind::Linear { a, b, direction: LinearDirection::Aligned, reading: Reading::Projected }, *value)
        }
        ConstraintKind::DistanceX { p, q, value } | ConstraintKind::DistanceY { p, q, value } => {
            let axis = if matches!(c.kind, ConstraintKind::DistanceX { .. }) { ss.plane.x } else { ss.plane.y };
            let (a, b) = (corner(point(*p)?)?, corner(point(*q)?)?);
            length(DimensionKind::Linear { a, b, direction: along(axis, basis), reading: Reading::Projected }, *value)
        }
        ConstraintKind::Radius { c: curve, value } | ConstraintKind::Diameter { c: curve, value } => {
            let on = match sketch.curves.get(*curve).map(|c| &c.kind) {
                Some(CurveKind::Circle { c: centre, r }) => point(*centre)? + Vec2::new(ON_CIRCLE.cos(), ON_CIRCLE.sin()) * *r,
                Some(CurveKind::Arc { c: centre, a, b }) => arc_middle(point(*centre)?, point(*a)?, point(*b)?),
                Some(CurveKind::Ellipse { .. }) => return Err(not_supported()),
                _ => return Err(lost(format!("the sketch's curve {curve} is not a circle or an arc"))),
            };
            let diameter = matches!(c.kind, ConstraintKind::Diameter { .. });
            length(DimensionKind::Radial { edge: edge(on)?, diameter }, *value)
        }
        ConstraintKind::Angle { a, b, value, .. } => {
            // The lines' angle, its size as the sketch shows it, which a flip (half a turn more)
            // leaves the same; the drawing reads the smaller angle between the lines, or with
            // other_side π minus it.
            let between = value.abs().rem_euclid(PI);
            let other_side = between > FRAC_PI_2;
            let (a, b) = (edge(middle(line(*a)?))?, edge(middle(line(*b)?))?);
            Ok(Measure { kind: DimensionKind::Angle { a, b, reading: Reading::Projected, other_side }, expected: between, angle: true })
        }
        _ => Err(not_supported()),
    }
}

/// The paper direction of sketch axis `axis` on a view looking along `basis`: the view's
/// horizontal or vertical when it lies along one, else that direction.
fn along(axis: Vec3, basis: &Plane) -> LinearDirection {
    let d = Vec2::new(axis.dot(basis.x), axis.dot(basis.y));
    if d.y.abs() <= FACE_ON {
        LinearDirection::Horizontal
    } else if d.x.abs() <= FACE_ON {
        LinearDirection::Vertical
    } else {
        LinearDirection::Along(d)
    }
}

/// The middle of the counter-clockwise arc round `centre` from `a` to `b`.
fn arc_middle(centre: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let (ra, rb) = (a - centre, b - centre);
    let (start, end) = (ra.y.atan2(ra.x), rb.y.atan2(rb.x));
    let middle = start + (end - start).rem_euclid(2.0 * PI) / 2.0;
    centre + Vec2::new(middle.cos(), middle.sin()) * ra.len()
}

/// Extrude `f`'s distance: between its two caps, the faces it names by where they lie (`start` on
/// the sketch's plane, `top` or `bottom` off it, `end`), each one whole face of one body of the
/// design. Whether that reads its distance is checked as every model dimension's value is.
fn extrude_distance(s: &Session, state: &ModelState, f: &Feature) -> std::result::Result<Measure, (SkipReason, String)> {
    let expected = s.doc.length_input(f.id, EXTRUDE_DISTANCE).ok_or((SkipReason::NotFound, "its distance doesn't evaluate".to_string()))?;
    let caps = state.bodies.iter().find_map(|b| match naming::face_names(b).iter().filter(|name| is_cap(name, f.id)).collect::<Vec<_>>()[..] {
        [a, z] if a != z => Some((b, [a.clone(), z.clone()])),
        _ => None,
    });
    let (body, names) = caps.ok_or((SkipReason::NotFound, "its two caps aren't each one face of one body".to_string()))?;
    let face = |name: &String| ModelFace { body: body.name.clone(), origin_feature: body.feature, name: name.clone(), path: Vec::new() };
    let [a, b] = &names;
    Ok(Measure { kind: DimensionKind::Faces { a: face(a), b: face(b), direction: LinearDirection::Aligned }, expected, angle: false })
}

#[cfg(test)]
#[path = "drawing_model_dimensions_tests.rs"]
mod tests;
