//! Grabbing the model.
//!
//! - One hand's grip held: the model is held like a thing in the hand. It moves and turns with
//!   the hand, about the hand.
//! - Both grips held, as [`TwoHands`] says: the model grows or shrinks as the hands move apart or
//!   together and moves with them; it turns with them too, or only with the hand that took hold
//!   first, or not at all. Brought near its real size, it snaps to real size (unless asked not
//!   to) and stays there until the hands move it clearly away again.
//!
//! The room and its floor never move; only the model does. Whenever which hands hold changes,
//! the grab starts again from where the model is, so the model never jumps when a second hand
//! joins or one lets go.

use solidcraft_xr::view::Pose;

use crate::room::{MAX_SCALE, MIN_SCALE, REAL_SIZE, Transform};
use crate::vector::{add, cross, dot, finite, length, scale, sub, turn, unit};

/// What both grabs do together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TwoHands {
    /// Scale about the point between the hands and move with it; no turn.
    #[default]
    ScaleMove,
    /// Both hands equal: scale, move with the point between them, and turn with the line between
    /// them and with their roll about it. Hands closer than [`MIN_SPAN`] do not turn it.
    Equal,
    /// The hand that took hold first holds and turns the model as one hand does; the other only
    /// scales it, by its distance to the first.
    Lead,
}

/// How the grab behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub two_hands: TwoHands,
    /// A two-handed scale near real size snaps to it.
    pub snap: bool,
}

impl Default for Mode {
    /// As the grab was before it had settings: scale and move, snapping to real size.
    fn default() -> Self {
        Mode { two_hands: TwoHands::ScaleMove, snap: true }
    }
}

/// Hands closer than this (metres) are taken as this far apart: their distance never sets a
/// scale from nothing.
pub const MIN_SPAN: f64 = 0.01;

/// A two-handed scale brought within this fraction of [`REAL_SIZE`] snaps to it...
pub const SNAP_IN: f64 = 0.06;

/// ...and lets go again only past this fraction, so a hand's tremor at the edge does not flick
/// it in and out.
pub const SNAP_OUT: f64 = 0.10;

/// Whether `scale` is within `fraction` of [`REAL_SIZE`], either way (by ratio).
fn near_real_size(scale: f64, fraction: f64) -> bool {
    let off = (scale / REAL_SIZE).ln().abs();
    off.is_finite() && off < (1.0 + fraction).ln()
}

/// The real-size snap of one two-handed grab.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    /// The scale is held at real size.
    on: bool,
    /// The hands have scaled the model out of [`SNAP_IN`] during this grab, so bringing it back
    /// snaps. A grab that starts just off real size is not snapped by the second hand joining.
    armed: bool,
}

impl Snap {
    /// The snap as a grab starts from a model at `scale`: on when it is at real size already,
    /// armed when it is out of [`SNAP_IN`].
    fn starting(scale: f64) -> Snap {
        Snap { on: scale == REAL_SIZE, armed: !near_real_size(scale, SNAP_IN) }
    }

    /// The snap after the hands scaled the model to `raw`, and the scale to show.
    fn next(self, raw: f64) -> (Snap, f64) {
        let next = if self.on {
            Snap { on: near_real_size(raw, SNAP_OUT), armed: true }
        } else if near_real_size(raw, SNAP_IN) {
            Snap { on: self.armed, armed: self.armed }
        } else {
            Snap { on: false, armed: true }
        };
        (next, if next.on { REAL_SIZE } else { raw })
    }
}

/// Where the hands held from, and the model as it was then.
// One lives in each grab, never in a list: the size of the two-handed state costs nothing, and
// boxing it would cost the grab its `Copy`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Held {
    Free,
    /// One hand: where it was (position, and its turn as the columns of a matrix).
    One {
        hand: usize,
        at: [f64; 3],
        turn: [[f64; 3]; 3],
        start: Transform,
    },
    /// Both hands: where each was and how it was turned, the real-size snap, which hand took
    /// hold first, and the turn of both hands together ([`TwoHands::Equal`]).
    Two {
        from: [[f64; 3]; 2],
        turns: [[[f64; 3]; 3]; 2],
        start: Transform,
        snap: Snap,
        lead: usize,
        line: Line,
    },
}

/// The turn both hands make together ([`TwoHands::Equal`]), which follows the line between
/// them. Hands closer than [`MIN_SPAN`] have no line to follow (a millimetre of tracking jitter
/// would swing it by degrees): the turn holds still until they part again, and then starts from
/// where the line is then, so the model neither spins nor jumps.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Line {
    /// The turn made so far in this grab (as columns), as of the last frame.
    turned: [[f64; 3]; 3],
    /// Where the hands were and how they were turned when the line was last taken up, and the
    /// turn made before that; `None` while the hands are closer than [`MIN_SPAN`].
    base: Option<LineBase>,
}

/// Where the line between the hands was taken up from (see [`Line`]).
#[derive(Clone, Copy, Debug, PartialEq)]
struct LineBase {
    at: [[f64; 3]; 2],
    turns: [[[f64; 3]; 3]; 2],
    before: [[f64; 3]; 3],
}

/// No turn, as columns.
const NO_TURN: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Whether hands at `at` are far enough apart for the line between them to say where it points.
fn apart(at: [[f64; 3]; 2]) -> bool {
    let d = length(sub(at[1], at[0]));
    d.is_finite() && d >= MIN_SPAN
}

impl Line {
    /// The line as both hands take hold at `at`, turned `turns`: no turn made yet.
    fn starting(at: [[f64; 3]; 2], turns: [[[f64; 3]; 3]; 2]) -> Line {
        Line { turned: NO_TURN, base: apart(at).then_some(LineBase { at, turns, before: NO_TURN }) }
    }

    /// The turn made so far with the hands now at `at`, turned `now`: none added while they are
    /// closer than [`MIN_SPAN`]; when they part again, the line is taken up from there.
    fn next(&mut self, at: [[f64; 3]; 2], now: [[[f64; 3]; 3]; 2]) -> [[f64; 3]; 3] {
        if !apart(at) {
            self.base = None;
            return self.turned;
        }
        let base = *self.base.get_or_insert(LineBase { at, turns: now, before: self.turned });
        self.turned = multiply(&hands_turn(base.at, base.turns, at, now), &base.before);
        self.turned
    }
}

/// The grab between frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldGrab {
    held: Held,
}

impl Default for WorldGrab {
    /// Nothing held.
    fn default() -> Self {
        WorldGrab { held: Held::Free }
    }
}

impl WorldGrab {
    /// Whether a hand holds the model now.
    pub fn holding(&self) -> bool {
        !matches!(self.held, Held::Free)
    }

    /// Let go, whatever the hands hold (the model was put back, or tracking was lost).
    pub fn release(&mut self) {
        self.held = Held::Free;
    }

    /// The model this frame: `current` moved by the hands that hold it, each given by its grip's
    /// pose in the room (left first; `None`: that hand does not hold). A hand whose pose is not
    /// one (no finite place, no turn) does not hold. The result is always tidy
    /// ([`Transform::tidy`]); `current` itself is kept when moving it would not be (its middle
    /// out of reach).
    pub fn update(&mut self, current: &Transform, grips: [Option<Pose>; 2]) -> Transform {
        self.update_with(current, grips, Mode::default())
    }

    /// [`WorldGrab::update`], the grab behaving as `mode` says.
    pub fn update_with(&mut self, current: &Transform, grips: [Option<Pose>; 2], mode: Mode) -> Transform {
        let grips = grips.map(|g| g.and_then(|pose| Some((pose.position, pose.axes()?))));
        let moved = match (grips, self.held) {
            ([None, None], _) => {
                self.held = Held::Free;
                None
            }
            ([Some((at, now)), None], Held::One { hand: 0, at: from, turn: was, start })
            | ([None, Some((at, now))], Held::One { hand: 1, at: from, turn: was, start }) => Some(held_in_hand(&start, from, &was, at, &now)),
            ([Some((at, turn)), None], _) => {
                self.held = Held::One { hand: 0, at, turn, start: *current };
                None
            }
            ([None, Some((at, turn))], _) => {
                self.held = Held::One { hand: 1, at, turn, start: *current };
                None
            }
            ([Some((left, left_turn)), Some((right, right_turn))], Held::Two { from, turns, start, snap, lead, mut line }) => {
                let snap = if mode.snap { snap } else { Snap { on: false, armed: false } };
                let (snap, moved) = two_handed(&start, snap, &mut line, from, turns, [left, right], [left_turn, right_turn], lead, mode.two_hands);
                self.held = Held::Two { from, turns, start, snap, lead, line };
                Some(moved)
            }
            ([Some((left, left_turn)), Some((right, right_turn))], held) => {
                // The hand already holding leads; both at once: the right.
                let lead = match held {
                    Held::One { hand, .. } => hand,
                    _ => 1,
                };
                let snap = if mode.snap { Snap::starting(current.scale) } else { Snap { on: false, armed: false } };
                let (from, turns) = ([left, right], [left_turn, right_turn]);
                self.held = Held::Two { from, turns, start: *current, snap, lead, line: Line::starting(from, turns) };
                None
            }
        };
        moved.and_then(|t| t.tidy()).unwrap_or(*current)
    }
}

/// `start` carried by a hand that went from `from` (turned `was`) to `at` (turned `now`): every
/// model point keeps where it is in the hand.
fn held_in_hand(start: &Transform, from: [f64; 3], was: &[[f64; 3]; 3], at: [f64; 3], now: &[[f64; 3]; 3]) -> Transform {
    // The hand's turn since: now · wasᵀ, as columns.
    let delta: [[f64; 3]; 3] = std::array::from_fn(|k| turn(now, [was[0][k], was[1][k], was[2][k]]));
    // A room point p goes to at + delta · (p − from).
    let axes = start.axes.map(|a| turn(&delta, a));
    let translation = add(at, turn(&delta, sub(start.translation, from)));
    Transform { axes, translation, ..*start }
}

/// `start` moved by both hands, which went from `from` (turned `turns`) to `at` (turned `now`), as
/// `two_hands` says; `lead` the hand that took hold first; `snap` the grab's real-size snap;
/// `line` the turn both hands make together.
#[allow(clippy::too_many_arguments)]
fn two_handed(
    start: &Transform,
    snap: Snap,
    line: &mut Line,
    from: [[f64; 3]; 2],
    turns: [[[f64; 3]; 3]; 2],
    at: [[f64; 3]; 2],
    now: [[[f64; 3]; 3]; 2],
    lead: usize,
    two_hands: TwoHands,
) -> (Snap, Transform) {
    let distance = length(sub(from[1], from[0]));
    let now_distance = length(sub(at[1], at[0]));
    let grow = if distance > MIN_SPAN && now_distance.is_finite() { now_distance.max(MIN_SPAN) / distance } else { 1.0 };
    // Within the scale's limits: past them the hands no longer scale, and the model does not
    // creep away from between them.
    let (snap, s) = snap.next((start.scale * grow).clamp(MIN_SCALE, MAX_SCALE));
    let grow = s / start.scale;
    let middle = |p: [[f64; 3]; 2]| scale(add(p[0], p[1]), 0.5);
    let moved = match two_hands {
        // A room point p goes to now + grow · (p − from), about the points between the hands.
        TwoHands::ScaleMove => {
            let (m0, m1) = (middle(from), middle(at));
            Transform { scale: s, translation: add(m1, scale(sub(start.translation, m0), grow)), ..*start }
        }
        TwoHands::Equal => {
            let (m0, m1) = (middle(from), middle(at));
            let delta = line.next(at, now);
            let axes = start.axes.map(|a| turn(&delta, a));
            Transform { axes, scale: s, translation: add(m1, scale(turn(&delta, sub(start.translation, m0)), grow)), ..*start }
        }
        TwoHands::Lead => {
            let l = lead.min(1);
            let held = held_in_hand(start, from[l], &turns[l], at[l], &now[l]);
            Transform { scale: s, translation: add(at[l], scale(sub(held.translation, at[l]), grow)), ..held }
        }
    };
    (snap, moved)
}

/// The turn both hands made together: the line between them from where it pointed to where it
/// points, then about that line by the hands' own roll about it, averaged. Each hand's roll
/// counts by how far its up stands across the line, before and after: a hand whose up lies
/// nearly along the line says little about a roll, and its noise is not taken for one.
fn hands_turn(from: [[f64; 3]; 2], turns: [[[f64; 3]; 3]; 2], at: [[f64; 3]; 2], now: [[[f64; 3]; 3]; 2]) -> [[f64; 3]; 3] {
    let (Some(v0), Some(v1)) = (unit(sub(from[1], from[0])), unit(sub(at[1], at[0]))) else { return NO_TURN };
    let swing = arc(v0, v1);
    // Each hand's roll about the new line: where its up went, against where the swing alone
    // would have put it.
    let (mut sin, mut cos) = (0.0, 0.0);
    for h in 0..2 {
        let across = |v: [f64; 3], line: [f64; 3]| sub(v, scale(line, dot(v, line)));
        let (was, is) = (across(turns[h][1], v0), across(now[h][1], v1));
        let (Some(was_dir), Some(is_dir)) = (unit(was), unit(is)) else { continue };
        let weight = length(was) * length(is);
        let swung = turn(&swing, was_dir);
        let angle = dot(cross(swung, is_dir), v1).atan2(dot(swung, is_dir));
        sin += weight * angle.sin();
        cos += weight * angle.cos();
    }
    let roll = if sin == 0.0 && cos == 0.0 { 0.0 } else { sin.atan2(cos) };
    multiply(&about(v1, roll), &swing)
}

/// The turn by `angle` (radians, right-handed) about the unit `axis`, as columns.
fn about(axis: [f64; 3], angle: f64) -> [[f64; 3]; 3] {
    let (s, c) = angle.sin_cos();
    let [x, y, z] = axis;
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y + s * z, t * x * z - s * y],
        [t * x * y - s * z, t * y * y + c, t * y * z + s * x],
        [t * x * z + s * y, t * y * z - s * x, t * z * z + c],
    ]
}

/// The shortest turn taking unit `a` to unit `b`.
fn arc(a: [f64; 3], b: [f64; 3]) -> [[f64; 3]; 3] {
    let c = dot(a, b);
    match unit(cross(a, b)) {
        Some(axis) => about(axis, cross(a, b).iter().map(|v| v * v).sum::<f64>().sqrt().atan2(c)),
        // Along each other: no turn; opposite: half a turn about any line across.
        None if c > 0.0 => about([1.0, 0.0, 0.0], 0.0),
        None => {
            let across = unit(cross(a, [1.0, 0.0, 0.0])).or_else(|| unit(cross(a, [0.0, 1.0, 0.0]))).unwrap_or([0.0, 0.0, 1.0]);
            about(across, std::f64::consts::PI)
        }
    }
}

/// `a` · `b`, matrices as columns.
fn multiply(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    b.map(|column| turn(a, column))
}

/// The model turned by `angle` (radians) about the room's vertical through `centre` (the room).
pub fn turned_about_vertical(t: &Transform, centre: [f64; 3], angle: f64) -> Transform {
    turned_about(t, centre, [0.0, 1.0, 0.0], angle)
}

/// The model turned by `angle` (radians) about `axis` (the room, made unit) through `centre`. An
/// axis with no direction, or an angle or centre that is no number, leaves the model as it is.
pub fn turned_about(t: &Transform, centre: [f64; 3], axis: [f64; 3], angle: f64) -> Transform {
    let Some(axis) = unit(axis).filter(|_| angle.is_finite() && finite(centre)) else { return *t };
    let r = about(axis, angle);
    Transform { axes: t.axes.map(|a| turn(&r, a)), translation: add(centre, turn(&r, sub(t.translation, centre))), ..*t }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::turn_about_vertical;

    /// Whether two points are the same but for rounding.
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    const START: Transform = Transform::MILLIMETRES;

    /// A hand at `at`, turned `angle` about the vertical.
    fn hand(at: [f64; 3], angle: f64) -> Option<Pose> {
        let (s, c) = (angle * 0.5).sin_cos();
        Some(Pose { position: at, orientation: [0.0, s, 0.0, c] })
    }

    /// A hand at `at`, turned `angle` about the room's X (a wrist tipped forward).
    fn tipped(at: [f64; 3], angle: f64) -> Option<Pose> {
        let (s, c) = (angle * 0.5).sin_cos();
        Some(Pose { position: at, orientation: [s, 0.0, 0.0, c] })
    }

    /// Both hands `half` either side of x = 0, at height 1 m.
    fn spread(half: f64) -> [Option<Pose>; 2] {
        around(half, [0.0, 1.0, 0.0])
    }

    /// Both hands `half` either side of `middle` along the room's X.
    fn around(half: f64, middle: [f64; 3]) -> [Option<Pose>; 2] {
        [hand([middle[0] - half, middle[1], middle[2]], 0.0), hand([middle[0] + half, middle[1], middle[2]], 0.0)]
    }

    /// One hand moves the model with it; letting go keeps it where it is; a hand that does not
    /// hold moves nothing.
    #[test]
    fn one_hand_moves_the_model() {
        let mut grab = WorldGrab::default();
        let t = grab.update(&START, [None, hand([0.0, 1.0, 0.0], 0.0)]);
        assert_eq!(t, START, "taking hold does not move it");
        assert!(grab.holding());
        let t = grab.update(&t, [None, hand([0.1, 1.2, -0.3], 0.0)]);
        assert!(close(t.translation, [0.1, 0.2, -0.3]));
        assert_eq!(t.axes, START.axes);
        assert_eq!(t.scale, START.scale);
        let t = grab.update(&t, [None, None]);
        assert!(close(t.translation, [0.1, 0.2, -0.3]));
        assert!(!grab.holding());
        let t = grab.update(&t, [None, None]);
        assert!(close(t.translation, [0.1, 0.2, -0.3]));
    }

    /// One hand turns the model as it turns, about itself, any way: a point in the hand stays in
    /// the hand, and the rest goes round it.
    #[test]
    fn one_hand_turns_the_model_about_the_hand() {
        let mut grab = WorldGrab::default();
        let at = [0.2, 1.0, -0.4];
        grab.update(&START, [hand(at, 0.0), None]);
        // A model point in the hand, and one 10 cm to its right.
        let in_hand = [200.0, 1000.0, -400.0];
        let right = [300.0, 1000.0, -400.0];
        let quarter = std::f64::consts::FRAC_PI_2;
        let t = grab.update(&START, [hand(at, quarter), None]);
        assert!(close(t.apply(in_hand), at), "the point in the hand stays there");
        assert!(close(t.apply(right), [0.2, 1.0, -0.5]), "the point to the right swings to the front");
        assert_eq!(t.scale, START.scale);
        // Tipped forward, the model tips with the hand: it is held, not the room.
        let t = grab.update(&START, [tipped(at, quarter), None]);
        assert!(close(turn(&t.axes, [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]), "the model's Y follows the wrist");
        // Moved and turned at once.
        let t = grab.update(&START, [hand([0.2, 1.5, -0.4], quarter), None]);
        assert!(close(t.apply(in_hand), [0.2, 1.5, -0.4]));
        assert!(close(t.apply(right), [0.2, 1.5, -0.5]));
    }

    /// Both hands scale about the point between them and move with it, and never turn the model,
    /// however the hands themselves are turned.
    #[test]
    fn two_hands_scale_and_move_without_turning() {
        let mut grab = WorldGrab::default();
        let turned = Transform { axes: turn_about_vertical(0.4), ..START };
        let (left, right) = ([-0.1, 1.0, -0.5], [0.1, 1.0, -0.5]);
        grab.update(&START, [hand(left, 0.0), hand(right, 0.0)]);
        let held = [100.0, 1000.0, -500.0];
        assert!(close(START.apply(held), right));

        let t = grab.update(&START, [hand([-0.2, 1.0, -0.5], 0.0), hand([0.2, 1.0, -0.5], 0.0)]);
        assert!((t.scale - 2.0 * START.scale).abs() < 1e-15);
        assert!(close(t.apply(held), [0.2, 1.0, -0.5]), "the held point stays in the hand");

        let t = grab.update(&START, [hand([-0.1, 2.0, -0.5], 0.0), hand([0.1, 2.0, -0.5], 0.0)]);
        assert!(close(t.apply(held), [0.1, 2.0, -0.5]), "both hands up: the model with them");

        // A turned model, hands that turn and swing round: the model keeps its turn.
        let mut grab = WorldGrab::default();
        grab.update(&turned, [hand(left, 0.0), hand(right, 0.0)]);
        let t = grab.update(&turned, [hand([0.0, 1.0, -0.4], 1.0), hand([0.0, 1.0, -0.6], -2.0)]);
        assert_eq!(t.axes, turned.axes, "two hands never turn it");
    }

    /// A grab that brings the scale near real size snaps to it, holds it there through small
    /// moves, lets go only well past it, and snaps again on the way back.
    #[test]
    fn two_hands_snap_to_real_size_with_a_margin() {
        let half = Transform { scale: REAL_SIZE * 0.5, ..START };
        let mut grab = WorldGrab::default();
        grab.update(&half, spread(0.1));
        let scale_at = |grab: &mut WorldGrab, half_span: f64| grab.update(&half, spread(half_span)).scale / REAL_SIZE;
        assert!((scale_at(&mut grab, 0.15) - 0.75).abs() < 1e-12, "far off: as the hands say");
        assert_eq!(scale_at(&mut grab, 0.195), 1.0, "0.975: snapped");
        assert_eq!(scale_at(&mut grab, 0.21), 1.0, "1.05: still held");
        assert_eq!(scale_at(&mut grab, 0.214), 1.0, "1.07, past where it snaps in: still held");
        assert!((scale_at(&mut grab, 0.225) - 1.125).abs() < 1e-12, "1.125: let go");
        assert!((scale_at(&mut grab, 0.214) - 1.07).abs() < 1e-12, "1.07 on the way back: not yet");
        assert_eq!(scale_at(&mut grab, 0.21), 1.0, "1.05: snapped again");
    }

    /// When a second hand joins or one lets go, the grab starts again from where the model is:
    /// no jump, on that frame or the next, also just off real size and after a turn.
    #[test]
    fn a_hand_joining_or_letting_go_does_not_jump() {
        let near = Transform { scale: REAL_SIZE * 1.05, ..START };
        let mut grab = WorldGrab::default();
        let mut t = grab.update(&near, [hand([0.0, 1.0, 0.0], 0.0), None]);
        t = grab.update(&t, [hand([0.5, 1.0, 0.0], 0.7), None]);
        let hands = [hand([0.5, 1.0, 0.0], 0.7), hand([3.0, 7.0, 1.0], 0.0)];
        let joined = grab.update(&t, hands);
        assert_eq!(joined, t, "the second hand joining moves nothing");
        let still = grab.update(&joined, hands);
        assert!(close(still.translation, joined.translation) && still.scale == joined.scale, "nor the frame after: no snap");
        let left = grab.update(&still, [None, hand([3.0, 7.0, 1.0], 2.0)]);
        assert_eq!(left, still, "the first hand letting go moves nothing, whatever the other's turn");
        let moved = grab.update(&left, [None, hand([3.0, 7.5, 1.0], 2.0)]);
        assert!(close(moved.translation, add(left.translation, [0.0, 0.5, 0.0])));
        let other = grab.update(&moved, [hand([9.0, 9.0, 9.0], 1.0), None]);
        assert_eq!(other, moved, "the other hand taking over");
        // A grab that starts at real size stays there while the hands hold still.
        let mut grab = WorldGrab::default();
        grab.update(&START, spread(0.1));
        assert_eq!(grab.update(&START, spread(0.1)).scale, REAL_SIZE);
    }

    /// Hands brought closer than [`MIN_SPAN`] keep the scale they had there, not the grab's
    /// first one; hands that start at one place set no scale.
    #[test]
    fn hands_brought_together_do_not_jump_back() {
        let far = Transform { scale: REAL_SIZE * 3.0, ..START };
        let mut grab = WorldGrab::default();
        grab.update(&far, spread(0.1));
        let close_in = grab.update(&far, spread(0.006)).scale;
        let touching = grab.update(&far, spread(0.0)).scale;
        assert!((close_in - far.scale * 0.06).abs() < 1e-15, "{close_in}");
        assert!((touching - far.scale * 0.05).abs() < 1e-15 && touching < close_in, "closer than the least span: no jump back up ({touching})");

        let mut grab = WorldGrab::default();
        grab.update(&far, [hand([0.0; 3], 0.0), hand([0.0; 3], 0.0)]);
        assert_eq!(grab.update(&far, [hand([0.0; 3], 0.0), hand([0.5, 0.0, 0.0], 0.0)]).scale, far.scale);
    }

    /// Hands at no place, poses with no turn, scaling past the limits and moving the model out
    /// of reach: the model stays usable and where it was, and a held point stays in the hand.
    #[test]
    fn the_grab_refuses_hands_that_say_nothing() {
        let mut grab = WorldGrab::default();
        let t = grab.update(&START, [hand([f64::NAN, 0.0, 0.0], 0.0), hand([f64::INFINITY; 3], 0.0)]);
        assert_eq!(t, START);
        assert!(!grab.holding(), "a hand at no place does not hold");
        let unturned = Some(Pose { position: [0.0; 3], orientation: [0.0; 4] });
        assert_eq!(grab.update(&START, [unturned, None]), START);
        assert!(!grab.holding(), "a pose with no turn does not hold");

        // About the model's middle, the scale stops at its limits.
        let mut grab = WorldGrab::default();
        grab.update(&START, around(0.1, [0.0; 3]));
        assert_eq!(grab.update(&START, around(1e6, [0.0; 3])).scale, MAX_SCALE);
        let small = grab.update(&START, around(0.01, [0.0; 3]));
        assert!(small.scale >= MIN_SCALE && small.scale < START.scale);
        // About a point a metre off the model, scaling up ten thousand times would throw it ten
        // kilometres away: refused.
        let mut grab = WorldGrab::default();
        grab.update(&START, spread(0.1));
        assert_eq!(grab.update(&START, spread(1e6)), START);

        // Carried out of reach: refused, the model stays where it was.
        let mut grab = WorldGrab::default();
        grab.update(&START, [hand([0.0, 1.0, 0.0], 0.0), None]);
        let gone = grab.update(&START, [hand([1e7, 1.0, 0.0], 0.0), None]);
        assert_eq!(gone, START);

        // Scaled up a lot about the middle of a part far from its own origin: the middle stays
        // between the hands while the origin goes far away.
        let part = Transform { translation: [0.0, 0.0, -2.0], pivot: [0.0, 0.0, 2000.0], ..START };
        let mut grab = WorldGrab::default();
        grab.update(&part, around(0.1, [0.0; 3]));
        let up = grab.update(&part, around(5.0, [0.0; 3]));
        assert!((up.scale - 50.0 * START.scale).abs() < 1e-12);
        assert!(close(up.apply(up.pivot), [0.0; 3]), "the middle stays between the hands");
        assert!(close(up.translation, [0.0, 0.0, -100.0]), "its origin goes 100 m out");
    }

    /// Letting go and taking hold again every frame while turning keeps the model's axes square:
    /// turns added up do not drift.
    #[test]
    fn many_grabs_do_not_drift() {
        let mut grab = WorldGrab::default();
        let mut t = START;
        for k in 0..2000 {
            grab.update(&t, [hand([0.0, 1.0, 0.0], 0.0), None]);
            t = grab.update(&t, [hand([0.0, 1.0, 0.0], 0.37 + f64::from(k % 3) * 1e-3), None]);
            grab.release();
        }
        for i in 0..3 {
            assert!((crate::vector::dot(t.axes[i], t.axes[i]) - 1.0).abs() < 1e-12);
            assert!(crate::vector::dot(t.axes[i], t.axes[(i + 1) % 3]).abs() < 1e-12);
        }
        assert!(close(turn(&t.axes, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]), "turns about the vertical keep Y up");
    }

    /// Both hands as [`TwoHands::Equal`] says.
    const EQUAL: Mode = Mode { two_hands: TwoHands::Equal, snap: true };

    /// Without settings the grab scales and moves with both hands and snaps to real size, as it
    /// did before it had settings.
    #[test]
    fn the_default_mode_is_the_grab_as_it_was() {
        assert_eq!(Mode::default(), Mode { two_hands: TwoHands::ScaleMove, snap: true });
        assert_eq!(TwoHands::default(), TwoHands::ScaleMove);
        let frames = [spread(0.1), spread(0.15), [hand([0.0, 1.0, 0.0], 0.3), None], spread(0.2)];
        let (mut a, mut b) = (WorldGrab::default(), WorldGrab::default());
        let (mut ta, mut tb) = (START, START);
        for grips in frames {
            ta = a.update(&ta, grips);
            tb = b.update_with(&tb, grips, Mode::default());
            assert_eq!(ta, tb);
        }
    }

    /// Both hands equal, swung a quarter about the point between them: the model swings with
    /// the line between them, and what each hand held stays in it.
    #[test]
    fn both_hands_equal_turn_the_model_with_the_line_between_them() {
        let mut grab = WorldGrab::default();
        grab.update_with(&START, [hand([-0.1, 1.0, -0.5], 0.0), hand([0.1, 1.0, -0.5], 0.0)], EQUAL);
        let quarter = std::f64::consts::FRAC_PI_2;
        let t = grab.update_with(&START, [hand([0.0, 1.0, -0.4], quarter), hand([0.0, 1.0, -0.6], quarter)], EQUAL);
        assert!(close(t.apply([100.0, 1000.0, -500.0]), [0.0, 1.0, -0.6]), "the right hand's point stays in it");
        assert!(close(t.apply([-100.0, 1000.0, -500.0]), [0.0, 1.0, -0.4]), "the left hand's too");
        assert!(close(turn(&t.axes, [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]), "X swung to −Z");
        assert!(close(turn(&t.axes, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]), "up stays up");
        assert_eq!(t.scale, START.scale);
    }

    /// Both hands equal, both wrists rolled about the line between them: the model rolls with
    /// them; wrists rolled opposite ways cancel out.
    #[test]
    fn both_hands_equal_roll_the_model_with_the_wrists() {
        let (left, right) = ([-0.1, 1.0, -0.5], [0.1, 1.0, -0.5]);
        let quarter = std::f64::consts::FRAC_PI_2;
        let mut grab = WorldGrab::default();
        grab.update_with(&START, [hand(left, 0.0), hand(right, 0.0)], EQUAL);
        let t = grab.update_with(&START, [tipped(left, quarter), tipped(right, quarter)], EQUAL);
        assert!(close(turn(&t.axes, [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]), "the model's Y follows the wrists");
        assert!(close(t.apply([0.0, 1000.0, -500.0]), [0.0, 1.0, -0.5]), "about the point between the hands");
        let t = grab.update_with(&START, [tipped(left, 0.5), tipped(right, -0.5)], EQUAL);
        assert!(close(t.axes[1], [0.0, 1.0, 0.0]) && close(t.translation, START.translation), "{t:?}");
    }

    /// Both hands equal, swapping sides without rolling: half a turn about the vertical, the
    /// model still upright and each held point still in its hand.
    #[test]
    fn hands_swapping_sides_turn_the_model_half_about_the_vertical() {
        let (left, right) = ([-0.1, 1.0, -0.5], [0.1, 1.0, -0.5]);
        let mut grab = WorldGrab::default();
        grab.update_with(&START, [hand(left, 0.0), hand(right, 0.0)], EQUAL);
        let t = grab.update_with(&START, [hand(right, 0.0), hand(left, 0.0)], EQUAL);
        assert!(close(turn(&t.axes, [1.0, 0.0, 0.0]), [-1.0, 0.0, 0.0]), "{t:?}");
        assert!(close(turn(&t.axes, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]));
        assert!(close(t.apply([100.0, 1000.0, -500.0]), left), "the right hand's point went with it");
    }

    /// Both hands at one place, before or after, turn nothing and send nothing to no number.
    #[test]
    fn hands_at_one_place_turn_nothing() {
        let at = [0.0, 1.0, -0.5];
        let mut grab = WorldGrab::default();
        grab.update_with(&START, [hand(at, 0.0), hand(at, 0.0)], EQUAL);
        let t = grab.update_with(&START, [tipped(at, 1.0), hand(at, 2.0)], EQUAL);
        assert_eq!(t, START, "no line between the hands: no turn, no scale");
        let mut grab = WorldGrab::default();
        grab.update_with(&START, spread(0.1), EQUAL);
        let t = grab.update_with(&START, spread(0.0), EQUAL);
        assert_eq!(t.axes, START.axes);
        assert!((t.scale - START.scale * MIN_SPAN / 0.2).abs() < 1e-15, "{}", t.scale);
        assert!(crate::vector::finite(t.translation));
    }

    /// A hand at `at`, turned `angle` about the room's Z (its up leaning sideways).
    fn leaning(at: [f64; 3], angle: f64) -> Option<Pose> {
        let (s, c) = (angle * 0.5).sin_cos();
        Some(Pose { position: at, orientation: [0.0, 0.0, s, c] })
    }

    /// Whether two turns (as columns) are the same but for rounding.
    fn same_turn(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> bool {
        (0..3).all(|k| close(a[k], b[k]))
    }

    /// Both hands equal, brought within 5 mm of each other: a millimetre of tracking jitter
    /// either way, which swings the line between hands that close by degrees, turns nothing.
    #[test]
    fn hands_almost_together_do_not_spin_the_model() {
        let mut grab = WorldGrab::default();
        grab.update_with(&START, spread(0.1), EQUAL);
        let together = grab.update_with(&START, around(0.0025, [0.0, 1.0, 0.0]), EQUAL);
        assert!(same_turn(&together.axes, &START.axes), "{together:?}");
        let jitter = [[0.001, 0.0, 0.0], [0.0, 0.001, 0.0], [0.0, 0.0, -0.001], [-0.001, 0.001, 0.0], [0.0, -0.001, 0.001], [0.0, 0.0, 0.0]];
        for (k, d) in jitter.iter().enumerate() {
            let e = jitter[(k + 2) % jitter.len()];
            let (left, right) = (add([-0.0025, 1.0, 0.0], *d), sub([0.0025, 1.0, 0.0], e));
            let t = grab.update_with(&START, [hand(left, 0.0), hand(right, 0.0)], EQUAL);
            assert!(same_turn(&t.axes, &START.axes), "jitter {k}: {t:?}");
        }
    }

    /// Both hands equal, parting again after being closer than [`MIN_SPAN`] along another line:
    /// the model does not jump to that line's turn; the line is taken up from where the hands
    /// part, and turning it from there turns the model.
    #[test]
    fn hands_parting_again_take_up_the_line_without_a_jump() {
        let mut grab = WorldGrab::default();
        grab.update_with(&START, spread(0.1), EQUAL);
        grab.update_with(&START, around(0.0025, [0.0, 1.0, 0.0]), EQUAL);
        // Tilted steeply by a jitter while together: the line now points mostly along −Z.
        let before = grab.update_with(&START, [hand([-0.0005, 1.0, 0.002], 0.0), hand([0.0005, 1.0, -0.002], 0.0)], EQUAL);
        assert!(same_turn(&before.axes, &START.axes));
        let middle = [0.0, 1.0, 0.0];
        let held = before.to_model(middle);
        // Parted along Z about the same middle: no turn, the middle stays where it was.
        let parted = grab.update_with(&START, [hand([0.0, 1.0, 0.1], 0.0), hand([0.0, 1.0, -0.1], 0.0)], EQUAL);
        assert!(same_turn(&parted.axes, &START.axes), "no jump to the line's turn: {parted:?}");
        assert!(close(parted.apply(held), middle), "the point between the hands stays between them");
        // The line swung from −Z to X: a quarter about the vertical, from where the hands parted.
        let swung = grab.update_with(&START, [hand([-0.1, 1.0, 0.0], 0.0), hand([0.1, 1.0, 0.0], 0.0)], EQUAL);
        assert!(close(turn(&swung.axes, [1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]), "{swung:?}");
        assert!(close(turn(&swung.axes, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]));
    }

    /// Both hands equal, one hand's up lying nearly along the line between them and flipping
    /// across it (a roll that says nothing), the other rolled a quarter: the model rolls by the
    /// other's quarter, not by the average of both.
    #[test]
    fn a_hand_whose_up_lies_along_the_line_does_not_roll_the_model() {
        let (left, right) = ([-0.1, 1.0, -0.5], [0.1, 1.0, -0.5]);
        let quarter = std::f64::consts::FRAC_PI_2;
        let mut grab = WorldGrab::default();
        grab.update_with(&START, [leaning(left, quarter - 0.01), hand(right, 0.0)], EQUAL);
        let t = grab.update_with(&START, [leaning(left, quarter + 0.01), tipped(right, quarter)], EQUAL);
        let up = turn(&t.axes, [0.0, 1.0, 0.0]);
        assert!((0..3).all(|k| (up[k] - [0.0, 0.0, 1.0][k]).abs() < 1e-3), "the model's Y follows the right wrist: {up:?}");
    }

    /// Set to [`TwoHands::Lead`], the hand that took hold first holds and turns the model; the
    /// other only scales it, about the first, and its own turn does nothing. Both taking hold at
    /// once, the right leads.
    #[test]
    fn the_first_hand_leads_when_set() {
        let mode = Mode { two_hands: TwoHands::Lead, snap: false };
        let (left, right) = ([-0.1, 1.0, -0.5], [0.1, 1.0, -0.5]);
        let mut grab = WorldGrab::default();
        grab.update_with(&START, [hand(left, 0.0), None], mode);
        assert_eq!(grab.update_with(&START, [hand(left, 0.0), hand(right, 0.0)], mode), START, "the second hand joining moves nothing");
        let t = grab.update_with(&START, [hand(left, 0.0), hand([0.3, 1.0, -0.5], 1.0)], mode);
        assert!((t.scale - 2.0 * START.scale).abs() < 1e-15, "twice as far apart: twice the size");
        assert!(close(t.apply([-100.0, 1000.0, -500.0]), left), "the leading hand's point stays in it");
        assert_eq!(t.axes, START.axes, "the other hand's turn does nothing");
        let quarter = std::f64::consts::FRAC_PI_2;
        let t = grab.update_with(&START, [hand(left, quarter), hand(right, 0.0)], mode);
        assert!(close(turn(&t.axes, [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]), "the leading hand turns it");
        assert!(close(t.apply([-100.0, 1000.0, -500.0]), left), "about itself");

        let mut grab = WorldGrab::default();
        grab.update_with(&START, [hand(left, 0.0), hand(right, 0.0)], mode);
        let t = grab.update_with(&START, [hand([-0.3, 1.0, -0.5], 0.0), hand(right, 0.0)], mode);
        assert!(close(t.apply([100.0, 1000.0, -500.0]), right), "the right led: its point stays");
    }

    /// With the snap off, a two-handed scale near real size is what the hands say.
    #[test]
    fn without_the_snap_the_scale_is_what_the_hands_say() {
        let half = Transform { scale: REAL_SIZE * 0.5, ..START };
        let mode = Mode { snap: false, ..Mode::default() };
        let mut grab = WorldGrab::default();
        grab.update_with(&half, spread(0.1), mode);
        let s = grab.update_with(&half, spread(0.195), mode).scale / REAL_SIZE;
        assert!((s - 0.975).abs() < 1e-12, "{s}");
        // Starting at real size, a small move is not held there either.
        let mut grab = WorldGrab::default();
        grab.update_with(&START, spread(0.1), mode);
        let s = grab.update_with(&START, spread(0.102), mode).scale / REAL_SIZE;
        assert!((s - 1.02).abs() < 1e-12, "{s}");
    }

    /// Turning about a point keeps that point where it is and turns the rest round it, right
    /// handed; an axis of any length counts as its direction.
    #[test]
    fn turning_about_a_point_keeps_that_point() {
        let quarter = std::f64::consts::FRAC_PI_2;
        let t = Transform { translation: [1.0, 1.0, 0.0], ..START };
        let centre = [0.0, 1.0, 0.0];
        let turned = turned_about_vertical(&t, centre, quarter);
        assert!(close(turned.translation, [0.0, 1.0, -1.0]), "the origin went from +X to −Z: {turned:?}");
        assert!(close(turned.apply(t.to_model(centre)), centre), "the centre stays");
        assert!(close(turn(&turned.axes, [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
        assert_eq!((turned.scale, turned.pivot), (t.scale, t.pivot));
        let tipped = turned_about(&t, centre, [1.0, 0.0, 0.0], quarter);
        assert!(close(turn(&tipped.axes, [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]), "about X: Y to Z");
        let long = turned_about(&t, centre, [0.0, 3.0, 0.0], quarter);
        assert!(close(long.translation, turned.translation) && (0..3).all(|k| close(long.axes[k], turned.axes[k])));
        let round = turned_about(&t, centre, [0.3, 0.4, 0.5], std::f64::consts::TAU);
        assert!(close(round.translation, t.translation) && (0..3).all(|k| close(round.axes[k], t.axes[k])), "a whole turn");
    }

    /// An axis with no direction, or an angle or centre that is no number, leaves the model as
    /// it is.
    #[test]
    fn turning_about_nothing_leaves_the_model_be() {
        let t = Transform { axes: turn_about_vertical(0.3), translation: [1.0, 1.0, 0.0], ..START };
        assert_eq!(turned_about(&t, [0.0; 3], [0.0; 3], 1.0), t);
        assert_eq!(turned_about(&t, [0.0; 3], [f64::NAN, 1.0, 0.0], 1.0), t);
        assert_eq!(turned_about(&t, [0.0; 3], [0.0, 1.0, 0.0], f64::NAN), t);
        assert_eq!(turned_about(&t, [0.0; 3], [0.0, 1.0, 0.0], f64::INFINITY), t);
        assert_eq!(turned_about_vertical(&t, [f64::NAN; 3], 1.0), t);
        assert_eq!(turned_about(&t, [0.0; 3], [0.0, 1.0, 0.0], 0.0).tidy().map(|x| x.scale), Some(t.scale));
    }
}
