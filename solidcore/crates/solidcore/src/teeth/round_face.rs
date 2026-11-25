//! What a body has on and past a round face of it, read from the exact body: how far the body
//! reaches in the seat's frame, what lies past an end of the face (nothing, a chamfer or a round, a
//! neck, a counterbore, a shoulder), the largest radius it reaches over a stretch, whether a
//! stretch beside an end is empty, whether the face is a whole band, and the block a face grown on
//! its own stretch of the body is cut out with.
//!
//! The body itself is not read here. A caller that holds it (`solidcore-ops`) answers probes: the
//! kernel says which plain ring or disc ([`Probe`]: its radii and its stretch along the axis of a
//! seat) to intersect the body with, and the caller hands back what the intersection met, as the
//! exact box of the piece in the seat's frame ([`Met`]; the map into the frame is [`into_seat`]).
//! Every number (the map, each probe's size, every tolerance) and the reading of the answers is
//! computed here.
//!
//! A seat is the kernel's [`SeatFrame`] (the plane square to the axis at the face's free end, the
//! side the face runs on to, its length) with the face's radius beside it. Measured "into the face"
//! means along the axis from the free end towards the face, so below zero past the end.
//!
//! Not upstream: SolidCraft's shaft spline, a kernel primitive because it computes geometry any
//! client could use (docs/layers.md §1). No recursion; each reading asks a fixed number of probes.
//! Inputs are hostile: numbers that are not finite, a seat with no frame, probes that cannot be
//! read give a typed refusal or the plain answer the reading falls back to, never a panic.

use std::fmt;

use crate::eval::Frame;
use crate::math::Transform;
use crate::teeth::seat::SeatFrame;
use crate::teeth::shaft_spline::same_size;
use crate::Vec3;

/// The most probes one reading of [`past_end`] asks.
pub const MOST_PROBES: usize = 7;

/// What could not be read, or the number that rules a reading out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundFaceRefusal {
    /// A number given is not finite, or a radius or a stretch is not above zero.
    Number,
    /// The seat gives no frame: its plane's axes are not finite or do not span a plane.
    Seat,
    /// The body's exact box in the seat's frame could not be read.
    Extent,
    /// What a probe over a stretch of the body met could not be read.
    Probe,
    /// No face of the body lies on the seat's cylinder.
    NoBand,
    /// The body's faces and edges could not be read.
    Faces,
}

impl fmt::Display for RoundFaceRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RoundFaceRefusal::Number => {
                "a number given is not finite, or a radius or a stretch is not above zero"
            }
            RoundFaceRefusal::Seat => "the seat on the round face gives no frame",
            RoundFaceRefusal::Extent => "the body's size in the seat's frame could not be read",
            RoundFaceRefusal::Probe => {
                "what the body has over a stretch along the round face could not be read"
            }
            RoundFaceRefusal::NoBand => "no face of the body lies on the round face's cylinder",
            RoundFaceRefusal::Faces => "the body's faces and edges could not be read",
        })
    }
}

impl std::error::Error for RoundFaceRefusal {}

/// How far a body reaches in a seat's frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extent {
    /// The least it reaches into the face from the free end: below zero where it runs on past the
    /// free end.
    pub low: f64,
    /// The most it reaches into the face from the free end.
    pub high: f64,
    /// The most it reaches from the axis along the frame's two cross directions (a round body's
    /// radius).
    pub across: f64,
}

/// A plain ring (a disc where it has no hole) about a seat's axis, over a stretch along it: what a
/// reading asks the body to be intersected with.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    /// The plane the ring lies in: the seat's plane.
    pub plane: Frame,
    /// The radius of its hole; none for a disc.
    pub inner: Option<f64>,
    /// Its outer radius.
    pub outer: f64,
    /// Where it starts along the plane's normal, from the plane.
    pub from: f64,
    /// Where it ends along the plane's normal, from the plane: above `from`.
    pub to: f64,
}

/// What a probe met of the body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Met {
    /// Nothing: the intersection is empty.
    Nothing,
    /// A piece, as its exact box in the seat's frame ([`into_seat`]).
    Piece {
        /// The least corner of the box.
        min: [f64; 3],
        /// The greatest corner of the box.
        max: [f64; 3],
    },
    /// The intersection or its box could not be made.
    Unread,
}

/// One end of a round face, as the body has it: what lies past it. A shaft's end may carry a
/// chamfer or a round, or step down to a neck; a bore's entry may carry a chamfer, or open into a
/// counterbore.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // independent readings of one end
pub struct End {
    /// The body ends where the face ends: nothing of it lies past the end.
    pub free: bool,
    /// Nothing stands in the way past the end: no shoulder larger than a shaft, no narrowing of a
    /// bore. A free end is open.
    pub open: bool,
    /// The cuts come out of the body past this end, as a cutter runs out of the work: the end is
    /// open, and what the body has past it at the radius the cuts reach ends within the face's
    /// radius of the end (a journal that runs on further is not cut into).
    pub out: bool,
    /// How far the cuts must run on past the end to come out: as far as the body has anything at
    /// the radius the cuts reach. A chamfer on the end is cut right through; what lies deeper than
    /// that radius is not reached.
    pub through: f64,
    /// The face stops at a step: past the end the body is smaller from the very end on (a shaft's
    /// neck) or wider (a bore's counterbore).
    pub step: bool,
    /// At a shaft's step, the radius of the body just past the end.
    pub neck: f64,
    /// How far past the face's end the body's own end lies, where what lies between belongs to
    /// the face's end: no longer than the face's radius and no more than twice as far across as
    /// it is long (a chamfer, a round). Zero otherwise.
    pub end: f64,
    /// The radius at the body's own end: the rim of a shaft's end face, the mouth of a bore.
    pub rim: f64,
    /// What lies between is a plain chamfer: straight from the rim to the face.
    pub cone: bool,
}

impl End {
    /// An end nothing is known to be past, or that something stands in the way of, on a face of
    /// radius `radius`: the cuts stop at it.
    #[must_use]
    pub fn shut(radius: f64) -> End {
        End {
            free: false,
            open: false,
            out: false,
            through: 0.0,
            step: false,
            neck: radius,
            end: 0.0,
            rim: radius,
            cone: false,
        }
    }

    /// How far past the end the cuts run to come out of the body: the farther of
    /// [`End::through`] and [`End::end`] where the cuts come out, else zero.
    #[must_use]
    pub fn run_out(&self) -> f64 {
        if self.out {
            self.through.max(self.end)
        } else {
            0.0
        }
    }

    /// How far a tool that takes the whole round face runs on past this end, with `margin` to
    /// spare: through what the body has on its end (a chamfer, a round) and the margin, the margin
    /// past an end the body ends at, and not at all at a shoulder or a neck.
    #[must_use]
    pub fn run_past(&self, margin: f64) -> f64 {
        if self.end > 0.0 {
            self.end + margin
        } else if self.free {
            margin
        } else {
            0.0
        }
    }

    /// How far into the round face, past the end's plane, a chamfer on this end reaches once the
    /// face has been moved from the radius `from` to the radius `to`: a moved face takes the
    /// chamfer with it along the chamfer's own cone, and that cone meets the new surface further
    /// into the face by its slope ([`End::end`] along the axis for `from - rim` across) times the
    /// move. Negative where the face moved towards the chamfer's rim: the cone then meets the new
    /// surface before the end's plane. Zero where the end has no chamfer of its own (no cone, or
    /// no length) and where the numbers give no finite length (a rim on the radius `from`).
    #[must_use]
    pub fn carried(&self, from: f64, to: f64) -> f64 {
        if !(self.cone && self.end > 0.0) {
            return 0.0;
        }
        let reach = self.end * (to - from) / (from - self.rim);
        if reach.is_finite() {
            reach
        } else {
            0.0
        }
    }
}

/// The block a round face is grown on its own stretch of a body with: a round slab about the
/// seat's axis, wider than the body and than the grown face, over the stretch the face runs along.
#[derive(Clone, Debug, PartialEq)]
pub struct GrowBlock {
    /// The plane the slab lies in: the seat's plane.
    pub plane: Frame,
    /// The slab's radius.
    pub radius: f64,
    /// Where it starts along the plane's normal, from the plane.
    pub from: f64,
    /// Where it ends along the plane's normal, from the plane: above `from`.
    pub to: f64,
}

/// A circle or a cylinder as a record holds it: its frame (the origin on its axis, its axis the
/// frame's x cross y) and its radius.
#[derive(Clone, Debug, PartialEq)]
pub struct Round {
    /// The frame.
    pub frame: Frame,
    /// The radius.
    pub radius: f64,
}

/// An edge of a face, as [`whole_band`] reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandEdge {
    /// Which edge of the body it is.
    pub index: usize,
    /// Its circle, where its curve is held as one.
    pub circle: Option<Round>,
    /// Its curve is held as a straight line.
    pub line: bool,
    /// Its two ends, where they are known.
    pub ends: Option<[Vec3; 2]>,
    /// Points along it between its ends, for an edge held as another curve (empty where none
    /// were taken).
    pub points: Vec<Vec3>,
}

/// A face of a body, as [`whole_band`] reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandFace {
    /// Its cylinder, where its surface is held as one.
    pub cylinder: Option<Round>,
    /// The edges that bound it.
    pub edges: Vec<BandEdge>,
}

/// A vector of a frame's numbers.
fn v(a: [f64; 3]) -> Vec3 {
    Vec3::from_array(a)
}

/// The normal of a frame: its x cross its y.
fn normal(plane: &Frame) -> Vec3 {
    v(plane.x_axis).cross(v(plane.y_axis))
}

/// How far the body may stand off a radius or a plane and still be taken as on it, for a face of
/// radius `radius`.
fn clear_of(radius: f64) -> f64 {
    1e-6 * (1.0 + radius)
}

/// Whether `radius` is a face's radius: finite and above zero.
fn a_radius(radius: f64) -> bool {
    radius.is_finite() && radius > 0.0
}

/// The distances along the plane's normal of the stretch from `from` to `to` measured into the
/// face from its free end.
fn span(seat: &SeatFrame, from: f64, to: f64) -> (f64, f64) {
    if seat.inward > 0.0 {
        (from, to)
    } else {
        (-to, -from)
    }
}

/// The map that takes world coordinates into the seat's frame: its plane's x, y and normal, its
/// origin to zero (rows the images of the world axes, then the translation, as
/// [`Transform::m`]). A rigid motion, so a body moved by it stays exact.
///
/// # Errors
///
/// [`RoundFaceRefusal::Seat`]: the plane's numbers are not finite, or its axes span no plane.
pub fn into_seat(seat: &SeatFrame) -> Result<[[f64; 4]; 4], RoundFaceRefusal> {
    let plane = &seat.plane;
    let (x, y, o) = (v(plane.x_axis), v(plane.y_axis), v(plane.origin));
    Transform::from_axes(x, y, normal(plane), o)
        .inverse()
        .map(|t| t.m)
        .map_err(|_| RoundFaceRefusal::Seat)
}

/// How far a body reaches in the seat's frame, from its exact box there (`min`..`max`, as the body
/// moved by [`into_seat`] has it).
///
/// # Errors
///
/// [`RoundFaceRefusal::Extent`]: a number of the box, or one read from it, is not finite.
pub fn extent(seat: &SeatFrame, min: [f64; 3], max: [f64; 3]) -> Result<Extent, RoundFaceRefusal> {
    let [min_x, min_y, min_z] = min;
    let [max_x, max_y, max_z] = max;
    let (a, b) = (min_z * seat.inward, max_z * seat.inward);
    let across = min_x
        .abs()
        .max(max_x.abs())
        .max(min_y.abs())
        .max(max_y.abs());
    if a.is_finite() && b.is_finite() && across.is_finite() {
        Ok(Extent {
            low: a.min(b),
            high: a.max(b),
            across,
        })
    } else {
        Err(RoundFaceRefusal::Extent)
    }
}

/// The seat seen from the other end of its round face: the far end as the free end, the plane
/// moved there along its normal with angle 0 where it was, the face running on the other way.
#[must_use]
pub fn far_end(seat: &SeatFrame) -> SeatFrame {
    let plane = &seat.plane;
    let origin = v(plane.origin) + normal(plane) * (seat.inward * seat.room);
    SeatFrame {
        plane: Frame {
            origin: origin.to_array(),
            x_axis: plane.x_axis,
            y_axis: plane.y_axis,
        },
        inward: -seat.inward,
        room: seat.room,
        at: seat.at,
    }
}

/// A body read through its probes: the seat, and the caller that answers each probe.
struct Reader<'s, 'p> {
    seat: &'s SeatFrame,
    probe: &'p mut dyn FnMut(&Probe) -> Met,
    asked: usize,
}

impl Reader<'_, '_> {
    /// What the body has within the ring from `inner` (none where it is not above zero) to `outer`
    /// over the stretch `from`..`to` measured into the face. A ring that is no ring (radii not
    /// finite or out of order, a stretch not finite or of no length) and a probe past
    /// [`MOST_PROBES`] are not asked: they are not read.
    fn met(&mut self, inner: f64, outer: f64, from: f64, to: f64) -> Met {
        let (a, b) = span(self.seat, from, to);
        let inner = (inner > 0.0).then_some(inner);
        let ring = outer.is_finite()
            && outer > 0.0
            && inner.is_none_or(|r| r.is_finite() && r < outer)
            && a.is_finite()
            && b.is_finite()
            && a < b;
        if !ring || self.asked >= MOST_PROBES {
            return Met::Unread;
        }
        self.asked += 1;
        (self.probe)(&Probe {
            plane: self.seat.plane.clone(),
            inner,
            outer,
            from: a,
            to: b,
        })
    }

    /// The extent in the seat's frame of a piece a probe met; none where it is not read.
    fn piece(&self, min: [f64; 3], max: [f64; 3]) -> Option<Extent> {
        extent(self.seat, min, max).ok()
    }

    /// How far towards the body's end (up to `zone` past the face's end) what the body has within
    /// `rho` of the axis reaches; 0 where it has nothing there, none where it is not read.
    fn reaches(&mut self, rho: f64, zone: f64, clear: f64) -> Option<f64> {
        match self.met(0.0, rho, -(zone + clear), -clear) {
            Met::Piece { min, max } => self.piece(min, max).map(|e| (-e.low).max(0.0).min(zone)),
            Met::Nothing => Some(0.0),
            Met::Unread => None,
        }
    }

    /// The largest radius of the body between `from` and `to` past the end, inside `far`; 0 where
    /// it has nothing there, none where it is not read.
    fn radius_in(&mut self, far: f64, from: f64, to: f64) -> Option<f64> {
        match self.met(0.0, far, from, to) {
            Met::Piece { min, max } => self.piece(min, max).map(|e| e.across),
            Met::Nothing => Some(0.0),
            Met::Unread => None,
        }
    }
}

/// What lies past the seat's free end of a round face of radius `radius` (a `bore`'s wall, else
/// the outside of a shaft), on a body whose extent in the seat's frame is `body` (none where it
/// was not read); `reach` is the radius the cuts reach (the bottoms of a shaft's tooth spaces, of a
/// hub's in a bore). For the far end, the seat seen from there ([`far_end`]).
///
/// The body is asked through `probe` (at most [`MOST_PROBES`] times): plain rings and discs over
/// the stretch past the end, and the exact boxes of what they meet. Anything in the way (a
/// shoulder, a bore that narrows), or a body that cannot be read, shuts the end. Past an open end,
/// how far the cuts must run to come out; a step from the very end on (a shaft's neck, a bore's
/// counterbore); and a short stretch belonging to the end (a chamfer or a round), whose slope at
/// two places along it tells a plain chamfer from a round.
pub fn past_end(
    seat: &SeatFrame,
    radius: f64,
    bore: bool,
    reach: f64,
    body: Option<Extent>,
    probe: &mut dyn FnMut(&Probe) -> Met,
) -> End {
    let shut = End::shut(radius);
    let Some(body) = body else { return shut };
    if !a_radius(radius) {
        return shut;
    }
    let zone = -body.low;
    let same = same_size(radius);
    if !zone.is_finite() {
        return shut;
    }
    if zone <= same {
        return End {
            free: true,
            open: true,
            out: true,
            ..shut
        };
    }
    let mut reader = Reader {
        seat,
        probe,
        asked: 0,
    };
    let clear = clear_of(radius);
    let far = radius + body.across + 1.0;
    // Anything in the way: a shoulder larger than the shaft, a bore that narrows.
    let blocked = if bore {
        reader.met(0.0, radius - clear, -(zone + clear), -clear)
    } else {
        reader.met(radius + clear, far, -(zone + clear), -clear)
    };
    if blocked != Met::Nothing {
        return shut;
    }
    // As far as the body has anything at the radius the cuts reach.
    let cut = if bore {
        reader.met(0.0, reach + clear, -(zone + clear), -clear)
    } else {
        reader.met((reach - clear).max(clear), far, -(zone + clear), -clear)
    };
    let stands = match cut {
        Met::Piece { min, max } => reader.piece(min, max).map(|e| -e.low),
        Met::Nothing => Some(0.0),
        Met::Unread => None,
    };
    let (out, through) = match stands {
        Some(stands) if stands.is_finite() && stands <= radius => (true, stands.max(0.0).min(zone)),
        _ => (false, 0.0),
    };
    let open = End {
        open: true,
        out,
        through,
        ..shut
    };
    // The slope of a straight chamfer is told from a round by where it is at two places along it.
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-3 * zone + 10.0 * clear;
    if bore {
        let wider = zone / 16.0;
        let Some(first) = reader.reaches(radius + wider, zone, clear) else {
            return open;
        };
        if first <= 10.0 * clear {
            // Wider from the very end on: a counterbore.
            return End { step: true, ..open };
        }
        if zone > radius || first >= zone - 10.0 * clear {
            // A long stretch, or one hardly wider at the body's end than the bore: not an entry
            // to carry.
            return open;
        }
        // A straight chamfer that is `wider` across where it is `first` long is this far across
        // at the end.
        let across = wider * zone / first;
        if across > 2.0 * zone + same {
            return open;
        }
        let cone = [0.5, 0.9].iter().all(|k| {
            reader
                .reaches(radius + k * across, zone, clear)
                .is_some_and(|at| near(at, k * zone))
        });
        return End {
            end: zone,
            rim: if cone { radius + across } else { radius },
            cone,
            ..open
        };
    }
    let thin = (0.05 * zone).min(1e3 * clear).max(clear);
    let Some(beside) = reader.radius_in(far, -(thin + clear), -clear) else {
        return open;
    };
    if beside < radius - 10.0 * clear {
        // Smaller from the very end on: a neck.
        return End {
            step: true,
            neck: beside,
            ..open
        };
    }
    if zone > radius {
        return open;
    }
    // The body in the last of its length: where a straight chamfer comes to, its rim lies this
    // far in.
    let Some(last) = reader.radius_in(far, -(zone + clear), -zone + thin) else {
        return open;
    };
    let across = (radius - last) * zone / (zone - thin);
    if !(across >= -same && across <= 2.0 * zone + same) {
        return open;
    }
    let cone = across > 10.0 * clear
        && [0.5, 0.1].iter().all(|k| {
            reader
                .radius_in(far, -(k * zone + thin), -k * zone)
                .is_some_and(|at| near(at, radius - k * across))
        });
    End {
        end: zone,
        rim: if cone { radius - across } else { last },
        cone,
        ..open
    }
}

/// The largest radius a body reaches in the stretch `from`..`to` (measured from the seat's free
/// end into a face of radius `radius`) outside the radius `above`; `above` itself where it has
/// nothing there or the stretch is empty. `body` is the body's extent in the seat's frame; the
/// body is asked through `probe` once at most: what a ring over the stretch meets.
///
/// # Errors
///
/// [`RoundFaceRefusal::Number`] for a radius or `above` that is not a number;
/// [`RoundFaceRefusal::Probe`] where what the ring meets could not be read (a stretch that is
/// not finite among them). Never taken as nothing standing there.
pub fn stands_over(
    seat: &SeatFrame,
    radius: f64,
    body: Extent,
    above: f64,
    from: f64,
    to: f64,
    probe: &mut dyn FnMut(&Probe) -> Met,
) -> Result<f64, RoundFaceRefusal> {
    if !(a_radius(radius) && above.is_finite()) {
        return Err(RoundFaceRefusal::Number);
    }
    let clear = clear_of(radius);
    if !(to > from) || body.across <= above + clear {
        return Ok(above);
    }
    let mut reader = Reader {
        seat,
        probe,
        asked: 0,
    };
    match reader.met(above + clear, body.across + 1.0, from, to) {
        Met::Piece { min, max } => reader
            .piece(min, max)
            .map(|e| e.across.max(above))
            .ok_or(RoundFaceRefusal::Probe),
        Met::Nothing => Ok(above),
        Met::Unread => Err(RoundFaceRefusal::Probe),
    }
}

/// Whether a body leaves the stretch just past the seat's end (a thousand times the clearance of
/// a face of radius `radius`) empty within `within` of the axis: a chamfer that wide at a bore's
/// step cuts into no wall there. The body is asked through `probe` once: what a disc over the
/// stretch meets.
///
/// # Errors
///
/// [`RoundFaceRefusal::Number`] for a radius or `within` that is not finite and above zero;
/// [`RoundFaceRefusal::Probe`] where what the disc meets could not be read.
pub fn beside_clear(
    seat: &SeatFrame,
    radius: f64,
    within: f64,
    probe: &mut dyn FnMut(&Probe) -> Met,
) -> Result<bool, RoundFaceRefusal> {
    if !(a_radius(radius) && a_radius(within)) {
        return Err(RoundFaceRefusal::Number);
    }
    let clear = clear_of(radius);
    let mut reader = Reader {
        seat,
        probe,
        asked: 0,
    };
    match reader.met(0.0, within, -(1e3 * clear + clear), -clear) {
        Met::Nothing => Ok(true),
        Met::Piece { .. } => Ok(false),
        Met::Unread => Err(RoundFaceRefusal::Probe),
    }
}

/// How a line through `p` along `dir` lies to the axis through `o` along the unit `k`: the length
/// of `dir` cross `k` (0 when parallel), and the distance of `p` from the axis line.
fn axis_gap(o: Vec3, k: Vec3, p: Vec3, dir: Vec3) -> (f64, f64) {
    let off = p - o;
    (dir.cross(k).length(), (off - k * off.dot(k)).length())
}

/// The unit axis of a round's frame (its x cross y); none where it has no direction.
fn round_axis(round: &Round) -> Option<Vec3> {
    normal(&round.frame).unitize().ok()
}

/// Whether the round `round` lies about the axis through `o` along `k` with the radius `r`, its
/// axis leaning at most `lean` from it, within `near`.
fn round_on(round: &Round, o: Vec3, k: Vec3, r: f64, near: f64, lean: f64) -> bool {
    let Some(axis) = round_axis(round) else {
        return false;
    };
    let (leans, off) = axis_gap(o, k, v(round.frame.origin), axis);
    (round.radius - r).abs() <= near && leans <= lean && off <= near
}

/// Whether the edge bounds a band of the cylinder of radius `r` about the axis through `o` along
/// `k` within `near`: an end of the band (a circle across the axis, about it, of the cylinder's
/// radius) or a seam (a straight line along the axis). An edge held as another curve is read from
/// its ends and the points along it.
fn bounds_the_band(edge: &BandEdge, o: Vec3, k: Vec3, r: f64, near: f64) -> bool {
    if let Some(circle) = &edge.circle {
        return round_on(circle, o, k, r, near, 1e-6);
    }
    let Some([a, b]) = edge.ends else {
        return false;
    };
    let chord = b - a;
    let length = chord.length();
    let seam = length > near && chord.cross(k).length() <= 1e-6 * length;
    if edge.line {
        return seam;
    }
    if edge.points.is_empty() {
        return false;
    }
    let all = || [a, b].into_iter().chain(edge.points.iter().copied());
    // A seam held as another curve: every point on the chord's line.
    let straight = seam
        && all().all(|p| {
            let (_, off) = axis_gap(a, chord * (1.0 / length), p, chord);
            off <= near
        });
    // An end of the band held as another curve: every point on the cylinder, at one place along
    // the axis.
    let level = (a - o).dot(k);
    let rim = all().all(|p| {
        let (_, off) = axis_gap(o, k, p, k);
        (off - r).abs() <= near && ((p - o).dot(k) - level).abs() <= near
    });
    straight || rim
}

/// Whether the round face of radius `radius` on the seat is a whole band of the body whose faces
/// are `faces`: every face on the seat's cylinder is bounded only by circles across the axis on
/// that cylinder (its ends) and by straight lines along the axis (seams). A hole across a shaft, a
/// flat, a keyway or a set screw's hole in a hub that opens in the face leaves other edges in it.
///
/// # Errors
///
/// [`RoundFaceRefusal::Number`] for a radius that is not finite and above zero;
/// [`RoundFaceRefusal::Seat`] for a seat whose plane has no normal;
/// [`RoundFaceRefusal::NoBand`] where no face lies on the seat's cylinder.
pub fn whole_band(
    seat: &SeatFrame,
    radius: f64,
    faces: &[BandFace],
) -> Result<bool, RoundFaceRefusal> {
    if !a_radius(radius) {
        return Err(RoundFaceRefusal::Number);
    }
    let k = normal(&seat.plane)
        .unitize()
        .map_err(|_| RoundFaceRefusal::Seat)?;
    let o = v(seat.plane.origin);
    if !(o.x.is_finite() && o.y.is_finite() && o.z.is_finite()) {
        return Err(RoundFaceRefusal::Seat);
    }
    let near = clear_of(radius);
    let on: Vec<&BandFace> = faces
        .iter()
        .filter(|f| {
            f.cylinder
                .as_ref()
                .is_some_and(|c| round_on(c, o, k, radius, near, 1e-9))
        })
        .collect();
    if on.is_empty() {
        return Err(RoundFaceRefusal::NoBand);
    }
    Ok(on.iter().all(|f| band_edges(f, o, k, radius, near)))
}

/// Whether every edge of the face bounds the band ([`bounds_the_band`]).
fn band_edges(face: &BandFace, o: Vec3, k: Vec3, r: f64, near: f64) -> bool {
    face.edges.iter().all(|e| bounds_the_band(e, o, k, r, near))
}

/// The block that takes the stretch of a body a round face of radius `radius` runs along out of
/// it, for the face to be grown by `by` there on its own: a round slab wider than the body (whose
/// extent in the seat's frame is `body`) and than the grown face, from `before` past the free end
/// to `after` past the far end.
///
/// # Errors
///
/// [`RoundFaceRefusal::Number`] for a number that is not finite, a radius not above zero, or a
/// stretch that comes to no length.
pub fn grow_block(
    seat: &SeatFrame,
    radius: f64,
    body: Extent,
    by: f64,
    before: f64,
    after: f64,
) -> Result<GrowBlock, RoundFaceRefusal> {
    let slab = body.across + radius + by.abs() + 1.0;
    let (from, to) = span(seat, -before, seat.room + after);
    let numbers = [
        radius,
        body.across,
        by,
        before,
        after,
        seat.room,
        slab,
        from,
        to,
    ];
    if !(a_radius(radius) && numbers.iter().all(|x| x.is_finite()) && slab > 0.0 && from < to) {
        return Err(RoundFaceRefusal::Number);
    }
    Ok(GrowBlock {
        plane: seat.plane.clone(),
        radius: slab,
        from,
        to,
    })
}
