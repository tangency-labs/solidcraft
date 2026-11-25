//! Opacity in the menus of bodies, components and placements (`browser.opacity`): the percentages
//! that apply at once, and Custom Opacity's card asking for any other.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::context_menu::act;
use crate::ui_kit::{self, Item};

/// The percentages with a row of their own are this far apart.
const STEP: u8 = 10;

/// What an Opacity entry sets: bodies by name, or exact placements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Targets {
    pub bodies: Vec<String>,
    pub paths: Vec<Vec<u64>>,
}

/// The Custom Opacity card.
#[derive(Clone, Debug)]
pub struct Prompt {
    /// The design it was asked for in: it goes when another is shown.
    session: u64,
    targets: Targets,
    /// What is typed.
    pub text: String,
    /// Its text has been selected, so typing replaces it.
    selected: bool,
}

impl Targets {
    pub fn bodies(bodies: Vec<String>) -> Targets {
        Targets { bodies, paths: Vec::new() }
    }

    pub fn paths(paths: Vec<Vec<u64>>) -> Targets {
        Targets { bodies: Vec::new(), paths }
    }

    fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.paths.is_empty()
    }

    /// The percentage they all have, if they have one.
    fn percent(&self, app: &SolveApp) -> Option<u8> {
        let set = &app.session.visibility.opacity;
        let mut all = self.bodies.iter().map(|b| set.body(b)).chain(self.paths.iter().map(|p| set.placement(p)));
        let first = all.next()?;
        all.all(|percent| percent == first).then_some(first)
    }

    /// The command's parameters for them, without the percentage.
    fn params(&self) -> Value {
        let mut p = json!({});
        if !self.bodies.is_empty() {
            p["bodies"] = json!(self.bodies);
        }
        if !self.paths.is_empty() {
            p["paths"] = json!(self.paths);
        }
        p
    }

    /// The targets `params` gives back, as [`Self::params`] wrote them.
    fn of(p: &Value) -> Targets {
        fn list<T: serde::de::DeserializeOwned>(p: &Value, key: &str) -> Vec<T> {
            p.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
        }
        Targets { bodies: list(p, "bodies"), paths: list(p, "paths") }
    }
}

/// Opacity ▸ Custom Opacity, then 0% to 100% by tens. The one they all have is checked: its own
/// row, or Custom Opacity for a percentage without one. Different percentages check none.
pub fn menu(app: &SolveApp, targets: &Targets) -> Item {
    let enabled = !targets.is_empty();
    let current = targets.percent(app);
    let mark = |on: bool| if on { "current" } else { "" };
    let mut menu = act("ui.opacity", "Opacity", "style_translucent").on(enabled);
    let custom = current.is_some_and(|percent| percent % STEP != 0);
    menu.children =
        vec![act("ui.customOpacity", "Custom Opacity", "style_translucent").with(targets.params()).key(mark(custom)).on(enabled), Item::sep()];
    menu.children.extend((0..=100u8).step_by(usize::from(STEP)).map(|percent| {
        let mut p = targets.params();
        p["percent"] = json!(percent);
        act("browser.opacity", &format!("{percent}%"), "style_translucent").with(p).key(mark(current == Some(percent))).on(enabled)
    }));
    menu
}

/// Put Opacity directly under Show/Hide.
pub fn add(app: &SolveApp, items: &mut Vec<Item>, targets: &Targets) {
    let entry = menu(app, targets);
    let under = items.iter().position(|i| i.label == "Show/Hide").map_or(items.len(), |at| at + 1);
    items.insert(under, entry);
}

/// Open the Custom Opacity card for what a menu entry's `params` name.
pub fn ask(app: &mut SolveApp, params: &Value) {
    open(app, Targets::of(params));
}

/// Opacity started without a menu (the command search, a key): the card for what is selected.
pub fn ask_for_selection(app: &mut SolveApp) {
    let paths: Vec<Vec<u64>> = app
        .session
        .selection
        .iter()
        .filter_map(|s| if let solidcraft_engine::Sel::Occurrence { path } = s { Some(path.clone()) } else { None })
        .collect();
    let mut bodies = if app.session.all_bodies_selected() { crate::context_menu::sel_bodies(app) } else { crate::context_menu::touched_bodies(app) };
    for component in app.tree.picked_components.clone() {
        for body in crate::browser::selected_component_bodies(app, component) {
            if !bodies.contains(&body) {
                bodies.push(body);
            }
        }
    }
    // A placement selected stands for its bodies.
    if !paths.is_empty() {
        bodies.clear();
    }
    let targets = Targets { bodies, paths };
    if targets.is_empty() {
        app.set_status(solidcraft_ui_text::tr("opacity.select_first"), true);
        return;
    }
    open(app, targets);
}

fn open(app: &mut SolveApp, targets: Targets) {
    if targets.is_empty() || crate::render_workspace::active(app) {
        return;
    }
    let text = targets.percent(app).unwrap_or(solidcraft_engine::doc::display_opacity::OPAQUE).to_string();
    app.menu.opacity = Some(Prompt { session: app.session.uid, targets, text, selected: false });
}

/// A typed percentage: a whole number from 0 to 100, its sign optional.
fn typed(text: &str) -> Option<u8> {
    let text = text.trim();
    text.strip_suffix('%').unwrap_or(text).trim_end().parse::<u8>().ok().filter(|p| *p <= solidcraft_engine::doc::display_opacity::OPAQUE)
}

/// Is the card open? It owns the keyboard.
pub fn asking(app: &SolveApp) -> bool {
    app.menu.opacity.is_some()
}

/// The Custom Opacity card.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut prompt) = app.menu.opacity.take().filter(|p| p.session == app.session.uid) else { return };
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        return;
    }
    let valid = typed(&prompt.text).is_some();
    let (closed, action) =
        ui_kit::Confirm::new(egui::Id::unique("sc_custom_opacity"), "style_translucent", solidcraft_ui_text::tr_label("Custom Opacity"))
            .actions("OK", "Cancel", None)
            .show(ctx, valid, |ui| {
                ui_kit::row(ui, "Custom opacity %", |ui| ui_kit::focused_text_field(ui, &mut prompt.text, "0–100"));
                // A percentage needs a few characters; anything longer is refused below in any case.
                if let Some((end, _)) = prompt.text.char_indices().nth(16) {
                    prompt.text.truncate(end);
                }
                if typed(&prompt.text).is_none() {
                    ui_kit::text(ui, solidcraft_ui_text::tr("opacity.out_of_range"), ui_kit::Text::Report("opacity_error", true));
                }
            });
    if !prompt.selected
        && let Some(id) = ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_focused_editor"))).filter(|id| ctx.memory(|m| m.has_focus(*id)))
    {
        ui_kit::select_all_text(ctx, id, &prompt.text);
        prompt.selected = true;
    }
    if closed || action == ui_kit::Action::Cancel {
        return;
    }
    if action == ui_kit::Action::Ok
        && let Some(percent) = typed(&prompt.text)
    {
        let mut p = prompt.targets.params();
        p["percent"] = json!(percent);
        if let Err(error) = app.run("browser.opacity", p) {
            app.set_status(crate::friendly_error("browser.opacity", &error), true);
        }
        return;
    }
    app.menu.opacity = Some(prompt);
}

#[cfg(test)]
mod tests {
    use super::typed;

    #[test]
    fn a_typed_percentage_is_a_whole_number_to_100() {
        for (text, percent) in [("0", 0), ("35", 35), (" 100 ", 100), ("5%", 5), ("007", 7), ("40 %", 40)] {
            assert_eq!(typed(text), Some(percent), "{text}");
        }
        for text in ["", " ", "101", "-1", "1.5", "a", "%", "5%%", "99999999999999999999", "∞"] {
            assert_eq!(typed(text), None, "{text}");
        }
    }
}
