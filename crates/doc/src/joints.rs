//! Joints between occurrences: joint origins (snap frames on component geometry, re-found on
//! every solve so they follow edits), joints (rigid, revolute, slider, cylindrical, pin-slot,
//! planar, ball, and relations that tie only part of a placement, `joints_relations.rs`) with
//! their motion values and limits, motion links (gears, racks and screws are
//! links whose ratio comes from radii or a pitch; a cam's follower follows a table worked out from
//! its profile, `joints_cam.rs`), and the solver that places occurrences from the joint graph.
//!
//! A joint makes B's joint frame coincide with A's frame moved by the joint's motion:
//! `T_B · F_b = T_A · F_a · M(q) · Flip · Tail(q)`, so `T_B = T_A · F_a · M(q) · Flip · Tail(q) · F_b⁻¹`
//! (`Tail` is a relation's tilt and turns after the mate; the identity for the others), where `T`
//! are occurrence world transforms, `F` the joint frames in each component's own frame, and `q`
//! the joint's values (angles in radians, distances in mm). The solver walks the graph from
//! grounded occurrences (and the root, which never moves). Joints that close a loop (a four-bar
//! linkage, a slider-crank) are met by moving the values of the other joints around the loop;
//! those that still disagree are reported.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb3, Vec3};

use solidcraft_sketch::linalg;

use crate::construct::ConstructGeom;
use crate::{DocError, Document, IDENTITY, Mat, ModelState, PlaneRef, Result, apply_point, apply_vector, mat_inverse, mat_mul};

#[path = "joints_handles.rs"]
mod handles;
pub use handles::{MotionAxis, motion_axes};

/// Joints the solver (and the URDF export) reads at most.
pub const MAX_JOINTS: usize = 10_000;

/// A reference to an axis for a joint origin (component coordinates): an origin axis (X, Y, Z),
/// a named construction axis, or an axis from model geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AxisRef {
    Target(AxisTarget),
    /// Name shorthand: "X", "Y", "Z" for origin axes, or a construction axis feature name.
    Name(String),
}

/// A structured reference to an axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AxisTarget {
    /// An origin axis: "X", "Y", or "Z".
    Origin { axis: String },
    /// A construction axis feature, by name.
    Construction { name: String },
    /// An axis from model or sketch geometry.
    Geo(crate::construct::GeoRef),
}

impl AxisRef {
    pub fn origin(axis: impl Into<String>) -> Self {
        AxisRef::Target(AxisTarget::Origin { axis: axis.into() })
    }

    pub fn construction(name: impl Into<String>) -> Self {
        AxisRef::Target(AxisTarget::Construction { name: name.into() })
    }

    pub fn geo(geo: crate::construct::GeoRef) -> Self {
        AxisRef::Target(AxisTarget::Geo(geo))
    }
}

/// Where a joint origin sits on a component's geometry (component coordinates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Snap {
    /// An explicit frame (origin, z axis, x axis).
    Frame {
        origin: Vec3,
        z: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<Vec3>,
    },
    /// The centre of the planar face nearest `pick`, z along its outward normal. `face` is the
    /// face's persistent name: when it names a planar face, that face's own plane and centre are
    /// taken (the pick only tells the pieces of a split face apart).
    FaceCenter {
        pick: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
        /// What turns the frame about z (default: any direction square to z).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<XRef>,
    },
    /// The centre of a circular end of the cylindrical face at `pick`, z along its axis
    /// (pointing out of the face's end). `face` is the face's persistent name: when it names a
    /// cylindrical face, that face's own axis, radius and extent are taken. `end` is the name of
    /// the face across the rim at the chosen end (the plate's face a hole opens onto), so the
    /// end stays the same when the design changes; without it (or when neither end has that
    /// face any more), the end nearer `pick` along the axis.
    CircleCenter {
        pick: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<String>,
        /// What turns the frame about z (default: any direction square to z).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<XRef>,
    },
    /// A point on an edge of a planar face: the edge's middle, or the corner where it meets the
    /// edge `corner` names (`joints_edge.rs`). `edge` and `face` are the persistent names of the
    /// edge and the face (found first); `pick` is a point on the edge as it was picked.
    Edge {
        pick: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edge: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        corner: Option<String>,
        /// The frame's z when it isn't the face's normal (an edge or a corner picked as it is,
        /// not on a face): the point still follows the edge, found by name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<EdgeAxis>,
    },
    /// A point, z along `z` (default +Z) and x toward `x` laid square to z (default: one chosen
    /// from z alone).
    Point {
        point: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<Vec3>,
    },
    /// A plane (origin plane, construction plane, or any plane definition): z along its normal,
    /// x along the plane's x axis (or `x` if given). Origin at `pick` projected onto the plane,
    /// or the plane's own origin when `pick` is omitted.
    Plane {
        plane: Box<PlaneRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pick: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<XRef>,
    },
    /// An axis (origin axis "X"|"Y"|"Z", construction axis, or model edge/axis): z along the
    /// axis, x square to z (or `x` if given). Origin at `pick` projected onto the axis line,
    /// or the axis origin when `pick` is None.
    Axis {
        axis: Box<AxisRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pick: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<XRef>,
    },
}

/// The z of an edge origin that isn't its face's normal (component coordinates); its x follows
/// from z alone, as a point origin's does.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeAxis {
    /// Along a straight edge, the way this points: a hinge on the edge turns about it as it moves.
    Along(Vec3),
    /// This direction.
    Fixed(Vec3),
}

impl EdgeAxis {
    pub fn dir(self) -> Vec3 {
        match self {
            EdgeAxis::Along(d) | EdgeAxis::Fixed(d) => d,
        }
    }
}

/// What sets a joint origin's x axis (its turn about z): a direction, or a face found by its
/// persistent name, so a round origin keeps its turn when the design changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum XRef {
    /// A direction (component coordinates); its part square to z is taken.
    Dir(Vec3),
    /// A face: a plane's outward normal, a cylinder's way from the origin to its axis (another
    /// hole), any other face's way from the origin to the point of it nearest `pick` (component
    /// coordinates; it also tells the pieces of a split face apart, default the origin).
    Face {
        face: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pick: Option<Vec3>,
    },
}

impl Snap {
    /// The persistent face and edge names the snap refers to (its face, its end's, its x reference's;
    /// an edge origin's edge, face and corner edge).
    pub fn face_names(&self) -> Vec<&String> {
        let mut out = Vec::new();
        if let Snap::FaceCenter { face, x, .. } | Snap::CircleCenter { face, x, .. } = self {
            out.extend(face.iter());
            if let Some(XRef::Face { face, .. }) = x {
                out.push(face);
            }
        }
        if let Snap::CircleCenter { end: Some(end), .. } = self {
            out.push(end);
        }
        if let Snap::Edge { edge, face, corner, .. } = self {
            out.extend(edge.iter().chain(face.iter()).chain(corner.iter()));
        }
        if let Snap::Plane { x: Some(XRef::Face { face, .. }), .. } | Snap::Axis { x: Some(XRef::Face { face, .. }), .. } = self {
            out.push(face);
        }
        out
    }

    /// [`Snap::face_names`], to rewrite them (a copied component's faces have new names).
    pub fn face_names_mut(&mut self) -> Vec<&mut String> {
        let mut out = Vec::new();
        match self {
            Snap::FaceCenter { face, x, .. } => {
                out.extend(face.iter_mut());
                if let Some(XRef::Face { face, .. }) = x {
                    out.push(face);
                }
            }
            Snap::CircleCenter { face, end, x, .. } => {
                out.extend(face.iter_mut());
                out.extend(end.iter_mut());
                if let Some(XRef::Face { face, .. }) = x {
                    out.push(face);
                }
            }
            Snap::Edge { edge, face, corner, .. } => out.extend(edge.iter_mut().chain(face.iter_mut()).chain(corner.iter_mut())),
            Snap::Plane { x: Some(XRef::Face { face, .. }), .. } | Snap::Axis { x: Some(XRef::Face { face, .. }), .. } => {
                out.push(face);
            }
            _ => {}
        }
        out
    }

    /// The direction a face, circle, plane, or axis snap keeps as its frame's x (`XRef::Dir`), in the
    /// component's coordinates; `None` for a face reference or none.
    pub fn frame_x(&self) -> Option<Vec3> {
        match self {
            Snap::FaceCenter { x: Some(XRef::Dir(d)), .. }
            | Snap::CircleCenter { x: Some(XRef::Dir(d)), .. }
            | Snap::Plane { x: Some(XRef::Dir(d)), .. }
            | Snap::Axis { x: Some(XRef::Dir(d)), .. } => Some(*d),
            _ => None,
        }
    }

    /// A face, circle, plane, or axis snap given no x reference keeps `x` (the frame's x as it is now): each
    /// solve lays it onto the face, or square to the axis, as the geometry is then, so a
    /// part that tilts keeps its turn about z.
    pub fn with_frame_x(mut self, x: Vec3) -> Snap {
        if let Snap::FaceCenter { x: slot @ None, .. }
        | Snap::CircleCenter { x: slot @ None, .. }
        | Snap::Plane { x: slot @ None, .. }
        | Snap::Axis { x: slot @ None, .. } = &mut self
        {
            *slot = Some(XRef::Dir(x));
        }
        self
    }

    /// Whether every point and direction in the snap is finite.
    pub fn is_finite(&self) -> bool {
        let x_ok = |x: &Option<XRef>| match x {
            Some(XRef::Dir(d)) => d.is_finite(),
            Some(XRef::Face { pick, .. }) => pick.is_none_or(|p| p.is_finite()),
            None => true,
        };
        match self {
            Snap::Frame { origin, z, x } => origin.is_finite() && z.is_finite() && x.is_none_or(|x| x.is_finite()),
            Snap::FaceCenter { pick, x, .. } | Snap::CircleCenter { pick, x, .. } => pick.is_finite() && x_ok(x),
            Snap::Point { point, z, x } => point.is_finite() && z.is_none_or(|z| z.is_finite()) && x.is_none_or(|x| x.is_finite()),
            Snap::Edge { pick, axis, .. } => pick.is_finite() && axis.is_none_or(|a| a.dir().is_finite()),
            Snap::Plane { pick, x, .. } => pick.is_none_or(|p| p.is_finite()) && x_ok(x),
            Snap::Axis { pick, x, .. } => pick.is_none_or(|p| p.is_finite()) && x_ok(x),
        }
    }
}

/// A joint origin: a snap on an occurrence's component (occurrence 0 = the root design).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointOrigin {
    pub occurrence: u64,
    pub snap: Snap,
}

/// A named joint origin (Joint Origin command), referenced by joints by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedOrigin {
    pub name: String,
    pub origin: JointOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    Rigid,
    Revolute,
    Slider,
    Cylindrical,
    PinSlot,
    Planar,
    Ball,
    /// B's z stays parallel to A's (against it unless flipped); B moves and turns about z freely.
    Parallel,
    /// B's z stays square to A's; B moves freely and turns about either z.
    Perpendicular,
    /// B's z stays at the joint's `angle` from mated (about their common x); B moves freely and
    /// turns about either z.
    Angle,
    /// B's origin stays on A's xy plane (`offset` off it); B slides on it and turns any way.
    PointOnPlane,
    /// B's origin stays on A's z axis; B slides along it and turns any way.
    PointOnLine,
    /// B's z axis lies in A's xy plane (`offset` off it: a cylinder's radius makes it tangent);
    /// B slides on the plane, turns about A's z and about its own axis.
    LineOnPlane,
}

impl JointKind {
    pub fn parse(s: &str) -> Option<JointKind> {
        Some(match s.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "rigid" => JointKind::Rigid,
            "revolute" | "rotate" => JointKind::Revolute,
            "slider" | "slide" => JointKind::Slider,
            "cylindrical" => JointKind::Cylindrical,
            "pinslot" => JointKind::PinSlot,
            "planar" => JointKind::Planar,
            "ball" => JointKind::Ball,
            "parallel" => JointKind::Parallel,
            "perpendicular" | "square" => JointKind::Perpendicular,
            "angle" => JointKind::Angle,
            "pointonplane" | "pointonface" => JointKind::PointOnPlane,
            "pointonline" | "pointonaxis" => JointKind::PointOnLine,
            "lineonplane" | "lineonface" => JointKind::LineOnPlane,
            _ => return None,
        })
    }
    /// Names of the joint's values, in order (angles in radians, distances in mm).
    pub fn dofs(self) -> &'static [&'static str] {
        match self {
            JointKind::Rigid => &[],
            JointKind::Revolute => &["angle"],
            JointKind::Slider => &["distance"],
            JointKind::Cylindrical => &["angle", "distance"],
            JointKind::PinSlot => &["angle", "slide"],
            JointKind::Planar => &["x", "y", "angle"],
            JointKind::Ball => &["yaw", "pitch", "roll"],
            JointKind::Parallel => &["x", "y", "z", "angle"],
            JointKind::Perpendicular | JointKind::Angle => &["x", "y", "z", "turn", "spin"],
            JointKind::PointOnPlane => &["x", "y", "yaw", "pitch", "roll"],
            JointKind::PointOnLine => &["slide", "yaw", "pitch", "roll"],
            JointKind::LineOnPlane => &["x", "y", "turn", "spin"],
        }
    }
    pub fn is_angle(self, i: usize) -> bool {
        matches!(self.dofs().get(i), Some(&("angle" | "yaw" | "pitch" | "roll" | "turn" | "spin")))
    }
    /// A relation that only ties some directions or points (parallel, on a plane…): made where
    /// the parts are, its values start as the nearest that meet it, so B moves no more than it
    /// must.
    pub fn is_relation(self) -> bool {
        matches!(
            self,
            JointKind::Parallel
                | JointKind::Perpendicular
                | JointKind::Angle
                | JointKind::PointOnPlane
                | JointKind::PointOnLine
                | JointKind::LineOnPlane
        )
    }
}

/// A joint between two occurrences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub id: u64,
    pub name: String,
    pub kind: JointKind,
    pub a: JointOrigin,
    pub b: JointOrigin,
    /// Current values of the joint's degrees of freedom.
    #[serde(default)]
    pub values: Vec<f64>,
    /// Per value: (min, max) limits.
    #[serde(default)]
    pub limits: Vec<Option<(f64, f64)>>,
    /// Don't mate: B's z along A's instead of against it (by default the origins mate, as two
    /// faces meeting).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Distance from A's frame to B's along z, and a turn about it (radians).
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// The expressions the offset, the angle and the values were typed as: they are worked out
    /// again whenever the design changes, so a joint follows the parameters it is defined by.
    #[serde(default, skip_serializing_if = "JointExprs::is_empty")]
    pub exprs: JointExprs,
}

/// Expressions a joint's numbers follow (over the design's parameters): the offset (a length),
/// the angle and each value (angles or lengths as the joint's values are). A number typed in
/// place of one drops it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JointExprs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// By value index, as in `Joint::values`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Option<String>>,
}

impl JointExprs {
    pub fn is_empty(&self) -> bool {
        self.offset.is_none() && self.angle.is_none() && self.values.iter().all(Option::is_none)
    }

    /// The expression value `i` follows.
    pub fn value(&self, i: usize) -> Option<&str> {
        self.values.get(i).and_then(Option::as_deref)
    }

    /// Value `i` follows `e` (or nothing).
    pub fn set_value(&mut self, i: usize, e: Option<String>) {
        if e.is_some() && self.values.len() <= i {
            self.values.resize(i + 1, None);
        }
        if let Some(slot) = self.values.get_mut(i) {
            *slot = e;
        }
        while self.values.last().is_some_and(Option::is_none) {
            self.values.pop();
        }
    }
}

/// Largest offset or value an expression may give a joint (mm, or radians for an angle).
const MAX_EXPR_VALUE: f64 = 1e9;

/// A design's parameter values, worked out the first time an expression names one and kept for
/// the next: a pass over the joints works out every joint's expressions again, and each would
/// otherwise evaluate every parameter of the design anew.
pub struct Params<'a> {
    doc: &'a Document,
    vals: std::cell::OnceCell<BTreeMap<String, crate::expr::Value>>,
}

impl<'a> Params<'a> {
    pub fn new(doc: &'a Document) -> Params<'a> {
        Params { doc, vals: std::cell::OnceCell::new() }
    }

    /// What [`Document::eval`] gives for `e`.
    pub fn eval(&self, e: &str, kind: crate::expr::Kind) -> Result<f64> {
        crate::expr::eval_with(e, &|n| {
            self.vals.get_or_init(|| self.doc.param_values().0).get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")))
        })?
        .to_kind(kind)
    }
}

/// A joint's offset, angle and values worked out from its expressions (those without one keep
/// their numbers), or why one can't be: an unknown name, a wrong unit, or a number too large.
pub fn expressed(params: &Params, j: &Joint) -> Result<(f64, f64, Vec<f64>)> {
    let eval = |e: &str, kind: crate::expr::Kind, what: &str| -> Result<f64> {
        let v = params.eval(e, kind).map_err(|err| DocError::Invalid(format!("joint {}: {what} `{e}`: {err}", j.name)))?;
        if v.is_finite() && v.abs() < MAX_EXPR_VALUE { Ok(v) } else { Err(DocError::Invalid(format!("joint {}: {what} `{e}` gives {v}", j.name))) }
    };
    let x = &j.exprs;
    let offset = match &x.offset {
        Some(e) => eval(e, crate::expr::Kind::Length, "offset")?,
        None => j.offset,
    };
    let angle = match &x.angle {
        Some(e) => eval(e, crate::expr::Kind::Angle, "angle")?,
        None => j.angle,
    };
    let mut values = j.values.clone();
    for (i, e) in x.values.iter().enumerate().take(j.kind.dofs().len()) {
        let Some(e) = e else { continue };
        let kind = if j.kind.is_angle(i) { crate::expr::Kind::Angle } else { crate::expr::Kind::Length };
        let name = j.kind.dofs().get(i).copied().unwrap_or("value");
        let v = eval(e, kind, name)?;
        if values.len() <= i {
            values.resize(i + 1, 0.0);
        }
        if let Some(slot) = values.get_mut(i) {
            *slot = v;
        }
    }
    Ok((offset, angle, values))
}

/// Values the loop solver leaves alone: those a motion link sets and those an expression does.
fn set_values(asm: &Assembly) -> BTreeSet<(u64, usize)> {
    let mut set: BTreeSet<(u64, usize)> = asm.links.iter().map(|l| (l.b, l.ib)).collect();
    for j in &asm.joints {
        set.extend(j.exprs.values.iter().enumerate().filter(|(_, e)| e.is_some()).map(|(i, _)| (j.id, i)));
    }
    set
}

/// Value of joint `b` (index `ib`) = ratio · value of joint `a` (index `ia`) + offset, or for a
/// cam, its `lift` table at a's value + offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionLink {
    pub a: u64,
    #[serde(default)]
    pub ia: usize,
    pub b: u64,
    #[serde(default)]
    pub ib: usize,
    pub ratio: f64,
    #[serde(default)]
    pub offset: f64,
    /// Gears, a rack and pinion, a screw or a cam: what the ratio (or a cam's table) comes from.
    /// It is worked out from it again whenever the design changes (`coupling_values`), so it
    /// follows the radii, the pitch and the cam's profile; the last one worked out is kept in
    /// `ratio` and `lift`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupling: Option<Coupling>,
    /// A cam's follower values over one turn of a (`cam::CAM_SAMPLES` of them, in b's units, the
    /// offset left out); empty for every other link.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lift: Vec<f64>,
}

impl MotionLink {
    /// b's value for a's value `x` (before b's limits). A cam's table that is not whole (a
    /// hand-edited file) gives no value, so b keeps where it is; a table on any other link is
    /// not read.
    pub fn follow(&self, x: f64) -> f64 {
        match &self.coupling {
            Some(Coupling::Cam(_)) if self.lift.len() == cam::CAM_SAMPLES => cam::lift_at(&self.lift, x) + self.offset,
            Some(Coupling::Cam(_)) => f64::NAN,
            _ => self.ratio * x + self.offset,
        }
    }
}

/// A length a coupling's ratio comes from: millimetres, an expression over the design's
/// parameters, or the radius of the round face picked on an occurrence (a circle-centre snap,
/// found again on every solve like a joint origin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Size {
    Length(f64),
    Expr(String),
    Circle(JointOrigin),
}

/// The geometry a motion link's ratio comes from. `reverse` turns the follower the other way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Coupling {
    /// Two gears on revolute joints, `ra` on a's and `rb` on b's (pitch radii, or any two lengths
    /// in their proportion such as tooth counts): b turns ra/rb as far as a, the other way round,
    /// or the same way when one is an internal (ring) gear.
    Gear {
        ra: Size,
        rb: Size,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        internal: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// A pinion of pitch radius `radius` on a revolute joint and a rack on a slider: the rack
    /// travels the radius times the pinion's turn in radians (either joint may drive).
    RackPinion {
        radius: Size,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// A screw: the travel advances `pitch` per turn (a cylindrical joint's angle and distance,
    /// or a revolute joint and a slider). Right-handed unless reversed.
    Screw {
        pitch: Size,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// A cam on a revolute joint (a) and its follower (b, a slider or a revolute): b rides the
    /// cam's profile (`joints_cam.rs`).
    Cam(cam::Cam),
}

impl Coupling {
    /// The lengths the ratio comes from: a's gear and b's, the pinion's radius, the pitch, or a
    /// cam's roller radius.
    pub fn sizes(&self) -> Vec<&Size> {
        match self {
            Coupling::Gear { ra, rb, .. } => vec![ra, rb],
            Coupling::RackPinion { radius: s, .. } | Coupling::Screw { pitch: s, .. } => vec![s],
            Coupling::Cam(c) => match &c.follower {
                cam::Follower::Roller { radius } => vec![radius],
                _ => Vec::new(),
            },
        }
    }

    pub fn sizes_mut(&mut self) -> Vec<&mut Size> {
        match self {
            Coupling::Gear { ra, rb, .. } => vec![ra, rb],
            Coupling::RackPinion { radius: s, .. } | Coupling::Screw { pitch: s, .. } => vec![s],
            Coupling::Cam(c) => match &mut c.follower {
                cam::Follower::Roller { radius } => vec![radius],
                _ => Vec::new(),
            },
        }
    }
}

/// Largest length a coupling takes (mm).
const MAX_SIZE: f64 = 1e7;

/// A coupling length in millimetres: positive and finite.
fn size_of(doc: &Document, st: &ModelState, s: &Size) -> Result<f64> {
    let v = match s {
        Size::Length(v) => *v,
        Size::Expr(e) => doc.eval(e, crate::expr::Kind::Length)?,
        Size::Circle(o) => circle_at(doc, st, o)?.radius,
    };
    if v.is_finite() && v > 1e-9 && v <= MAX_SIZE {
        Ok(v)
    } else {
        Err(DocError::Invalid(format!("a gear, rack, screw or roller needs a positive length, not {v}")))
    }
}

/// The ratio a link's coupling gives it (b's value per a's, in radians and mm) and a cam's table
/// (`MotionLink::lift`; empty for the others), worked out from the geometry and parameters it is
/// defined by; `None` for a plain link.
pub fn coupling_values(doc: &Document, st: &ModelState, l: &MotionLink) -> Option<Result<(f64, Vec<f64>)>> {
    match &l.coupling {
        Some(Coupling::Cam(cam)) => Some(cam::table(doc, st, l, cam).map(|t| (1.0, t))),
        Some(c) => Some(coupled(doc, st, l, c).map(|r| (r, Vec::new()))),
        None => None,
    }
}
fn coupled(doc: &Document, st: &ModelState, l: &MotionLink, c: &Coupling) -> Result<f64> {
    let angle = |id: u64, i: usize| doc.assembly.joints.iter().find(|j| j.id == id).is_some_and(|j| j.kind.is_angle(i));
    let (turn_a, turn_b) = (angle(l.a, l.ia), angle(l.b, l.ib));
    // Travel per radian, for a turn coupled to a travel either way round.
    let travel = |k: f64| match (turn_a, turn_b) {
        (true, false) => Ok(k),
        (false, true) => Ok(1.0 / k),
        _ => Err(DocError::Invalid("a rack or a screw couples a turn and a travel".into())),
    };
    let sign = |reverse: bool| if reverse { -1.0 } else { 1.0 };
    match c {
        Coupling::Gear { ra, rb, internal, reverse } => {
            if !(turn_a && turn_b) {
                return Err(DocError::Invalid("gears couple two turns".into()));
            }
            Ok(size_of(doc, st, ra)? / size_of(doc, st, rb)? * sign(!internal) * sign(*reverse))
        }
        Coupling::RackPinion { radius, reverse } => Ok(travel(size_of(doc, st, radius)?)? * sign(*reverse)),
        Coupling::Screw { pitch, reverse } => Ok(travel(size_of(doc, st, pitch)? / std::f64::consts::TAU)? * sign(*reverse)),
        // A cam's follower follows its table (`coupling_values`), not a ratio.
        Coupling::Cam(_) => Ok(1.0),
    }
}

/// Joints, joint origins and motion links of a design.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub joints: Vec<Joint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<NamedOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<MotionLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contact_sets: Vec<crate::motion::ContactSet>,
    /// Contact sets take effect.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contact_enabled: bool,
    /// Every occurrence is in contact with every other (Enable All Contact).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contact_all: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studies: Vec<crate::motion::MotionStudy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traces: Vec<crate::motion::MotionTrace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exploded: Vec<crate::motion::ExplodedView>,
}

impl Assembly {
    /// Every expression the joints and motion links keep, with the joint (or the driven joint of
    /// a link) and what it sets: `offset`, `angle`, a value's name, or a coupling's length.
    pub fn exprs(&self) -> Vec<(u64, String, &str)> {
        let mut out = Vec::new();
        for j in &self.joints {
            let x = &j.exprs;
            out.extend(x.offset.as_deref().map(|e| (j.id, "offset".to_string(), e)));
            out.extend(x.angle.as_deref().map(|e| (j.id, "angle".to_string(), e)));
            for (i, e) in x.values.iter().enumerate() {
                if let Some(e) = e {
                    out.push((j.id, j.kind.dofs().get(i).copied().unwrap_or("value").to_string(), e.as_str()));
                }
            }
        }
        for l in &self.links {
            for s in l.coupling.as_ref().map(Coupling::sizes).unwrap_or_default() {
                if let Size::Expr(e) = s {
                    out.push((l.b, "coupling".to_string(), e.as_str()));
                }
            }
        }
        for trace in &self.traces {
            if let crate::motion::TraceMotion::Joint { joint, from, to, .. } = &trace.motion {
                for (label, value) in [("from", from), ("to", to)] {
                    if let serde_json::Value::String(e) = value {
                        out.push((*joint, format!("trace {} {label}", trace.sketch), e.as_str()));
                    }
                }
            }
        }
        out
    }

    /// The expressions of `exprs`, to rewrite (a renamed parameter).
    pub fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut out = Vec::new();
        for j in &mut self.joints {
            let x = &mut j.exprs;
            out.extend(x.offset.as_mut());
            out.extend(x.angle.as_mut());
            out.extend(x.values.iter_mut().flatten());
        }
        for l in &mut self.links {
            if let Some(c) = &mut l.coupling {
                for s in c.sizes_mut() {
                    if let Size::Expr(ref mut e) = *s {
                        out.push(e);
                    }
                }
            }
        }
        for trace in &mut self.traces {
            if let crate::motion::TraceMotion::Joint { from, to, .. } = &mut trace.motion {
                if let serde_json::Value::String(ref mut e) = *from {
                    out.push(e);
                }
                if let serde_json::Value::String(ref mut e) = *to {
                    out.push(e);
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.joints.is_empty()
            && self.origins.is_empty()
            && self.links.is_empty()
            && self.contact_sets.is_empty()
            && !self.contact_enabled
            && !self.contact_all
            && self.studies.is_empty()
            && self.traces.is_empty()
            && self.exploded.is_empty()
    }
}

/// A frame: origin and orthonormal axes, as a transform (local → parent).
fn frame(o: Vec3, z: Vec3, x: Option<Vec3>) -> Option<Mat> {
    let z = z.normalized()?;
    let x = x.and_then(|x| (x - z * x.dot(z)).normalized()).unwrap_or_else(|| z.any_perp());
    let y = z.cross(x).normalized()?;
    Some([[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [o.x, o.y, o.z, 1.0]])
}

fn rot_z(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn rot_x(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[1.0, 0.0, 0.0, 0.0], [0.0, c, s, 0.0], [0.0, -s, c, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn rot_y(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn shift(x: f64, y: f64, z: f64) -> Mat {
    let mut m = IDENTITY;
    m[3] = [x, y, z, 1.0];
    m
}

/// The joint's motion in A's frame for its values.
pub fn motion(kind: JointKind, q: &[f64]) -> Mat {
    let v = |i: usize| q.get(i).copied().unwrap_or(0.0);
    match kind {
        JointKind::Rigid => IDENTITY,
        JointKind::Revolute => rot_z(v(0)),
        JointKind::Slider => shift(0.0, 0.0, v(0)),
        JointKind::Cylindrical => mat_mul(&rot_z(v(0)), &shift(0.0, 0.0, v(1))),
        JointKind::PinSlot => mat_mul(&shift(v(1), 0.0, 0.0), &rot_z(v(0))),
        JointKind::Planar => mat_mul(&shift(v(0), v(1), 0.0), &rot_z(v(2))),
        JointKind::Ball => mat_mul(&mat_mul(&rot_z(v(0)), &rot_y(v(1))), &rot_z(v(2))),
        JointKind::Parallel => mat_mul(&shift(v(0), v(1), v(2)), &rot_z(v(3))),
        JointKind::Perpendicular | JointKind::Angle => mat_mul(&shift(v(0), v(1), v(2)), &rot_z(v(3))),
        // Their turn is about B's origin, after the offset (`tail`).
        JointKind::PointOnPlane => shift(v(0), v(1), 0.0),
        JointKind::PointOnLine => shift(0.0, 0.0, v(0)),
        JointKind::LineOnPlane => mat_mul(&shift(v(0), v(1), 0.0), &rot_z(v(2))),
    }
}

fn turn_zyx(yaw: f64, pitch: f64, roll: f64) -> Mat {
    mat_mul(&mat_mul(&rot_z(yaw), &rot_y(pitch)), &rot_x(roll))
}

/// What follows the mate (`flip_of`) in a joint's link: for a relation that tilts B's z, the tilt
/// and B's turn about its own z (`spin`); for a point relation, B's turn about its origin (yaw,
/// pitch and roll about z, y and x: free of gimbal lock at the start, all 0); nothing for the
/// other joints.
fn tail(j: &Joint, q: &[f64]) -> Mat {
    let v = |i: usize| q.get(i).copied().unwrap_or(0.0);
    match j.kind {
        JointKind::Perpendicular => mat_mul(&rot_x(std::f64::consts::FRAC_PI_2), &rot_z(v(4))),
        JointKind::Angle => mat_mul(&rot_x(j.angle), &rot_z(v(4))),
        JointKind::LineOnPlane => mat_mul(&rot_y(std::f64::consts::FRAC_PI_2), &rot_z(v(3))),
        JointKind::PointOnPlane => turn_zyx(v(2), v(3), v(4)),
        JointKind::PointOnLine => turn_zyx(v(1), v(2), v(3)),
        _ => IDENTITY,
    }
}

/// B's joint frame in A's for the joint's values: `M(q) · Flip · tail`.
fn link_of(j: &Joint, q: &[f64]) -> Mat {
    mat_mul(&mat_mul(&motion(j.kind, q), &flip_of(j)), &tail(j, q))
}

/// Bodies of a component in the model (its own frame).
fn component_bodies<'a>(doc: &Document, st: &'a ModelState, component: u64) -> Vec<&'a crate::ModelBody> {
    st.bodies.iter().filter(|b| doc.model_body_component(b) == component).collect()
}

/// The component an occurrence places (0 for the root).
fn component_of(doc: &Document, occ: u64) -> Result<u64> {
    if occ == 0 {
        return Ok(0);
    }
    doc.occurrences.iter().find(|o| o.id == occ).map(|o| o.component).ok_or_else(|| DocError::Unknown(format!("occurrence {occ}")))
}

/// A snap's pick point moved onto its named face of `component` (an error when the face is gone:
/// a joint must not quietly attach elsewhere).
fn named_pick(doc: &Document, st: &ModelState, component: u64, pick: Vec3, face: Option<&str>) -> Result<Vec3> {
    match face {
        Some(n) => face::named_face(doc, st, component, pick, n).map(|f| f.point),
        None => Ok(pick),
    }
}

/// The centre (the middle of its outline, `face::outline_centre`) and outward normal of the
/// planar face nearest `pick` (component coordinates; a body moved in from another component
/// placed by its offset, as a named face is).
fn nearest_plane(doc: &Document, st: &ModelState, o: &JointOrigin, pick: Vec3) -> Result<(Vec3, Vec3)> {
    let comp = component_of(doc, o.occurrence)?;
    let mut best: Option<(f64, &crate::ModelBody, solidcraft_kernel::FaceInfo, Vec3)> = None;
    for b in component_bodies(doc, st, comp) {
        let offset = doc.model_body_offset(b);
        for f in b.joint_faces()?.iter() {
            let Some(own) = f.plane_normal else { continue };
            let (c, n) = match offset {
                Some(m) => (apply_point(m, f.centroid), apply_vector(m, own)),
                None => (f.centroid, own),
            };
            let d = (pick - c).dot(n).abs() + 1e-3 * (pick - c).len();
            if best.as_ref().is_none_or(|(bd, ..)| d < *bd) {
                best = Some((d, b, f.clone(), own));
            }
        }
    }
    let (_, b, f, n) = best.ok_or_else(|| DocError::Invalid("joint origin: no planar face there".into()))?;
    // The face's middle: its outline's on a face with holes.
    let c = face::outline_centre(&b.mesh(), f.index, f.centroid, n).unwrap_or(f.centroid);
    Ok(match doc.model_body_offset(b) {
        Some(m) => (apply_point(m, c), apply_vector(m, n)),
        None => (c, n),
    })
}

/// Resolve an axis reference to a line (origin, unit direction) in component coordinates.
pub fn resolve_axis_ref(doc: &Document, st: &ModelState, component: u64, axis: &AxisRef) -> Result<(Vec3, Vec3)> {
    match axis {
        AxisRef::Target(AxisTarget::Origin { axis }) => {
            let d = match axis.to_ascii_uppercase().as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                "Z" => Vec3::Z,
                _ => return Err(DocError::Unknown(format!("axis `{axis}`"))),
            };
            Ok((Vec3::ZERO, d))
        }
        AxisRef::Name(axis) if axis.eq_ignore_ascii_case("X") => Ok((Vec3::ZERO, Vec3::X)),
        AxisRef::Name(axis) if axis.eq_ignore_ascii_case("Y") => Ok((Vec3::ZERO, Vec3::Y)),
        AxisRef::Name(axis) if axis.eq_ignore_ascii_case("Z") => Ok((Vec3::ZERO, Vec3::Z)),
        AxisRef::Target(AxisTarget::Construction { name }) | AxisRef::Name(name) => {
            let mut match_comp = None;
            let mut match_any = None;
            for c in &st.construct {
                if c.name == *name {
                    if let ConstructGeom::Axis { origin, dir } = &c.geom {
                        let feat_comp = doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
                        if feat_comp == component {
                            match_comp = Some((*origin, *dir));
                            break;
                        }
                        if match_any.is_none() {
                            match_any = Some((*origin, *dir));
                        }
                    } else {
                        return Err(DocError::Invalid(format!("construction `{name}` is not an axis")));
                    }
                }
            }
            match_comp.or(match_any).ok_or_else(|| DocError::Unknown(format!("construction axis `{name}`")))
        }
        AxisRef::Target(AxisTarget::Geo(geo)) => {
            let (vals, _) = doc.param_values();
            let ctx = crate::construct::Ctx { doc, vals: &vals, st, depth: 0 };
            crate::construct::line(&ctx, geo)
        }
    }
}

/// Resolve a joint origin to a frame in its component's coordinates.
pub fn resolve_origin(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<Mat> {
    let bad = || DocError::Invalid("joint origin: degenerate frame".into());
    match &o.snap {
        Snap::Frame { origin, z, x } => frame(*origin, *z, *x).ok_or_else(bad),
        Snap::Point { point, z, x } => frame(*point, z.unwrap_or(Vec3::Z), *x).ok_or_else(bad),
        Snap::FaceCenter { pick, face, x } => {
            // A named planar face gives its own centre and normal; another face (or none) the
            // planar face nearest the point.
            let comp = component_of(doc, o.occurrence)?;
            let named = match face.as_deref() {
                Some(n) => Some(face::named_face(doc, st, comp, *pick, n)?),
                None => None,
            };
            let (c, n) = match named.as_ref().map(face::NamedFace::plane).transpose()?.flatten() {
                Some(cn) => cn,
                None => nearest_plane(doc, st, o, named.as_ref().map_or(*pick, |f| f.point))?,
            };
            frame(c, n, face::x_axis(doc, st, comp, x.as_ref(), c)?).ok_or_else(bad)
        }
        Snap::Edge { .. } => edge::edge_frame(doc, st, component_of(doc, o.occurrence)?, &o.snap),
        Snap::CircleCenter { x, .. } => {
            let r = round_of(doc, st, o)?;
            frame(r.centre, r.z, face::x_axis(doc, st, component_of(doc, o.occurrence)?, x.as_ref(), r.centre)?).ok_or_else(bad)
        }
        Snap::Plane { plane, pick, x } => {
            let comp = component_of(doc, o.occurrence)?;
            let (vals, _) = doc.param_values();
            let pl = doc
                .resolve_plane_in(&vals, plane, 0, Some(st))
                .or_else(|err| {
                    if let PlaneRef::Origin { name } = plane.as_ref() {
                        let mut match_comp = None;
                        let mut match_any = None;
                        for c in &st.construct {
                            if c.name == *name {
                                if let ConstructGeom::Plane(p) = &c.geom {
                                    let feat_comp = doc.feature(c.feature).map(|f| f.component).unwrap_or(0);
                                    if feat_comp == comp {
                                        match_comp = Some(*p);
                                        break;
                                    }
                                    if match_any.is_none() {
                                        match_any = Some(*p);
                                    }
                                } else {
                                    return Err(DocError::Invalid(format!("construction `{name}` is not a plane")));
                                }
                            }
                        }
                        match_comp.or(match_any).ok_or(err)
                    } else {
                        Err(err)
                    }
                })
                .map_err(|e| DocError::Invalid(e.to_string()))?;
            let n = pl.normal().normalized().ok_or_else(bad)?;
            let origin = match pick {
                Some(p) => *p - n * (*p - pl.origin).dot(n),
                None => pl.origin,
            };
            let x_axis = match x {
                Some(_) => face::x_axis(doc, st, comp, x.as_ref(), origin)?,
                None => Some(pl.x),
            };
            frame(origin, n, x_axis).ok_or_else(bad)
        }
        Snap::Axis { axis, pick, x } => {
            let comp = component_of(doc, o.occurrence)?;
            let (line_origin, line_dir) = resolve_axis_ref(doc, st, comp, axis)?;
            let z = line_dir.normalized().ok_or_else(bad)?;
            let origin = match pick {
                Some(p) => line_origin + z * (*p - line_origin).dot(z),
                None => line_origin,
            };
            let x_axis = face::x_axis(doc, st, comp, x.as_ref(), origin)?;
            frame(origin, z, x_axis).ok_or_else(bad)
        }
    }
}

/// A circle-centre snap's cylinder and the end it takes (component coordinates): through its
/// named face when it names a cylinder, else the cylindrical face at its pick and the end nearer
/// it, as older builds found them. Any other snap is refused.
fn round_of(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<face::Round> {
    let Snap::CircleCenter { pick, face, end, .. } = &o.snap else { return Err(DocError::Invalid("pick a round face".into())) };
    let comp = component_of(doc, o.occurrence)?;
    if let Some(n) = face.as_deref()
        && let Some(r) = face::named_round(doc, st, comp, *pick, n, end.as_deref())?
    {
        return Ok(r);
    }
    let pick = named_pick(doc, st, comp, *pick, face.as_deref())?;
    // A body moved in from another component: looked at in its own coordinates, its cylinder
    // placed by its offset (as a named face is).
    let cyl = component_bodies(doc, st, comp)
        .into_iter()
        .filter_map(|b| match doc.model_body_offset(b) {
            Some(m) => solidcraft_kernel::cylinder_face_at(&b.body, apply_point(&mat_inverse(m)?, pick)).map(|c| face::placed(m, c)),
            None => solidcraft_kernel::cylinder_face_at(&b.body, pick),
        })
        .min_by(|a, b| {
            let off = |c: &solidcraft_kernel::CylinderFace| {
                let v = pick - c.axis_point;
                ((v - c.axis * v.dot(c.axis)).len() - c.radius).abs()
            };
            off(a).total_cmp(&off(b))
        })
        .ok_or_else(|| DocError::Invalid("joint origin: no cylindrical face there".into()))?;
    let s = (pick - cyl.axis_point).dot(cyl.axis);
    let (at, dir) = if (s - cyl.start).abs() <= (cyl.end - s).abs() { (cyl.start, -1.0) } else { (cyl.end, 1.0) };
    Ok(face::Round { cylinder: cyl, centre: cyl.axis_point + cyl.axis * at, z: cyl.axis * dir })
}

/// The cylindrical face of a circle-centre snap (component coordinates).
fn circle_at(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<solidcraft_kernel::CylinderFace> {
    round_of(doc, st, o).map(|r| r.cylinder)
}

/// World transform of an occurrence (through its parents).
pub fn occurrence_world(doc: &Document, occ: u64) -> Mat {
    let mut m = IDENTITY;
    let mut cur = doc.occurrences.iter().find(|o| o.id == occ);
    for _ in 0..1000 {
        let Some(o) = cur else { break };
        m = mat_mul(&o.transform, &m);
        if o.parent == 0 {
            break;
        }
        cur = doc.occurrence_of(o.parent);
    }
    m
}

/// Clamp values to the joint's limits.
pub fn clamp(j: &Joint, q: &mut [f64]) {
    for (i, v) in q.iter_mut().enumerate() {
        if let Some(Some((lo, hi))) = j.limits.get(i) {
            *v = v.clamp(lo.min(*hi), hi.max(*lo));
        }
    }
}

/// Work requested after placing occurrences and checking their joint constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Analysis {
    /// Placement, loop closure, conflicts and errors only; `Solution::dof` is empty.
    Placement,
    /// Also count each occurrence's remaining motion, including nearby singular poses.
    DegreesOfFreedom,
}

/// The result of placing occurrences by their joints.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Solution {
    /// New world transforms of occurrences the joints place.
    #[serde(skip)]
    pub world: BTreeMap<u64, Mat>,
    /// New values of the joints that moved to close loops (joint id → values).
    #[serde(skip)]
    pub values: BTreeMap<u64, Vec<f64>>,
    /// Joints that disagree with the placement found through others: (joint id, name, why).
    pub conflicts: Vec<(u64, String, String)>,
    /// Joints that could not be applied (bad origin…): (joint id, name, why).
    pub errors: Vec<(u64, String, String)>,
    /// Remaining degrees of freedom per occurrence (6 = free, 0 = fixed): how many independent
    /// values still move it, from the fixed occurrences, with loops kept closed and a value a
    /// motion link sets moving with the values its link chain starts from (a driven gear has its
    /// driving gear's one, a screw's travel its turn's). Empty for [`Analysis::Placement`].
    pub dof: BTreeMap<u64, usize>,
    /// Occurrences fixed in place (grounded, or the root's).
    pub fixed: Vec<u64>,
}

/// Joint values by joint id.
type Values = BTreeMap<u64, Vec<f64>>;

/// How far two placements differ: (translation mm, rotation rad).
fn mismatch(a: &Mat, b: &Mat) -> (f64, f64) {
    let t = Vec3::new(a[3][0] - b[3][0], a[3][1] - b[3][1], a[3][2] - b[3][2]).len();
    // The turn between them from both its cosine and its sine: the cosine alone loses small
    // angles to rounding.
    let c = |i: usize, k: usize| (0..3).map(|n| a[i][n] * b[k][n]).sum::<f64>();
    let sin2 = Vec3::new(c(1, 2) - c(2, 1), c(2, 0) - c(0, 2), c(0, 1) - c(1, 0)).len();
    (t, sin2.atan2(c(0, 0) + c(1, 1) + c(2, 2) - 1.0))
}

/// Passes over the motion links at most: a chain of up to this many links carries its value
/// through, whatever order its links are stored in.
const MAX_LINK_PASSES: usize = 64;

/// Motion links, pass after pass until nothing changes (a chain stored from its end takes a pass
/// per link), each linked value within its joint's limits as it is set. The result is the same
/// whatever the values were before and however the links are stored, so applying them again
/// changes nothing (links that form a cycle stop after `MAX_LINK_PASSES`).
fn apply_links(asm: &Assembly, vals: &mut Values) {
    let limits: BTreeMap<(u64, usize), (f64, f64)> = asm
        .links
        .iter()
        .filter_map(|l| {
            let (lo, hi) = asm.joints.iter().find(|j| j.id == l.b)?.limits.get(l.ib).copied().flatten()?;
            Some(((l.b, l.ib), (lo.min(hi), hi.max(lo))))
        })
        .collect();
    // The linked values, to see whether a pass changed any (several links writing one value
    // change it within a pass, but settle from one pass to the next).
    let linked = |vals: &Values| -> Vec<Option<f64>> { asm.links.iter().map(|l| vals.get(&l.b).and_then(|q| q.get(l.ib)).copied()).collect() };
    for _ in 0..MAX_LINK_PASSES {
        let before = linked(vals);
        for l in &asm.links {
            let Some(src) = vals.get(&l.a).and_then(|q| q.get(l.ia)).copied() else { continue };
            let mut next = l.follow(src);
            // A hand-edited table (or source) that gives no number leaves b where it is.
            if !next.is_finite() {
                continue;
            }
            if let Some((lo, hi)) = limits.get(&(l.b, l.ib)) {
                next = next.max(*lo).min(*hi);
            }
            if let Some(v) = vals.get_mut(&l.b).and_then(|q| q.get_mut(l.ib)) {
                *v = next;
            }
        }
        if linked(vals) == before {
            break;
        }
    }
}

/// Effective values of every joint after motion links and limits.
pub fn joint_values(asm: &Assembly) -> BTreeMap<u64, Vec<f64>> {
    let mut vals: Values = asm
        .joints
        .iter()
        .map(|j| {
            let mut q = j.values.clone();
            q.resize(j.kind.dofs().len(), 0.0);
            (j.id, q)
        })
        .collect();
    apply_links(asm, &mut vals);
    for j in &asm.joints {
        if let Some(q) = vals.get_mut(&j.id) {
            clamp(j, q);
        }
    }
    vals
}

/// The turn and offset from A's joint frame to B's: mated (B's z against A's, as faces meet)
/// unless flipped.
fn flip_of(j: &Joint) -> Mat {
    // An angle relation's `angle` is its tilt (`tail`); a perpendicular one has none.
    let turn = if matches!(j.kind, JointKind::Angle | JointKind::Perpendicular) { 0.0 } else { j.angle };
    let mut m = rot_z(turn);
    m[3][2] = j.offset;
    if !j.flip { mat_mul(&m, &[[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]) } else { m }
}

/// Component-space frames stay fixed throughout one solve. Cache their inverses without
/// regrouping products: reversed joints still invert the value-dependent link as before.
struct JointFrames {
    a: Mat,
    b: Mat,
    inverse_a: Option<Mat>,
    inverse_b: Option<Mat>,
}

impl JointFrames {
    fn new(a: Mat, b: Mat) -> Self {
        Self { a, b, inverse_a: mat_inverse(&a), inverse_b: mat_inverse(&b) }
    }

    fn through(&self, joint: &Joint, values: &[f64], from_a: bool, t: &Mat) -> Option<Mat> {
        let link = mat_mul(&link_of(joint, values), self.inverse_b.as_ref()?);
        if from_a { Some(mat_mul(&mat_mul(t, &self.a), &link)) } else { Some(mat_mul(&mat_mul(t, &mat_inverse(&link)?), self.inverse_a.as_ref()?)) }
    }
}

/// B's world transform from A's (or A's from B's, going the other way) for the joint's values.
fn through(j: &Joint, (fa, fb): &(Mat, Mat), q: &[f64], from_a: bool, t: &Mat) -> Option<Mat> {
    JointFrames::new(*fa, *fb).through(j, q, from_a, t)
}

/// The spanning tree the solver places occurrences along, out from the fixed ones.
struct Tree<'a> {
    joints: BTreeMap<u64, &'a Joint>,
    /// Joint frames in component coordinates (A's, B's).
    frames: BTreeMap<u64, JointFrames>,
    fixed: BTreeMap<u64, Mat>,
    /// Placed occurrence → (the occurrence it is placed from, the joint, from the joint's A side).
    parent: BTreeMap<u64, (u64, u64, bool)>,
    /// Placed occurrences in placement order.
    order: Vec<u64>,
}

impl Tree<'_> {
    fn step(&self, joint: u64, from_a: bool, t: &Mat, vals: &Values) -> Option<Mat> {
        #[cfg(test)]
        tests::PLACEMENT_STEPS.with(|steps| steps.set(steps.get() + 1));
        let q = vals.get(&joint).map(Vec::as_slice).unwrap_or(&[]);
        self.frames.get(&joint)?.through(self.joints.get(&joint)?, q, from_a, t)
    }

    /// Where every placed occurrence is for these values.
    fn place(&self, vals: &Values) -> BTreeMap<u64, Mat> {
        let mut placed = self.fixed.clone();
        for occ in &self.order {
            let Some(&(from, joint, from_a)) = self.parent.get(occ) else { continue };
            if let Some(m) = placed.get(&from).and_then(|t| self.step(joint, from_a, t, vals)) {
                placed.insert(*occ, m);
            }
        }
        placed
    }

    /// The fixed occurrence an occurrence is placed from, and the tree joints from it up to that one.
    fn path(&self, mut occ: u64) -> (u64, Vec<(u64, bool)>) {
        let mut up = Vec::new();
        while let Some(&(from, joint, from_a)) = self.parent.get(&occ) {
            if up.len() > self.order.len() {
                break;
            }
            up.push((joint, from_a));
            occ = from;
        }
        (occ, up)
    }

    /// One occurrence's world placement for these values.
    fn world(&self, occ: u64, vals: &Values) -> Option<Mat> {
        let (root, up) = self.path(occ);
        let mut t = *self.fixed.get(&root)?;
        for (joint, from_a) in up.iter().rev() {
            t = self.step(*joint, *from_a, &t, vals)?;
        }
        Some(t)
    }
}

/// Storage shared by one residual callback's evaluations, scoped to its immutable tree.
/// Keep map nodes and path capacity, but invalidate every placement before reading new values.
struct PlacementWorkspace<'tree, 'doc> {
    tree: &'tree Tree<'doc>,
    placed: BTreeMap<u64, Option<Mat>>,
    path: Vec<(u64, u64, bool)>,
}

impl<'tree, 'doc> PlacementWorkspace<'tree, 'doc> {
    fn new(tree: &'tree Tree<'doc>) -> Self {
        Self { tree, placed: BTreeMap::new(), path: Vec::new() }
    }

    fn at<'values>(&'values mut self, values: &'values Values) -> Placements<'values, 'tree, 'doc> {
        for placed in self.placed.values_mut() {
            *placed = None;
        }
        Placements { workspace: self, values }
    }
}

/// Only this guard can read placements. Its values stay immutable until it is dropped;
/// obtaining the next guard clears validity while retaining the allocated storage.
struct Placements<'values, 'tree, 'doc> {
    workspace: &'values mut PlacementWorkspace<'tree, 'doc>,
    values: &'values Values,
}

impl Placements<'_, '_, '_> {
    fn world(&mut self, mut occurrence: u64) -> Option<Mat> {
        self.workspace.path.clear();
        let mut t = loop {
            if let Some(t) = self.workspace.placed.get(&occurrence).and_then(Option::as_ref).or_else(|| self.workspace.tree.fixed.get(&occurrence)) {
                break *t;
            }
            if self.workspace.path.len() >= self.workspace.tree.order.len() {
                return None;
            }
            let &(from, joint, from_a) = self.workspace.tree.parent.get(&occurrence)?;
            self.workspace.path.push((occurrence, joint, from_a));
            occurrence = from;
        };
        while let Some((occurrence, joint, from_a)) = self.workspace.path.pop() {
            t = self.workspace.tree.step(joint, from_a, &t, self.values)?;
            #[cfg(test)]
            if !self.workspace.placed.contains_key(&occurrence) {
                tests::PLACEMENT_INSERTS.with(|inserts| inserts.set(inserts.get() + 1));
            }
            self.workspace.placed.insert(occurrence, Some(t));
        }
        Some(t)
    }

    fn closure_gaps(&mut self, closing: &[u64], scale: f64, out: &mut Vec<f64>) -> Option<()> {
        for id in closing {
            let (joint, frames) = (self.workspace.tree.joints.get(id)?, self.workspace.tree.frames.get(id)?);
            let tb = self.world(joint.b.occurrence)?;
            let ta = self.world(joint.a.occurrence)?;
            let want = self.workspace.tree.step(*id, true, &ta, self.values)?;
            for p in [Vec3::ZERO, Vec3::X * scale, Vec3::Y * scale] {
                let at = apply_point(&frames.b, p);
                let d = apply_point(&want, at) - apply_point(&tb, at);
                out.extend([d.x, d.y, d.z]);
            }
        }
        Some(())
    }
}

/// Values the loop solver moves at most in one group of loops, and loop-closing joints it takes
/// on at most: larger ones are only checked.
const MAX_LOOP_VALUES: usize = 64;
const MAX_LOOPS: usize = 100;
/// Loop-closing joints one solve takes on at most, over all its groups: a part placed by several
/// relations closes a loop with each one after the first, so a panel of parts has many small
/// groups.
const MAX_CLOSING: usize = 5_000;
/// Residual evaluations one solve may spend on all its loops.
const MAX_LOOP_WORK: usize = 200_000;

/// Held joint values by (joint, value index): the value each moves from.
pub type Held = BTreeMap<(u64, usize), f64>;

/// The values that set the `around` joints' values through motion links, followed back through
/// every link to them (several links may write one value; `apply_links`' order decides which
/// wins on the way) for as many links as `apply_links` passes: all the values reached, and those
/// that no link sets (the ones a solve can move).
fn link_chains(asm: &Assembly, tree: &Tree, around: &BTreeSet<u64>, linked: &BTreeSet<(u64, usize)>) -> (BTreeSet<(u64, usize)>, Vec<(u64, usize)>) {
    let (mut feeds, mut starts) = (BTreeSet::new(), Vec::new());
    let mut frontier: BTreeSet<(u64, usize)> =
        around.iter().filter_map(|id| tree.joints.get(id)).flat_map(|j| (0..j.kind.dofs().len()).map(|i| (j.id, i))).collect();
    let mut seen = frontier.clone();
    for _ in 0..MAX_LINK_PASSES {
        let next: BTreeSet<(u64, usize)> =
            asm.links.iter().filter(|l| frontier.contains(&(l.b, l.ib))).map(|l| (l.a, l.ia)).filter(|v| !seen.contains(v)).collect();
        if next.is_empty() {
            break;
        }
        for &(id, i) in &next {
            feeds.insert((id, i));
            if !linked.contains(&(id, i)) && tree.joints.get(&id).is_some_and(|j| i < j.kind.dofs().len()) {
                starts.push((id, i));
            }
        }
        seen.extend(next.iter().copied());
        frontier = next;
    }
    (feeds, starts)
}

/// The values that move each value a motion link sets: the values its link chain starts from,
/// which no link sets (none for a chain that comes back round on itself, which only a hand-edited
/// design has). A driven gear's turn is its driving gear's: one degree of freedom, not two.
fn link_sources(asm: &Assembly) -> BTreeMap<(u64, usize), BTreeSet<(u64, usize)>> {
    let mut into: BTreeMap<(u64, usize), Vec<(u64, usize)>> = BTreeMap::new();
    for l in &asm.links {
        into.entry((l.b, l.ib)).or_default().push((l.a, l.ia));
    }
    let mut sources = BTreeMap::new();
    if into.is_empty() {
        return sources;
    }
    // A link from a value its joint doesn't have (a hand-edited design) moves nothing.
    let real: BTreeSet<(u64, usize)> = asm.joints.iter().flat_map(|j| (0..j.kind.dofs().len()).map(|i| (j.id, i))).collect();
    for &v in into.keys() {
        // Back along the links, as far as a link chain carries a value (`MAX_LINK_PASSES`).
        let (mut from, mut seen) = (BTreeSet::new(), BTreeSet::from([v]));
        let mut frontier = vec![v];
        for _ in 0..MAX_LINK_PASSES {
            let mut next = Vec::new();
            for w in frontier.iter().filter_map(|w| into.get(w)).flatten() {
                if !seen.insert(*w) {
                    continue;
                }
                if into.contains_key(w) {
                    next.push(*w);
                } else if real.contains(w) {
                    from.insert(*w);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        sources.insert(v, from);
    }
    sources
}

/// The held values among `touches` that move: (joint, index, from, to), `from` within the
/// joint's limits. A suppressed joint still sets values through its motion links.
fn held_moves(asm: &Assembly, held: &Held, touches: &BTreeSet<(u64, usize)>, vals: &Values) -> Vec<(u64, usize, f64, f64)> {
    let mut moves = Vec::new();
    for ((id, i), from) in held.iter().filter(|(v, _)| touches.contains(v)) {
        let (Some(j), Some(to)) = (asm.joints.iter().find(|j| j.id == *id), vals.get(id).and_then(|q| q.get(*i))) else { continue };
        let from = match j.limits.get(*i).copied().flatten() {
            Some((lo, hi)) => from.max(lo.min(hi)).min(hi.max(lo)),
            None => *from,
        };
        if from != *to {
            moves.push((*id, *i, from, *to));
        }
    }
    moves
}

/// Close the loops, each group of loops that share joints on its own: see `close_group`.
/// Returns the moved joints' new stored values (and updates `vals`), and the closing joints of
/// the groups whose held values moved but which can't stay closed on the way (nothing in those
/// moves; a group that can't close without moving is left to the conflict check).
fn close_loops(tree: &Tree, closing: &[u64], asm: &Assembly, held: &Held, vals: &mut Values) -> (Values, BTreeSet<u64>) {
    let (mut changed, mut stuck) = (BTreeMap::new(), BTreeSet::new());
    if closing.len() > MAX_CLOSING {
        return (Values::new(), stuck);
    }
    let linked = set_values(asm);
    let groups = loop_groups(tree, closing, asm, &linked);
    let spent = std::cell::Cell::new(0usize);
    for (group, around, touches) in groups.iter().filter(|(c, _, _)| c.len() <= MAX_LOOPS) {
        let moves = held_moves(asm, held, touches, vals);
        match close_group(tree, group, around, &moves, asm, held, &linked, &spent, vals) {
            Some(m) => changed.extend(m),
            None if !moves.is_empty() => stuck.extend(group.iter().copied()),
            None => {}
        }
    }
    // The stored values with what each group moved (groups may move different values of one
    // joint).
    let mut moved = Values::new();
    for ((id, i), v) in changed {
        let Some(j) = tree.joints.get(&id) else { continue };
        let q = moved.entry(id).or_insert_with(|| {
            let mut q = j.values.clone();
            q.resize(j.kind.dofs().len(), 0.0);
            q
        });
        if let Some(slot) = q.get_mut(i) {
            *slot = v;
        }
    }
    (moved, stuck)
}

/// A group of loops solved together: its closing joints, the joints around them, and the values
/// it touches.
type Group = (Vec<u64>, BTreeSet<u64>, BTreeSet<(u64, usize)>);

/// The loops' groups. Each loop: its closing joint and the joints on the tree paths between its
/// sides; loops that share a value (of a joint, or one feeding them through links) are solved
/// together.
fn loop_groups(tree: &Tree, closing: &[u64], asm: &Assembly, linked: &BTreeSet<(u64, usize)>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for id in closing {
        let Some(j) = tree.joints.get(id) else { continue };
        // A joint between an occurrence and itself (only a hand-edited file has one) is only checked.
        if j.a.occurrence == j.b.occurrence {
            continue;
        }
        let side = |occ: u64| -> BTreeSet<u64> { tree.path(occ).1.iter().map(|(k, _)| *k).collect() };
        let mut around: BTreeSet<u64> = side(j.a.occurrence).symmetric_difference(&side(j.b.occurrence)).copied().collect();
        around.insert(*id);
        // The values the loop depends on: its joints' and those its links read. Loops tie together
        // through a shared value only (a rigid joint on both paths has none; two values of one
        // joint can feed two independent loops).
        let mut touches = link_chains(asm, tree, &around, linked).0;
        touches.extend(around.iter().filter_map(|id| tree.joints.get(id)).flat_map(|j| (0..j.kind.dofs().len()).map(|i| (j.id, i))));
        let mut members = vec![*id];
        groups.retain(|(c, a, t)| {
            if t.is_disjoint(&touches) {
                return true;
            }
            members.extend(c.iter().copied());
            around.extend(a.iter().copied());
            touches.extend(t.iter().copied());
            false
        });
        groups.push((members, around, touches));
    }
    groups
}

/// The values a solve of the loops `around` moves: every value of their joints but the `held`
/// ones and motion-link targets, and the values the targets' link chains start from.
fn unknowns(tree: &Tree, around: &BTreeSet<u64>, asm: &Assembly, linked: &BTreeSet<(u64, usize)>, held: &Held) -> Vec<(u64, usize)> {
    let mut vars: Vec<(u64, usize)> = Vec::new();
    for j in around.iter().filter_map(|id| tree.joints.get(id)) {
        vars.extend((0..j.kind.dofs().len()).map(|i| (j.id, i)).filter(|v| !linked.contains(v) && !held.contains_key(v)));
    }
    for v in link_chains(asm, tree, around, linked).1 {
        if !held.contains_key(&v) && !vars.contains(&v) {
            vars.push(v);
        }
    }
    vars
}

/// Each value's limits, or none.
fn limit_bounds(tree: &Tree, vars: &[(u64, usize)]) -> Vec<(f64, f64)> {
    vars.iter()
        .map(|(id, i)| match tree.joints.get(id).and_then(|j| j.limits.get(*i).copied().flatten()) {
            Some((lo, hi)) => (lo.min(hi), hi.max(lo)),
            None => (f64::NEG_INFINITY, f64::INFINITY),
        })
        .collect()
}

/// The loops' size: the diagonal of the box around their joints' A frames, 1 mm to 1 km. Points
/// that far apart weigh turns like moves.
fn loop_scale(tree: &Tree, around: &BTreeSet<u64>, vals: &Values) -> f64 {
    let joints: Aabb3 = around
        .iter()
        .filter_map(|id| {
            let (j, frames) = (tree.joints.get(id)?, tree.frames.get(id)?);
            Some(apply_point(&mat_mul(&tree.world(j.a.occurrence, vals)?, &frames.a), Vec3::ZERO))
        })
        .collect();
    if joints.is_empty() { 1.0 } else { joints.diagonal().clamp(1.0, 1e6) }
}

/// How far each closing joint is from meeting: three points of B's joint frame, `scale` apart,
/// where the joint wants them against where the tree puts them (appended to `out`).
fn closure_gaps(workspace: &mut PlacementWorkspace, closing: &[u64], vals: &Values, scale: f64, out: &mut Vec<f64>) -> Option<()> {
    workspace.at(vals).closure_gaps(closing, scale, out)
}

/// How far a closing joint may be off before it is a conflict: its frames apart by more than
/// this many millimetres or turned by more than this many radians (a micrometre over a metre).
/// Holes picked on parts are rarely exactly in line: axes a few hundredths of a microradian
/// apart (#6211) are not a conflict, and the loop through them still moves.
const CONFLICT_MM: f64 = 1e-4;
const CONFLICT_RAD: f64 = 1e-6;

/// The largest gap (`closure_gaps`, for loops of size `scale`) that still passes the conflict
/// check, with room to spare.
fn loose_gap(scale: f64) -> f64 {
    CONFLICT_MM.min(CONFLICT_RAD * scale) / 4.0
}

/// Close loops whose gaps `f` gives from `x`, and the tolerance to keep them closed to from here
/// on: `tol`, or, where they come no closer than that but within `loose`, a little more than they
/// do come. Faces modelled a few nanometres apart along parallel axes, or axes a hair out of
/// parallel, leave a gap no value of a planar loop takes out (the second one that grows as the
/// loop turns); the loop still moves, as closed as it comes. `None` when they can't close.
fn close_within(f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &mut Vec<f64>, bounds: &[(f64, f64)], tol: f64, loose: f64) -> Option<f64> {
    if least_squares(f, x, bounds, tol) {
        return Some(tol);
    }
    let left = largest(&f(x)?);
    (left <= loose).then(|| (4.0 * left).clamp(tol, loose.max(tol)))
}

/// Occurrences whose degrees of freedom are worked out through their loops, at most; the rest
/// keep the tree's count.
const MAX_DOF_OCCURRENCES: usize = 256;

/// How far `nearby_pose` moves each value: radians for an angle, a share of the loops' size for a
/// distance. Far enough for a part's motion that vanishes at a dead point to show in a rank, near
/// enough to stay on the mechanism's branch.
const NUDGE: f64 = 1e-3;

/// A pose of the loops near `vals`: each value they move nudged by an amount of its own within
/// its limits (so the nudge points in no special direction), then each group of loops closed
/// again. A group that can't close again, or only further away than a few nudges (on another
/// branch), keeps its values. It has a work budget of its own.
fn nearby_pose(tree: &Tree, groups: &[Group], asm: &Assembly, linked: &BTreeSet<(u64, usize)>, vals: &Values) -> Values {
    let mut near = vals.clone();
    let mut spent = 0usize;
    for (closing, around, _) in groups {
        let vars = unknowns(tree, around, asm, linked, &Held::new());
        if vars.is_empty() || vars.len() > MAX_LOOP_VALUES {
            continue;
        }
        let scale = loop_scale(tree, around, vals);
        let step: Vec<f64> =
            vars.iter().map(|(id, i)| NUDGE * if tree.joints.get(id).is_some_and(|j| j.kind.is_angle(*i)) { 1.0 } else { scale }).collect();
        let bounds = limit_bounds(tree, &vars);
        let x0: Vec<f64> = vars.iter().map(|(id, i)| near.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
        let mut x: Vec<f64> = x0
            .iter()
            .zip(step.iter().zip(&bounds))
            .enumerate()
            .map(|(k, (v, (d, (lo, hi))))| {
                // 0.5 to 1.5 nudges, alternately up and down.
                let w = 0.5 + (k as f64 * 0.618_034).fract();
                (v + d * if k % 2 == 0 { w } else { -w }).max(*lo).min(*hi)
            })
            .collect();
        let mut workspace = PlacementWorkspace::new(tree);
        let mut work = near.clone();
        let mut gaps = |x: &[f64]| -> Option<Vec<f64>> {
            spent += 1;
            if spent > MAX_LOOP_WORK {
                return None;
            }
            for ((id, i), v) in vars.iter().zip(x) {
                if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                    *slot = *v;
                }
            }
            apply_links(asm, &mut work);
            let mut out = Vec::with_capacity(closing.len() * 9);
            closure_gaps(&mut workspace, closing, &work, scale, &mut out)?;
            out.iter().all(|v| v.is_finite()).then_some(out)
        };
        if close_within(&mut gaps, &mut x, &bounds, (1e-10 * scale).min(1e-7), loose_gap(scale)).is_some()
            && x.iter().zip(&x0).zip(&step).all(|((a, b), d)| (a - b).abs() <= 4.0 * d)
            && gaps(&x).is_some()
        {
            near = work;
        }
    }
    near
}

/// The degrees of freedom of the occurrences loops constrain: how many independent ways each can
/// still move with every loop kept closed, the rank of its placement over the joint values once
/// the loops' closure is taken out (rank [J_loops; J_placement] - rank J_loops). An occurrence's
/// values are those on its tree path and the unknowns of the loops on that path. It is counted at
/// the pose and at a pose nearby, and the larger count stands: at a dead point of its loop (an
/// actuator in line with the path of its tip pin) a part's motion vanishes to first order, so the
/// pose alone counts 0. The tree's count stands where a loop group is over the solver's caps or
/// the work runs out.
fn loop_dof(tree: &Tree, closing: &[u64], asm: &Assembly, vals: &Values, dof: &mut BTreeMap<u64, usize>) {
    if closing.len() > MAX_CLOSING {
        return;
    }
    let linked = set_values(asm);
    let mut groups = loop_groups(tree, closing, asm, &linked);
    groups.retain(|(c, _, _)| c.len() <= MAX_LOOPS);
    // The joints loops depend on: those around them, those whose values they read, and those a
    // motion link carries any of these values into.
    let mut relevant: BTreeSet<u64> = groups.iter().flat_map(|(_, a, t)| a.iter().copied().chain(t.iter().map(|(j, _)| *j))).collect();
    for _ in 0..MAX_LINK_PASSES {
        let more: Vec<u64> = asm.links.iter().filter(|l| relevant.contains(&l.a) && !relevant.contains(&l.b)).map(|l| l.b).collect();
        if more.is_empty() {
            break;
        }
        relevant.extend(more);
    }
    // The occurrences a loop may hold or move: placed by such a joint, or below one that is
    // (worked out down the tree, parents first). The others keep the tree's count.
    let mut candidates: BTreeSet<u64> = BTreeSet::new();
    for occ in &tree.order {
        if let Some(&(from, joint, _)) = tree.parent.get(occ)
            && (relevant.contains(&joint) || candidates.contains(&from))
        {
            candidates.insert(*occ);
        }
    }
    let (mut spent, mut counted) = (0usize, 0usize);
    // A pose nearby, when one differs.
    let near = (!candidates.is_empty()).then(|| nearby_pose(tree, &groups, asm, &linked, vals)).filter(|n| n != vals);
    for occ in tree.order.iter().filter(|o| candidates.contains(o)) {
        let path: Vec<u64> = tree.path(*occ).1.iter().map(|(k, _)| *k).collect();
        let (mut values, mut have): (Vec<(u64, usize)>, BTreeSet<(u64, usize)>) = (Vec::new(), BTreeSet::new());
        let mut add = |values: &mut Vec<(u64, usize)>, v: (u64, usize)| {
            if have.insert(v) {
                values.push(v);
            }
        };
        for j in path.iter().filter_map(|k| tree.joints.get(k)) {
            for v in (0..j.kind.dofs().len()).map(|i| (j.id, i)).filter(|v| !linked.contains(v)) {
                add(&mut values, v);
            }
            if values.len() > MAX_LOOP_VALUES {
                break;
            }
        }
        if values.len() > MAX_LOOP_VALUES {
            continue;
        }
        // A value on the path a motion link sets moves with the values its link chain starts from.
        let on_path: BTreeSet<u64> = path.iter().copied().collect();
        for v in link_chains(asm, tree, &on_path, &linked).1 {
            add(&mut values, v);
        }
        // The loops on the path, and every loop whose values those reach (a linked source may be
        // held by a loop of its own), each once.
        let (mut closers, mut around) = (Vec::new(), BTreeSet::new());
        let mut taken: BTreeSet<usize> = BTreeSet::new();
        loop {
            let more: Vec<usize> = groups
                .iter()
                .enumerate()
                .filter(|(g, _)| !taken.contains(g))
                .filter(|(_, (_, a, t))| path.iter().any(|k| a.contains(k)) || t.iter().any(|v| values.contains(v)))
                .map(|(g, _)| g)
                .collect();
            if more.is_empty() {
                break;
            }
            for (c, a, _) in more.iter().filter_map(|g| groups.get(*g)) {
                closers.extend(c.iter().copied());
                around.extend(a.iter().copied());
                for v in unknowns(tree, a, asm, &linked, &Held::new()) {
                    add(&mut values, v);
                }
            }
            taken.extend(more);
        }
        // No loop reaches this occurrence's values (not even through a motion link it drives):
        // the tree's count stands.
        if taken.is_empty() {
            continue;
        }
        counted += 1;
        if counted > MAX_DOF_OCCURRENCES || spent > MAX_LOOP_WORK {
            return;
        }
        if values.len() > MAX_LOOP_VALUES {
            continue;
        }
        let scale = loop_scale(tree, &around, vals);
        let (n, gaps) = (values.len(), closers.len() * 9);
        // How many ways the occurrence moves at a pose, with the loops kept closed.
        let mut free_at = |at: &Values| -> Option<usize> {
            let mut workspace = PlacementWorkspace::new(tree);
            let mut work = at.clone();
            // The loops' gaps, then three points of the occurrence `scale` apart.
            let mut f = |x: &[f64]| -> Option<Vec<f64>> {
                spent += 1;
                for ((id, i), v) in values.iter().zip(x) {
                    if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                        *slot = *v;
                    }
                }
                apply_links(asm, &mut work);
                let mut out = Vec::with_capacity(closers.len() * 9 + 9);
                let mut placed = workspace.at(&work);
                placed.closure_gaps(&closers, scale, &mut out)?;
                let t = placed.world(*occ)?;
                for p in [Vec3::ZERO, Vec3::X * scale, Vec3::Y * scale] {
                    let q = apply_point(&t, p);
                    out.extend([q.x, q.y, q.z]);
                }
                out.iter().all(|v| v.is_finite()).then_some(out)
            };
            let x0: Vec<f64> = values.iter().map(|(id, i)| at.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
            let jac = jacobian(&mut f, &x0, gaps + 9)?;
            let rank = |rows: usize| {
                let mut a = linalg::Mat::zeros(rows, n);
                let mut big = 0.0f64;
                for r in 0..rows {
                    for c in 0..n {
                        let v = jac.get(r * n + c).copied().unwrap_or(0.0);
                        big = big.max(v.abs());
                        a.set(r, c, v);
                    }
                }
                linalg::rref(&mut a, 1e-6 * big.max(1.0)).len()
            };
            Some(rank(gaps + 9).saturating_sub(rank(gaps)))
        };
        let here = free_at(vals);
        let there = near.as_ref().and_then(&mut free_at);
        let Some(free) = here.max(there) else { continue };
        dof.insert(*occ, free.min(6));
    }
}

/// Close one group of loops: move the values of the joints `around` them (all but the `held`
/// values, motion-link targets and rigid joints, within their limits; a linked value moves
/// through the start of its link chain) until every loop-closing joint meets. A loop open at the
/// start is closed first, from where it is. Then the held values `moves` go from the values they
/// are held from to their current ones in small steps, each solved from the last, so the mechanism
/// stays on its branch (an open four-bar does not flip crossed). Returns the values that changed
/// from what is stored, by (joint, index), and updates `vals` (empty when there is nothing to do
/// or the group is too big to solve), or `None` when the loops can't close, or can't stay closed
/// on the way.
fn close_group(
    tree: &Tree,
    closing: &[u64],
    around: &BTreeSet<u64>,
    moves: &[(u64, usize, f64, f64)],
    asm: &Assembly,
    held: &Held,
    linked: &BTreeSet<(u64, usize)>,
    spent: &std::cell::Cell<usize>,
    vals: &mut Values,
) -> Option<BTreeMap<(u64, usize), f64>> {
    let vars = unknowns(tree, around, asm, linked, held);
    if vars.len() > MAX_LOOP_VALUES {
        return Some(BTreeMap::new());
    }
    let bounds = limit_bounds(tree, &vars);
    let scale = loop_scale(tree, around, vals);
    // Closer than the conflict check needs: 1e-7 mm, and turns of 1e-10 rad across the loop.
    let tol = (1e-10 * scale).min(1e-7);
    // The values with the held ones `t` of the way along (and the unknowns at `x`).
    let place = |work: &mut Values, x: &[f64], t: f64| {
        for (id, i, from, to) in moves {
            if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                *slot = from + (to - from) * t;
            }
        }
        for ((id, i), (v, (lo, hi))) in vars.iter().zip(x.iter().zip(&bounds)) {
            if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                *slot = v.max(*lo).min(*hi);
            }
        }
        apply_links(asm, work);
    };
    let x0: Vec<f64> = vars.iter().map(|(id, i)| vals.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
    // Steps that move no value around the loops (links included) more than 5 degrees or a
    // twentieth of the loops' size.
    // How many such steps apart two sets of values are, over the loops' values.
    let stride = |a: &Values, b: &Values| -> f64 {
        let mut most = 0.0f64;
        for j in around.iter().filter_map(|id| tree.joints.get(id)) {
            for i in 0..j.kind.dofs().len() {
                let (Some(p), Some(q)) = (a.get(&j.id).and_then(|v| v.get(i)), b.get(&j.id).and_then(|v| v.get(i))) else { continue };
                let step = if j.kind.is_angle(i) { 5f64.to_radians() } else { 0.05 * scale };
                most = most.max((q - p).abs() / step);
            }
        }
        most
    };
    let effective = |x: &[f64], t: f64| {
        let mut v = vals.clone();
        place(&mut v, x, t);
        v
    };
    let mut first = 1.0f64;
    if !moves.is_empty() {
        first = first.min(1.0 / stride(&effective(&x0, 0.0), &effective(&x0, 1.0)));
    }
    let mut workspace = PlacementWorkspace::new(tree);
    let mut work = vals.clone();
    // How far each closing joint is from meeting with the held values `t` of the way along:
    // three points of B's joint frame, where the joint wants them against where the tree puts them.
    let mut gaps = |x: &[f64], t: f64| -> Option<Vec<f64>> {
        spent.set(spent.get() + 1);
        if spent.get() > MAX_LOOP_WORK {
            return None;
        }
        place(&mut work, x, t);
        let mut out = Vec::with_capacity(closing.len() * 9);
        closure_gaps(&mut workspace, closing, &work, scale, &mut out)?;
        out.iter().all(|v| v.is_finite()).then_some(out)
    };
    // Already closed, with no held value moving. (A moving one is followed all the way: a
    // rocker driven a full turn ends where it started, but can't get there.)
    if moves.is_empty() && gaps(&x0, 1.0).is_some_and(|r| largest(&r) <= tol) {
        return Some(BTreeMap::new());
    }
    // A loop open at the start closes first. One that comes only to within the conflict check
    // (`close_within`) moves on as closed as it is; with nothing held moving, one that started
    // that close is left as it is.
    let mut x = x0;
    let start = largest(&gaps(&x, 0.0)?);
    let loose = loose_gap(scale);
    let tol = match close_within(&mut |x| gaps(x, 0.0), &mut x, &bounds, tol, loose)? {
        t if t > tol && moves.is_empty() && start <= t => return Some(BTreeMap::new()),
        t => t,
    };
    let (mut t, mut dt) = (if moves.is_empty() { 1.0 } else { 0.0 }, first);
    let mut at = effective(&x, t);
    while t < 1.0 {
        if dt < first * 1e-4 {
            return None;
        }
        let t1 = (t + dt).min(1.0);
        let mut trial = x.clone();
        // Each step as closed as the start, or within the conflict check where the loop's gap
        // grows as it moves (axes a hair out of parallel, #6211).
        if close_within(&mut |x| gaps(x, t1), &mut trial, &bounds, tol, loose).is_some() {
            // A step that moved some loop value too far (through a link and a limit, or close to
            // a dead point) is taken again in halves.
            let next = effective(&trial, t1);
            if stride(&at, &next) > 2.0 {
                dt *= 0.5;
                continue;
            }
            (x, t, dt, at) = (trial, t1, (dt * 2.0).min(first), next);
        } else if spent.get() > MAX_LOOP_WORK {
            return None;
        } else {
            dt *= 0.5;
        }
    }
    if largest(&gaps(&x, 1.0)?) > tol.max(loose) {
        return None;
    }
    // The values that changed from what is stored.
    let moved = vars
        .iter()
        .zip(&x)
        .filter(|((id, i), v)| tree.joints.get(id).is_some_and(|j| j.values.get(*i).is_none_or(|s| (s - *v).abs() > 1e-12)))
        .map(|(var, v)| (*var, *v))
        .collect();
    *vals = work;
    Some(moved)
}

fn largest(r: &[f64]) -> f64 {
    r.iter().fold(0.0f64, |m, v| m.max(v.abs()))
}

/// Levenberg–Marquardt: move `x` (kept within `bounds`) until every value of `f` is within
/// `tol` of zero. False when it can't get there.
/// The Jacobian of `f` (`m` values) at `x` by central differences, row-major.
fn jacobian(f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &[f64], m: usize) -> Option<Vec<f64>> {
    const H: f64 = 1e-6;
    let n = x.len();
    let mut jac = vec![0.0; m * n];
    for k in 0..n {
        let (mut xp, mut xm) = (x.to_vec(), x.to_vec());
        if let (Some(p), Some(q)) = (xp.get_mut(k), xm.get_mut(k)) {
            *p += H;
            *q -= H;
        }
        let (rp, rm) = (f(&xp)?, f(&xm)?);
        for (i, (a, b)) in rp.iter().zip(&rm).enumerate().take(m) {
            if let Some(slot) = jac.get_mut(i * n + k) {
                *slot = (a - b) / (2.0 * H);
            }
        }
    }
    Some(jac)
}

fn least_squares(f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &mut Vec<f64>, bounds: &[(f64, f64)], tol: f64) -> bool {
    let n = x.len();
    let norm = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let Some(mut r) = f(x) else { return false };
    if n == 0 {
        return largest(&r) <= tol;
    }
    let mut lambda = 1e-3;
    for _ in 0..50 {
        if largest(&r) <= tol {
            return true;
        }
        // The Jacobian by central differences, then the normal equations JᵀJ δ = −Jᵀr.
        let m = r.len();
        let Some(jac) = jacobian(f, x, m) else { return false };
        let mut jtj = linalg::Mat::zeros(n, n);
        let mut g = vec![0.0; n];
        for (i, ri) in r.iter().enumerate() {
            let row = jac.get(i * n..(i + 1) * n).unwrap_or(&[]);
            for (p, a) in row.iter().enumerate().filter(|(_, a)| **a != 0.0) {
                if let Some(gp) = g.get_mut(p) {
                    *gp -= a * ri;
                }
                for (q, b) in row.iter().enumerate().skip(p) {
                    jtj.add(p, q, a * b);
                }
            }
        }
        for p in 0..n {
            for q in 0..p {
                jtj.set(p, q, jtj.get(q, p));
            }
        }
        let mut stepped = false;
        while lambda < 1e12 {
            let mut a = jtj.clone();
            for p in 0..n {
                a.add(p, p, lambda * (1.0 + jtj.get(p, p)));
            }
            if let Some(d) = linalg::solve(a, g.clone()) {
                let next: Vec<f64> = x.iter().zip(&d).zip(bounds).map(|((v, dv), (lo, hi))| (v + dv).max(*lo).min(*hi)).collect();
                if let Some(rn) = f(&next)
                    && norm(&rn) < norm(&r)
                {
                    *x = next;
                    r = rn;
                    lambda = (lambda / 3.0).max(1e-9);
                    stepped = true;
                    break;
                }
            }
            lambda *= 4.0;
        }
        if !stepped {
            return false;
        }
    }
    largest(&r) <= tol
}

/// Place occurrences from the joint graph.
pub fn solve(doc: &Document, st: &ModelState) -> Solution {
    solve_holding(doc, st, &BTreeMap::new())
}

/// Place occurrences from the joint graph. Joints that close loops are met by moving the values
/// of the other joints around the loop. The `held` values (the ones being driven) keep theirs,
/// moving there from the values given (where the loops last closed) so the others follow them.
pub fn solve_holding(doc: &Document, st: &ModelState, held: &Held) -> Solution {
    solve_holding_with_analysis(doc, st, held, Analysis::DegreesOfFreedom)
}

/// Place occurrences and check constraints with the requested diagnostic work. Placement-only
/// solves still close and check every loop, respect held values and limits, and report errors;
/// they leave `Solution::dof` empty rather than returning the spanning tree's approximate count.
pub fn solve_holding_with_analysis(doc: &Document, st: &ModelState, held: &Held, analysis: Analysis) -> Solution {
    let asm = &doc.assembly;
    let mut sol = Solution::default();
    let mut vals = joint_values(asm);
    let Built { tree, mut placed, closing, mut dof } = build(doc, st, &vals, &mut sol);
    let mut stuck = BTreeSet::new();
    if !closing.is_empty() {
        (sol.values, stuck) = close_loops(&tree, &closing, asm, held, &mut vals);
        if !sol.values.is_empty() {
            placed = tree.place(&vals);
        }
    }
    for id in &closing {
        let Some(j) = tree.joints.get(id) else { continue };
        let (Some(ta), Some(tb)) = (placed.get(&j.a.occurrence), placed.get(&j.b.occurrence)) else { continue };
        let (Some(want), Some(frames)) = (tree.step(*id, true, ta, &vals), tree.frames.get(id)) else { continue };
        // Measured at the joint, where the loop solver closes it.
        let (dt, da) = mismatch(&mat_mul(&want, &frames.b), &mat_mul(tb, &frames.b));
        if dt > CONFLICT_MM || da > CONFLICT_RAD {
            sol.conflicts.push((
                j.id,
                j.name.clone(),
                format!("off by {dt:.4} mm and {:.3}° from the placement the other joints give", da.to_degrees()),
            ));
        } else if stuck.contains(id) {
            // It meets there, but the loop would have to come apart on the way (a rocker
            // driven a full turn ends where it started).
            sol.conflicts.push((j.id, j.name.clone(), "the loop can't stay closed on the way to these values".into()));
        }
    }
    if analysis == Analysis::DegreesOfFreedom {
        if !closing.is_empty() {
            loop_dof(&tree, &closing, asm, &vals, &mut dof);
        }
        for o in &doc.occurrences {
            dof.entry(o.id).or_insert(6);
        }
    } else {
        dof.clear();
    }
    placed.remove(&0);
    sol.world = placed;
    sol.dof = dof;
    sol
}

/// The joint graph laid out for a solve: the tree, where it places each occurrence for the
/// values it was built with, the joints that close loops, and each occurrence's degrees of
/// freedom along the tree.
struct Built<'a> {
    tree: Tree<'a>,
    placed: BTreeMap<u64, Mat>,
    closing: Vec<u64>,
    dof: BTreeMap<u64, usize>,
}

/// Lay out the joint graph: the spanning tree out from the fixed occurrences (the root, grounded
/// ones; with neither involved, the first joint's A side) and the joints left over, which close
/// loops. Joints whose origins can't be resolved go to `sol.errors`; `sol.fixed` is set.
fn build<'a>(doc: &'a Document, st: &ModelState, vals: &Values, sol: &mut Solution) -> Built<'a> {
    let asm = &doc.assembly;
    let joints: Vec<&Joint> = asm.joints.iter().filter(|j| !j.suppressed).take(MAX_JOINTS).collect();
    // Joint frames in component coordinates (errors leave the joint out). An origin several joints
    // share (a panel face every part sits on) is found once.
    let mut frames: BTreeMap<u64, JointFrames> = BTreeMap::new();
    let mut found: BTreeMap<(u64, String), std::result::Result<Mat, String>> = BTreeMap::new();
    let mut resolve = |o: &JointOrigin| -> Result<Mat> {
        let Ok(comp) = component_of(doc, o.occurrence) else { return resolve_origin(doc, st, o) };
        found
            .entry((comp, format!("{:?}", o.snap)))
            .or_insert_with(|| resolve_origin(doc, st, o).map_err(|e| e.to_string()))
            .clone()
            .map_err(DocError::Invalid)
    };
    let params = Params::new(doc);
    for j in &joints {
        // A joint whose expressions can't be worked out (a parameter gone) is left out rather
        // than placed by numbers that no longer say what it was defined by.
        if !j.exprs.is_empty()
            && let Err(e) = expressed(&params, j)
        {
            sol.errors.push((j.id, j.name.clone(), e.to_string()));
            continue;
        }
        match (resolve(&j.a), resolve(&j.b)) {
            (Ok(fa), Ok(fb)) => {
                frames.insert(j.id, JointFrames::new(fa, fb));
            }
            (Err(e), _) | (_, Err(e)) => sol.errors.push((j.id, j.name.clone(), e.to_string())),
        }
    }
    // Fixed: the root, grounded occurrences; with neither involved, the first joint's A side.
    let mut placed: BTreeMap<u64, Mat> = BTreeMap::new();
    let mut dof: BTreeMap<u64, usize> = BTreeMap::new();
    placed.insert(0, IDENTITY);
    dof.insert(0, 0);
    for o in doc.occurrences.iter().filter(|o| o.grounded) {
        placed.insert(o.id, occurrence_world(doc, o.id));
        dof.insert(o.id, 0);
    }
    let involved: BTreeSet<u64> = joints.iter().flat_map(|j| [j.a.occurrence, j.b.occurrence]).collect();
    if !involved.iter().any(|o| placed.contains_key(o))
        && let Some(j) = joints.first()
    {
        placed.insert(j.a.occurrence, occurrence_world(doc, j.a.occurrence));
        dof.insert(j.a.occurrence, 0);
    }
    sol.fixed = placed.keys().copied().collect();
    let mut tree =
        // Every joint's values are known, suppressed ones too (their motion links still set values).
        Tree { joints: asm.joints.iter().map(|j| (j.id, j)).collect(), frames, fixed: placed.clone(), parent: BTreeMap::new(), order: Vec::new() };
    let mut used: BTreeSet<u64> = BTreeSet::new();

    // An occurrence's degrees of freedom are the distinct values that move it: those of the
    // joints on its path, a value a motion link sets standing for the values its link chain
    // starts from (a driven gear turns with its driving gear's one value). Kept per occurrence as
    // a count of the values no link reads or sets, which no other path value can repeat, and the
    // set of the ones that may be repeated (links' sources).
    let sources = link_sources(asm);
    let shared: BTreeSet<(u64, usize)> = sources.values().flatten().copied().collect();
    let mut moved_by: BTreeMap<u64, (usize, BTreeSet<(u64, usize)>)> = BTreeMap::new();

    // Adjacency list with creation index so established joints take priority over later (closing)
    // joints when building the spanning tree.
    let mut at: BTreeMap<u64, Vec<(usize, &Joint)>> = BTreeMap::new();
    for (idx, j) in joints.iter().copied().enumerate() {
        at.entry(j.a.occurrence).or_default().push((idx, j));
        if j.b.occurrence != j.a.occurrence {
            at.entry(j.b.occurrence).or_default().push((idx, j));
        }
    }

    // Spanning tree construction prioritizing the earliest defined joints (which define the
    // kinematic chain) before newly added joints that connect to ground or datum planes.
    let mut heap: BinaryHeap<(std::cmp::Reverse<usize>, u64, u64, u64, bool)> = BinaryHeap::new();
    for &occ in placed.keys() {
        for &(idx, j) in at.get(&occ).map(Vec::as_slice).unwrap_or_default() {
            if !tree.frames.contains_key(&j.id) {
                continue;
            }
            let (other, from_a) = if j.a.occurrence == occ {
                (j.b.occurrence, true)
            } else if j.b.occurrence == occ {
                (j.a.occurrence, false)
            } else {
                continue;
            };
            if !placed.contains_key(&other) {
                heap.push((std::cmp::Reverse(idx), j.id, occ, other, from_a));
            }
        }
    }

    while let Some((_, j_id, occ, other, from_a)) = heap.pop() {
        if placed.contains_key(&other) || used.contains(&j_id) {
            continue;
        }
        let Some(t) = placed.get(&occ).copied() else { continue };
        let Some(m) = tree.step(j_id, from_a, &t, vals) else { continue };
        used.insert(j_id);
        placed.insert(other, m);
        tree.parent.insert(other, (occ, j_id, from_a));
        tree.order.push(other);
        let (mut own, mut common) = moved_by.get(&occ).cloned().unwrap_or_default();
        if let Some(j) = tree.joints.get(&j_id) {
            for v in (0..j.kind.dofs().len()).map(|i| (j.id, i)) {
                match sources.get(&v) {
                    Some(from) => common.extend(from.iter().copied()),
                    None if shared.contains(&v) => {
                        common.insert(v);
                    }
                    None => own += 1,
                }
            }
        }
        dof.insert(other, own + common.len());
        moved_by.insert(other, (own, common));
        for &(idx, next_j) in at.get(&other).map(Vec::as_slice).unwrap_or_default() {
            if used.contains(&next_j.id) || !tree.frames.contains_key(&next_j.id) {
                continue;
            }
            let (next_other, next_from_a) = if next_j.a.occurrence == other {
                (next_j.b.occurrence, true)
            } else if next_j.b.occurrence == other {
                (next_j.a.occurrence, false)
            } else {
                continue;
            };
            if !placed.contains_key(&next_other) {
                heap.push((std::cmp::Reverse(idx), next_j.id, other, next_other, next_from_a));
            }
        }
    }

    // Joints that close loops: the solve moves the loops' values to meet them, then checks they
    // agree.
    let closing: Vec<u64> = joints
        .iter()
        .filter(|j| !used.contains(&j.id) && tree.frames.contains_key(&j.id))
        .filter(|j| placed.contains_key(&j.a.occurrence) && placed.contains_key(&j.b.occurrence))
        .map(|j| j.id)
        .collect();
    Built { tree, placed, closing, dof }
}

impl Document {
    /// Apply a solution: occurrence transforms (in their parents) for the occurrences it places,
    /// and the values of the joints that moved to close loops. Returns true when something moved.
    pub fn apply_joint_solution(&mut self, sol: &Solution) -> bool {
        // Joints and occurrences are indexed once (the first of each id, as a search finds):
        // searching them for each of a large design's thousands of values and occurrences took
        // 20 ms on every change to the design.
        let mut joint_at = std::collections::HashMap::new();
        for (i, j) in self.assembly.joints.iter().enumerate() {
            joint_at.entry(j.id).or_insert(i);
        }
        let mut changed = false;
        for (id, q) in &sol.values {
            if let Some(j) = joint_at.get(id).and_then(|&i| self.assembly.joints.get_mut(i)) {
                j.values.clone_from(q);
                changed = true;
            }
        }
        let parents: BTreeMap<u64, u64> = self.occurrences.iter().map(|o| (o.id, o.parent)).collect();
        let (mut occurrence_at, mut first_of) = (std::collections::HashMap::new(), std::collections::HashMap::new());
        for (i, o) in self.occurrences.iter().enumerate() {
            occurrence_at.entry(o.id).or_insert(i);
            first_of.entry(o.component).or_insert(i);
        }
        for (occ, world) in &sol.world {
            let parent = parents.get(occ).copied().unwrap_or(0);
            // Through the transforms as this loop has left them, as `component_transform` reads.
            let occurrences = &self.occurrences;
            let pw = if parent == 0 {
                IDENTITY
            } else {
                crate::assembly::transform_through(parent, |c| first_of.get(&c).and_then(|&i| occurrences.get(i)))
            };
            let Some(local) = mat_inverse(&pw).map(|inv| mat_mul(&inv, world)) else { continue };
            if let Some(o) = occurrence_at.get(occ).and_then(|&i| self.occurrences.get_mut(i)) {
                let (dt, da) = mismatch(&o.transform, &local);
                if dt > 1e-9 || da > 1e-10 {
                    o.transform = local;
                    changed = true;
                }
            }
        }
        changed
    }

    /// A frame for an as-built joint: the world frame `w` seen from an occurrence.
    pub fn frame_in_occurrence(&self, occ: u64, w: &Mat) -> Option<Snap> {
        let t = if occ == 0 { IDENTITY } else { occurrence_world(self, occ) };
        let local = mat_mul(&mat_inverse(&t)?, w);
        let o = apply_point(&local, Vec3::ZERO);
        let z = apply_vector(&local, Vec3::Z);
        let x = apply_vector(&local, Vec3::X);
        Some(Snap::Frame { origin: o, z, x: Some(x) })
    }

    /// A joint origin's frame in the world (for as-built joints and display).
    pub fn origin_world(&self, st: &ModelState, o: &JointOrigin) -> Result<Mat> {
        let f = resolve_origin(self, st, o)?;
        let t = if o.occurrence == 0 { IDENTITY } else { occurrence_world(self, o.occurrence) };
        Ok(mat_mul(&t, &f))
    }
}

#[path = "joints_cam.rs"]
pub mod cam;

#[path = "joints_edge.rs"]
mod edge;
pub use edge::{edge_axis, edge_snap};

#[path = "joints_reach.rs"]
mod reach;

#[path = "joints_face.rs"]
mod face;
pub use face::{end_faces, face_name_at, named_face_spot, pin_end, round_end_at};
#[path = "joints_relations.rs"]
mod relations;
pub use reach::{REACH_TOL, Reach, Reached, reach};
pub use relations::relation_values;

/// The rest angle that turns B least as joint `j` is made, as B sits now: none or a half turn
/// about the joint's z. Origins whose x is chosen for them (a face, a circle, a point without
/// x) get opposite x when their z are opposite, so two faces that already meet would, at angle
/// 0, spin B a half turn about their shared normal.
pub fn least_turn_angle(doc: &Document, st: &ModelState, j: &Joint) -> f64 {
    let (Ok(fa), Ok(fb)) = (resolve_origin(doc, st, &j.a), resolve_origin(doc, st, &j.b)) else { return 0.0 };
    let (ta, tb) = (occurrence_world(doc, j.a.occurrence), occurrence_world(doc, j.b.occurrence));
    let mut q = j.values.clone();
    q.resize(j.kind.dofs().len(), 0.0);
    clamp(j, &mut q);
    let turn = |angle: f64| {
        let k = Joint { angle, ..j.clone() };
        through(&k, &(fa, fb), &q, true, &ta).map(|m| mismatch(&m, &tb).1)
    };
    match (turn(0.0), turn(std::f64::consts::PI)) {
        (Some(none), Some(half)) if half + 1e-9 < none => std::f64::consts::PI,
        _ => 0.0,
    }
}

/// The frame matrix for an explicit origin / z / x (public for commands).
pub fn frame_of(o: Vec3, z: Vec3, x: Option<Vec3>) -> Option<Mat> {
    frame(o, z, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub(super) static PLACEMENT_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        pub(super) static PLACEMENT_INSERTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub(super) fn placement_tree(asm: &Assembly) -> Tree<'_> {
        Tree {
            joints: asm.joints.iter().map(|j| (j.id, j)).collect(),
            frames: asm.joints.iter().map(|j| (j.id, JointFrames::new(mat_mul(&shift(3.0, 4.0, 5.0), &rot_y(0.3)), rot_x(-0.2)))).collect(),
            fixed: BTreeMap::from([(0, IDENTITY)]),
            parent: BTreeMap::from([(1, (0, 1, true)), (2, (1, 2, false)), (3, (2, 3, true)), (4, (1, 4, true)), (99, (0, 5, true))]),
            order: vec![1, 2, 3, 4, 99],
        }
    }

    /// Shared ancestors are evaluated once, without visiting an unrelated assembly branch.
    /// The same arithmetic must be used after values change and on a reversed tree edge.
    #[test]
    fn residual_placements_reuse_ancestors_only_for_the_current_values() {
        let asm = chain(6, 0.2);
        let tree = placement_tree(&asm);
        let mut values = joint_values(&asm);
        let expected: BTreeMap<u64, Mat> = [1, 2, 3, 4].into_iter().map(|id| (id, tree.world(id, &values).unwrap())).collect();
        let mut workspace = PlacementWorkspace::new(&tree);
        PLACEMENT_STEPS.with(|steps| steps.set(0));
        {
            let mut placed = workspace.at(&values);
            for id in [3, 4, 3, 2, 4] {
                assert_eq!(placed.world(id), expected.get(&id).copied());
            }
            assert_eq!(placed.world(0), Some(IDENTITY));
            assert_eq!(placed.world(1234), None);
        }
        assert_eq!(PLACEMENT_STEPS.with(|steps| steps.get()), 4, "one step per needed occurrence; no unrelated branch");
        values.get_mut(&1).unwrap()[0] += 0.1;
        apply_links(&asm, &mut values);
        let changed = tree.world(3, &values).unwrap();
        assert_ne!(Some(&changed), expected.get(&3));
        let mut placed = workspace.at(&values);
        assert_eq!(placed.world(3), Some(changed), "a new residual evaluation cannot reuse the previous values' placements");
    }

    /// Cached static inverses retain the original multiplication order for every joint kind,
    /// flip and traversal direction. A non-invertible frame remains a failed step.
    #[test]
    fn prepared_joint_frames_match_fresh_inverses() {
        fn reference(j: &Joint, fa: &Mat, fb: &Mat, q: &[f64], from_a: bool, t: &Mat) -> Option<Mat> {
            let link = mat_mul(&link_of(j, q), &mat_inverse(fb)?);
            if from_a { Some(mat_mul(&mat_mul(t, fa), &link)) } else { Some(mat_mul(&mat_mul(t, &mat_inverse(&link)?), &mat_inverse(fa)?)) }
        }
        let mut joint = chain(1, 0.0).joints.remove(0);
        let fa = mat_mul(&shift(3.0, -7.0, 11.0), &rot_y(0.3));
        let fb = mat_mul(&shift(-2.0, 5.0, 13.0), &rot_x(-0.2));
        let t = mat_mul(&shift(17.0, 19.0, -23.0), &rot_z(0.7));
        let singular = [[0.0; 4]; 4];
        for (a, b) in [(fa, fb), (singular, fb), (fa, singular)] {
            let frames = JointFrames::new(a, b);
            for kind in [
                JointKind::Rigid,
                JointKind::Revolute,
                JointKind::Slider,
                JointKind::Cylindrical,
                JointKind::PinSlot,
                JointKind::Planar,
                JointKind::Ball,
                JointKind::Parallel,
                JointKind::Perpendicular,
                JointKind::Angle,
                JointKind::PointOnPlane,
                JointKind::PointOnLine,
                JointKind::LineOnPlane,
            ] {
                joint.kind = kind;
                joint.offset = 2.7;
                joint.angle = -0.41;
                for flip in [false, true] {
                    joint.flip = flip;
                    for angle in [0.0, 1e-6, -0.37, std::f64::consts::PI] {
                        let q: Vec<f64> = (0..kind.dofs().len()).map(|i| angle * (i + 1) as f64).collect();
                        for forward in [false, true] {
                            assert_eq!(
                                frames.through(&joint, &q, forward, &t),
                                reference(&joint, &a, &b, &q, forward, &t),
                                "{kind:?}, {flip}, {angle}, {forward}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The loop solver's residual entry point reuses shared ancestors for each perturbation;
    /// row order, duplicate rows, append semantics and missing-origin failure stay unchanged.
    #[test]
    fn closure_residuals_reuse_shared_ancestors_for_each_perturbation() {
        let mut asm = chain(6, 0.2);
        asm.joints[5].a.occurrence = 3;
        asm.joints[5].b.occurrence = 4;
        let tree = placement_tree(&asm);
        let mut workspace = PlacementWorkspace::new(&tree);
        let mut cold = true;
        for start in [0.2, 0.2 + 1e-6, 0.2 - 1e-6, 1.3] {
            let mut values = joint_values(&asm);
            values.get_mut(&1).unwrap()[0] = start;
            apply_links(&asm, &mut values);
            let mut expected = vec![42.0];
            for id in [6, 6] {
                let joint = tree.joints.get(&id).unwrap();
                let frames = tree.frames.get(&id).unwrap();
                let a = tree.world(joint.a.occurrence, &values).unwrap();
                let b = tree.world(joint.b.occurrence, &values).unwrap();
                let want = through(joint, &(frames.a, frames.b), values.get(&id).unwrap(), true, &a).unwrap();
                for point in [Vec3::ZERO, Vec3::X * 37.0, Vec3::Y * 37.0] {
                    let at = apply_point(&frames.b, point);
                    let d = apply_point(&want, at) - apply_point(&b, at);
                    expected.extend([d.x, d.y, d.z]);
                }
            }
            PLACEMENT_STEPS.with(|steps| steps.set(0));
            PLACEMENT_INSERTS.with(|inserts| inserts.set(0));
            let mut actual = vec![42.0];
            assert_eq!(closure_gaps(&mut workspace, &[6, 6], &values, 37.0, &mut actual), Some(()));
            assert_eq!(actual, expected);
            assert_eq!(PLACEMENT_INSERTS.with(|inserts| inserts.get()), if cold { 4 } else { 0 }, "reuse cache storage between perturbations");
            cold = false;
            assert_eq!(PLACEMENT_STEPS.with(|steps| steps.get()), 6, "four tree placements and two closing-joint steps");
            assert_eq!(closure_gaps(&mut workspace, &[u64::MAX], &values, 37.0, &mut actual), None);
            assert_eq!(actual, expected, "an unresolved joint does not insert a zero residual");
        }
    }

    /// The parameter values kept between expressions give what evaluating each expression on
    /// its own gives, errors included.
    #[test]
    fn kept_parameters_evaluate_as_the_design_does() {
        let mut doc = Document::new("p");
        doc.set_param("W", "300 mm", None, None).unwrap();
        doc.set_param("H", "W / 2", None, None).unwrap();
        doc.set_param("A", "30 deg", None, None).unwrap();
        let params = Params::new(&doc);
        for (e, kind) in [
            ("W / 4", crate::expr::Kind::Length),
            ("H + 2 mm", crate::expr::Kind::Length),
            ("A * 2", crate::expr::Kind::Angle),
            ("12", crate::expr::Kind::Length),
        ] {
            assert_eq!(params.eval(e, kind).unwrap(), doc.eval(e, kind).unwrap(), "{e}");
        }
        for (e, kind) in [("Q + 1", crate::expr::Kind::Length), ("A", crate::expr::Kind::Length)] {
            assert_eq!(params.eval(e, kind).unwrap_err().to_string(), doc.eval(e, kind).unwrap_err().to_string(), "{e}");
        }
    }

    fn close(a: &Mat, b: &Mat) -> bool {
        let (t, r) = mismatch(a, b);
        t < 1e-9 && r < 1e-9
    }

    /// Revolute joints 1…n (joint 1 at `start`, the others at 0) linked 1 → 2 → … → n with
    /// ratio 2, the links stored from the end of the chain; joint 3 limited to ±50.
    pub(super) fn chain(n: u64, start: f64) -> Assembly {
        let origin = JointOrigin { occurrence: 0, snap: Snap::Point { point: Vec3::ZERO, z: None, x: None } };
        let joints = (1..=n)
            .map(|id| Joint {
                id,
                name: format!("J{id}"),
                kind: JointKind::Revolute,
                a: origin.clone(),
                b: origin.clone(),
                values: vec![if id == 1 { start } else { 0.0 }],
                limits: vec![(id == 3).then_some((-50.0, 50.0))],
                flip: false,
                offset: 0.0,
                angle: 0.0,
                suppressed: false,
                exprs: Default::default(),
            })
            .collect();
        let links =
            (1..n).rev().map(|a| MotionLink { a, ia: 0, b: a + 1, ib: 0, ratio: 2.0, offset: 0.0, coupling: None, lift: Vec::new() }).collect();
        Assembly { joints, links, ..Assembly::default() }
    }

    /// A linked value moves with the free value its chain starts from, however the chain is
    /// stored; a chain that comes back round on itself, or a link from a value its joint lacks
    /// (hand-edited designs), moves it with nothing.
    #[test]
    fn linked_values_move_with_their_chains_start() {
        let mut asm = chain(5, 0.0);
        let sources = link_sources(&asm);
        for b in 2..=5 {
            assert_eq!(sources.get(&(b, 0)), Some(&BTreeSet::from([(1, 0)])), "J{b}: {sources:?}");
        }
        assert!(!sources.contains_key(&(1, 0)));
        let link = |a: u64, ia: usize, b: u64| MotionLink { a, ia, b, ib: 0, ratio: 1.0, offset: 0.0, coupling: None, lift: Vec::new() };
        asm.links.push(link(5, 0, 1));
        assert!(link_sources(&asm).values().all(BTreeSet::is_empty), "{:?}", link_sources(&asm));
        let mut asm = chain(2, 0.0);
        asm.links = vec![link(1, 7, 2)];
        assert_eq!(link_sources(&asm).get(&(2, 0)), Some(&BTreeSet::new()));
        assert!(link_sources(&Assembly::default()).is_empty());
    }

    /// Links carry a value down a chain stored from its end, each linked value within its limits
    /// as it is set, and applying them again changes nothing.
    #[test]
    fn links_settle_whatever_order_they_are_stored_in() {
        let asm = chain(8, 10.0);
        let vals = joint_values(&asm);
        let at = |id: u64| vals.get(&id).and_then(|q| q.first()).copied().unwrap_or(f64::NAN);
        // 10 → 20 → 40 (within joint 3's limit) → 80 → … → 1280.
        assert_eq!((at(1), at(2), at(3), at(4), at(8)), (10.0, 20.0, 40.0, 80.0, 1280.0));
        let asm = chain(8, 40.0);
        let vals = joint_values(&asm);
        let at = |id: u64| vals.get(&id).and_then(|q| q.first()).copied().unwrap_or(f64::NAN);
        // Joint 3 would be 160: held at 50, and what follows it follows the 50.
        assert_eq!((at(3), at(4), at(8)), (50.0, 100.0, 1600.0));
        let mut again = vals.clone();
        apply_links(&asm, &mut again);
        assert_eq!(again, vals);
    }

    /// A coupling saves as its definition: sizes as numbers, expressions or picked faces, flags
    /// only when set; a link without one (as every design before couplings has) reads as before.
    #[test]
    fn couplings_read_and_write_their_definition() {
        let old: MotionLink = serde_json::from_str(r#"{"a": 1, "b": 2, "ratio": 2}"#).unwrap();
        assert_eq!((old.coupling, old.offset, old.ia), (None, 0.0, 0));
        let text = r#"{"a": 1, "b": 2, "ratio": -0.5, "coupling": {"type": "gear", "ra": 10, "rb": "r2 * 2",
            "internal": true}}"#;
        let l: MotionLink = serde_json::from_str(text).unwrap();
        let Some(Coupling::Gear { ra, rb, internal, reverse }) = &l.coupling else { panic!("{l:?}") };
        assert_eq!((ra, rb, *internal, *reverse), (&Size::Length(10.0), &Size::Expr("r2 * 2".into()), true, false));
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["coupling"], serde_json::json!({"type": "gear", "ra": 10.0, "rb": "r2 * 2", "internal": true}));
        let picked: Size = serde_json::from_str(r#"{"occurrence": 3, "snap": {"type": "circle_center", "pick": [1, 0, 0]}}"#).unwrap();
        assert!(matches!(picked, Size::Circle(JointOrigin { occurrence: 3, snap: Snap::CircleCenter { .. } })));
        let screw: Coupling = serde_json::from_str(r#"{"type": "screw", "pitch": 1.5, "reverse": true}"#).unwrap();
        assert_eq!(screw, Coupling::Screw { pitch: Size::Length(1.5), reverse: true });
    }

    /// Ratios from the sizes: gears turn by their radii (the other way unless internal), a rack
    /// and a screw travel per radian whichever of the two joints drives.
    #[test]
    fn coupling_ratios_follow_their_sizes() {
        let mut doc = Document { assembly: chain(3, 0.0), ..Document::default() };
        if let Some(j) = doc.assembly.joints.get_mut(2) {
            j.kind = JointKind::Slider;
        }
        let st = ModelState::default();
        let ratio = |a: u64, b: u64, c: Coupling| {
            let l = MotionLink { a, ia: 0, b, ib: 0, ratio: 0.0, offset: 0.0, coupling: Some(c), lift: Vec::new() };
            coupling_values(&doc, &st, &l).map(|r| r.map(|(ratio, _)| ratio).map_err(|e| e.to_string()))
        };
        let len = Size::Length;
        assert_eq!(ratio(1, 2, Coupling::Gear { ra: len(10.0), rb: len(40.0), internal: false, reverse: false }), Some(Ok(-0.25)));
        assert_eq!(ratio(1, 2, Coupling::Gear { ra: len(10.0), rb: len(40.0), internal: true, reverse: false }), Some(Ok(0.25)));
        assert_eq!(ratio(1, 3, Coupling::RackPinion { radius: len(8.0), reverse: false }), Some(Ok(8.0)));
        assert_eq!(ratio(3, 1, Coupling::RackPinion { radius: len(8.0), reverse: true }), Some(Ok(-0.125)));
        let screw = ratio(1, 3, Coupling::Screw { pitch: len(std::f64::consts::TAU), reverse: false });
        assert_eq!(screw, Some(Ok(1.0)));
        assert!(matches!(ratio(1, 2, Coupling::RackPinion { radius: len(8.0), reverse: false }), Some(Err(_))));
        assert!(matches!(ratio(1, 3, Coupling::Gear { ra: len(1.0), rb: len(1.0), internal: false, reverse: false }), Some(Err(_))));
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY, 1e300] {
            assert!(matches!(ratio(1, 2, Coupling::Gear { ra: len(bad), rb: len(1.0), internal: false, reverse: false }), Some(Err(_))), "{bad}");
        }
        assert!(matches!(ratio(1, 2, Coupling::Gear { ra: Size::Expr("nope".into()), rb: len(1.0), internal: false, reverse: false }), Some(Err(_))));
    }

    #[test]
    fn motions_compose() {
        assert!(close(&motion(JointKind::Revolute, &[std::f64::consts::FRAC_PI_2]), &rot_z(std::f64::consts::FRAC_PI_2)));
        let m = motion(JointKind::Cylindrical, &[0.0, 5.0]);
        assert!((m[3][2] - 5.0).abs() < 1e-12);
        let m = motion(JointKind::PinSlot, &[0.0, 3.0]);
        assert!((m[3][0] - 3.0).abs() < 1e-12);
        assert_eq!(JointKind::Ball.dofs().len(), 3);
        assert!(JointKind::parse("Pin Slot").is_some() && JointKind::parse("nope").is_none());
        let f = frame(Vec3::new(1.0, 2.0, 3.0), Vec3::Z, None).unwrap();
        assert!(close(&mat_mul(&f, &mat_inverse(&f).unwrap()), &IDENTITY));
    }

    #[test]
    fn mismatch_resolves_tiny_turns() {
        let (_, a) = mismatch(&rot_z(1e-9), &IDENTITY);
        assert!((a - 1e-9).abs() < 1e-15, "{a}");
        let (_, a) = mismatch(&rot_z(3.0), &IDENTITY);
        assert!((a - 3.0).abs() < 1e-12, "{a}");
    }

    /// Applying a solution stores each occurrence in its parent's frame as the loop has left it:
    /// a component the same solution moves is read moved, through its first occurrence. Joint
    /// values land on their joints; a value for a missing joint is skipped.
    #[test]
    fn a_solution_places_occurrences_in_their_moved_parents() {
        let occurrence =
            |id, component, parent| crate::Occurrence { id, component, parent, name: format!("O{id}"), transform: IDENTITY, grounded: false };
        // Component 1 sits at the root twice (its first occurrence is the frame its children sit
        // in), component 2 inside 1 and component 3 inside 2.
        let mut doc = Document {
            assembly: chain(2, 0.0),
            occurrences: vec![occurrence(10, 1, 0), occurrence(11, 1, 0), occurrence(20, 2, 1), occurrence(30, 3, 2)],
            ..Document::default()
        };
        let w10 = mat_mul(&shift(3.0, 4.0, 5.0), &rot_y(0.3));
        let w20 = mat_mul(&shift(-1.0, 2.0, 0.5), &rot_x(-0.2));
        let w30 = mat_mul(&shift(0.0, 7.0, 1.0), &rot_y(1.1));
        let world = BTreeMap::from([(10, w10), (20, w20), (30, w30)]);
        let sol = Solution { world: world.clone(), values: BTreeMap::from([(2, vec![0.75]), (99, vec![1.0])]), ..Solution::default() };
        assert!(doc.apply_joint_solution(&sol));
        for (component, w) in [(1, w10), (2, w20), (3, w30)] {
            assert!(close(&doc.component_transform(component), &w), "component {component}");
        }
        assert!(close(&doc.occurrences[1].transform, &IDENTITY), "only the occurrence the solution names moves");
        let values: Vec<_> = doc.assembly.joints.iter().map(|j| j.values.clone()).collect();
        assert_eq!(values, vec![vec![0.0], vec![0.75]]);
        assert!(!doc.apply_joint_solution(&Solution { world, ..Solution::default() }), "applied twice, nothing moves");
    }

    #[test]
    fn resolve_plane_and_axis_origins() {
        let doc = Document::default();
        let mut st = ModelState::default();
        st.construct.push(crate::construct::Construct {
            feature: 10,
            name: "Spine".into(),
            geom: ConstructGeom::Axis { origin: Vec3::new(10.0, 5.0, 0.0), dir: Vec3::Z },
        });
        st.construct.push(crate::construct::Construct {
            feature: 11,
            name: "WorkPlane".into(),
            geom: ConstructGeom::Plane(solidcraft_geom::Plane::new(Vec3::new(0.0, 0.0, 15.0), Vec3::X, Vec3::Y).unwrap()),
        });

        // 1. Origin plane XY
        let o_xy = JointOrigin { occurrence: 0, snap: Snap::Plane { plane: Box::new(PlaneRef::Origin { name: "XY".into() }), pick: None, x: None } };
        let m = resolve_origin(&doc, &st, &o_xy).unwrap();
        assert_eq!(apply_point(&m, Vec3::ZERO), Vec3::ZERO);
        assert_eq!(apply_vector(&m, Vec3::Z), Vec3::Z);

        // 2. Origin plane XY with pick projected
        let o_xy_pick = JointOrigin {
            occurrence: 0,
            snap: Snap::Plane { plane: Box::new(PlaneRef::Origin { name: "XY".into() }), pick: Some(Vec3::new(12.0, 34.0, 56.0)), x: None },
        };
        let m_pick = resolve_origin(&doc, &st, &o_xy_pick).unwrap();
        assert_eq!(apply_point(&m_pick, Vec3::ZERO), Vec3::new(12.0, 34.0, 0.0));

        // 3. Construction plane by name
        let o_wp =
            JointOrigin { occurrence: 0, snap: Snap::Plane { plane: Box::new(PlaneRef::Origin { name: "WorkPlane".into() }), pick: None, x: None } };
        let m_wp = resolve_origin(&doc, &st, &o_wp).unwrap();
        assert_eq!(apply_point(&m_wp, Vec3::ZERO), Vec3::new(0.0, 0.0, 15.0));
        assert_eq!(apply_vector(&m_wp, Vec3::Z), Vec3::Z);

        // 4. Origin axis Z
        let o_z = JointOrigin { occurrence: 0, snap: Snap::Axis { axis: Box::new(AxisRef::origin("Z")), pick: None, x: None } };
        let m_z = resolve_origin(&doc, &st, &o_z).unwrap();
        assert_eq!(apply_point(&m_z, Vec3::ZERO), Vec3::ZERO);
        assert_eq!(apply_vector(&m_z, Vec3::Z), Vec3::Z);

        // 5. Construction axis
        let o_spine = JointOrigin { occurrence: 0, snap: Snap::Axis { axis: Box::new(AxisRef::Name("Spine".into())), pick: None, x: None } };
        let m_spine = resolve_origin(&doc, &st, &o_spine).unwrap();
        assert_eq!(apply_point(&m_spine, Vec3::ZERO), Vec3::new(10.0, 5.0, 0.0));
        assert_eq!(apply_vector(&m_spine, Vec3::Z), Vec3::Z);

        // 6. Serde roundtrip
        let json = serde_json::to_string(&o_xy).unwrap();
        let decoded: JointOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(o_xy, decoded);
    }
}
