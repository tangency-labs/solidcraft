//! What a body has on and past a round face of it: how far the body reaches in a seat's frame,
//! what lies past an end of the face, the largest radius it reaches over a stretch, whether the
//! stretch beside an end is empty, whether the face is a whole band, and the block a face grown on
//! its own stretch of the body is cut out with.
//!
//! A recipe of kernel calls (`solidcore::teeth::round_face`): the kernel gives the map into the
//! seat's frame and the size of every probe, and reads the answers; this module converts the seat
//! ([`solidcore_convert::round_face`]), makes each probe (a ring or a disc extruded over its
//! stretch, [`crate::prism::extrude_planar_regions`]), intersects it with a copy of the body
//! ([`crate::boolean::boolean`]), moves the piece into the seat's frame
//! ([`crate::Body::transform`]) and hands its exact box ([`crate::Body::bounds`]) back. It computes
//! no geometric quantity. The body is only read: every probe meets a copy.
//!
//! A seat is the record's [`SeatFrame`] with the round face's radius beside it.

use solidcore::teeth::round_face as kernel;
use solidcore::teeth::seat::SeatFrame as KernelSeat;
use solidcore_convert::record_plane;
use solidcore_convert::round_face as convert;
use solidcore_record::round_face::{End, Extent, GrowBlock, RoundFaceRefusal};
use solidcore_record::shaft_spline::SeatFrame;
use solidcore_record::{Loop2, Region2, Vec2};

use crate::boolean::{boolean, BooleanOp};
use crate::prism::extrude_planar_regions;
use crate::{Body, Result};

/// How many points along an edge held as a curve other than a circle or a line the band reading
/// takes.
pub const BAND_EDGE_POINTS: usize = 24;

/// The map into the seat's frame, as [`Body::transform`] takes it.
type Matrix = [[f64; 4]; 4];

/// The exact box of a copy of `body` moved by `into`.
fn moved_bounds(body: &Body, into: &Matrix) -> Result<([f64; 3], [f64; 3])> {
    let moved = body.copy()?;
    moved.transform(into)?;
    moved.bounds()
}

/// What of `body` lies within the probe's ring over its stretch, as the box of the piece in the
/// frame `into` maps to; `None` where nothing does.
fn piece(
    body: &Body,
    into: &Matrix,
    probe: &kernel::Probe,
) -> Result<Option<([f64; 3], [f64; 3])>> {
    let region = Region2 {
        outer: Loop2::circle(Vec2::ZERO, probe.outer),
        holes: probe
            .inner
            .map(|inner| Loop2::circle(Vec2::ZERO, inner))
            .into_iter()
            .collect(),
    };
    let tool =
        extrude_planar_regions(&record_plane(&probe.plane), &[region], probe.from, probe.to)?;
    let Some(part) = boolean(body.copy()?, tool, BooleanOp::Intersect)? else {
        return Ok(None);
    };
    part.transform(into)?;
    part.bounds().map(Some)
}

/// The kernel's probe answered on `body`.
fn met(body: &Body, into: &Matrix, probe: &kernel::Probe) -> kernel::Met {
    match piece(body, into, probe) {
        Ok(Some((min, max))) => kernel::Met::Piece { min, max },
        Ok(None) => kernel::Met::Nothing,
        Err(_) => kernel::Met::Unread,
    }
}

/// A seat as the kernel's, the map into its frame and a body's extent there.
type Seated = (KernelSeat, Matrix, kernel::Extent);

/// The seat as the kernel's, with the map into its frame and the body's extent there.
fn seated(body: &Body, seat: &SeatFrame) -> std::result::Result<Seated, RoundFaceRefusal> {
    let seat = convert::seat_frame(seat);
    let into = kernel::into_seat(&seat).map_err(convert::record_refusal)?;
    let (min, max) = moved_bounds(body, &into).map_err(|_| RoundFaceRefusal::Extent)?;
    let extent = kernel::extent(&seat, min, max).map_err(convert::record_refusal)?;
    Ok((seat, into, extent))
}

/// How far `body` reaches in the seat's frame, exactly: how far it reaches from the free end into
/// the face (the least, below zero where it runs on past the free end, and the most), and the
/// most it reaches from the axis along the frame's two cross directions (a round body's radius).
/// The body is moved into the frame first (a rigid motion, which keeps it exact), so a shaft that
/// lies askew to the world axes is measured as truly as one along them.
///
/// # Errors
///
/// [`RoundFaceRefusal::Seat`] for a seat with no frame; [`RoundFaceRefusal::Extent`] where the
/// body's box in the frame could not be read.
pub fn extent_in(body: &Body, seat: &SeatFrame) -> std::result::Result<Extent, RoundFaceRefusal> {
    seated(body, seat).map(|(_, _, extent)| convert::record_extent(&extent))
}

/// What lies past the seat's free end of a round face of radius `radius` of `body` (a `bore`'s
/// wall, else the outside of a shaft), read off the exact body; `reach` is the radius the cuts
/// reach (the bottoms of a shaft's tooth spaces, of a hub's in a bore). For the far end, the seat
/// seen from there ([`far_end`]). An end the body cannot be read past is shut: the cuts stop at it.
#[must_use]
pub fn past_end(body: &Body, seat: &SeatFrame, radius: f64, bore: bool, reach: f64) -> End {
    let (seat, into, extent) = match seated(body, seat) {
        Ok((seat, into, extent)) => (seat, Some(into), Some(extent)),
        Err(_) => (convert::seat_frame(seat), None, None),
    };
    let mut probe = |p: &kernel::Probe| match &into {
        Some(into) => met(body, into, p),
        None => kernel::Met::Unread,
    };
    convert::record_end(&kernel::past_end(
        &seat, radius, bore, reach, extent, &mut probe,
    ))
}

/// The largest radius `body` reaches in the stretch `from`..`to` (measured from the seat's free
/// end into a face of radius `radius`) outside the radius `above`; `above` itself where it has
/// nothing there. From the exact body: what a ring over the stretch meets, and that piece's exact
/// box.
///
/// # Errors
///
/// [`RoundFaceRefusal`]: the seat has no frame, a number is not one, or the body or what the ring
/// meets could not be read. A reading that could not be made is never taken as nothing standing
/// there.
pub fn stands_over(
    body: &Body,
    seat: &SeatFrame,
    radius: f64,
    above: f64,
    from: f64,
    to: f64,
) -> std::result::Result<f64, RoundFaceRefusal> {
    let (seat, into, extent) = seated(body, seat)?;
    let mut probe = |p: &kernel::Probe| met(body, &into, p);
    kernel::stands_over(&seat, radius, extent, above, from, to, &mut probe)
        .map_err(convert::record_refusal)
}

/// Whether `body` leaves the stretch just past the seat's end of a round face of radius `radius`
/// empty within `within` of the axis: a chamfer that wide at a bore's step cuts into no wall there.
///
/// # Errors
///
/// [`RoundFaceRefusal`]: the seat has no frame, a radius is not finite and above zero, or what
/// the disc over the stretch meets could not be read.
pub fn beside_clear(
    body: &Body,
    seat: &SeatFrame,
    radius: f64,
    within: f64,
) -> std::result::Result<bool, RoundFaceRefusal> {
    let seat = convert::seat_frame(seat);
    let into = kernel::into_seat(&seat).map_err(convert::record_refusal)?;
    let mut probe = |p: &kernel::Probe| met(body, &into, p);
    kernel::beside_clear(&seat, radius, within, &mut probe).map_err(convert::record_refusal)
}

/// Whether the round face of radius `radius` on the seat is a whole band of `body`: every face of
/// the body on the seat's cylinder is bounded only by circles across the axis on that cylinder
/// (its ends) and by straight lines along the axis (seams). A hole across a shaft, a flat, a
/// keyway or a set screw's hole in a hub that opens in the face leaves other edges in it. The
/// faces and edges are read off the body's record; an edge of a cylinder's face held as a curve
/// other than a circle or a line is read from [`BAND_EDGE_POINTS`] points along it.
///
/// # Errors
///
/// [`RoundFaceRefusal::Faces`] where the body's record or the points along an edge could not be
/// read; [`RoundFaceRefusal::NoBand`] where no face of the body lies on the seat's cylinder;
/// [`RoundFaceRefusal::Number`] and [`RoundFaceRefusal::Seat`] for a radius or a seat the kernel
/// refuses.
pub fn whole_band(
    body: &Body,
    seat: &SeatFrame,
    radius: f64,
) -> std::result::Result<bool, RoundFaceRefusal> {
    let (record, order) = body.to_record().map_err(|_| RoundFaceRefusal::Faces)?;
    let mut faces = convert::band_faces(&record);
    for face in faces.iter_mut().filter(|f| f.cylinder.is_some()) {
        for edge in face
            .edges
            .iter_mut()
            .filter(|e| e.circle.is_none() && !e.line)
        {
            edge.points = body
                .edge_points(&order, edge.index, BAND_EDGE_POINTS)
                .map_err(|_| RoundFaceRefusal::Faces)?
                .into_iter()
                .map(solidcore::Vec3::from_array)
                .collect();
        }
    }
    kernel::whole_band(&convert::seat_frame(seat), radius, &faces).map_err(convert::record_refusal)
}

/// The block that takes the stretch of `body` a round face of radius `radius` runs along out of
/// it, for the face to be grown by `by` there on its own: a round slab about the seat's axis,
/// wider than the body and than the grown face, from `before` past the free end to `after` past
/// the far end (with [`end_run_past`] of each end, where the body has its own end there).
///
/// # Errors
///
/// [`RoundFaceRefusal`]: the seat has no frame, the body's size could not be read, or a number
/// is not finite or comes to a stretch of no length.
pub fn grow_block(
    body: &Body,
    seat: &SeatFrame,
    radius: f64,
    by: f64,
    before: f64,
    after: f64,
) -> std::result::Result<GrowBlock, RoundFaceRefusal> {
    let (seat, _, extent) = seated(body, seat)?;
    kernel::grow_block(&seat, radius, extent, by, before, after)
        .map(|block| convert::record_grow_block(&block))
        .map_err(convert::record_refusal)
}

/// The seat seen from the other end of its round face: the far end as the free end, with angle 0
/// where it was.
#[must_use]
pub fn far_end(seat: &SeatFrame) -> SeatFrame {
    let far = kernel::far_end(&convert::seat_frame(seat));
    solidcore_convert::shaft_spline::record_seat_frame(&far)
}

/// An end nothing is known to be past, or that something stands in the way of, on a face of
/// radius `radius`: the cuts stop at it.
#[must_use]
pub fn shut_end(radius: f64) -> End {
    convert::record_end(&kernel::End::shut(radius))
}

/// How far past the end `end` the cuts run to come out of the body (zero where they do not come
/// out there).
#[must_use]
pub fn end_run_out(end: &End) -> f64 {
    convert::end(end).run_out()
}

/// How far a tool that takes the whole round face runs on past the end `end`, with `margin` to
/// spare: through what the body has on its end and the margin, the margin past an end the body
/// ends at, and not at all at a shoulder or a neck.
#[must_use]
pub fn end_run_past(end: &End, margin: f64) -> f64 {
    convert::end(end).run_past(margin)
}

/// How far into the round face, past the end's plane, a chamfer on the end `end` reaches once the
/// face has been moved from the radius `from` to the radius `to`: the chamfer goes with the face
/// along its own cone, which meets the new surface further in. Zero where the end has no chamfer
/// of its own.
#[must_use]
pub fn end_carried(end: &End, from: f64, to: f64) -> f64 {
    convert::end(end).carried(from, to)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_arithmetic)]
mod tests {
    use super::*;
    use crate::{make_cone, make_cylinder};
    use solidcore_record::{Plane, Vec3};

    /// A round face's free end in the plane z = `z`, the face running on along +Z for `room`.
    fn seat_at(z: f64, radius: f64, room: f64) -> SeatFrame {
        SeatFrame {
            plane: Plane {
                origin: Vec3::new(0.0, 0.0, z),
                x: Vec3::X,
                y: Vec3::Y,
            },
            inward: 1.0,
            room,
            at: Vec3::new(radius, 0.0, z + room / 2.0),
        }
    }

    fn union(a: Body, b: Body) -> Body {
        boolean(a, b, BooleanOp::Union).unwrap().unwrap()
    }

    fn cut(a: Body, b: Body) -> Body {
        boolean(a, b, BooleanOp::Subtract).unwrap().unwrap()
    }

    /// A shaft of radius 10 from z = 0 to z = 30.
    fn shaft() -> Body {
        make_cylinder([0.0; 3], 10.0, 30.0).unwrap()
    }

    #[test]
    fn a_plain_shaft_ends_free_and_is_a_whole_band() {
        let body = shaft();
        let seat = seat_at(0.0, 10.0, 30.0);
        let e = extent_in(&body, &seat).unwrap();
        assert!(e.low.abs() < 1e-5 && (e.high - 30.0).abs() < 1e-5, "{e:?}");
        assert!((e.across - 10.0).abs() < 1e-5, "{e:?}");
        let end = past_end(&body, &seat, 10.0, false, 9.0);
        assert!(end.free && end.open && end.out, "{end:?}");
        let far = far_end(&seat);
        assert!((far.plane.origin.z - 30.0).abs() < 1e-12 && far.inward < 0.0);
        let end = past_end(&body, &far, 10.0, false, 9.0);
        assert!(end.free && end.open && end.out, "{end:?}");
        assert_eq!(whole_band(&body, &seat, 10.0), Ok(true));
        // Nothing stands over the shaft past its far end.
        assert_eq!(stands_over(&body, &seat, 10.0, 10.0, 30.0, 35.0), Ok(10.0));
        // A face the body does not have: no band to read.
        assert_eq!(whole_band(&body, &seat, 4.0), Err(RoundFaceRefusal::NoBand));
    }

    #[test]
    fn a_chamfered_end_is_read_as_a_cone() {
        // The end chamfered 1 mm by 1 mm: a cone from radius 9 at z = -1 up to 10 at z = 0.
        let body = union(
            shaft(),
            make_cone([0.0, 0.0, -1.0], 9.0, 10.0, 1.0).unwrap(),
        );
        let seat = seat_at(0.0, 10.0, 30.0);
        let end = past_end(&body, &seat, 10.0, false, 9.5);
        assert!(end.open && end.out && !end.step && end.cone, "{end:?}");
        assert!(
            (end.end - 1.0).abs() < 1e-5 && (end.rim - 9.0).abs() < 1e-3,
            "{end:?}"
        );
        assert!((end.through - 0.5).abs() < 1e-3, "{end:?}");
        assert!((end_run_out(&end) - 1.0).abs() < 1e-5);
        assert!((end_run_past(&end, 0.1) - 1.1).abs() < 1e-5);
        // The shaft grown to radius 12: the 45 degree cone meets the new surface 2 mm further
        // in (the rim is read to a thousandth, so the slope is known to as much).
        assert!((end_carried(&end, 10.0, 12.0) - 2.0).abs() < 1e-2);
        // A shut end has no chamfer to carry.
        assert!(end_carried(&shut_end(10.0), 10.0, 12.0).abs() < 1e-12);
    }

    #[test]
    fn a_neck_is_a_step_and_a_collar_shuts_the_end() {
        let seat = seat_at(0.0, 10.0, 30.0);
        let necked = union(
            shaft(),
            make_cylinder([0.0, 0.0, -20.0], 6.0, 20.0).unwrap(),
        );
        let end = past_end(&necked, &seat, 10.0, false, 9.0);
        assert!(
            end.open && end.step && (end.neck - 6.0).abs() < 1e-5,
            "{end:?}"
        );
        assert!(end.out && end.through.abs() < 1e-9, "{end:?}");
        assert!(end_run_past(&end, 0.1).abs() < 1e-12);
        let collared = union(shaft(), make_cylinder([0.0, 0.0, -5.0], 15.0, 5.0).unwrap());
        let end = past_end(&collared, &seat, 10.0, false, 9.0);
        assert!(!end.open && !end.out, "{end:?}");
        // The collar stands 15 mm from the axis over the 5 mm past the end.
        let over = stands_over(&collared, &seat, 10.0, 10.0, -5.0, 0.0).unwrap();
        assert!((over - 15.0).abs() < 1e-5, "{over}");
        // The block a face grown by 2 mm on its own stretch is cut out with.
        let block = grow_block(&collared, &seat, 10.0, 2.0, 0.0, 0.5).unwrap();
        assert!(
            (block.radius - (15.0 + 10.0 + 2.0 + 1.0)).abs() < 1e-5,
            "{block:?}"
        );
        assert!(
            block.from.abs() < 1e-12 && (block.to - 30.5).abs() < 1e-12,
            "{block:?}"
        );
    }

    #[test]
    fn a_counterbore_past_a_bore_is_a_step_with_a_clear_ledge() {
        // A hub of radius 20 from z = -4 to 30, a bore of radius 5 through it and a counterbore
        // of radius 8 from its end at z = -4 to z = 0.
        let hub = make_cylinder([0.0, 0.0, -4.0], 20.0, 34.0).unwrap();
        let hub = cut(hub, make_cylinder([0.0, 0.0, -10.0], 5.0, 50.0).unwrap());
        let hub = cut(hub, make_cylinder([0.0, 0.0, -10.0], 8.0, 10.0).unwrap());
        let seat = seat_at(0.0, 5.0, 30.0);
        let end = past_end(&hub, &seat, 5.0, true, 5.5);
        assert!(end.open && end.out && end.step && !end.cone, "{end:?}");
        assert_eq!(beside_clear(&hub, &seat, 5.0, 6.0), Ok(true));
        assert_eq!(beside_clear(&hub, &seat, 5.0, 9.0), Ok(false));
        assert_eq!(whole_band(&hub, &seat, 5.0), Ok(true));
    }

    #[test]
    fn hostile_numbers_are_refused_or_shut_never_a_panic() {
        let body = shaft();
        let seat = seat_at(0.0, 10.0, 30.0);
        let no_frame = SeatFrame {
            plane: Plane {
                origin: Vec3::ZERO,
                x: Vec3::X,
                y: Vec3::X,
            },
            ..seat
        };
        assert_eq!(extent_in(&body, &no_frame), Err(RoundFaceRefusal::Seat));
        assert!(!past_end(&body, &no_frame, 10.0, false, 9.0).open);
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let _ = past_end(&body, &seat, bad, false, bad);
            let _ = past_end(&body, &seat, 10.0, true, bad);
            assert!(
                stands_over(&body, &seat, bad, 10.0, 0.0, 5.0).is_err(),
                "{bad}"
            );
            assert!(beside_clear(&body, &seat, 10.0, bad).is_err(), "{bad}");
            assert!(whole_band(&body, &seat, bad).is_err(), "{bad}");
            assert!(
                grow_block(&body, &seat, bad, 1.0, 0.0, 0.0).is_err(),
                "{bad}"
            );
        }
        assert!(grow_block(&body, &seat, 10.0, f64::NAN, 0.0, 0.0).is_err());
        assert!(stands_over(&body, &seat, 10.0, f64::NAN, 0.0, 5.0).is_err());
    }
}
