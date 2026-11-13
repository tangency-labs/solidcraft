//! `drawing.dimension.add`, `.edit`, `.repair` and `.inspect` (W19d, #4896; B5, #4892): a dimension
//! on a saved view of the open drawing, its settings, a new name for a reference it lost, and what
//! it reads now. What a dimension measures is picked (a point, or an edge's index) and saved by
//! name, never by the point picked ([`solidcraft_doc::Document::edge_reference`]); its value comes
//! from the model each time it is read ([`solidcraft_doc::Document::resolve_dimensions`]). Adding,
//! editing and repairing are one undo step of the drawing each, and none of the commands evaluates
//! the model.

use std::sync::Arc;

use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::dimension::{
    Candidate, Dimension, DimensionKind, ModelEdge, ModelFace, ModelPoint, Reference, Resolved, Silhouette, Status, TextPlace, check_chain_length,
};
use solidcraft_doc::ids;
use solidcraft_geom::drawing_measure::{LinearDirection, Reading};
use solidcraft_geom::prelude::*;

use solidcraft_geom::paper::Scene;
use solidcraft_sketch::PaperFonts;

use super::CommandSpec;
use super::drawing_view::{check_drawn, drawn_warning};
use crate::drawing_dimension_paper::{LineRole, paper_group};
use crate::drawing_scene::{NO_PLACE, NOT_DRAWN, designed, moved};
use crate::params::bad;
use crate::schema::drawing_dimension::{
    AddParams, ArrowResult, CandidateResult, CornerParam, DimensionLayoutResult, DimensionResult, DimensionSet, EdgeParam, EditParams, EditResult,
    FaceParam, InspectParams, KindParam, MemberResult, PickParam, RepairParams, SilhouetteParam, SlotParam,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.dimension.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EDIT: &str = "drawing.dimension.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const INSPECT: &str = "drawing.dimension.inspect";
const INSPECT_CONTRACT: TypedContract<InspectParams> = TypedContract::new();
const REPAIR: &str = "drawing.dimension.repair";
const REPAIR_CONTRACT: TypedContract<RepairParams> = TypedContract::new();

/// How far a new dimension's line is from what it measures, on paper, until a dimension style
/// gives it (D1). A new linear, length or chain dimension takes its side ([`side_by_default`]).
const DEFAULT_OFFSET_MM: f64 = 10.0;
/// How far beside a line's middle, on paper, [`side_by_default`] looks for material.
const LOOK_PAST_MM: f64 = 0.5;
/// How near the middle of a view's model box, in model mm, a line's middle is on it.
const ON_CENTRE_MM: f64 = 1e-6;
/// How many decimals a new dimension shows unless told.
const DEFAULT_DECIMALS: u8 = 2;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Drawing Dimension", crate::checked!("drawing.dimension.add", add))
        .icon("dimension")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<DimensionResult>)
        .params(
            "view: a saved view's id; kind: {linear: {a, b: corners, direction?: aligned (default), horizontal, vertical or {along: [x, y]}, \
             reading?: projected (default) or true}}, {length: {edge, direction?, reading?}}, {angle: {a, b: straight edges, reading?, \
             other_side?}}, {radial: {edge: a circle or an arc the view sees face on, diameter?: by default for a whole circle or \
             half a turn or more}}, {cylinder: {face: a face of a whole cylinder the view doesn't see end-on, read as Ø between its \
             silhouettes}}, {sphere: {face: a face of a sphere, read as SØ when the body's faces on it are the whole sphere, else as SR \
             on the arc of its outline in the view that is on them}}, {cone: {face: a face of a cone whose two silhouettes in the view \
             are on it, read as its included angle, foreshortened unless the view's plane holds its axis}}, {silhouettes: {a, b: {face, side: near or far, the silhouette nearer the other cylinder or farther}, \
             direction?: as a linear one's, aligned being square to them}: two of one cylinder make its cylinder dimension}, \
             {faces: {a, b: flat, parallel faces the view sees edge-on, direction?: as a linear one's, aligned being square to them}} or \
             {chain: {points: [3 to 64 corners, in order along one line], direction?: as a linear one's, aligned being first to \
             last}}; an edge is a point on it [x, y, z] in the design's frame, {point} or {body, index}, and a \
             corner a point, {point}, {edges: [2 to 16 edges]} or {centre: an edge that is a circle or an arc}, and a face a point on \
             it, {point} or {body, index}, each with path?: the \
             occurrences its body is placed through, from the top; offset_mm?: the dimension line's distance on paper (default 10;              a linear, length or chain one's on the side away from what it measures); \
             text?: centred (default) or {at: [x, y]} from the view's centre; decimals?: 0..6 (default 2); override_text? (not a \
             chain's); dry_run?: true to answer what it would add, adding nothing (no id is taken, no undo step) → what \
             drawing.dimension.inspect answers for it. Saved by the edges' names, never the points picked; one the \
             view can't measure, or on a part it doesn't draw, is refused. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Drawing Dimension", crate::checked!("drawing.dimension.edit", edit))
        .icon("dimension")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "dimension: its id; set: {offset_mm?, text?: centred or {at: [x, y]}, decimals?: 0..6, override_text?: text, or null to show \
             the number again; direction?, reading?: a linear or length dimension's, reading? and other_side?: an angle's, diameter?: a \
             radial dimension's, direction?: a chain's or a silhouettes one's} → \
             {dimension, changed: [fields]}. What it measures stays: to measure something else, add a new one. One undo step, none \
             when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(REPAIR, "Repair Drawing Dimension", crate::checked!("drawing.dimension.repair", repair))
        .icon("dimension")
        .in_drawing()
        .doc_only()
        .contract(&REPAIR_CONTRACT)
        .output(output_schema_for::<DimensionResult>)
        .params(
            "dimension: its id, one that reads lost or ambiguous; slot: which of its references, a or b (a linear dimension's corners, \
             an angle's edges, the faces of silhouettes or of faces), edge (a length's or a radial one's), face (a cylinder's, a sphere's or a cone's) or a \
             chain's point by its number from 0, one that no longer finds the one thing it names; pick: what that reference names \
             now, {edge}, {corner} or {face}, each as add takes it but without path (it keeps its own); a candidate inspect lists \
             of the slot's kind passes back as it stands → what drawing.dimension.inspect answers for it. Saved by name; its id, \
             view, place and settings stay, and one the view can't measure is refused. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(INSPECT, "Inspect Drawing Dimension", crate::checked!("drawing.dimension.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&INSPECT_CONTRACT)
        .output(output_schema_for::<DimensionResult>)
        .params(
            "dimension: its id → {dimension, view, record: as saved, status: ok, overridden, ambiguous, suppressed, failed, lost or unmeasurable, \
             why?, value? (model mm, or degrees for an angle; a chain's overall), foreshortened?, text? (none for a chain), \
             members?: a chain's [{value, text}] from its first point, last_good? (the value it last read, kept when the design \
             is saved, when it has none now), exact, candidates?: [{edge: {body, index}}, {corner: [x, y, z]} or {face: {body, index}}]: \
             what an ambiguous name finds, or a lost one's leads; one of a slot's kind passes back to drawing.dimension.repair as it \
             stands, warnings?: its view edited since to leave out a part it measures, layout?: where it sits on its view's sheet \
             now, in paper mm from the sheet's lower-left corner, its lines before an arrowhead stops one: {extension_lines, \
             dimension_lines: [[[x, y], ..]], arrows: [{tip, points: the unit direction}], text_boxes: [[4 corners]]: start and end \
             on the line's side, then end and start on the far side} (none when it isn't drawn; one the sheet leaves out for its \
             limits, or on a view whose lines fail, still has one, and drawing.scene's drawn says), layout_why?: why it has no layout, or no box for its text, when \
             status doesn't say}. Read-only; no model is evaluated.",
        ),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    if request.dry_run == Some(true) {
        // As selection.delete and sketch.trim.path: the add itself, on a copy of the session, so the
        // answer is the add's and nothing it takes (an id, an undo step, a last good value) stays.
        let mut reply = added(&mut s.scratch(), request)?;
        reply["dry_run"] = json!(true);
        return Ok(reply);
    }
    added(s, request)
}

/// The dimension `request` asks for, added to the open drawing of `s`, and what inspect answers for it.
fn added(s: &mut Session, request: AddParams) -> Result<Value> {
    let mut record = Drawing::clone(s.drawing(ADD)?);
    let view = request.view.0;
    record.view(view).ok_or_else(|| bad(ADD, format!("the drawing has no view {}", ids::encode(view))))?;
    let (doc, model) = (&s.doc, &s.model);
    let edge = |e: &EdgeParam| doc.edge_reference(model, &e.pick, &e.path).map_err(|why| bad(ADD, why.to_string()));
    let corner = |c: &CornerParam| doc.corner_reference(model, &c.pick, &c.path).map_err(|why| bad(ADD, why.to_string()));
    let face = |f: &FaceParam| doc.face_reference(model, &f.pick, &f.path).map_err(|why| bad(ADD, why.to_string()));
    let choose_diameter = matches!(request.kind, KindParam::Radial { diameter: None, .. });
    let kind = match request.kind {
        KindParam::Linear { a, b, direction, reading } => DimensionKind::Linear {
            a: corner(&a)?,
            b: corner(&b)?,
            direction: direction.unwrap_or(LinearDirection::Aligned),
            reading: reading.unwrap_or(Reading::Projected),
        },
        KindParam::Length { edge: e, direction, reading } => DimensionKind::Length {
            edge: edge(&e)?,
            direction: direction.unwrap_or(LinearDirection::Aligned),
            reading: reading.unwrap_or(Reading::Projected),
        },
        KindParam::Angle { a, b, reading, other_side } => {
            DimensionKind::Angle { a: edge(&a)?, b: edge(&b)?, reading: reading.unwrap_or(Reading::Projected), other_side }
        }
        KindParam::Radial { edge: e, diameter } => DimensionKind::Radial { edge: edge(&e)?, diameter: diameter.unwrap_or(false) },
        KindParam::Cylinder { face: f } => DimensionKind::Cylinder { face: face(&f)? },
        KindParam::Sphere { face: f } => DimensionKind::Sphere { face: face(&f)? },
        KindParam::Cone { face: f } => DimensionKind::Cone { face: face(&f)? },
        KindParam::Silhouettes { a, b, direction } => {
            let silhouette = |s: &SilhouetteParam| face(&s.face).map(|face| Silhouette { face, side: s.side });
            doc.silhouettes_kind(model, silhouette(&a)?, silhouette(&b)?, direction).map_err(|why| bad(ADD, why.to_string()))?
        }
        KindParam::Faces { a, b, direction } => {
            DimensionKind::Faces { a: face(&a)?, b: face(&b)?, direction: direction.unwrap_or(LinearDirection::Aligned) }
        }
        KindParam::Chain { points, direction } => {
            // Counted before any point is looked up.
            check_chain_length(points.len()).map_err(|why| bad(ADD, why.to_string()))?;
            let points = points.iter().map(corner).collect::<Result<_>>()?;
            DimensionKind::Chain { points, direction: direction.unwrap_or(LinearDirection::Aligned) }
        }
    };
    let new = NewDimension {
        view,
        kind,
        offset_mm: request.offset_mm,
        text: request.text,
        decimals: request.decimals,
        override_text: request.override_text,
        choose_diameter,
    };
    let (dimension, _) = placed(s, &mut record, ADD, new)?.map_err(Refused::error)?;
    let id = dimension.id;
    s.edit_drawing(ADD, |d| {
        d.add_dimensions([Arc::new(dimension)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

/// A new dimension before it is placed: its view, what it measures, and what the request gave of
/// the rest.
pub(crate) struct NewDimension {
    pub view: u64,
    pub kind: DimensionKind,
    pub offset_mm: Option<f64>,
    pub text: Option<TextPlace>,
    pub decimals: Option<u8>,
    pub override_text: Option<String>,
    /// A radial dimension whose diameter is left to the default.
    pub choose_diameter: bool,
}

/// Why [`placed`] won't place a new dimension, as the command would say it.
pub(crate) enum Refused {
    /// Its view doesn't draw what it measures.
    NotDrawn(EngineError),
    /// Its view can't measure it.
    Unmeasurable(EngineError),
}

impl Refused {
    pub(crate) fn error(self) -> EngineError {
        match self {
            Refused::NotDrawn(e) | Refused::Unmeasurable(e) => e,
        }
    }
}

/// `new` as `drawing.dimension.add` places it on `record`, with how it reads, and nothing added: an
/// error when its view draws its parts apart ([`crate::drawing_view::refuse_apart`]); refused
/// when its view doesn't draw what it measures or can't measure it; a new id; a radial one
/// a diameter when it is a whole circle, if left to the default; a linear, length or chain one,
/// with no offset given, on the side away from what it measures. It is read on `record` itself,
/// which is left as it was. One rule for every command that adds a dimension.
pub(crate) fn placed(
    s: &mut Session,
    record: &mut Drawing,
    cmd: &str,
    new: NewDimension,
) -> Result<std::result::Result<(Dimension, Option<Resolved>), Refused>> {
    crate::drawing_view::refuse_apart(record, new.view, "dimensions", cmd)?;
    if let Err(e) = check_drawn(record, new.view, new.kind.paths(), cmd) {
        return Ok(Err(Refused::NotDrawn(e)));
    }
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(cmd, e.to_string()))?;
    let mut dimension = Dimension {
        id,
        view: new.view,
        kind: new.kind,
        offset_mm: new.offset_mm.unwrap_or(DEFAULT_OFFSET_MM),
        text: new.text.unwrap_or(TextPlace::Centred),
        decimals: new.decimals.unwrap_or(DEFAULT_DECIMALS),
        override_text: new.override_text,
        last_good: None,
        from_model: None,
    };
    // Read before it is added: a dimension its view can't measure would show nothing. Its id is the
    // newest, so taking it out again leaves the record as it was.
    record.add_dimensions([Arc::new(dimension.clone())]);
    let read = s.doc.resolve_dimensions(&s.model, record, &[id]).map_err(|e| bad(cmd, e.to_string()));
    record.dimensions.retain(|d| d.id != id);
    let read = read?;
    if let Some(Status::Unmeasurable(why)) = read.first().map(|r| &r.status) {
        return Ok(Err(Refused::Unmeasurable(bad(cmd, why.clone()))));
    }
    if new.choose_diameter
        && let DimensionKind::Radial { diameter, .. } = &mut dimension.kind
    {
        *diameter = read.first().is_some_and(|r| diameter_by_default(&r.points));
    }
    if new.offset_mm.is_none()
        && let DimensionKind::Linear { direction, .. } | DimensionKind::Length { direction, .. } | DimensionKind::Chain { direction, .. } =
            &dimension.kind
        && let Some(r) = read.first()
    {
        dimension.offset_mm = DEFAULT_OFFSET_MM * side_by_default(s, record, new.view, *direction, &r.points);
    }
    Ok(Ok((dimension, read.into_iter().next())))
}

fn repair(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RepairParams = crate::schema::read(REPAIR, p)?;
    let id = request.dimension.0;
    let unknown = || bad(REPAIR, format!("the drawing has no dimension {}", ids::encode(id)));
    let d = s.drawing(REPAIR)?;
    d.dimension(id).ok_or_else(unknown)?;
    let read = s.doc.resolve_dimensions(&s.model, d, &[id]).map_err(|e| bad(REPAIR, e.to_string()))?;
    let now = read.into_iter().next().map(|r| r.status).ok_or_else(unknown)?;
    repair_reference(s, REPAIR, now, |s, copy| {
        let record = copy.dimension_index(id).and_then(|at| copy.dimensions.get_mut(at)).map(Arc::make_mut).ok_or_else(unknown)?;
        slot(&mut record.kind, request.slot)?.take(s, REPAIR, &request.pick)?;
        // As add has it: two silhouettes of one cylinder are its diameter, a cylinder dimension.
        if let DimensionKind::Silhouettes { a, b, .. } = &record.kind
            && let Ok(DimensionKind::Cylinder { .. }) = s.doc.silhouettes_kind(&s.model, a.clone(), b.clone(), None)
        {
            return Err(bad(REPAIR, "both silhouettes would be on one cylinder: its diameter is a dimension of its own (add one)"));
        }
        // Read before it is kept, as add does: a reference its view can't measure would show nothing.
        let read = s.doc.resolve_dimensions(&s.model, copy, &[id]).map_err(|e| bad(REPAIR, e.to_string()))?;
        match read.first().map(|r| &r.status) {
            Some(Status::Unmeasurable(why)) => Err(bad(REPAIR, why.clone())),
            _ => Ok(()),
        }
    })?;
    answer(s, REPAIR, id)
}

/// One reference of a record on a copy of the open drawing, for a repair to put a new name into.
pub(super) enum Slot<'a> {
    Edge(&'a mut ModelEdge),
    Point(&'a mut ModelPoint),
    Face(&'a mut ModelFace),
}

impl Slot<'_> {
    /// `pick` named as `add` names one, on the reference's own occurrence path, and put in its
    /// place. A reference that still finds the one thing it names is kept, and a pick of another
    /// kind is refused, naming the kind it takes.
    pub(super) fn take(self, s: &Session, cmd: &str, pick: &PickParam) -> Result<()> {
        let (doc, model) = (&s.doc, &s.model);
        let reference = match &self {
            Slot::Edge(edge) => Reference::Edge(edge),
            Slot::Point(point) => Reference::Point(point),
            Slot::Face(face) => Reference::Face(face),
        };
        if doc.finds_one(model, reference) {
            return Err(bad(cmd, "this reference still finds what it names: only a lost or ambiguous one is repaired"));
        }
        let named = |why: solidcraft_doc::DocError| bad(cmd, why.to_string());
        match (self, pick) {
            (Slot::Edge(edge), PickParam::Edge(p)) => *edge = doc.edge_reference(model, p, &edge.path).map_err(named)?,
            (Slot::Point(point), PickParam::Corner(p)) => *point = doc.corner_reference(model, p, &point.path).map_err(named)?,
            (Slot::Face(face), PickParam::Face(p)) => *face = doc.face_reference(model, p, &face.path).map_err(named)?,
            (Slot::Edge(_), _) => return Err(bad(cmd, "this reference is an edge: give {edge: …}")),
            (Slot::Point(_), _) => return Err(bad(cmd, "this reference is a corner: give {corner: …}")),
            (Slot::Face(_), _) => return Err(bad(cmd, "this reference is a face: give {face: …}")),
        }
        Ok(())
    }
}

/// Repairs a reference of the open drawing as `cmd`, the user's own pick for one whose record reads
/// `now` lost or ambiguous: one that reads is changed by an edit (or a new one), and a suppressed or
/// failed one comes back by itself. `put` finds the reference on a copy of the drawing, puts the
/// pick in ([`Slot::take`]) and may read the copy; the copy is kept as one undo step. The record's
/// id, view, place and settings stay. Balloons are repaired through it too.
pub(super) fn repair_reference(s: &mut Session, cmd: &str, now: Status, put: impl FnOnce(&Session, &mut Drawing) -> Result<()>) -> Result<()> {
    if !matches!(now, Status::Lost(..) | Status::Ambiguous(_)) {
        let word = status_words(now).0;
        return Err(bad(cmd, format!("it reads {word}: only a lost or ambiguous reference is repaired (to point at something else, add a new one)")));
    }
    let mut copy = Drawing::clone(s.drawing(cmd)?);
    put(&*s, &mut copy)?;
    s.edit_drawing(cmd, |d| {
        *d = copy;
        Ok(())
    })
}

/// The reference of `kind` that `slot` names: a linear dimension's corners `a` and `b`, a length's
/// or a radial one's `edge`, an angle's edges `a` and `b`, a chain's points by number, a cylinder's,
/// a sphere's or a cone's `face`, and the faces `a` and `b` of silhouettes (each keeps its side) or of faces.
fn slot(kind: &mut DimensionKind, slot: SlotParam) -> Result<Slot<'_>> {
    let fits = slots(kind);
    let found = match kind {
        DimensionKind::Linear { a, b, .. } => a_or_b(slot, a, b).map(Slot::Point),
        DimensionKind::Length { edge, .. } | DimensionKind::Radial { edge, .. } => (slot == SlotParam::Edge).then_some(Slot::Edge(edge)),
        DimensionKind::Angle { a, b, .. } => a_or_b(slot, a, b).map(Slot::Edge),
        DimensionKind::Chain { points, .. } => match slot {
            SlotParam::Point(k) => points.get_mut(k).map(Slot::Point),
            _ => None,
        },
        DimensionKind::Cylinder { face } | DimensionKind::Sphere { face } | DimensionKind::Cone { face } => {
            (slot == SlotParam::Face).then_some(Slot::Face(face))
        }
        DimensionKind::Silhouettes { a, b, .. } => a_or_b(slot, &mut a.face, &mut b.face).map(Slot::Face),
        DimensionKind::Faces { a, b, .. } => a_or_b(slot, a, b).map(Slot::Face),
    };
    found.ok_or_else(|| bad(REPAIR, format!("this dimension's slots are {fits}")))
}

/// `a` for slot `a`, `b` for slot `b`, and nothing for any other.
fn a_or_b<'a, T>(slot: SlotParam, a: &'a mut T, b: &'a mut T) -> Option<&'a mut T> {
    match slot {
        SlotParam::A => Some(a),
        SlotParam::B => Some(b),
        _ => None,
    }
}

/// The slots a dimension of `kind` has, for a message.
fn slots(kind: &DimensionKind) -> String {
    match kind {
        DimensionKind::Linear { .. } => "`a` and `b`, its corners".into(),
        DimensionKind::Length { .. } | DimensionKind::Radial { .. } => "`edge`".into(),
        DimensionKind::Angle { .. } => "`a` and `b`, its edges".into(),
        DimensionKind::Chain { points, .. } => format!("its points, 0 to {}", points.len().saturating_sub(1)),
        DimensionKind::Cylinder { .. } | DimensionKind::Sphere { .. } | DimensionKind::Cone { .. } => "`face`".into(),
        DimensionKind::Silhouettes { .. } | DimensionKind::Faces { .. } => "`a` and `b`, its faces".into(),
    }
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.dimension.0;
    let changed = s.edit_drawing(EDIT, |d| change(d, id, &request.set))?;
    let reply = EditResult { dimension: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// A new radial dimension's default from its arc as read (its centre, start and middle): a diameter
/// for a whole circle or an arc of half a turn or more, as a hole's rim is (the kernel makes it of
/// two halves), else a radius.
fn diameter_by_default(points: &[Vec3]) -> bool {
    let [centre, start, middle, ..] = points else { return false };
    let (from, to) = (*start - *centre, *middle - *centre);
    // The middle is half the arc's turn from its start: a quarter turn or more away.
    from.dot(to) <= from.dot(from) * 1e-9
}

/// A new linear, length or chain dimension's side, as its offset's sign, from its points as read
/// (a chain's first and last): away from the material beside its line's middle, so it clears what
/// it measures. Beside means half a paper millimetre either way across the line in the view's
/// plane, at the depth of what it measures, asked of each shown body's B-rep exactly (inside or on
/// its boundary is material). With material on both sides or neither, away from the middle of the
/// view's model box; on that middle (a centre line, a symmetric pair) the fixed rule is +1, above
/// or to the left ([`crate::drawing_dimension_paper::line_direction`]).
fn side_by_default(s: &Session, drawing: &Drawing, view: u64, direction: LinearDirection, points: &[Vec3]) -> f64 {
    let (Ok(resolved), Ok(shown)) = (drawing.resolve_view(view), crate::drawing_view::shown(s)) else { return 1.0 };
    let basis = resolved.basis;
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return 1.0 };
    let (a, b) = (basis.to_local(*first), basis.to_local(*last));
    // The plane's directions are paper's: a view's paper map only scales and shifts.
    let Some(n) = crate::drawing_dimension_paper::line_direction(direction, b - a).map(Vec2::perp) else { return 1.0 };
    let middle = (a + b) * 0.5;
    let beside = (basis.x * n.x + basis.y * n.y) * (LOOK_PAST_MM / resolved.scale.value.factor());
    let at = (*first + *last) * 0.5;
    let material = |p: Vec3| {
        let class = |body: &solidcraft_doc::ModelBody| body.body.classify_point(p).is_ok_and(|c| c != solidcraft_kernel::PointClass::Outside);
        shown.bodies.iter().filter_map(|b| shown.body(b)).any(class)
    };
    match (material(at + beside), material(at - beside)) {
        (true, false) => -1.0,
        (false, true) => 1.0,
        _ => {
            let from_centre = crate::drawing_view::measure(&shown, &basis).map_or(0.0, |e| (middle - e.model.center()).dot(n));
            if from_centre < -ON_CENTRE_MM { -1.0 } else { 1.0 }
        }
    }
}

/// `set` made on dimension `id` of `d`: the fields it changes, by name.
fn change(d: &mut Drawing, id: u64, set: &DimensionSet) -> Result<Vec<String>> {
    let at = d.dimension_index(id).ok_or_else(|| bad(EDIT, format!("the drawing has no dimension {}", ids::encode(id))))?;
    let Some(dimension) = d.dimensions.get_mut(at).map(Arc::make_mut) else { return Ok(Vec::new()) };
    let before = dimension.clone();
    if let Some(offset) = set.offset_mm {
        dimension.offset_mm = offset;
    }
    if let Some(text) = set.text {
        dimension.text = text;
    }
    if let Some(decimals) = set.decimals {
        dimension.decimals = decimals;
    }
    if let Some(text) = &set.override_text {
        dimension.override_text = text.clone();
    }
    if set.diameter.is_some() && !matches!(dimension.kind, DimensionKind::Radial { .. }) {
        return Err(bad(EDIT, "`diameter` is a radial dimension's"));
    }
    match &mut dimension.kind {
        DimensionKind::Linear { direction, reading, .. } | DimensionKind::Length { direction, reading, .. } => {
            if set.other_side.is_some() {
                return Err(bad(EDIT, "`other_side` is an angle's"));
            }
            *direction = set.direction.unwrap_or(*direction);
            *reading = set.reading.unwrap_or(*reading);
        }
        DimensionKind::Angle { reading, other_side, .. } => {
            if set.direction.is_some() {
                return Err(bad(EDIT, "an angle has no `direction`: it measures between its edges' lines"));
            }
            *reading = set.reading.unwrap_or(*reading);
            *other_side = set.other_side.unwrap_or(*other_side);
        }
        DimensionKind::Radial { diameter, .. } => {
            if set.direction.is_some() || set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a radial dimension has only `diameter`: it reads its circle as the view sees it face on"));
            }
            *diameter = set.diameter.unwrap_or(*diameter);
        }
        DimensionKind::Chain { direction, .. } => {
            if set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a chain has only `direction`: it reads as the view shows it"));
            }
            *direction = set.direction.unwrap_or(*direction);
        }
        DimensionKind::Silhouettes { direction, .. } => {
            if set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a dimension between silhouettes has only `direction`: it reads as the view shows them"));
            }
            *direction = set.direction.unwrap_or(*direction);
        }
        DimensionKind::Faces { direction, .. } => {
            if set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a dimension between faces has only `direction`: it reads as the view shows them"));
            }
            *direction = set.direction.unwrap_or(*direction);
        }
        DimensionKind::Cylinder { .. } => {
            if set.direction.is_some() || set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a cylinder's diameter has no direction, reading or side: it reads between its silhouettes"));
            }
        }
        DimensionKind::Sphere { .. } => {
            if set.direction.is_some() || set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a sphere's size has no direction, reading or side: it reads on its outline in the view"));
            }
        }
        DimensionKind::Cone { .. } => {
            if set.direction.is_some() || set.reading.is_some() || set.other_side.is_some() {
                return Err(bad(EDIT, "a cone's angle has no direction, reading or side: it reads between its silhouettes"));
            }
        }
    }
    Ok(changed_fields(&before, dimension))
}

/// The direction, reading, side and diameter of a dimension's kind, where it has them.
fn kind_settings(kind: &DimensionKind) -> (Option<LinearDirection>, Option<Reading>, bool, bool) {
    match kind {
        DimensionKind::Linear { direction, reading, .. } | DimensionKind::Length { direction, reading, .. } => {
            (Some(*direction), Some(*reading), false, false)
        }
        DimensionKind::Angle { reading, other_side, .. } => (None, Some(*reading), *other_side, false),
        DimensionKind::Radial { diameter, .. } => (None, None, false, *diameter),
        DimensionKind::Chain { direction, .. } | DimensionKind::Silhouettes { direction, .. } | DimensionKind::Faces { direction, .. } => {
            (Some(*direction), None, false, false)
        }
        DimensionKind::Cylinder { .. } | DimensionKind::Sphere { .. } | DimensionKind::Cone { .. } => (None, None, false, false),
    }
}

/// The names of the fields that differ between `a` and `b`, two states of one dimension.
fn changed_fields(a: &Dimension, b: &Dimension) -> Vec<String> {
    let (x, y) = (kind_settings(&a.kind), kind_settings(&b.kind));
    let fields = [
        ("offset_mm", a.offset_mm != b.offset_mm),
        ("text", a.text != b.text),
        ("decimals", a.decimals != b.decimals),
        ("override_text", a.override_text != b.override_text),
        ("direction", x.0 != y.0),
        ("reading", x.1 != y.1),
        ("other_side", x.2 != y.2),
        ("diameter", x.3 != y.3),
    ];
    fields.into_iter().filter(|(_, changed)| *changed).map(|(name, _)| name.to_string()).collect()
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: InspectParams = crate::schema::read(INSPECT, p)?;
    answer(s, INSPECT, request.dimension.0)
}

/// Dimension `id` of the open drawing as the model has it now. A value read is kept as its last
/// good one ([`Session::dimension_values`]); with none this session, the file's is its last good one.
fn answer(s: &mut Session, cmd: &str, id: u64) -> Result<Value> {
    let d = s.drawing(cmd)?.clone();
    let dimension = d.dimension(id).ok_or_else(|| bad(cmd, format!("the drawing has no dimension {}", ids::encode(id))))?;
    let resolved = s.doc.resolve_dimensions(&s.model, &d, &[id]).map_err(|e| bad(cmd, e.to_string()))?;
    let r = resolved.into_iter().next().ok_or_else(|| bad(cmd, format!("dimension {} wasn't read", ids::encode(id))))?;
    let last_good = match r.measured {
        Some(m) => {
            s.dimension_values.insert((d.id, id), m.value);
            None
        }
        None => s.dimension_values.get(&(d.id, id)).copied().or(dimension.last_good),
    };
    // Before `status_words` takes the status: the layout reads the whole reading.
    let (layout, layout_why) = layout_on_sheet(s, &d, dimension, &r);
    let (status, why, candidates) = status_words(r.status);
    let angle = dimension.kind.is_angle();
    let shown = |value: f64| if angle { value.to_degrees() } else { value };
    let reply = DimensionResult {
        dimension: ids::encode(id),
        view: ids::encode(dimension.view),
        record: serde_json::to_value(dimension).map_err(|e| EngineError::Other(e.to_string()))?,
        status: status.to_string(),
        why,
        value: r.measured.map(|m| shown(m.value)),
        foreshortened: r.measured.map(|m| m.foreshortened),
        text: r.text,
        members: r.members.into_iter().map(|m| MemberResult { value: m.measured.value, text: m.text }).collect(),
        last_good: last_good.map(shown),
        exact: r.exact,
        candidates,
        layout,
        layout_why,
        warnings: drawn_warning(&d, dimension.view, dimension.kind.paths()),
        dry_run: false,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Where `dimension` of `d`, read as `r`, sits on its view's sheet now, as the sheet draws it: laid out
/// through its view's frame ([`paper_group`]), then moved to the view's centre. Or none, with why when
/// its status doesn't say it, in the sheet's words: its view draws its parts apart, isn't drawn (a
/// style it can't draw yet, or nothing to measure), has no place for what it measures, or its group
/// alone fails the paper scene's checks. A text that can't be set has no box, and says why. It reads
/// the model as it is evaluated and draws no view's lines, so a view whose lines fail still answers.
fn layout_on_sheet(s: &Session, d: &Drawing, dimension: &Dimension, r: &Resolved) -> (Option<DimensionLayoutResult>, Option<String>) {
    let id = dimension.view;
    if let Some(apart) = crate::drawing_view::draws_apart(d, id) {
        return (None, Some(apart.held().into()));
    }
    let not_drawn = || (None, Some(NOT_DRAWN.to_string()));
    // The views draw from where the design places its parts, as the sheet does; a dimension reads the
    // same either way, as it reads the model, not the session's moves.
    let copy = (!s.pending_moves.is_empty()).then(|| designed(s));
    let s = copy.as_ref().unwrap_or(s);
    let Ok(view) = d.resolve_view(id) else { return not_drawn() };
    let unstyled = crate::drawing_view::not_drawable(view.visual_style.value, view.reference_style.value);
    if unstyled.or_else(|| crate::drawing_view::not_drawable_apart(&view)).is_some() {
        return not_drawn();
    }
    let on = match crate::drawing_view::on_sheet(s, d, id, INSPECT) {
        Ok(on) if on.unmeasured.is_none() => on,
        _ => return not_drawn(),
    };
    let size = d.views.iter().find(|v| v.id == id).and_then(|v| d.sheets.iter().find(|sheet| sheet.id == v.sheet)).map(|sheet| sheet.sheet.size_mm());
    let (Ok(frame), Ok(centre), Some(Ok(size))) =
        (crate::drawing_view::frame(view.basis, view.scale.value, &on.extents.model, Vec2::ZERO, INSPECT), d.view_centre(id), size)
    else {
        return not_drawn();
    };
    let Some(paper) = paper_group(dimension, r, &frame, view.line_group.value, &PaperFonts::new(&d.font, &d.language, true)) else {
        return (None, (!r.points.is_empty()).then(|| NO_PLACE.to_string()));
    };
    // As the sheet takes it: alone on its sheet, its group must pass the paper scene's checks.
    let mut alone = Scene::new(size, vec![moved(&paper.group, centre)]);
    alone.add_fonts(paper.fonts);
    if let Err(e) = alone.validate() {
        return (None, Some(e.to_string()));
    }
    // Each point plus the centre, as the sheet moves the group, so the two are the same numbers.
    let at = |p: &Vec2| {
        let p = *p + centre;
        [p.x, p.y]
    };
    let lines = |role: LineRole| paper.layout.lines.iter().filter(|(of, _)| *of == role).map(|(_, line)| line.iter().map(at).collect()).collect();
    let layout = DimensionLayoutResult {
        extension_lines: lines(LineRole::Extension),
        dimension_lines: lines(LineRole::Dimension),
        arrows: paper.layout.arrows.iter().map(|(tip, dir)| ArrowResult { tip: at(tip), points: [dir.x, dir.y] }).collect(),
        text_boxes: paper.layout.text_boxes.iter().map(|corners| corners.each_ref().map(at)).collect(),
    };
    (Some(layout), paper.why)
}

/// How a command's answer says where a reference stands with the model: the status's word, why
/// it failed, is lost or can't be measured, and the candidates when several things match or the
/// leads when it is lost. Balloons say it the same way.
pub(super) fn status_words(status: Status) -> (&'static str, Option<String>, Vec<CandidateResult>) {
    match status {
        Status::Ok => ("ok", None, Vec::new()),
        Status::Overridden => ("overridden", None, Vec::new()),
        Status::Ambiguous(found) => ("ambiguous", None, found.into_iter().map(candidate).collect()),
        Status::Suppressed => ("suppressed", None, Vec::new()),
        Status::Failed(why) => ("failed", Some(why), Vec::new()),
        Status::Lost(why, leads) => ("lost", Some(why), leads.into_iter().map(candidate).collect()),
        Status::Unmeasurable(why) => ("unmeasurable", Some(why), Vec::new()),
    }
}

fn candidate(c: Candidate) -> CandidateResult {
    match c {
        Candidate::Edge { body, index } => CandidateResult::Edge { body, index },
        Candidate::Corner(p) => CandidateResult::Corner([p.x, p.y, p.z]),
        Candidate::Face { body, index } => CandidateResult::Face { body, index },
    }
}

#[cfg(test)]
#[path = "drawing_dimension_tests.rs"]
mod tests;
