//! Which part of the design the History bar shows, as a path from the whole design down to one
//! component. While a component is being edited, or one component is picked in the Browser, the
//! bar shows only its features and those of the components inside it. A path ahead of the
//! features names the way there: "All components", the components around it, then itself. A
//! click on a step shows that component's features (or the whole design's), with the way to the
//! edited or picked component still open; a step after the one shown goes back in. A click on a
//! deep run's chip (one holding runs that fold in turn) shows its component the same way. Once
//! another component is edited or picked, the bar follows it again.
//!
//! The history marker, dragging features and rolling back work on the design's own positions
//! (`timeline_fold::View`). The choice is kept with the design's tab (`TreeState::timeline_scope`).
use std::collections::{HashMap, HashSet};

use egui::{Painter, Rect, Sense, pos2, vec2};

use crate::SolveApp;
use crate::theme::Tokens;

/// A step's name is cut at this width (its tooltip has it whole).
const STEP_MAX: f32 = 120.0;
/// The path names at most this many components; the others fold into a "…" step.
const STEPS_SHOWN: usize = 3;
/// Components nested deeper than this end a path (a damaged design's loop of parents).
const DEPTH_MAX: usize = 64;

/// Which part of the design the History bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Scope {
    /// The component being edited, else the one picked in the Browser; the whole design when
    /// there is neither.
    #[default]
    Follow,
    /// The whole design.
    All,
    /// This component and the components inside it.
    Component(u64),
}

/// The component being edited, else the one component picked in the Browser.
pub(crate) fn focus(app: &SolveApp) -> Option<u64> {
    let focus = match app.tree.picked_components.as_slice() {
        _ if app.session.active_component != 0 => Some(app.session.active_component),
        [one] if *one != 0 => Some(*one),
        _ => None,
    };
    focus.filter(|c| exists(app, *c))
}

fn exists(app: &SolveApp, component: u64) -> bool {
    app.session.doc.components.iter().any(|c| c.id == component)
}

/// The component whose features the bar shows; none for the whole design.
pub(crate) fn shown(app: &SolveApp) -> Option<u64> {
    match app.tree.timeline_scope {
        Scope::Follow => focus(app),
        Scope::All => None,
        Scope::Component(c) => Some(c).filter(|c| exists(app, *c)).or_else(|| focus(app)),
    }
}

/// The component `component` and every component inside it.
pub(crate) fn components_within(app: &SolveApp, component: u64) -> HashSet<u64> {
    let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
    for c in &app.session.doc.components {
        children.entry(c.parent).or_default().push(c.id);
    }
    let mut out = HashSet::from([component]);
    let mut stack = vec![component];
    while let Some(c) = stack.pop() {
        for child in children.get(&c).into_iter().flatten() {
            if out.insert(*child) {
                stack.push(*child);
            }
        }
    }
    out
}

/// The components from the outermost one below the root down to `component`.
fn path_to(app: &SolveApp, component: u64) -> Vec<u64> {
    let parent: HashMap<u64, u64> = app.session.doc.components.iter().map(|c| (c.id, c.parent)).collect();
    let mut path = Vec::new();
    let mut at = component;
    while at != 0 && path.len() < DEPTH_MAX && !path.contains(&at) {
        path.push(at);
        at = parent.get(&at).copied().unwrap_or(0);
    }
    path.reverse();
    path
}

/// Once another component is edited or picked, the bar shows it and scrolls its last feature
/// into view. A feature to scroll to (Find in Timeline, a face picked in the view) outside the
/// part shown widens it to the whole design.
pub(crate) fn follow(app: &mut SolveApp) {
    let focus = focus(app);
    if app.tree.timeline_followed != Some(focus) {
        app.tree.timeline_followed = Some(focus);
        app.tree.timeline_scope = Scope::Follow;
        if let Some(component) = focus {
            app.tree.timeline_reveal = last_feature(app, component);
        }
    }
    let Some(shown) = shown(app) else {
        return;
    };
    let outside = app.tree.timeline_reveal.and_then(|id| app.session.doc.features.iter().find(|f| f.id == id)).map(|f| f.component);
    if let Some(component) = outside
        && !components_within(app, shown).contains(&component)
    {
        app.tree.timeline_scope = if focus.is_some() { Scope::All } else { Scope::Follow };
    }
}

/// The last feature of `component` or of a component inside it.
fn last_feature(app: &SolveApp, component: u64) -> Option<u64> {
    let within = components_within(app, component);
    app.session.doc.features.iter().rev().find(|f| within.contains(&f.component)).map(|f| f.id)
}

/// Shows `component`'s features (a deep run's chip clicked).
pub(crate) fn enter(app: &mut SolveApp, component: u64) {
    app.tree.timeline_scope = if focus(app) == Some(component) { Scope::Follow } else { Scope::Component(component) };
}

/// One step of the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    All,
    /// Components left out between two steps; a click shows the innermost of them.
    More(u64),
    Component(u64),
}

/// The steps from "All components" to the edited or picked component, or to the one shown when
/// that is not on the way to it. The shown step and the last one always show.
fn steps(app: &SolveApp, shown: Option<u64>) -> Vec<Step> {
    let target = match (shown, focus(app)) {
        (Some(s), Some(f)) if path_to(app, f).contains(&s) => Some(f),
        (Some(s), _) => Some(s),
        (None, f) => f,
    };
    let path = target.map(|c| path_to(app, c)).unwrap_or_default();
    let mut keep = vec![false; path.len()];
    let mut left = STEPS_SHOWN;
    let shown_at = shown.and_then(|s| path.iter().position(|c| *c == s));
    for k in shown_at.into_iter().chain((0..path.len()).rev()) {
        if left == 0 {
            break;
        }
        if let Some(kept) = keep.get_mut(k)
            && !*kept
        {
            *kept = true;
            left -= 1;
        }
    }
    let mut out = vec![Step::All];
    let mut left_out = None;
    for (component, kept) in path.iter().zip(keep) {
        if kept {
            if let Some(innermost) = left_out.take() {
                out.push(Step::More(innermost));
            }
            out.push(Step::Component(*component));
        } else {
            left_out = Some(*component);
        }
    }
    out
}

/// Draws the path from `x` in the bar `r` and returns where the features start; in a design
/// without components nothing is drawn.
pub(crate) fn header(app: &mut SolveApp, ui: &mut egui::Ui, p: &Painter, x: f32, r: Rect) -> f32 {
    let shown = shown(app);
    if shown.is_none() && focus(app).is_none() && !app.session.doc.features.iter().any(|f| f.component != 0) {
        return x;
    }
    let t = Tokens::get();
    let mut x = x;
    for (k, step) in steps(app, shown).into_iter().enumerate() {
        if k > 0 {
            x = crate::ui_kit::history_crumb_arrow(p, pos2(x, r.center().y));
        }
        x = step_button(app, ui, p, x, r, step, shown) + t.space_2;
    }
    x + t.space_8
}

fn step_name(app: &SolveApp, step: Step) -> String {
    match step {
        Step::All => crate::i18n::tr_label("All components").to_string(),
        Step::More(_) => "\u{2026}".to_string(),
        Step::Component(c) => crate::timeline_fold::name(app, c),
    }
}

/// One step: the shown one stands out; a click on another shows its features. Returns where it
/// ends.
fn step_button(app: &mut SolveApp, ui: &mut egui::Ui, p: &Painter, x: f32, r: Rect, step: Step, shown: Option<u64>) -> f32 {
    let t = Tokens::get();
    let current = match step {
        Step::All => shown.is_none(),
        Step::Component(c) => shown == Some(c),
        Step::More(_) => false,
    };
    let name = step_name(app, step);
    let accent = crate::theme::TabAccent::for_tab(&t, &app.ui.tab);
    let galley = p.layout_no_wrap(name.clone(), egui::FontId::proportional(t.font_small), t.text_dim);
    let size = vec2(galley.size().x.min(STEP_MAX) + 2.0 * t.space_4, t.space_24);
    let rect = Rect::from_min_size(pos2(x, r.center().y - t.space_24 / 2.0), size);
    let response = ui.interact(rect, ui.id().with(("history_path", step_key(step))), Sense::click());
    crate::scenario::publish_handle(&format!("timeline-path:{name}"), rect.center());
    let ink = if current {
        p.rect_filled(rect, t.radius_sm, accent.soft);
        accent.ink
    } else if response.hovered() {
        p.rect_filled(rect, t.radius_sm, t.hover);
        t.text
    } else {
        t.text_dim
    };
    let text_at = pos2(rect.left() + t.space_4, rect.center().y - galley.size().y / 2.0);
    p.with_clip_rect(rect.shrink2(vec2(t.space_4, 0.0)).intersect(p.clip_rect())).galley(text_at, galley, ink);
    let tip = step_tip(app, step, current);
    let response = if current { response } else { response.on_hover_cursor(egui::CursorIcon::PointingHand) };
    if response.on_hover_text(tip).clicked() && !current {
        show_step(app, step);
    }
    rect.right()
}

/// Shows a step's features; the edited or picked component's last feature scrolls into view
/// when they hold it.
fn show_step(app: &mut SolveApp, step: Step) {
    let focus = focus(app);
    app.tree.timeline_scope = match step {
        Step::All if focus.is_none() => Scope::Follow,
        Step::All => Scope::All,
        Step::More(c) | Step::Component(c) if focus == Some(c) => Scope::Follow,
        Step::More(c) | Step::Component(c) => Scope::Component(c),
    };
    let holds = |f: u64| match shown(app) {
        None => true,
        Some(c) => components_within(app, c).contains(&f),
    };
    if let Some(focus) = focus.filter(|f| holds(*f)) {
        app.tree.timeline_reveal = last_feature(app, focus);
    }
}

fn step_key(step: Step) -> (u8, u64) {
    match step {
        Step::All => (0, 0),
        Step::More(c) => (1, c),
        Step::Component(c) => (2, c),
    }
}

fn step_tip(app: &SolveApp, step: Step, current: bool) -> String {
    match step {
        Step::All if current && focus(app).is_none() => crate::i18n::tr("ui.history_scope_pick").to_string(),
        Step::All if current => crate::i18n::tr("ui.history_scope_all_shown").to_string(),
        Step::All => crate::i18n::tr("ui.history_scope_all").to_string(),
        Step::More(c) => path_to(app, c).iter().map(|c| crate::timeline_fold::name(app, *c)).collect::<Vec<_>>().join(" \u{203a} "),
        Step::Component(c) if current => crate::i18n::format("ui.history_scope_shown", &[("name", &crate::timeline_fold::name(app, c))]),
        Step::Component(c) => crate::i18n::format("ui.history_scope_show", &[("name", &crate::timeline_fold::name(app, c))]),
    }
}
