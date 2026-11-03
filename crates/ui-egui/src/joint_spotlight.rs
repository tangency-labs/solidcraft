//! A joint picked in the browser or the timeline lights the parts it joins: they stay solid and
//! the others fade, so the joint can be looked at. This is a glance, so it gives way easily: it
//! ends with the pick, and sooner when anything shows the user has moved on (a tool, a dialog, a
//! selection, an edit, a part shown or hidden). The joint stays picked then, and doesn't light up
//! again until it is picked anew. Moving the camera is looking, and leaves it on. The preference
//! `highlight_joint_parts` turns it off.

use std::sync::Arc;

use solidcraft_engine::doc::{Document, Visibility};

use crate::SolveApp;

/// What the last pick lit, kept between frames.
#[derive(Default)]
pub struct Spotlight {
    /// The pick this answers.
    pick: Vec<u64>,
    /// `None` when nothing is lit: no pick, or it has given way.
    lit: Option<Lit>,
}

/// What was lit, and the state of the design it was lit in.
struct Lit {
    bodies: Vec<String>,
    doc: Arc<Document>,
    visibility: Visibility,
    active: (u64, Option<Vec<u64>>),
    moves: usize,
}

impl Lit {
    fn of(app: &SolveApp, joints: &[u64]) -> Option<Lit> {
        let s = &app.session;
        let bodies = s.joint_bodies(joints);
        (!bodies.is_empty()).then(|| Lit {
            bodies,
            doc: s.doc.clone(),
            visibility: s.visibility.clone(),
            active: (s.active_component, s.active_occurrence_path.clone()),
            moves: s.pending_moves.len(),
        })
    }

    /// Is the design as it was lit?
    fn holds(&self, app: &SolveApp) -> bool {
        let s = &app.session;
        Arc::ptr_eq(&self.doc, &s.doc)
            && self.visibility == s.visibility
            && self.active.0 == s.active_component
            && self.active.1 == s.active_occurrence_path
            && self.moves == s.pending_moves.len()
    }
}

/// Is the light turned off, or the user at something else than looking at the design?
fn gives_way(app: &SolveApp) -> bool {
    let s = &app.session;
    !app.preferences.highlight_joint_parts
        || app.tool.is_some()
        || app.dialog.is_some()
        || app.pending_feature_edit.is_some()
        || s.active_sketch.is_some()
        || !s.selection.is_empty()
        || s.joint_focus().is_some()
        || crate::render_workspace::active(app)
}

/// Bring the spotlight in line with the pick and the app, once a frame. True if it changed.
pub(crate) fn follow(app: &mut SolveApp) -> bool {
    if app.tree.joint_spotlight.pick != app.tree.picked_joints {
        let pick = app.tree.picked_joints.clone();
        let lit = if pick.is_empty() || gives_way(app) { None } else { Lit::of(app, &pick) };
        let changed = lit.is_some() || app.tree.joint_spotlight.lit.is_some();
        app.tree.joint_spotlight = Spotlight { pick, lit };
        changed
    } else if app.tree.joint_spotlight.lit.as_ref().is_some_and(|lit| gives_way(app) || !lit.holds(app)) {
        app.tree.joint_spotlight.lit = None;
        true
    } else {
        false
    }
}

/// The bodies that stay solid while the spotlight is on.
pub(crate) fn bodies(app: &SolveApp) -> Option<&[String]> {
    app.tree.joint_spotlight.lit.as_ref().map(|lit| lit.bodies.as_slice())
}
