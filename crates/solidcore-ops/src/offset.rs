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
//!
//! Each changes the body it is given, as the kernel does; a refused or failed one leaves it as
//! it came in ([`crate::session`]'s edit wrapper).

use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::offset::offset_executive::p2::{
    offset_brep_simple, shell_brep_simple, OffsetOptions, ShellOptions,
};
use solidcore::offset::offset_faces::{OffsetFacesExecutive, OffsetFacesRefusal};
use solidcore::offset::wall_room::{shell_wall_room, WallRoom};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, FaceId, Kernel, SolidError};
use solidcore_convert::brep::Order;

use crate::body::{finish, Body, Expect};
use crate::error::status as kernel_status;
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
        let f = order.faces.get(i).copied().ok_or(Error::Argument)?;
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

#[cfg(test)]
mod tests;
