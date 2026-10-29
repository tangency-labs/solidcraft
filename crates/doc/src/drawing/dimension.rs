//! A drawing's dimensions (W19b, #4896): each the distance or angle between named parts of its
//! view's model, measured as the view shows it ([`solidcraft_geom::drawing_measure`]). A dimension
//! saves what it measures by name, never a picked point, so when the model changes it follows the
//! model or says what it lost: it never moves to other geometry. Nothing worked out is saved: its
//! value, its status and its last good value come from the model each time.
//!
//! - **Linear**: between two corners ([`ModelPoint`]), horizontal, vertical or aligned as the view
//!   shows them, or along a direction on the view.
//! - **Length**: a straight edge's ends ([`ModelEdge`]).
//! - **Angle**: between two straight edges, the smaller angle between their lines, or on its other
//!   side π minus it, whichever way the kernel runs the edges.
//! - **Radial** (W20a): a circle's or an arc's radius, or its diameter, on a view that sees it face
//!   on, read exactly from the body.
//!
//! A reference names its body, the feature that made the body (so a suppressed feature can be told
//! from a deleted one) and its occurrence path (so a part placed twice has a dimension on each copy).
//! The dimension line sits `offset_mm` of paper from what it measures, and the text at a place on
//! paper from its view's centre, so it moves with the view and not with the model; or centred on
//! the dimension line.
//!
//! The rules, beside the record's: a dimension is on one of the drawing's views; a reference names
//! a body and its feature, and 1 to [`MAX_REFERENCE_NAME_BYTES`] bytes in each name, without control
//! characters; a corner names 2 to [`MAX_VERTEX_EDGES`] edges; an occurrence path is at most
//! [`MAX_PLACEMENT_DEPTH`] ids, none 0; a true length is measured aligned; a direction to measure
//! along isn't zero, and is at most [`MAX_REAL`] each way, as every number a drawing saves (a whole
//! number past it reads, then saves as one the file's check refuses); the offset and the text place
//! are on paper ([`is_paper_coordinate`]); at most [`MAX_DECIMALS`] decimals; an override text of 1
//! to [`MAX_TEXT_BYTES`] bytes; at most [`MAX_DIMENSIONS`] dimensions, listed by id.
//!
//! [`Document::resolve_dimensions`] finds what a dimension measures again, strictly: an edge by
//! its name, or the one edge between the faces it names, never the piece nearest an old pick; a
//! corner where all its named edges meet, never the nearer of two. What it can't decide it says
//! ([`Status`]), and it never chooses. [`Document::edge_reference`] and
//! [`Document::corner_reference`] turn what a command picks (a point, or an edge's index) into
//! those names, and refuse a pick that doesn't name one thing.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcraft_geom::drawing_measure::{self, AngleFrom, LinearDirection, Measured, Reading};
use solidcraft_geom::drawing_view::{Invalid, is_paper_coordinate};
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

use super::{Drawing, Units, add_by_id, in_id_order, require};
use crate::config::MAX_REFERENCE_NAME_BYTES;
use crate::construct::{MAX_VERTEX_EDGES, as_line, point_tolerance, shared_ends};
use crate::display_units::length_number;
use crate::format::MAX_REAL;
use crate::name_lookup::{BodyEdges, EdgeMatch};
use crate::{DocError, Document, MAX_PLACEMENT_DEPTH, Mat, Model, ModelState, Result, apply_point, ids, map_plane, mat_inverse};

/// The most dimensions a drawing may have.
pub const MAX_DIMENSIONS: usize = 10_000;
/// The most decimals a dimension shows.
pub const MAX_DECIMALS: u8 = 6;
/// The most candidates an ambiguous dimension lists.
pub const MAX_CANDIDATES: usize = 16;
/// The longest text shown instead of a dimension's number, in bytes.
pub const MAX_TEXT_BYTES: usize = 1024;
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

/// A corner of a body: the end its named edges share.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPoint {
    pub body: String,
    /// The feature that made the body.
    pub origin_feature: u64,
    /// The persistent names of the edges that meet there.
    pub edges: Vec<String>,
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
        }
        let off_paper = || format!("its offset and its text's place are at most {MAX_SIDE_MM} mm on paper");
        let text = match d.text {
            TextPlace::At([x, y]) => is_paper_coordinate(x) && is_paper_coordinate(y),
            TextPlace::Centred => true,
        };
        require(is_paper_coordinate(d.offset_mm) && text, off_paper)?;
        require(d.decimals <= MAX_DECIMALS, || format!("it shows at most {MAX_DECIMALS} decimals"))?;
        match &d.override_text {
            Some(text) => require(!text.is_empty() && text.len() <= MAX_TEXT_BYTES, || {
                format!("the text shown instead of its number is 1 to {MAX_TEXT_BYTES} bytes")
            }),
            None => Ok(()),
        }
    }
}

/// An edge reference's rules (a balloon's edge keeps them too).
pub(super) fn check_edge(edge: &ModelEdge) -> Result<()> {
    check_body(&edge.body, edge.origin_feature, &edge.path)?;
    check_reference_name(&edge.name)
}

/// A corner reference's rules.
fn check_point(point: &ModelPoint) -> Result<()> {
    check_body(&point.body, point.origin_feature, &point.path)?;
    require((2..=MAX_VERTEX_EDGES).contains(&point.edges.len()), || format!("a corner is where 2 to {MAX_VERTEX_EDGES} named edges meet"))?;
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
    /// `None` with neither.
    pub text: Option<String>,
    /// Every name it measures was found by exact history, each as one edge.
    pub exact: bool,
    /// What it measures, in the design's frame: a linear dimension's two points, an angle's two
    /// lines (four points, each line from its first to its second), or a radial dimension's
    /// centre and its arc's start, middle and end. Empty unless measured.
    pub points: Vec<Vec3>,
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
}

/// How a command names an edge to measure, before it is saved by name
/// ([`Document::edge_reference`]).
#[derive(Clone, Debug, PartialEq)]
pub enum EdgePick {
    /// A point on it, in the design's frame.
    Point(Vec3),
    /// Its index among its body's edges as the model lists them: a [`Candidate::Edge`]'s.
    Index { body: String, index: usize },
}

/// How a command names a corner to measure ([`Document::corner_reference`]).
#[derive(Clone, Debug, PartialEq)]
pub enum CornerPick {
    /// The corner, in the design's frame.
    Point(Vec3),
    /// Edges that meet there.
    Edges(Vec<EdgePick>),
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

    /// The corner `pick` names, as [`Document::edge_reference`] names an edge: by the edges that
    /// meet there. A point must be the end of 2 to [`MAX_VERTEX_EDGES`] edges of one body, and
    /// edges given must meet at one point.
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
        };
        let (body, origin_feature, edges) = picker.names(body, &edges)?;
        Ok(ModelPoint { body, origin_feature, edges, path: path.to_vec() })
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
}

/// What a dimension measures, found and measured.
struct Found {
    measured: Measured,
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
        Lookup { doc, model, state: model.state(), named: BTreeMap::new() }
    }

    fn resolve(&mut self, d: &Dimension, basis: &Plane, units: &str) -> Resolved {
        let (status, found) = match self.measure(d, basis) {
            Ok(found) => (if d.override_text.is_some() { Status::Overridden } else { Status::Ok }, Some(found)),
            Err(status) => (status, None),
        };
        let measured = found.as_ref().map(|f| f.measured);
        let number = |m: Measured| match d.kind {
            DimensionKind::Angle { .. } => format!("{:.*}°", usize::from(d.decimals), m.value.to_degrees()),
            DimensionKind::Radial { diameter, .. } => {
                let symbol = if diameter { DIAMETER_SIGN } else { "R" };
                format!("{symbol}{}", length_number(m.value, units, d.decimals))
            }
            _ => length_number(m.value, units, d.decimals),
        };
        Resolved {
            status,
            text: d.override_text.clone().or_else(|| measured.map(number)),
            exact: found.as_ref().is_some_and(|f| f.exact),
            points: found.map_or_else(Vec::new, |f| f.points),
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
                Ok(Found { measured, points: vec![a, b], exact: exact_a && exact_b })
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
                Ok(Found { measured, points: vec![a, b], exact: found.exact })
            }
            DimensionKind::Angle { a, b, reading, other_side } => {
                let (a, exact_a) = self.line(a)?;
                let (b, exact_b) = self.line(b)?;
                let between = drawing_measure::angle(basis, (a[0], a[1]), AngleFrom::Edge(b[0], b[1]), *reading).map_err(unmeasurable)?;
                let value = between_lines(between.value, *other_side);
                Ok(Found { measured: Measured { value, ..between }, points: vec![a[0], a[1], b[0], b[1]], exact: exact_a && exact_b })
            }
            DimensionKind::Radial { edge, diameter } => {
                let found = self.edges(edge)?;
                // A split edge is several arcs, each drawn in its own place.
                if !found.whole {
                    return Err(Status::Ambiguous(edge_candidates(&edge.body, &found)));
                }
                let (radius, points) = self.arc(edge, &found, basis)?;
                let value = if *diameter { 2.0 * radius } else { radius };
                Ok(Found { measured: Measured { value, foreshortened: false }, points: points.to_vec(), exact: found.exact })
            }
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

    /// The corner `point` names: the one end all its named edges (or their pieces) share, in the
    /// design's frame.
    fn corner(&mut self, point: &ModelPoint) -> std::result::Result<(Vec3, bool), Status> {
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
                let named: Vec<usize> = (0..self.state.bodies.len()).filter(|i| self.state.bodies.get(*i).is_some_and(|b| b.name == *body)).collect();
                let placed: Vec<usize> = named.into_iter().filter(|i| self.placed(*i).is_some()).collect();
                let b = match placed.as_slice() {
                    [b] => *b,
                    [] => return Err(DocError::Invalid(format!("no body `{body}` with edges is placed here"))),
                    _ => return Err(DocError::Invalid(format!("more than one body is called `{body}`: give a point on the edge"))),
                };
                let count = self.placed(b).map_or(0, Vec::len);
                require(*index < count, || format!("body `{body}` has no edge {index}"))?;
                vec![(b, *index)]
            }
            EdgePick::Point(p) => {
                let mut on = Vec::new();
                for i in 0..self.state.bodies.len() {
                    let Some(lines) = self.placed(i) else { continue };
                    let near = |line: &Vec<Vec3>| line.windows(2).any(|w| matches!(w, [a, b] if p.dist_to_segment(*a, *b) <= point_tolerance(*p)));
                    on.extend(lines.iter().enumerate().filter(|(_, line)| near(line)).map(|(e, _)| (i, e)));
                }
                on
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
