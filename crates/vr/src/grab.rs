//! Grabbing the model.
//!
//! - One hand's grip held: the model is held like a thing in the hand. It moves and turns with
//!   the hand, about the hand.
//! - Both grips held: the model grows or shrinks as the hands move apart or together, about the
//!   point between them, and moves with that point. It does not turn: turning is one hand's.
//!   Brought near its real size, it snaps to real size and stays there until the hands move it
//!   clearly away again.
//!
//! The room and its floor never move; only the model does. Whenever which hands hold changes,
//! the grab starts again from where the model is, so the model never jumps when a second hand
//! joins or one lets go.

use solidcraft_xr::view::Pose;

use crate::room::{MAX_SCALE, MIN_SCALE, REAL_SIZE, Transform};
use crate::vector::{add, length, scale, sub, turn};

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
    /// Both hands: the point between them, how far apart they were, and the real-size snap.
    Two {
        middle: [f64; 3],
        distance: f64,
        start: Transform,
        snap: Snap,
    },
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
            ([Some((left, _)), Some((right, _))], Held::Two { middle, distance, start, snap }) => {
                let (snap, moved) = two_handed(&start, snap, middle, distance, scale(add(left, right), 0.5), length(sub(right, left)));
                self.held = Held::Two { middle, distance, start, snap };
                Some(moved)
            }
            ([Some((left, _)), Some((right, _))], _) => {
                let (middle, distance) = (scale(add(left, right), 0.5), length(sub(right, left)));
                self.held = Held::Two { middle, distance, start: *current, snap: Snap::starting(current.scale) };
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

/// `start` scaled as the hands went from `distance` apart about `from` to `now_distance` apart
/// about `now`, and moved with the point between them; `snap` is the grab's real-size snap.
fn two_handed(start: &Transform, snap: Snap, from: [f64; 3], distance: f64, now: [f64; 3], now_distance: f64) -> (Snap, Transform) {
    let grow = if distance > MIN_SPAN && now_distance.is_finite() { now_distance.max(MIN_SPAN) / distance } else { 1.0 };
    // Within the scale's limits: past them the hands no longer scale, and the model does not
    // creep away from between them.
    let (snap, s) = snap.next((start.scale * grow).clamp(MIN_SCALE, MAX_SCALE));
    let grow = s / start.scale;
    // A room point p goes to now + grow · (p − from).
    (snap, Transform { scale: s, translation: add(now, scale(sub(start.translation, from), grow)), ..*start })
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
}
