//! Ordered transient Insert Point clicks consume the shared exact-curve cache.
use super::*;
use std::collections::VecDeque;
struct Click {
    source: curves::Key,
    projection: Proj,
    position: Pos2,
}
#[derive(Default)]
pub(super) struct State {
    clicks: VecDeque<Click>,
    chain_source: Option<curves::Key>,
    commit: bool,
}
fn fail(app: &mut SolveApp, message: String) {
    app.forms_prototype.tools.curve.clicks.clear();
    app.forms_prototype.tools.curve.commit = false;
    app.set_status(message, true);
}
pub(super) fn point(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, pos: Pos2, clicked: bool) -> Option<([usize; 2], f64, Vec3)> {
    if clicked {
        if app.forms_prototype.tools.curve.clicks.len() + app.forms_prototype.tools.chain.len() >= 32 {
            fail(app, app.tr("forms.prototype.insert_point_limit").to_owned());
            return None;
        }
        if let Some(source) = curves::source(app) {
            app.forms_prototype.tools.curve.clicks.push_back(Click { source, projection: *proj, position: pos });
            curves::prepare_explicit(app, ctx);
        }
    }
    tick(app, ctx);
    curves::nearest(app, proj, pos)
}
pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    curves::tick(app, ctx);
    let current = curves::source(app);
    if app.forms_prototype.tools.curve.chain_source.as_ref().is_some_and(|given| Some(given) != current.as_ref()) {
        app.forms_prototype.tools.chain.clear();
        app.forms_prototype.tools.curve.chain_source = None;
    }
    while let Some(click) = app.forms_prototype.tools.curve.clicks.pop_front() {
        if Some(&click.source) != current.as_ref() {
            fail(app, app.tr("forms.prototype.changed_during_fit").to_owned());
            break;
        }
        match curves::ready(app) {
            None => {
                app.forms_prototype.tools.curve.clicks.push_front(click);
                break;
            }
            Some(Err(error)) => {
                let error = error.to_owned();
                fail(app, error);
                break;
            }
            Some(Ok(_)) => {}
        }
        let Some(point) = curves::nearest(app, &click.projection, click.position) else {
            fail(app, app.tr("forms.prototype.insert_curve_miss").to_owned());
            break;
        };
        app.forms_prototype.tools.curve.chain_source = Some(click.source);
        app.forms_prototype.tools.chain.push(point);
    }
    if app.forms_prototype.tools.curve.commit && app.forms_prototype.tools.curve.clicks.is_empty() {
        app.forms_prototype.tools.curve.commit = false;
        super::commit_chain(app, ctx);
    }
}
pub(super) fn defer_commit(app: &mut SolveApp) -> bool {
    if app.forms_prototype.tools.curve.chain_source.as_ref().is_some_and(|given| curves::source(app).as_ref() != Some(given)) {
        app.forms_prototype.tools.chain.clear();
        fail(app, app.tr("forms.prototype.changed_during_fit").to_owned());
        return true;
    }
    if app.forms_prototype.tools.curve.clicks.is_empty() {
        false
    } else {
        app.forms_prototype.tools.curve.commit = true;
        true
    }
}
pub(super) fn empty(app: &SolveApp) -> bool {
    app.forms_prototype.tools.chain.is_empty() && app.forms_prototype.tools.curve.clicks.is_empty()
}
#[cfg(test)]
pub(super) fn queue_for_test(app: &mut SolveApp, projection: Proj, position: Pos2) {
    let source = curves::source(app).unwrap();
    app.forms_prototype.tools.curve.clicks.push_back(Click { source, projection, position });
}
