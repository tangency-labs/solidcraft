//! A button that does one thing when clicked and another when held: a short press counts when it
//! is let go, a long one as soon as it has been held [`LONG`].

/// Held this long (seconds), a press is a long one: long enough that a click never is, short
/// enough that holding for the menu does not feel like waiting.
pub const LONG: f64 = 0.35;

/// What a press was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Let go before [`LONG`].
    Short,
    /// Held [`LONG`]; nothing more counts until it is let go.
    Long,
}

/// One button's presses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Press {
    /// When the button went down, while it is down.
    since: Option<f64>,
    /// This press has counted as a long one already.
    long: bool,
}

impl Press {
    /// The button `down` or not at `now` (seconds, from any start): a short press when it is let
    /// go before [`LONG`], a long one once when it has been held that long. A time that is not a
    /// number, or that goes back, counts as the press's start.
    pub fn next(&mut self, down: bool, now: f64) -> Option<Kind> {
        self.next_after(down, now, LONG)
    }

    /// [`Press::next`], a press held `long` seconds (not [`LONG`]) being a long one.
    pub fn next_after(&mut self, down: bool, now: f64, long: f64) -> Option<Kind> {
        let long_after = if long.is_finite() && long > 0.0 { long } else { LONG };
        match (down, self.since) {
            (true, None) => {
                self.since = Some(now);
                self.long = false;
                None
            }
            (true, Some(since)) => {
                let held = now - since;
                if !self.long && held.is_finite() && held >= long_after {
                    self.long = true;
                    return Some(Kind::Long);
                }
                if !held.is_finite() || held < 0.0 {
                    self.since = Some(now);
                }
                None
            }
            (false, Some(_)) => {
                self.since = None;
                (!std::mem::take(&mut self.long)).then_some(Kind::Short)
            }
            (false, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `press` says for each `(down, now)` in turn, a press held `long` seconds a long one.
    fn run(press: &mut Press, long: f64, frames: &[(bool, f64)]) -> Vec<Option<Kind>> {
        frames.iter().map(|&(down, now)| press.next_after(down, now, long)).collect()
    }

    /// Let go before [`LONG`], a press is a short one, counted when it is let go.
    #[test]
    fn a_click_counts_as_short_when_it_is_let_go() {
        let mut press = Press::default();
        let seen: Vec<_> = [(true, 10.0), (true, 10.1), (true, 10.3), (false, 10.34)].map(|(d, t)| press.next(d, t)).to_vec();
        assert_eq!(seen, [None, None, None, Some(Kind::Short)]);
    }

    /// Held [`LONG`], a press counts as long once, on that frame; letting go then counts nothing,
    /// and the next press starts afresh.
    #[test]
    fn a_long_press_counts_once_and_its_release_counts_nothing() {
        let mut press = Press::default();
        let seen = run(&mut press, LONG, &[(true, 0.0), (true, 0.2), (true, 0.35), (true, 0.5), (true, 9.0), (false, 9.1), (false, 9.2)]);
        assert_eq!(seen, [None, None, Some(Kind::Long), None, None, None, None]);
        let again = run(&mut press, LONG, &[(true, 10.0), (false, 10.1)]);
        assert_eq!(again, [None, Some(Kind::Short)], "the long one does not carry over");
    }

    /// The hold time can be set; one that is no time (zero, negative, not a number, endless)
    /// falls back to [`LONG`].
    #[test]
    fn the_hold_time_is_set_or_falls_back_to_long() {
        let mut press = Press::default();
        let seen = run(&mut press, 1.0, &[(true, 0.0), (true, 0.5), (true, 0.99), (true, 1.0)]);
        assert_eq!(seen, [None, None, None, Some(Kind::Long)]);
        for long in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut press = Press::default();
            let seen = run(&mut press, long, &[(true, 0.0), (true, 0.3), (true, 0.36)]);
            assert_eq!(seen, [None, None, Some(Kind::Long)], "{long}");
        }
    }

    /// A clock that reads no number, or goes back, restarts the press instead of making it long
    /// or never letting it be: the press counts from the next good reading.
    #[test]
    fn a_clock_that_reads_nothing_or_goes_back_restarts_the_press() {
        let mut press = Press::default();
        let seen = run(&mut press, LONG, &[(true, 0.0), (true, f64::NAN), (true, 0.1), (true, 0.4), (true, 0.45)]);
        assert_eq!(seen, [None, None, None, None, Some(Kind::Long)], "long 0.35 s after 0.1, not after 0");
        let mut press = Press::default();
        let seen = run(&mut press, LONG, &[(true, 1.0), (true, 0.5), (true, 0.8), (true, 0.86)]);
        assert_eq!(seen, [None, None, None, Some(Kind::Long)], "long 0.35 s after 0.5, where the clock went back to");
        let mut press = Press::default();
        let seen = run(&mut press, LONG, &[(true, f64::INFINITY), (true, 0.0), (false, 0.1)]);
        assert_eq!(seen, [None, None, Some(Kind::Short)]);
    }

    /// A button never pressed counts nothing, however long it stays up.
    #[test]
    fn a_button_left_up_counts_nothing() {
        let mut press = Press::default();
        assert!(run(&mut press, LONG, &[(false, 0.0), (false, 100.0), (false, f64::NAN)]).iter().all(Option::is_none));
        assert_eq!(press, Press::default());
    }
}
