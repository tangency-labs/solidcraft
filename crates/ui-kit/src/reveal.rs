//! Timing and keyboard state for chrome that temporarily overlays a workspace.

pub const HIDE_DELAY: f64 = 0.600;
pub const SLIDE_DURATION: f64 = 0.120;

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub hovered: bool,
    pub focused: bool,
    pub held: bool,
    pub keyboard_show: bool,
    pub escape: bool,
    pub dismiss_on_escape: bool,
    pub reduced_motion: bool,
    pub hide_delay: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    shown: bool,
    keyboard: bool,
    dismissed: bool,
    hide_at: Option<f64>,
    started: f64,
    from: f32,
}

impl State {
    /// The visible fraction. Animations reverse from the current painted position.
    pub fn fraction(self, now: f64) -> f32 {
        let progress = ((now - self.started) / SLIDE_DURATION).clamp(0.0, 1.0) as f32;
        let target = if self.shown { 1.0 } else { 0.0 };
        self.from + (target - self.from) * (1.0 - (1.0 - progress).powi(3))
    }

    pub fn keyboard(self) -> bool {
        self.keyboard
    }

    /// End the keyboard hold when focus leaves the revealed controls.
    pub fn release_keyboard(&mut self) {
        self.keyboard = false;
    }

    /// Repaint each animation frame, then once when the leave delay expires.
    pub fn repaint_after(self, now: f64) -> Option<f64> {
        let target = if self.shown { 1.0 } else { 0.0 };
        if self.from != target && self.started + SLIDE_DURATION > now { Some(0.0) } else { self.hide_at.map(|until| (until - now).max(0.0)) }
    }
}

pub fn reveal_step(mut state: State, input: Input, now: f64) -> State {
    if !now.is_finite() {
        return state;
    }
    let triggered = input.hovered || input.focused || input.held;
    if input.keyboard_show {
        state.keyboard = true;
        state.dismissed = false;
    } else if input.escape {
        state.keyboard = false;
        if input.dismiss_on_escape {
            state.dismissed = triggered;
            state.hide_at = None;
            state.shown = false;
            state.from = 0.0;
            return state;
        }
    } else if !triggered {
        state.dismissed = false;
    }
    let shown = if state.dismissed {
        false
    } else if triggered || state.keyboard {
        state.hide_at = None;
        true
    } else if state.shown {
        let until = *state.hide_at.get_or_insert(now + input.hide_delay.unwrap_or(HIDE_DELAY));
        if now >= until {
            state.hide_at = None;
            false
        } else {
            true
        }
    } else {
        false
    };
    if shown != state.shown {
        state.from = state.fraction(now);
        state.started = now;
        state.shown = shown;
    }
    if input.reduced_motion {
        state.from = if shown { 1.0 } else { 0.0 };
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_can_finish_hiding_before_the_top_bar_waits_to_hide() {
        let instant = Input { reduced_motion: true, ..Input::default() };
        let shown = reveal_step(State::default(), Input { hovered: true, ..instant }, 0.0);
        let ribbon = Input { hide_delay: Some(0.12), ..instant };
        let hiding_ribbon = reveal_step(shown, ribbon, 1.0);
        let hiding_bar = reveal_step(shown, instant, 1.0);
        assert_eq!(reveal_step(hiding_ribbon, ribbon, 1.2).fraction(1.2), 0.0);
        assert_eq!(reveal_step(hiding_bar, instant, 1.2).fraction(1.2), 1.0);
    }

    #[test]
    fn hover_and_focus_each_reveal_and_both_must_leave_before_the_delay() {
        let s = reveal_step(State::default(), Input { hovered: true, ..Input::default() }, 0.0);
        assert_eq!(s.fraction(0.0), 0.0);
        assert!((s.fraction(0.06) - 0.875).abs() < 0.001);
        assert_eq!(s.fraction(0.12), 1.0);
        let s = reveal_step(s, Input { focused: true, ..Input::default() }, 1.0);
        assert_eq!(s.repaint_after(1.0), None);
        let s = reveal_step(s, Input::default(), 2.0);
        assert!((s.repaint_after(2.0).unwrap() - 0.6).abs() < 0.001);
        let s = reveal_step(s, Input::default(), 2.599);
        assert_eq!(s.fraction(2.599), 1.0);
        let s = reveal_step(s, Input::default(), 2.6);
        assert_eq!(s.fraction(2.73), 0.0);
    }

    #[test]
    fn hold_restarts_delay_and_reentry_reverses_without_a_jump() {
        let s = reveal_step(State::default(), Input { hovered: true, ..Input::default() }, 0.0);
        let s = reveal_step(s, Input::default(), 1.0);
        let s = reveal_step(s, Input { held: true, ..Input::default() }, 1.5);
        assert_eq!(s.repaint_after(1.5), None);
        let s = reveal_step(s, Input::default(), 2.0);
        let s = reveal_step(s, Input::default(), 2.6);
        let part_way = s.fraction(2.63);
        assert!(0.0 < part_way && part_way < 1.0);
        let s = reveal_step(s, Input { hovered: true, ..Input::default() }, 2.63);
        assert_eq!(s.fraction(2.63), part_way);
        assert_eq!(s.fraction(2.76), 1.0);
    }

    #[test]
    fn keyboard_hold_can_end_with_focus_and_respects_the_leave_delay() {
        let mut state = reveal_step(State::default(), Input { keyboard_show: true, reduced_motion: true, ..Input::default() }, 1.0);
        state.release_keyboard();
        let released = reveal_step(state, Input::default(), 2.0);
        assert_eq!(released.fraction(2.0), 1.0);
        assert_eq!(reveal_step(released, Input::default(), 3.0).fraction(3.12), 0.0);
    }

    #[test]
    fn keyboard_reveal_and_escape_respect_chrome_kind_and_reduced_motion() {
        let instant = Input { reduced_motion: true, ..Input::default() };
        let s = reveal_step(State::default(), Input { keyboard_show: true, ..instant }, 1.0);
        assert!(s.keyboard());
        assert_eq!(s.fraction(1.0), 1.0);
        let s = reveal_step(s, Input { focused: true, escape: true, ..instant }, 2.0);
        assert!(!s.keyboard());
        assert_eq!(s.fraction(2.0), 1.0, "the window still has focus");
        let s = reveal_step(s, Input { hovered: true, escape: true, dismiss_on_escape: true, ..instant }, 3.0);
        assert_eq!(s.fraction(3.0), 0.0);
        let s = reveal_step(s, Input { hovered: true, ..instant }, 4.0);
        assert_eq!(s.fraction(4.0), 0.0, "leave the edge before revealing again");
        let s = reveal_step(s, Input { keyboard_show: true, ..instant }, 5.0);
        assert_eq!(s.fraction(5.0), 1.0);
    }
}
