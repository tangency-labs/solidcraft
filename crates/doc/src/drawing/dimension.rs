//! A drawing's dimensions (W19b, #4896): each the distance or angle between named parts of its
//! view's model, measured as the view shows it ([`super::measure`]). A dimension
//! saves what it measures by name, never a picked point, so when the model changes it follows the
//! model or says what it lost: it never moves to other geometry. Its value and status come from
//! the model each time. The one thing worked out that is saved is its last good value, what it last
//! read, for when it can't be measured any more: nothing reads that as its value.
//!
//! - **Linear**: between two points ([`ModelPoint`]): corners, or the centres of circles and arcs
//!   (a hole's), horizontal, vertical or aligned as the view shows them, or along a direction on
//!   the view.
//! - **Length**: a straight edge's ends ([`ModelEdge`]).
//! - **Angle**: between two straight edges, the smaller angle between their lines, or on its other
//!   side π minus it, whichever way the kernel runs the edges.
//! - **Radial** (W20a): a circle's or an arc's radius, or its diameter, on a view that sees it face
//!   on, read exactly from the body.
//! - **Chain** (W20c): from each of its points to the next along one line, as the view shows them,
//!   the members adding up to the overall from the first point to the last.
//! - **Baseline** (W41, #13052): from one origin to each of its targets along one line, as the view
//!   shows them, each dimension stacked above the last on paper, nearest first.
//! - **Ordinate** (W41, #13052): the same distances, each shown as a number at the end of a leader
//!   from its target, the origin's as 0, with no dimension lines.
//! - **Cylinder** (W20b): a whole cylinder's diameter between its two silhouettes, on a view that
//!   doesn't look along its axis (a face, [`ModelFace`]).
//! - **Sphere** (R2b): a sphere's size on any view (a face, [`ModelFace`]): SØ, its diameter, when
//!   the body's faces on it are the whole sphere, else SR, its radius, where its contour in the
//!   view lies on them (a ball end, a dome).
//! - **Cone** (R2b): a cone's included angle between its two silhouettes, on a view that doesn't
//!   look along its axis (a face, [`ModelFace`]): a chamfer's, a countersink's, a drill point's.
//! - **Silhouettes** (W20b): between a silhouette of one cylinder and a silhouette of another, each
//!   named by its face and by whether it is the one nearer the other cylinder or the one farther,
//!   as the view shows them ([`Silhouette`]). Two silhouettes of one cylinder are its diameter.
//! - **Faces**: between two flat, parallel faces the view sees edge-on, which a fillet that takes
//!   their corners leaves measured.
//!
//! A reference names its body, the feature that made the body (so a suppressed feature can be told
//! from a deleted one) and its occurrence path (so a part placed twice has a dimension on each copy).
//! The dimension line sits `offset_mm` of paper from what it measures, and the text at a place on
//! paper from its view's centre, so it moves with the view and not with the model; or centred on
//! the dimension line.
//!
//! The rules, beside the record's: a dimension is on one of the drawing's views; a reference names
//! a body and its feature, and 1 to [`MAX_REFERENCE_NAME_BYTES`] bytes in each name, without control
//! characters; a corner names 2 to [`MAX_VERTEX_EDGES`] edges, a centre one; a chain has 3 to
//! [`MAX_CHAIN_POINTS`] points, a baseline set 2 to [`MAX_SET_TARGETS`] targets and an ordinate set 1
//! to [`MAX_SET_TARGETS`], and each shows its members' numbers, never a text instead; an occurrence
//! path is at most [`MAX_PLACEMENT_DEPTH`] ids, none 0; a true length is measured aligned; a
//! direction to measure along isn't zero, and is at most [`MAX_REAL`] each way, as every number a
//! drawing saves (a whole number past it reads, then saves as one the file's check refuses); the
//! offset and the text place are on paper ([`is_paper_coordinate`]); at most [`MAX_DECIMALS`]
//! decimals; an override text of 1 to [`MAX_TEXT_BYTES`] bytes; a last good value from 0 to
//! [`MAX_REAL`]; a model dimension's constraint id or parameter label of 1 to
//! [`MAX_MODEL_NAME_BYTES`] bytes; at most [`MAX_DIMENSIONS`] dimensions, listed by id. Two
//! dimensions that show one model dimension aren't refused, so a file a hand edit doubled opens.
//!
//! [`Document::resolve_dimensions`] finds what a dimension measures again, strictly: an edge by
//! its name, or the one edge between the faces it names, never the piece nearest an old pick; a
//! corner where all its named edges meet, never the nearer of two; a centre the centre of its one
//! named edge, which must be a circle or an arc; a cylinder the faces its named face is now, all
//! on one cylinder, with the silhouettes it measures on them; a flat face the faces its named face
//! is now, all on one oriented plane and not split on purpose. What it can't decide it says
//! ([`Status`]), and it never chooses: a lost one lists leads to what took its place (the face a
//! fillet put where its edge was), which only a repair, the user's own pick, puts in.
//! [`Document::edge_reference`], [`Document::corner_reference`] and [`Document::face_reference`]
//! turn what a command picks (a point, or an edge's or a face's index) into those names, and
//! refuse a pick that doesn't name one thing.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use super::measure::{self, AngleFrom, Cone, Cylinder, LinearDirection, Measured, Reading, SilhouetteSide, Sphere};
use serde::{Deserialize, Serialize};
use solidcore_record::brep::{Curve, Frame, Loop};
use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::drawing_view::{Invalid, is_paper_coordinate};
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::prelude::*;

use super::coverage::{edge_makers, face_maker};
use super::{Drawing, Units, add_by_id, in_id_order, require};
use crate::config::MAX_REFERENCE_NAME_BYTES;
use crate::construct::{MAX_VERTEX_EDGES, as_circle, as_line, point_tolerance, shared_ends};
use crate::display_units::length_number;
use crate::format::MAX_REAL;
use solidcraft_kernel::{FaceSurface, PointClass, edge_circle, face_surface};

use crate::appearance::face_index_at;
use crate::name_lookup::{BodyEdges, EdgeMatch, face_has_name, faces_with_stem};
use crate::naming::{face_name_exact, face_names};
use crate::persistent_name::{Base, EdgeName, FaceName, FeatureId, Rounds};
use crate::{
    DocError, Document, MAX_PLACEMENT_DEPTH, Mat, Model, ModelBody, ModelState, Result, apply_point, apply_vector, ids, map_plane, mat_inverse,
};

/// What a reading's number is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Length,
    Angle,
    Radius,
    Diameter,
    /// A sphere's radius, SR (ISO 129-1).
    SphericalRadius,
    /// A sphere's diameter, SØ (ISO 129-1).
    SphericalDiameter,
}

/// What a reading says: `value`, in model millimetres or radians for an angle, in `units` with
/// `decimals`. The one rule for a dimension's text and a view's measurements.
pub fn reading_label(kind: LabelKind, value: f64, units: &str, decimals: u8) -> String {
    let length = || length_number(value, units, decimals);
    match kind {
        LabelKind::Length => length(),
        LabelKind::Angle => format!("{:.*}°", usize::from(decimals), value.to_degrees()),
        LabelKind::Radius => format!("R{}", length()),
        LabelKind::Diameter => format!("{DIAMETER_SIGN}{}", length()),
        LabelKind::SphericalRadius => format!("SR{}", length()),
        LabelKind::SphericalDiameter => format!("S{DIAMETER_SIGN}{}", length()),
    }
}

/// The token of a dimension's text that stands for its measured number as it reads with no text of
/// its own: with its R or diameter sign, in the drawing's units, with the dimension's decimals.
pub const VALUE_TOKEN: &str = "{value}";

/// A piece of a dimension's text ([`Dimension::override_text`]): words as they are, or the
/// measured number.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TextPiece {
    Words(String),
    Value,
}

/// A dimension's text read as a template: `{value}` is the measured number, `{{` and `}}` are a
/// brace each, everything else is itself. A brace on its own, or a name in braces other than
/// `value`, is refused, saying which.
fn text_pieces(text: &str) -> std::result::Result<Vec<TextPiece>, String> {
    let mut pieces = Vec::new();
    let mut words = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['{', '}']) {
        words.push_str(&rest[..at]);
        let tail = &rest[at..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            words.push_str(&tail[..1]);
            rest = &tail[2..];
        } else if tail.starts_with('}') {
            return Err("a `}` on its own: write `}}` for a brace".into());
        } else {
            let end = tail.find('}').ok_or("a `{` on its own: write `{{` for a brace, or `{value}` for the measured number")?;
            let name = &tail[1..end];
            if name != "value" {
                return Err(format!("`{{{name}}}` is not something a dimension's text can show: `{{value}}` is its measured number"));
            }
            if !words.is_empty() {
                pieces.push(TextPiece::Words(std::mem::take(&mut words)));
            }
            pieces.push(TextPiece::Value);
            rest = &tail[end + 1..];
        }
    }
    words.push_str(rest);
    if !words.is_empty() {
        pieces.push(TextPiece::Words(words));
    }
    Ok(pieces)
}

/// Whether `text` shows the measured number somewhere ([`VALUE_TOKEN`]): it then follows the model.
/// A text without it is shown instead of the number. A text that can't be read shows none.
pub fn text_shows_value(text: &str) -> bool {
    text_pieces(text).is_ok_and(|pieces| pieces.contains(&TextPiece::Value))
}

/// `text` as a dimension shows it, with `number` where it says `{value}` (a question mark while
/// there is nothing measured to show). A text that can't be read as a template is shown as typed.
pub fn text_shown(text: &str, number: Option<&str>) -> String {
    let Ok(pieces) = text_pieces(text) else { return text.to_owned() };
    pieces
        .iter()
        .map(|piece| match piece {
            TextPiece::Words(words) => words.as_str(),
            TextPiece::Value => number.unwrap_or("?"),
        })
        .collect()
}

/// `text` written so that a template reads it back as these very characters: each brace doubled.
pub fn text_literal(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

/// The most dimensions a drawing may have.
pub const MAX_DIMENSIONS: usize = 10_000;
/// The most decimals a dimension shows.
pub const MAX_DECIMALS: u8 = 6;
/// The most candidates an ambiguous dimension lists.
pub const MAX_CANDIDATES: usize = 16;
/// The longest text shown instead of a dimension's number, in bytes.
pub const MAX_TEXT_BYTES: usize = 1024;
/// The most points a chain of dimensions runs through.
pub const MAX_CHAIN_POINTS: usize = 64;
/// The longest constraint id or parameter label a model dimension names, in bytes.
pub const MAX_MODEL_NAME_BYTES: usize = 256;
/// The most targets a set of dimensions from one origin measures.
pub const MAX_SET_TARGETS: usize = 64;
/// What a diameter's number follows: Ø (U+00D8), as tolerance texts write it. The diameter sign
/// U+2300 isn't in the bundled fonts, so it would print as a missing glyph.
const DIAMETER_SIGN: &str = "Ø";

/// One dimension of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The view it measures in and moves with.
    #[serde(with = "crate::ids")]
    pub view: u64,
    pub kind: DimensionKind,
    /// How far the dimension line is from what it measures, in paper millimetres; the sign says on
    /// which side.
    pub offset_mm: f64,
    pub text: TextPlace,
    /// 0 to [`MAX_DECIMALS`].
    pub decimals: u8,
    /// Shown instead of the number, which is still worked out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_text: Option<String>,
    /// What it last read (model millimetres, or radians for an angle), as of the last save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_good: Option<f64>,
    /// The model dimension it shows, when it was brought in from the model: each shows once on the
    /// drawing ([`Drawing::showing`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_model: Option<ModelDimension>,
}

/// One of the model's own dimensions, by what defines it: ids, so it is still the same one after
/// the model is edited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelDimension {
    /// A sketch's dimension: the sketch's feature and the constraint's own id.
    Sketch { sketch: u64, constraint: String },
    /// A feature's parameter: the feature and the parameter's label (`Distance`).
    Feature { feature: u64, param: String },
}

impl ModelDimension {
    /// The constraint id or the parameter label.
    pub fn name(&self) -> &str {
        match self {
            ModelDimension::Sketch { constraint: name, .. } | ModelDimension::Feature { param: name, .. } => name,
        }
    }
}

/// What a dimension measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionKind {
    /// The distance between two corners.
    Linear { a: ModelPoint, b: ModelPoint, direction: LinearDirection, reading: Reading },
    /// A straight edge's length: the distance between its ends. A round edge the view sees
    /// edge-on is drawn as a line, and its length is that line's.
    Length { edge: ModelEdge, direction: LinearDirection, reading: Reading },
    /// The angle between two straight edges' lines: the smaller one, or with `other_side` π minus it.
    Angle {
        a: ModelEdge,
        b: ModelEdge,
        reading: Reading,
        #[serde(default, skip_serializing_if = "is_false")]
        other_side: bool,
    },
    /// A circle's or an arc's radius, or with `diameter` its diameter, on a view that sees it face
    /// on.
    Radial {
        edge: ModelEdge,
        #[serde(default, skip_serializing_if = "is_false")]
        diameter: bool,
    },
    /// The distances from each point to the next, in order along one line as the view shows them.
    Chain { points: Vec<ModelPoint>, direction: LinearDirection },
    /// The distances from `origin` to each of `targets`, in any order, along one line as the view
    /// shows them: a dimension each, stacked on paper.
    Baseline { origin: ModelPoint, targets: Vec<ModelPoint>, direction: LinearDirection },
    /// The same distances as a baseline set's, each shown at the end of a leader from its target.
    Ordinate { origin: ModelPoint, targets: Vec<ModelPoint>, direction: LinearDirection },
    /// A whole cylinder's diameter between its two silhouettes, on a view that doesn't look along
    /// its axis: the cylinder a face of it lies on.
    Cylinder { face: ModelFace },
    /// A sphere's size, on any view: its diameter when the body's faces on the sphere a face of it
    /// lies on are the whole sphere, else its radius.
    Sphere { face: ModelFace },
    /// A cone's included angle, between its two silhouettes, on a view that doesn't look along its
    /// axis: the cone a face of it lies on.
    Cone { face: ModelFace },
    /// The distance between a silhouette of one cylinder and a silhouette of another, as the view
    /// shows them.
    Silhouettes { a: Silhouette, b: Silhouette, direction: LinearDirection },
    /// The distance between two flat, parallel faces, on a view that sees both edge-on.
    Faces { a: ModelFace, b: ModelFace, direction: LinearDirection },
}

impl DimensionKind {
    /// Each model reference it measures, in its order: the one list of them that what reads a
    /// dimension's references goes through.
    pub fn references(&self) -> Vec<Reference<'_>> {
        match self {
            DimensionKind::Linear { a, b, .. } => vec![Reference::Point(a), Reference::Point(b)],
            DimensionKind::Length { edge, .. } | DimensionKind::Radial { edge, .. } => vec![Reference::Edge(edge)],
            DimensionKind::Angle { a, b, .. } => vec![Reference::Edge(a), Reference::Edge(b)],
            DimensionKind::Chain { points, .. } => points.iter().map(Reference::Point).collect(),
            DimensionKind::Baseline { origin, targets, .. } | DimensionKind::Ordinate { origin, targets, .. } => {
                std::iter::once(origin).chain(targets).map(Reference::Point).collect()
            }
            DimensionKind::Cylinder { face } | DimensionKind::Sphere { face } | DimensionKind::Cone { face } => vec![Reference::Face(face)],
            DimensionKind::Silhouettes { a, b, .. } => vec![Reference::Face(&a.face), Reference::Face(&b.face)],
            DimensionKind::Faces { a, b, .. } => vec![Reference::Face(a), Reference::Face(b)],
        }
    }

    /// The occurrence path of each model reference it measures, in its order.
    pub fn paths(&self) -> Vec<&[u64]> {
        self.references().into_iter().map(Reference::path).collect()
    }

    /// Whether it reads an angle, as its label says: between two edges, or a cone's.
    pub fn is_angle(&self) -> bool {
        label_of(self) == Some(LabelKind::Angle)
    }
}

/// One of a cylinder's two silhouettes on a view: the cylinder by a face of it, and the side by
/// where the other cylinder of its dimension is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Silhouette {
    pub face: ModelFace,
    pub side: SilhouetteSide,
}

/// An edge of a body, by its persistent name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEdge {
    pub body: String,
    /// The feature that made the body.
    pub origin_feature: u64,
    pub name: String,
    /// The occurrences the body is placed through, from the top; empty for the design's own body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<u64>,
}

/// A face of a body, by its persistent name, as [`ModelEdge`] names an edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFace {
    pub body: String,
    /// The feature that made the body.
    pub origin_feature: u64,
    /// The face's persistent name.
    pub name: String,
    /// As [`ModelEdge::path`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<u64>,
}

/// A point of a body: a corner, the end its named edges share, or with `centre` the centre of the
/// circle or arc its one named edge is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPoint {
    pub body: String,
    /// The feature that made the body.
    pub origin_feature: u64,
    /// The persistent names of the edges that meet there, or of the circle or arc.
    pub edges: Vec<String>,
    /// The centre of the circle or arc `edges` names, not a corner.
    #[serde(default, skip_serializing_if = "is_false")]
    pub centre: bool,
    /// As [`ModelEdge::path`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<u64>,
}

/// Where a dimension's text sits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextPlace {
    /// At this point of paper, in millimetres from its view's centre: it stays there when the
    /// model changes.
    At([f64; 2]),
    /// On the middle of the dimension line, wherever that is.
    Centred,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Whether a dimension's record may keep `value` as its last good one.
fn is_last_good(value: f64) -> bool {
    (0.0..=MAX_REAL).contains(&value)
}

impl Drawing {
    /// Adds `dimensions`, each where its id puts it, as [`Drawing::add_views`] adds views.
    pub fn add_dimensions(&mut self, dimensions: impl IntoIterator<Item = Arc<Dimension>>) {
        add_by_id(&mut self.dimensions, dimensions, |d| d.id);
    }

    /// Dimension `id`, found by halves in the dimensions' id order.
    pub fn dimension(&self, id: u64) -> Option<&Dimension> {
        self.dimension_index(id).and_then(|i| self.dimensions.get(i)).map(|d| &**d)
    }

    /// The dimension that shows model dimension `model`, if one does: the first, on any view of any
    /// sheet.
    pub fn showing(&self, model: &ModelDimension) -> Option<u64> {
        self.dimensions.iter().find(|d| d.from_model.as_ref() == Some(model)).map(|d| d.id)
    }

    /// Where dimension `id` is in [`Drawing::dimensions`].
    pub fn dimension_index(&self, id: u64) -> Option<usize> {
        self.dimensions.binary_search_by_key(&id, |d| d.id).ok()
    }

    /// The dimensions' rules (the module's notes). The views are checked first, so they are in id
    /// order here.
    pub(super) fn check_dimensions(&self) -> Result<()> {
        require(self.dimensions.len() <= MAX_DIMENSIONS, || format!("a drawing has at most {MAX_DIMENSIONS} dimensions"))?;
        require(in_id_order(&self.dimensions, |d| d.id), || "a drawing lists its dimensions by id".into())?;
        for dimension in &self.dimensions {
            self.check_dimension(dimension).map_err(|e| DocError::Invalid(format!("dimension {}: {e}", ids::encode(dimension.id))))?;
        }
        Ok(())
    }

    fn check_dimension(&self, d: &Dimension) -> Result<()> {
        let on = || format!("it is on view {}, which the drawing doesn't have", ids::encode(d.view));
        require(self.view(d.view).is_some(), on)?;
        match &d.kind {
            DimensionKind::Linear { a, b, direction, reading } => {
                check_point(a)?;
                check_point(b)?;
                check_direction(*direction, *reading)?;
            }
            DimensionKind::Length { edge, direction, reading } => {
                check_edge(edge)?;
                check_direction(*direction, *reading)?;
            }
            DimensionKind::Angle { a, b, .. } => {
                check_edge(a)?;
                check_edge(b)?;
            }
            DimensionKind::Radial { edge, .. } => check_edge(edge)?,
            DimensionKind::Chain { points, direction } => {
                check_chain_length(points.len())?;
                points.iter().try_for_each(check_point)?;
                check_direction(*direction, Reading::Projected)?;
                let instead = || "a chain shows its members' numbers, never a text instead".into();
                require(d.override_text.is_none(), instead)?;
            }
            DimensionKind::Baseline { origin, targets, direction } | DimensionKind::Ordinate { origin, targets, direction } => {
                d.kind.set().map_or(Ok(()), |set| check_set_targets(set, targets.len()))?;
                std::iter::once(origin).chain(targets).try_for_each(check_point)?;
                check_direction(*direction, Reading::Projected)?;
                let instead = || "a set shows each target's number, never a text instead".into();
                require(d.override_text.is_none(), instead)?;
            }
            DimensionKind::Cylinder { face } | DimensionKind::Sphere { face } | DimensionKind::Cone { face } => check_face(face)?,
            DimensionKind::Silhouettes { a, b, direction } => {
                check_face(&a.face)?;
                check_face(&b.face)?;
                check_direction(*direction, Reading::Projected)?;
            }
            DimensionKind::Faces { a, b, direction } => {
                check_face(a)?;
                check_face(b)?;
                check_direction(*direction, Reading::Projected)?;
            }
        }
        let off_paper = || format!("its offset and its text's place are at most {MAX_SIDE_MM} mm on paper");
        let text = match d.text {
            TextPlace::At([x, y]) => is_paper_coordinate(x) && is_paper_coordinate(y),
            TextPlace::Centred => true,
        };
        require(is_paper_coordinate(d.offset_mm) && text, off_paper)?;
        require(d.decimals <= MAX_DECIMALS, || format!("it shows at most {MAX_DECIMALS} decimals"))?;
        require(d.last_good.is_none_or(is_last_good), || format!("its last good value is from 0 to {MAX_REAL}"))?;
        let name_fits = |model: &ModelDimension| {
            let name = model.name();
            !name.is_empty() && name.len() <= MAX_MODEL_NAME_BYTES && !name.chars().any(char::is_control)
        };
        require(d.from_model.as_ref().is_none_or(name_fits), || {
            format!("the model dimension it shows is named in 1 to {MAX_MODEL_NAME_BYTES} bytes, without control characters")
        })?;
        match &d.override_text {
            Some(text) => {
                require(!text.is_empty() && text.len() <= MAX_TEXT_BYTES, || {
                    format!("the text shown instead of its number is 1 to {MAX_TEXT_BYTES} bytes")
                })?;
                text_pieces(text).map(|_| ()).map_err(|why| DocError::Invalid(format!("its text: {why}")))
            }
            None => Ok(()),
        }
    }
}

/// A chain's number of points: 3 (2 make a linear dimension) to [`MAX_CHAIN_POINTS`].
pub fn check_chain_length(points: usize) -> Result<()> {
    require((3..=MAX_CHAIN_POINTS).contains(&points), || format!("a chain has 3 to {MAX_CHAIN_POINTS} points"))
}

/// The two kinds of dimension set, each measured from one origin to each of its targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetKind {
    Baseline,
    Ordinate,
}

impl DimensionKind {
    /// The kind of set it is, if it is one.
    pub fn set(&self) -> Option<SetKind> {
        match self {
            DimensionKind::Baseline { .. } => Some(SetKind::Baseline),
            DimensionKind::Ordinate { .. } => Some(SetKind::Ordinate),
            _ => None,
        }
    }
}

/// A set's number of targets: a baseline set's 2 (one makes a linear dimension) to
/// [`MAX_SET_TARGETS`], an ordinate set's 1 (beside its origin's 0) to it.
pub fn check_set_targets(set: SetKind, targets: usize) -> Result<()> {
    let (named, least) = match set {
        SetKind::Baseline => ("a baseline set", 2),
        SetKind::Ordinate => ("an ordinate set", 1),
    };
    require((least..=MAX_SET_TARGETS).contains(&targets), || format!("{named} has {least} to {MAX_SET_TARGETS} targets"))
}

/// An edge reference's rules (a balloon's edge keeps them too).
pub(super) fn check_edge(edge: &ModelEdge) -> Result<()> {
    check_body(&edge.body, edge.origin_feature, &edge.path)?;
    check_reference_name(&edge.name)
}

/// A point reference's rules.
fn check_face(face: &ModelFace) -> Result<()> {
    check_body(&face.body, face.origin_feature, &face.path)?;
    check_reference_name(&face.name)
}

pub(super) fn check_point(point: &ModelPoint) -> Result<()> {
    check_body(&point.body, point.origin_feature, &point.path)?;
    if point.centre {
        require(point.edges.len() == 1, || "a centre is of one named circle or arc".into())?;
    } else {
        require((2..=MAX_VERTEX_EDGES).contains(&point.edges.len()), || format!("a corner is where 2 to {MAX_VERTEX_EDGES} named edges meet"))?;
    }
    point.edges.iter().try_for_each(|name| check_reference_name(name))
}

/// The body a reference is on: its name, the feature that made it, and its occurrence path.
fn check_body(body: &str, origin_feature: u64, path: &[u64]) -> Result<()> {
    check_reference_name(body)?;
    require(origin_feature != 0, || "a reference names the feature that made its body".into())?;
    require(path.len() <= MAX_PLACEMENT_DEPTH && !path.contains(&0), || {
        format!("an occurrence path is at most {MAX_PLACEMENT_DEPTH} occurrences, none of them 0")
    })
}

/// A body's or an edge's name in a reference.
fn check_reference_name(name: &str) -> Result<()> {
    require(!name.is_empty() && name.len() <= MAX_REFERENCE_NAME_BYTES && !name.chars().any(char::is_control), || {
        format!("a name in a reference is 1 to {MAX_REFERENCE_NAME_BYTES} bytes, without control characters")
    })
}

/// A linear dimension's direction and reading: a true length is the segment's own, so it is
/// measured aligned (as [`measure::linear`] reads it), and a direction to
/// measure along isn't zero.
fn check_direction(direction: LinearDirection, reading: Reading) -> Result<()> {
    if let LinearDirection::Along(along) = direction {
        require(along.normalized().is_some(), || "the direction to measure along is zero or not finite".into())?;
        require(along.x.abs() <= MAX_REAL && along.y.abs() <= MAX_REAL, || format!("the direction to measure along is at most {MAX_REAL} each way"))?;
    }
    require(reading == Reading::Projected || direction == LinearDirection::Aligned, || "a true length is measured aligned".into())
}

/// A dimension as the model has it now ([`Document::resolve_dimensions`]): never saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub status: Status,
    /// What it reads (model millimetres, or radians for an angle), and whether a measured line
    /// leaves the view's plane: whenever what it measures was found and measured, so an
    /// overridden dimension has it too.
    pub measured: Option<Measured>,
    /// What it shows: the text given instead of its number, else the number in the drawing's
    /// units with its decimals (degrees for an angle, after R or Ø for a radius or a diameter, SR
    /// or SØ for a sphere's). `None` with neither, and for a chain, which shows its members'.
    pub text: Option<String>,
    /// Its measured number as it reads with no text of its own (R, the diameter sign and the
    /// degree sign included), whenever it was measured; none for a chain.
    pub number: Option<String>,
    /// What its number is, whenever it was measured, a chain's members aside: a sphere's says
    /// whether it is drawn as a diameter or a radius.
    pub label: Option<LabelKind>,
    /// A chain's members, from its first point to its last, or a set's, one for each target in its
    /// order, each as it reads and its text: empty unless measured, and for other kinds. `measured`
    /// is then the chain's overall, or the set's farthest target.
    pub members: Vec<Member>,
    /// Every name it measures was found by exact history, each as one edge.
    pub exact: bool,
    /// What it measures, in the design's frame: a linear dimension's two points, an angle's two
    /// lines (four points, each line from its first to its second), a radial dimension's
    /// centre and its arc's start, middle and end (a sphere's: the arc of its contour on its faces,
    /// or the whole contour from one point round to it again), a chain's points, a set's origin then
    /// its targets, or the ends of
    /// each silhouette measured (a cylinder's two, from its faces' one end along its axis to the
    /// other). Empty unless measured.
    pub points: Vec<Vec3>,
}

/// One of a chain's members as the model has it now.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub measured: Measured,
    /// Its number in the drawing's units with the chain's decimals.
    pub text: String,
}

/// Where a dimension, or a balloon's edge, stands with the model.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Measured.
    Ok,
    /// Measured, and showing the text given instead of its number.
    Overridden,
    /// A name finds several things, and none is chosen: at most [`MAX_CANDIDATES`] of them, in
    /// the model's order.
    Ambiguous(Vec<Candidate>),
    /// A feature it depends on is suppressed or rolled back.
    Suppressed,
    /// A feature it depends on failed: why. It comes back when that feature works again.
    Failed(String),
    /// What it measures is gone: why, and at most [`MAX_CANDIDATES`] leads to what took its place,
    /// offered and never taken.
    Lost(String, Vec<Candidate>),
    /// Found, but the view can't measure it so: why (such as an angle to a line the view sees
    /// end-on).
    Unmeasurable(String),
}

impl Status {
    /// Lost, and why, with no leads: its body or occurrence is gone, or its edges no longer meet.
    pub(crate) fn lost(why: impl Into<String>) -> Status {
        Status::Lost(why.into(), Vec::new())
    }
}

/// One of the things an ambiguous name finds, or a lead a lost one lists.
#[derive(Clone, Debug, PartialEq)]
pub enum Candidate {
    /// An edge of the body, by its index (as a `{body, index}` reference names it).
    Edge { body: String, index: usize },
    /// A point where the named edges meet, in the design's frame.
    Corner(Vec3),
    /// A face of the body, by its index (as a `{body, index}` face pick names it).
    Face { body: String, index: usize },
}

/// One model reference a record keeps ([`DimensionKind::references`], a note's anchor's), for
/// [`Document::finds_one`], its path and what it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reference<'a> {
    Edge(&'a ModelEdge),
    Point(&'a ModelPoint),
    Face(&'a ModelFace),
}

impl<'a> Reference<'a> {
    /// The occurrences its body is placed through, as [`ModelEdge::path`].
    pub fn path(self) -> &'a [u64] {
        match self {
            Reference::Edge(edge) => &edge.path,
            Reference::Point(point) => &point.path,
            Reference::Face(face) => &face.path,
        }
    }

    /// Its body's name, as its definition names it.
    pub fn body(self) -> &'a str {
        match self {
            Reference::Edge(edge) => &edge.body,
            Reference::Point(point) => &point.body,
            Reference::Face(face) => &face.body,
        }
    }
}

/// How a command names an edge (or a face) to measure, before it is saved by name
/// ([`Document::edge_reference`], [`Document::face_reference`]).
#[derive(Clone, Debug, PartialEq)]
pub enum EdgePick {
    /// A point on it, in the design's frame.
    Point(Vec3),
    /// Its index among its body's edges (or faces) as the model lists them: a [`Candidate::Edge`]'s
    /// (or a [`Candidate::Face`]'s).
    Index { body: String, index: usize },
}

/// How a command names a point to measure ([`Document::corner_reference`]).
#[derive(Clone, Debug, PartialEq)]
pub enum CornerPick {
    /// The corner, in the design's frame.
    Point(Vec3),
    /// Edges that meet there.
    Edges(Vec<EdgePick>),
    /// The centre of a circle or an arc: the edge.
    Centre(EdgePick),
}

impl Document {
    /// The edge `pick` names on `model` (this design's), on a body placed through the occurrences
    /// `path`, as a dimension saves it: its body, the feature that made the body, and its
    /// persistent name. A point must be on exactly one edge.
    pub fn edge_reference(&self, model: &Model, pick: &EdgePick, path: &[u64]) -> Result<ModelEdge> {
        let mut picker = Picker::new(self, model, path);
        let (body, edge) = picker.edge(pick)?;
        let (body, origin_feature, mut names) = picker.names(body, &[edge])?;
        Ok(ModelEdge { body, origin_feature, name: names.remove(0), path: path.to_vec() })
    }

    /// How far along `edge` (found on `model`, this design's) the point nearest `at` lies, as a
    /// part of its length from its start: where a balloon's arrow or a note's leader picked on it
    /// ends (#22195). `None` when the edge isn't found as one edge, or has no length.
    pub fn along_edge(&self, model: &Model, edge: &ModelEdge, at: Vec3) -> Option<f64> {
        let found = Lookup::new(self, model).edges(edge).ok().filter(|found| found.whole)?;
        found.pieces.first().and_then(|(_, line)| solidcraft_kernel::polyline_fraction_at(line, at))
    }

    /// The face `pick` names, by its persistent name, as [`Document::edge_reference`] names an edge:
    /// a point must be on one face of one body (to the body's pick tolerance).
    pub fn face_reference(&self, model: &Model, pick: &EdgePick, path: &[u64]) -> Result<ModelFace> {
        let mut picker = Picker::new(self, model, path);
        let (body, face) = picker.face(pick)?;
        let b = picker.state.bodies.get(body).ok_or_else(|| DocError::Invalid("the body picked no longer exists".into()))?;
        let name = face_names(b).get(face).filter(|n| !n.is_empty()).cloned();
        let name = name.ok_or_else(|| DocError::Invalid(format!("face {face} of `{}` has no name", b.name)))?;
        Ok(ModelFace { body: b.name.clone(), origin_feature: b.feature, name, path: path.to_vec() })
    }

    /// The straight edge `pick` names, found as [`Document::edge_reference`] finds it: its two ends in
    /// the design's frame, exactly, from the body's record (a line's two vertices), never from its
    /// mesh, for a decision about the model (which way an auxiliary view looks). An edge that isn't a
    /// line is refused.
    pub fn edge_line(&self, model: &Model, pick: &EdgePick, path: &[u64]) -> Result<[Vec3; 2]> {
        let mut picker = Picker::new(self, model, path);
        let (body, edge) = picker.edge(pick)?;
        let b = picker.state.bodies.get(body).ok_or_else(|| DocError::Invalid("the body picked no longer exists".into()))?;
        let not_straight = || DocError::Invalid(format!("edge {edge} of `{}` isn't straight: give a straight edge", b.name));
        // The model's edges are listed in the record's order, so the pick's index is the record's.
        let record = b.body.record();
        let line = record.edges.get(edge).filter(|e| matches!(e.curve, solidcore_record::brep::Curve::Line { .. })).ok_or_else(not_straight)?;
        let vertex = |v: u32| usize::try_from(v).ok().and_then(|v| record.vertices.get(v)).map(|v| Vec3::from(v.point));
        let [Some(start), Some(end)] = line.vertices.map(vertex) else { return Err(not_straight()) };
        let frame = self.body_frame_at(b, path)?;
        Ok([apply_point(&frame, start), apply_point(&frame, end)])
    }

    /// The flat face `pick` names, found as [`Document::face_reference`] finds it: a point of its
    /// plane and its normal, in the design's frame, exactly (the body's planar face, never its mesh).
    /// A face that isn't flat is refused.
    pub fn face_plane(&self, model: &Model, pick: &EdgePick, path: &[u64]) -> Result<(Vec3, Vec3)> {
        let mut picker = Picker::new(self, model, path);
        let (body, face) = picker.face(pick)?;
        let b = picker.state.bodies.get(body).ok_or_else(|| DocError::Invalid("the body picked no longer exists".into()))?;
        let plane = b.body.planar_face(face).ok_or_else(|| DocError::Invalid(format!("face {face} of `{}` isn't flat: give a flat face", b.name)))?;
        let frame = self.body_frame_at(b, path)?;
        Ok((apply_point(&frame, plane.origin), apply_vector(&frame, plane.normal())))
    }

    /// The dimension for the overall width (`vertical` false: along the view's horizontal) or height
    /// of body `body`, placed through `path`, on the view through `basis`, rounded corners included:
    /// a dimension between the two flat faces that bound it, each found by name, so it is the
    /// `faces` kind. Exact: the faces are the body's planar faces square to the view's axis, and the
    /// body's exact box must reach no farther than the two (a rounded side, whose bulge a flat face
    /// doesn't bound, is refused).
    pub fn overall_kind(&self, model: &Model, body: &str, path: &[u64], basis: &Plane, vertical: bool) -> Result<DimensionKind> {
        let state = model.state();
        let b = state
            .bodies
            .iter()
            .find(|b| b.name == body && !b.body.is_mesh())
            .ok_or_else(|| DocError::Invalid(format!("no body `{body}` to measure (a mesh body has no exact faces)")))?;
        let frame = self.body_frame_at(b, path)?;
        let axis: [f64; 3] = (if vertical { basis.y } else { basis.x }).into();
        let tol = surface_tolerance(b);
        // The flat faces square to the axis, by where they cut it.
        let mut square: Vec<(usize, f64)> = Vec::new();
        for f in 0..b.body.face_count() {
            let Some(plane) = b.body.planar_face(f) else { continue };
            match solidcraft_kernel::affine::square_plane_offset(&frame, plane.origin.into(), plane.normal().into(), axis) {
                Ok(Some(offset)) => square.push((f, offset)),
                Ok(None) => {}
                Err(why) => return Err(axis_refusal(why)),
            }
        }
        let way = if vertical { "height" } else { "width" };
        let by_offset = |p: &&(usize, f64), q: &&(usize, f64)| p.1.total_cmp(&q.1);
        let (Some(low), Some(high)) = (square.iter().min_by(by_offset), square.iter().max_by(by_offset)) else {
            return Err(DocError::Invalid(format!(
                "body `{body}` has no flat face square to the view's {way}: its overall {way} is not between two faces"
            )));
        };
        let gap = high.1 - low.1;
        require(gap > tol, || {
            format!("body `{body}`'s flat faces square to the view's {way} are one plane: there is no overall {way} between them")
        })?;
        let extent = body_extent(b, &frame, axis)?;
        require((extent - gap).abs() <= tol * 10.0, || {
            format!(
                "body `{body}` reaches {:.3} mm past its two flat faces along the view's {way}: dimension what bounds it there (silhouettes), or turn the view square to the part",
                (extent - gap).abs()
            )
        })?;
        let face = |f: usize| self.face_reference(model, &EdgePick::Index { body: body.to_string(), index: f }, path);
        Ok(DimensionKind::Faces { a: face(low.0)?, b: face(high.0)?, direction: LinearDirection::Aligned })
    }

    /// How far body `body` (placed through `path`) reaches along the unit vector `axis` of the design's
    /// frame, from its exact box, when `axis` runs along one of the body's own axes (so the box is
    /// tight there); none otherwise, or when the body has no exact box. Used to tell a straight
    /// edge shorter than the body it belongs to.
    pub fn body_extent_along(&self, model: &Model, body: &str, path: &[u64], axis: Vec3) -> Option<f64> {
        let state = model.state();
        let b = state.bodies.iter().find(|b| b.name == body && !b.body.is_mesh())?;
        let frame = self.body_frame_at(b, path).ok()?;
        let along_one = solidcraft_kernel::affine::along_own_axes(&frame, axis.into()).ok()?;
        if along_one { body_extent(b, &frame, axis.into()).ok() } else { None }
    }

    /// What a dimension between silhouettes `a` and `b` is on `model` (this design's): when both
    /// faces lie on one cylinder of one body placed once, that cylinder's diameter, which takes no
    /// direction; else the distance between them along `direction`, aligned if none.
    pub fn silhouettes_kind(&self, model: &Model, a: Silhouette, b: Silhouette, direction: Option<LinearDirection>) -> Result<DimensionKind> {
        let mut lookup = Lookup::new(self, model);
        let one = match (lookup.on_cylinder(&a.face), lookup.on_cylinder(&b.face)) {
            (Ok(x), Ok(y)) => x.body == y.body && a.face.path == b.face.path && same_cylinder(&x.cylinder, &y.cylinder, x.tol),
            _ => false,
        };
        match (one, direction) {
            (true, Some(_)) => Err(DocError::Invalid("both silhouettes are on one cylinder: its diameter takes no direction".into())),
            (true, None) => Ok(DimensionKind::Cylinder { face: a.face }),
            (false, direction) => Ok(DimensionKind::Silhouettes { a, b, direction: direction.unwrap_or(LinearDirection::Aligned) }),
        }
    }

    /// Whether `reference` still finds on `model` (this design's) the one edge, corner or face it
    /// names, neither lost nor cut into pieces: a repair keeps such a reference, whatever else its
    /// dimension lost.
    pub fn finds_one(&self, model: &Model, reference: Reference<'_>) -> bool {
        Lookup::new(self, model).finds_one(reference)
    }

    /// The point `pick` names, as [`Document::edge_reference`] names an edge: a corner by the edges
    /// that meet there, a centre by its circle or arc. A point must be the end of 2 to
    /// [`MAX_VERTEX_EDGES`] edges of one body, edges given must meet at one point, and a centre's
    /// edge must be a circle or an arc.
    pub fn corner_reference(&self, model: &Model, pick: &CornerPick, path: &[u64]) -> Result<ModelPoint> {
        let mut picker = Picker::new(self, model, path);
        let (body, edges) = match pick {
            CornerPick::Point(p) => picker.corner_at(*p)?,
            CornerPick::Edges(picks) => {
                // One past the most a corner names, so the record's rule refuses the rest.
                let picked = picks.iter().take(MAX_VERTEX_EDGES + 1).map(|e| picker.edge(e)).collect::<Result<Vec<_>>>()?;
                let body = picked.first().map_or(0, |(b, _)| *b);
                require(picked.iter().all(|(b, _)| *b == body), || "a corner's edges are on one body".into())?;
                let edges: Vec<usize> = picked.iter().map(|(_, e)| *e).collect();
                picker.meeting(body, &edges)?;
                (body, edges)
            }
            CornerPick::Centre(pick) => {
                let (body, edge) = picker.edge(pick)?;
                picker.round(body, edge)?;
                (body, vec![edge])
            }
        };
        let centre = matches!(pick, CornerPick::Centre(_));
        let (body, origin_feature, edges) = picker.names(body, &edges)?;
        Ok(ModelPoint { body, origin_feature, edges, centre, path: path.to_vec() })
    }

    /// Keeps what each dimension last read, by drawing and dimension id, in its record, as a
    /// design's file does. A dimension no longer there is passed over, and so is a value its record
    /// would refuse (a model can read past what a drawing saves), so the file still opens. A drawing
    /// with no new value stays shared.
    pub fn keep_last_good(&mut self, values: &BTreeMap<(u64, u64), f64>) {
        for drawing in &mut self.drawings {
            let id = drawing.id;
            for (&(_, dimension), &value) in values.range((id, 0)..=(id, u64::MAX)) {
                let Some(at) = drawing.dimension_index(dimension) else { continue };
                let new = is_last_good(value) && drawing.dimensions.get(at).is_some_and(|d| d.last_good != Some(value));
                if new && let Some(record) = Arc::make_mut(drawing).dimensions.get_mut(at) {
                    Arc::make_mut(record).last_good = Some(value);
                }
            }
        }
    }

    /// The radius of the circle or arc `edge` names, on `model` (this design's), as the view through
    /// `basis` sees it face on, and whether it was found exactly.
    pub fn radius_on_view(&self, model: &Model, edge: &ModelEdge, basis: &Plane) -> std::result::Result<(f64, bool), Status> {
        let (radius, _, exact) = Lookup::new(self, model).whole_arc(edge, basis)?;
        Ok((radius, exact))
    }

    /// The units `drawing` (one of this design's drawings) shows its numbers in.
    pub fn drawing_units<'a>(&'a self, drawing: &'a Drawing) -> &'a str {
        match &drawing.units {
            Units::Fixed { unit } => unit.as_str(),
            Units::FollowModel => self.units.as_str(),
        }
    }

    /// The dimensions `wanted` of `drawing` (one of this design's drawings) as `model` (this
    /// design's) has them now: what each measures found again by name, measured on its view, and
    /// the text it shows. A body is looked up and its edge names worked out once for the call,
    /// however many dimensions name it; a body none names isn't touched. `Err` only for an id the
    /// drawing doesn't have, or a damaged record.
    pub fn resolve_dimensions(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<Resolved>> {
        let units = self.drawing_units(drawing);
        let mut lookup = Lookup::new(self, model);
        wanted
            .iter()
            .map(|id| {
                let d = drawing.dimension(*id).ok_or_else(|| DocError::Unknown(format!("dimension {}", ids::encode(*id))))?;
                let basis = drawing.resolve_view(d.view)?.basis;
                Ok(lookup.resolve(d, &basis, units))
            })
            .collect()
    }
}

/// One call's lookups: the model's bodies, and the edge names of each body named, worked out once.
/// Balloons find their edges through it too ([`super::balloon`]).
pub(super) struct Lookup<'a> {
    doc: &'a Document,
    model: &'a Model,
    state: Arc<ModelState>,
    /// Each named body's edge names, by its index in `state`.
    named: BTreeMap<usize, BodyEdges>,
    /// Each body a cylinder or a sphere dimension names: the curved surface each face lies on, in
    /// the body's own frame, by face.
    curved: BTreeMap<usize, Vec<Option<Curved>>>,
    /// Each body a lost reference names: its leads, read on its first miss.
    leads: BTreeMap<usize, Leads>,
}

/// A curved surface a face lies on, in its body's own frame: the kinds a dimension reads by a face
/// ([`curved_of`]).
#[derive(Clone, Copy, Debug)]
enum Curved {
    /// Its axis a unit vector.
    Cylinder(Cylinder),
    Sphere(Sphere),
    /// Its axis a unit vector, from the apex into the cone.
    Cone(Cone),
}

impl Curved {
    /// Whether `self` and `other` are one surface, to `tol`: the kernel's own condition for faces on
    /// a common one. The kernel has none for a cone, so a cone takes the cylinder's margins
    /// ([`same_cylinder`]): `tol` for its apex and [`SAME_WAY`] for its axis's way and its
    /// half-angle. Its axis keeps its sense, as a double cone's two halves are two surfaces.
    fn same(&self, other: &Curved, tol: f64) -> bool {
        match (self, other) {
            (Curved::Cylinder(a), Curved::Cylinder(b)) => same_cylinder(a, b, tol),
            (Curved::Sphere(a), Curved::Sphere(b)) => a.center.dist(b.center) < tol && (a.radius - b.radius).abs() < tol,
            (Curved::Cone(a), Curved::Cone(b)) => {
                a.apex.dist(b.apex) < tol && a.axis.dot(b.axis) > SAME_WAY && (a.half_angle - b.half_angle).cos() > SAME_WAY
            }
            _ => false,
        }
    }
}

/// The faces of a body that lie on the surface a face lies on, as the model has it now
/// ([`Lookup::on_surface`]).
struct OnSurface {
    /// The body's index, and the frame its geometry is in.
    body: usize,
    frame: Mat,
    /// Every face of the body on the surface.
    faces: Vec<usize>,
    /// The kernel's tolerance for faces on one surface of the body.
    tol: f64,
    exact: bool,
}

/// The cylinder a face lies on as the model has it now ([`Lookup::on_cylinder`]).
struct OnCylinder {
    /// In the design's frame, its axis a unit vector.
    cylinder: Cylinder,
    /// The body's index, and the corners of its faces' triangles on the cylinder, in the design's
    /// frame.
    body: usize,
    corners: Vec<[Vec3; 3]>,
    /// The kernel's tolerance for faces on one cylinder of the body.
    tol: f64,
    exact: bool,
}

impl OnCylinder {
    /// Whether the silhouette `way` from the axis (a unit vector square to it) lies on the faces:
    /// a triangle of them lies across it.
    fn covers(&self, way: Vec3) -> bool {
        let Cylinder { origin, axis, radius } = self.cylinder;
        let round = |p: Vec3| (p - origin).dot(axis.cross(way)).atan2((p - origin).dot(way));
        self.corners.iter().any(|t| straddles(t.map(round), self.tol / radius.max(self.tol)))
    }

    /// That silhouette's ends, from the faces' one end along the axis to the other.
    fn ends(&self, way: Vec3) -> [Vec3; 2] {
        let Cylinder { origin, axis, radius } = self.cylinder;
        let along = |p: &Vec3| (*p - origin).dot(axis);
        let (low, high) = self.corners.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(along(p)), hi.max(along(p))));
        [origin + axis * low + way * radius, origin + axis * high + way * radius]
    }
}

/// A flat face as the model has it now ([`Lookup::on_plane`]), in the design's frame: a point on it,
/// its normal, and the corners of its triangles.
struct OnPlane {
    origin: Vec3,
    normal: Vec3,
    corners: Vec<[Vec3; 3]>,
    exact: bool,
}

impl OnPlane {
    /// The face's two ends as the view through `basis` sees it, edge-on: a line on paper.
    fn ends(&self, basis: &Plane) -> Option<[Vec3; 2]> {
        let across = basis.normal().cross(self.normal);
        let order = |p: &&Vec3, q: &&Vec3| p.dot(across).total_cmp(&q.dot(across));
        let points = || self.corners.iter().flatten();
        Some([*points().min_by(order)?, *points().max_by(order)?])
    }
}

/// What a dimension measures, found and measured: a chain's overall, and its members.
struct Found {
    measured: Measured,
    members: Vec<Measured>,
    points: Vec<Vec3>,
    exact: bool,
    /// What its number is, where the kind alone doesn't say: a sphere's SØ or SR.
    label: Option<LabelKind>,
}

impl Found {
    /// One measurement, no chain, whose kind says what its number is.
    fn new(measured: Measured, points: Vec<Vec3>, exact: bool) -> Found {
        Found { measured, members: Vec::new(), points, exact, label: None }
    }
}

/// The edges a name finds on a body: their indices and polylines in the design's frame, whether
/// they are the one edge it names, whether exact history found it, and the body's index and the
/// frame it is placed in.
pub(super) struct Edges {
    pub(super) pieces: Vec<(usize, Vec<Vec3>)>,
    pub(super) whole: bool,
    pub(super) exact: bool,
    body: usize,
    frame: Mat,
}

/// One body's face names, read once for the leads a lost reference lists: the edge each blend face
/// rounds (without its `~k`), by face, and the feature that made each face.
struct Leads {
    body: String,
    blends: Vec<(usize, EdgeName)>,
    makers: Vec<Option<FeatureId>>,
}

impl Leads {
    fn new(body: &str, faces: &[String]) -> Leads {
        let mut leads = Leads { body: body.to_string(), blends: Vec::new(), makers: Vec::with_capacity(faces.len()) };
        for (f, name) in faces.iter().enumerate() {
            let parsed = FaceName::parse(name).ok();
            if let Some(FaceName { base: Base::Blend { rounds: Rounds::Edge(edge), .. }, .. }) = &parsed {
                leads.blends.push((f, untied(edge)));
            }
            leads.makers.push(parsed.as_ref().and_then(FaceName::feature));
        }
        leads
    }

    /// What lost edge `name` leads to, on a body whose edges separate `edge_faces`: the blend faces
    /// that round it (a piece of one too), then their edges, then the other faces the features
    /// that made it made; each in index order, at most [`MAX_CANDIDATES`].
    fn for_edge(&self, name: &str, edge_faces: &[Vec<u32>]) -> Vec<Candidate> {
        let lost = EdgeName::parse(name).ok().map(|e| untied(&e));
        let rounds: Vec<usize> = self.blends.iter().filter(|(_, e)| lost.as_ref() == Some(e)).map(|(f, _)| *f).collect();
        // Only a blend's own edges are leads: with no blend, the body's edges aren't walked.
        let walked = if rounds.is_empty() { &[][..] } else { edge_faces };
        let bounding = walked.iter().enumerate().filter(|(_, fs)| fs.iter().any(|f| rounds.contains(&(*f as usize))));
        let edges = bounding.map(|(e, _)| Candidate::Edge { body: self.body.clone(), index: e });
        let makers = edge_makers(name);
        let made = self.made_by(&makers).filter(|f| !rounds.contains(f)).map(|f| self.face(f));
        rounds.iter().map(|f| self.face(*f)).chain(edges).chain(made).take(MAX_CANDIDATES).collect()
    }

    /// What lost face `name` leads to: the faces the feature that made it made, in index order, at
    /// most [`MAX_CANDIDATES`].
    fn for_face(&self, name: &str) -> Vec<Candidate> {
        let makers: Vec<FeatureId> = face_maker(name).into_iter().collect();
        self.made_by(&makers).map(|f| self.face(f)).take(MAX_CANDIDATES).collect()
    }

    /// The faces one of `makers` made.
    fn made_by<'s>(&'s self, makers: &'s [FeatureId]) -> impl Iterator<Item = usize> + 's {
        self.makers.iter().enumerate().filter(move |(_, m)| m.is_some_and(|m| makers.contains(&m))).map(|(f, _)| f)
    }

    fn face(&self, f: usize) -> Candidate {
        Candidate::Face { body: self.body.clone(), index: f }
    }
}

/// `edge` without its `~k`: a blend names the edge it rounds as it was, whichever of several it was.
fn untied(edge: &EdgeName) -> EdgeName {
    EdgeName { faces: edge.faces.clone(), tie: None }
}

/// How a reference stands when what it names isn't found, by the features that made it: suppressed
/// or rolled back, failed, or else lost, and why.
fn gone(model: &Model, makers: &[FeatureId], lost: String) -> Status {
    let results = || model.results.iter().filter(|r| makers.iter().any(|m| m.0 == r.id));
    if results().any(|r| r.skipped) {
        return Status::Suppressed;
    }
    results().find_map(|r| r.error.clone()).map_or(Status::lost(lost), Status::Failed)
}

impl<'a> Lookup<'a> {
    pub(super) fn new(doc: &'a Document, model: &'a Model) -> Lookup<'a> {
        Lookup { doc, model, state: model.state(), named: BTreeMap::new(), curved: BTreeMap::new(), leads: BTreeMap::new() }
    }

    fn resolve(&mut self, d: &Dimension, basis: &Plane, units: &str) -> Resolved {
        let (status, found) = match self.measure(d, basis) {
            // A text that shows the measured number follows the model, as a plain dimension does:
            // only one that hides the number is typed over it.
            Ok(found) => {
                (if d.override_text.as_deref().is_some_and(|text| !text_shows_value(text)) { Status::Overridden } else { Status::Ok }, Some(found))
            }
            Err(status) => (status, None),
        };
        let measured = found.as_ref().map(|f| f.measured);
        let label = found.as_ref().and_then(|f| f.label.or_else(|| label_of(&d.kind)));
        let length = |m: Measured| length_number(m.value, units, d.decimals);
        let number = measured.zip(label).map(|(m, kind)| reading_label(kind, m.value, units, d.decimals));
        let (members, points, exact) = found.map_or_else(Default::default, |f| (f.members, f.points, f.exact));
        Resolved {
            status,
            text: match &d.override_text {
                Some(text) => Some(text_shown(text, number.as_deref())),
                None => number.clone(),
            },
            number,
            label,
            members: members.into_iter().map(|m| Member { measured: m, text: length(m) }).collect(),
            exact,
            points,
            measured,
        }
    }

    /// `d` found and measured on the view through `basis`, else the status that says why not.
    fn measure(&mut self, d: &Dimension, basis: &Plane) -> std::result::Result<Found, Status> {
        let unmeasurable = |e: Invalid| Status::Unmeasurable(e.0.to_string());
        match &d.kind {
            DimensionKind::Linear { a, b, direction, reading } => {
                let (a, exact_a) = self.corner(a)?;
                let (b, exact_b) = self.corner(b)?;
                let measured = measure::linear(basis, a, b, *direction, *reading).map_err(unmeasurable)?;
                Ok(Found::new(measured, vec![a, b], exact_a && exact_b))
            }
            DimensionKind::Length { edge, direction, reading } => {
                let found = self.edges(edge)?;
                // A split edge's length is no longer one number.
                if !found.whole {
                    return Err(Status::Ambiguous(edge_candidates(&edge.body, &found)));
                }
                let ends = found.pieces.first().and_then(|(_, line)| straight(line)).or_else(|| self.edge_on(&found, basis));
                let [a, b] = ends.ok_or_else(|| Status::Unmeasurable(format!("edge `{}` isn't straight", edge.name)))?;
                let measured = measure::linear(basis, a, b, *direction, *reading).map_err(unmeasurable)?;
                Ok(Found::new(measured, vec![a, b], found.exact))
            }
            DimensionKind::Angle { a, b, reading, other_side } => {
                let (a, exact_a) = self.line(a)?;
                let (b, exact_b) = self.line(b)?;
                let between = measure::angle(basis, (a[0], a[1]), AngleFrom::Edge(b[0], b[1]), *reading).map_err(unmeasurable)?;
                let value = between_lines(between.value, *other_side);
                let points = vec![a[0], a[1], b[0], b[1]];
                Ok(Found::new(Measured { value, ..between }, points, exact_a && exact_b))
            }
            DimensionKind::Radial { edge, diameter } => {
                let (radius, points, exact) = self.whole_arc(edge, basis)?;
                let value = if *diameter { 2.0 * radius } else { radius };
                let measured = Measured { value, foreshortened: false };
                Ok(Found::new(measured, points.to_vec(), exact))
            }
            DimensionKind::Chain { points, direction } => {
                let found = points.iter().map(|p| self.corner(p)).collect::<std::result::Result<Vec<_>, _>>()?;
                let exact = found.iter().all(|(_, exact)| *exact);
                let points: Vec<Vec3> = found.into_iter().map(|(p, _)| p).collect();
                let chain = measure::chain(basis, &points, *direction).map_err(unmeasurable)?;
                Ok(Found { members: chain.members, ..Found::new(chain.overall, points, exact) })
            }
            DimensionKind::Baseline { origin, targets, direction } | DimensionKind::Ordinate { origin, targets, direction } => {
                let found = std::iter::once(origin).chain(targets).map(|p| self.corner(p)).collect::<std::result::Result<Vec<_>, _>>()?;
                let exact = found.iter().all(|(_, exact)| *exact);
                let points: Vec<Vec3> = found.into_iter().map(|(p, _)| p).collect();
                let (origin, targets) = points.split_first().ok_or_else(|| Status::Unmeasurable("a set has no origin".into()))?;
                let set = measure::from_origin(basis, *origin, targets, *direction).map_err(unmeasurable)?;
                // An ordinate shows a target in line with its origin as 0; a baseline set has no line for it.
                let baseline = matches!(d.kind, DimensionKind::Baseline { .. });
                if baseline && set.members.iter().any(|m| m.value <= solidcraft_geom::EPS) {
                    return Err(Status::Unmeasurable("a target in line with its origin has no distance to dimension".into()));
                }
                Ok(Found { members: set.members, ..Found::new(set.farthest, points, exact) })
            }
            DimensionKind::Cylinder { face } => self.cylinder(face, basis),
            DimensionKind::Sphere { face } => self.sphere(face, basis),
            DimensionKind::Cone { face } => self.cone(face, basis),
            DimensionKind::Silhouettes { a, b, direction } => self.silhouettes(a, b, *direction, basis),
            DimensionKind::Faces { a, b, direction } => self.faces(a, b, *direction, basis),
        }
    }

    /// The body a reference names, as the model has it now: its index, and the frame its geometry
    /// is in through the reference's occurrence path.
    fn body(&self, body: &str, feature: u64, path: &[u64]) -> std::result::Result<(usize, Mat), Status> {
        let found = self.state.bodies.iter().enumerate().find(|(_, b)| b.name == body && b.feature == feature && !b.body.is_mesh());
        let Some((i, b)) = found else {
            let remade = self.state.bodies.iter().any(|b| b.name == body);
            let why = if remade { format!("body `{body}` is made by another feature now") } else { format!("body `{body}` no longer exists") };
            return Err(gone(self.model, &[FeatureId(feature)], why));
        };
        let frame = self.doc.body_frame_at(b, path).map_err(|_| Status::lost("the occurrence its body is placed through no longer exists"))?;
        Ok((i, frame))
    }

    /// How a reference to edge `name` of body `i` stands when it isn't found ([`gone`]): when lost,
    /// with its leads ([`Leads::for_edge`]). A suppressed or failed one comes back by itself.
    fn lost_edge(&mut self, i: usize, name: &str) -> Status {
        match gone(self.model, &edge_makers(name), format!("edge `{name}` no longer exists")) {
            Status::Lost(why, _) => {
                let mesh = self.state.bodies.get(i).map(ModelBody::mesh);
                let leads = self.body_leads(i).zip(mesh).map_or_else(Vec::new, |(leads, mesh)| leads.for_edge(name, &mesh.edge_faces));
                Status::Lost(why, leads)
            }
            status => status,
        }
    }

    /// As [`Lookup::lost_edge`], for face `name` ([`Leads::for_face`]).
    fn lost_face(&mut self, i: usize, name: &str) -> Status {
        let makers: Vec<FeatureId> = face_maker(name).into_iter().collect();
        match gone(self.model, &makers, format!("face `{name}` no longer exists")) {
            Status::Lost(why, _) => Status::Lost(why, self.body_leads(i).map_or_else(Vec::new, |leads| leads.for_face(name))),
            status => status,
        }
    }

    /// Body `i`'s leads, read on its first miss.
    fn body_leads(&mut self, i: usize) -> Option<&Leads> {
        let b = self.state.bodies.get(i)?;
        Some(self.leads.entry(i).or_insert_with(|| Leads::new(&b.name, &face_names(b))))
    }

    /// Body `i`'s edge names, worked out on first use.
    fn names(&mut self, i: usize) -> std::result::Result<&BodyEdges, Status> {
        let b = self.state.bodies.get(i).ok_or_else(|| Status::lost("its body no longer exists"))?;
        Ok(self.named.entry(i).or_insert_with(|| BodyEdges::new(b)))
    }

    /// Where a balloon's arrow or a note's leader ends on the edge `edge` names, in the design's
    /// frame: `along` its length (0 at its start, 1 at its end), else its middle. How it stands, the
    /// point, and whether it was found by exact history. A split edge has no one place along it: the
    /// pieces are listed, and none is chosen.
    pub(super) fn along(&mut self, edge: &ModelEdge, along: Option<f64>) -> (Status, Option<Vec3>, bool) {
        match self.edges(edge) {
            Ok(found) if !found.whole => (Status::Ambiguous(edge_candidates(&edge.body, &found)), None, false),
            Ok(found) => {
                let point = found.pieces.first().and_then(|(_, line)| solidcraft_kernel::polyline_point_at(line, along.unwrap_or(0.5)));
                let status = if point.is_some() { Status::Ok } else { Status::lost(format!("edge `{}` has no length", edge.name)) };
                (status, point, found.exact)
            }
            Err(status) => (status, None, false),
        }
    }

    /// The edges `edge` names: the one edge, or the pieces of it. One piece is that edge, though not
    /// found exactly: a name saved from a split face finds it on a body with the one face.
    pub(super) fn edges(&mut self, edge: &ModelEdge) -> std::result::Result<Edges, Status> {
        let (i, frame) = self.body(&edge.body, edge.origin_feature, &edge.path)?;
        let names = self.names(i)?;
        let (indices, named) = match names.matching(&edge.name) {
            Some(EdgeMatch::Edge(e)) => (vec![e], true),
            Some(EdgeMatch::Pieces(pieces)) => (pieces, false),
            None => return Err(self.lost_edge(i, &edge.name)),
        };
        let whole = named || indices.len() == 1;
        let exact = named && indices.iter().all(|e| names.verified(*e));
        let placed = |e: &usize| names.mesh.edges.get(*e).map(|line| (*e, line.iter().map(|p| apply_point(&frame, *p)).collect()));
        Ok(Edges { pieces: indices.iter().filter_map(placed).collect(), whole, exact, body: i, frame })
    }

    /// [`Document::finds_one`]: the one edge (or the one piece left of it), the corner, the one face.
    fn finds_one(&mut self, reference: Reference<'_>) -> bool {
        match reference {
            Reference::Edge(edge) => self.edges(edge).is_ok_and(|found| found.whole),
            Reference::Point(point) => self.corner(point).is_ok(),
            Reference::Face(face) => {
                let body = self.body(&face.body, face.origin_feature, &face.path).ok().and_then(|(i, _)| self.state.bodies.get(i));
                body.is_some_and(|b| faces_named(b, &face.name).0.len() == 1)
            }
        }
    }

    /// The circle or arc `edge` names, as [`Lookup::arc`] reads it, and whether it was found exactly.
    /// The pieces of a split edge on one circle are still that circle, as [`Lookup::line`] keeps a
    /// line: read from the longest, so its points lie on what is left of it. Pieces on several
    /// circles are listed, and none is chosen.
    pub(super) fn whole_arc(&mut self, edge: &ModelEdge, basis: &Plane) -> std::result::Result<(f64, [Vec3; 4], bool), Status> {
        let found = self.edges(edge)?;
        if !found.whole && one_circle(&found.pieces).is_none() {
            return Err(Status::Ambiguous(edge_candidates(&edge.body, &found)));
        }
        let length = |line: &[Vec3]| line.windows(2).map(|w| w.first().zip(w.last()).map_or(0.0, |(a, b)| a.dist(*b))).sum::<f64>();
        let longest = found.pieces.iter().max_by(|a, b| length(&a.1).total_cmp(&length(&b.1)));
        let not_found = || Status::lost(format!("edge `{}` no longer exists", edge.name));
        let (radius, points) = self.arc(edge, &found, longest.ok_or_else(not_found)?, basis)?;
        Ok((radius, points, found.exact))
    }

    /// The circle or arc that `piece` of `found` is, as the view through `basis` sees it face on:
    /// its radius, and its centre and its start, middle and end in the design's frame. Read exactly
    /// from the body, in the view's plane through the edge, so the points are the edge's.
    fn arc(&self, edge: &ModelEdge, found: &Edges, piece: &(usize, Vec<Vec3>), basis: &Plane) -> std::result::Result<(f64, [Vec3; 4]), Status> {
        let face_on = || Status::Unmeasurable(format!("edge `{}` isn't a circle in this view (it isn't parallel to it)", edge.name));
        let (index, line) = piece;
        let through = Plane { origin: *line.first().ok_or_else(face_on)?, ..*basis };
        let local = mat_inverse(&found.frame).and_then(|back| map_plane(&back, &through)).ok_or_else(face_on)?;
        let body = self.state.bodies.get(found.body).ok_or_else(|| Status::lost("its body no longer exists"))?;
        let pieces = body.body.planar_edges(&local, &[*index]).pop().flatten().ok_or_else(face_on)?;
        let not_round = || Status::Unmeasurable(format!("edge `{}` isn't a circle or an arc", edge.name));
        // A spline piece is no arc: only the pieces the kernel gives as segments can be one.
        let segments = |pieces: Vec<crate::kernel::PlanarPiece>| -> Option<Vec<Seg2>> {
            pieces
                .into_iter()
                .map(|piece| match piece {
                    crate::kernel::PlanarPiece::Seg(seg) => Some(seg),
                    crate::kernel::PlanarPiece::Spline(_) => None,
                })
                .collect()
        };
        // An edge held only as a spline, such as where a countersink meets its bore (#22308), is
        // still the circle of a cylinder it bounds when the view looks along that cylinder's axis:
        // the kernel's certified circular reading of it.
        let segs = segments(pieces).or_else(|| body.body.cylinder_edge_projection(&local, *index).and_then(segments));
        let (radius, points) = segs.as_deref().and_then(one_arc).ok_or_else(not_round)?;
        Ok((radius, points.map(|p| apply_point(&found.frame, local.to_world(p)))))
    }

    /// The straight line `edge` names, from end to end, in the design's frame. The pieces of a
    /// split edge that lie on one line are still that line.
    fn line(&mut self, edge: &ModelEdge) -> std::result::Result<([Vec3; 2], bool), Status> {
        let found = self.edges(edge)?;
        let not_straight = || Status::Unmeasurable(format!("edge `{}` isn't straight", edge.name));
        let (_, along) = found.pieces.first().and_then(|(_, line)| as_line(line)).ok_or_else(not_straight)?;
        let mut points: Vec<Vec3> = found.pieces.iter().flat_map(|(_, line)| line.iter().copied()).collect();
        points.sort_by(|p, q| p.dot(along).total_cmp(&q.dot(along)));
        match straight(&points) {
            Some(ends) => Ok((ends, found.exact)),
            None if found.whole => Err(not_straight()),
            None => Err(Status::Ambiguous(edge_candidates(&edge.body, &found))),
        }
    }

    /// The line the view through `basis` draws for the round edge `found` when it sees the edge's
    /// circle edge-on: the edge's two points furthest apart along that line, in the design's
    /// frame. They are read from the circle the edge lies on ([`edge_circle`], exact), never from
    /// its mesh: a whole circle's line is its diameter, and an arc's runs between its ends or out
    /// to the side of the circle it reaches. `None` for an edge on no circle, or one the view sees
    /// at a slant (an ellipse) or face on.
    fn edge_on(&self, found: &Edges, basis: &Plane) -> Option<[Vec3; 2]> {
        let (index, _) = found.pieces.first()?;
        let body = self.state.bodies.get(found.body)?;
        let circle = edge_circle(&body.body, *index)?;
        // Worked in the body's own frame, which the occurrence carries rigidly to the design's.
        let back = mat_inverse(&found.frame)?;
        let view = apply_vector(&back, basis.normal()).normalized()?;
        let axis = Vec3::from(circle.axis);
        if axis.dot(view).abs() > EDGE_ON {
            return None;
        }
        // The line runs square to both the view and the circle's axis.
        let along = view.cross(axis).normalized()?;
        let from = circle.start - circle.center;
        let whole = circle.angle >= TAU - SAME_ANGLE;
        let on_arc = |u: Vec3| whole || axis.dot(from.cross(u)).atan2(from.dot(u)).rem_euclid(TAU) <= circle.angle + SAME_ANGLE;
        let mut points = vec![circle.start, circle.end];
        points.extend([along, -along].into_iter().map(|side| side * circle.radius).filter(|u| on_arc(*u)).map(|u| circle.center + u));
        let at = |p: &&Vec3| p.dot(along);
        let lo = points.iter().min_by(|p, q| at(p).total_cmp(&at(q)))?;
        let hi = points.iter().max_by(|p, q| at(p).total_cmp(&at(q)))?;
        Some([apply_point(&found.frame, *lo), apply_point(&found.frame, *hi)])
    }

    /// The point `point` names, in the design's frame: a corner, the one end all its named edges
    /// (or their pieces) share, or a centre.
    pub(super) fn corner(&mut self, point: &ModelPoint) -> std::result::Result<(Vec3, bool), Status> {
        if point.centre {
            return self.centre(point);
        }
        let (i, frame) = self.body(&point.body, point.origin_feature, &point.path)?;
        let names = self.names(i)?;
        let mut exact = true;
        let mut ends: Vec<Vec<Vec3>> = Vec::with_capacity(point.edges.len());
        for name in &point.edges {
            let (indices, whole) = match names.matching(name) {
                Some(EdgeMatch::Edge(e)) => (vec![e], true),
                Some(EdgeMatch::Pieces(pieces)) => (pieces, false),
                None => return Err(self.lost_edge(i, name)),
            };
            exact &= whole && indices.iter().all(|e| names.verified(*e));
            let lines = indices.iter().filter_map(|e| names.mesh.edges.get(*e));
            ends.push(lines.flat_map(|line| [line.first(), line.last()]).flatten().copied().collect());
        }
        match shared_ends(&ends).as_slice() {
            [] => Err(Status::lost(format!("edges `{}` no longer meet", point.edges.join("`, `")))),
            [p] => Ok((apply_point(&frame, *p), exact)),
            several => Err(Status::Ambiguous(several.iter().take(MAX_CANDIDATES).map(|p| Candidate::Corner(apply_point(&frame, *p))).collect())),
        }
    }

    /// The centre `point` names: of the circle or arc its one named edge is, or the one circle its
    /// pieces lie on, as construction geometry finds a round edge's ([`as_circle`]).
    fn centre(&mut self, point: &ModelPoint) -> std::result::Result<(Vec3, bool), Status> {
        let name = point.edges.first().ok_or_else(|| Status::lost("its centre names no edge"))?;
        let edge = ModelEdge { body: point.body.clone(), origin_feature: point.origin_feature, name: name.clone(), path: point.path.clone() };
        let found = self.edges(&edge)?;
        match one_circle(&found.pieces) {
            Some(centre) => Ok((centre, found.exact)),
            None if found.whole => Err(Status::Unmeasurable(format!("edge `{name}` isn't a circle or an arc"))),
            None => Err(Status::Ambiguous(edge_candidates(&edge.body, &found))),
        }
    }

    /// The cylinder `face` lies on (or the pieces it was cut into, all on one cylinder), with every
    /// face of the body on that cylinder.
    fn on_cylinder(&mut self, face: &ModelFace) -> std::result::Result<OnCylinder, Status> {
        let (on, first) = self.on_surface(face, "cylinder", |c| if let Curved::Cylinder(c) = c { Some(*c) } else { None })?;
        let state = Arc::clone(&self.state);
        let b = state.bodies.get(on.body).ok_or_else(|| Status::lost("its body no longer exists"))?;
        let axis = apply_vector(&on.frame, first.axis).normalized().ok_or_else(|| Status::lost("its occurrence flattens it"))?;
        let cylinder = Cylinder { origin: apply_point(&on.frame, first.origin), axis, radius: first.radius };
        Ok(OnCylinder { cylinder, body: on.body, corners: face_corners(b, &on.frame, &on.faces), tol: on.tol, exact: on.exact })
    }

    /// The curved surface `face` lies on (or the pieces it was cut into, all on one), with every
    /// face of the body on it: `kind` reads the surface wanted off it, and a face on no such
    /// surface isn't a `noun`.
    fn on_surface<T>(&mut self, face: &ModelFace, noun: &str, kind: impl Fn(&Curved) -> Option<T>) -> std::result::Result<(OnSurface, T), Status> {
        let (i, frame) = self.body(&face.body, face.origin_feature, &face.path)?;
        let state = Arc::clone(&self.state);
        let b = state.bodies.get(i).ok_or_else(|| Status::lost("its body no longer exists"))?;
        let (found, exact) = faces_named(b, &face.name);
        let curved = self.curved.entry(i).or_insert_with(|| curved_of(b));
        let on = |f: &usize| curved.get(*f).copied().flatten();
        let isnt = || Status::Unmeasurable(format!("face `{}` isn't a {noun}", face.name));
        let first = match found.first() {
            None => return Err(self.lost_face(i, &face.name)),
            Some(f) => on(f).ok_or_else(isnt)?,
        };
        let wanted = kind(&first).ok_or_else(isnt)?;
        let tol = surface_tolerance(b);
        if !found.iter().all(|f| on(f).is_some_and(|c| first.same(&c, tol))) {
            return Err(Status::Ambiguous(face_candidates(b, &found)));
        }
        let faces = (0..curved.len()).filter(|f| on(f).is_some_and(|c| first.same(&c, tol))).collect();
        Ok((OnSurface { body: i, frame, faces, tol, exact }, wanted))
    }

    /// A sphere's size on the view through `basis`: its diameter when the body's faces on it are
    /// the whole sphere ([`rim`] finds none), else its radius, read on the arc of its contour in
    /// this view that lies on them ([`contour_on`]). Its points are its centre and that arc's start,
    /// middle and end, as a radial dimension's are; a whole sphere's arc is its whole contour.
    fn sphere(&mut self, face: &ModelFace, basis: &Plane) -> std::result::Result<Found, Status> {
        let (on, sphere) = self.on_surface(face, "sphere", |c| if let Curved::Sphere(s) = c { Some(*s) } else { None })?;
        let state = Arc::clone(&self.state);
        let b = state.bodies.get(on.body).ok_or_else(|| Status::lost("its body no longer exists"))?;
        // The contour is read in the body's own frame, where its faces and edges are.
        let flattens = || Status::lost("its occurrence flattens it");
        let to_body = mat_inverse(&on.frame).ok_or_else(flattens)?;
        let seen = Plane::new(sphere.center, apply_vector(&to_body, basis.x), apply_vector(&to_body, basis.y)).ok_or_else(flattens)?;
        let rim = rim(b, &on.faces);
        let (label, value, (start, sweep)) = if rim.is_empty() {
            (LabelKind::SphericalDiameter, 2.0 * sphere.radius, (0.0, TAU))
        } else {
            let arc = contour_on(b, &seen, &sphere, &on.faces, &rim)?.ok_or_else(|| {
                Status::Unmeasurable(format!("face `{}`'s contour in this view isn't on it: the view sees its rim, not its radius", face.name))
            })?;
            (LabelKind::SphericalRadius, sphere.radius, arc)
        };
        let at = |angle: f64| apply_point(&on.frame, sphere.contour_point(&seen, angle));
        let points = vec![apply_point(&on.frame, sphere.center), at(start), at(start + sweep / 2.0), at(start + sweep)];
        let measured = Measured { value, foreshortened: false };
        Ok(Found { label: Some(label), ..Found::new(measured, points, on.exact) })
    }

    /// The included angle of the cone `face` lies on, between its two silhouettes on the view through
    /// `basis`: both must lie on the body's faces on that cone ([`ray_on`]), so half a cone is
    /// refused. Its points are each silhouette's piece on them, from the end nearer the apex, as an
    /// angle's two edges are.
    fn cone(&mut self, face: &ModelFace, basis: &Plane) -> std::result::Result<Found, Status> {
        let (on, cone) = self.on_surface(face, "cone", |c| if let Curved::Cone(c) = c { Some(*c) } else { None })?;
        let state = Arc::clone(&self.state);
        let b = state.bodies.get(on.body).ok_or_else(|| Status::lost("its body no longer exists"))?;
        // The silhouettes are read in the body's own frame, where its faces and edges are.
        let flattens = || Status::lost("its occurrence flattens it");
        let to_body = mat_inverse(&on.frame).ok_or_else(flattens)?;
        let seen = Plane::new(cone.apex, apply_vector(&to_body, basis.x), apply_vector(&to_body, basis.y)).ok_or_else(flattens)?;
        let (sides, foreshortened) = cone.silhouettes(&seen).map_err(|e| Status::Unmeasurable(e.0.to_string()))?;
        let bounds = b.body.exact_bounds().map_err(|e| Status::Unmeasurable(e.to_string()))?;
        // Every point of the body is within this of the apex.
        let reach = cone.apex.dist(bounds.center()) + bounds.size().len() / 2.0;
        let rim = rim(b, &on.faces);
        let mut points = Vec::with_capacity(4);
        for way in sides {
            let (near, far) = ray_on(b, cone.apex, cone.axis, way, reach, &on.faces, &rim)?.ok_or_else(|| {
                Status::Unmeasurable(format!("face `{}` isn't a whole cone in this view: one of its silhouettes isn't on it", face.name))
            })?;
            points.extend([near, far].map(|t| apply_point(&on.frame, cone.apex + way * t)));
        }
        let measured = Measured { value: 2.0 * cone.half_angle, foreshortened };
        Ok(Found::new(measured, points, on.exact))
    }

    /// The diameter of the whole cylinder `face` lies on, between its two silhouettes on the view
    /// through `basis`: both must lie on the body's faces on that cylinder, so a fillet's quarter
    /// round is refused. Its points are the silhouettes' ends.
    fn cylinder(&mut self, face: &ModelFace, basis: &Plane) -> std::result::Result<Found, Status> {
        let on = self.on_cylinder(face)?;
        let side = measure::silhouette_side(basis, on.cylinder.axis).map_err(|e| Status::Unmeasurable(e.0.to_string()))?;
        if !(on.covers(side) && on.covers(-side)) {
            return Err(Status::Unmeasurable(format!(
                "face `{}` isn't a whole cylinder in this view: one of its silhouettes isn't on it (give an arc's radius)",
                face.name
            )));
        }
        let measured = Measured { value: 2.0 * on.cylinder.radius, foreshortened: false };
        Ok(Found::new(measured, [on.ends(side), on.ends(-side)].concat(), on.exact))
    }

    /// The distance between silhouettes `a` and `b` on the view through `basis`, each on its
    /// cylinder's faces. Its points are the two silhouettes' ends, `a`'s first.
    fn silhouettes(&mut self, a: &Silhouette, b: &Silhouette, direction: LinearDirection, basis: &Plane) -> std::result::Result<Found, Status> {
        let (on_a, on_b) = (self.on_cylinder(&a.face)?, self.on_cylinder(&b.face)?);
        let between = measure::between_silhouettes(basis, &on_a.cylinder, a.side, &on_b.cylinder, b.side, direction);
        let (measured, [way_a, way_b]) = between.map_err(|e| Status::Unmeasurable(e.0.to_string()))?;
        for (on, way, s) in [(&on_a, way_a, a), (&on_b, way_b, b)] {
            if !on.covers(way) {
                let side = if s.side == SilhouetteSide::Near { "near" } else { "far" };
                return Err(Status::Unmeasurable(format!("face `{}`'s {side} silhouette in this view isn't on it", s.face.name)));
            }
        }
        let points = [on_a.ends(way_a), on_b.ends(way_b)].concat();
        Ok(Found::new(measured, points, on_a.exact && on_b.exact))
    }

    /// The flat face `face` names, or the pieces it was cut into, all on one oriented plane. A face
    /// split on purpose (Split Face) is separate faces, so its pieces are listed and none is chosen.
    fn on_plane(&mut self, face: &ModelFace) -> std::result::Result<OnPlane, Status> {
        let (i, frame) = self.body(&face.body, face.origin_feature, &face.path)?;
        let state = Arc::clone(&self.state);
        let b = state.bodies.get(i).ok_or_else(|| Status::lost("its body no longer exists"))?;
        let (found, exact) = faces_named(b, &face.name);
        let Some(first) = found.first() else { return Err(self.lost_face(i, &face.name)) };
        let plane = b.body.planar_face(*first).ok_or_else(|| Status::Unmeasurable(format!("face `{}` isn't flat", face.name)))?;
        if self.doc.coplanar_pieces(b, &found, |_| true).is_err() {
            return Err(Status::Ambiguous(face_candidates(b, &found)));
        }
        let normal = apply_vector(&frame, plane.normal()).normalized().ok_or_else(|| Status::lost("its occurrence flattens it"))?;
        Ok(OnPlane { origin: apply_point(&frame, plane.origin), normal, corners: face_corners(b, &frame, &found), exact })
    }

    /// The distance between flat faces `a` and `b` on the view through `basis`. Its points are each
    /// face's ends on the view, `a`'s first.
    fn faces(&mut self, a: &ModelFace, b: &ModelFace, direction: LinearDirection, basis: &Plane) -> std::result::Result<Found, Status> {
        let (on_a, on_b) = (self.on_plane(a)?, self.on_plane(b)?);
        let between = measure::between_faces(basis, (on_a.origin, on_a.normal), (on_b.origin, on_b.normal), direction);
        let measured = between.map_err(|e| Status::Unmeasurable(e.0.to_string()))?;
        let ends = |on: &OnPlane, face: &ModelFace| on.ends(basis).ok_or_else(|| Status::Unmeasurable(format!("face `{}` has no mesh", face.name)));
        let points = [ends(&on_a, a)?, ends(&on_b, b)?].concat();
        Ok(Found::new(measured, points, on_a.exact && on_b.exact))
    }
}

/// How far `b`'s exact box, placed by `frame`, reaches along `axis`, a direction of the design's
/// frame (any length), by solidcore's `extent_along`. Tight when `axis` runs along the body's own axes.
fn body_extent(b: &ModelBody, frame: &Mat, axis: [f64; 3]) -> Result<f64> {
    let bounds = b.body.exact_bounds().map_err(|e| DocError::Invalid(e.to_string()))?;
    solidcraft_kernel::affine::extent_along(bounds.min.into(), bounds.max.into(), frame, axis).map_err(axis_refusal)
}

/// The refusal of one of solidcore's axis ops, in the words of a dimension on a view.
fn axis_refusal(why: solidcraft_kernel::affine::AxisRefusal) -> DocError {
    use solidcraft_kernel::affine::AxisRefusal;
    DocError::Invalid(
        match why {
            AxisRefusal::ZeroAxis => "the view's axis is zero",
            AxisRefusal::FlatMap => "the occurrence its body is placed through flattens it",
            AxisRefusal::NotFinite => "the body's box is not finite",
        }
        .into(),
    )
}

/// The faces of `b` that `name` finds: the ones with that name, else the pieces of it; and whether
/// it found one face by exact history.
fn faces_named(b: &ModelBody, name: &str) -> (Vec<usize>, bool) {
    let named: Vec<usize> = (0..b.body.face_count()).filter(|f| face_has_name(b, *f, name)).collect();
    match named.as_slice() {
        [f] => {
            let exact = face_name_exact(b, *f);
            (named, exact)
        }
        [] => (faces_with_stem(b, name), false),
        _ => (named, false),
    }
}

/// `faces` of `b`, as candidates.
fn face_candidates(b: &ModelBody, faces: &[usize]) -> Vec<Candidate> {
    faces.iter().take(MAX_CANDIDATES).map(|f| Candidate::Face { body: b.name.clone(), index: *f }).collect()
}

/// The cylinder, sphere or cone each face of `b` lies on, by face; `None` for a face of another
/// surface.
fn curved_of(b: &ModelBody) -> Vec<Option<Curved>> {
    (0..b.body.face_count())
        .map(|f| match face_surface(&b.body, f) {
            Some(FaceSurface::Cylinder { origin, axis, radius }) => Some(Curved::Cylinder(Cylinder { origin, axis: Vec3::from(axis), radius })),
            Some(FaceSurface::Sphere { center, radius }) => Some(Curved::Sphere(Sphere { center, radius })),
            Some(FaceSurface::Cone { apex, axis, half_angle }) => Some(Curved::Cone(Cone { apex, axis: Vec3::from(axis), half_angle })),
            _ => None,
        })
        .collect()
}

/// How near two surfaces of `b` must be to be one: the kernel's own tolerance for two faces on a
/// common cylinder, taken for a sphere's centre and radius too.
fn surface_tolerance(b: &ModelBody) -> f64 {
    (b.body.size() * 1e-7).max(1e-9) * 10.0
}

/// The edges where `faces` of `b` meet a face that isn't one of them, or meet none (a free edge),
/// each once. None when the faces close up on their own, as a whole sphere's do: its seam's and
/// its poles' uses are all its own. Read off the record's topology, so exact.
fn rim(b: &ModelBody, faces: &[usize]) -> Vec<usize> {
    let record = b.body.record();
    let edges = faces.iter().filter_map(|f| record.faces.get(*f)).flat_map(|f| &f.loops).flat_map(|l| match l {
        Loop::Edges { edges } => edges.as_slice(),
        Loop::Vertex { .. } => [].as_slice(),
    });
    let bounds =
        |e: &usize| record.edges.get(*e).is_some_and(|edge| edge.uses.len() < 2 || edge.uses.iter().any(|u| !faces.contains(&(u.face as usize))));
    let mut rim: Vec<usize> = edges.map(|u| u.edge as usize).filter(bounds).collect();
    rim.sort_unstable();
    rim.dedup();
    rim
}

/// The most rim edges [`contour_on`] reads a sphere's contour across: past it, the faces are too
/// broken up to read a radius off.
const MAX_RIM_EDGES: usize = 128;
/// Cuts closer than this round the contour (radians) are one.
const SAME_ANGLE: f64 = 1e-9;

/// A circle whose axis is within this (the sine of the angle) of square to the view is seen
/// edge-on: its ellipse is then under a millionth of its diameter wide, which the Dimension tool's
/// pick already reads as a line on the sheet.
const EDGE_ON: f64 = 1e-6;

/// The longest arc of `sphere`'s contour on the view `seen` (both in `b`'s own frame) that lies on
/// `faces` of `b`: its start angle and its sweep, or `None` when none does. The contour is cut where
/// it crosses the faces' rim `rim` (the kernel's `outline_cuts`: one call for every rim edge, each
/// cut exact on the surface across it). Between two cuts it is all on the faces or all off them, so
/// one point classification at its middle (exact, SMLib's) decides, and pieces on them next to each
/// other join.
fn contour_on(b: &ModelBody, seen: &Plane, sphere: &Sphere, faces: &[usize], rim: &[usize]) -> std::result::Result<Option<(f64, f64)>, Status> {
    if rim.len() > MAX_RIM_EDGES {
        return Err(Status::Unmeasurable(format!("the sphere's faces end at more than {MAX_RIM_EDGES} edges: give a radius another way")));
    }
    // The contour as a circle in its own axes about the centre, so a cut's parameter is the angle
    // `Sphere::contour_point` turns.
    let frame = Frame { origin: sphere.center.into(), x_axis: seen.x.into(), y_axis: seen.y.into() };
    let contour = Curve::Circle { frame, radius: sphere.radius };
    let crossed = b
        .body
        .outline_cuts(faces, &contour, [0.0, TAU], sphere.center, seen.normal())
        .map_err(|why| Status::Unmeasurable(format!("the sphere's faces end at an edge that doesn't read ({why}): give a radius another way")))?;
    let mut cuts: Vec<f64> = crossed.into_iter().map(|t| t.rem_euclid(TAU)).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= SAME_ANGLE);
    // Each cut to the next round, the last back to the first; no cut leaves the whole contour.
    let arcs: Vec<(f64, f64)> = match cuts.as_slice() {
        [] => vec![(0.0, TAU)],
        [one] => vec![(*one, TAU)],
        _ => cuts.iter().zip(cuts.iter().cycle().skip(1)).map(|(a, b)| (*a, (b - a).rem_euclid(TAU))).collect(),
    };
    let mut pieces = Vec::with_capacity(arcs.len());
    for (start, sweep) in arcs.into_iter().filter(|(_, sweep)| *sweep > SAME_ANGLE) {
        let class = b.body.classify_point(sphere.contour_point(seen, start + sweep / 2.0)).map_err(|e| Status::Unmeasurable(e.to_string()))?;
        pieces.push((start, sweep, class == PointClass::OnBoundary));
    }
    Ok(longest_run(&pieces, true))
}

/// The longest run of `pieces` (each a start, a length, and whether it lies on the faces) on the
/// faces, the pieces next to each other joined: its start and its length. `round`, the pieces go
/// once round a full turn, so runs are counted round from a piece off the faces and none is split
/// at the list's end; with none off, the faces hold the whole turn.
fn longest_run(pieces: &[(f64, f64, bool)], round: bool) -> Option<(f64, f64)> {
    let from = match pieces.iter().position(|(_, _, on)| !on) {
        Some(off) if round => off,
        None if round => return pieces.first().map(|(start, _, _)| (*start, TAU)),
        _ => 0,
    };
    let (mut longest, mut run): (Option<(f64, f64)>, Option<(f64, f64)>) = (None, None);
    for (start, length, on) in pieces[from..].iter().chain(&pieces[..from]) {
        run = if *on { Some(run.map_or((*start, *length), |(at, so_far)| (at, so_far + length))) } else { None };
        if let Some((_, so_far)) = run
            && longest.is_none_or(|(_, most)| so_far > most)
        {
            longest = run;
        }
    }
    longest
}

/// The longest piece of the ray from `apex` along the unit `way` (both in `b`'s own frame, `way`
/// a side of the cone along `axis`), out to `reach`, that lies on `faces` of `b`: how far from the
/// apex it starts and ends, or `None` when none does. Cut as [`contour_on`] cuts a sphere's
/// contour, where the ray crosses the faces' rim `rim`, with one point classification at each
/// piece's middle.
fn ray_on(
    b: &ModelBody,
    apex: Vec3,
    axis: Vec3,
    way: Vec3,
    reach: f64,
    faces: &[usize],
    rim: &[usize],
) -> std::result::Result<Option<(f64, f64)>, Status> {
    if rim.len() > MAX_RIM_EDGES {
        return Err(Status::Unmeasurable(format!("the cone's faces end at more than {MAX_RIM_EDGES} edges: give its angle another way")));
    }
    // The ray lies in the plane of the axis and the ray; as a line from the apex one unit along
    // the ray, a cut's parameter is its distance from the apex.
    let normal = way.cross(axis).normalized().ok_or_else(|| Status::Unmeasurable("the cone's silhouette runs along its axis".into()))?;
    let ray = Curve::Line { start: apex.into(), end: (apex + way).into() };
    let crossed = b
        .body
        .outline_cuts(faces, &ray, [0.0, reach], apex, normal)
        .map_err(|why| Status::Unmeasurable(format!("the cone's faces end at an edge that doesn't read ({why}): give its angle another way")))?;
    let mut cuts = vec![0.0, reach];
    cuts.extend(crossed.into_iter().filter(|t| *t > 0.0 && *t < reach));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= SAME_ANGLE * reach);
    let mut pieces = Vec::with_capacity(cuts.len());
    for pair in cuts.windows(2) {
        let (start, length) = (pair[0], pair[1] - pair[0]);
        let class = b.body.classify_point(apex + way * (start + length / 2.0)).map_err(|e| Status::Unmeasurable(e.to_string()))?;
        pieces.push((start, length, class == PointClass::OnBoundary));
    }
    Ok(longest_run(&pieces, false).map(|(start, length)| (start, start + length)))
}

/// What a dimension of `kind` reads, where the kind says: `None` for a chain, which shows its
/// members', and for a sphere, whose measure says (SØ or SR).
fn label_of(kind: &DimensionKind) -> Option<LabelKind> {
    match kind {
        DimensionKind::Angle { .. } | DimensionKind::Cone { .. } => Some(LabelKind::Angle),
        DimensionKind::Radial { diameter: false, .. } => Some(LabelKind::Radius),
        DimensionKind::Radial { diameter: true, .. } | DimensionKind::Cylinder { .. } => Some(LabelKind::Diameter),
        DimensionKind::Chain { .. } | DimensionKind::Baseline { .. } | DimensionKind::Ordinate { .. } | DimensionKind::Sphere { .. } => None,
        DimensionKind::Linear { .. } | DimensionKind::Length { .. } | DimensionKind::Silhouettes { .. } | DimensionKind::Faces { .. } => {
            Some(LabelKind::Length)
        }
    }
}

/// Two faces' unit directions are one surface's past this cosine: a cylinder's axes, a cone's axes
/// and, as the cosine of their difference, a cone's half-angles ([`Curved::same`]).
const SAME_WAY: f64 = 1.0 - 1e-9;

/// Whether two cylinders are one: parallel axes, on one line, and one radius, to `tol` (the
/// kernel's own condition for faces on a common cylinder).
fn same_cylinder(a: &Cylinder, b: &Cylinder, tol: f64) -> bool {
    let off = b.origin - a.origin;
    a.axis.dot(b.axis).abs() > SAME_WAY && (a.radius - b.radius).abs() < tol && (off - a.axis * off.dot(a.axis)).len() < tol
}

/// The corners of the triangles of `faces` of `b`'s mesh, in the design's frame through `frame`.
fn face_corners(b: &ModelBody, frame: &Mat, faces: &[usize]) -> Vec<[Vec3; 3]> {
    let mesh = b.mesh();
    let on = mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, f)| faces.contains(&(**f as usize)));
    on.filter_map(|(t, _)| mesh.tri(t)).map(|corners| corners.map(|p| apply_point(frame, p))).collect()
}

/// Whether a triangle whose corners lie at `angles` round a cylinder's axis (from the way a
/// silhouette is, each from −π to π) lies across the silhouette: its corners round it, to `slack`,
/// and less than half a turn apart (so not round the far side).
fn straddles(angles: [f64; 3], slack: f64) -> bool {
    let (low, high) = angles.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), a| (lo.min(*a), hi.max(*a)));
    low <= slack && high >= -slack && high - low < PI
}

/// One command's picks on the bodies placed through one occurrence path. Each body's edges are
/// placed in the design's frame once, and only the body picked has its edge names worked out.
struct Picker<'a> {
    doc: &'a Document,
    state: Arc<ModelState>,
    path: &'a [u64],
    /// Each body's edges in the design's frame, by its index in `state`; `None` for a mesh body or
    /// one the path doesn't place.
    placed: BTreeMap<usize, Option<Vec<Vec<Vec3>>>>,
}

impl<'a> Picker<'a> {
    fn new(doc: &'a Document, model: &Model, path: &'a [u64]) -> Picker<'a> {
        Picker { doc, state: model.state(), path, placed: BTreeMap::new() }
    }

    /// Body `i`'s edges in the design's frame, worked out on first use.
    fn placed(&mut self, i: usize) -> Option<&Vec<Vec<Vec3>>> {
        if !self.placed.contains_key(&i) {
            let b = self.state.bodies.get(i).filter(|b| !b.body.is_mesh());
            let frame = b.and_then(|b| self.doc.body_frame_at(b, self.path).ok());
            let lines =
                b.zip(frame).map(|(b, frame)| b.mesh().edges.iter().map(|line| line.iter().map(|p| apply_point(&frame, *p)).collect()).collect());
            self.placed.insert(i, lines);
        }
        self.placed.get(&i).and_then(Option::as_ref)
    }

    /// The edge `pick` names: its body's index and its own.
    fn edge(&mut self, pick: &EdgePick) -> Result<(usize, usize)> {
        let on: Vec<(usize, usize)> = match pick {
            EdgePick::Index { body, index } => {
                let b = self.named(body, "edge")?;
                let count = self.placed(b).map_or(0, Vec::len);
                require(*index < count, || format!("body `{body}` has no edge {index}"))?;
                vec![(b, *index)]
            }
            EdgePick::Point(p) => {
                // A point on a curve lies off its chords by up to what the body was meshed to, so
                // that is looked for only when no edge passes through the point itself.
                let (mut on, mut curved) = (Vec::new(), Vec::new());
                for i in 0..self.state.bodies.len() {
                    let meshed_to = self.state.bodies.get(i).map_or(0.0, |b| b.display_tol());
                    let Some(lines) = self.placed(i) else { continue };
                    for (e, line) in lines.iter().enumerate() {
                        let off = line.windows(2).fold(f64::INFINITY, |off, w| if let [a, b] = w { off.min(p.dist_to_segment(*a, *b)) } else { off });
                        if off <= point_tolerance(*p) {
                            on.push((i, e));
                        } else if off <= meshed_to {
                            curved.push((i, e));
                        }
                    }
                }
                if on.is_empty() { curved } else { on }
            }
        };
        match on.as_slice() {
            [one] => Ok(*one),
            [] => Err(DocError::Invalid(format!("no edge at {}", at(pick)))),
            several => {
                Err(DocError::Invalid(format!("{} is on {} edges: give a point inside one edge, or its body and index", at(pick), several.len())))
            }
        }
    }

    /// The one body called `body` that the path places here, for a pick of its `what` (edge or
    /// face) by index.
    fn named(&mut self, body: &str, what: &str) -> Result<usize> {
        let named: Vec<usize> = (0..self.state.bodies.len()).filter(|i| self.state.bodies.get(*i).is_some_and(|b| b.name == *body)).collect();
        let placed: Vec<usize> = named.into_iter().filter(|i| self.placed(*i).is_some()).collect();
        match placed.as_slice() {
            [b] => Ok(*b),
            [] => Err(DocError::Invalid(format!("no body `{body}` with edges is placed here"))),
            _ => Err(DocError::Invalid(format!("more than one body is called `{body}`: give a point on the {what}"))),
        }
    }

    /// The face `pick` names: its body's index and its own. A point picks the face it lies on, to
    /// its body's pick tolerance ([`face_index_at`]), on whichever placed body has one there.
    fn face(&mut self, pick: &EdgePick) -> Result<(usize, usize)> {
        let on: Vec<(usize, usize)> = match pick {
            EdgePick::Index { body, index } => {
                let b = self.named(body, "face")?;
                let count = self.state.bodies.get(b).map_or(0, |b| b.body.face_count());
                require(*index < count, || format!("body `{body}` has no face {index}"))?;
                vec![(b, *index)]
            }
            EdgePick::Point(p) => {
                let mut on = Vec::new();
                for (i, b) in self.state.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()) {
                    let back = self.doc.body_frame_at(b, self.path).ok().and_then(|frame| mat_inverse(&frame));
                    on.extend(back.and_then(|back| face_index_at(b, apply_point(&back, *p))).map(|f| (i, f)));
                }
                on
            }
        };
        match on.as_slice() {
            [one] => Ok(*one),
            [] => Err(DocError::Invalid(format!("no face at {}", at(pick)))),
            several => Err(DocError::Invalid(format!("{} is on {} faces: give its body and index", at(pick), several.len()))),
        }
    }

    /// The corner at `p`: the body whose edges end there, and those edges.
    fn corner_at(&mut self, p: Vec3) -> Result<(usize, Vec<usize>)> {
        let mut found: Vec<(usize, Vec<usize>)> = Vec::new();
        for i in 0..self.state.bodies.len() {
            let Some(lines) = self.placed(i) else { continue };
            let ends_here = |line: &Vec<Vec3>| [line.first(), line.last()].into_iter().flatten().any(|q| q.dist(p) <= point_tolerance(p));
            let edges: Vec<usize> = lines.iter().enumerate().filter(|(_, line)| ends_here(line)).map(|(e, _)| e).collect();
            if !edges.is_empty() {
                found.push((i, edges));
            }
        }
        let corner = at(&EdgePick::Point(p));
        match found.pop() {
            Some(_) if !found.is_empty() => Err(DocError::Invalid(format!("{corner} is a corner of more than one body: give its edges"))),
            Some((body, edges)) if (2..=MAX_VERTEX_EDGES).contains(&edges.len()) => Ok((body, edges)),
            Some((_, edges)) => {
                Err(DocError::Invalid(format!("{corner} is the end of {} edges: a corner is where 2 to {MAX_VERTEX_EDGES} edges meet", edges.len())))
            }
            None => Err(DocError::Invalid(format!("no corner at {corner}"))),
        }
    }

    /// Body `body`'s edge `edge` is a circle or an arc.
    fn round(&mut self, body: usize, edge: usize) -> Result<()> {
        let round = self.placed(body).and_then(|lines| lines.get(edge)).is_some_and(|line| as_circle(line).is_some());
        require(round, || format!("edge {edge} isn't a circle or an arc: a centre is a circle's or an arc's"))
    }

    /// Body `body`'s `edges` meet at one point.
    fn meeting(&mut self, body: usize, edges: &[usize]) -> Result<()> {
        let lines = self.placed(body).map(Vec::as_slice).unwrap_or_default();
        let ends: Vec<Vec<Vec3>> =
            edges.iter().filter_map(|e| lines.get(*e)).map(|line| [line.first(), line.last()].into_iter().flatten().copied().collect()).collect();
        match shared_ends(&ends).len() {
            1 => Ok(()),
            0 => Err(DocError::Invalid("the corner's edges don't meet".into())),
            n => Err(DocError::Invalid(format!("the corner's edges meet at {n} points: give the corner as a point"))),
        }
    }

    /// Body `body`'s `edges` as a reference names them: the body's name, the feature that made
    /// it, and each edge's persistent name.
    fn names(&self, body: usize, edges: &[usize]) -> Result<(String, u64, Vec<String>)> {
        let b = self.state.bodies.get(body).ok_or_else(|| DocError::Invalid("the body picked no longer exists".into()))?;
        let named = BodyEdges::new(b);
        let names = edges.iter().map(|e| named.name(*e).map(str::to_string).ok_or_else(|| DocError::Invalid(format!("edge {e} has no name"))));
        Ok((b.name.clone(), b.feature, names.collect::<Result<_>>()?))
    }
}

/// A pick, for a message.
fn at(pick: &EdgePick) -> String {
    match pick {
        EdgePick::Point(p) => format!("[{}, {}, {}]", p.x, p.y, p.z),
        EdgePick::Index { body, index } => format!("edge {index} of `{body}`"),
    }
}

/// The pieces a name found, as candidates.
pub(super) fn edge_candidates(body: &str, found: &Edges) -> Vec<Candidate> {
    found.pieces.iter().take(MAX_CANDIDATES).map(|(index, _)| Candidate::Edge { body: body.to_string(), index: *index }).collect()
}

/// The angle between two lines, from `between` (from one's direction to the other's, 0 to π): the
/// smaller one, whichever way the kernel runs each edge, or on its `other_side` π minus it.
fn between_lines(between: f64, other_side: bool) -> f64 {
    let smaller = between.min(PI - between);
    if other_side { PI - smaller } else { smaller }
}

/// One edge's arcs as one arc: its radius, and its centre, start, middle and end. The kernel gives a
/// whole circle as two halves. `None` unless every piece is an arc of one circle.
fn one_arc(segs: &[Seg2]) -> Option<(f64, [Vec2; 4])> {
    let &Seg2::Arc { center, radius, start, .. } = segs.first()? else { return None };
    let mut sweep = 0.0;
    for seg in segs {
        let &Seg2::Arc { center: c, radius: r, sweep: s, .. } = seg else { return None };
        if c.dist(center) > radius * 1e-9 || (r - radius).abs() > radius * 1e-9 {
            return None;
        }
        sweep += s;
    }
    let arc = Seg2::Arc { center, radius, start, sweep };
    Some((radius, [center, arc.start(), arc.mid(), arc.end()]))
}

/// The centre of the one circle every one of `pieces` lies on: `None` if they aren't on one circle.
/// The circle is read from all their points together ([`as_circle`], four at least), since a short
/// arc's three points are too few to read one from alone, and every point lies on it, in its plane.
/// Two points could be a chord, so a piece needs three.
fn one_circle(pieces: &[(usize, Vec<Vec3>)]) -> Option<Vec3> {
    if pieces.iter().any(|(_, line)| line.len() < 3) {
        return None;
    }
    let points: Vec<Vec3> = pieces.iter().flat_map(|(_, line)| line.iter().copied()).collect();
    let (centre, normal, radius) = as_circle(&points)?;
    let tol = radius * 1e-6;
    let on = |p: &Vec3| ((*p - centre).len() - radius).abs() <= tol && (*p - centre).dot(normal).abs() <= tol;
    points.iter().all(on).then_some(centre)
}

/// A straight polyline's ends.
fn straight(line: &[Vec3]) -> Option<[Vec3; 2]> {
    as_line(line)?;
    Some([*line.first()?, *line.last()?])
}

#[cfg(test)]
#[path = "dimension_tests.rs"]
mod tests;
