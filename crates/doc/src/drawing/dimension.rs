//! A drawing's dimensions (W19b, #4896): each the distance or angle between named parts of its
//! view's model, measured as the view shows it ([`solidcraft_geom::drawing_measure`]). A dimension
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
//! - **Cylinder** (W20b): a whole cylinder's diameter between its two silhouettes, on a view that
//!   doesn't look along its axis (a face, [`ModelFace`]).
//! - **Silhouettes** (W20b): between a silhouette of one cylinder and a silhouette of another, each
//!   named by its face and by whether it is the one nearer the other cylinder or the one farther,
//!   as the view shows them ([`Silhouette`]). Two silhouettes of one cylinder are its diameter.
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
//! [`MAX_CHAIN_POINTS`] points and shows its members' numbers, never a text instead; an occurrence
//! path is at most [`MAX_PLACEMENT_DEPTH`] ids, none 0; a true length is measured aligned; a
//! direction to measure along isn't zero, and is at most [`MAX_REAL`] each way, as every number a
//! drawing saves (a whole number past it reads, then saves as one the file's check refuses); the
//! offset and the text place are on paper ([`is_paper_coordinate`]); at most [`MAX_DECIMALS`]
//! decimals; an override text of 1 to [`MAX_TEXT_BYTES`] bytes; a last good value from 0 to
//! [`MAX_REAL`]; at most [`MAX_DIMENSIONS`] dimensions, listed by id.
//!
//! [`Document::resolve_dimensions`] finds what a dimension measures again, strictly: an edge by
//! its name, or the one edge between the faces it names, never the piece nearest an old pick; a
//! corner where all its named edges meet, never the nearer of two; a centre the centre of its one
//! named edge, which must be a circle or an arc; a cylinder the faces its named face is now, all
//! on one cylinder, with the silhouettes it measures on them. What it can't decide it says
//! ([`Status`]), and it never chooses. [`Document::edge_reference`],
//! [`Document::corner_reference`] and [`Document::face_reference`] turn what a command picks (a
//! point, or an edge's or a face's index) into those names, and refuse a pick that doesn't name
//! one thing.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcraft_geom::drawing_measure::{self, AngleFrom, Cylinder, LinearDirection, Measured, Reading, SilhouetteSide};
use solidcraft_geom::drawing_view::{Invalid, is_paper_coordinate};
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

use super::{Drawing, Units, add_by_id, in_id_order, require};
use crate::config::MAX_REFERENCE_NAME_BYTES;
use crate::construct::{MAX_VERTEX_EDGES, as_circle, as_line, point_tolerance, shared_ends};
use crate::display_units::length_number;
use crate::format::MAX_REAL;
use solidcraft_kernel::{FaceSurface, face_surface};

use crate::appearance::face_index_at;
use crate::name_lookup::{BodyEdges, EdgeMatch, face_has_name, faces_with_stem};
use crate::naming::{face_name_exact, face_names};
use crate::{
    DocError, Document, MAX_PLACEMENT_DEPTH, Mat, Model, ModelBody, ModelState, Result, apply_point, apply_vector, ids, map_plane, mat_inverse,
};

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
}

/// What a dimension measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionKind {
    /// The distance between two corners.
    Linear { a: ModelPoint, b: ModelPoint, direction: LinearDirection, reading: Reading },
    /// A straight edge's length: the distance between its ends.
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
    /// A whole cylinder's diameter between its two silhouettes, on a view that doesn't look along
    /// its axis: the cylinder a face of it lies on.
    Cylinder { face: ModelFace },
    /// The distance between a silhouette of one cylinder and a silhouette of another, as the view
    /// shows them.
    Silhouettes { a: Silhouette, b: Silhouette, direction: LinearDirection },
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
            DimensionKind::Cylinder { face } => check_face(face)?,
            DimensionKind::Silhouettes { a, b, direction } => {
                check_face(&a.face)?;
                check_face(&b.face)?;
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
        match &d.override_text {
            Some(text) => require(!text.is_empty() && text.len() <= MAX_TEXT_BYTES, || {
                format!("the text shown instead of its number is 1 to {MAX_TEXT_BYTES} bytes")
            }),
            None => Ok(()),
        }
    }
}

/// A chain's number of points: 3 (2 make a linear dimension) to [`MAX_CHAIN_POINTS`].
pub fn check_chain_length(points: usize) -> Result<()> {
    require((3..=MAX_CHAIN_POINTS).contains(&points), || format!("a chain has 3 to {MAX_CHAIN_POINTS} points"))
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

fn check_point(point: &ModelPoint) -> Result<()> {
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
/// measured aligned (as [`solidcraft_geom::drawing_measure::linear`] reads it), and a direction to
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
    /// units with its decimals (degrees for an angle, after R or Ø for a radius or a diameter).
    /// `None` with neither, and for a chain, which shows its members'.
    pub text: Option<String>,
    /// A chain's members, from its first point to its last, each as it reads and its text: empty
    /// unless measured, and for other kinds. `measured` is then the chain's overall.
    pub members: Vec<Member>,
    /// Every name it measures was found by exact history, each as one edge.
    pub exact: bool,
    /// What it measures, in the design's frame: a linear dimension's two points, an angle's two
    /// lines (four points, each line from its first to its second), a radial dimension's
    /// centre and its arc's start, middle and end, a chain's points, or the ends of each silhouette
    /// measured (a cylinder's two, from its faces' one end along its axis to the other). Empty
    /// unless measured.
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
    /// The feature that made its body is suppressed or rolled back.
    Suppressed,
    /// What it measures is gone: why.
    Lost(String),
    /// Found, but the view can't measure it so: why (such as an angle to a line the view sees
    /// end-on).
    Unmeasurable(String),
}

/// One of the things an ambiguous name finds.
#[derive(Clone, Debug, PartialEq)]
pub enum Candidate {
    /// An edge of the body, by its index (as a `{body, index}` reference names it).
    Edge { body: String, index: usize },
    /// A point where the named edges meet, in the design's frame.
    Corner(Vec3),
    /// A face of the body, by its index (as a `{body, index}` face pick names it).
    Face { body: String, index: usize },
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

    /// The dimensions `wanted` of `drawing` (one of this design's drawings) as `model` (this
    /// design's) has them now: what each measures found again by name, measured on its view, and
    /// the text it shows. A body is looked up and its edge names worked out once for the call,
    /// however many dimensions name it; a body none names isn't touched. `Err` only for an id the
    /// drawing doesn't have, or a damaged record.
    pub fn resolve_dimensions(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<Resolved>> {
        let units = match &drawing.units {
            Units::Fixed { unit } => unit.as_str(),
            Units::FollowModel => self.units.as_str(),
        };
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
    /// Each body a cylinder dimension names: the cylinder each face lies on (its axis a unit
    /// vector, in the body's own frame), by face.
    cylinders: BTreeMap<usize, Vec<Option<Cylinder>>>,
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

/// What a dimension measures, found and measured: a chain's overall, and its members.
struct Found {
    measured: Measured,
    members: Vec<Measured>,
    points: Vec<Vec3>,
    exact: bool,
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

impl<'a> Lookup<'a> {
    pub(super) fn new(doc: &'a Document, model: &'a Model) -> Lookup<'a> {
        Lookup { doc, model, state: model.state(), named: BTreeMap::new(), cylinders: BTreeMap::new() }
    }

    fn resolve(&mut self, d: &Dimension, basis: &Plane, units: &str) -> Resolved {
        let (status, found) = match self.measure(d, basis) {
            Ok(found) => (if d.override_text.is_some() { Status::Overridden } else { Status::Ok }, Some(found)),
            Err(status) => (status, None),
        };
        let measured = found.as_ref().map(|f| f.measured);
        let length = |m: Measured| length_number(m.value, units, d.decimals);
        let number = |m: Measured| match d.kind {
            DimensionKind::Angle { .. } => Some(format!("{:.*}°", usize::from(d.decimals), m.value.to_degrees())),
            DimensionKind::Radial { diameter, .. } => {
                let symbol = if diameter { DIAMETER_SIGN } else { "R" };
                Some(format!("{symbol}{}", length(m)))
            }
            DimensionKind::Chain { .. } => None,
            DimensionKind::Cylinder { .. } => Some(format!("{DIAMETER_SIGN}{}", length(m))),
            _ => Some(length(m)),
        };
        let (members, points, exact) = found.map_or_else(Default::default, |f| (f.members, f.points, f.exact));
        Resolved {
            status,
            text: d.override_text.clone().or_else(|| measured.and_then(number)),
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
                let measured = drawing_measure::linear(basis, a, b, *direction, *reading).map_err(unmeasurable)?;
                Ok(Found { measured, members: Vec::new(), points: vec![a, b], exact: exact_a && exact_b })
            }
            DimensionKind::Length { edge, direction, reading } => {
                let found = self.edges(edge)?;
                // A split edge's length is no longer one number.
                if !found.whole {
                    return Err(Status::Ambiguous(edge_candidates(&edge.body, &found)));
                }
                let ends = found.pieces.first().and_then(|(_, line)| straight(line));
                let [a, b] = ends.ok_or_else(|| Status::Unmeasurable(format!("edge `{}` isn't straight", edge.name)))?;
                let measured = drawing_measure::linear(basis, a, b, *direction, *reading).map_err(unmeasurable)?;
                Ok(Found { measured, members: Vec::new(), points: vec![a, b], exact: found.exact })
            }
            DimensionKind::Angle { a, b, reading, other_side } => {
                let (a, exact_a) = self.line(a)?;
                let (b, exact_b) = self.line(b)?;
                let between = drawing_measure::angle(basis, (a[0], a[1]), AngleFrom::Edge(b[0], b[1]), *reading).map_err(unmeasurable)?;
                let value = between_lines(between.value, *other_side);
                let points = vec![a[0], a[1], b[0], b[1]];
                Ok(Found { measured: Measured { value, ..between }, members: Vec::new(), points, exact: exact_a && exact_b })
            }
            DimensionKind::Radial { edge, diameter } => {
                let found = self.edges(edge)?;
                // A split edge is several arcs, each drawn in its own place.
                if !found.whole {
                    return Err(Status::Ambiguous(edge_candidates(&edge.body, &found)));
                }
                let (radius, points) = self.arc(edge, &found, basis)?;
                let value = if *diameter { 2.0 * radius } else { radius };
                let measured = Measured { value, foreshortened: false };
                Ok(Found { measured, members: Vec::new(), points: points.to_vec(), exact: found.exact })
            }
            DimensionKind::Chain { points, direction } => {
                let found = points.iter().map(|p| self.corner(p)).collect::<std::result::Result<Vec<_>, _>>()?;
                let exact = found.iter().all(|(_, exact)| *exact);
                let points: Vec<Vec3> = found.into_iter().map(|(p, _)| p).collect();
                let chain = drawing_measure::chain(basis, &points, *direction).map_err(unmeasurable)?;
                Ok(Found { measured: chain.overall, members: chain.members, points, exact })
            }
            DimensionKind::Cylinder { face } => self.cylinder(face, basis),
            DimensionKind::Silhouettes { a, b, direction } => self.silhouettes(a, b, *direction, basis),
        }
    }

    /// The body a reference names, as the model has it now: its index, and the frame its geometry
    /// is in through the reference's occurrence path.
    fn body(&self, body: &str, feature: u64, path: &[u64]) -> std::result::Result<(usize, Mat), Status> {
        let found = self.state.bodies.iter().enumerate().find(|(_, b)| b.name == body && b.feature == feature && !b.body.is_mesh());
        let Some((i, b)) = found else {
            if self.model.results.iter().any(|r| r.id == feature && r.skipped) {
                return Err(Status::Suppressed);
            }
            let remade = self.state.bodies.iter().any(|b| b.name == body);
            return Err(Status::Lost(if remade {
                format!("body `{body}` is made by another feature now")
            } else {
                format!("body `{body}` no longer exists")
            }));
        };
        let frame = self.doc.body_frame_at(b, path).map_err(|_| Status::Lost("the occurrence its body is placed through no longer exists".into()))?;
        Ok((i, frame))
    }

    /// Body `i`'s edge names, worked out on first use.
    fn names(&mut self, i: usize) -> std::result::Result<&BodyEdges, Status> {
        let b = self.state.bodies.get(i).ok_or_else(|| Status::Lost("its body no longer exists".into()))?;
        Ok(self.named.entry(i).or_insert_with(|| BodyEdges::new(b)))
    }

    /// The edges `edge` names: the one edge, or the pieces of it.
    pub(super) fn edges(&mut self, edge: &ModelEdge) -> std::result::Result<Edges, Status> {
        let (i, frame) = self.body(&edge.body, edge.origin_feature, &edge.path)?;
        let names = self.names(i)?;
        let (indices, whole) = match names.matching(&edge.name) {
            Some(EdgeMatch::Edge(e)) => (vec![e], true),
            Some(EdgeMatch::Pieces(pieces)) => (pieces, false),
            None => return Err(Status::Lost(format!("edge `{}` no longer exists", edge.name))),
        };
        let exact = whole && indices.iter().all(|e| names.verified(*e));
        let placed = |e: &usize| names.mesh.edges.get(*e).map(|line| (*e, line.iter().map(|p| apply_point(&frame, *p)).collect()));
        Ok(Edges { pieces: indices.iter().filter_map(placed).collect(), whole, exact, body: i, frame })
    }

    /// The circle or arc that `found` (one whole edge) is, as the view through `basis` sees it face
    /// on: its radius, and its centre and its start, middle and end in the design's frame. Read
    /// exactly from the body, in the view's plane through the edge, so the points are the edge's.
    fn arc(&self, edge: &ModelEdge, found: &Edges, basis: &Plane) -> std::result::Result<(f64, [Vec3; 4]), Status> {
        let face_on = || Status::Unmeasurable(format!("edge `{}` isn't a circle in this view (it isn't parallel to it)", edge.name));
        let (index, line) = found.pieces.first().ok_or_else(face_on)?;
        let through = Plane { origin: *line.first().ok_or_else(face_on)?, ..*basis };
        let local = mat_inverse(&found.frame).and_then(|back| map_plane(&back, &through)).ok_or_else(face_on)?;
        let body = self.state.bodies.get(found.body).ok_or_else(|| Status::Lost("its body no longer exists".into()))?;
        let segs = body.body.planar_edges(&local, &[*index]).pop().flatten().ok_or_else(face_on)?;
        let not_round = || Status::Unmeasurable(format!("edge `{}` isn't a circle or an arc", edge.name));
        let (radius, points) = one_arc(&segs).ok_or_else(not_round)?;
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

    /// The point `point` names, in the design's frame: a corner, the one end all its named edges
    /// (or their pieces) share, or a centre.
    fn corner(&mut self, point: &ModelPoint) -> std::result::Result<(Vec3, bool), Status> {
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
                None => return Err(Status::Lost(format!("edge `{name}` no longer exists"))),
            };
            exact &= whole && indices.iter().all(|e| names.verified(*e));
            let lines = indices.iter().filter_map(|e| names.mesh.edges.get(*e));
            ends.push(lines.flat_map(|line| [line.first(), line.last()]).flatten().copied().collect());
        }
        match shared_ends(&ends).as_slice() {
            [] => Err(Status::Lost(format!("edges `{}` no longer meet", point.edges.join("`, `")))),
            [p] => Ok((apply_point(&frame, *p), exact)),
            several => Err(Status::Ambiguous(several.iter().take(MAX_CANDIDATES).map(|p| Candidate::Corner(apply_point(&frame, *p))).collect())),
        }
    }

    /// The centre `point` names: of the circle or arc its one named edge is, whole, as construction
    /// geometry finds a round edge's ([`as_circle`]).
    fn centre(&mut self, point: &ModelPoint) -> std::result::Result<(Vec3, bool), Status> {
        let name = point.edges.first().ok_or_else(|| Status::Lost("its centre names no edge".into()))?;
        let edge = ModelEdge { body: point.body.clone(), origin_feature: point.origin_feature, name: name.clone(), path: point.path.clone() };
        let found = self.edges(&edge)?;
        // A split edge is several arcs.
        if !found.whole {
            return Err(Status::Ambiguous(edge_candidates(&edge.body, &found)));
        }
        let round = found.pieces.first().and_then(|(_, line)| as_circle(line));
        let (centre, ..) = round.ok_or_else(|| Status::Unmeasurable(format!("edge `{name}` isn't a circle or an arc")))?;
        Ok((centre, found.exact))
    }

    /// The cylinder `face` lies on (or the pieces it was cut into, all on one cylinder), with every
    /// face of the body on that cylinder.
    fn on_cylinder(&mut self, face: &ModelFace) -> std::result::Result<OnCylinder, Status> {
        let (i, frame) = self.body(&face.body, face.origin_feature, &face.path)?;
        let state = Arc::clone(&self.state);
        let b = state.bodies.get(i).ok_or_else(|| Status::Lost("its body no longer exists".into()))?;
        let named: Vec<usize> = (0..b.body.face_count()).filter(|f| face_has_name(b, *f, &face.name)).collect();
        let (found, exact) = match named.as_slice() {
            [f] => (named.clone(), face_name_exact(b, *f)),
            [] => (faces_with_stem(b, &face.name), false),
            _ => (named.clone(), false),
        };
        let candidates = || found.iter().take(MAX_CANDIDATES).map(|f| Candidate::Face { body: face.body.clone(), index: *f }).collect();
        let cylinders = self.cylinders.entry(i).or_insert_with(|| cylinders_of(b));
        let on = |f: &usize| cylinders.get(*f).copied().flatten();
        let first = match found.first() {
            None => return Err(Status::Lost(format!("face `{}` no longer exists", face.name))),
            Some(f) => on(f).ok_or_else(|| Status::Unmeasurable(format!("face `{}` isn't a cylinder", face.name)))?,
        };
        let tol = cylinder_tolerance(b);
        if !found.iter().all(|f| on(f).is_some_and(|c| same_cylinder(&first, &c, tol))) {
            return Err(Status::Ambiguous(candidates()));
        }
        let faces: Vec<usize> = (0..cylinders.len()).filter(|f| on(f).is_some_and(|c| same_cylinder(&first, &c, tol))).collect();
        let axis = apply_vector(&frame, first.axis).normalized().ok_or_else(|| Status::Lost("its occurrence flattens it".into()))?;
        let cylinder = Cylinder { origin: apply_point(&frame, first.origin), axis, radius: first.radius };
        Ok(OnCylinder { cylinder, body: i, corners: face_corners(b, &frame, &faces), tol, exact })
    }

    /// The diameter of the whole cylinder `face` lies on, between its two silhouettes on the view
    /// through `basis`: both must lie on the body's faces on that cylinder, so a fillet's quarter
    /// round is refused. Its points are the silhouettes' ends.
    fn cylinder(&mut self, face: &ModelFace, basis: &Plane) -> std::result::Result<Found, Status> {
        let on = self.on_cylinder(face)?;
        let side = drawing_measure::silhouette_side(basis, on.cylinder.axis).map_err(|e| Status::Unmeasurable(e.0.to_string()))?;
        if !(on.covers(side) && on.covers(-side)) {
            return Err(Status::Unmeasurable(format!(
                "face `{}` isn't a whole cylinder in this view: one of its silhouettes isn't on it (give an arc's radius)",
                face.name
            )));
        }
        let measured = Measured { value: 2.0 * on.cylinder.radius, foreshortened: false };
        Ok(Found { measured, members: Vec::new(), points: [on.ends(side), on.ends(-side)].concat(), exact: on.exact })
    }

    /// The distance between silhouettes `a` and `b` on the view through `basis`, each on its
    /// cylinder's faces. Its points are the two silhouettes' ends, `a`'s first.
    fn silhouettes(&mut self, a: &Silhouette, b: &Silhouette, direction: LinearDirection, basis: &Plane) -> std::result::Result<Found, Status> {
        let (on_a, on_b) = (self.on_cylinder(&a.face)?, self.on_cylinder(&b.face)?);
        let between = drawing_measure::between_silhouettes(basis, &on_a.cylinder, a.side, &on_b.cylinder, b.side, direction);
        let (measured, [way_a, way_b]) = between.map_err(|e| Status::Unmeasurable(e.0.to_string()))?;
        for (on, way, s) in [(&on_a, way_a, a), (&on_b, way_b, b)] {
            if !on.covers(way) {
                let side = if s.side == SilhouetteSide::Near { "near" } else { "far" };
                return Err(Status::Unmeasurable(format!("face `{}`'s {side} silhouette in this view isn't on it", s.face.name)));
            }
        }
        let points = [on_a.ends(way_a), on_b.ends(way_b)].concat();
        Ok(Found { measured, members: Vec::new(), points, exact: on_a.exact && on_b.exact })
    }
}

/// The cylinder each face of `b` lies on, by face; `None` for a face of another surface.
fn cylinders_of(b: &ModelBody) -> Vec<Option<Cylinder>> {
    (0..b.body.face_count())
        .map(|f| match face_surface(&b.body, f) {
            Some(FaceSurface::Cylinder { origin, axis, radius }) => Some(Cylinder { origin, axis: Vec3::from(axis), radius }),
            _ => None,
        })
        .collect()
}

/// How near two cylinders of `b` must be to be one: the kernel's own tolerance for two faces on a
/// common cylinder.
fn cylinder_tolerance(b: &ModelBody) -> f64 {
    (b.body.size() * 1e-7).max(1e-9) * 10.0
}

/// Whether two cylinders are one: parallel axes, on one line, and one radius, to `tol` (the
/// kernel's own condition for faces on a common cylinder).
fn same_cylinder(a: &Cylinder, b: &Cylinder, tol: f64) -> bool {
    let off = b.origin - a.origin;
    a.axis.dot(b.axis).abs() > 1.0 - 1e-9 && (a.radius - b.radius).abs() < tol && (off - a.axis * off.dot(a.axis)).len() < tol
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

/// A straight polyline's ends.
fn straight(line: &[Vec3]) -> Option<[Vec3; 2]> {
    as_line(line)?;
    Some([*line.first()?, *line.last()?])
}

#[cfg(test)]
#[path = "dimension_tests.rs"]
mod tests;
