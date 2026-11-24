//! Arrange on the record: the kernel's [`solidcore::arrange`] packing of outlines on sheets, the
//! footprints of record bodies seen from above, the flat sides they can be laid on and the turns
//! that lay them there. Bodies are copied into the kernel's kinds as the hidden-line drawing
//! copies them ([`crate::hlr`]); shapes and vectors are copied field by field.
//!
//! Nothing here computes: every answer is the kernel's.

use solidcore::arrange as k;
use solidcore::arrange::footprint as kf;
use solidcore::arrange::lay as kl;
use solidcore::arrange::rest as kr;
use solidcore_convert::{record_vec2, record_vec3, vec2, vec3};
use solidcore_record::brep::{Brep, Shell};
use solidcore_record::{Vec2, Vec3};

pub use k::coarsen::COARSEN_BUDGET;
pub use k::{
    ArrangeError, ArrangeMethod, ArrangeOptions, Hurry, MAX_COORD, MAX_COPIES, MAX_CORNERS,
    MAX_IMPROVE, MAX_OBSTACLES, MAX_POSE_CORNERS, MAX_ROTATIONS,
};
pub use kf::{FootprintError, MAX_PIECES};
pub use kr::{CHORD, MAX_FACE_CORNERS, MAX_SIDES};

/// A region: an outer ring and holes (the kernel's [`k::Shape2`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape2 {
    /// The outer ring.
    pub outer: Vec<Vec2>,
    /// The holes.
    pub holes: Vec<Vec<Vec2>>,
}

fn ring(r: &[Vec2]) -> Vec<solidcore::math::Vec2> {
    r.iter().copied().map(vec2).collect()
}

fn record_ring(r: &[solidcore::math::Vec2]) -> Vec<Vec2> {
    r.iter().copied().map(record_vec2).collect()
}

impl Shape2 {
    fn kernel(&self) -> k::Shape2 {
        k::Shape2 {
            outer: ring(&self.outer),
            holes: self.holes.iter().map(|h| ring(h)).collect(),
        }
    }

    fn of(s: &k::Shape2) -> Shape2 {
        Shape2 {
            outer: record_ring(&s.outer),
            holes: s.holes.iter().map(|h| record_ring(h)).collect(),
        }
    }
}

/// A part to place (the kernel's [`k::ArrangeItem`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeItem {
    /// Its outline.
    pub shape: Shape2,
    /// How many copies.
    pub quantity: usize,
    /// The turns it may take, degrees.
    pub rotations_deg: Vec<f64>,
}

/// Where one copy went (the kernel's [`k::Placement2`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement2 {
    /// The part.
    pub item: usize,
    /// Which of its copies.
    pub copy: usize,
    /// Its turn, radians.
    pub angle: f64,
    /// Its move after the turn.
    pub shift: Vec2,
}

impl Placement2 {
    fn of(p: &k::Placement2) -> Placement2 {
        Placement2 {
            item: p.item,
            copy: p.copy,
            angle: p.angle,
            shift: record_vec2(p.shift),
        }
    }
}

/// A box in the plane (the kernel's [`k::Bounds2`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds2 {
    /// Its low corner.
    pub min: Vec2,
    /// Its high corner.
    pub max: Vec2,
}

/// A layout (the kernel's [`k::ArrangeResult`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeResult {
    /// The copies placed.
    pub placed: Vec<Placement2>,
    /// The parts with copies left over.
    pub unplaced: Vec<usize>,
    /// The box round what was placed.
    pub used: Bounds2,
    /// How much of the sheet the parts cover.
    pub fill: f64,
    /// How far the layout gave up quality for time.
    pub hurried: Hurry,
}

/// [`k::arrange_hurried`] on the record's vectors.
///
/// # Errors
/// As [`k::arrange_hurried`].
pub fn arrange_hurried(
    items: &[ArrangeItem],
    sheet: &Shape2,
    obstacles: &[Shape2],
    options: &ArrangeOptions,
    cancel: &dyn Fn() -> bool,
    hurry: &dyn Fn() -> Hurry,
) -> Result<ArrangeResult, ArrangeError> {
    let items: Vec<k::ArrangeItem> = items
        .iter()
        .map(|it| k::ArrangeItem {
            shape: it.shape.kernel(),
            quantity: it.quantity,
            rotations_deg: it.rotations_deg.clone(),
        })
        .collect();
    let obstacles: Vec<k::Shape2> = obstacles.iter().map(Shape2::kernel).collect();
    let r = k::arrange_hurried(&items, &sheet.kernel(), &obstacles, options, cancel, hurry)?;
    Ok(ArrangeResult {
        placed: r.placed.iter().map(Placement2::of).collect(),
        unplaced: r.unplaced,
        used: Bounds2 {
            min: record_vec2(r.used.min),
            max: record_vec2(r.used.max),
        },
        fill: r.fill,
        hurried: r.hurried,
    })
}

/// [`k::apart`] on the record's vectors.
#[must_use]
pub fn apart(a: &Shape2, b: &Shape2, clear: f64) -> bool {
    k::apart(&a.kernel(), &b.kernel(), clear)
}

/// [`k::margin_line`] on the record's vectors.
#[must_use]
pub fn margin_line(ring: &[Vec2], by: f64, hole: bool) -> Option<Vec<Vec2>> {
    k::margin_line(&self::ring(ring), by, hole).map(|r| record_ring(&r))
}

/// [`k::coarsen`] on the record's vectors.
#[must_use]
pub fn coarsen(shape: &Shape2, reach: f64, budget: f64) -> Shape2 {
    Shape2::of(&k::coarsen(&shape.kernel(), reach, budget))
}

/// The outline a body covers seen from above (the kernel's [`kf::Footprint`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Footprint {
    /// The outline, counter-clockwise, in the XY coordinates of the turned body; no holes.
    pub shape: Shape2,
    /// How far the outline may be from the true one (mm).
    pub error: f64,
    /// Whether this is the convex hull of the drawing rather than its outline.
    pub hull: bool,
}

impl Footprint {
    fn of(f: &kf::Footprint) -> Footprint {
        Footprint {
            shape: Shape2::of(&f.shape),
            error: f.error,
            hull: f.hull,
        }
    }
}

fn turn(r: [Vec3; 3]) -> [solidcore::Vec3; 3] {
    r.map(vec3)
}

fn record_turn(r: [solidcore::Vec3; 3]) -> [Vec3; 3] {
    r.map(record_vec3)
}

/// A record body made ready for its footprints (the kernel's [`kf::Shadow`]).
pub struct Shadow(kf::Shadow);

impl Shadow {
    /// `body` validated, copied into the kernel's kinds and made ready.
    ///
    /// # Errors
    /// [`FootprintError::Hlr`] for an invalid record, geometry the kernel's kinds don't hold, or
    /// as [`kf::Shadow::new`].
    pub fn new(body: &Brep, cancel: &(dyn Fn() -> bool + Sync)) -> Result<Self, FootprintError> {
        crate::record_check::validate(body)
            .map_err(|_| solidcore::hlr::HlrError::Invalid("portable B-rep"))?;
        let b = crate::hlr::kernel_body(body)?;
        Ok(Shadow(kf::Shadow::new(&b, cancel)?))
    }
}

/// [`kf::footprint`] of a record body.
///
/// # Errors
/// As [`Shadow::new`] and [`footprint_of`].
pub fn footprint(
    body: &Brep,
    rotation: [Vec3; 3],
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Footprint, FootprintError> {
    footprint_of(&[&Shadow::new(body, cancel)?], rotation, cancel)
}

/// [`kf::footprint_of`] on the record's vectors.
///
/// # Errors
/// As [`kf::footprint_of`].
pub fn footprint_of(
    shadows: &[&Shadow],
    rotation: [Vec3; 3],
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Footprint, FootprintError> {
    let list: Vec<&kf::Shadow> = shadows.iter().map(|s| &s.0).collect();
    Ok(Footprint::of(&kf::footprint_of(
        &list,
        turn(rotation),
        cancel,
    )?))
}

/// A plane the body has planar faces in (the kernel's [`kr::FlatSide`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatSide {
    /// Out of the material, unit length.
    pub normal: Vec3,
    /// `normal · p` for a point `p` of the plane.
    pub offset: f64,
    /// The area of the body's faces in the plane, mm².
    pub area: f64,
    /// The direction of the longest straight edge of those faces, unit length, if they have one.
    pub edge: Option<Vec3>,
}

impl FlatSide {
    fn of(s: &kr::FlatSide) -> FlatSide {
        FlatSide {
            normal: record_vec3(s.normal),
            offset: s.offset,
            area: s.area,
            edge: s.edge.map(record_vec3),
        }
    }
}

/// A planar face flattened in its own frame (the kernel's [`kr::FlatFace`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FlatFace {
    /// The outer ring and holes, in `(e1 · p, e2 · p)`.
    pub shape: Shape2,
    /// The first axis along the face, unit length.
    pub e1: Vec3,
    /// The second axis along the face, unit length.
    pub e2: Vec3,
    /// Out of the material, unit length.
    pub normal: Vec3,
    /// `normal · p` for a point `p` of the face.
    pub height: f64,
    /// How far the flattened rings may stray from the true boundary, mm.
    pub error: f64,
}

/// `body` in the kernel's kinds with the side its shells use each face on (`true` where no
/// shell says), or `None` when its geometry isn't one the kernel's kinds hold.
fn rest_body(body: &Brep) -> Option<(solidcore::hlr::Brep, Vec<bool>)> {
    let b = crate::hlr::kernel_body(body).ok()?;
    let mut forward = vec![true; body.faces.len()];
    for shell in &body.shells {
        if let Shell::Faces { faces, .. } = shell {
            for f in faces {
                if let Some(slot) = usize::try_from(f.face)
                    .ok()
                    .and_then(|i| forward.get_mut(i))
                {
                    *slot = f.forward;
                }
            }
        }
    }
    Some((b, forward))
}

/// [`kr::flat_sides`] of a record body; none when its geometry isn't one the kernel's kinds
/// hold.
#[must_use]
pub fn flat_sides(body: &Brep) -> Vec<FlatSide> {
    let Some((brep, forward)) = rest_body(body) else {
        return Vec::new();
    };
    kr::flat_sides(kr::Body {
        brep: &brep,
        forward: &forward,
    })
    .iter()
    .map(FlatSide::of)
    .collect()
}

/// [`kr::side_at`] of a record body.
#[must_use]
pub fn side_at(body: &Brep, at: Vec3, tol: f64) -> Option<FlatSide> {
    let (brep, forward) = rest_body(body)?;
    kr::side_at(
        kr::Body {
            brep: &brep,
            forward: &forward,
        },
        vec3(at),
        tol,
    )
    .as_ref()
    .map(FlatSide::of)
}

/// [`kr::flat_face_at`] of a record body.
#[must_use]
pub fn flat_face_at(body: &Brep, at: Vec3, tol: f64) -> Option<FlatFace> {
    let (brep, forward) = rest_body(body)?;
    let f = kr::flat_face_at(
        kr::Body {
            brep: &brep,
            forward: &forward,
        },
        vec3(at),
        tol,
    )?;
    Some(FlatFace {
        shape: Shape2::of(&f.shape),
        e1: record_vec3(f.e1),
        e2: record_vec3(f.e2),
        normal: record_vec3(f.normal),
        height: f.height,
        error: f.error,
    })
}

/// No turn: X, Y and Z where they are.
pub const IDENTITY: [Vec3; 3] = [
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
];

/// [`kl::plane_frame`] on the record's vectors.
#[must_use]
pub fn plane_frame(x: Vec3, y: Vec3) -> Option<[Vec3; 3]> {
    kl::plane_frame(vec3(x), vec3(y)).map(record_turn)
}

/// [`kl::in_frame`] on the record's vectors.
#[must_use]
pub fn in_frame(rows: [Vec3; 3], v: Vec3) -> Vec3 {
    record_vec3(kl::in_frame(turn(rows), vec3(v)))
}

/// [`kl::parallel`] on the record's vectors.
#[must_use]
pub fn parallel(a: Vec3, b: Vec3, tol: f64) -> bool {
    kl::parallel(vec3(a), vec3(b), tol)
}

/// [`kl::turned_about`] on the record's vectors.
#[must_use]
pub fn turned_about(axis: Vec3, angle: f64, v: Vec3) -> Vec3 {
    record_vec3(kl::turned_about(vec3(axis), angle, vec3(v)))
}

/// [`kl::pointing_down`] on the record's vectors.
#[must_use]
pub fn pointing_down(n: Vec3) -> [Vec3; 3] {
    record_turn(kl::pointing_down(vec3(n)))
}

/// [`kl::squared_up`] on the record's vectors.
#[must_use]
pub fn squared_up(rotation: [Vec3; 3], edge: Vec3) -> [Vec3; 3] {
    record_turn(kl::squared_up(turn(rotation), vec3(edge)))
}

/// [`kl::axis_angle`] on the record's vectors.
#[must_use]
pub fn axis_angle(r: [Vec3; 3]) -> (Vec3, f64) {
    let (axis, angle) = kl::axis_angle(turn(r));
    (record_vec3(axis), angle)
}
