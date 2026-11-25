//! What a body has on and past a round face of it (`solidcore_ops::round_face`), asked of the app's bodies: how
//! far a body reaches in a seat's frame, what lies past an end of the face, the largest radius it reaches over a
//! stretch, whether the stretch beside an end is empty, whether the face is a whole band, and the block a face is
//! grown on its own stretch with. Every reading, size and tolerance is solidcore's; this module only hands it the
//! body's exact form. A mesh body, or one whose exact form cannot be made, is not read.

use solidcore_record::round_face::{End, Extent, GrowBlock, RoundFaceRefusal};
use solidcore_record::shaft_spline::SeatFrame;

use crate::Body;

/// The answer of `ask` on the body's exact form; `unread` where the body has none.
fn asked<T>(body: &Body, unread: T, ask: impl FnOnce(&solidcore_ops::Body) -> T) -> T {
    match body.ops_body() {
        Some(form) => ask(&form.0),
        None => unread,
    }
}

/// How far `body` reaches in the seat's frame ([`solidcore_ops::round_face::extent_in`]).
pub fn round_face_extent(body: &Body, seat: &SeatFrame) -> Result<Extent, RoundFaceRefusal> {
    asked(body, Err(RoundFaceRefusal::Extent), |b| solidcore_ops::round_face::extent_in(b, seat))
}

/// What lies past the seat's free end of a round face of `radius` of `body`
/// ([`solidcore_ops::round_face::past_end`]); a body that cannot be read has a shut end.
pub fn round_face_past_end(body: &Body, seat: &SeatFrame, radius: f64, bore: bool, reach: f64) -> End {
    asked(body, solidcore_ops::round_face::shut_end(radius), |b| solidcore_ops::round_face::past_end(b, seat, radius, bore, reach))
}

/// The largest radius `body` reaches over the stretch `from`..`to` outside the radius `above`
/// ([`solidcore_ops::round_face::stands_over`]).
pub fn round_face_stands_over(body: &Body, seat: &SeatFrame, radius: f64, above: f64, from: f64, to: f64) -> Result<f64, RoundFaceRefusal> {
    asked(body, Err(RoundFaceRefusal::Extent), |b| solidcore_ops::round_face::stands_over(b, seat, radius, above, from, to))
}

/// Whether `body` leaves the stretch just past the seat's end empty within `within` of the axis
/// ([`solidcore_ops::round_face::beside_clear`]).
pub fn round_face_beside_clear(body: &Body, seat: &SeatFrame, radius: f64, within: f64) -> Result<bool, RoundFaceRefusal> {
    asked(body, Err(RoundFaceRefusal::Probe), |b| solidcore_ops::round_face::beside_clear(b, seat, radius, within))
}

/// Whether the round face of `radius` on the seat is a whole band of `body`
/// ([`solidcore_ops::round_face::whole_band`]).
pub fn round_face_whole_band(body: &Body, seat: &SeatFrame, radius: f64) -> Result<bool, RoundFaceRefusal> {
    asked(body, Err(RoundFaceRefusal::Faces), |b| solidcore_ops::round_face::whole_band(b, seat, radius))
}

/// The block that takes the stretch of `body` a round face runs along out of it
/// ([`solidcore_ops::round_face::grow_block`]).
pub fn round_face_grow_block(body: &Body, seat: &SeatFrame, radius: f64, by: f64, before: f64, after: f64) -> Result<GrowBlock, RoundFaceRefusal> {
    asked(body, Err(RoundFaceRefusal::Extent), |b| solidcore_ops::round_face::grow_block(b, seat, radius, by, before, after))
}
