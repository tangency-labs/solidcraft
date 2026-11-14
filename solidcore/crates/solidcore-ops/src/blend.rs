//! Fillets and chamfers: each a sequence of the kernel's fillet session calls on edges, and
//! corners, taken by their record index.
//!
//! - [`blend`]: edges rounded (circular, curvature-continuous or chord-sized) or chamfered at
//!   equal distances, with the corners named set back, filled or left open; when some rounds
//!   overflow their faces across cliffs and others fit, the overflowing ones go first. With
//!   record picks ([`blend_ordered`]), two rounds that meet at a sharp side edge are mitred or
//!   swept there ([`Turn`]).
//! - [`chamfer_sides`]: edges chamfered with a distance along a face each, and a second
//!   distance or an angle on the other.
//! - [`fillet_variable`]: one edge rounded with a radius law.
//! - [`edge_faces_along`]: the two faces of an edge with how far each faces a direction, for an
//!   application choosing the face a chamfer's first distance is along.
//! - [`tangent_chain`]: the edges a blend of some edges runs on into tangentially, on the
//!   kernel's measures.
//!
//! The kernel's session is driven as the C API's `round_edges` drove SMLib's executive:
//! the turn, parameters, the set-back vertices, the corners (setbacks set, a chamfer's
//! bevelled), the filleting, then `StitchAndOrient`. The geometry (sizes to offsets, the wedge,
//! the law's curve and its corners, the room) is the kernel's; ops chooses, sequences and maps
//! refusals.
//!
//! Each changes the body it is given, as the kernel does; a refused or failed one leaves it as
//! it came in ([`crate::session`]'s edit wrapper).

use std::collections::{HashMap, HashSet};

use solidcore::blend::band_room::law_fits_band;
use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::blend_refusal::{BlendRefusal, NotBlendable, SetbackTooShortWhy};
use solidcore::blend::chamfer_side::faces_along;
use solidcore::blend::definition_check::BlendFaceLog;
use solidcore::blend::fillet_executive::p3::{
    ErrorInfo, FitKind as KernelFit, FitSize as KernelFitSize, NoRoomReason,
};
use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession};
use solidcore::blend::fillet_executive::FilletLaw;
use solidcore::blend::fillet_geom::p1::{FilletCornerType, FilletErrorType};
use solidcore::blend::fillet_section::section::GN_THUMBWEIGHT_EQUAL_AREA;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::blend::round_room::rounds_cross;
use solidcore::geom::attribute::{ids, Attribute, AttributeHandle, AttributeHandlers, Behavior};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::point_distance;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::topology::brep::p2::{is_manifold_solid, ArenaNotifyState};
use solidcore::topology::brep::p3::{brep_copy, delete_face, DeleteFaceOptions};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2::outer_faceuse;
use solidcore::topology::face::p4 as face4;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{edge, EdgeId, FaceId, VertexId};
use solidcore::{Brep, EntityRef, Kernel, SolidError, Vec3};

use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::session::{answer, measure_body, with_body};
use crate::tangent::Chain;
use crate::{Error, FitKind, Result, RoomReason, Unblendable};

mod room;
pub use room::{band_room, BandRoom, EndRoom, Room};

/// The largest size, setback or radius the operations take.
pub const MAX_SIZE: f64 = 1e6;

/// The stations, less one, at which [`fillet_variable`] has the kernel check a law against
/// its band's room.
const LAW_STATIONS: usize = 32;

/// The most control radii a radius law takes.
pub const MAX_LAW_RADII: usize = 100;

/// What [`blend`] makes along each edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// A rolling ball's round (circular section), the size its radius.
    Round,
    /// The curvature-continuous round: the Gⁿ section of order 2 at the equal-area thumbweight,
    /// the size the radius of the ball whose area it takes off.
    Curvature,
    /// A circular round sized by the width of its chord.
    Chord,
    /// A chamfer at equal distances from the edge along both faces, the size the distance; its
    /// corners are bevelled.
    Chamfer,
}

/// What [`blend`] does with the corners it sets back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    /// Filled, as the kernel builds them.
    Filled,
    /// Removed: each set-back corner's patch taken off, its hole returned ([`Hole`]).
    Open,
}

/// How two rounds of [`blend_ordered`] turn where they meet at a sharp side edge on a face
/// they share (a boss's base at its upright edges, a pocket's floor at its corners): the blend
/// corner's Turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Turn {
    /// The rounds run on to where they cross and meet there in a crease.
    #[default]
    Mitre,
    /// Each round ends on its section through the side edge, and the corner's face is that
    /// section revolved about the side edge, tangent to both rounds: between planes at a
    /// straight side edge, a torus. For [`Blend::Round`] only.
    Swept,
}

/// How chamfers close a corner where every edge at a vertex is chamfered between planes (the
/// kernel's blend corner, `solidcore::blend::blend_corner::chamfer`, under the kernel's
/// `fillet_chamfer_corner` fix).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChamferCorner {
    /// The chamfer planes run on until they meet: what ops builds without asking (a corner the
    /// session is not told a type for), so a design saved without one keeps its corners.
    #[default]
    Mitre,
    /// The corner is cut off by the plane through the points where the chamfers' rails cross:
    /// a flat facet, a triangle where three chamfers meet. Four or more whose crossings are not
    /// in one plane close as the kernel's corner patch.
    Facet,
}

/// How [`blend_ordered`]'s blends close where they meet; the default is what ops builds without
/// asking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlendOptions {
    /// How two rounds turn where they meet at a sharp side edge on a face they share.
    pub turn: Turn,
    /// How chamfers close a corner where every edge is chamfered (for [`Blend::Chamfer`]).
    pub chamfer_corner: ChamferCorner,
}

/// A corner set back: its vertex and the setback along each of its edges.
/// The vertex indexes the native list for [`blend`], or the supplied record order for
/// [`blend_ordered`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetBack {
    /// The corner's vertex.
    pub vertex: usize,
    /// How far back along each of its edges the rounds stop.
    pub distance: f64,
}

/// One edge round an opened corner's hole: the face beside it outside the hole, and whether
/// that face's boundary, run counter-clockwise about its outward normal, runs the edge from
/// its start vertex to its end. Indices are the result's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoleEdge {
    /// The edge.
    pub edge: usize,
    /// The face beside it, outside the hole.
    pub face: usize,
    /// Whether that face runs the edge from its start to its end.
    pub forward: bool,
}

/// An opened corner's hole: its edges in order, each starting where the one before ends.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Hole {
    /// The edges round the hole.
    pub edges: Vec<HoleEdge>,
}

/// What [`blend`] reports besides the blended body.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Blended {
    /// Per edge asked, whether a larger round that overflowed its faces and went first removed
    /// it whole (nothing of it was left to round).
    pub dropped: Vec<bool>,
    /// With [`Corners::Open`], each set-back corner's hole, in the setbacks' order (none
    /// otherwise).
    pub holes: Vec<Hole>,
    /// The faces the blend made (its rounds', chamfers' and corners'), by index in the result's
    /// native face list, as the kernel's fillet sessions mark them; every other face of the
    /// result is a face of the body, kept or cut back.
    pub made: Vec<usize>,
}

/// The second size of [`chamfer_sides`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSecond {
    /// The distance back along the other face.
    Distance(f64),
    /// The chamfer's angle to the first face, in radians, between 0 and pi.
    Angle(f64),
}

/// A size, setback or radius: finite, positive, at most [`MAX_SIZE`].
fn size_ok(x: f64) -> bool {
    x.is_finite() && x > 0.0 && x <= MAX_SIZE
}

/// The entity of `list` that record index `i` names, each once in `out`. The kernel's own
/// order until the record's canonical order (the conversion's `Order`) takes its place.
fn each_once<T: Copy + PartialEq>(list: &[T], indices: &[usize]) -> Result<Vec<T>> {
    let mut out: Vec<T> = Vec::new();
    for &i in indices {
        let x = list.get(i).copied().ok_or(Error::Argument)?;
        if out.contains(&x) {
            return Err(Error::Argument);
        }
        out.push(x);
    }
    Ok(out)
}

/// A refusal of the session, or of one set-back corner it does not set back (its index among
/// the setbacks).
enum Refused {
    Corner(usize),
    Error(Error),
}

impl From<Error> for Refused {
    fn from(e: Error) -> Refused {
        Refused::Error(e)
    }
}

/// What the session makes along the edges, with each edge's size.
#[derive(Clone, Copy)]
enum Section<'a> {
    Round(&'a [f64]),
    Curvature(&'a [f64]),
    Chord(&'a [f64]),
    /// A radius law; the size is its larger end radius.
    Variable(f64),
    /// Chamfers by their sizes, each first size along the face named (empty: the blend edge
    /// use's).
    Chamfer {
        sizes: &'a [ChamferSize],
        along: &'a [FaceId],
    },
}

impl Section<'_> {
    const fn chamfer(self) -> bool {
        matches!(self, Section::Chamfer { .. })
    }
}

/// The session's overflow-first switches ([`blend_edges`]): with them it stops, nothing
/// built, when blends of several edges overflow their faces across cliffs (or, `classify_only`,
/// always), and reports the overflowing edges and the mitred pairs.
#[derive(Clone, Copy)]
struct Overflow {
    classify_only: bool,
}

/// Why a stopped overflow-first call stopped.
#[derive(Default)]
struct Stopped {
    /// The edges whose blends overflow, when the session stopped for them.
    overflow: Option<Vec<EdgeId>>,
    /// The edges of each two rounds mitred at a face's concave corner, two by two.
    mitred: Vec<EdgeId>,
}

/// The kernel's reason for a blend too large, as ops names it; `None` for the reasons it does
/// not name (the cliff overflow's own refusals).
fn room_reason(r: NoRoomReason) -> Option<RoomReason> {
    use RoomReason as R;
    Some(match r {
        NoRoomReason::BallDoesNotFit => R::BallDoesNotFit,
        // the crease trim's refusal: the blend runs off its face across a tangent edge onto
        // the face beyond, which would need a crease
        NoRoomReason::RailLeavesFace
        | NoRoomReason::CreaseAcrossTangentEdge
        | NoRoomReason::ConcaveOverflow
        | NoRoomReason::OverflowWithConcaveBlend
        | NoRoomReason::OverflowWithRollover
        | NoRoomReason::OverflowWithGlobalMerge => R::RailLeavesFace,
        NoRoomReason::FaceConsumed => R::FaceConsumed,
        NoRoomReason::BandsOverlap => R::BandsOverlap,
        NoRoomReason::BallsFold => R::BallsFold,
        NoRoomReason::OffsetFolds => R::OffsetFolds,
        NoRoomReason::CutsThePart => R::CutsThePart,
        NoRoomReason::BreaksThrough => R::BreaksThrough,
        NoRoomReason::OffsetCollapses => R::OffsetCollapses,
        NoRoomReason::CornersTakeWholeEdge => R::CornersTakeWholeEdge,
        NoRoomReason::RoundIntoMaterial | NoRoomReason::RailRunsPastFaceIntoMaterial => {
            R::RunsIntoMaterial
        }
        _ => return None,
    })
}

/// Without `fixes.fillet_cliff_overflow` the kernel refuses every blend whose rail leaves its
/// face, where with it it decides between building it over the cliff and refusing it.
const RAIL_LEAVES_FACE: &str =
    "a blend whose rail leaves its face, refused without the cliff overflow fix";

/// What a fillet session was doing when it failed, for [`session_refusal`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionCall {
    /// The corners step noted a vertex no corner kind handles (`SM_FILERR_CORNER_UNSUPPORTED`
    /// in the session's error info after `create_corners` succeeded): the corner it could not
    /// make stands for a later step's failure, other than one the kernel itself types as not
    /// supported or not ported (which is its own cause).
    pub corner: bool,
    /// The session marks the set-back corners' patches for another patch to replace
    /// (`set_mark_corner_patches`): the kernel's fill is then not the corner, so a setback it
    /// names is not one shown to build.
    pub patches: bool,
    /// The session blends chamfers: a fit of an equal chamfer's offset is given as its distance.
    pub chamfer: bool,
}

/// A refusal of a fillet session ([`session_refusal`]): exactly the refusals it gives, each
/// the [`Error`] variant of the same name and fields ([`From`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// [`Error::TooLarge`]: the largest size that builds, what it sizes, a point at the
    /// middle of the edge it was found for, and why there is no room.
    TooLarge {
        /// The largest size that builds it.
        size: f64,
        /// What the size sizes.
        kind: FitKind,
        /// A point at the middle of the edge it was found for.
        at: Option<[f64; 3]>,
        /// Why there is no room.
        why: Option<RoomReason>,
    },
    /// [`Error::SetbackTooShort`].
    SetbackTooShort {
        /// The corner's vertex index; `None` where the vertex is no longer the body's.
        vertex: Option<usize>,
        /// The least setback.
        least: f64,
        /// The corner's point, where the kernel's session notes it.
        at: Option<[f64; 3]>,
        /// Whether a setback of `least` builds the corner.
        builds: bool,
    },
    /// [`Error::NoRoom`]: larger than its faces allow, unsized.
    NoRoom,
    /// [`Error::CornerUnsupported`]: a vertex no corner kind handles.
    CornerUnsupported,
    /// [`Error::Unsupported`]: a blend whose rail leaves its face, refused without the cliff
    /// overflow fix (its reason).
    RailLeavesFace(&'static str),
}

impl From<Refusal> for Error {
    fn from(r: Refusal) -> Error {
        match r {
            Refusal::TooLarge {
                size,
                kind,
                at,
                why,
            } => Error::TooLarge {
                size,
                kind,
                at,
                why,
            },
            Refusal::SetbackTooShort {
                vertex,
                least,
                at,
                builds,
            } => Error::SetbackTooShort {
                vertex,
                least,
                at,
                builds,
            },
            Refusal::NoRoom => Error::NoRoom,
            Refusal::CornerUnsupported => Error::CornerUnsupported,
            Refusal::RailLeavesFace(why) => Error::Unsupported(why),
        }
    }
}

/// A refusal of a fillet session ([`session_refusal`]), with the edge a sized refusal was
/// found for, until [`SessionRefusal::on`] reads that edge's point.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRefusal {
    refusal: Refusal,
    edge: Option<EdgeId>,
}

impl SessionRefusal {
    /// The refusal, a [`Refusal::TooLarge`] given the point at the middle of the edge its size
    /// was found for, read on `b` once the session has let it go. An edge or point that cannot
    /// be read leaves the size without a point.
    ///
    /// # Errors
    /// The kernel's failure to evaluate the edge's curve.
    pub fn on(self, k: &Kernel, b: &Brep) -> solidcore::Result<Refusal> {
        Ok(match (self.refusal, self.edge) {
            (
                Refusal::TooLarge {
                    size, kind, why, ..
                },
                Some(edge),
            ) => Refusal::TooLarge {
                size,
                kind,
                at: edge_point(k, b, edge)?,
                why,
            },
            (r, _) => r,
        })
    }
}

/// A failure `e` of the kernel's fillet session `s` (its preparation, corners or filleting) as
/// the refusal it stands for, for an application that drives the session itself: one
/// implementation of the refusal for ops and its callers.
///
/// - A setback too short (the kernel's `BlendRefusal::SetbackTooShort`) is
///   [`Error::SetbackTooShort`] with the least setback, the corner's vertex and point, and
///   whether that setback builds: only where the corner fill's convex or concave test named
///   it and no other patch replaces the fill (`call.patches`). A setback short of where the
///   rails cross names one that passes that test only, so it does not build.
/// - A failure the kernel itself types as not supported or not ported
///   (`SolidError::Unsupported`, `SolidError::Unported`) is its own cause: it is given back
///   (below), whatever an earlier step of the session noted (no room, a fit, an unsupported
///   vertex), since a size or corner noted before it is not what failed.
/// - A blend too large that the kernel sizes (`SolidError::FilletRefused`, or the session's
///   fit under `SM_FILERR_NO_ROOM`) is [`Error::TooLarge`] with the size, what it sizes, and
///   the kernel's reason ([`RoomReason`]), unless the corners step noted an unsupported vertex
///   or the rail leaves its face; a chamfer's fit by radius whose offsets are equal as its
///   distance ([`FilletSession::chamfer_fit`], a knife edge's included), or [`Error::NoRoom`]
///   where it cannot be read so.
/// - Otherwise, a vertex no corner kind handles (noted by the corners step, `call.corner`, or
///   by the failing step) is [`Error::CornerUnsupported`]; a blend whose
///   rail leaves its face without the cliff overflow fix is [`Error::Unsupported`]; any other
///   blend too large is [`Error::NoRoom`].
///
/// # Errors
/// `e` back, for a failure that is no refusal of the blend's size or corners: a kernel failure
/// (`SolidError::Unsupported`, `SolidError::Unported`, or a status other than
/// `SM_FILERR_NO_ROOM`), which the caller maps as it maps the kernel's failures.
pub fn session_refusal(
    s: &FilletSession<'_>,
    e: SolidError,
    call: SessionCall,
) -> std::result::Result<SessionRefusal, SolidError> {
    let info = s.error_info();
    if let SolidError::BlendRefused {
        refusal: BlendRefusal::SetbackTooShort { vertex, least, why },
        ..
    } = e
    {
        let fill = matches!(
            why,
            SetbackTooShortWhy::ConvexFill | SetbackTooShortWhy::ConcaveFill
        );
        let vertex = s.target().vertex_list().iter().position(|&x| x == vertex);
        let refusal = Refusal::SetbackTooShort {
            vertex,
            least,
            at: info.location.map(|p| [p.x, p.y, p.z]),
            builds: fill && !call.patches,
        };
        return Ok(SessionRefusal {
            refusal,
            edge: None,
        });
    }
    let (fit, reason) = match &e {
        SolidError::FilletRefused { fit, reason, .. } => {
            (Some(*fit), reason.or(info.no_room_reason()))
        }
        _ if info.code == FilletErrorType::NoRoom as i32 => (s.fit_size(), info.no_room_reason()),
        _ => (None, None),
    };
    if matches!(
        e,
        SolidError::Unsupported { .. } | SolidError::Unported { .. }
    ) {
        return Err(e);
    }
    if let Some(fit) = fit {
        if !call.corner
            && info.no_room_reason() != Some(NoRoomReason::RailLeavesFace)
            && fit.size.is_finite()
            && fit.size > 0.0
        {
            let kind = match fit.kind {
                KernelFit::Radius => FitKind::Radius,
                KernelFit::Scale => FitKind::Scale,
                KernelFit::Chord => FitKind::Chord,
            };
            let why = reason.and_then(room_reason);
            let refusal = Refusal::TooLarge {
                size: fit.size,
                kind,
                at: None,
                why,
            };
            return Ok(SessionRefusal {
                refusal: chamfer_sized(s, call.chamfer, fit.edge, refusal),
                edge: fit.edge,
            });
        }
    }
    blend_failure_unsized(info, e, call.corner).map(|refusal| SessionRefusal {
        refusal,
        edge: None,
    })
}

/// A failure of the session as the operation's: the kernel's refusal of an edge, or of a
/// corner's setback as too long, by its entity's index ([`refused`]), else
/// [`session_refusal`]'s, the kernel's failures mapped by [`status`].
fn blend_failure(
    s: &FilletSession<'_>,
    e: SolidError,
    call: SessionCall,
) -> (Error, Option<EdgeId>) {
    if let SolidError::BlendRefused { refusal, .. } = &e {
        if let Some(error) = refused(s, refusal) {
            return (error, None);
        }
    }
    match session_refusal(s, e, call) {
        Ok(r) => (r.refusal.into(), r.edge),
        Err(e) => (status(e), None),
    }
}

/// The kernel's refusal of an edge, or of a corner's setback as too long, as the operation's,
/// its entities by their indices in the session's body; `None` for an entity that body does
/// not hold, and for a setback too short ([`session_refusal`]'s).
fn refused(s: &FilletSession<'_>, refusal: &BlendRefusal) -> Option<Error> {
    let b = s.target();
    let edge = |e: EdgeId| b.edge_list().iter().position(|&x| x == e);
    let vertex = |v: VertexId| b.vertex_list().iter().position(|&x| x == v);
    Some(match *refusal {
        BlendRefusal::EdgeNotBlendable { edge: e, reason } => Error::EdgeNotBlendable {
            edge: edge(e)?,
            reason: match reason {
                NotBlendable::Smooth => Unblendable::Smooth,
                NotBlendable::NotTwoFaces => Unblendable::NotTwoFaces,
                NotBlendable::Tangent => Unblendable::Tangent,
                NotBlendable::Ambiguous => Unblendable::Ambiguous,
            },
        },
        BlendRefusal::ContinuesInto {
            edge: e,
            continues_into,
            ..
        } => Error::ContinuesInto {
            edge: edge(e)?,
            continues_into: edge(continues_into)?,
        },
        BlendRefusal::RoundTooSmall {
            edge: e,
            turn,
            tolerance,
            smallest_radius,
        } => Error::RoundTooSmall {
            edge: edge(e)?,
            turn,
            tolerance,
            smallest: smallest_radius,
        },
        BlendRefusal::SetbackTooLong {
            vertex: v, largest, ..
        } => Error::SetbackTooLong {
            vertex: vertex(v)?,
            largest,
        },
        BlendRefusal::ChamferAngleTooLarge {
            edge: e,
            angle,
            largest,
        } => Error::ChamferAngleTooLarge {
            edge: edge(e)?,
            angle,
            largest,
        },
        _ => return None,
    })
}

/// [`session_refusal`] where the kernel gives no size; `e` back for a failure that is not
/// the blend's.
fn blend_failure_unsized(
    info: &ErrorInfo<FilletObject>,
    e: SolidError,
    corner: bool,
) -> std::result::Result<Refusal, SolidError> {
    if corner || info.code == FilletErrorType::CornerUnsupported as i32 {
        Ok(Refusal::CornerUnsupported)
    } else if info.code != FilletErrorType::NoRoom as i32 {
        Err(e)
    } else if info.no_room_reason() == Some(NoRoomReason::RailLeavesFace) {
        Ok(Refusal::RailLeavesFace(RAIL_LEAVES_FACE))
    } else {
        Ok(Refusal::NoRoom)
    }
}

/// A chamfer's fit by radius ([`FitKind::Radius`], an equal chamfer's one offset) as the
/// distance that builds, which the kernel reads through the chamfer's definition
/// ([`FilletSession::chamfer_fit`]); one it cannot read so is [`Refusal::NoRoom`]. Any other
/// fit, or a blend that is no chamfer, as it is.
fn chamfer_sized(
    s: &FilletSession<'_>,
    chamfer: bool,
    edge: Option<EdgeId>,
    e: Refusal,
) -> Refusal {
    match e {
        Refusal::TooLarge {
            size,
            kind: FitKind::Radius,
            at,
            why,
        } if chamfer => {
            let fit = KernelFitSize {
                size,
                edge,
                kind: KernelFit::Radius,
            };
            match s.chamfer_fit(fit) {
                Some(size) => Refusal::TooLarge {
                    size,
                    kind: FitKind::Distance,
                    at,
                    why,
                },
                None => Refusal::NoRoom,
            }
        }
        e => e,
    }
}

/// `e` ([`Error::TooLarge`]) with the point at the middle of `edge`, the edge its size was
/// found for. An edge or point that cannot be read leaves the size without a point.
fn sized_on(k: &Kernel, b: &Brep, edge: Option<EdgeId>, e: Error) -> solidcore::Result<Error> {
    let Error::TooLarge {
        size, kind, why, ..
    } = e
    else {
        return Ok(e);
    };
    let Some(edge) = edge else {
        return Ok(e);
    };
    Ok(Error::TooLarge {
        size,
        kind,
        at: edge_point(k, b, edge)?,
        why,
    })
}

/// The point at the middle of `edge` of `b` (`half_way` along its interval); `None` for an
/// edge or curve that cannot be read.
fn edge_point(k: &Kernel, b: &Brep, edge: EdgeId) -> solidcore::Result<Option<[f64; 3]>> {
    Ok(match b.edge(edge) {
        Ok(rec) => match rec.curve.as_ref() {
            Some(c) => {
                answer(c.evaluate_point(k, half_way(rec.interval.min(), rec.interval.max())))?
                    .map(|p| [p.x, p.y, p.z])
            }
            None => None,
        },
        Err(_) => None,
    })
}

/// The parameter half way between `lo` and `hi`: the sample position ops chooses.
#[allow(
    clippy::float_arithmetic,
    reason = "a sample parameter ops chooses, not a geometric quantity (docs/layers.md)"
)]
pub(crate) fn middle(lo: f64, hi: f64) -> f64 {
    0.5 * (lo + hi)
}

/// The parameter half way along `[lo, hi]` as the kernel's interval evaluates it
/// (`Extent1d::evaluate(0.5)`: `lo + 0.5 (hi - lo)`), the point a sized refusal names, as
/// SolidCraft named it before its refusals were ops'.
#[allow(
    clippy::float_arithmetic,
    reason = "a sample parameter ops chooses, not a geometric quantity (docs/layers.md)"
)]
fn half_way(lo: f64, hi: f64) -> f64 {
    lo + 0.5 * (hi - lo)
}

/// The faces of `b`.
fn face_set(b: &Brep) -> HashSet<FaceId> {
    b.face_list().iter().copied().collect()
}

/// Session preparation: the edges' sections and sizes (`SetFilletParameters`), as the C API's
/// `round_edges` and `chamfer_offsets` set them.
fn prepare(
    k: &mut Kernel,
    s: &mut FilletSession<'_>,
    edges: &[EdgeId],
    section: Section<'_>,
    law: Option<FilletLaw>,
) -> solidcore::Result<()> {
    let circular = [FilletSurfaceGeneratorType::Circular];
    match (section, law) {
        (Section::Round(sizes), _) => {
            s.prepare_per_edge_constant_radius(k, edges, sizes, &circular, 1, 1.0)
        }
        (Section::Variable(size), Some(law)) => {
            s.prepare_variable_radius_edges(k, edges, size, law, 1, 1)
        }
        (Section::Variable(_), None) => Err(SolidError::invalid_input(
            "solidcore-ops blend",
            "a variable round needs its law",
        )),
        // the equal-area thumbweight by the kernel's name, never its value
        (Section::Curvature(sizes), _) => s.prepare_per_edge_constant_radius(
            k,
            edges,
            sizes,
            &[FilletSurfaceGeneratorType::BlendCurve],
            2,
            GN_THUMBWEIGHT_EQUAL_AREA,
        ),
        (Section::Chord(sizes), _) => {
            s.prepare_per_edge_constant_distance(k, edges, sizes, &circular, 1, 1.0)
        }
        (Section::Chamfer { sizes, along }, _) => {
            s.prepare_chamfers_along(k, edges, sizes, along).map(|_| ())
        }
    }
}

/// The corners' step: a chamfer's corners bevelled; each setback set on its vertex's corner,
/// which must be the same-convexity N x N corner or a blend corner whose event is the setback
/// (else that setback's index is refused). The set-back corners' indices among the session's
/// corners, in the setbacks' order.
fn corners(
    s: &mut FilletSession<'_>,
    section: Section<'_>,
    setbacks: &[(VertexId, f64)],
) -> solidcore::Result<std::result::Result<Vec<usize>, Refused>> {
    let ids = s.fillet_corners()?;
    if section.chamfer() {
        for id in ids {
            s.set_corner_bevel(id, true)?;
        }
        return Ok(Ok(Vec::new()));
    }
    let mut set_back = Vec::new();
    for (j, &(v, distance)) in setbacks.iter().enumerate() {
        let mut found = None;
        for (i, &id) in ids.iter().enumerate() {
            if s.corner(id)?.vertex == Some(v) {
                found = Some((i, id));
                break;
            }
        }
        let Some((i, id)) = found else {
            return Ok(Err(Refused::Corner(j)));
        };
        let corner = s.corner(id)?;
        let setback_blend = corner
            .blend
            .is_some_and(|b| b.event == BlendCornerEvent::Setback);
        if corner.corner_type() != FilletCornerType::NByNConvex && !setback_blend {
            return Ok(Err(Refused::Corner(j)));
        }
        s.set_corner_setback(id, distance)?;
        set_back.push(i);
    }
    Ok(Ok(set_back))
}

/// What one pass of the session was asked.
struct Pass<'a> {
    edges: &'a [EdgeId],
    section: Section<'a>,
    law: Option<FilletLaw>,
    setbacks: &'a [(VertexId, f64)],
    /// Faces an earlier pass of the same blend made ([`FilletSession::add_earlier_pass_face`]).
    earlier: &'a [FaceId],
    /// Mark the set-back corners' patches and return them.
    patches: bool,
    overflow: Option<Overflow>,
    /// How two rounds turn at a sharp side edge, set before the corners are made.
    turn: Turn,
    /// How chamfers close their corners, set before the corners are made.
    chamfer_corner: ChamferCorner,
}

/// What one pass made: the faces new after it, and each set-back corner's patch.
struct Made {
    faces: Vec<FaceId>,
    patches: Vec<Vec<FaceId>>,
}

/// One pass of the session on `b`: the turn set, prepared, the set-back vertices named, the
/// earlier pass's faces handed on, the corners made and set, the filleting, then
/// `StitchAndOrient`. A pass that made no face is [`Error::NoEffect`]; a set-back corner whose
/// patch has no face is [`Error::Unsupported`]. With `overflow`, a stop for overflowing blends
/// fills `stopped`.
fn pass(
    k: &mut Kernel,
    b: &mut Brep,
    p: Pass<'_>,
    stopped: &mut Stopped,
) -> solidcore::Result<std::result::Result<Made, Refused>> {
    let before = face_set(b);
    let mut fit_edge = None;
    let done = {
        let mut s = FilletSession::new(k, b)?;
        // read as the corners are made
        s.set_swept_turns(p.turn == Turn::Swept);
        // the Facet asked of the kernel's chamfer corners; the default leaves the session as it
        // always was (no type set), so it builds what ops built before
        if p.chamfer_corner == ChamferCorner::Facet {
            s.set_chamfer_corner(solidcore::blend::blend_corner::BlendChamferCorner::Facet);
        }
        let prepared = prepare(k, &mut s, p.edges, p.section, p.law);
        // a set-back vertex is no Ball: its corner is the one the setback is set on
        if !p.section.chamfer() {
            for &(v, _) in p.setbacks {
                s.add_set_back_vertex(v);
            }
        }
        for &f in p.earlier {
            s.add_earlier_pass_face(f);
        }
        let mut step = prepared
            .and_then(|()| s.create_corners(k, None))
            .map(|()| Ok(Vec::new()));
        // a vertex the corners step made no corner for, noted before a later step notes its
        // own failure over it
        let corner =
            step.is_ok() && s.error_info().code == FilletErrorType::CornerUnsupported as i32;
        s.set_overflow_first(p.overflow.is_some());
        s.set_classify_only(p.overflow.is_some_and(|o| o.classify_only));
        if let Ok(Ok(_)) = step {
            step = corners(&mut s, p.section, p.setbacks);
        }
        s.set_mark_corner_patches(p.patches);
        let call = SessionCall {
            corner,
            patches: p.patches,
            chamfer: p.section.chamfer(),
        };
        match step {
            // the marks go with the session: read them before it drops
            Ok(Ok(set_back)) => match s.do_filleting(k, None) {
                Ok(()) => s.corner_patch_faces().map(|marked| Ok((set_back, marked))),
                Err(e) => {
                    if p.overflow.is_some()
                        && s.error_info().code == FilletErrorType::OverflowFirst as i32
                    {
                        stopped.overflow = Some(s.overflow_edges());
                        stopped.mitred = s.mitred_edges();
                    }
                    let (error, edge) = blend_failure(&s, e, call);
                    fit_edge = edge;
                    Ok(Err(Refused::Error(error)))
                }
            },
            Ok(Err(refusal)) => Ok(Err(refusal)),
            Err(e) => {
                let (error, edge) = blend_failure(&s, e, call);
                fit_edge = edge;
                Ok(Err(Refused::Error(error)))
            }
        }
    };
    let (set_back, marked) = match done? {
        Ok(d) => d,
        Err(refusal) => {
            b.refresh_cached_topology();
            return Ok(Err(match refusal {
                Refused::Error(e @ Error::TooLarge { .. }) => {
                    Refused::Error(sized_on(k, b, fit_edge, e)?)
                }
                r => r,
            }));
        }
    };
    let stitched = stitch_and_orient(k, b, &StitchAndOrientOptions::default());
    // the cached topology, true again for what reads the body next
    b.refresh_cached_topology();
    if let Err(e) = stitched {
        return Ok(Err(Refused::Error(status(e))));
    }
    let faces: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|f| !before.contains(f))
        .collect();
    if faces.is_empty() {
        return Ok(Err(Refused::Error(Error::NoEffect)));
    }
    // the rounds' faces and their neighbours: none may cross its own boundary
    match rounds_cross(k, b, &faces) {
        Ok(()) => {}
        Err(e @ SolidError::RoundRefused { .. }) => {
            return Ok(Err(Refused::Error(room::refusal(b, e, false))))
        }
        Err(e) => return Err(e),
    }
    let mut patches = Vec::new();
    if p.patches {
        // the stitch keeps the faces filleting made: a corner's patch is the faces marked
        // with its index
        let live = b.face_list().to_vec();
        let marked: HashMap<FaceId, usize> = marked.into_iter().map(|(i, f)| (f, i)).collect();
        for &index in &set_back {
            let patch: Vec<FaceId> = live
                .iter()
                .copied()
                .filter(|f| marked.get(f) == Some(&index))
                .collect();
            if patch.is_empty() {
                return Ok(Err(Refused::Error(Error::Unsupported(
                    "a set-back corner whose patch has no face",
                ))));
            }
            patches.push(patch);
        }
        // the marks are the session's: none is left on the body's faces
        for f in live {
            let at = EntityRef::Face(f);
            let marks: Vec<AttributeHandle> = b
                .entity_attributes(at)
                .into_iter()
                .filter(|(_, a)| a.attribute_id() == ids::FILLET_CORNER_FACE)
                .map(|(h, _)| h)
                .collect();
            for h in marks {
                b.remove_attribute(at, h)?;
            }
        }
    }
    Ok(Ok(Made { faces, patches }))
}

/// [`pass`] whose refusals are the operation's (a corner refused is [`Error::CornerRefused`]).
fn pass_or_refuse(k: &mut Kernel, b: &mut Brep, p: Pass<'_>) -> solidcore::Result<Result<Made>> {
    Ok(match pass(k, b, p, &mut Stopped::default())? {
        Ok(made) => Ok(made),
        Err(Refused::Corner(corner)) => Err(Error::CornerRefused { corner }),
        Err(Refused::Error(e)) => Err(e),
    })
}

/// A plain pass of `edges` with `section`, turning as `turn`, handed `earlier`'s faces.
fn together<'a>(
    edges: &'a [EdgeId],
    section: Section<'a>,
    turn: Turn,
    earlier: &'a [FaceId],
) -> Pass<'a> {
    Pass {
        edges,
        section,
        law: None,
        setbacks: &[],
        earlier,
        patches: false,
        overflow: None,
        turn,
        chamfer_corner: ChamferCorner::Mitre,
    }
}

/// Whether the rounds that overflow go first ([`overflow_first`]).
const OVERFLOW_GOES_FIRST: bool = true;

/// The attribute id of [`Picked`]: ops' own, below the applications' (`ids::USER_1` and up,
/// [`crate::provenance::FIRST_ID`]) and above the kernel's.
const PICKED_ID: u64 = ids::USER_1 - 1;

/// Which asked edge an edge stands for, put on the asked edges before an overflowing blend
/// goes first and handed on through every split and copy, so afterwards the edges carrying it
/// are what the overflow left of that edge (none: it removed the edge whole).
#[derive(Debug, Clone, PartialEq)]
struct Picked(usize);

impl AttributeHandlers for Picked {}

/// The attribute [`Picked`] on `e`, with its handle.
fn picked_on(b: &Brep, e: EdgeId) -> Option<(AttributeHandle, usize)> {
    let (h, a) = b
        .entity_attributes(EntityRef::Edge(e))
        .into_iter()
        .find(|(_, a)| a.attribute_id() == PICKED_ID)?;
    a.application_value::<Picked>().map(|p| (h, p.0))
}

/// `b` with [`Picked`] off its edges.
fn untagged(b: &mut Brep) -> solidcore::Result<()> {
    for e in b.edge_list().to_vec() {
        if let Some((h, _)) = picked_on(b, e) {
            b.remove_attribute(EntityRef::Edge(e), h)?;
        }
    }
    Ok(())
}

/// The rounds of [`blend`] when blends of several edges overflow their faces (the first call
/// stopped in `b`, `edges` its edges, `sizes` theirs, `mitred` the mitred pairs it found): the
/// blends that overflow on their own (each edge's alone, classified in `b`, where nothing was
/// built) go first on `fresh` (the untouched input, copied before the first call; a copy keeps
/// the order, so the edge at index i of `b` is at index i of `fresh`), the asked edges tagged
/// ([`Picked`]). Each other asked edge is then what is left of it, the edges carrying its tag,
/// rounded at its size in a second pass that is handed the faces the first made, so it meets
/// the first rounds as one blend. An asked edge with nothing left is `dropped`. Two rounds
/// mitred at a face's concave corner go first together. Where none overflows on its own, or
/// all do, the edges are rounded together as before. Every pass turns as `turn`. The result is
/// `fresh`, untagged.
#[allow(clippy::too_many_arguments)] // the C API's overflow_first
fn overflow_first(
    k: &mut Kernel,
    b: &mut Brep,
    mut fresh: Brep,
    edges: &[EdgeId],
    sizes: &[f64],
    section: fn(&[f64]) -> Section<'_>,
    turn: Turn,
    mitred: &[EdgeId],
    dropped: &mut [bool],
) -> solidcore::Result<Result<Brep>> {
    let order = b.edge_list().to_vec();
    let fresh_list = fresh.edge_list().to_vec();
    let mut again = Vec::new();
    for e in edges {
        let at = order.iter().position(|x| x == e);
        match at.and_then(|i| fresh_list.get(i)) {
            Some(&to) => again.push(to),
            None => return Ok(Err(Error::Exception)),
        }
    }
    let mut goes_first = vec![false; edges.len()];
    for (i, ((&e, &to), &size)) in edges.iter().zip(&again).zip(sizes).enumerate() {
        let mut stopped = Stopped::default();
        let alone = Pass {
            edges: &[e],
            section: section(std::slice::from_ref(&size)),
            law: None,
            setbacks: &[],
            earlier: &[],
            patches: false,
            overflow: Some(Overflow {
                classify_only: true,
            }),
            turn,
            chamfer_corner: ChamferCorner::Mitre,
        };
        match pass(k, b, alone, &mut stopped)? {
            Err(Refused::Error(why)) if stopped.overflow.is_none() => return Ok(Err(why)),
            Err(Refused::Corner(_)) => {
                return Ok(Err(Error::Unsupported(
                    "an overflowing round refused at a corner it was not asked to set back",
                )))
            }
            // a classification builds nothing: a built one is the kernel's inconsistency
            Ok(_) => return Ok(Err(Error::Exception)),
            Err(Refused::Error(_)) => {}
        }
        fresh.add_attribute(
            EntityRef::Edge(to),
            Attribute::application(PICKED_ID, Behavior::Copy, Picked(i)),
        )?;
        if let Some(flag) = goes_first.get_mut(i) {
            *flag = stopped.overflow.is_some_and(|over| !over.is_empty());
        }
    }
    // a round mitred to one that goes first goes with it, through every mitre
    let place = |e: &EdgeId| edges.iter().position(|p| p == e);
    // two by two (an odd last one is no pair)
    let (mitred_pairs, _) = mitred.as_chunks::<2>();
    let pairs: Vec<(usize, usize)> = mitred_pairs
        .iter()
        .filter_map(|[a, b]| Some((place(a)?, place(b)?)))
        .collect();
    let mut grew = true;
    while grew {
        grew = false;
        for &(a, b) in &pairs {
            if goes_first.get(a) != goes_first.get(b) {
                for i in [a, b] {
                    if let Some(flag) = goes_first.get_mut(i) {
                        *flag = true;
                    }
                }
                grew = true;
            }
        }
    }
    let (mut lead, mut lead_sizes, mut rest) = (Vec::new(), Vec::new(), Vec::new());
    for (i, ((&to, &size), &first)) in again.iter().zip(sizes).zip(&goes_first).enumerate() {
        if first {
            lead.push(to);
            lead_sizes.push(size);
        } else {
            rest.push(i);
        }
    }
    if lead.is_empty() || rest.is_empty() {
        // a single pass: its rounds go together
        let sizes: Vec<f64> = sizes.to_vec();
        let one = pass_or_refuse(k, &mut fresh, together(&again, section(&sizes), turn, &[]))?;
        if let Err(e) = one {
            return Ok(Err(e));
        }
        untagged(&mut fresh)?;
        return Ok(Ok(fresh));
    }
    let lead_pass = together(&lead, section(&lead_sizes), turn, &[]);
    let first = match pass_or_refuse(k, &mut fresh, lead_pass)? {
        Ok(made) => made.faces,
        Err(e) => return Ok(Err(e)),
    };
    let (mut rest_edges, mut rest_sizes) = (Vec::new(), Vec::new());
    for i in rest {
        let mut left = false;
        for e in fresh.edge_list().to_vec() {
            if picked_on(&fresh, e).is_some_and(|(_, p)| p == i) {
                rest_edges.push(e);
                rest_sizes.push(sizes.get(i).copied().unwrap_or(0.0));
                left = true;
            }
        }
        if !left {
            if let Some(flag) = dropped.get_mut(i) {
                *flag = true;
            }
        }
    }
    if !rest_edges.is_empty() {
        // the second pass meets the first rounds as one blend: it is handed their faces
        let second = together(&rest_edges, section(&rest_sizes), turn, &first);
        if let Err(e) = pass_or_refuse(k, &mut fresh, second)? {
            return Ok(Err(e));
        }
    }
    untagged(&mut fresh)?;
    Ok(Ok(fresh))
}

/// A kernel error as ops' blend failure, for a caller that runs the kernel's blend itself: a
/// failure the kernel states in words where it replaced a bare status (its `BlendFailure`), as
/// [`Error::BlendFailed`] with its class, its status and the kernel's statement; `None` for any
/// other error.
#[must_use]
pub fn failure(e: &SolidError) -> Option<Error> {
    let b = e.blend_failure().filter(|b| b.replaces_upstream_status())?;
    Some(Error::BlendFailed {
        class: b.class(),
        status: e.sm_status().unwrap_or(solidcore::error::SM_ERR),
        phrase: b.to_string(),
    })
}

/// The edges of `body` at native-list indices `edges` blended by `kind`, edge i at `sizes[i]` (or
/// `sizes[0]` for every edge), the corners at `setbacks` set back (not for a chamfer, whose
/// corners are bevelled) and, with [`Corners::Open`], left open. When some of the rounds
/// overflow their faces across cliffs and others fit (more than one edge, no setbacks, corners
/// filled), the overflowing ones go first and the others then round what is left of their
/// edges, meeting them as one blend; an edge the overflow removed whole is reported dropped.
/// A solid gives a solid. Two rounds that meet at a sharp side edge on a face they share turn
/// mitred ([`blend_ordered`] takes the [`Turn`]).
///
/// # Errors
/// [`Error::Argument`] for no edges, an index past its list or repeated, sizes neither one nor
/// one per edge, a size or setback not finite, positive and at most [`MAX_SIZE`];
/// [`Error::Unsupported`] for setbacks on a chamfer; [`Error::SetbackTooLong`] for a setback that
/// does not fit its corner's edges (the kernel's, naming the largest that does);
/// [`Error::NoRoom`] for a blend larger than its faces allow; [`Error::TooLarge`] with the
/// largest size that builds, where the kernel finds it (a chamfer's as its distance);
/// [`Error::CornerRefused`] for a setback at a vertex the kernel does not set back (its index
/// among `setbacks`);
/// [`Error::CornerUnsupported`] for a vertex no corner kind handles; [`Error::EdgeNotBlendable`]
/// for an edge with no blend; [`Error::RoundTooSmall`] for a round within its edge's tolerance;
/// [`Error::ContinuesInto`] for a blend that cannot end where its edge runs on tangentially;
/// [`Error::NoEffect`] for a blend that made no face; [`Error::RoundsCross`] for blends that
/// leave a face whose boundary crosses itself (the kernel's `rounds_cross`, on the blends' faces
/// and their neighbours); with [`Corners::Open`], [`Error::NotSolid`] for a body that is no
/// closed solid once blended and [`Error::Unsupported`] for a corner patch that is not a disc
/// between faces outside every patch; the kernel's other refusals; those of [`finish`].
pub fn blend(
    body: &Body,
    kind: Blend,
    edges: &[usize],
    sizes: &[f64],
    setbacks: &[SetBack],
    corners: Corners,
) -> Result<Blended> {
    blend_impl(
        body,
        None,
        kind,
        edges,
        sizes,
        setbacks,
        corners,
        BlendOptions::default(),
    )
}

/// Blend edges and setback vertices selected through the input record's [`crate::Order`].
///
/// `edges` indexes `order.edges`; each [`SetBack::vertex`] indexes `order.vertices`.
/// The order must belong to `body` (or have been mapped through its copy). Construction,
/// sizes, corner handling and refusals are those of [`blend`]; two rounds that meet at a sharp
/// side edge on a face they share turn as `options.turn` says, and chamfers close a corner
/// where every edge is chamfered as `options.chamfer_corner` says. Returned hole indices, when
/// corners are open, refer to the result's native lists, as in [`blend`].
///
/// # Errors
/// The errors of [`blend`], including [`Error::Argument`] for an absent index or an entity
/// in the supplied order that does not belong to the body, for [`Turn::Swept`] with a blend
/// other than [`Blend::Round`], and for [`ChamferCorner::Facet`] with one other than
/// [`Blend::Chamfer`]; [`Error::Unsupported`] for the Facet without the kernel's
/// `fillet_chamfer_corner` fix. A swept turn the kernel does not build is refused as it
/// classifies it: [`Error::NoRoom`] at a sharp concave side edge (no radius of the rounds
/// passes it), [`Error::CornerUnsupported`] where the blend corner does not build one yet.
pub fn blend_ordered(
    body: &Body,
    order: &crate::Order,
    kind: Blend,
    edges: &[usize],
    sizes: &[f64],
    setbacks: &[SetBack],
    corners: Corners,
    options: BlendOptions,
) -> Result<Blended> {
    blend_impl(
        body,
        Some(order),
        kind,
        edges,
        sizes,
        setbacks,
        corners,
        options,
    )
}

/// [`blend`] and [`blend_ordered`]: the edges and setback vertices picked natively or through
/// `order`, blended with every pass turning and chamfers closing as `options` says.
fn blend_impl(
    body: &Body,
    order: Option<&crate::Order>,
    kind: Blend,
    edges: &[usize],
    sizes: &[f64],
    setbacks: &[SetBack],
    corners: Corners,
    options: BlendOptions,
) -> Result<Blended> {
    let turn = options.turn;
    let one_size = sizes.len() == 1;
    if edges.is_empty()
        || !(one_size || sizes.len() == edges.len())
        || !sizes.iter().all(|&s| size_ok(s))
        || !setbacks.iter().all(|s| size_ok(s.distance))
        || (turn == Turn::Swept && kind != Blend::Round)
        || (options.chamfer_corner == ChamferCorner::Facet && kind != Blend::Chamfer)
    {
        return Err(Error::Argument);
    }
    if kind == Blend::Chamfer && !setbacks.is_empty() {
        return Err(Error::Unsupported(
            "a chamfer's corners are bevelled: they take no setback",
        ));
    }
    let each: Vec<f64> = (0..edges.len())
        .map(|i| {
            sizes
                .get(if one_size { 0 } else { i })
                .copied()
                .unwrap_or(0.0)
        })
        .collect();
    with_body("blend", body, |k, b| {
        // the faces the blend makes, as the kernel's fillet sessions mark them: its log on for
        // this blend, whatever the caller had it at, and put back after
        let outer = k.blend_face_log.replace(BlendFaceLog::default());
        let ran = blend_on_body(k, b, order, kind, edges, &each, setbacks, corners, options);
        let ours = std::mem::replace(&mut k.blend_face_log, outer);
        let made: HashSet<FaceId> = ours
            .iter()
            .flat_map(|log| log.sessions.iter().flatten())
            .map(|m| m.face)
            .collect();
        if let (Some(log), Some(ours)) = (k.blend_face_log.as_mut(), ours) {
            log.sessions.extend(ours.sessions);
        }
        Ok(ran?.map(|mut blended| {
            blended.made = b
                .face_list()
                .iter()
                .enumerate()
                .filter(|(_, f)| made.contains(f))
                .map(|(i, _)| i)
                .collect();
            blended
        }))
    })?
}

/// [`blend_impl`] on the kernel's body: every pass, its corners and refusals, the body
/// finished as the result.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn blend_on_body(
    k: &mut Kernel,
    b: &mut Brep,
    order: Option<&crate::Order>,
    kind: Blend,
    edges: &[usize],
    each: &[f64],
    setbacks: &[SetBack],
    corners: Corners,
    options: BlendOptions,
) -> solidcore::Result<Result<Blended>> {
    let turn = options.turn;
    // the Facet is the kernel's blend corner's, under its fix: refused, not built as the
    // Mitre, without it
    if options.chamfer_corner == ChamferCorner::Facet && !k.fixes.fillet_chamfer_corner {
        return Ok(Err(Error::Unsupported(
            "the chamfered corner needs the kernel's fillet_chamfer_corner fix",
        )));
    }
    let edge_order = order.map_or_else(|| b.edge_list(), |o| o.edges.as_slice());
    let picked = match each_once(edge_order, edges) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    if order.is_some() && picked.iter().any(|e| !b.edge_list().contains(e)) {
        return Ok(Err(Error::Argument));
    }
    let vertices: Vec<usize> = setbacks.iter().map(|s| s.vertex).collect();
    let vertex_order = order.map_or_else(|| b.vertex_list(), |o| o.vertices.as_slice());
    let back: Vec<(VertexId, f64)> = match each_once(vertex_order, &vertices) {
        Ok(vs) => vs
            .into_iter()
            .zip(setbacks)
            .map(|(v, s)| (v, s.distance))
            .collect(),
        Err(e) => return Ok(Err(e)),
    };
    if order.is_some() && back.iter().any(|(v, _)| !b.vertex_list().contains(v)) {
        return Ok(Err(Error::Argument));
    }
    let expect = Expect::of(k, b)?;
    let mut dropped = vec![false; edges.len()];
    let section: fn(&[f64]) -> Section<'_> = match kind {
        Blend::Round => |s| Section::Round(s),
        Blend::Curvature => |s| Section::Curvature(s),
        Blend::Chord => |s| Section::Chord(s),
        Blend::Chamfer => {
            let sizes: Vec<ChamferSize> = each
                .iter()
                .map(|&d| ChamferSize::Distances([d, d]))
                .collect();
            let p = Pass {
                edges: &picked,
                section: Section::Chamfer {
                    sizes: &sizes,
                    along: &[],
                },
                law: None,
                setbacks: &[],
                earlier: &[],
                patches: false,
                overflow: None,
                turn,
                chamfer_corner: options.chamfer_corner,
            };
            if let Err(e) = pass_or_refuse(k, b, p)? {
                return Ok(Err(e));
            }
            return Ok(done(k, b, expect)?.map(|()| Blended {
                dropped,
                holes: Vec::new(),
                made: Vec::new(),
            }));
        }
    };
    let may_go_first =
        OVERFLOW_GOES_FIRST && back.is_empty() && corners == Corners::Filled && picked.len() > 1;
    // the untouched input, for the overflowing rounds to go first on
    let fresh = if may_go_first {
        Some(brep_copy(k, b)?)
    } else {
        None
    };
    let mut stopped = Stopped::default();
    let p = Pass {
        edges: &picked,
        section: section(each),
        law: None,
        setbacks: &back,
        earlier: &[],
        patches: corners == Corners::Open,
        overflow: may_go_first.then_some(Overflow {
            classify_only: false,
        }),
        turn,
        chamfer_corner: ChamferCorner::Mitre,
    };
    let made = pass(k, b, p, &mut stopped)?;
    if stopped.overflow.is_some() {
        let Some(fresh) = fresh else {
            return Ok(Err(Error::Exception));
        };
        let mitred = std::mem::take(&mut stopped.mitred);
        return match overflow_first(
            k,
            b,
            fresh,
            &picked,
            each,
            section,
            turn,
            &mitred,
            &mut dropped,
        )? {
            Ok(made) => {
                *b = made;
                Ok(done(k, b, expect)?.map(|()| Blended {
                    dropped,
                    holes: Vec::new(),
                    made: Vec::new(),
                }))
            }
            Err(e) => Ok(Err(e)),
        };
    }
    let made = match made {
        Ok(made) => made,
        Err(Refused::Corner(corner)) => return Ok(Err(Error::CornerRefused { corner })),
        Err(Refused::Error(e)) => return Ok(Err(e)),
    };
    if corners == Corners::Filled {
        return Ok(done(k, b, expect)?.map(|()| Blended {
            dropped,
            holes: Vec::new(),
            made: Vec::new(),
        }));
    }
    // each corner opened: its patch removed, the edges round its hole in the result's
    // orders
    if !is_manifold_solid(b)? {
        return Ok(Err(Error::NotSolid));
    }
    let rims = match open_corners(k, b, &made.patches)? {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    let edge_index: HashMap<EdgeId, usize> = b
        .edge_list()
        .iter()
        .enumerate()
        .map(|(i, &e)| (e, i))
        .collect();
    let face_index: HashMap<FaceId, usize> = b
        .face_list()
        .iter()
        .enumerate()
        .map(|(i, &f)| (f, i))
        .collect();
    let mut holes = Vec::new();
    for rim in rims {
        let mut hole = Hole::default();
        for h in rim {
            let (Some(&edge), Some(&face)) = (edge_index.get(&h.edge), face_index.get(&h.face))
            else {
                return Ok(Err(Error::Exception));
            };
            hole.edges.push(HoleEdge {
                edge,
                face,
                forward: h.forward,
            });
        }
        holes.push(hole);
    }
    Ok(done(k, b, Expect::FACES)?.map(|()| Blended {
        dropped,
        holes,
        made: Vec::new(),
    }))
}

/// The result contract on the body in place ([`finish`]).
fn done(k: &mut Kernel, b: &mut Brep, expect: Expect) -> solidcore::Result<Result<()>> {
    let made = std::mem::take(b);
    Ok(match finish(k, made, expect)? {
        Ok(r) => {
            *b = r;
            Ok(())
        }
        Err(e) => Err(e),
    })
}

/// One edge round an opened corner's hole, by the kernel's ids.
#[derive(Clone, Copy)]
struct RimEdge {
    edge: EdgeId,
    face: FaceId,
    forward: bool,
}

/// The faces of each patch removed from the solid `b`, with the edges and vertices only they
/// used, and the edges round each patch's hole as the faces beside it run them, each edge's
/// start the previous one's end. Read on the solid before anything is removed, where each
/// face's outward side is known. Each edge round a hole must lie between a face of its patch
/// and one other face, outside every patch, and the edges must close one loop (a patch that
/// is a disc), else [`Error::Unsupported`].
fn open_corners(
    k: &mut Kernel,
    b: &mut Brep,
    patches: &[Vec<FaceId>],
) -> solidcore::Result<Result<Vec<Vec<RimEdge>>>> {
    const NOT_A_DISC: Error =
        Error::Unsupported("an open corner whose patch is not a disc between the faces round it");
    let any: HashSet<FaceId> = patches.iter().flatten().copied().collect();
    let mut holes = Vec::new();
    for patch in patches {
        let mine: HashSet<FaceId> = patch.iter().copied().collect();
        let mut seen = HashSet::new();
        let mut rim: Vec<RimEdge> = Vec::new();
        for &f in patch {
            for e in face4::edges(b, f, None, false, None)? {
                if !seen.insert(e) {
                    continue;
                }
                let beside = edge::faces(b, e, None)?;
                let mut outside = beside.iter().copied().filter(|g| !mine.contains(g));
                let (Some(other), more) = (outside.next(), outside.next()) else {
                    continue;
                };
                if more.is_some() || beside.len() != 2 || any.contains(&other) {
                    return Ok(Err(NOT_A_DISC));
                }
                let Some(fu) = outer_faceuse(b, other)? else {
                    return Ok(Err(NOT_A_DISC));
                };
                let mut used = None;
                for u in edge::edgeuses(b, e, None, None)? {
                    if eu1::faceuse(b, u)? == Some(fu) {
                        used = Some(u);
                    }
                }
                let Some(u) = used else {
                    return Ok(Err(NOT_A_DISC));
                };
                rim.push(RimEdge {
                    edge: e,
                    face: other,
                    forward: b.edgeuse(u)?.orientation == OrientType::Same,
                });
            }
        }
        let ends = |h: &RimEdge| -> solidcore::Result<(Option<VertexId>, Option<VertexId>)> {
            let (s, t) = edge::start_end_vertices(b, h.edge)?;
            Ok(if h.forward { (s, t) } else { (t, s) })
        };
        let Some(&front) = rim.first() else {
            return Ok(Err(NOT_A_DISC));
        };
        let start = ends(&front)?.0;
        let mut used = vec![false; rim.len()];
        if let Some(u) = used.first_mut() {
            *u = true;
        }
        let mut hole = vec![front];
        let mut end = ends(&front)?.1;
        // each turn takes a rim edge not yet in the hole, so it ends within the rim's length
        while end != start {
            let mut next = None;
            for (i, h) in rim.iter().enumerate() {
                if !used.get(i).copied().unwrap_or(true) && ends(h)?.0 == end {
                    if next.is_some() {
                        return Ok(Err(NOT_A_DISC));
                    }
                    next = Some((i, *h));
                }
            }
            let Some((i, h)) = next else {
                return Ok(Err(NOT_A_DISC));
            };
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            hole.push(h);
            end = ends(&h)?.1;
        }
        if hole.len() != rim.len() {
            return Ok(Err(NOT_A_DISC));
        }
        holes.push(hole);
    }
    // each face deleted once, in the patches' order, with the B-rep open for editing
    let mut order = Vec::new();
    let mut taken = HashSet::new();
    for &f in patches.iter().flatten() {
        if taken.insert(f) {
            order.push(f);
        }
    }
    let editing = b.editing_enabled();
    b.set_editing_enabled(true);
    let mut deleted = Ok(());
    {
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        for f in order {
            let options = DeleteFaceOptions {
                delete_edges_vertices: true,
                ..DeleteFaceOptions::default()
            };
            if let Err(e) = delete_face(k, b, &mut env, &mut marks, f, options) {
                deleted = Err(e);
                break;
            }
        }
    }
    b.set_editing_enabled(editing);
    b.refresh_cached_topology();
    Ok(match deleted {
        Ok(()) => Ok(holes),
        Err(e) => Err(status(e)),
    })
}

/// The two faces of `body`'s edge at record index `edge`, each with how far its outward normal
/// at the middle of the edge faces along `dir` (the kernel's
/// [`solidcore::blend::chamfer_side::faces_along`]): for an application choosing the face a
/// chamfer's first distance is along ([`chamfer_sides`]).
///
/// # Errors
/// [`Error::Argument`] for an index past the edges or a direction that is not finite; the
/// kernel's refusal of an edge not between two faces.
pub fn edge_faces_along(body: &Body, edge: usize, dir: [f64; 3]) -> Result<[(usize, f64); 2]> {
    edge_faces_along_impl(body, None, edge, dir)
}

/// [`edge_faces_along`] with the edge and returned faces indexed by `order`.
///
/// # Errors
/// As [`edge_faces_along`] and [`Body::copy_with_order`].
pub fn edge_faces_along_ordered(
    body: &Body,
    order: &crate::Order,
    edge: usize,
    dir: [f64; 3],
) -> Result<[(usize, f64); 2]> {
    edge_faces_along_impl(body, Some(order), edge, dir)
}

fn edge_faces_along_impl(
    body: &Body,
    order: Option<&crate::Order>,
    edge: usize,
    dir: [f64; 3],
) -> Result<[(usize, f64); 2]> {
    if !dir.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    if let Some(order) = order {
        let (copy, mapped) = body.copy_with_order(order)?;
        return with_body("edge_faces_along", &copy, |k, b| {
            faces_along_indices(k, b, Some(&mapped), edge, dir)
        })?;
    }
    measure_body("edge_faces_along", body, |k, b| {
        faces_along_indices(k, b, None, edge, dir)
    })?
}

fn faces_along_indices(
    k: &mut Kernel,
    b: &mut Brep,
    order: Option<&crate::Order>,
    edge: usize,
    dir: [f64; 3],
) -> solidcore::Result<Result<[(usize, f64); 2]>> {
    let edges = order.map_or_else(|| b.edge_list(), |o| o.edges.as_slice());
    let Some(&e) = edges.get(edge) else {
        return Ok(Err(Error::Argument));
    };
    if !b.edge_list().contains(&e) {
        return Ok(Err(Error::Argument));
    }
    let faces = faces_along(k, b, e, Vec3::new(dir[0], dir[1], dir[2]))?;
    // The supplied order is mapped through the measurement copy.
    let face_order = order.map_or_else(|| b.face_list(), |o| o.faces.as_slice());
    let index = |f: FaceId| face_order.iter().position(|&g| g == f);
    let [(f0, l0), (f1, l1)] = faces;
    Ok(match (index(f0), index(f1)) {
        (Some(i0), Some(i1)) => Ok([(i0, l0), (i1, l1)]),
        _ => Err(Error::Exception),
    })
}

/// A flat face of a concave straight edge that a round can be carried on past, with the slab
/// that carries it on (the kernel's [`solidcore::blend::carried_face::CarriedFace`], its face
/// by index).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarriedFace {
    /// The flat face's index.
    pub face: usize,
    /// The edge's start vertex: the origin of the slab's frame.
    pub origin: [f64; 3],
    /// The unit direction of the edge.
    pub along: [f64; 3],
    /// The unit direction in the face's plane square to the edge; `along` × `across` is the
    /// face's outward normal.
    pub across: [f64; 3],
    /// The slab's rectangle in (`along`, `across`) from `origin`.
    pub outline: [[f64; 2]; 4],
    /// The slab's extent along the outward normal, up to the face's plane.
    pub depth: [f64; 2],
    /// How far to either side of its plane the face's outline is to be carried.
    pub reach: f64,
    /// Which way along `across` from the edge's line the other face's material stands, in the
    /// face's plane: `1.0` or `-1.0`.
    pub other_side: f64,
}

/// The flat faces of `body`'s edge at index `edge` that a round of radius `radius` can be
/// carried on past (the kernel's [`solidcore::blend::carried_face::carried_faces`]): for an
/// application building a round wider than the flat face beside its edge, by rounding the
/// edge on the body with that face carried on as a slab and keeping what the round adds
/// within the face's outline. None where that does not give the round of an unbounded face:
/// the edge is not straight and concave, no face of it is flat, or an end of it has no flat
/// face square to it.
///
/// # Errors
/// [`Error::Argument`] for an index past the edges or a radius not finite, positive and at
/// most [`MAX_SIZE`]; the kernel's refusals.
pub fn carried_faces(body: &Body, edge: usize, radius: f64) -> Result<Vec<CarriedFace>> {
    carried_faces_impl(body, None, edge, radius)
}

/// [`carried_faces`] with the edge and the returned faces indexed by `order`.
///
/// # Errors
/// As [`carried_faces`] and [`Body::copy_with_order`].
pub fn carried_faces_ordered(
    body: &Body,
    order: &crate::Order,
    edge: usize,
    radius: f64,
) -> Result<Vec<CarriedFace>> {
    carried_faces_impl(body, Some(order), edge, radius)
}

fn carried_faces_impl(
    body: &Body,
    order: Option<&crate::Order>,
    edge: usize,
    radius: f64,
) -> Result<Vec<CarriedFace>> {
    if !size_ok(radius) {
        return Err(Error::Argument);
    }
    if let Some(order) = order {
        let (copy, mapped) = body.copy_with_order(order)?;
        return with_body("carried_faces", &copy, |k, b| {
            carried_face_indices(k, b, Some(&mapped), edge, radius)
        })?;
    }
    measure_body("carried_faces", body, |k, b| {
        carried_face_indices(k, b, None, edge, radius)
    })?
}

fn carried_face_indices(
    k: &mut Kernel,
    b: &mut Brep,
    order: Option<&crate::Order>,
    edge: usize,
    radius: f64,
) -> solidcore::Result<Result<Vec<CarriedFace>>> {
    let edges = order.map_or_else(|| b.edge_list(), |o| o.edges.as_slice());
    let Some(&e) = edges.get(edge) else {
        return Ok(Err(Error::Argument));
    };
    if !b.edge_list().contains(&e) {
        return Ok(Err(Error::Argument));
    }
    let found = solidcore::blend::carried_face::carried_faces(k, b, e, radius)?;
    // The supplied order is mapped through the measurement copy.
    let face_order = order.map_or_else(|| b.face_list(), |o| o.faces.as_slice());
    let mut out = Vec::new();
    for c in found {
        let Some(face) = face_order.iter().position(|&g| g == c.face) else {
            return Ok(Err(Error::Exception));
        };
        out.push(CarriedFace {
            face,
            origin: [c.origin.x, c.origin.y, c.origin.z],
            along: [c.along.x, c.along.y, c.along.z],
            across: [c.across.x, c.across.y, c.across.z],
            outline: c.outline,
            depth: c.depth,
            reach: c.reach,
            other_side: c.other_side,
        });
    }
    Ok(Ok(out))
}

/// The tangent chain of the edges of `body` at record indices `edges` (through `order`, its
/// record's [`crate::Order`], or the body's own edge order): those edges (each once, in
/// order), then every edge that continues one of them within `angle` radians at a shared
/// vertex, and so on
/// until no more do (the walk: [`crate::tangent`]). An edge does not join when it is not
/// between two faces, when it is smooth (its faces meet within `angle` all along it, a seam
/// among them), when it turns over (convex where the edge it continues is concave, or the
/// reverse), or when it is in `passed_over`. An edge whose smoothness the kernel leaves
/// undecided joins nothing and is named in [`Chain::undecided`]. Every measure is the kernel's
/// (`query::tangent_edge`, `query::edge_turn`). For a blend that is to run on: the edges to
/// blend together.
///
/// # Errors
/// [`Error::Argument`] for no edges, an index past the edges or naming an edge the body does
/// not have, or an angle not finite and strictly between 0 and a right angle; the kernel's
/// refusals (a face with no NURBS form or in power-basis form, an edge use with no pcurve, a
/// pcurve leaving its face's net); [`Error::Cancelled`] when the session's cancel token is
/// cancelled.
pub fn tangent_chain(
    body: &Body,
    order: Option<&crate::Order>,
    edges: &[usize],
    passed_over: &[usize],
    angle: f64,
) -> Result<Chain> {
    let walk = crate::tangent::edge_walk;
    match order {
        Some(order) => {
            let (copy, mapped) = body.copy_with_order(order)?;
            with_body("tangent_chain", &copy, |k, b| {
                walk(k, b, &mapped.edges, edges, passed_over, angle)
            })?
        }
        None => measure_body("tangent_chain", body, |k, b| {
            // the copy keeps the order: an edge's index in it is its index in the body
            let list = b.edge_list().to_vec();
            walk(k, b, &list, edges, passed_over, angle)
        })?,
    }
}

/// The edges of `body` at record indices `edges` chamfered `distance` back along the face of
/// record index `along[i]` (one of edge i's two faces) and by `second` on the other, the
/// corners bevelled. A solid gives a solid.
///
/// # Errors
/// [`Error::Argument`] for no edges, `along` not one per edge, an index past its list or an
/// edge repeated, a distance not finite, positive and at most [`MAX_SIZE`], or an angle not
/// strictly between 0 and pi; the kernel's refusals ([`Error::Status`] for a face that is not
/// the edge's, faces that make no wedge or distances that fold on it,
/// [`Error::ChamferAngleTooLarge`] for an angle past the wedge's room); [`Error::TooLarge`],
/// [`Error::NoRoom`] and [`Error::NoEffect`] as for [`blend`]; those of [`finish`].
pub fn chamfer_sides(
    body: &Body,
    edges: &[usize],
    along: &[usize],
    distance: f64,
    second: ChamferSecond,
) -> Result<()> {
    chamfer_sides_impl(body, None, edges, along, distance, second)
}

/// [`chamfer_sides`] with edge and face indices taken through `order`.
///
/// # Errors
/// As [`chamfer_sides`]; [`Error::Argument`] for an order naming a missing entity.
pub fn chamfer_sides_ordered(
    body: &Body,
    order: &crate::Order,
    edges: &[usize],
    along: &[usize],
    distance: f64,
    second: ChamferSecond,
) -> Result<()> {
    chamfer_sides_impl(body, Some(order), edges, along, distance, second)
}

fn chamfer_sides_impl(
    body: &Body,
    order: Option<&crate::Order>,
    edges: &[usize],
    along: &[usize],
    distance: f64,
    second: ChamferSecond,
) -> Result<()> {
    let second_ok = match second {
        ChamferSecond::Angle(a) => a.is_finite() && a > 0.0 && a < std::f64::consts::PI,
        ChamferSecond::Distance(d) => size_ok(d),
    };
    if edges.is_empty() || along.len() != edges.len() || !size_ok(distance) || !second_ok {
        return Err(Error::Argument);
    }
    let size = match second {
        ChamferSecond::Distance(d) => ChamferSize::Distances([distance, d]),
        ChamferSecond::Angle(angle) => ChamferSize::DistanceAngle { distance, angle },
    };
    with_body("chamfer_sides", body, |k, b| {
        let edge_order = order.map_or_else(|| b.edge_list(), |o| o.edges.as_slice());
        let picked = match each_once(edge_order, edges) {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        if picked.iter().any(|e| !b.edge_list().contains(e)) {
            return Ok(Err(Error::Argument));
        }
        let face_order = order.map_or_else(|| b.face_list(), |o| o.faces.as_slice());
        let mut faces = Vec::new();
        for &f in along {
            match face_order.get(f) {
                Some(&f) if b.face_list().contains(&f) => faces.push(f),
                _ => return Ok(Err(Error::Argument)),
            }
        }
        let expect = Expect::of(k, b)?;
        let p = Pass {
            edges: &picked,
            section: Section::Chamfer {
                sizes: std::slice::from_ref(&size),
                along: &faces,
            },
            law: None,
            setbacks: &[],
            earlier: &[],
            patches: false,
            overflow: None,
            // a swept turn is a round's: these are chamfers
            turn: Turn::Mitre,
            chamfer_corner: ChamferCorner::Mitre,
        };
        if let Err(e) = pass_or_refuse(k, b, p)? {
            return Ok(Err(e));
        }
        done(k, b, expect)
    })?
}

/// A radius law's data checked: a clamped B-spline of `degree` 1 to 3 from 0 to 1, 2 to
/// [`MAX_LAW_RADII`] control radii, each finite, at least 0 and at most [`MAX_SIZE`].
fn law_ok(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    let n = radii.len();
    if !(1..=3).contains(&degree)
        || !(2..=MAX_LAW_RADII).contains(&n)
        || n <= degree
        || knots.len() != n + degree + 1
    {
        return false;
    }
    if !knots.iter().chain(radii).all(|x| x.is_finite()) {
        return false;
    }
    for (i, &u) in knots.iter().enumerate() {
        let (low_end, high_end) = (i <= degree, i >= n);
        let descends = i > 0 && knots.get(i - 1).is_some_and(|&p| u < p);
        if (low_end && u != 0.0) || (high_end && u != 1.0) || descends {
            return false;
        }
    }
    radii.iter().all(|&r| (0.0..=MAX_SIZE).contains(&r))
}

/// Whether the law's radius is zero somewhere inside (0, 1): a span all of whose control
/// radii are zero, or a knot repeated so that the radius there is a zero control radius.
fn law_pinches(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    let n = radii.len();
    let all_zero = |from: usize, to: usize| -> bool {
        from <= to && (from..=to).all(|j| radii.get(j).is_none_or(|&r| r == 0.0))
    };
    let kn = |i: usize| knots.get(i).copied().unwrap_or(f64::NAN);
    let n_knots = knots.len();
    let mut i = degree;
    while i + 1 < n_knots && i < n {
        if kn(i) < kn(i + 1) && all_zero(i.saturating_sub(degree), i) {
            return true;
        }
        i += 1;
    }
    let mut s = degree + 1;
    while s < n {
        let mut m = 1;
        while s + m < n && kn(s + m) == kn(s) {
            m += 1;
        }
        // s > degree >= 1: s - 1 cannot underflow
        if all_zero((s + m).saturating_sub(1 + degree), s - 1) {
            return true;
        }
        s += m;
    }
    false
}

/// The edge of `body` at native edge-list index `edge` rounded with the radius law of `degree`, `knots`
/// and control radii `radii` (a clamped B-spline from 0 to 1), running from the edge's end at
/// its vertex of native vertex-list index `start`. A solid gives a solid. A law with a corner gives the
/// union of its balls, the round's faces split there (the kernel's law corners).
///
/// # Errors
/// [`Error::Argument`] for a law that is no clamped B-spline of degree 1 to 3 from 0 to 1 with
/// 2 to [`MAX_LAW_RADII`] radii each in [0, [`MAX_SIZE`]], an index past its list, or a
/// `start` that is not a vertex of the edge; [`Error::Unsupported`] for a law whose radius is
/// zero inside the edge; [`Error::RoomRunsOut`] on a straight edge between planes for a law larger
/// than its band's room at a station (the kernel's `law_fits_band`); [`Error::NoRoom`] for a
/// law whose balls fold over one another; the refusals of [`blend`]; those of [`finish`].
pub fn fillet_variable(
    body: &Body,
    edge: usize,
    start: usize,
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<()> {
    fillet_variable_impl(
        body,
        None,
        edge,
        VariableStart::Vertex(start),
        degree,
        knots,
        radii,
    )
}

/// Record edge `edge` of `body`, taken through `order`, rounded with the radius law of
/// [`fillet_variable`], starting at the edge curve's end nearest `start`.
///
/// Distances are measured by the kernel at the edge's parameter-interval ends. A tie starts
/// the law at the interval's first end. Selection and filleting hold the same body lock;
/// neither native list positions nor a vertex index are inferred from the record index.
///
/// # Errors
///
/// Those of [`fillet_variable`]; [`Error::Argument`] for an order naming a missing edge or
/// a non-finite start point; the kernel's refusal of endpoint evaluation or distance.
pub fn fillet_variable_ordered(
    body: &Body,
    order: &crate::Order,
    edge: usize,
    start: [f64; 3],
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<()> {
    fillet_variable_impl(
        body,
        Some(order),
        edge,
        VariableStart::Point(start),
        degree,
        knots,
        radii,
    )
}

#[derive(Clone, Copy)]
enum VariableStart {
    Vertex(usize),
    Point([f64; 3]),
}

fn fillet_variable_impl(
    body: &Body,
    order: Option<&crate::Order>,
    edge: usize,
    start: VariableStart,
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<()> {
    if !law_ok(degree, knots, radii) {
        return Err(Error::Argument);
    }
    if law_pinches(degree, knots, radii) {
        return Err(Error::Unsupported(
            "a radius law that is zero inside its edge",
        ));
    }
    if matches!(start, VariableStart::Point(p) if p.iter().any(|x| !x.is_finite())) {
        return Err(Error::Argument);
    }
    with_body("fillet_variable", body, |k, b| {
        let edges = order.map_or_else(|| b.edge_list(), |o| o.edges.as_slice());
        let Some(&e) = edges.get(edge) else {
            return Ok(Err(Error::Argument));
        };
        if order.is_some() && !b.edge_list().contains(&e) {
            return Ok(Err(Error::Argument));
        }
        let reversed = match start {
            VariableStart::Vertex(start) => {
                let Some(&v) = b.vertex_list().get(start) else {
                    return Ok(Err(Error::Argument));
                };
                let Some(reversed) = room::runs_back(b, e, v)? else {
                    return Ok(Err(Error::Argument));
                };
                reversed
            }
            VariableStart::Point([x, y, z]) => {
                let edge = b.edge(e)?;
                let Some(curve) = edge.curve.as_ref() else {
                    return Ok(Err(Error::Argument));
                };
                let lo = curve.evaluate_point(k, edge.interval.min())?;
                let hi = curve.evaluate_point(k, edge.interval.max())?;
                let point = Vec3::new(x, y, z);
                point_distance(hi, point)?.value < point_distance(lo, point)?.value
            }
        };
        let law = FilletLaw::of_radii(degree, knots, radii, reversed)?;
        // the band's room along a straight edge between planes, at evenly spaced stations
        #[allow(
            clippy::float_arithmetic,
            reason = "the stations' parameters ops chooses, not a geometric quantity (docs/layers.md)"
        )]
        let stations: Vec<f64> = (0..=LAW_STATIONS)
            .map(|i| i as f64 / LAW_STATIONS as f64)
            .collect();
        match law_fits_band(k, b, e, &stations, &law) {
            Ok(()) => {}
            Err(e @ SolidError::RoundRefused { .. }) => {
                return Ok(Err(room::refusal(b, e, reversed)))
            }
            Err(e) => return Err(e),
        }
        let expect = Expect::of(k, b)?;
        let (r0, r1) = (
            radii.first().copied().unwrap_or(0.0),
            radii.last().copied().unwrap_or(0.0),
        );
        let p = Pass {
            edges: &[e],
            section: Section::Variable(if r0 > r1 { r0 } else { r1 }),
            law: Some(law),
            setbacks: &[],
            earlier: &[],
            patches: false,
            overflow: None,
            // one edge: no two rounds meet to turn
            turn: Turn::Mitre,
            chamfer_corner: ChamferCorner::Mitre,
        };
        if let Err(e) = pass_or_refuse(k, b, p)? {
            return Ok(Err(e));
        }
        done(k, b, expect)
    })?
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod overflow_contract_tests;
