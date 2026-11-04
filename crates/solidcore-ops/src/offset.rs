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
use solidcore::offset::draft::{draft_surface, Draft, DraftRefusal};
use solidcore::offset::offset_executive::p2::{
    local_operation_simple, offset_brep_simple, shell_brep_simple, OffsetOptions, ShellOptions,
};
use solidcore::offset::offset_faces::{OffsetFacesExecutive, OffsetFacesRefusal};
use solidcore::offset::wall_room::{shell_wall_room, WallRoom};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, EntityRef, FaceId, Kernel, SolidError, Vec3};

use crate::body::{finish, Body, Expect};
use crate::error::status as kernel_status;
use crate::order::{self, Order};
use crate::session::with_body;
use crate::{Error, Result};

/// The largest offset distance or wall the operations take.
const MAX_DISTANCE: f64 = 1e6;

/// An offset's kernel error as the operation's: walls that touch ([`Error::WallsTouch`]), a wall
/// whose face's offset folds ([`Error::WallFolds`], with the largest wall the kernel names), a
/// vertex moved past an edge's other end ([`Error::Unsupported`]), any other as
/// [`crate::error::status`].
fn status(e: SolidError) -> Error {
    match e.kind() {
        ErrorKind::OffsetWallsTouch => Error::WallsTouch,
        ErrorKind::OffsetFolds => {
            let largest = match e.failure().map(|f| &f.reason) {
                Some(Reason::Offset(OffsetFailure::Folds { largest, .. })) => Some(*largest),
                _ => None,
            };
            Error::WallFolds { largest }
        }
        ErrorKind::OffsetEdgeVanishes => {
            Error::Unsupported("a vertex moved past the other end of an edge")
        }
        _ => kernel_status(e),
    }
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
    shell_brep_simple(k, b, options, open)
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
        if thickness < 0.0 && expect.solid() {
            if let Err(e) = room(k, b, &open, -thickness)? {
                return Ok(Err(e));
            }
        }
        match shell_brep_full(k, b, thickness, &open) {
            Ok(made) => replace(k, b, made, expect),
            Err(e) => Ok(Err(status(e))),
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
            if let Err(e) = room(k, b, &others, -distance)? {
                return Ok(Err(e));
            }
        }
        let mut exec = OffsetFacesExecutive::new();
        if let Err(e) = exec.offset_faces(k, b, &picked, distance) {
            return Ok(Err(match exec.refusal() {
                OffsetFacesRefusal::NoRoom => Error::NoRoom,
                OffsetFacesRefusal::Unsupported | OffsetFacesRefusal::NotPutBack => {
                    Error::Unsupported("a blend on a moved face cannot be taken off and put back")
                }
                OffsetFacesRefusal::NotExact => Error::NotExact,
                OffsetFacesRefusal::NoEffect => Error::NoEffect,
                OffsetFacesRefusal::None => status(e),
            }));
        }
        let moved = std::mem::take(b);
        replace(k, b, moved, expect)
    })?
}

/// The sheet `body` thickened by `thickness` along its normals, or (`symmetric`) moved back by
/// half first: a closed solid ([`Expect::SOLID`], [`Error::NotSolid`] if not).
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
        if symmetric {
            match offset_brep_simple(k, b, offset_options(-0.5 * thickness)) {
                Ok(moved) => *b = moved,
                Err(e) => return Ok(Err(status(e))),
            }
        }
        match shell_brep_full(k, b, thickness, &[]) {
            Ok(made) => replace(k, b, made, expect),
            Err(e) => Ok(Err(status(e))),
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
            Err(e) => Ok(Err(status(e))),
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
/// ([`Error::Unsupported`]).
fn refused(r: DraftRefusal) -> Error {
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
/// where its neighbours meet; [`Error::NoEffect`] for a draft that moved no face; the kernel's
/// other errors; those of [`finish`].
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
                Ok(Err(r)) => return Ok(Err(refused(r))),
                Err(e) => return Ok(Err(kernel_status(e))),
            }
        }
        // the marks live for this call only: put on the picked faces, read and removed once the
        // kernel has moved them (a refused call puts the body back as it came in, unmarked)
        mark_moved(b, &picked)?;
        if let Err(e) = local_operation_simple(k, b, &picked, surfaces) {
            return Ok(Err(status(e)));
        }
        if !moved_faces(b)? {
            return Ok(Err(Error::NoEffect));
        }
        let drafted = std::mem::take(b);
        replace(k, b, drafted, expect)
    })?
}

#[cfg(test)]
mod tests;
