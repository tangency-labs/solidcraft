//! Shells, face offsets, thickening and sheet offsets: each a sequence of kernel calls on faces
//! taken by their record index, through the body's record [`Order`] ([`Body::to_record`]).
//!
//! - [`shell`]: the faces named opened, the rest offset into a wall (`SmApiShellBrepFull` with
//!   convex edges extended and the results merged); an inward wall of a solid is refused before
//!   the kernel sees it unless the kernel's room test shows a cavity
//!   ([`solidcore::offset::wall_room::shell_wall_room`]).
//! - [`offset_faces`]: the faces named moved along their outward normals
//!   ([`solidcore::offset::offset_faces::OffsetFacesExecutive`]), the same room test first for a
//!   move inward.
//! - [`thicken`]: a sheet thickened, or moved back by half first.
//! - [`offset_sheet`]: every face of a sheet moved onto its surface's offset
//!   (`SmApiOffsetBrepFull`).
//! - [`draft`]: the faces named turned about their line or circle on a neutral plane
//!   ([`solidcore::offset::draft::draft_surface`]), the faces around them extended or trimmed to
//!   meet them (`SmBrep::LocalOperation`).
//!
//! Each changes the body it is given, as the kernel does; a refused or failed one leaves it as
//! it came in ([`crate::session`]'s edit wrapper).

use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::geom::attribute::{ids, Attribute, Behavior};
use solidcore::offset::draft::{draft_surface, refuse_closing_wall, Draft, DraftRefusal};
use solidcore::offset::offset_executive::p2::{
    local_operation_simple, offset_brep_simple, OffsetOptions, ShellOptions,
};
use solidcore::offset::offset_faces::{OffsetFacesExecutive, OffsetFacesRefusal};
use solidcore::offset::thin_features::shell_brep;
use solidcore::offset::wall_room::{shell_wall_room, WallRoom};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, EntityRef, FaceId, Kernel, SolidError, Vec3};

use crate::body::{finish, Body, Expect};
use crate::error::status as kernel_status;
use crate::order::{self, Order};
use crate::session::{read_body, with_body};
use crate::{Error, OffsetRefusal, RecordEntity, Result, Unsoundness};

/// A face a draft of plane faces would cut through without meeting it (a hole's wall within the
/// draft of the side), for a client that drafts through the kernel's local operation itself and
/// names that face when the operation refuses ([`drafted_through`]).
pub use solidcore::offset::draft::{drafted_through, DraftedThrough};

/// The largest offset distance or wall the operations take.
const MAX_DISTANCE: f64 = 1e6;

/// An offset's kernel error as the operation's: its offset refusal where it is one
/// ([`typed`]), else as [`crate::error::status`].
fn status(e: SolidError, index: &dyn Fn(EntityRef) -> Option<RecordEntity>) -> Error {
    match typed(&e, index) {
        Some(r) => crate::session::noted(&e, r),
        None => kernel_status(e),
    }
}

/// A kernel error as ops' offset refusal, for a caller that holds no [`Order`] of the body (any
/// entity it names is unnamed): what the offset operations give for it ([`typed`]), `None` where
/// they give the kernel's status instead.
#[must_use]
pub fn refusal(e: &SolidError) -> Option<Error> {
    typed(e, &|_| None)
}

/// (#20767) The wall check of a draft of `faces` of `b` by `angle` radians about the neutral
/// plane through `neutral_origin` with normal `neutral_normal`, pulled along `pull`, as
/// [`draft`] runs it before its local operation: a cone or cylinder about the pull whose
/// drafted cone would reach another shell of the body (a cavity's wall) is refused with the
/// largest angle that keeps the wall (the kernel's `OffsetFailure::DraftClosesWall`, which
/// [`diagnostic`] gives as [`OffsetRefusal::DraftClosesWall`]). For a caller that drafts the
/// faces itself (the kernel wrapper's `smlib_draft`).
///
/// # Errors
///
/// The kernel's `DraftClosesWall`; an invalid input for a neutral normal or pull not finite or
/// not longer than 1e-12, or an origin or angle not finite; those of the kernel's check.
pub fn refuse_draft_closing_wall(
    k: &Kernel,
    b: &Brep,
    faces: &[FaceId],
    neutral_origin: [f64; 3],
    neutral_normal: [f64; 3],
    pull: [f64; 3],
    angle: f64,
) -> solidcore::Result<()> {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let the_draft = Draft::new(v(neutral_origin), v(neutral_normal), v(pull), angle)?;
    refuse_closing_wall(k, b, faces, &the_draft)
}

impl Body {
    /// Where a face of this body has an outline that touches itself ([`outline_touch`]): a
    /// loop through one vertex twice (not a seam's end), or a vertex of one loop on another
    /// loop of the face; the first such vertex's point, or `None`. What a shell that failed
    /// without saying why can be told: an offset cannot keep such a face's outline.
    ///
    /// # Errors
    /// The kernel's error; [`Error::Internal`] for a body a panic left unfinished.
    pub fn outline_touch(&self) -> Result<Option<[f64; 3]>> {
        read_body("outline_touch", self, |k, b| outline_touch(k, b))
    }
}

/// (#18568) For a shell that failed without saying why: where a face of `b` has an outline
/// that touches itself. That is a loop that passes through one vertex twice (not a seam's end),
/// or a vertex of one loop that is a vertex of another loop of the face too, or lies on one of
/// its edges within the vertex's and the edge's tolerances (exact distances to the edges'
/// curves). The first such vertex's point, if any.
pub fn outline_touch(k: &Kernel, b: &Brep) -> solidcore::Result<Option<[f64; 3]>> {
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::vertex;
    for f in solidcore::topology::brep::p1::faces(b, None)? {
        let Some(fu) = b.face(f)?.faceuse else {
            continue;
        };
        let loopuses = eu1::faceuse_loopuses(b, fu, false, None)?;
        // each loop's vertices and edges
        let mut loops = Vec::new();
        for lu in loopuses {
            let (mut vertices, mut edges) = (Vec::new(), Vec::new());
            // each edge use's edge and the vertex it starts at
            for eu in eu1::loopuse_edgeuses(b, lu)? {
                let start = match eu1::vertexuse(b, eu)? {
                    Some(vu) => b.vertexuse(vu)?.vertex,
                    None => None,
                };
                if let (Some(e), Some(v)) = (b.edgeuse(eu)?.edge, start) {
                    edges.push(e);
                    vertices.push(v);
                }
            }
            // A loop through one vertex twice pinches there, unless it is a seam's end: the
            // seam's edge in the loop twice, once each way.
            for (n, &v) in vertices.iter().enumerate() {
                let again = vertices.iter().skip(n + 1).any(|&w| w == v);
                let seam_end = edges
                    .iter()
                    .zip(&vertices)
                    .any(|(e, &w)| w == v && edges.iter().filter(|&f| f == e).count() > 1);
                if again && !seam_end {
                    let p = vertex::point(b, v)?;
                    return Ok(Some([p.x, p.y, p.z]));
                }
            }
            loops.push((vertices, edges));
        }
        for (i, (vertices, _)) in loops.iter().enumerate() {
            for (j, (others, edges)) in loops.iter().enumerate() {
                if i == j {
                    continue;
                }
                for &v in vertices {
                    let p = vertex::point(b, v)?;
                    if others.contains(&v) {
                        return Ok(Some([p.x, p.y, p.z]));
                    }
                    for &e in edges {
                        // an edge the kernel cannot measure the point against is not touched
                        if matches!(
                            solidcore::query::api_queries::vertex_on_edge(k, b, v, e),
                            Ok(true)
                        ) {
                            return Ok(Some([p.x, p.y, p.z]));
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// An offset call's failure as application data: its typed refusal where there is
/// one, alongside the complete kernel diagnostic. A caller still holding a kernel
/// error can retain its operation, entities and cause chain without interpreting it.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// The typed offset refusal, with any face unnamed as in [`refusal`]. Phrase
    /// refusals carry the complete diagnostic in their phrase as well.
    pub refusal: Option<Error>,
    /// The kernel's complete statement, including nested causes.
    pub detail: String,
}

/// Extracts an offset call's diagnostic, retaining its refusal as typed data.
#[must_use]
pub fn diagnostic(e: &SolidError) -> Diagnostic {
    let detail = e.to_string();
    let mut refusal = refusal(e);
    if let Some(Error::OffsetRefused {
        why: OffsetRefusal::Phrase { phrase, .. },
        ..
    }) = &mut refusal
    {
        phrase.clone_from(&detail);
    }
    Diagnostic { refusal, detail }
}

/// The offset refusal a kernel error is, the one routing [`status`] and [`refusal`] share: a
/// refusal the kernel gives a reason for by that reason ([`offset_refused`]), its entity through
/// `index`; one without by its kind ([`by_kind`]); `None` for any other error.
fn typed(e: &SolidError, index: &dyn Fn(EntityRef) -> Option<RecordEntity>) -> Option<Error> {
    if let Some(Reason::Offset(o)) = e.failure().map(|f| &f.reason) {
        return Some(offset_refused(*o, e.entity(), index));
    }
    by_kind(e.kind())
}

/// The operation's error for an offset refusal of `kind` that has one of its own: walls that
/// touch ([`Error::WallsTouch`]), a wall that folds ([`Error::WallFolds`], no largest wall
/// named), a vertex moved past an edge's other end ([`Error::OffsetRefused`] with
/// [`OffsetRefusal::EdgeVanishes`], no face named).
fn by_kind(kind: ErrorKind) -> Option<Error> {
    match kind {
        ErrorKind::OffsetWallsTouch => Some(Error::WallsTouch),
        ErrorKind::OffsetFolds => Some(Error::WallFolds { largest: None }),
        ErrorKind::OffsetEdgeVanishes => Some(Error::OffsetRefused {
            face: None,
            why: OffsetRefusal::EdgeVanishes,
        }),
        _ => None,
    }
}

/// The kernel's offset refusal `o` as the operation's error: walls that touch
/// ([`Error::WallsTouch`]), a wall whose face's offset folds ([`Error::WallFolds`], with the
/// largest wall the kernel names), and every other reason as [`Error::OffsetRefused`] (#14783,
/// a vertex moved past an edge's other end among them, #13989): why,
/// and the face the failure is about (`entity`, the reason's own unless the kernel set another)
/// through `index` (`None` where it names none, or one `index` does not find), and a body that
/// fails what its input did not with the entity its check names through `index` too
/// ([`unsound`]). A reason this version does not name yet takes the error of its class where
/// that has one ([`by_kind`]), as it did before it had a reason; it and a phrase are otherwise
/// said by their class and in the kernel's words ([`unnamed`], #15035).
fn offset_refused(
    o: OffsetFailure,
    entity: Option<EntityRef>,
    index: &dyn Fn(EntityRef) -> Option<RecordEntity>,
) -> Error {
    let face = match entity.and_then(index) {
        Some(RecordEntity::Face(i)) => Some(i),
        _ => None,
    };
    let why = match o {
        OffsetFailure::Folds { largest, .. } => {
            return Error::WallFolds {
                largest: Some(largest),
            }
        }
        // a rounded offset's tube that folds (#13685): the wall is the tube's radius
        OffsetFailure::TubeFolds { largest, .. } => return Error::WallFolds { largest },
        OffsetFailure::WallsTouch { .. } => return Error::WallsTouch,
        OffsetFailure::EdgeVanishes { .. } => OffsetRefusal::EdgeVanishes,
        // (the kernel's fold kind also covers an offset not certified regular, which is no
        // fold: its own reason, #14783)
        OffsetFailure::NotCertified { distance, .. }
        | OffsetFailure::TubeNotCertified { distance, .. } => {
            OffsetRefusal::NotCertified { distance }
        }
        OffsetFailure::Collapses {
            distance, radius, ..
        } => OffsetRefusal::Collapses { distance, radius },
        OffsetFailure::WallsMeetAcross {
            distance, largest, ..
        } => OffsetRefusal::WallsMeetAcross { distance, largest },
        OffsetFailure::LoopsCross { crossing, .. } => OffsetRefusal::LoopsCross { crossing },
        OffsetFailure::DistanceUnresolved {
            distance,
            resolution,
        } => OffsetRefusal::DistanceUnresolved {
            distance,
            resolution,
        },
        OffsetFailure::ToleranceUnresolved { tolerance } => {
            OffsetRefusal::ToleranceUnresolved { tolerance }
        }
        OffsetFailure::SeamOpened { .. } => OffsetRefusal::SeamOpened,
        OffsetFailure::ExactCurveUncut => OffsetRefusal::ExactCurveUncut,
        OffsetFailure::FacesApart { .. } => OffsetRefusal::FacesApart,
        OffsetFailure::PastSide { largest, .. } => OffsetRefusal::PastSide { largest },
        OffsetFailure::FaceCloses { largest, .. } => OffsetRefusal::FaceCloses { largest },
        OffsetFailure::DraftClosesWall { largest, .. } => {
            OffsetRefusal::DraftClosesWall { largest }
        }
        OffsetFailure::ReachUncertified { .. } => OffsetRefusal::ReachUncertified,
        OffsetFailure::LoopGapUncertified { .. } => OffsetRefusal::LoopGapUncertified,
        OffsetFailure::Unsound { what } => match unsound(what, index) {
            Some(what) => OffsetRefusal::Unsound { what },
            None => unnamed(&o),
        },
        // the kernel's phrase causes (`OffsetFailure::is_phrase`), and a reason the kernel adds
        // later: still classed and said, never a bare status
        other => match by_kind(other.kind()) {
            Some(e) => return e,
            None => unnamed(&other),
        },
    };
    Error::OffsetRefused { face, why }
}

/// A kernel offset failure ops does not name, as [`OffsetRefusal::Phrase`]: the kernel's class
/// of it, whether it is a defect, and its own statement of it.
fn unnamed(o: &OffsetFailure) -> OffsetRefusal {
    OffsetRefusal::Phrase {
        class: o.kind(),
        defect: o.is_defect(),
        phrase: o.to_string(),
    }
}

/// What the kernel says a built body fails, as ops' [`Unsoundness`]: the entity it names through
/// `index`. `None` for a reason this version does not name yet ([`unnamed`] says it).
fn unsound(
    what: solidcore::error::offset::Unsoundness,
    index: &dyn Fn(EntityRef) -> Option<RecordEntity>,
) -> Option<Unsoundness> {
    use solidcore::error::offset::Unsoundness as K;
    Some(match what {
        K::Check { failures, first } => Unsoundness::Check { failures, first },
        K::GeometryUnchecked => Unsoundness::GeometryUnchecked,
        K::LoopOrientationUndecided => Unsoundness::LoopOrientationUndecided,
        K::LoopGapNotClosable => Unsoundness::LoopGapNotClosable,
        K::NotManifoldSolid => Unsoundness::NotManifoldSolid,
        K::VolumeNotPositive => Unsoundness::VolumeNotPositive,
        K::VolumeUndecided => Unsoundness::VolumeUndecided,
        K::ToleranceBeyondBound {
            entity,
            tolerance,
            bound,
        } => Unsoundness::ToleranceBeyondBound {
            entity: entity.and_then(index),
            tolerance,
            bound,
        },
        _ => return None,
    })
}

/// A face's, edge's or vertex's record index through `order`, the [`Order`] of the call's body
/// (of an edge solids touch along, its first record edge, as [`Order::record_edges`] gives it).
fn in_order(order: &Order) -> impl Fn(EntityRef) -> Option<RecordEntity> + '_ {
    move |e| match e {
        EntityRef::Face(f) => position(&order.faces, f).map(RecordEntity::Face),
        EntityRef::Edge(x) => position(&order.edges, x).map(RecordEntity::Edge),
        EntityRef::Vertex(v) => position(&order.vertices, v).map(RecordEntity::Vertex),
        _ => None,
    }
}

/// A face's index among the faces of `b`, for a call that takes no [`Order`]; an edge or vertex
/// is unnamed (without the record, its index is not one the caller knows).
fn in_list(b: &Brep) -> impl Fn(EntityRef) -> Option<RecordEntity> + '_ {
    move |e| match e {
        EntityRef::Face(f) => position(b.face_list(), f).map(RecordEntity::Face),
        _ => None,
    }
}

/// Where `x` is in `list`.
fn position<T: PartialEq + Copy>(list: &[T], x: T) -> Option<usize> {
    list.iter().position(|y| *y == x)
}

/// A distance or wall: finite, not zero, at most [`MAX_DISTANCE`] across.
fn distance_ok(x: f64) -> bool {
    x.is_finite() && x != 0.0 && x.abs() <= MAX_DISTANCE
}

/// The faces of `b` that record indices `faces` name through `order` (the [`Order`] of `b`'s
/// record), each once: an index past the record's faces, one repeated, or one naming a face `b`
/// does not have (an order taken before an edit removed it) is [`Error::Argument`].
fn faces_of(b: &Brep, order: &Order, faces: &[usize]) -> Result<Vec<FaceId>> {
    let mut out: Vec<FaceId> = Vec::new();
    for &i in faces {
        let f = order::face(order, i)?;
        if out.contains(&f) || !b.face_list().contains(&f) {
            return Err(Error::Argument);
        }
        out.push(f);
    }
    Ok(out)
}

/// The convex edges extended and the results merged: `SmApiOffsetBrepFull`'s and
/// `SmApiShellBrepFull`'s offset.
const fn offset_options(distance: f64) -> OffsetOptions {
    OffsetOptions {
        distance,
        extend_convex_edges: true,
        merge_results: true,
    }
}

/// `SmApiShellBrepFull(b, distance, TRUE, TRUE, TRUE, open, r)`: side walls made at a sheet's
/// edges, and a thickened sheet's coplanar side walls one face.
fn shell_brep_full(
    k: &mut Kernel,
    b: &Brep,
    distance: f64,
    open: &[FaceId],
) -> solidcore::Result<Brep> {
    let options = ShellOptions {
        offset: offset_options(distance),
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    // (a feature narrower than twice an inward wall stays solid: `thin_features`)
    shell_brep(k, b, options, open)
}

/// The kernel's room test for a wall of `wall` (> 0) inward from every face of `b` but `open`:
/// a cavity shown (or nothing offset) goes on; none shown is [`Error::NoRoom`]; a face the
/// test cannot measure is [`Error::Unsupported`].
fn room(k: &mut Kernel, b: &mut Brep, open: &[FaceId], wall: f64) -> solidcore::Result<Result<()>> {
    Ok(match shell_wall_room(k, b, open, wall)? {
        WallRoom::Room { .. } | WallRoom::NoWalls => Ok(()),
        WallRoom::NotShown => Err(Error::NoRoom),
        WallRoom::Unmeasurable { .. } => Err(Error::Unsupported(
            "a face whose surface the room test cannot measure",
        )),
    })
}

/// Puts `made` in `b`'s place once it meets `expect`.
fn replace(
    k: &mut Kernel,
    b: &mut Brep,
    made: Brep,
    expect: Expect,
) -> solidcore::Result<Result<()>> {
    Ok(finish(k, made, expect)?.map(|r| *b = r))
}

/// The faces of `body` with record indices `open` (through `order`, its record's [`Order`])
/// opened, the rest offset by `thickness` (negative: inward) into a wall; none opened gives a
/// closed hollow body. A solid gives a solid.
///
/// # Errors
/// [`Error::Argument`] for a thickness not finite, zero or past 1e6, an index past the faces,
/// repeated or naming a face the body does not have, or a closed manifold body that is not a
/// valid solid facing out (facing in, or invalid); [`Error::NoRoom`] for an inward wall of a solid the kernel's room test shows no
/// cavity for; the kernel's refusals; those of [`finish`].
pub fn shell(body: &Body, order: &Order, open: &[usize], thickness: f64) -> Result<()> {
    if !distance_ok(thickness) {
        return Err(Error::Argument);
    }
    with_body("SmApiShellBrepFull", body, |k, b| {
        let open = match faces_of(b, order, open) {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        let expect = Expect::of(k, b)?;
        // a closed manifold body that is no solid (facing in, or invalid) has no inside the
        // room test can read off its faces
        if is_manifold_solid(b)? && !expect.solid() {
            return Ok(Err(Error::Argument));
        }
        #[allow(
            clippy::float_arithmetic,
            reason = "the asked thickness's size: a sign, not geometry"
        )]
        let inward = -thickness;
        if thickness < 0.0 && expect.solid() {
            if let Err(e) = room(k, b, &open, inward)? {
                return Ok(Err(e));
            }
        }
        match shell_brep_full(k, b, thickness, &open) {
            Ok(made) => replace(k, b, made, expect),
            Err(e) => Ok(Err(status(e, &in_order(order)))),
        }
    })?
}

/// The faces of the solid `body` with record indices `faces` (through `order`, its record's
/// [`Order`]) moved `distance` along their outward normals (negative: inward): each gets its
/// surface's offset and the faces around it are extended or trimmed to meet it; the tangent
/// blends on them follow.
///
/// # Errors
/// [`Error::Argument`] for a distance not finite, zero or past 1e6, no faces, an index past the
/// faces, repeated or naming a face the body does not have, or a body that is not a valid solid
/// facing out; [`Error::NoRoom`] for faces moved in so far that the kernel's room test shows
/// no material behind them, or a blend that no longer fits; [`Error::Unsupported`] for a blend that cannot be put back as it was;
/// [`Error::NotExact`] for one that needs a continuation that cannot be made exactly;
/// [`Error::BlendsNotFollowed`] for either of the last two where the kernel says why;
/// [`Error::NoEffect`] for a move that moved nothing; the kernel's other refusals.
pub fn offset_faces(body: &Body, order: &Order, faces: &[usize], distance: f64) -> Result<()> {
    if !distance_ok(distance) || faces.is_empty() {
        return Err(Error::Argument);
    }
    with_body("OffsetFaces", body, |k, b| {
        let picked = match faces_of(b, order, faces) {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        let expect = Expect::of(k, b)?;
        // a solid facing out: a manifold body facing in has no inside to move faces into
        if !expect.solid() {
            return Ok(Err(Error::Argument));
        }
        // faces moved in: the body must reach deeper than the move behind them
        if distance < 0.0 {
            let others: Vec<FaceId> = b
                .face_list()
                .iter()
                .copied()
                .filter(|f| !picked.contains(f))
                .collect();
            #[allow(
                clippy::float_arithmetic,
                reason = "the asked distance's size: a sign, not geometry"
            )]
            let inward = -distance;
            if let Err(e) = room(k, b, &others, inward)? {
                return Ok(Err(e));
            }
        }
        let mut exec = OffsetFacesExecutive::new();
        if let Err(e) = exec.offset_faces(k, b, &picked, distance) {
            // where the kernel says why the blends cannot follow, the refusal says it too
            let why = exec.reason().trim();
            let not_followed = |exact: bool, bare: Error| {
                if why.is_empty() {
                    bare
                } else {
                    Error::BlendsNotFollowed {
                        exact,
                        why: why.to_owned(),
                    }
                }
            };
            return Ok(Err(match exec.refusal() {
                OffsetFacesRefusal::NoRoom => Error::NoRoom,
                OffsetFacesRefusal::Unsupported | OffsetFacesRefusal::NotPutBack => not_followed(
                    false,
                    Error::Unsupported("a blend on a moved face cannot be taken off and put back"),
                ),
                OffsetFacesRefusal::NotExact => not_followed(true, Error::NotExact),
                OffsetFacesRefusal::NoEffect => Error::NoEffect,
                OffsetFacesRefusal::None => status(e, &in_order(order)),
            }));
        }
        let moved = std::mem::take(b);
        replace(k, b, moved, expect)
    })?
}

/// The sheet `body` thickened by `thickness` along its normals, or (`symmetric`) moved back by
/// half first: a closed solid ([`Expect::SOLID`], [`Error::NotSolidBecause`] if not).
///
/// # Errors
/// [`Error::Argument`] for a thickness not finite, zero or past 1e6, or a body that is a
/// manifold solid; the kernel's refusals; those of [`finish`].
pub fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<()> {
    if !distance_ok(thickness) {
        return Err(Error::Argument);
    }
    with_body("SmApiShellBrepFull", body, |k, b| {
        if is_manifold_solid(b)? {
            return Ok(Err(Error::Argument));
        }
        // a thickened sheet must be a closed solid, whatever the sheet
        let expect = Expect::SOLID;
        #[allow(
            clippy::float_arithmetic,
            reason = "half the asked thickness back for a symmetric thicken: a choice of parameter"
        )]
        let back = -0.5 * thickness;
        if symmetric {
            match offset_brep_simple(k, b, offset_options(back)) {
                Ok(moved) => *b = moved,
                Err(e) => return Ok(Err(status(e, &in_list(b)))),
            }
        }
        match shell_brep_full(k, b, thickness, &[]) {
            Ok(made) => replace(k, b, made, expect),
            Err(e) => Ok(Err(status(e, &in_list(b)))),
        }
    })?
}

/// Every face of the sheet `body` moved onto its surface's offset by `distance` along its
/// normal; faces that close into a solid (a sphere's one face, a block's six) give the offset
/// solid.
///
/// # Errors
/// [`Error::Argument`] for a distance not finite, zero or past 1e6; [`Error::Empty`] for a
/// result without faces (a sphere moved in to its centre); the kernel's refusals.
pub fn offset_sheet(body: &Body, distance: f64) -> Result<()> {
    if !distance_ok(distance) {
        return Err(Error::Argument);
    }
    with_body("SmApiOffsetBrepFull", body, |k, b| {
        let expect = Expect::of(k, b)?;
        match offset_brep_simple(k, b, offset_options(distance)) {
            Ok(made) => replace(k, b, made, expect),
            Err(e) => Ok(Err(status(e, &in_list(b)))),
        }
    })?
}

/// The largest draft angle a draft takes, radians, not included (SolidCraft's: a turn short of
/// square to the pull, where a face's line on the neutral plane would carry it off to infinity).
const MAX_DRAFT_ANGLE: f64 = 1.569_796_326_794_896_6;

/// The farthest a neutral plane's origin may lie from the origin along each axis.
const MAX_NEUTRAL_ORIGIN: f64 = 1e9;

/// Marks each face of `picked` as moved, with the kernel's draft mark ([`ids::DRAFT_MOVED`]): the
/// mark rides on the face, copied to what the kernel's local operation makes of it
/// (`Behavior::Copy`), so a face the operation keeps or replaces still carries it. A merge keeping an unpicked face over a picked one hands that
/// survivor the mark, as SolidCraft's marks do.
fn mark_moved(b: &mut Brep, picked: &[FaceId]) -> solidcore::Result<()> {
    for &f in picked {
        b.add_attribute(
            EntityRef::Face(f),
            Attribute::long(ids::DRAFT_MOVED, 0, Behavior::Copy),
        )?;
    }
    Ok(())
}

/// Whether any face of `b` carries the draft's mark; every such mark removed, so none
/// outlives the call.
fn moved_faces(b: &mut Brep) -> solidcore::Result<bool> {
    let mut moved = false;
    for f in b.face_list().to_vec() {
        let e = EntityRef::Face(f);
        let marks: Vec<_> = b
            .entity_attributes(e)
            .into_iter()
            .filter(|(_, a)| a.attribute_id() == ids::DRAFT_MOVED)
            .map(|(h, _)| h)
            .collect();
        moved = moved || !marks.is_empty();
        for h in marks {
            b.remove_attribute(e, h)?;
        }
    }
    Ok(moved)
}

/// A face the kernel cannot draft, as the operation's error: a pull in the neutral plane is
/// the draft's own argument ([`Error::Argument`]); any other refusal names what the face is
/// ([`Error::Unsupported`]). The app's own draft words the same refusals with it.
pub fn draft_refused(r: DraftRefusal) -> Error {
    Error::Unsupported(match r {
        DraftRefusal::PullInNeutralPlane => return Error::Argument,
        DraftRefusal::NotPlaneOrCone => "a drafted face is neither a plane nor a cone or cylinder",
        DraftRefusal::PlaneAcrossPull => "a drafted plane is not parallel to the pull",
        DraftRefusal::PlaneParallelToNeutral => {
            "a drafted plane is parallel, or too near parallel, to the neutral plane: it has no \
             line on it to turn about"
        }
        DraftRefusal::HingeAlongPull => "a drafted plane meets the neutral plane along the pull",
        DraftRefusal::AngleBeyondHinge => {
            "a drafted plane's line on the neutral plane is too near the pull for the draft angle"
        }
        DraftRefusal::AxisNotAlongPull => "a drafted cone or cylinder's axis is not along the pull",
        DraftRefusal::AxisInNeutralPlane => {
            "a drafted cone or cylinder's axis lies in the neutral plane"
        }
        DraftRefusal::SenseNotProved => {
            "whether a drafted cone or cylinder faces away from its axis is not proved"
        }
        DraftRefusal::ConeCloses => "a drafted cone would close within the face",
    })
}

/// The faces of the solid `body` with record indices `faces` (through `order`, its record's
/// [`Order`]) drafted by `angle` radians about the neutral plane through `neutral_origin` with
/// normal `neutral_normal`, so that they lean in going along `pull` (a negative angle leans
/// them out): a plane parallel to the pull turns about the line where it meets the neutral
/// plane, a cone or cylinder about the pull about its circle on it ([`draft_surface`]), and the
/// faces around them are extended or trimmed to meet them.
///
/// # Errors
/// [`Error::Argument`] for a neutral normal or pull not finite or not longer than 1e-12 (the
/// kernel's [`Draft::new`]), an angle zero, not finite or not below 1.5697963267948966 across, a
/// neutral origin past 1e9 on an axis, no faces, an index past the faces, repeated or naming a
/// face the body does not have, a body that is not a valid solid facing out, or a pull in the
/// neutral plane for a plane face;
/// [`Error::Unsupported`] for a face the kernel cannot draft (what it is), or a face drafted past
/// where its neighbours meet; [`Error::NoEffect`] for a draft that moved no face;
/// [`Error::OffsetRefused`] for a face the kernel refuses with a reason (a drafted cone that
/// would reach a cavity's wall: [`OffsetRefusal::DraftClosesWall`], the largest angle that keeps
/// the wall); [`Error::DraftCutsThrough`] for a draft the kernel refuses without a reason of its
/// own whose drafted face would cut through a face it does not meet ([`drafted_through`]); the
/// kernel's other errors, its typed refusals passed on as they are;
/// those of [`finish`].
pub fn draft(
    body: &Body,
    order: &Order,
    faces: &[usize],
    neutral_origin: [f64; 3],
    neutral_normal: [f64; 3],
    pull: [f64; 3],
    angle: f64,
) -> Result<()> {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let Ok(the_draft) = Draft::new(v(neutral_origin), v(neutral_normal), v(pull), angle) else {
        return Err(Error::Argument);
    };
    if faces.is_empty()
        || angle == 0.0
        || angle.abs().partial_cmp(&MAX_DRAFT_ANGLE) != Some(std::cmp::Ordering::Less)
        || !neutral_origin.iter().all(|x| x.abs() <= MAX_NEUTRAL_ORIGIN)
    {
        return Err(Error::Argument);
    }
    with_body("Draft", body, |k, b| {
        let picked = match faces_of(b, order, faces) {
            Ok(f) => f,
            Err(e) => return Ok(Err(e)),
        };
        let expect = Expect::of(k, b)?;
        if !expect.solid() {
            return Ok(Err(Error::Argument));
        }
        let mut surfaces = Vec::new();
        for &f in &picked {
            match draft_surface(k, b, f, &the_draft) {
                Ok(Ok(s)) => surfaces.push(s),
                Ok(Err(r)) => return Ok(Err(draft_refused(r))),
                Err(e) => return Ok(Err(status(e, &in_order(order)))),
            }
        }
        // a drafted cone that would reach a cavity's wall closes the wall: refused with the
        // largest angle that keeps it, before anything changes
        if let Err(e) = refuse_closing_wall(k, b, &picked, &the_draft) {
            return Ok(Err(status(e, &in_order(order))));
        }
        // the marks live for this call only: put on the picked faces, read and removed once the
        // kernel has moved them (a refused call puts the body back as it came in, unmarked)
        // a face the drafted faces would cut through (a hole's wall within the draft of the
        // side), read before the kernel edits the body and named if it then refuses; best
        // effort: a check that fails itself leaves the kernel's own refusal, and never refuses
        // a draft the kernel builds
        let through = drafted_through(k, b, &picked, &surfaces).ok().flatten();
        mark_moved(b, &picked)?;
        if let Err(e) = local_operation_simple(k, b, &picked, surfaces) {
            // the cut through is named only where the kernel gives no reason of its own (a bare
            // status): any refusal it words or types is its answer, passed on as it is
            return Ok(Err(match (status(e, &in_order(order)), through) {
                (Error::Status(_) | Error::Exception, Some(t)) => Error::DraftCutsThrough {
                    at: [t.at.x, t.at.y, t.at.z],
                    depth: t.depth,
                },
                (refused, _) => refused,
            }));
        }
        if !moved_faces(b)? {
            return Ok(Err(Error::NoEffect));
        }
        let drafted = std::mem::take(b);
        replace(k, b, drafted, expect)
    })?
}

mod surface;
pub use surface::offset_surface;

#[cfg(test)]
mod tests;
