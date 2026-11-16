//! Plane, Axis and Point (`construct.plane`, `construct.axis`, `construct.point`) have no
//! dialog of their own. With references picked, the window is the dialog of the first method of
//! the tool's family (`construct_methods`, in the engine's order) that takes every pick in its
//! inputs and builds from them, with that method's rows, preview and canvas handles. A pick goes
//! to the tool, which chooses the method again. A method added to a family is chosen as well,
//! with its own dialog, and nothing here changes.

use super::{
    AXES, AXIS_PICKS, Accept, CURVES, Dialog, EDGES, FACES, Kind, PLANES, SelInput, apply_commands, fits, planar_face, selection, takes_as_axis,
};
use crate::SolveApp;
use crate::viewport::Hit;
use solidcraft_engine::{Sel, Session};

/// Whatever a method builds from.
const PICKS: Accept = PLANES | FACES | EDGES | AXES | CURVES | selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS;
/// The most references a method takes.
const MAX: usize = 3;
const REFERENCES: &str = "ui.construct_auto.refs";

/// The tool a dialog stands in for, and what was picked for it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Auto {
    tool: &'static str,
    /// Every reference picked, in pick order.
    picks: Vec<Sel>,
    /// What the dialog's inputs held when it was made: a change there (a pick taken out in its
    /// row, a box selection) is a change of picks.
    placed: Vec<Sel>,
    /// Picked since the method was chosen.
    dirty: bool,
}

impl Auto {
    /// A click: a new reference is added (the oldest goes past [`MAX`]); one picked already is
    /// taken out.
    pub(super) fn toggle(&mut self, sel: Sel) {
        if let Some(i) = self.picks.iter().position(|p| selection::same_item(p, &sel)) {
            self.picks.remove(i);
        } else {
            self.picks.push(sel);
            if self.picks.len() > MAX {
                self.picks.remove(0);
            }
        }
        self.dirty = true;
    }
}

/// The line over the form: that the method was chosen from the picks, or what to pick.
pub(super) fn hint(a: &Auto) -> String {
    solidcraft_ui_text::tr(if a.picks.is_empty() { "ui.construct_auto.pick" } else { "ui.construct_auto.chosen" }).to_owned()
}

/// Why OK does nothing while no method takes the picks.
pub(super) fn refusal(d: &Dialog) -> String {
    let none = d.auto.as_ref().is_none_or(|a| a.picks.is_empty());
    solidcraft_ui_text::tr(if none { "ui.construct_auto.select" } else { "ui.construct_auto.no_fit" }).to_owned()
}

/// What a hit picks for the tool.
pub(super) fn candidate(s: &Session, h: &Hit) -> Option<Sel> {
    SelInput::new(REFERENCES, PICKS, true).accepts(h, s.active_sketch, |body, face| planar_face(s, body, face).is_some())
}

/// The tool's window, with the selection as its first picks.
pub(super) fn start(app: &SolveApp, tool: &str) -> Option<Dialog> {
    let tool = solidcraft_engine::find_command(tool)?.id;
    let picks: Vec<Sel> = app.session.selection.iter().filter(|sel| fits(PICKS, sel)).take(MAX).cloned().collect();
    Some(choose(app, tool, picks, None))
}

/// Choose again once the picks changed: a click, or the inputs edited in the form.
pub(super) fn sync(app: &SolveApp, d: &mut Dialog) {
    let Some(a) = &d.auto else { return };
    let items = d.items();
    if !a.dirty && items == a.placed {
        return;
    }
    let mut picks = a.picks.clone();
    if !a.dirty {
        picks.retain(|p| items.iter().any(|x| selection::same_item(x, p)));
        for x in items {
            if !picks.iter().any(|p| selection::same_item(p, &x)) {
                picks.push(x);
            }
        }
    }
    let tool = a.tool;
    let was = std::mem::replace(d, Dialog::new(Kind::ConstructAuto { command: tool }, Vec::new()));
    *d = choose(app, tool, picks, Some(&was));
}

/// Whether `inp` can hold `sel` too, as a pre-selection fills it.
fn takes(s: &Session, inp: &SelInput, sel: &Sel) -> bool {
    let planar_ok = match sel {
        Sel::Face { body, index, .. } => inp.accept & FACES != 0 || planar_face(s, body, *index).is_some(),
        _ => true,
    };
    (inp.multi || inp.items.is_empty()) && fits(inp.accept, sel) && planar_ok && (inp.accept != AXIS_PICKS || takes_as_axis(s, sel))
}

/// Put every one of `picks` in an input of `d` that takes it, the first way `good` accepts.
fn place(s: &Session, d: &mut Dialog, picks: &[Sel], good: &dyn Fn(&Dialog) -> bool) -> bool {
    let Some((sel, rest)) = picks.split_first() else { return good(d) };
    for k in 0..d.inputs.len() {
        if !d.inputs.get(k).is_some_and(|inp| takes(s, inp, sel)) {
            continue;
        }
        if let Some(inp) = d.inputs.get_mut(k) {
            inp.items.push(sel.clone());
        }
        if place(s, d, rest, good) {
            return true;
        }
        if let Some(inp) = d.inputs.get_mut(k) {
            inp.items.pop();
        }
    }
    false
}

/// The first method's dialog, in the family's order, whose inputs take `picks` and that builds
/// from them; else the first whose inputs take them (its error says why it doesn't build); else
/// the tool's own window, waiting for picks. A method chosen again keeps its values.
fn choose(app: &SolveApp, tool: &'static str, picks: Vec<Sel>, was: Option<&Dialog>) -> Dialog {
    let s = &app.session;
    let mut chosen = None;
    if !picks.is_empty() {
        let complete = |d: &Dialog| apply_commands(app, d).is_ok();
        let builds = |d: &Dialog| apply_commands(app, d).is_ok_and(|cmds| s.preview(&cmds).is_ok());
        let methods = solidcraft_engine::cmd::construct_methods(tool);
        'tries: for good in [&builds as &dyn Fn(&Dialog) -> bool, &complete] {
            for spec in &methods {
                let Some(mut d) = Dialog::for_command(app, spec.id) else { continue };
                if let Some(w) = was
                    && w.auto.is_some()
                    && w.command() == Some(spec.id)
                {
                    d.kind = w.kind.clone();
                    d.extra = w.extra.clone();
                }
                for inp in &mut d.inputs {
                    inp.items.clear();
                }
                if place(s, &mut d, &picks, good) {
                    chosen = Some(d);
                    break 'tries;
                }
            }
        }
    }
    let mut d = chosen.unwrap_or_else(|| {
        let mut refs = SelInput::new(REFERENCES, PICKS, true);
        refs.items = picks.clone();
        Dialog::new(Kind::ConstructAuto { command: tool }, vec![refs])
    });
    if let Some(w) = was {
        d.collapsed = w.collapsed;
        d.preview_enabled = w.preview_enabled;
    }
    d.advance();
    d.focus = true;
    d.auto = Some(Auto { tool, placed: d.items(), picks, dirty: false });
    d
}
