//! Fields elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use super::{
    layout::{label, publish},
    metrics::*,
    text::paint_text,
};
use crate::ui_kit::{Container, Surface};
use crate::{scenario, theme::Tokens};
use egui::{Response, RichText, Ui, vec2};

/// One alignment policy for every kit text input. Align the galley itself rather than
/// approximating font metrics with integer margins (Plex Mono and Sans differ).
pub(super) fn text_input(edit: egui::TextEdit<'_>) -> egui::TextEdit<'_> {
    let t = Tokens::get();
    edit.vertical_align(egui::Align::Center).margin(vec2(t.form_inset, 0.0)).min_size(vec2(0.0, t.control_height))
}

/// Expression editor with the application's existing completion and number-focus behaviour.
/// Values carry their own unit text ("20 mm", "45 deg"): there is no unit suffix. Returns accepted Enter.
/// Tokens: `control_height, control_fill, form_inset, font_label, space_4; scope tokens`.
pub(crate) fn field(ui: &mut Ui, value: &mut String) -> bool {
    editor(ui, value, "", true).0
}

/// Expression editor with empty-value hint. Tokens: `same as field`.
pub(crate) fn hint_field(ui: &mut Ui, value: &mut String, hint: &str) -> bool {
    editor(ui, value, hint, true).0
}

/// The whole numbers a count field may hold, and how its text evaluates (parameters and
/// expressions included). `eval` returns None for text that is not a value.
pub(crate) struct Count<'a> {
    pub min: i64,
    pub max: i64,
    pub eval: &'a dyn Fn(&str) -> Option<f64>,
}

/// Expression editor for a whole-number count (a pattern's quantity), with up and down step
/// arrows at its right edge. An arrow steps the evaluated count by one and stays within the
/// limits: a plain number is replaced, an expression keeps its parameters and gains or adjusts
/// a trailing `+ n`. The text changes as if typed, so previews follow. Returns accepted Enter.
/// Tokens: field tokens, `control_border`, `selected_bg`, `form_ink`, `form_dim`.
pub(crate) fn count_field(ui: &mut Ui, value: &mut String, limits: &Count<'_>) -> bool {
    let t = Tokens::get();
    let reserve = (t.control_height * 0.75).round();
    let (enter, _, response) = editor_reserving(ui, value, "", true, reserve);
    let rect = response.rect;
    let column = egui::Rect::from_min_max(egui::pos2((rect.right() - reserve).max(rect.left()), rect.top()), rect.max);
    let painter = ui.painter_at(rect);
    painter.vline(column.left(), column.y_range().shrink(t.space_4), egui::Stroke::new(1.0, t.control_border));
    let row = label(ui).to_owned();
    for (delta, half, name) in [(1, column.split_top_bottom_at_fraction(0.5).0, "up"), (-1, column.split_top_bottom_at_fraction(0.5).1, "down")] {
        let next = step_count(value, delta, limits);
        let arrow = ui.interact(half, ui.id().with(("sc_count_step", &row, name)), egui::Sense::click());
        let enabled = ui.is_enabled() && next.is_some();
        if enabled && arrow.hovered() {
            painter.rect_filled(half.shrink(1.0), t.control_radius, t.selected_bg);
        }
        let ink = if enabled { t.form_ink } else { t.form_dim };
        let (c, w, h) = (half.center(), (reserve * 0.18).max(2.0), (half.height() * 0.2).max(2.0));
        let tip = if delta > 0 { -h } else { h };
        painter.add(egui::Shape::convex_polygon(
            vec![egui::pos2(c.x, c.y + tip * 0.5), egui::pos2(c.x + w, c.y - tip * 0.5), egui::pos2(c.x - w, c.y - tip * 0.5)],
            ink,
            egui::Stroke::NONE,
        ));
        let sign = if delta > 0 { "+1" } else { "−1" };
        arrow.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, format!("{} {sign}", crate::i18n::tr_label(&row))));
        scenario::publish_handle(&format!("dialog:{row} {name}"), half.center());
        if let (true, true, Some(next)) = (enabled, arrow.clicked(), next) {
            *value = next;
        }
    }
    enter
}

/// `text` stepped by `delta` whole numbers within `limits`, or None when it does not evaluate
/// or the step would leave the limits. A typed number between whole numbers steps to the next
/// one; an expression that evaluates between them is left alone.
fn step_count(text: &str, delta: i64, limits: &Count<'_>) -> Option<String> {
    const SAFE: f64 = 1e15;
    let trimmed = text.trim();
    let now = (limits.eval)(trimmed).filter(|v| v.is_finite() && v.abs() < SAFE)?;
    let from = if delta > 0 { now.floor() } else { now.ceil() } as i64;
    let target = from.saturating_add(delta).clamp(limits.min, limits.max);
    let moves = if delta > 0 { target as f64 > now } else { (target as f64) < now };
    if !moves {
        return None;
    }
    if trimmed.parse::<f64>().is_ok() {
        return Some(target.to_string());
    }
    // An expression: adjust its trailing `+ n` / `- n`, or add one, and check that it now says `target`.
    let offset = |base: &str, n: i64| match n {
        0 => base.to_owned(),
        n if n > 0 => format!("{base} + {n}"),
        n => format!("{base} - {}", n.unsigned_abs()),
    };
    let tail = trimmed.rfind(['+', '-']).and_then(|at| {
        let (base, number) = (trimmed[..at].trim_end(), trimmed[at + 1..].trim());
        let n: i64 = number.bytes().all(|b| b.is_ascii_digit()).then(|| number.parse().ok()).flatten()?;
        let base_ok = !base.is_empty() && !base.ends_with(['+', '-', '*', '/', '^', '(', ',']);
        base_ok.then(|| (base, if trimmed[at..].starts_with('-') { -n } else { n }))
    });
    let step = target - now.round() as i64;
    let candidates =
        [tail.map(|(base, n)| offset(base, n.saturating_add(step))), Some(offset(trimmed, step)), Some(offset(&format!("({trimmed})"), step))];
    candidates.into_iter().flatten().find(|c| (limits.eval)(c).is_some_and(|v| (v - target as f64).abs() < 1e-9))
}

/// Names, thread designations and plain numeric counts do not accept expression completion.
/// Plain text editor without expression completion; returns (Enter, changed). Tokens: `same as field`.
pub(crate) fn text_field(ui: &mut Ui, value: &mut String, hint: &str) -> (bool, bool) {
    let (enter, changed, _) = editor(ui, value, hint, false);
    (enter, changed)
}

/// Expression field for a single-value popup which keeps ownership of keyboard focus.
/// Tokens: field tokens. Preserves the legacy Edit Dimension focus behavior.
pub(crate) fn focused_field(ui: &mut Ui, value: &mut String) -> bool {
    let (enter, _, response) = editor(ui, value, "", true);
    response.request_focus();
    enter
}

/// Plain text editor which keeps keyboard focus (a name asked for in a one-field card); returns
/// accepted Enter. Tokens: `same as field`.
pub(crate) fn focused_text_field(ui: &mut Ui, value: &mut String, hint: &str) -> bool {
    let (enter, _, response) = editor(ui, value, hint, false);
    response.request_focus();
    enter
}

/// Multiline plain text with a stable focus ID. The caller owns Enter/Shift+Enter routing.
/// Tokens: control_height, control_fill, form_inset and font_label.
pub(crate) fn multiline_text_field(ui: &mut Ui, label: &str, id: egui::Id, value: &mut String) -> Response {
    super::layout::sized_row(ui, label, row_height() * 3.0, |ui| {
        let t = Tokens::get();
        let mut response = ui.add_sized(
            vec2(dimension(ui.available_width()), row_height() * 3.0),
            text_input(egui::TextEdit::multiline(value))
                .id(id)
                .desired_rows(3)
                .font(egui::FontId::monospace(t.font_label))
                .background_color(t.input_fill()),
        );
        super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINES);
        publish(ui, &response, egui::WidgetType::TextEdit);
        response
    })
}

/// Supplied entry in a searchable dropdown. Handles are optional stable application aliases.
pub(crate) struct SearchOption<'a> {
    pub value: &'a str,
    pub label: &'a str,
    pub handle: Option<&'a str>,
}

/// Searchable dropdown with an optional custom-value action and loading message.
/// Returns a chosen value; callers keep domain-specific validation and discovery.
/// Tokens: control_height, space_24, font_label and scope control tokens.
pub(crate) fn searchable_dropdown(
    ui: &mut Ui,
    id: &str,
    current: &str,
    filter: &mut String,
    options: &[SearchOption<'_>],
    custom: Option<&str>,
    pending: Option<&str>,
) -> Option<String> {
    searchable_dropdown_with(ui, id, current, filter, options, SearchMenu { custom, pending, icon: None })
}

struct SearchMenu<'a> {
    custom: Option<&'a str>,
    pending: Option<&'a str>,
    icon: Option<&'static str>,
}

fn searchable_dropdown_with(
    ui: &mut Ui,
    id: &str,
    current: &str,
    filter: &mut String,
    options: &[SearchOption<'_>],
    menu: SearchMenu<'_>,
) -> Option<String> {
    let SearchMenu { custom, pending, icon } = menu;
    crate::prefs_search::note_current(ui, current);
    let mut chosen = None;
    let row = label(ui).to_owned();
    let combo = egui::ComboBox::from_id_salt(id)
        .width(if icon.is_some() { Tokens::get().control_height } else { dimension(ui.available_width()) })
        .height(Tokens::get().space_24 * 14.0)
        .selected_text(if icon.is_some() { "" } else { current })
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let combo = if let Some(icon) = icon {
        combo.icon(move |ui, rect, _, _| crate::icons::paint_ink(ui.painter(), rect.shrink(Tokens::get().space_2), icon, Tokens::get().form_dim))
    } else {
        combo.icon(dropdown_arrow)
    };
    let response = combo
        .show_ui(ui, |ui| {
            let search = ui.add(
                text_input(egui::TextEdit::singleline(filter))
                    .id_salt(format!("{id}_search"))
                    .hint_text(crate::i18n::tr_label("Search"))
                    .desired_width(f32::INFINITY),
            );
            // Preferences types into its own search only while no field has the keyboard.
            if search.has_focus() {
                crate::prefs_search::note_key_sink(ui.ctx());
            }
            scenario::publish_handle(&format!("dialog:{row} search"), search.rect.center());
            if let Some(custom) = custom {
                let response = ui.selectable_label(false, custom);
                scenario::publish_handle(&format!("dialog:{row} file"), response.rect.center());
                if response.clicked() {
                    chosen = Some(filter.trim().to_owned());
                    ui.close();
                }
                return;
            }
            let query = filter.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt(format!("{id}_list"))
                .max_height(Tokens::get().space_24 * 10.0)
                .min_scrolled_height(Tokens::get().space_24 * 10.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for option in options.iter().filter(|o| o.label.to_lowercase().contains(&query)) {
                        let response = ui.selectable_label(current == option.label, option.label);
                        if let Some(handle) = option.handle {
                            scenario::publish_handle(handle, response.rect.center());
                        }
                        if response.clicked() {
                            chosen = Some(option.value.to_owned());
                            ui.close();
                        }
                    }
                    if let Some(pending) = pending {
                        ui.label(pending);
                    }
                });
        })
        .response;
    publish(ui, &response, egui::WidgetType::ComboBox);
    chosen
}

/// Longest search text and custom value a `search_picker` takes.
const SEARCH_TEXT_CAP: usize = 256;
/// Characters of a label shown on a `search_picker` row, and of a tip or note shown with it.
const SEARCH_LABEL_CAP: usize = 64;
const SEARCH_NOTE_CAP: usize = 1024;
/// Characters of a `search_picker` note shown under its list; the rest shows on hover.
const SEARCH_NOTE_SHOWN: usize = 200;
/// Rows a `search_picker` makes room for when it opens with none.
const SEARCH_ROWS_AWAITED: usize = 6;

/// One row of a `search_picker`. The value, label and tip are searched; `tip` also shows on hover,
/// and a change of `group` between rows draws a heading.
pub(crate) struct SearchChoice<'a> {
    pub value: &'a str,
    pub label: &'a str,
    pub tip: &'a str,
    pub group: Option<&'a str>,
}

/// What a `search_picker` shows.
pub(crate) struct SearchPicker<'a> {
    /// ComboBox id salt; it also keys the search text, so one per picker in the window.
    pub id: &'a str,
    /// Scenario names: the control `{handles}`, its box `{handles}:search`, its custom row
    /// `{handles}:custom` and its rows `{handles}:{value}`.
    pub handles: &'a str,
    /// The text on the closed control, and the value of the row to highlight.
    pub current: &'a str,
    pub current_value: &'a str,
    /// The search box's empty-text hint.
    pub hint: &'a str,
    pub choices: &'a [SearchChoice<'a>],
    /// Offer what was typed as a value of its own when it is not a listed one.
    pub custom: bool,
    /// A dim line under the list: still listing, or why it could not.
    pub note: Option<&'a str>,
    /// With two or more groups, each folds under its header. Before anything is typed, only the
    /// current value's group and this one are open; a group with a match opens once something is
    /// typed; a fold made by hand stays until the popup closes.
    pub open_group: Option<&'a str>,
    pub enabled: bool,
}

/// A `search_picker` choice: a listed value, or one typed into the box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Picked {
    Listed(String),
    Custom(String),
}

/// Dropdown whose popup opens on a focused search box over a list of `choices`, filtered as the
/// box is typed in, with an optional row taking the typed text as a value of its own. Enter picks
/// the exact or first match, else the typed text. Only the rows in view are laid out, so a long
/// list stays cheap. Returns the pick and the control's response.
/// Tokens: dropdown tokens, field tokens, `space_24`, `font_small`, `text_dim`, `form_card_width`.
pub(crate) fn search_picker(ui: &mut Ui, p: SearchPicker<'_>) -> egui::InnerResponse<Option<Picked>> {
    crate::prefs_search::note_current(ui, p.current);
    // As in `dropdown_menu_with_close_behavior`: only a search probe skips the real control.
    if crate::prefs_search::querying(ui)
        && (!ui.is_visible() || (crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[p.current])))
    {
        return egui::InnerResponse::new(None, crate::prefs_search::hidden_response(ui));
    }
    let state = egui::Id::new(p.id);
    let mut picked = None;
    let response = ui
        .add_enabled_ui(p.enabled, |ui| {
            let response = egui::ComboBox::from_id_salt(p.id)
                .width(dimension(ui.available_width()))
                .height(Tokens::get().space_24 * 20.0)
                .selected_text(p.current)
                .icon(dropdown_arrow)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| picked = search_picker_popup(ui, &p, state))
                .response;
            // Closed: the next opening starts with an empty box that has the focus.
            if !egui::ComboBox::is_open(ui.ctx(), response.id) {
                ui.data_mut(|data| {
                    data.remove::<String>(state.with("filter"));
                    data.remove::<bool>(state.with("focused"));
                    data.remove::<f32>(state.with("height"));
                    data.remove::<SearchFolds>(state.with("folds"));
                });
            }
            response
        })
        .inner;
    publish(ui, &response, egui::WidgetType::ComboBox);
    scenario::publish_target(p.handles, response.rect);
    egui::InnerResponse::new(picked, response)
}

/// What a `search_picker` shows while it is open; the pick, if one was made this frame.
fn search_picker_popup(ui: &mut Ui, p: &SearchPicker<'_>, state: egui::Id) -> Option<Picked> {
    let t = Tokens::get();
    let (filter_id, focused_id, folds_id) = (state.with("filter"), state.with("focused"), state.with("folds"));
    // Esc closes this popup only: left in the input, it would also cancel a dialog around it.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ui.close();
    }
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    // The rows' text starts where the box's and the headings' do.
    ui.spacing_mut().button_padding.x = t.form_inset;
    ui.set_min_width(search_popup_width(ui, p));
    let mut filter: String = ui.data(|data| data.get_temp(filter_id)).unwrap_or_default();
    let search = ui.add(
        text_input(egui::TextEdit::singleline(&mut filter))
            .id(state.with("search"))
            .char_limit(SEARCH_TEXT_CAP)
            .hint_text(p.hint)
            .background_color(t.input_fill())
            .desired_width(f32::INFINITY),
    );
    t.input_bevel(ui, &search);
    scenario::publish_target(&format!("{}:search", p.handles), search.rect);
    // Asked until it has it: over a modal window egui drops the request of the opening's first frame.
    if !ui.data(|data| data.get_temp::<bool>(focused_id)).unwrap_or(false) {
        if search.has_focus() {
            ui.data_mut(|data| data.insert_temp(focused_id, true));
        } else {
            search.request_focus();
            ui.ctx().request_repaint();
        }
    }
    if search.has_focus() {
        crate::prefs_search::note_key_sink(ui.ctx());
    }
    if search.changed() {
        ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));
    }
    let typed = filter.trim();
    let query: String = typed.chars().flat_map(char::to_lowercase).collect();
    // One buffer for every row: this runs each frame the popup is open, over a list of thousands.
    let mut searched = String::new();
    let mut hit = |choice: &SearchChoice<'_>| {
        searched.clear();
        for text in [choice.value, choice.label, choice.tip] {
            searched.extend(text.chars().take(SEARCH_TEXT_CAP).flat_map(char::to_lowercase));
            searched.push('\n');
        }
        searched.contains(&query)
    };
    let visible: Vec<&SearchChoice<'_>> = p.choices.iter().filter(|choice| query.is_empty() || hit(choice)).collect();
    // Two or more groups fold under their headers: open by default are the current value's group,
    // `open_group`, and every group once something is typed; a fold made by hand stays until the
    // popup closes.
    let grouped = search_groups(p.choices).iter().filter(|(group, _)| group.is_some()).count() >= 2;
    let folds: SearchFolds = ui.data(|data| data.get_temp(folds_id)).unwrap_or_default();
    let current_group = p.choices.iter().find(|c| c.value == p.current_value).and_then(|c| c.group).filter(|g| !g.is_empty());
    let is_open = |group: &str| {
        folds.get(group).copied().unwrap_or(!query.is_empty() || Some(group) == p.open_group || Some(group) == current_group)
    };
    let rows = search_rows(&visible, grouped, &is_open);
    let shown: Vec<&SearchChoice<'_>> = rows.iter().filter_map(|row| if let SearchRow::Choice(c) = row { Some(*c) } else { None }).collect();
    let custom = (p.custom && !typed.is_empty() && !p.choices.iter().any(|c| c.value == typed)).then_some(typed);
    let mut picked = None;
    let mut toggled: Option<(String, bool)> = None;
    if search.lost_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        let found = visible.iter().find(|c| c.value == typed).or_else(|| shown.first());
        picked = match (typed.is_empty(), found, custom) {
            // Nothing asked for: the choice stands.
            (true, ..) => {
                ui.close();
                None
            }
            (_, Some(choice), _) => Some(Picked::Listed(choice.value.to_owned())),
            (_, None, Some(model)) => Some(Picked::Custom(model.to_owned())),
            (_, None, None) => {
                search.request_focus();
                None
            }
        };
    }
    let width = ui.available_width();
    let note = p.note.map(|note| {
        let shown = compact(note, SEARCH_NOTE_SHOWN);
        ui.painter().layout(shown, egui::FontId::proportional(t.font_small), t.text_dim, width)
    });
    let gap = ui.spacing().item_spacing.y;
    let note_height = note.as_ref().map_or(0.0, |note| note.size().y + gap);
    let row_height = row_height();
    let most = ui.ctx().content_rect().height() - t.space_24 * 4.0;
    // A popup's Ui stays within its size of the last frame, so it can't grow once it is open. All
    // that can change below the box shares a height fixed when it opens, the list taking the rest.
    let height = ui.data_mut(|data| {
        *data.get_temp_mut_or_insert_with(state.with("height"), || {
            let rows = p.choices.len() + search_groups(p.choices).iter().filter(|(group, _)| group.is_some()).count();
            // No rows yet: they may still be on their way, and this can't grow to take them.
            let rows = if rows == 0 { SEARCH_ROWS_AWAITED } else { rows };
            let heading = t.space_16 + gap;
            let custom = if p.custom { heading + row_height + gap } else { 0.0 };
            let list = (rows as f32 * (row_height + gap)).clamp(custom + row_height + gap, t.space_24 * 10.0);
            (heading + list + note_height).min(most).max(heading + custom + row_height + gap)
        })
    });
    ui.allocate_ui(vec2(dimension(width), height), |ui| {
        ui.set_min_height(height);
        if let Some(model) = custom {
            search_heading(ui, crate::i18n::tr("ui.agent_chat_custom_heading"), t.space_16);
            let text = crate::i18n::format("ui.agent_chat_model_custom", &[("model", &compact(model, SEARCH_LABEL_CAP))]);
            let row = search_row(ui, &text, false);
            scenario::publish_target(&format!("{}:custom", p.handles), row.rect);
            if row.clicked() {
                picked = Some(Picked::Custom(model.to_owned()));
            }
        }
        if !visible.is_empty() {
            search_heading(ui, crate::i18n::tr("ui.agent_chat_models_heading"), t.space_16);
            // Rows of one height, group headings too: only those in view are laid out.
            let list = (ui.available_height() - note_height).max(row_height);
            egui::ScrollArea::vertical()
                .id_salt(state.with("list"))
                .max_height(list)
                .min_scrolled_height(list)
                .auto_shrink([false, false])
                .show_rows(ui, row_height, rows.len(), |ui, range| {
                    for row in rows.get(range).unwrap_or_default() {
                        match row {
                            SearchRow::Heading(group) => search_heading(ui, group, row_height),
                            SearchRow::Group { name, count, open } => {
                                let response = search_group_row(ui, name, *count, *open);
                                scenario::publish_target(&format!("{}:group:{}", p.handles, name), response.rect);
                                if response.clicked() {
                                    toggled = Some(((*name).to_owned(), !*open));
                                }
                            }
                            SearchRow::Choice(choice) => {
                                let response = search_row(ui, &compact(choice.label, SEARCH_LABEL_CAP), choice.value == p.current_value);
                                scenario::publish_target(&format!("{}:{}", p.handles, choice.value), response.rect);
                                let response =
                                    if choice.tip.is_empty() { response } else { response.on_hover_text(compact(choice.tip, SEARCH_NOTE_CAP)) };
                                if response.clicked() {
                                    picked = Some(Picked::Listed(choice.value.to_owned()));
                                }
                            }
                        }
                    }
                });
        }
        if let Some((group, open)) = toggled {
            let mut folds = folds;
            folds.insert(group, open);
            ui.data_mut(|data| data.insert_temp(folds_id, folds));
            // The click took the focus from the box; typing goes on filtering.
            search.request_focus();
        }
        if let Some(note) = note {
            let response = ui.add(egui::Label::new(note));
            if let Some(whole) = p.note.filter(|whole| whole.chars().count() > SEARCH_NOTE_SHOWN) {
                response.on_hover_text(compact(whole, SEARCH_NOTE_CAP));
            }
        }
    });
    if picked.is_some() {
        ui.close();
    }
    picked
}

enum SearchRow<'a> {
    /// A lone group's dim heading.
    Heading(&'a str),
    /// One of several groups: its header, with how many of its choices are listed, folded or open.
    Group { name: &'a str, count: usize, open: bool },
    Choice(&'a SearchChoice<'a>),
}

/// The folds made by hand in one opening of a `search_picker`: group name to open.
type SearchFolds = std::collections::HashMap<String, bool>;

/// The groups among `choices` in order of first appearance, each with its choices; `None` is the
/// choices with no group.
fn search_groups<'a, 'c, I>(choices: I) -> Vec<(Option<&'a str>, Vec<&'c SearchChoice<'a>>)>
where
    'a: 'c,
    I: IntoIterator<Item = &'c SearchChoice<'a>>,
{
    let mut groups: Vec<(Option<&'a str>, Vec<&'c SearchChoice<'a>>)> = Vec::new();
    for choice in choices {
        let group = choice.group.filter(|g| !g.is_empty());
        match groups.iter_mut().find(|(name, _)| *name == group) {
            Some((_, members)) => members.push(choice),
            None => groups.push((group, vec![choice])),
        }
    }
    groups
}

/// The list's rows: each group's choices under its header, which folds them when there are several
/// groups and is a plain heading when there is one; choices with no group as they are.
fn search_rows<'a>(visible: &[&'a SearchChoice<'a>], grouped: bool, is_open: &dyn Fn(&str) -> bool) -> Vec<SearchRow<'a>> {
    let mut rows = Vec::with_capacity(visible.len());
    for (group, members) in search_groups(visible.iter().copied()) {
        match group {
            Some(name) if grouped => {
                let open = is_open(name);
                rows.push(SearchRow::Group { name, count: members.len(), open });
                if open {
                    rows.extend(members.into_iter().map(SearchRow::Choice));
                }
            }
            Some(name) => {
                rows.push(SearchRow::Heading(name));
                rows.extend(members.into_iter().map(SearchRow::Choice));
            }
            None => rows.extend(members.into_iter().map(SearchRow::Choice)),
        }
    }
    rows
}

/// The header of one of several groups in a `search_picker`: a chevron, the name, how many of its
/// choices are listed, and a rule. A click folds or opens the group.
fn search_group_row(ui: &mut Ui, name: &str, count: usize, open: bool) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, t.radius_sm, t.hover);
    }
    let y = rect.center().y;
    super::disclosure(painter, egui::pos2(rect.left() + t.form_inset + t.space_8 * 0.5, y), open, t.text_dim);
    let small = egui::FontId::proportional(t.font_small);
    let title = painter.layout_no_wrap(compact(&name.to_uppercase(), SEARCH_LABEL_CAP), small.clone(), t.text);
    let title_pos = egui::pos2(rect.left() + t.form_inset + t.space_16, y - title.size().y / 2.0);
    let title_width = title.size().x;
    painter.galley(title_pos, title, t.text);
    let number = painter.layout_no_wrap(count.to_string(), small, t.text_dim);
    let pad = t.space_8 * 0.75;
    let badge_height = (number.size().y + pad).min(rect.height());
    let badge = egui::Rect::from_min_size(
        egui::pos2(title_pos.x + title_width + t.space_8, y - badge_height / 2.0),
        vec2(number.size().x + pad * 2.0, badge_height),
    );
    painter.rect_filled(badge, t.radius_sm, t.chip);
    painter.galley(egui::pos2(badge.left() + pad, y - number.size().y / 2.0), number, t.text_dim);
    let (rule_from, rule_to) = (badge.right() + t.space_8, rect.right() - t.form_inset);
    if rule_from < rule_to {
        painter.hline(rule_from..=rule_to, y, egui::Stroke::new(1.0, t.border));
    }
    response.on_hover_text(crate::i18n::format("ui.agent_chat_provider_models", &[("count", &count.to_string())]))
}

/// A `search_picker` row: as `list_picker`, but read from the left, as a list of ids is.
fn search_row(ui: &mut Ui, text: &str, selected: bool) -> Response {
    ui.add_sized(vec2(dimension(ui.available_width()), row_height()), egui::Button::selectable(selected, (text, egui::Atom::grow())))
}

/// A dim one-line heading in a `search_picker`'s popup.
fn search_heading(ui: &mut Ui, text: &str, height: f32) {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), height), egui::Sense::hover());
    paint_text(
        ui,
        rect.shrink2(vec2(t.form_inset, 0.0)),
        &compact(text, SEARCH_LABEL_CAP),
        egui::FontId::proportional(t.font_small),
        t.text_dim,
        false,
    );
    if text.chars().count() > SEARCH_LABEL_CAP {
        response.on_hover_text(compact(text, SEARCH_NOTE_CAP));
    }
}

/// A `search_picker` popup's width: the control's, wide enough for the longest label and to search
/// in, and never wider than the window.
fn search_popup_width(ui: &Ui, p: &SearchPicker<'_>) -> f32 {
    let t = Tokens::get();
    let longest = p.choices.iter().map(|c| c.label).max_by_key(|label| label.chars().take(SEARCH_LABEL_CAP + 1).count()).unwrap_or("");
    let text = ui.painter().layout_no_wrap(compact(longest, SEARCH_LABEL_CAP), egui::TextStyle::Button.resolve(ui.style()), t.text).size().x;
    let wanted = text + ui.spacing().button_padding.x * 2.0 + ui.spacing().scroll.bar_width + t.space_8;
    let most = (ui.ctx().content_rect().width() - t.space_24 * 2.0).max(0.0);
    dimension(wanted.max(ui.available_width()).max(t.form_card_width).min(most))
}

/// `text` cut to `max_chars`, with an ellipsis if anything was left out.
fn compact(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() { format!("{short}…") } else { short }
}

/// Completion/focus implementation shared by field variants. Tokens: `same as field`.
fn editor(ui: &mut Ui, value: &mut String, hint: &str, completion: bool) -> (bool, bool, Response) {
    editor_reserving(ui, value, hint, completion, 0.0)
}

/// `editor` whose text stops `reserve` points short of its right edge, for controls drawn there.
fn editor_reserving(ui: &mut Ui, value: &mut String, hint: &str, completion: bool, reserve: f32) -> (bool, bool, Response) {
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let font = egui::FontId::monospace(t.font_label);
    let mut edit = text_input(egui::TextEdit::singleline(value)).hint_text(hint).font(font).background_color(t.input_fill()).clip_text(true);
    if reserve > 0.0 {
        let inset = t.form_inset.round() as i8;
        edit = edit.margin(egui::Margin { left: inset, right: inset.saturating_add(reserve.round() as i8), top: 0, bottom: 0 });
    }
    let mut response = ui.add_sized(vec2(width, row_height()), edit);
    if response.has_focus() {
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("sc_focused_editor"), response.id));
    }
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE);
    if completion && ui.is_enabled() {
        ui.ctx().data_mut(|data| {
            let id = egui::Id::new("sc_value_fields");
            if data.get_temp::<Vec<(egui::Id, usize)>>(id).is_some() {
                data.get_temp_mut_or_default::<Vec<(egui::Id, usize)>>(id).push((response.id, value.chars().count()));
            }
        });
    }
    crate::theme::TabAccent::number_focus(ui, &response);
    t.input_bevel(ui, &response);
    let label = crate::i18n::tr_label(label(ui));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), label);
        info.current_text_value = Some(value.clone());
        info
    });
    super::layout::publish_geometry(ui, &response);
    // A completion's Enter accepts the suggestion, rather than submitting the dialog.
    let completed = completion && crate::params_dialog::complete(ui, &response, value);
    (!completed && response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)), response.changed(), response)
}

/// The one dropdown arrow every kit dropdown draws (the kit chevron, dimmed), instead of egui's triangle.
fn dropdown_arrow(ui: &Ui, rect: egui::Rect, _visuals: &egui::style::WidgetVisuals, _open: bool) {
    super::dropdown_indicator(ui.painter(), rect, Tokens::get().text_dim);
}

/// Native dropdown with kit arrow and scenario option handles. Tokens: `text_dim; scope control/font tokens`.
pub(crate) fn dropdown(ui: &mut Ui, id: &str, selected: &mut usize, options: &[&str]) -> Response {
    let text = options.get(*selected).copied().map(crate::i18n::tr_label).unwrap_or("");
    let row = label(ui).to_string();
    dropdown_menu(ui, id, text, |ui| {
        for (index, option) in options.iter().enumerate() {
            let response = ui.selectable_value(selected, index, crate::i18n::tr_label(option));
            scenario::publish_target(&format!("dialog:{row}:{option}"), response.rect);
        }
    })
}

/// [`dropdown`] whose options each show a picture beside them while hovered, painted by `preview`
/// for that option's index, so a choice can be seen before it is picked. Tokens: dropdown tokens,
/// the tooltip frame.
pub(crate) fn dropdown_previews(ui: &mut Ui, id: &str, selected: &mut usize, options: &[&str], preview: impl Fn(&mut Ui, usize)) -> Response {
    let text = options.get(*selected).copied().map(crate::i18n::tr_label).unwrap_or("");
    let row = label(ui).to_string();
    dropdown_menu(ui, id, text, |ui| {
        for (index, option) in options.iter().enumerate() {
            let response = ui.selectable_value(selected, index, crate::i18n::tr_label(option));
            scenario::publish_target(&format!("dialog:{row}:{option}"), response.rect);
            Container::tooltip_ui(&response, |ui| {
                preview(ui, index);
                scenario::publish_rect(&format!("dialog:preview:{row}:{option}"), ui.min_rect());
            });
        }
    })
}

/// [`dropdown`] that also says whether an option was clicked, the one already shown included (a
/// pick of the shown option changes nothing, yet it is a choice).
pub(crate) fn dropdown_picked(ui: &mut Ui, id: &str, selected: &mut usize, options: &[&str]) -> bool {
    let text = options.get(*selected).copied().map(crate::i18n::tr_label).unwrap_or("");
    let row = label(ui).to_string();
    let mut picked = false;
    dropdown_menu(ui, id, text, |ui| {
        for (index, option) in options.iter().enumerate() {
            let response = ui.selectable_value(selected, index, crate::i18n::tr_label(option));
            picked |= response.clicked();
            scenario::publish_handle(&format!("dialog:{row}:{option}"), response.rect.center());
        }
    });
    picked
}

/// Dropdown with caller-supplied kit choices/actions, including disabled entries and stable
/// application handles. Tokens: dropdown tokens. Use `dropdown` for a simple indexed list.
pub(crate) fn dropdown_menu(ui: &mut Ui, id: impl egui::AsIdSalt, current: &str, add: impl FnOnce(&mut Ui)) -> Response {
    dropdown_menu_with_close_behavior(ui, id, current, egui::PopupCloseBehavior::CloseOnClick, add)
}

/// Dropdown with explicit dismissal behavior. For editable content use `CloseOnClickOutside`
/// and close the popup when a choice is accepted.
pub(crate) fn dropdown_menu_with_close_behavior(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    current: &str,
    close_behavior: egui::PopupCloseBehavior,
    add: impl FnOnce(&mut Ui),
) -> Response {
    dropdown_menu_width(ui, id, current, false, close_behavior, add)
}

/// Intrinsic-width variant for horizontal toolbars and bottom strips.
pub(crate) fn compact_dropdown_menu(ui: &mut Ui, id: impl egui::AsIdSalt, current: &str, add: impl FnOnce(&mut Ui)) -> Response {
    dropdown_menu_width(ui, id, current, true, egui::PopupCloseBehavior::CloseOnClick, add)
}

fn dropdown_menu_width(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    current: &str,
    compact: bool,
    close_behavior: egui::PopupCloseBehavior,
    add: impl FnOnce(&mut Ui),
) -> Response {
    crate::prefs_search::note_current(ui, current);
    // Only a search probe may skip the real control. A dialog's first frame is not visible yet,
    // and a stand-in there makes the next frame too short to show OK.
    if crate::prefs_search::querying(ui)
        && (!ui.is_visible() || (crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[current])))
    {
        return crate::prefs_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let width = if compact {
        ui.painter().layout_no_wrap(current.to_owned(), egui::FontId::proportional(t.font_label), t.form_ink).size().x + t.space_24 + t.space_8
    } else {
        dimension(ui.available_width())
    };
    let response = egui::ComboBox::from_id_salt(id)
        .width(width)
        .height(Tokens::get().space_24 * 14.0)
        .selected_text(current)
        .icon(dropdown_arrow)
        .close_behavior(close_behavior)
        .show_ui(ui, add)
        .response;
    publish(ui, &response, egui::WidgetType::ComboBox);
    response
}

/// Selection target state; filled targets retain their reserved trailing space.
pub(crate) struct Selection {
    pub active: bool,
    pub filled: bool,
}

/// Three looks: the active input in accent, a filled one in a quiet neutral tint, an empty one plain.
pub(crate) fn selection_field(ui: &mut Ui, text: &str, options: Selection) -> Response {
    let Selection { active, filled } = options;
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::CLICK);
    // A separate action fill lets two-colour palettes reserve the focus accent for inverted ink.
    let inverted = t.form_primary_fill != t.form_primary;
    if active && !filled && !inverted {
        ui.painter().rect_filled(rect, t.control_radius, t.selected_bg);
    }
    let (fill, border) = match (active, filled) {
        (true, _) if inverted => (t.selected_bg, t.form_primary),
        (false, _) if inverted => (t.control_fill, t.control_border),
        (true, true) => (t.selected_bg, t.form_primary),
        (true, false) => (t.control_fill, t.form_primary),
        (false, true) => (t.control_fill.lerp_to_gamma(t.form_ink, 0.06), t.control_border.lerp_to_gamma(t.form_ink, 0.35)),
        (false, false) => (t.control_fill, t.control_border),
    };
    ui.painter().rect(
        if active && !filled && !inverted { rect.shrink(t.space_2) } else { rect },
        t.control_radius,
        fill,
        match (active, filled) {
            (true, _) => egui::Stroke::new(1.0, border),
            // A theme's control border can fix its own colour; a filled field keeps its tint.
            (false, true) => egui::Stroke::new(t.control_border_stroke(border).width, border),
            (false, false) => t.control_border_stroke(border),
        },
        egui::StrokeKind::Inside,
    );
    let end = if filled { t.space_24 } else { t.space_8 };
    let text_rect = egui::Rect::from_min_max(
        rect.min + vec2(t.form_inset.min(rect.width()), 0.0),
        egui::pos2((rect.right() - end).max(rect.left() + t.form_inset.min(rect.width())), rect.bottom()),
    );
    paint_text(
        ui,
        text_rect,
        text,
        crate::fonts::medium(t.font_label),
        match (active, filled) {
            (true, _) if inverted => t.selected_ink,
            (true, true) => t.selected_ink,
            (false, true) => t.form_ink,
            _ => t.form_dim,
        },
        false,
    );
    publish(ui, &response, egui::WidgetType::Button);
    scenario::publish_rect(&format!("dialog:field:{}", label(ui)), rect);
    scenario::publish_count(&format!("dialog:selection:{}:filled", label(ui)), f64::from(filled));
    scenario::publish_count(&format!("dialog:selection:{}:active", label(ui)), f64::from(active));
    response
}

/// Paired values with compact captions and optional independent scenario/accessibility names.
pub(crate) fn value_pair(ui: &mut Ui, label: &str, values: [&mut String; 2], axes: [&str; 2], names: Option<[&str; 2]>) -> bool {
    let names = names.unwrap_or(axes);
    let t = Tokens::get();
    super::row(ui, label, |ui| {
        let width = ((dimension(ui.available_width()) - t.space_8) / 2.0).max(0.0);
        let mut enter = false;
        ui.spacing_mut().item_spacing.x = t.space_8;
        for ((axis, value), name) in axes.into_iter().zip(values).zip(names) {
            ui.allocate_ui_with_layout(vec2(width, row_height()), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                ui.label(RichText::new(axis).size(t.font_label).color(t.dialog_label));
                enter |= super::layout::named_control(ui, name, |ui| field(ui, value));
            });
        }
        enter
    })
}

/// Paired bounded numeric fields in the shared value-pair row geometry.
pub(crate) fn number_pair<T: egui::emath::Numeric>(
    ui: &mut Ui,
    label: &str,
    values: [&mut T; 2],
    range: std::ops::RangeInclusive<T>,
    speed: f64,
    suffix: &str,
) -> [Response; 2] {
    let t = Tokens::get();
    super::row(ui, label, |ui| {
        let width = ((dimension(ui.available_width()) - t.space_8) / 2.0).max(0.0);
        ui.spacing_mut().item_spacing.x = t.space_8;
        values.map(|value| {
            ui.allocate_ui_with_layout(vec2(width, row_height()), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                number_field(ui, value, range.clone(), speed, suffix)
            })
            .inner
        })
    })
}

/// Variable-radius location editor. Tokens: scope's control and typography tokens.
/// Retains the established 0.01 step, fractional range and locale parser.
/// Locale-aware variable-radius position editor. Tokens: `scope control/font tokens`.
pub(crate) fn fraction_field(ui: &mut Ui, value: &mut f64) -> Response {
    let response = ui.add(egui::DragValue::new(value).speed(0.01).range(0.01..=0.99).fixed_decimals(2).custom_parser(crate::prefs::parse_number));
    scenario::publish_target(&format!("dialog:fraction:{}", label(ui)), response.rect);
    response
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;

/// Stable-ID inline editor for tables. Completion is owned by the caller so Enter can commit
/// a cell without submitting its panel. Tokens: control_height/fill, form_inset, font_label, text/error.
pub fn cell_editor(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, invalid: bool) -> Response {
    bounded_cell_editor(ui, id, value, hint, invalid, usize::MAX)
}

/// Stable-ID cell/name editor with a caller-supplied character cap. Tokens: cell_editor tokens.
pub(crate) fn bounded_cell_editor(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, invalid: bool, limit: usize) -> Response {
    cell_editor_frame(ui, id, value, hint, invalid, limit, true)
}

fn cell_editor_frame(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, invalid: bool, limit: usize, frame: bool) -> Response {
    let t = Tokens::get();
    let editor = text_input(egui::TextEdit::singleline(value))
        .char_limit(limit)
        .id(id)
        .hint_text(crate::i18n::tr_label(hint))
        .font(if frame { egui::FontId::monospace(t.font_label) } else { egui::FontId::proportional(t.font_label) })
        .background_color(if invalid { t.form_error_fill } else { t.input_fill() })
        .text_color(if invalid { t.form_error } else { t.form_ink })
        .clip_text(true);
    let editor = if invalid {
        editor.frame(
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(t.form_inset as i8, 0))
                .fill(t.form_error_fill)
                .stroke(egui::Stroke::new(1.0, t.form_error))
                .corner_radius(t.control_radius),
        )
    } else if frame {
        editor
    } else {
        editor.frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(t.space_8 as i8, 0)))
    };
    let mut response = ui.add_sized(vec2(dimension(ui.available_width()), row_height()), editor);
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE.limited(limit));
    if frame {
        publish(ui, &response, egui::WidgetType::TextEdit);
    } else {
        super::layout::publish_geometry(ui, &response);
    }
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), crate::i18n::tr_label(label(ui)));
        info.current_text_value = Some(value.clone());
        info
    });
    crate::theme::TabAccent::number_focus(ui, &response);
    t.input_bevel(ui, &response);
    response
}

/// Table cell editor that opens wide while it has the focus and its text no longer fits
/// (#9358): over the cells to its right up to `right`, wrapping onto as many lines as the text
/// needs, with `note` (the live value, or an error when its flag is set) under it. Otherwise,
/// and when there is no room to its right, it is [`cell_editor`]. Enter does not break the
/// line: the caller commits on it as for the cell. Tokens: cell_editor tokens, popup frame.
pub fn wide_cell_editor(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, invalid: bool, right: f32, note: (&str, bool)) -> Response {
    let t = Tokens::get();
    let cell = egui::Rect::from_min_size(ui.cursor().min, vec2(dimension(ui.available_width()), row_height()));
    let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
    let font = egui::FontId::monospace(t.font_label);
    let text_width = ui.fonts_mut(|f| f.layout_no_wrap(value.clone(), font.clone(), t.form_ink)).size().x;
    let fits = text_width + 2.0 * t.form_inset <= cell.width();
    if !focused || fits || right <= cell.right() || !ui.is_rect_visible(cell) {
        return cell_editor(ui, id, value, hint, invalid);
    }
    // The cell keeps its place in the row; the editor itself floats over it.
    let (rect, _) = ui.allocate_exact_size(cell.size(), egui::Sense::hover());
    ui.painter().rect_filled(rect, t.control_radius, if invalid { t.form_error_fill } else { t.input_fill() });
    let width = right - rect.left();
    let (ctx, enabled) = (ui.ctx().clone(), ui.is_enabled());
    // The border sits outside the cell, so the text stays where it was.
    let border = t.primary_border(t.dialog_border);
    let area = Container::new(&ctx, Surface::Popup)
        .area(id.with("wide"))
        .fixed_pos(rect.min - vec2(border.width, border.width))
        .constrain(true)
        .order(egui::Order::Foreground);
    let shown = area.show(&ctx, |ui| {
        Container::popup_frame(ui.style())
            .fill(t.control_fill)
            .stroke(border)
            .corner_radius(t.control_radius)
            .inner_margin(0.0)
            .show(ui, |ui| {
                ui.set_width(width);
                let editor = text_input(egui::TextEdit::multiline(value))
                    .id(id)
                    .return_key(None)
                    .desired_rows(1)
                    .desired_width(width)
                    .hint_text(crate::i18n::tr_label(hint))
                    .font(font)
                    .background_color(if invalid { t.form_error_fill } else { t.input_fill() })
                    .text_color(if invalid { t.form_error } else { t.form_ink });
                let mut response = ui.add(editor);
                // The area's first frame is a sizing pass that disables its widgets, and egui
                // takes the focus from a disabled one: hand it straight back.
                if ui.is_sizing_pass() {
                    response.request_focus();
                }
                // It wraps but stays one line: Enter ends the edit as in a single-line field.
                // Raw events, as the table reads them: an accepted completion has already taken its Enter.
                let enter = ui.input(|i| {
                    i.raw.events.iter().any(|e| {
                        matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. } if *modifiers == egui::Modifiers::NONE)
                    })
                });
                if response.has_focus() && enter {
                    response.surrender_focus();
                }
                super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE);
                publish(ui, &response, egui::WidgetType::TextEdit);
                crate::theme::TabAccent::number_focus(ui, &response);
                t.input_bevel(ui, &response);
                let (text, error) = note;
                if !text.is_empty() {
                    ui.add_space(t.space_2);
                    ui.horizontal(|ui| {
                        ui.add_space(t.form_inset);
                        ui.add(egui::Label::new(RichText::new(text).size(t.font_small).color(if error { t.form_error } else { t.form_dim })).wrap());
                    });
                    ui.add_space(t.space_2);
                }
                response
            })
            .inner
    });
    let response = shown.inner;
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, crate::i18n::tr_label(hint));
        info.current_text_value = Some(value.clone());
        info
    });
    response
}

/// Search input with native accessibility and a stable caller ID. Tokens: cell_editor tokens.
pub(crate) fn search_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    if !super::layout::in_catalog(ui) {
        return cell_editor(ui, id, value, hint, false);
    }
    let t = Tokens::get();
    let editor = text_input(egui::TextEdit::singleline(value))
        .id(id)
        .char_limit(128)
        .hint_text(hint)
        .min_size(vec2(0.0, t.space_16 + t.space_2))
        .margin(egui::Margin { left: t.space_24 as i8, right: t.space_8 as i8, top: 0, bottom: 0 })
        .font(egui::FontId::proportional(t.font_small))
        .text_color(t.form_ink);
    let mut response = ui.add_sized(vec2(dimension(ui.available_width()), t.space_16 + t.space_2), editor);
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE.limited(128));
    let icon = egui::Rect::from_center_size(egui::pos2(response.rect.left() + t.space_12, response.rect.center().y), vec2(t.space_12, t.space_12));
    crate::icons::paint_ink(ui.painter(), icon, "zoom", t.form_dim);
    super::layout::publish_geometry(ui, &response);
    response
}

/// Compact icon action opening the shared searchable menu.
pub(crate) fn searchable_icon_dropdown(
    ui: &mut Ui,
    icon: &'static str,
    label: &str,
    filter: &mut String,
    options: &[SearchOption<'_>],
) -> Option<String> {
    searchable_dropdown_with(ui, "icon_search_menu", label, filter, options, SearchMenu { custom: None, pending: None, icon: Some(icon) })
}

/// The filter at the top of a long menu: a search glyph, a frameless editor and a rule under
/// them, so it reads as the menu's search box and not as one more entry. Tokens: `space_8/16,
/// font_label, form_dim, form_ink, dialog_rule`.
pub(crate) fn menu_filter(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::hover());
    let glyph = egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_8 + t.space_16 / 2.0, rect.center().y), vec2(t.space_16, t.space_16));
    crate::icons::paint_ink(ui.painter(), glyph, "zoom", t.form_dim);
    let field = egui::Rect::from_min_max(egui::pos2((glyph.right() + t.space_8).min(rect.right()), rect.top()), rect.max);
    let hint_text = crate::i18n::tr_label(hint);
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(field));
            ui.add_sized(
                field.size(),
                text_input(egui::TextEdit::singleline(value))
                    .id(id)
                    .char_limit(256)
                    .hint_text(hint_text)
                    .font(egui::FontId::proportional(t.font_label))
                    .text_color(t.form_ink)
                    .frame(egui::Frame::NONE)
                    .clip_text(true),
            )
        })
        .inner;
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), hint_text);
        info.current_text_value = Some(value.clone());
        info
    });
    super::layout::publish_geometry(ui, &response);
    if ui.rect_contains_pointer(rect) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    super::Menu::separator(ui);
    response
}

/// Behaviour of the one kit slider. Appearance is always owned by `slider`.
/// Values/ranges stay in caller units; formatting never rounds saved values during drawing.
pub(crate) struct Slider {
    pub step: Option<f64>,
    pub format: fn(f64) -> String,
    pub show_value: bool,
    pub integer: bool,
    pub workspace_accent: bool,
}

/// Bounded read-only code box for diagnostic payloads. Native selection and copying
/// remain available; the caller's source cannot be edited. Uses shared form styling.
pub(crate) fn read_only_text(ui: &mut Ui, text: &str) -> Response {
    let mut source = text;
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            let mut response = ui.add(egui::TextEdit::multiline(&mut source).code_editor().desired_width(ui.available_width()).desired_rows(8));
            super::text_menu::menu(ui, &mut response, &mut source, super::text_menu::Field::LINES);
            response
        })
        .inner
}

impl Default for Slider {
    fn default() -> Self {
        Self { step: None, format: |v| format!("{v:.2}"), show_value: true, integer: false, workspace_accent: false }
    }
}

/// The only slider element, including stepped preferences and editor-free timeline controls.
/// Native input, keyboard routing, enabled state and accessibility; one token-painted track/thumb.
/// Tokens: control_height, space_24/8/4/2, dialog_body/rule, control_fill, form_primary.
pub(crate) fn slider<T: egui::emath::Numeric>(ui: &mut Ui, value: &mut T, range: std::ops::RangeInclusive<T>, options: Slider) -> Response {
    if crate::prefs_search::suppress_chrome(ui) {
        return crate::prefs_search::hidden_response(ui);
    }
    // Own form styling even in callers such as the viewport menu which have no kit scope.
    super::scope(ui, |ui| {
        let t = Tokens::get();
        ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
        let primary = if options.workspace_accent { crate::theme::TabAccent::current(ui.ctx()).ink } else { t.form_primary };
        let limits = (range.start().to_f64(), range.end().to_f64());
        let reserved = if options.show_value {
            // Reserve the actual formatted editor, including suffixes and localized Instant.
            // Endpoints keep its width stable as a preference is dragged through its range.
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let text_width = [limits.0, limits.1, value.to_f64()]
                .into_iter()
                .map(|v| ui.painter().layout_no_wrap((options.format)(v), font.clone(), t.form_ink).size().x.ceil())
                .fold(0.0, f32::max);
            let editor_width = (text_width + ui.spacing().button_padding.x * 2.0).max(ui.spacing().interact_size.x);
            (editor_width + ui.spacing().item_spacing.x).max(t.space_24 * 2.0 + t.space_4)
        } else {
            t.space_4
        };
        ui.spacing_mut().slider_width = dimension(ui.available_width() - reserved);
        let mut widget = egui::Slider::new(value, range).show_value(options.show_value).handle_shape(egui::style::HandleShape::Circle);
        if options.show_value {
            widget = widget.custom_formatter(move |v, _| (options.format)(v));
        }
        if let Some(step) = options.step {
            widget = widget.step_by(step);
        }
        if options.integer {
            widget = widget.integer();
        }
        let response = ui.add(widget);
        if ui.ctx().memory(|memory| memory.focused()).is_some_and(|id| {
            ui.ctx().read_response(id).is_some_and(|focused| response.rect.contains(focused.rect.center()))
                || egui::TextEdit::load_state(ui.ctx(), id).is_some()
        }) {
            crate::prefs_search::note_key_sink(ui.ctx());
        }
        let slider = egui::Rect::from_min_size(response.rect.min, vec2(ui.spacing().slider_width, response.rect.height()));
        let mut background = ui.painter().clone();
        background.set_opacity(if ui.is_visible() { 1.0 } else { 0.0 });
        // At the top of the range egui's own handle reaches `slider.right()` exactly, and a fill
        // that stops there leaves its last column showing beside the editor (#5708). The gap
        // before the editor is `item_spacing.x`, so a pixel past the edge covers nothing else.
        background.rect_filled(slider.with_max_x(slider.right() + 1.0), 0.0, t.dialog_body);
        // Match egui's native input range (handle_radius = thickness / 2.5), so the
        // painted thumb stays under the pointer even at the two ends of the range.
        let input_radius = ui.text_style_height(&egui::TextStyle::Body).max(ui.spacing().interact_size.y) / 2.5;
        let track = egui::Rect::from_center_size(slider.center(), vec2((slider.width() - input_radius * 2.0).max(0.0), t.space_4));
        let fraction = if limits.1 != limits.0 { ((value.to_f64() - limits.0) / (limits.1 - limits.0)).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let center = egui::pos2(egui::lerp(track.x_range(), fraction), track.center().y);
        ui.painter().rect_filled(track, t.control_radius, t.dialog_rule);
        ui.painter().rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(center.x, track.bottom())), t.control_radius, primary);
        ui.painter().circle(center, t.space_8, t.control_fill, egui::Stroke::new(t.space_2, primary));
        publish(ui, &response, egui::WidgetType::Slider);
        response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), value.to_f64(), crate::i18n::tr_label(label(ui))));
        scenario::publish_rect(&format!("dialog:slider_track:{}", label(ui)), slider);
        scenario::publish_target(&format!("dialog:slider:{}", label(ui)), response.rect);
        response
    })
}

/// Shared slider with a compact mixed-aware value editor at the right.
/// The caller owns commit timing, just as for `mixed_scalar_field`.
pub(crate) fn mixed_slider(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, mixed: Option<&str>) -> Response {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let width = if mixed.is_some() { t.space_24 * 2.0 } else { t.space_24 + t.space_12 };
        let track = ui
            .allocate_ui(vec2((ui.available_width() - width - t.space_8).max(0.0), row_height()), |ui| {
                slider(ui, value, range.clone(), Slider { show_value: false, workspace_accent: true, ..Default::default() })
            })
            .inner;
        let field = ui
            .allocate_ui(vec2(width, row_height()), |ui| {
                ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
                ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(t.font_label));
                scalar_field_impl(ui, value, range, mixed, true)
            })
            .inner;
        field.union(track)
    })
    .inner
}

/// Locale-aware bounded scalar field, used for dimensionless material properties.
pub(crate) fn scalar_field(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>) -> Response {
    mixed_scalar_field(ui, value, range, None)
}

/// A bounded scalar with a translated mixed-value placeholder. Typing or dragging
/// replaces the placeholder; commit ownership remains with the caller.
pub(crate) fn mixed_scalar_field(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, mixed: Option<&str>) -> Response {
    scalar_field_impl(ui, value, range, mixed, false)
}

fn scalar_field_impl(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, mixed: Option<&str>, plain: bool) -> Response {
    ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
    let before = *value;
    let parsed = std::cell::Cell::new(false);
    let field = egui::DragValue::new(value).range(range).speed(0.01).custom_parser(|text| {
        let value = crate::prefs::parse_number(text);
        parsed.set(value.is_some());
        value
    });
    let field = if plain { field.fixed_decimals(2) } else { field };
    let mut response = ui.add_sized(vec2(dimension(ui.available_width()), row_height()), |ui: &mut Ui| {
        let field = if let Some(mixed) = mixed.filter(|_| !ui.ctx().is_being_dragged(ui.next_auto_id())) {
            field.custom_formatter(move |_, _| mixed.to_owned())
        } else {
            field
        };
        ui.add(field)
    });
    // Entering the first target's existing number still sets every mixed target.
    // Merely focusing and leaving the translated placeholder is inert.
    if mixed.is_some() {
        response.flags.set(egui::response::Flags::CHANGED, *value != before || parsed.get());
    }
    if !plain {
        Tokens::get().input_bevel(ui, &response);
    }
    scenario::publish_target(&format!("dialog:scalar:{}", label(ui)), response.rect);
    response
}

/// Length held in millimetres, dragged or typed in the design's units ("0.5 in", "2 cm + 3 mm" parse).
/// Tokens: control_height, scope control/font tokens.
pub(crate) fn length_field(ui: &mut Ui, value: &mut f64, units: &str, decimals: u8) -> Response {
    let response = ui.add(length_drag(value, units, decimals));
    Tokens::get().input_bevel(ui, &response);
    scenario::publish_target(&format!("dialog:length:{}", label(ui)), response.rect);
    response
}

/// A model-space length with tool-specific bounds and drag step, shown in design units.
pub(crate) fn bounded_length_field(
    ui: &mut Ui,
    value: &mut f64,
    units: &str,
    decimals: u8,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
) -> Response {
    let response = ui.add(length_drag(value, units, decimals).range(range).speed(speed));
    scenario::publish_target(&format!("dialog:length:{}", label(ui)), response.rect);
    response
}

/// Angle in degrees, dragged or typed. Tokens: control_height, scope control/font tokens.
pub(crate) fn angle_field(ui: &mut Ui, value: &mut f64) -> Response {
    let response = ui.add(egui::DragValue::new(value).speed(1.0).suffix(" deg").custom_parser(crate::prefs::parse_number));
    Tokens::get().input_bevel(ui, &response);
    scenario::publish_target(&format!("dialog:angle:{}", label(ui)), response.rect);
    response
}

/// Bounded multiline data input. Caller owns validation and keyboard semantics.
/// Tokens: control_height/fill, form_inset, font_label.
pub(crate) fn text_area(ui: &mut Ui, label: &str, id: egui::Id, value: &mut String, rows: usize, limit: usize) -> Response {
    let t = Tokens::get();
    let mut response = ui.add_sized(
        vec2(dimension(ui.available_width()), row_height() * rows.clamp(1, 16) as f32),
        text_input(egui::TextEdit::multiline(value))
            .id(id)
            .desired_rows(rows.clamp(1, 16))
            .char_limit(limit)
            .font(egui::FontId::monospace(t.font_label))
            .background_color(t.input_fill()),
    );
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINES.limited(limit));
    t.input_bevel(ui, &response);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), label);
        info.current_text_value = Some(value.clone());
        info
    });
    response
}

/// Secret input with a stable ID. Never publishes its value in accessibility/scenario metadata.
/// Tokens: cell_editor tokens.
pub(crate) fn password_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    let t = Tokens::get();
    let mut response = ui.add_sized(
        vec2(dimension(ui.available_width()), row_height()),
        text_input(egui::TextEdit::singleline(value))
            .id(id)
            .password(true)
            .hint_text(hint)
            .font(egui::FontId::monospace(t.font_label))
            .background_color(t.input_fill()),
    );
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field { secret: true, ..super::text_menu::Field::LINE });
    publish(ui, &response, egui::WidgetType::TextEdit);
    response
}

/// Bounded integer or decimal preference with its original drag step and unit suffix.
/// Tokens: scope control/font tokens. Values and limits are caller data.
pub(crate) fn number_field<T: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    speed: f64,
    suffix: &str,
) -> Response {
    let response = ui.add(egui::DragValue::new(value).range(range).speed(speed).suffix(suffix).custom_parser(crate::prefs::parse_number));
    if response.has_focus() {
        crate::prefs_search::note_key_sink(ui.ctx());
    }
    Tokens::get().input_bevel(ui, &response);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), value.to_f64());
        info.label = Some(crate::i18n::tr_label(label(ui)).to_string());
        info
    });
    super::layout::publish_geometry(ui, &response);
    response
}

/// Three bounded numeric components sharing the available control-column width.
/// Each field retains the standard parser, bevel and focus behaviour. Tokens:
/// number_field typography/chrome and the enclosing row's spacing.
pub(crate) fn vector3_field(ui: &mut Ui, values: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, suffix: &str) -> bool {
    let mut changed = false;
    let label = super::layout::label(ui).to_owned();
    ui.columns(3, |columns| {
        for (axis, (ui, value)) in columns.iter_mut().zip(values).enumerate() {
            let width = ui.available_width().max(0.0);
            ui.spacing_mut().text_edit_width = width;
            ui.spacing_mut().interact_size.x = width;
            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
            let response = number_field(ui, value, range.clone(), 0.01, suffix);
            scenario::publish_target(&format!("dialog:vector:{label}:{axis}"), response.rect);
            changed |= response.changed();
        }
    });
    changed
}

/// Equal-width numeric cells within a shared `row`. Returns each field's response in order.
/// Tokens: `control_height`, inherited item spacing and number-field tokens.
pub(crate) fn number_fields<T: egui::emath::Numeric>(
    ui: &mut Ui,
    values: &mut [T],
    range: std::ops::RangeInclusive<T>,
    speed: f64,
    suffix: &str,
) -> Vec<Response> {
    let gap = ui.spacing().item_spacing.x;
    let width = dimension(ui.available_width() - gap * values.len().saturating_sub(1) as f32) / values.len().max(1) as f32;
    values
        .iter_mut()
        .enumerate()
        .map(|(index, value)| {
            ui.push_id(index, |ui| {
                ui.allocate_ui_with_layout(vec2(width, row_height()), egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    ui.spacing_mut().text_edit_width = width;
                    ui.spacing_mut().interact_size.x = width;
                    number_field(ui, value, range.clone(), speed, suffix)
                })
                .inner
            })
            .inner
        })
        .collect()
}

/// A number on a range track, without unit: dragged `speed` a point, shown with `decimals`, typed
/// as a sum of numbers (`relative_number`).
/// Tokens: scope control/font tokens. Values are caller data.
pub(crate) fn track_number(ui: &mut Ui, value: &mut f64, speed: f64, decimals: usize) -> Response {
    let now = *value;
    let response = ui.add(egui::DragValue::new(value).speed(speed).fixed_decimals(decimals).custom_parser(move |s| relative_number(s, now)));
    Tokens::get().input_bevel(ui, &response);
    super::layout::publish_geometry(ui, &response);
    response
}

/// A value typed as a sum of numbers (`crate::prefs::parse_number` each), added to `now` when it
/// starts with a sign: "+30" moves the value by 30, "0-5" is −5.
fn relative_number(text: &str, now: f64) -> Option<f64> {
    let text: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == '\u{2212}' { '-' } else { c }).collect();
    let mut total = if text.starts_with(['+', '-']) { now } else { 0.0 };
    let mut term = String::new();
    let mut prev = ' ';
    for c in text.chars() {
        if (c == '+' || c == '-') && !term.is_empty() && !matches!(prev, 'e' | 'E') {
            total += crate::prefs::parse_number(&term)?;
            term.clear();
        }
        term.push(c);
        prev = c;
    }
    total += crate::prefs::parse_number(&term)?;
    total.is_finite().then_some(total)
}

/// Growing plain-text message editor; its parent owns scrolling and Enter routing.
/// Tokens: control_fill, form_inset, font_label. Use text_area for bounded data input.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn message_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    let t = Tokens::get();
    let mut response = ui.add(
        text_input(egui::TextEdit::multiline(value))
            .id(id)
            .hint_text(hint)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .font(egui::FontId::proportional(t.font_label))
            .background_color(t.input_fill()),
    );
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINES);
    t.input_bevel(ui, &response);
    response
}

/// Edit a millimetre value through a control expressed in the current design units.
/// Keep the stored value untouched unless the user actually changes the field.
fn length_drag<'a>(value: &'a mut f64, units: &str, decimals: u8) -> egui::DragValue<'a> {
    let (scale, unit) = crate::prefs::unit_scale(units);
    egui::DragValue::new(value)
        .speed(0.5)
        .suffix(format!(" {unit}"))
        .custom_formatter(move |mm, _| crate::prefs::length_number(mm, unit, decimals))
        .custom_parser(move |text| crate::prefs::parse_length_mm(text, scale))
}

/// Small model-anchored value editor. Retains its focus ID and native hit height so it does
/// not intercept nearby canvas picks; the owner keeps completion and selection handling.
/// With `keep_tab` Tab stays with the field (the caller acts on it) instead of moving the
/// keyboard to the next widget.
pub(crate) fn canvas_value_field(ui: &mut Ui, id: egui::Id, value: &mut String, keep_tab: bool) -> Response {
    let t = Tokens::get();
    let width = grown_width(ui, value, &egui::TextStyle::Body.resolve(ui.style()), t.space_24 * 3.0);
    // At least that wide: in an area the space left is last frame's width, which can't grow.
    let mut edit = egui::TextEdit::singleline(value).id(id).desired_width(width).min_size(vec2(width, 0.0)).background_color(t.input_fill());
    if keep_tab {
        edit = edit.event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, ..Default::default() });
    }
    let mut response = ui.add(edit);
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE);
    t.input_bevel(ui, &response);
    let field = (ui.ctx().cumulative_pass_nr(), response.id, value.chars().count());
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("sc_canvas_value_focus"), field));
    response
}

/// How wide a value field on the canvas is: `base` for a number, as wide as an expression typed
/// into it (parameter names, a formula), up to three times `base`.
fn grown_width(ui: &Ui, value: &str, font: &egui::FontId, base: f32) -> f32 {
    let text = ui.fonts_mut(|fonts| fonts.layout_no_wrap(value.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x);
    // Room for the cursor after the last character.
    (text + Tokens::get().space_6).clamp(base, base * 3.0)
}

/// Placement editor whose owner routes Enter and Tab (including rejected expressions).
/// Completion remains with the caller. Tokens: cell_editor tokens.
pub(crate) fn placement_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_body);
    let size = placement_field_content_size(ui, value);
    let mut response = ui.add(
        text_input(egui::TextEdit::singleline(value))
            .id(id)
            .min_size(size)
            .desired_width(size.x)
            .return_key(None)
            .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, ..Default::default() })
            .font(font)
            .background_color(t.input_fill())
            .text_color(t.form_ink)
            .clip_text(true),
    );
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE);
    publish(ui, &response, egui::WidgetType::TextEdit);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), crate::i18n::tr_label(hint));
        info.current_text_value = Some(value.clone());
        info
    });
    crate::theme::TabAccent::number_focus(ui, &response);
    t.input_bevel(ui, &response);
    response.on_hover_text(crate::i18n::tr_label(hint))
}

/// Attached Library dropdown and text editor inside one bordered control.
/// The caller retains draft, completion and commit ownership.
pub(crate) fn library_cell(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, menu: impl FnOnce(&mut Ui)) -> Response {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::hover());
    ui.painter().rect(rect, t.control_radius, t.control_fill, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    let split = (rect.right() - t.space_24).max(rect.left());
    let field = egui::Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom()));
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(field));
            cell_editor_frame(ui, id, value, hint, false, usize::MAX, false)
        })
        .inner;
    let button = egui::Rect::from_min_max(egui::pos2(split, rect.top()), rect.max);
    t.secondary_vline(ui.painter(), split, rect.y_range(), t.dialog_rule);
    let library = super::icon_button_at(ui, button, "chevron", "Library");
    scenario::publish_handle("dialog:menu:Library", button.center());
    scenario::publish_rect("dialog:menu:Library", button);
    let popup = Container::new(ui.ctx(), Surface::Popup).menu(&library);
    let open = popup.is_open();
    popup.show(|ui| super::scope(ui, menu));
    if open {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
    if value.is_empty() { response.on_hover_text(crate::i18n::tr_label(hint)) } else { response }
}

/// Inline tree-name editor in caller-owned geometry; preserves its native focus and selection ID.
/// Tokens: text. The caller retains commit/cancel and select-all behavior.
pub(crate) fn inline_name(ui: &mut Ui, rect: egui::Rect, id: egui::Id, value: &mut String) -> Response {
    let mut response = ui.put(
        rect,
        egui::TextEdit::singleline(value).id(id).frame(egui::Frame::NONE).text_color(Tokens::get().text).vertical_align(egui::Align::Center),
    );
    super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE);
    response
}

/// The placement editor's content size, before its horizontal text margins and outer frame.
/// Floating owners use the same measurement to place adjacent editors without overlap.
pub(crate) fn placement_field_content_size(ui: &Ui, value: &str) -> egui::Vec2 {
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_body);
    // Retain the native hit target: a taller editor would intercept nearby sketch picks.
    let height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().extra_text_line_spacing + t.space_4;
    vec2(grown_width(ui, value, &font, t.control_height * 3.0 - t.space_6), height)
}
