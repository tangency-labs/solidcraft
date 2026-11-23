//! Where the ring menu's items stand ([`Ring`]): a ring of them, upright and facing the head, or a
//! fan (part of a ring) pointing one way; where each item is, and which item is nearest the hand.
//!
//! The menu itself is the app's (`apps/solidcraft/src/vr/menu.rs`): held open with one button,
//! it stands a [`Ring::facing`] the head around where the hand was, marks the item whose bubble
//! is nearest the hand ([`Ring::nearest`]) and takes it when the button is let go. A group opens
//! its items as a fan around its own bubble, pointing on outward ([`Ring::fan`], as wide as the
//! app chooses); moving back out of the fan closes it ([`Ring::distance`]). No stick, no second
//! button: it works the same with a one-button controller and with a pinch of a tracked hand. The
//! app shows at most [`MAX_ITEMS`] items a ring.

use crate::vector::{add, cross, dot, length, scale, sub, unit};

/// How far from the middle the items stand (metres), unless asked otherwise ([`Ring::radius`]).
pub const RADIUS: f64 = 0.09;

/// Within this share of the radius from the middle, nothing is marked: the hand has not chosen yet.
pub const DEAD: f64 = 0.39;

/// The most items a ring holds: past it the bubbles get too crowded to hit by hand.
pub const MAX_ITEMS: usize = 8;

/// Where a ring stands: its middle and its right and up directions, all in the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub centre: [f64; 3],
    pub right: [f64; 3],
    pub up: [f64; 3],
    /// How far from the middle the items stand (metres).
    pub radius: f64,
    /// A fan, not a whole ring: the direction its middle points (radians, clockwise from up, as
    /// the items are counted) and how wide it spreads.
    pub fan: Option<(f64, f64)>,
}

impl Ring {
    /// A ring of `radius` (metres) around `centre`, upright and facing `head`; `None` when the
    /// head is at the centre or a number is not one.
    pub fn facing(centre: [f64; 3], head: [f64; 3], radius: f64) -> Option<Ring> {
        let back = unit(sub(head, centre))?;
        let right = unit(cross([0.0, 1.0, 0.0], back)).or_else(|| unit(cross(back, [0.0, 0.0, 1.0])))?;
        let up = cross(back, right);
        let radius = if radius.is_finite() && radius > 0.0 { radius } else { RADIUS };
        Some(Ring { centre, right, up, radius, fan: None })
    }

    /// The angle item `i` of `count` stands at (radians, clockwise from up as the head sees it).
    pub fn angle(&self, i: usize, count: usize) -> f64 {
        match self.fan {
            None => std::f64::consts::TAU * i as f64 / count.max(1) as f64,
            Some((direction, _)) if count < 2 => direction,
            Some((direction, spread)) => direction - spread * 0.5 + spread * i as f64 / (count - 1) as f64,
        }
    }

    /// Within this of the middle (metres), nothing is marked.
    pub fn dead(&self) -> f64 {
        self.radius * DEAD
    }

    /// Where item `i` of `count` stands: the first at the top, the rest clockwise as the head
    /// sees them.
    pub fn item(&self, i: usize, count: usize) -> [f64; 3] {
        let (s, c) = self.angle(i, count).sin_cos();
        add(self.centre, add(scale(self.right, s * self.radius), scale(self.up, c * self.radius)))
    }

    /// How far `hand` is from the middle within the ring's plane (metres), and the item of
    /// `count` nearest to it with how far the hand is from that item (metres, in the plane);
    /// none within [`Ring::dead`]. Nearest by distance, not by angle: what the hand is closest
    /// to is what it takes, however it drifts.
    pub fn nearest(&self, hand: [f64; 3], count: usize) -> (f64, Option<(usize, f64)>) {
        let d = sub(hand, self.centre);
        let (x, y) = (dot(d, self.right), dot(d, self.up));
        let reach = (x * x + y * y).sqrt();
        if !reach.is_finite() || reach < self.dead() || count == 0 {
            return (reach, None);
        }
        let off = |i: usize| {
            let (s, c) = self.angle(i, count).sin_cos();
            ((x - s * self.radius).powi(2) + (y - c * self.radius).powi(2)).sqrt()
        };
        let best = (0..count).map(|i| (i, off(i))).min_by(|a, b| a.1.total_cmp(&b.1));
        (reach, best)
    }

    /// How far `hand` is from the middle, in the room (metres).
    pub fn distance(&self, hand: [f64; 3]) -> f64 {
        length(sub(hand, self.centre))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    /// Whether two points are the same but for rounding.
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    /// The centre of [`ring`].
    const CENTRE: [f64; 3] = [0.0, 1.0, -0.5];

    /// A ring at [`CENTRE`] facing a head half a metre behind it (+Z): its right is the room's
    /// X and its up the room's Y.
    fn ring() -> Ring {
        Ring::facing(CENTRE, [0.0, 1.0, 0.0], RADIUS).unwrap()
    }

    /// A hand `x` right and `y` up of the ring's middle, `out` toward the head.
    fn hand(x: f64, y: f64, out: f64) -> [f64; 3] {
        [CENTRE[0] + x, CENTRE[1] + y, CENTRE[2] + out]
    }

    /// The ring stands upright facing the head; its first item is at the top and the rest go
    /// clockwise as the head sees them, [`RADIUS`] out.
    #[test]
    fn the_ring_faces_the_head_with_its_items_clockwise_from_the_top() {
        let r = ring();
        assert!(close(r.right, [1.0, 0.0, 0.0]) && close(r.up, [0.0, 1.0, 0.0]), "{r:?}");
        assert_eq!((r.radius, r.fan), (RADIUS, None));
        assert!(close(r.item(0, 4), hand(0.0, RADIUS, 0.0)), "top");
        assert!(close(r.item(1, 4), hand(RADIUS, 0.0, 0.0)), "right");
        assert!(close(r.item(2, 4), hand(0.0, -RADIUS, 0.0)), "bottom");
        assert!(close(r.item(3, 4), hand(-RADIUS, 0.0, 0.0)), "left");
        assert!((r.angle(3, 4) - 3.0 * FRAC_PI_2).abs() < 1e-12);
        assert!((r.dead() - 0.09 * 0.39).abs() < 1e-15);
        // Seen from straight above, it still stands somewhere, its numbers all numbers.
        let above = Ring::facing(CENTRE, [0.0, 3.0, -0.5], 0.1).unwrap();
        assert!(close(above.right, [1.0, 0.0, 0.0]) && close(above.up, [0.0, 0.0, -1.0]), "{above:?}");
    }

    /// The item nearest the hand by distance in the ring's plane is the one it takes, with how
    /// far it is from it; within the dead middle none is, and how far toward the head the hand
    /// is does not count.
    #[test]
    fn the_nearest_item_is_taken_outside_the_dead_middle() {
        let r = ring();
        let (reach, nearest) = r.nearest(hand(0.06, 0.02, 0.0), 4);
        assert!((reach - 0.06_f64.hypot(0.02)).abs() < 1e-12, "{reach}");
        let (i, off) = nearest.unwrap();
        // From (0.06, 0.02) to the right item at (0.09, 0): √(0.03² + 0.02²).
        assert_eq!(i, 1);
        assert!((off - 0.0013_f64.sqrt()).abs() < 1e-12, "{off}");
        assert_eq!(r.nearest(hand(0.0, -0.05, 0.4), 4).1.map(|n| n.0), Some(2), "a hand far toward the head still takes the bottom");
        let (reach, nearest) = r.nearest(hand(0.02, 0.02, 0.0), 4);
        assert!(reach < r.dead() && nearest.is_none(), "inside the dead middle");
        // Just past the dead middle, toward the left: the left item.
        assert_eq!(r.nearest(hand(-r.dead() - 1e-6, 0.0, 0.0), 4).1.map(|n| n.0), Some(3));
        // Eight items: the hand straight down-right of the middle takes the fourth (item 3).
        let d = 0.07 * std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(r.nearest(hand(d, -d, 0.0), 8).1.map(|n| n.0), Some(3));
    }

    /// No items, or a hand at no place, take nothing; many items still give one of them.
    #[test]
    fn no_items_or_no_hand_take_nothing() {
        let r = ring();
        assert_eq!(r.nearest(hand(0.0, 0.08, 0.0), 0).1, None);
        let (reach, nearest) = r.nearest([f64::NAN, 1.0, 0.0], 4);
        assert!(reach.is_nan() && nearest.is_none());
        assert_eq!(r.nearest([f64::INFINITY, 1.0, 0.0], 4).1, None);
        let (_, many) = r.nearest(hand(0.0, 0.09, 0.0), 1000);
        assert_eq!(many.map(|n| n.0), Some(0), "the top one, however many");
        assert!(r.angle(0, 0).is_finite() && r.item(0, 0).iter().all(|v| v.is_finite()), "no division by no items");
    }

    /// A ring facing a head at its own middle, or with numbers that are none, is refused; a
    /// radius that is no radius falls back to [`RADIUS`].
    #[test]
    fn a_ring_from_numbers_that_are_not_any_is_refused() {
        assert_eq!(Ring::facing(CENTRE, CENTRE, RADIUS), None);
        assert_eq!(Ring::facing([f64::NAN; 3], [0.0; 3], RADIUS), None);
        assert_eq!(Ring::facing(CENTRE, [0.0, f64::INFINITY, 0.0], RADIUS), None);
        for radius in [0.0, -0.1, f64::NAN, f64::INFINITY] {
            assert_eq!(Ring::facing(CENTRE, [0.0, 1.0, 0.0], radius).unwrap().radius, RADIUS, "{radius}");
        }
        assert_eq!(Ring::facing(CENTRE, [0.0, 1.0, 0.0], 0.2).unwrap().radius, 0.2);
    }

    /// A fan spreads its items evenly across its width about the direction it points, in the
    /// ring's plane around its own centre; a fan of one item, or none, stands straight along its
    /// direction; and the nearest of its items is the one taken.
    #[test]
    fn a_fan_spreads_its_items_about_its_direction() {
        let r = ring();
        let centre = hand(RADIUS, 0.0, 0.0);
        let fan = Ring { centre, fan: Some((FRAC_PI_2, 1.8)), ..r };
        assert!((fan.angle(0, 3) - (FRAC_PI_2 - 0.9)).abs() < 1e-12);
        assert!((fan.angle(1, 3) - FRAC_PI_2).abs() < 1e-12);
        assert!((fan.angle(2, 3) - (FRAC_PI_2 + 0.9)).abs() < 1e-12);
        assert!(close(fan.item(1, 3), [2.0 * RADIUS, 1.0, -0.5]), "the middle one straight out to the right");
        assert_eq!(fan.nearest(fan.item(2, 3), 3).1.map(|n| n.0), Some(2));
        let one = Ring { fan: Some((1.0, 0.0)), ..fan };
        assert_eq!(one.angle(0, 1), 1.0);
        assert_eq!(one.angle(0, 0), 1.0);
        assert!(one.item(0, 0).iter().all(|v| v.is_finite()));
        // A whole ring wraps round: the last item stands just left of the top.
        assert!((r.angle(7, 8) - 7.0 * TAU / 8.0).abs() < 1e-12);
        assert!((r.angle(2, 4) - PI).abs() < 1e-12);
    }

    /// The hand's distance from the middle counts in the room, out of the ring's plane too.
    #[test]
    fn the_distance_counts_toward_the_head_too() {
        let r = ring();
        assert!((r.distance(hand(0.04, 0.0, 0.03)) - 0.05).abs() < 1e-12);
        assert_eq!(r.distance(CENTRE), 0.0);
        assert!(r.distance([f64::NAN; 3]).is_nan());
    }
}
