//! A sketch's solve state on its rows (#7245): the History strip, the left History and the
//! browser's Sketches folder draw the sketch icon in the colour the sketch's own curves take in
//! that state (under constrained: `sketch`; fully constrained: `sketch_fixed`; constraints
//! conflict: `error`), and the row's tooltip names the state. An empty sketch keeps its icon as
//! it is.
//!
//! The states come from the solves the model already holds ([`SolvedSketch::report`]): the
//! History reads them in one pass per frame, the browser when it re-indexes an evaluated model.
//! Drawing a row never solves anything. A sketch the model holds no solve for (rolled back behind
//! the history marker) keeps its icon too. The Sketch preference `sketch_state_icons` (on by
//! default) turns it all off: then every sketch row looks as before.
//!
//! [`SolvedSketch::report`]: solidcraft_engine::doc::SolvedSketch

use std::collections::HashMap;

use egui::Color32;

use crate::SolveApp;
use crate::theme::Tokens;

/// A sketch's state as its last solve reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SketchState {
    /// Nothing drawn in it yet.
    Empty,
    /// Degrees of freedom remain.
    Under(usize),
    Full,
    /// The constraints conflict (the solve failed).
    Conflict,
}

impl SketchState {
    pub(crate) fn of(ss: &solidcraft_engine::doc::SolvedSketch) -> SketchState {
        if !ss.report.ok() {
            SketchState::Conflict
        } else if ss.sketch.curves.is_empty() && ss.sketch.wires.is_empty() {
            SketchState::Empty
        } else if ss.report.dof == 0 {
            SketchState::Full
        } else {
            SketchState::Under(ss.report.dof)
        }
    }

    /// The icon's colour: the one the sketch's curves take in this state. The conflict colour is
    /// the error red (the `sketch_conflict` token of #7244, once it lands, is the one to use).
    /// None for an empty sketch: its icon is drawn as usual.
    pub(crate) fn ink(self, t: &Tokens) -> Option<Color32> {
        match self {
            SketchState::Empty => None,
            SketchState::Under(_) => Some(t.sketch),
            SketchState::Full => Some(t.sketch_fixed),
            SketchState::Conflict => Some(t.error),
        }
    }

    /// The state in words, for the row's tooltip.
    pub(crate) fn text(self) -> String {
        match self {
            SketchState::Empty => crate::i18n::tr("ui.no_geometry_yet").to_string(),
            SketchState::Under(dof) => crate::i18n::format(
                if dof == 1 { "history.sketch_under_constrained_one" } else { "history.sketch_under_constrained" },
                &[("count", &dof.to_string())],
            ),
            SketchState::Full => crate::i18n::tr("ui.fully_constrained").to_string(),
            SketchState::Conflict => crate::i18n::tr("ui.constraints_conflict").to_string(),
        }
    }
}

/// Every solved sketch's state, by its feature id: one pass over the model's solved sketches.
#[derive(Default)]
pub(crate) struct SketchStates(HashMap<u64, SketchState>);

impl SketchStates {
    /// Nothing while the preference is off.
    pub(crate) fn of(app: &SolveApp) -> SketchStates {
        if !app.preferences.sketch_state_icons {
            return SketchStates::default();
        }
        let st = app.session.model.state();
        SketchStates(st.sketches.iter().map(|ss| (ss.feature, SketchState::of(ss))).collect())
    }

    pub(crate) fn get(&self, sketch: u64) -> Option<SketchState> {
        self.0.get(&sketch).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_ink_their_icons_in_the_sketch_colours_and_name_themselves() {
        for t in [Tokens::light(), Tokens::dark()] {
            assert_eq!(SketchState::Under(3).ink(&t), Some(t.sketch));
            assert_eq!(SketchState::Full.ink(&t), Some(t.sketch_fixed));
            assert_eq!(SketchState::Conflict.ink(&t), Some(t.error));
            assert_eq!(SketchState::Empty.ink(&t), None);
        }
        assert_eq!(SketchState::Under(1).text(), "Under constrained (1 degree of freedom)");
        assert_eq!(SketchState::Under(4).text(), "Under constrained (4 degrees of freedom)");
        assert_eq!(SketchState::Full.text(), "Fully constrained");
        assert_eq!(SketchState::Conflict.text(), "Constraints conflict");
        assert_eq!(SketchState::Empty.text(), "No geometry yet");
    }
}
