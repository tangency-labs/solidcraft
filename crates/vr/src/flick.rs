//! A flick of the thumbstick: pushed out in one direction and let go. Each flick counts once; the
//! stick must come back near the middle before the next one counts, so holding it out does not
//! repeat.

/// Pushed this far out (0 to 1), the stick has flicked.
pub const OUT: f32 = 0.7;

/// Back this near the middle, the stick can flick again.
pub const BACK: f32 = 0.3;

/// Which way a flick went, as the user holds the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// One stick's flicks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flick {
    /// The stick flicked and has not come back near the middle since.
    out: bool,
}

impl Flick {
    /// The stick at `stick` (x right, y up, each −1 to 1): the direction it flicked in, once,
    /// when it first goes past [`OUT`]. A reading that is not a number counts as the middle.
    pub fn next(&mut self, stick: [f32; 2]) -> Option<Direction> {
        let [x, y] = stick.map(|v| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 });
        let reach = (x * x + y * y).sqrt();
        if reach <= BACK {
            self.out = false;
            return None;
        }
        if self.out || reach < OUT {
            return None;
        }
        self.out = true;
        Some(if x.abs() > y.abs() {
            if x > 0.0 { Direction::Right } else { Direction::Left }
        } else if y > 0.0 {
            Direction::Up
        } else {
            Direction::Down
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `flick` says for each of `sticks` in turn.
    fn run(flick: &mut Flick, sticks: &[[f32; 2]]) -> Vec<Option<Direction>> {
        sticks.iter().map(|s| flick.next(*s)).collect()
    }

    /// A stick pushed out counts once, held out or wobbling short of the middle it does not
    /// count again, and back near the middle it can flick again.
    #[test]
    fn a_flick_fires_once_until_the_stick_comes_back() {
        let mut flick = Flick::default();
        let seen = run(&mut flick, &[[0.0, 0.0], [0.5, 0.0], [0.8, 0.0], [0.95, 0.1], [0.5, 0.0], [0.9, 0.0], [0.2, 0.0], [0.8, 0.0]]);
        let right = Some(Direction::Right);
        assert_eq!(seen, [None, None, right, None, None, None, None, right], "0.5 is short of OUT and not back past BACK");
    }

    /// The direction is the axis the stick went furthest along; a stick pushed exactly between
    /// two counts as up or down.
    #[test]
    fn a_flick_goes_the_way_the_stick_went_furthest() {
        for (stick, want) in [
            ([0.0, 0.9], Direction::Up),
            ([0.0, -0.9], Direction::Down),
            ([-0.9, 0.0], Direction::Left),
            ([0.9, 0.0], Direction::Right),
            ([0.6, -0.5], Direction::Right),
            ([-0.5, 0.6], Direction::Up),
            ([0.6, 0.6], Direction::Up),
            ([-0.6, -0.6], Direction::Down),
        ] {
            assert_eq!(Flick::default().next(stick), Some(want), "{stick:?}");
        }
    }

    /// Readings past the ends count as the ends; a reading that is not a number counts as the
    /// middle, so it re-arms the stick and never flicks it.
    #[test]
    fn a_stick_reading_that_is_no_number_counts_as_the_middle() {
        let mut flick = Flick::default();
        assert_eq!(flick.next([5.0, 0.0]), Some(Direction::Right), "clamped to 1");
        assert_eq!(flick.next([f32::NAN, f32::NAN]), None);
        assert_eq!(flick.next([0.0, -3.0]), Some(Direction::Down), "the NaN re-armed it");
        assert_eq!(flick.next([f32::INFINITY, 0.0]), None, "infinity is not a number to count");
        assert_eq!(flick.next([f32::NEG_INFINITY, f32::NAN]), None);
        assert_eq!(flick.next([0.0, 0.9]), Some(Direction::Up));
        // Far out on both axes past their ends: clamped to the corner, which counts as vertical.
        assert_eq!(Flick::default().next([-1e9, 1e9]), Some(Direction::Up));
    }

    /// A stick going straight from one side to the other without passing the middle counts only
    /// the first flick: it has not come back.
    #[test]
    fn a_stick_swept_round_the_edge_counts_once() {
        let mut flick = Flick::default();
        let edge: Vec<[f32; 2]> = (0..=16).map(|k| f32::from(k as u8) * std::f32::consts::PI / 16.0).map(|a| [a.cos(), a.sin()]).collect();
        let seen = run(&mut flick, &edge);
        assert_eq!(seen.iter().flatten().count(), 1);
        assert_eq!(seen[0], Some(Direction::Right));
    }
}
