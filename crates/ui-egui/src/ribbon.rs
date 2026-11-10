//! The ribbon as components: its tab row ([`tabs`]), its groups' menus ([`menu_row`], [`grid`]) and
//! its panel row: groups of command buttons, each with its label under
//! them, fitted to the room there is ([`fit`]): buttons narrow, groups hand buttons to their menu
//! and collapse, and the last groups go into a » menu. The app describes the groups as plain data
//! ([`Row`]); [`show`] draws them and returns what was chosen ([`Action`]) with what the app
//! adds of its own to each button (hover text, drags, a split button's menu: [`Out`]). It reads
//! nothing of the app, and publishes its parts in the widget registry under the names the app
//! gives ([`Handles`]).

use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::type_face::RichTextExt as _;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use solidcraft_theme::{TabAccent, Tokens};

/// How a panel is drawn on a toolbar that may be too narrow for every button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Its first buttons (all of them, or the ones that fit); the panel's drop-down lists every
    /// command.
    Buttons,
    /// One button that opens the panel's drop-down (which lists every command, pinned or not).
    Collapsed,
    /// In the » menu at the end of the toolbar.
    More,
}

/// A ribbon button's preferred and minimum widths. The constraint gallery is a fixed-width
/// block; ordinary buttons narrow as their labels wrap and truncate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonSize {
    pub width: f32,
    pub min_width: f32,
}

/// One panel as [`fit`] sees it, in button order (the gallery, if present, is last).
#[derive(Clone, Debug, PartialEq)]
pub struct PanelSize {
    pub buttons: Vec<ButtonSize>,
}

/// Where [`fit`] put a panel: its first buttons and their individual fitted widths.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub fit: Fit,
    pub shown: usize,
    pub widths: Vec<f32>,
}

impl Plan {
    /// Every button at its full width.
    pub fn full(panel: &PanelSize) -> Self {
        Self { fit: Fit::Buttons, shown: panel.buttons.len(), widths: panel.buttons.iter().map(|b| b.width).collect() }
    }

    fn more() -> Self {
        Self { fit: Fit::More, shown: 0, widths: Vec::new() }
    }

    /// The panel's width on the toolbar (0 in the » menu).
    pub fn width(&self) -> f32 {
        match self.fit {
            Fit::More => 0.0,
            Fit::Collapsed => COLLAPSED_WIDTH,
            Fit::Buttons => panel_width(self.widths.get(..self.shown).unwrap_or_default()),
        }
    }
}

/// A panel's width with these individual button widths.
pub fn panel_width(buttons: &[f32]) -> f32 {
    (buttons.iter().sum::<f32>() + buttons.len().saturating_sub(1) as f32 * BUTTON_GAP + 8.0).max(COLLAPSED_WIDTH)
}

/// A collapsed panel's width: one button's panel.
pub const COLLAPSED_WIDTH: f32 = 72.0;

/// The gap after each panel (its separator).
pub const PANEL_GAP: f32 = 1.0;
pub const GROUP_HEIGHT: f32 = 86.0;
pub const BUTTON_HEIGHT: f32 = 60.0;
pub const ICON_BUTTON_HEIGHT: f32 = 36.0;
pub const ICON_GROUP_HEIGHT: f32 = 62.0;

pub const BUTTON_GAP: f32 = 3.0;
pub const BUTTON_MIN_WIDTH: f32 = 54.0;
pub const BUTTON_MAX_WIDTH: f32 = 76.0;
pub const BUTTON_LABEL_PADDING: f32 = 10.0;
/// Where a button's label starts below the button's top edge, under its icon.
pub const BUTTON_LABEL_TOP: f32 = 32.0;
/// Room kept below a button's label, inside its highlight.
pub const BUTTON_LABEL_BOTTOM: f32 = 2.0;
pub const SECTION_HEIGHT: f32 = 20.0;

/// The » button's room at the end of the panels, kept only while some panel is in it.
pub const MORE_ROOM: f32 = 40.0;

/// Fit panels into `room`, giving up as little as it takes, one small step at a time:
/// 1. buttons narrow from their measured label widths to their minimum, wrapping and truncating;
/// 2. panels hand their last buttons to their drop-down one at a time, the panel showing the
///    most buttons first (the rightmost of equals), down to one button (collapsed);
/// 3. the last panels go into the » menu, and any room that frees goes back to buttons.
///
/// Each removal frees at most one button (or the fixed-width constraint gallery), so the toolbar
/// never leaves a larger gap while something is hidden. The result depends only on `room`
/// (no state from earlier frames), so it never flickers. Nothing is dropped.
pub fn fit(panels: &[PanelSize], room: f32) -> Vec<Plan> {
    let room = if room.is_finite() { room } else { 0.0 };
    if let Some(plans) = shrink(panels, room) {
        return plans;
    }
    // 3. The last panels into the » menu (which takes its own room), as few as it takes; the
    // ones left keep as many buttons as fit.
    for kept in (0..panels.len()).rev() {
        if let Some(mut plans) = shrink(panels.get(..kept).unwrap_or_default(), room - MORE_ROOM) {
            plans.extend(panels.iter().skip(kept).map(|_| Plan::more()));
            return plans;
        }
    }
    panels.iter().map(|_| Plan::more()).collect()
}

/// The room plans take on the toolbar (each panel and its separator).
fn used(plans: &[Plan]) -> f32 {
    plans.iter().filter(|p| p.fit != Fit::More).map(|p| p.width() + PANEL_GAP).sum()
}

/// Steps 1 and 2 of [`fit`]: every panel on the toolbar, or None when even one button each
/// doesn't fit.
fn shrink(panels: &[PanelSize], room: f32) -> Option<Vec<Plan>> {
    let mut plans: Vec<Plan> = panels.iter().map(Plan::full).collect();
    if used(&plans) <= room {
        return Some(plans);
    }
    // 1. Narrow together in one-pixel steps, keeping each button's measured width until it
    // reaches its minimum. The gallery's minimum equals its full width.
    for reduction in 1..=((BUTTON_MAX_WIDTH - BUTTON_MIN_WIDTH) as usize) {
        for (plan, panel) in plans.iter_mut().zip(panels) {
            for (width, button) in plan.widths.iter_mut().zip(&panel.buttons) {
                *width = (button.width - reduction as f32).max(button.min_width);
            }
        }
        if used(&plans) <= room {
            return Some(plans);
        }
    }
    // 2. One button at a time into the drop-downs: from the panel showing the most (the
    // rightmost of equals), down to one button each.
    while used(&plans) > room {
        let pick = plans
            .iter()
            .enumerate()
            .filter(|(_, p)| p.fit == Fit::Buttons && (p.shown > 1 || p.width() > COLLAPSED_WIDTH))
            .max_by(|(i, a), (j, b)| a.shown.cmp(&b.shown).then(i.cmp(j)));
        let (i, _) = pick?;
        if let Some(p) = plans.get_mut(i) {
            p.shown = p.shown.saturating_sub(1).max(1);
            if p.shown == 1 {
                p.fit = Fit::Collapsed;
            }
        }
    }
    Some(plans)
}

/// The width of a group's gallery of small cells, with labels.
pub const GALLERY_WIDTH: f32 = 82.0;

/// Choose at most two lines at spaces or after `/`, then truncate each line at
/// Unicode character boundaries. An oversized word stays on one line with an ellipsis.
pub fn button_lines(painter: &egui::Painter, label: &str, width: f32) -> Vec<String> {
    let mut lines = label_lines(painter, label, width);
    // Right-to-left labels come in display order (see `bidi`): a line reads from its right end,
    // so a shortened line loses characters on the left, where the ellipsis then goes.
    let rtl = solidcraft_ui_text::bidi::has_rtl(label);
    for line in &mut lines {
        if label_width(painter, line) > width {
            while !line.is_empty() {
                if rtl {
                    line.remove(0);
                } else {
                    line.pop();
                }
                let shortened = if rtl { format!("…{}", line.trim_start()) } else { format!("{}…", line.trim_end()) };
                if label_width(painter, &shortened) <= width {
                    break;
                }
            }
            *line = if rtl { format!("…{}", line.trim_start()) } else { format!("{}…", line.trim_end()) };
        }
    }
    lines
}

pub fn label_width(painter: &egui::Painter, text: &str) -> f32 {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        FontId::proportional(solidcraft_theme::Tokens::get().font_body),
        // Measured only: the colour plays no part in the width.
        solidcraft_theme::Tokens::get().text,
        f32::INFINITY,
    );
    job.halign = egui::Align::Center;
    painter.layout_job(job).size().x
}

pub fn button_width(painter: &egui::Painter, label: &str) -> f32 {
    let widest =
        label_lines(painter, label, BUTTON_MAX_WIDTH - BUTTON_LABEL_PADDING).iter().map(|line| label_width(painter, line)).fold(0.0_f32, f32::max);
    (widest + BUTTON_LABEL_PADDING).clamp(BUTTON_MIN_WIDTH, BUTTON_MAX_WIDTH)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CjkScript {
    Han,
    Hiragana,
    Katakana,
    Other,
}

fn cjk_script(c: char) -> CjkScript {
    match c {
        '\u{3040}'..='\u{309F}' => CjkScript::Hiragana,
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF65}'..='\u{FF9F}' => CjkScript::Katakana,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' => CjkScript::Han,
        _ => CjkScript::Other,
    }
}

/// Whether byte offset `end` sits at a word boundary in a spaceless Japanese ribbon label.
pub(crate) fn cjk_word_break(label: &str, end: usize) -> bool {
    let (Some(prev), Some(next)) = (label.get(..end).and_then(|s| s.chars().next_back()), label.get(end..).and_then(|s| s.chars().next())) else {
        return false;
    };
    let (a, b) = (cjk_script(prev), cjk_script(next));
    if a == CjkScript::Katakana && b == CjkScript::Katakana {
        let before = label.get(..end).unwrap_or_default();
        return before.ends_with("サーフェス") || before.ends_with("フラット");
    }
    match (a, b) {
        (CjkScript::Han, CjkScript::Katakana)
        | (CjkScript::Katakana, CjkScript::Han | CjkScript::Hiragana)
        | (CjkScript::Hiragana, CjkScript::Han | CjkScript::Katakana) => true,
        (CjkScript::Han, CjkScript::Hiragana) => next == 'を',
        (CjkScript::Other, CjkScript::Katakana | CjkScript::Hiragana) => prev.is_ascii_alphabetic(),
        (CjkScript::Katakana | CjkScript::Hiragana, CjkScript::Other) => next.is_ascii_alphabetic(),
        _ => false,
    }
}

pub fn label_lines(painter: &egui::Painter, label: &str, width: f32) -> Vec<String> {
    if let Some((first, second)) = label.split_once('\n') {
        let first = first.trim();
        let second = second.trim();
        if !first.is_empty() && !second.is_empty() {
            return vec![first.to_owned(), second.to_owned()];
        }
    }
    let measure = |text: &str| label_width(painter, text);
    let normalized = label.split_whitespace().collect::<Vec<_>>().join(" ");
    let label = normalized.as_str();
    if solidcraft_ui_text::bidi::has_rtl(label) {
        // Display-order right-to-left text reads from its end: the first line takes the words
        // at the end of the string, the second line the words before them.
        let breaks = label.char_indices().filter(|(_, c)| c.is_whitespace() || *c == '/').map(|(at, c)| at + c.len_utf8());
        let mut start = label.len();
        for cut in std::iter::once(0).chain(breaks).collect::<Vec<_>>().into_iter().rev() {
            if cut >= start {
                continue;
            }
            if start < label.len() && measure(label.get(cut..).unwrap_or_default().trim()) > width {
                break;
            }
            start = cut;
        }
        let (rest, first) = label.split_at(start);
        let mut lines = vec![first.trim().to_owned()];
        if !rest.trim().is_empty() {
            lines.push(rest.trim().to_owned());
        }
        return lines;
    }
    let has_delimiters = label.chars().any(|c| c.is_whitespace() || c == '/');
    let mut end = 0;
    for (at, ch) in label.char_indices() {
        let next = at + ch.len_utf8();
        let is_break = next == label.len() || ch.is_whitespace() || ch == '/' || (!has_delimiters && cjk_word_break(label, next));
        if !is_break {
            continue;
        }
        if end > 0 && measure(label.get(..next).unwrap_or_default().trim()) > width {
            break;
        }
        end = next;
    }
    let (first, rest) = label.split_at(end);
    let mut lines = vec![first.trim().to_owned()];
    if !rest.trim().is_empty() {
        lines.push(rest.trim().to_owned());
    }
    lines
}

pub fn button_galley(painter: &egui::Painter, label: &str, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    button_galley_weight(painter, label, width, color, false)
}

pub fn button_galley_weight(painter: &egui::Painter, label: &str, width: f32, color: Color32, selected: bool) -> std::sync::Arc<egui::Galley> {
    let lines = button_lines(painter, label, width);
    // Explicit newlines only: egui must never split an oversized word.
    let size = solidcraft_theme::Tokens::get().font_body;
    let mut job = egui::text::LayoutJob::simple(
        lines.join("\n"),
        if selected { crate::ui_kit::type_face::semibold(size) } else { FontId::proportional(size) },
        color,
        f32::INFINITY,
    );
    job.halign = egui::Align::Center;
    // Two lines sit closer than the font's own spacing, so they fit in the button under its
    // icon (#8391).
    if lines.len() > 1
        && let Some(section) = job.sections.first_mut()
    {
        section.format.line_height = Some(size + 2.0);
    }
    painter.layout_job(job)
}

/// The top of a button's label, `height` tall, in the button `whole`: below the icon, raised
/// when it would otherwise run past the button's bottom edge (#8391).
pub fn label_top(whole: Rect, height: f32) -> f32 {
    (whole.top() + BUTTON_LABEL_TOP).min(whole.bottom() - BUTTON_LABEL_BOTTOM - height)
}

/// A group label's inset from the group's edges: the ribbon card's own inset, so their corners
/// are concentric.
pub const CARD_INSET: f32 = 4.0;

/// Where the row publishes its parts in the widget registry: each name is a prefix, followed by
/// a command id or a group's key.
#[derive(Clone, Copy, Debug)]
pub struct Handles<'a> {
    /// A button's centre, under each of these prefixes.
    pub button: &'a [&'a str],
    /// A button's whole rect, and the centre of the command a click on it runs when that is
    /// another (a split button's chosen variant).
    pub button_rect: &'a str,
    /// A split button's menu half, by its split key.
    pub split: &'a str,
    /// A gallery cell, under each of these prefixes.
    pub cell: &'a [&'a str],
    /// A collapsed group's one button.
    pub collapsed: &'a str,
    /// A group's label: its centre, and its rect.
    pub section: &'a str,
    pub section_target: &'a str,
    /// A group's whole rect.
    pub group: &'a str,
    /// A group's label lit for its running command that is in its menu only.
    pub running: &'a str,
    /// What the search points out (one name for every place).
    pub reveal: &'a str,
    /// The » button, and each of its rows by command id.
    pub more: &'a str,
    pub more_row: &'a str,
    /// The counts of groups collapsed, in the » menu, and showing fewer buttons than they have.
    pub collapsed_count: &'a str,
    pub more_count: &'a str,
    pub trimmed_count: &'a str,
    /// A dragged command's ghost under the pointer, and the same when releasing removes it.
    pub drag: &'a str,
    pub drag_remove: &'a str,
}

/// The row as the app describes it this frame.
pub struct Row<'a> {
    /// In order; with `exit`, the last one is the exit group.
    pub groups: Vec<Group<'a>>,
    /// Buttons carry their labels (taller buttons, wider cells).
    pub labels: bool,
    /// The active tab's accent: pressed buttons, open and running labels.
    pub accent: TabAccent,
    /// The last group leaves the mode (Finish): room is always kept for it, it never collapses,
    /// and it moves to the far right when groups go into the » menu.
    pub exit: bool,
    /// The command the search points out.
    pub reveal: Option<Reveal<'a>>,
    /// The » button's hover text.
    pub more_tip: &'a str,
    pub handles: Handles<'a>,
}

/// The command the search points out: a preview while a result is hovered, or a pulse after it
/// started; `strength` is the pulse's, now.
#[derive(Clone, Copy, Debug)]
pub struct Reveal<'a> {
    pub id: &'a str,
    pub preview: bool,
    pub strength: f32,
}

/// One group of buttons, with its label under them.
pub struct Group<'a> {
    /// Its name in handles and ids.
    pub key: &'a str,
    /// Its entry in the » menu.
    pub name: &'a str,
    /// The label under the buttons (the exit group's too).
    pub label: &'a str,
    /// A collapsed group's hover text.
    pub tip: &'a str,
    /// The buttons it may show, in order.
    pub buttons: Vec<Button<'a>>,
    /// Its gallery of small cells after the buttons, shown or handed to its menu together.
    pub cells: Vec<Button<'a>>,
    /// Every command of the group, in its menu or not.
    pub commands: Vec<&'a str>,
    /// A collapsed group's icon (a kit icon name).
    pub icon: Option<&'a str>,
    /// Its label and collapsed button open its menu.
    pub enabled: bool,
    /// Its menu is open.
    pub open: bool,
    /// The command running now, lit on the label while it is in the menu only.
    pub running: Option<&'a str>,
    /// The rows it lists in the » menu.
    pub more: Vec<MoreRow<'a>>,
}

/// A command button (or a gallery cell).
pub struct Button<'a> {
    /// The command it stands for (its handles).
    pub id: &'a str,
    /// What a click runs: `id`, or a split button's chosen variant.
    pub run: &'a str,
    /// Its label, already translated (not drawn without labels).
    pub label: &'a str,
    /// Its accessible name.
    pub name: &'a str,
    /// A kit icon name.
    pub icon: &'a str,
    pub size: ButtonSize,
    pub enabled: bool,
    pub pressed: bool,
    /// It leaves the mode: never shown pressed.
    pub exit: bool,
    /// A menu half under the button (its variants), with its key, title and accessible name.
    pub split: Option<Split<'a>>,
    /// The app paints its background and label itself, after the row (`Drawn::background`).
    pub app_paints: bool,
    /// The other commands it stands for (a split button's variants), in the search's eyes.
    pub covers: &'a [&'a str],
}

#[derive(Clone, Copy, Debug)]
pub struct Split<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub name: &'a str,
}

/// A row of a group in the » menu.
pub struct MoreRow<'a> {
    pub id: &'a str,
    /// Its label (the menu row translates it) and kit icon.
    pub label: &'a str,
    pub icon: &'a str,
    pub shortcut: &'a str,
    pub enabled: bool,
    /// A check mark: on or off (None: none).
    pub checked: Option<bool>,
    /// Its hover text.
    pub hint: Option<&'a str>,
}

/// Where a command was started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Button,
    Cell,
    More,
}

/// What the user chose on the row this frame, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Run command `id`, from `rect`.
    Run { id: String, rect: Rect, place: Place },
    /// A button's or cell's menu, at `at`.
    Menu { id: String, at: Pos2 },
    /// Group `group`'s label was clicked: open its menu, or close it when open.
    Section { group: usize },
    /// Group `group`'s collapsed button was clicked: open its menu.
    Open { group: usize },
    /// Command `id` was dropped in group `group`, before button `before` (None: at its end).
    Drop { id: String, group: usize, before: Option<String> },
}

/// A button the row drew, for the app's own part of it (hover text, drags, its menu half's menu).
pub struct Drawn {
    pub group: usize,
    /// Its place in the group's buttons (or cells).
    pub index: usize,
    pub cell: bool,
    /// The whole button, and the part that runs it (less a split button's menu half).
    pub whole: Rect,
    pub rect: Rect,
    pub response: egui::Response,
    /// A split button's menu half.
    pub split: Option<egui::Response>,
    /// An `app_paints` button's paint, for [`paint_deferred`].
    pub deferred: Option<Deferred>,
}

/// A group drawn on the row.
pub struct GroupOut {
    pub group: usize,
    /// The whole group, and its label.
    pub rect: Rect,
    pub label: Rect,
    pub label_response: egui::Response,
    /// Drawn as its one collapsed button.
    pub collapsed: bool,
    /// Shows fewer buttons than it has.
    pub trimmed: bool,
    /// How many of its buttons show, and whether its cells do.
    pub buttons: usize,
    pub cells: bool,
    /// The place of each button shown, and of the gallery when it shows.
    pub slots: Vec<Rect>,
    /// Its label or collapsed button is hovered, and it has a menu.
    pub hot: bool,
    /// The search previews a command that is in its menu only.
    pub previewed: bool,
}

/// The row drawn.
pub struct Out {
    pub actions: Vec<Action>,
    pub groups: Vec<GroupOut>,
    pub buttons: Vec<Drawn>,
}

/// The dropdown chevron every ribbon menu shares.
pub fn dropdown_arrow(painter: &egui::Painter, at: Pos2, ink: Color32) {
    crate::ui_kit::dropdown_indicator(painter, Rect::from_center_size(at, vec2(10.0, 10.0)), ink);
}

/// The search's mark on `rect`: an outline in the accent, `strength` of the pulse, filled with
/// its soft colour when `fill`.
pub fn paint_reveal(painter: &egui::Painter, rect: Rect, accent: &TabAccent, strength: f32, fill: bool) {
    if strength <= 0.0 {
        return;
    }
    let t = Tokens::get();
    painter.rect(
        rect,
        t.radius_sm,
        if fill { accent.soft.gamma_multiply(strength) } else { Color32::TRANSPARENT },
        Stroke::new(2.0, accent.color.gamma_multiply(strength)),
        egui::StrokeKind::Inside,
    );
}

/// A centred single line of ribbon text at `at`, on whole pixels.
pub fn text(ui: &egui::Ui, at: Pos2, text: &str, size: f32, color: Color32, bold: bool) {
    let text = egui::RichText::new(text).size(size).color(color);
    let text = if bold { text.semibold() } else { text };
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    galley_at(ui, at, galley, color);
}

/// `galley` centred at `at`, on whole pixels.
pub fn galley_at(ui: &egui::Ui, at: Pos2, galley: std::sync::Arc<egui::Galley>, color: Color32) {
    let origin = at - galley.size() / 2.0;
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
    ui.painter().galley(origin, galley, color);
}

/// The colour a command's icon means, for a pressed gallery cell on the default accent.
fn icon_accent(icon: &str) -> Color32 {
    crate::ui_kit::icons::primary(icon, solidcraft_theme::is_dark(), solidcraft_theme::vision())
}

fn handle(prefix: &str, key: &str) -> String {
    format!("{prefix}{key}")
}

/// Draws the groups in `rect` (below the tabs, from the first group's left edge), fitting them to
/// its width ([`fit`]); the groups that do not fit go into the » menu. `more_rows` may draw a
/// group's rows in the » menu itself: given the group's place, it returns true when it did (the
/// row then lists none of its own).
pub fn show(ui: &mut egui::Ui, row: &Row, rect: Rect, more_rows: &mut dyn FnMut(&mut egui::Ui, usize) -> bool) -> Out {
    let t = Tokens::get();
    let painter = ui.painter().clone();
    let labels = row.labels;
    let h = &row.handles;
    let button_height = if labels { BUTTON_HEIGHT } else { ICON_BUTTON_HEIGHT };
    let group_height = if labels { GROUP_HEIGHT } else { ICON_GROUP_HEIGHT };
    let gallery_width = if labels { GALLERY_WIDTH } else { 6.0 * 27.0 };
    let (top, right) = (rect.top(), rect.right());
    let mut px = rect.left();
    let sizes: Vec<PanelSize> = row
        .groups
        .iter()
        .map(|g| {
            let mut buttons: Vec<ButtonSize> = g.buttons.iter().map(|b| b.size).collect();
            if !g.cells.is_empty() {
                buttons.push(ButtonSize { width: gallery_width, min_width: gallery_width });
            }
            PanelSize { buttons }
        })
        .collect();
    // The exit group follows the last group and always keeps its room, so it stays reachable at
    // every width; when groups go into the » menu it moves to the far right, with a gap of its
    // own before it so the groups on its left collapse before they would touch it.
    let exit_plan = sizes.last().filter(|_| row.exit).map(Plan::full);
    let exit_width = exit_plan.as_ref().map_or(0.0, |p| p.width().max(80.0));
    let exit_gap = if exit_plan.is_some() { 4.0 } else { 0.0 };
    let normal = sizes.len().saturating_sub(usize::from(exit_plan.is_some()));
    let mut plans = fit(sizes.get(..normal).unwrap_or_default(), right - 4.0 - px - exit_width - exit_gap);
    let exit_right = plans.iter().any(|p| p.fit == Fit::More);
    plans.extend(exit_plan);
    solidcraft_ui_registry::publish_count(h.collapsed_count, plans.iter().filter(|p| p.fit == Fit::Collapsed).count() as f64);
    solidcraft_ui_registry::publish_count(h.more_count, plans.iter().filter(|p| p.fit == Fit::More).count() as f64);
    solidcraft_ui_registry::publish_count(
        h.trimmed_count,
        plans.iter().zip(&sizes).filter(|(p, s)| p.fit == Fit::Buttons && p.shown < s.buttons.len()).count() as f64,
    );
    let mut out = Out { actions: Vec::new(), groups: Vec::new(), buttons: Vec::new() };
    let mut overflow = Vec::new();
    let accent = row.accent;
    for (gi, (group, plan)) in row.groups.iter().zip(plans).enumerate() {
        if plan.fit == Fit::More {
            overflow.push(gi);
            continue;
        }
        // A group left with no buttons (all moved away) is drawn collapsed too: one button that
        // opens its menu, so its commands never vanish from the row (#7166).
        let collapsed = plan.fit == Fit::Collapsed || (group.buttons.is_empty() && group.cells.is_empty() && !group.commands.is_empty());
        let trimmed = plan.shown < plan.widths.len();
        let exit = row.exit && gi + 1 == row.groups.len();
        let width = if exit { exit_width } else { plan.width() };
        if exit && exit_right {
            px = right - exit_width;
            painter.line_segment([pos2(px, top), pos2(px, rect.bottom() - 1.0)], t.tertiary_border(t.border));
        }
        let button_widths = &plan.widths;
        let drawn = if collapsed { 0 } else { plan.shown.min(button_widths.len()) };
        let cells = !collapsed && drawn > group.buttons.len();
        // Buttons narrower than the group's minimum width (a one-button group) are centred.
        let buttons_width = button_widths.iter().take(drawn).sum::<f32>() + drawn.saturating_sub(1) as f32 * BUTTON_GAP;
        let inset = if drawn == 0 { 0.0 } else { ((width - 8.0 - buttons_width) / 2.0).max(0.0).floor() };
        let bx = px + 4.0 + inset;
        let free_x = bx + buttons_width + BUTTON_GAP;
        let free = Rect::from_min_size(pos2(free_x, top + 2.0), vec2((px + width - 4.0 - free_x).max(0.0), button_height));
        let fresp = ui.interact(free, ui.scope_id().with(("panel_free", group.key)), if collapsed { Sense::click() } else { Sense::hover() });
        let collapsed_hovered = collapsed && fresp.hovered();
        let mut slot_x = bx;
        let slots: Vec<Rect> = button_widths
            .iter()
            .take(drawn)
            .map(|w| {
                let slot = Rect::from_min_size(pos2(slot_x, top + 2.0), vec2(*w, button_height));
                slot_x += w + BUTTON_GAP;
                slot
            })
            .collect();
        if collapsed {
            // One button: the group's icon over a caret; it opens the group's menu.
            let br = Rect::from_center_size(free.center(), vec2((width - 8.0).max(0.0), button_height));
            solidcraft_ui_registry::publish_handle(&handle(h.collapsed, group.key), br.center());
            if fresp.hovered() && group.enabled {
                painter.rect_filled(br, t.radius_sm, t.hover);
            }
            if let Some(icon) = group.icon {
                let icon_rect = Rect::from_center_size(br.center() - vec2(0.0, 3.0), vec2(26.0, 26.0));
                crate::ui_kit::icons::paint_icon(&painter, icon_rect, icon, f32::from(t.icon.a()) / 255.0);
            }
            dropdown_arrow(&painter, pos2(br.center().x, br.bottom() - 3.0), t.text_dim);
            if fresp.backing_on_hover_text(group.tip).clicked() && group.enabled {
                out.actions.push(Action::Open { group: gi });
            }
        }
        let mut button_x = bx;
        for (bi, (b, button_width)) in group.buttons.iter().zip(button_widths.iter().copied()).take(drawn).enumerate() {
            let whole = Rect::from_min_size(pos2(button_x, top + 2.0), vec2(button_width, button_height));
            button_x += button_width + BUTTON_GAP;
            let mut br = whole;
            if b.split.is_some() {
                if labels {
                    br.max.y = whole.top() + 32.0;
                } else {
                    br.max.x -= 14.0;
                }
            }
            let resp = ui.interact(br, ui.scope_id().with(("cmd", b.id)), Sense::click_and_drag());
            for prefix in h.button {
                solidcraft_ui_registry::publish_handle(&handle(prefix, b.id), br.center());
            }
            solidcraft_ui_registry::publish_rect(&handle(h.button_rect, b.id), whole);
            if b.run != b.id {
                solidcraft_ui_registry::publish_handle(&handle(h.button_rect, b.run), br.center());
            }
            if resp.secondary_clicked() {
                out.actions.push(Action::Menu { id: b.id.to_string(), at: resp.interact_pointer_pos().unwrap_or(br.left_bottom()) });
            }
            let background = painter.add(egui::Shape::Noop);
            let split_id = b.split.map(|s| ui.scope_id().with(("family", s.key)));
            let split_open = split_id.is_some_and(|id| egui::Popup::is_id_open(ui.ctx(), id.with("popup")));
            let neutral = if !b.exit && (resp.is_pointer_button_down_on() || b.pressed || split_open) {
                egui::Shape::Rect(egui::epaint::RectShape::new(
                    whole,
                    t.radius_sm,
                    accent.soft,
                    Stroke::new(1.0, accent.color),
                    egui::StrokeKind::Inside,
                ))
            } else if resp.hovered() && b.enabled {
                egui::Shape::rect_filled(whole, t.radius_sm, t.hover)
            } else {
                egui::Shape::Noop
            };
            if !b.app_paints {
                painter.set(background, neutral.clone());
            }
            if let Some(r) = row.reveal.filter(|r| r.id == b.id || r.id == b.run || (b.split.is_some() && b.covers.contains(&r.id))) {
                paint_reveal(&painter, whole, &accent, r.strength, true);
                solidcraft_ui_registry::publish_rect(h.reveal, whole);
            }
            let side = if exit { 30.0 } else { 26.0 };
            let icon_rect =
                Rect::from_center_size(pos2(br.center().x, whole.top() + if labels { 16.0 } else { button_height / 2.0 }), vec2(side, side));
            crate::ui_kit::icons::paint_icon(&painter, icon_rect, b.icon, if b.enabled { 1.0 } else { 0.35 });
            if !b.app_paints {
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled() && b.enabled, b.name));
            }
            if resp.clicked() && b.enabled {
                out.actions.push(Action::Run { id: b.run.to_string(), rect: whole, place: Place::Button });
            }
            let mut drawn_button = Drawn { group: gi, index: bi, cell: false, whole, rect: br, response: resp, split: None, deferred: None };
            if b.app_paints {
                let label_slot = painter.add(egui::Shape::Noop);
                drawn_button.deferred = Some(Deferred {
                    response: drawn_button.response.clone(),
                    painter: painter.clone(),
                    rect: whole,
                    background,
                    label_slot,
                    neutral,
                });
            } else if let (Some(s), Some(split_id)) = (b.split, split_id) {
                let ar = if labels {
                    Rect::from_min_max(pos2(whole.left(), whole.top() + 30.0), pos2(whole.right(), whole.top() + 48.0))
                } else {
                    Rect::from_min_max(pos2(br.right(), whole.top()), whole.right_bottom())
                };
                let arrow = ui.interact(ar, split_id, Sense::click());
                solidcraft_ui_registry::publish_handle(&handle(h.split, s.key), ar.center());
                if arrow.hovered() {
                    painter.rect_filled(ar, t.radius_sm, t.hover);
                }
                // The title sits on the same line as the other buttons' labels.
                if labels {
                    let title_y = whole.top() + 39.0;
                    let galley = button_galley(&painter, s.title, ar.width() - 16.0, t.text);
                    painter.galley(pos2(ar.center().x - 4.0, title_y - galley.size().y / 2.0), galley, t.text);
                    dropdown_arrow(&painter, pos2(ar.right() - 6.0, title_y + 0.5), t.text_dim);
                } else {
                    dropdown_arrow(&painter, ar.center(), t.text_dim);
                }
                arrow.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, s.name));
                drawn_button.split = Some(arrow);
            } else if labels {
                let color = if b.enabled { t.text } else { t.text_dim };
                let galley = button_galley(&painter, b.label, whole.width() - BUTTON_LABEL_PADDING, color);
                painter.galley(pos2(whole.center().x, label_top(whole, galley.size().y)), galley, t.text);
            }
            drag_onto(&painter, &drawn_button.response, b.id, Some(br), gi, &mut out.actions);
            out.buttons.push(drawn_button);
        }
        if cells {
            // A quiet rule sets the gallery apart from the large button before it.
            t.secondary_line(
                &painter,
                [pos2(button_x - 1.5, top + 8.0), pos2(button_x - 1.5, top + button_height.min(46.0))],
                t.toolbar.lerp_to_gamma(t.border, 0.5),
            );
            for (index, c) in group.cells.iter().enumerate() {
                let columns = if labels { 3 } else { 6 };
                let cell = Rect::from_min_size(
                    pos2(button_x + (index % columns) as f32 * 27.0, top + 6.0 + (index / columns) as f32 * 23.0),
                    vec2(24.0, 20.0),
                );
                let resp = ui.interact(cell, ui.scope_id().with(("cmd", c.id)), Sense::click_and_drag());
                let (fill, stroke) = if c.pressed {
                    if accent.color == t.accent {
                        (t.panel.lerp_to_gamma(icon_accent(c.icon), 0.18), icon_accent(c.icon))
                    } else {
                        (accent.soft, accent.color)
                    }
                } else if resp.hovered() && c.enabled {
                    (t.hover, t.border)
                } else {
                    (t.panel, t.border)
                };
                painter.rect(cell, t.radius_sm, fill, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
                if let Some(r) = row.reveal.filter(|r| r.id == c.id) {
                    paint_reveal(&painter, cell, &accent, r.strength, true);
                    solidcraft_ui_registry::publish_rect(h.reveal, cell);
                }
                crate::ui_kit::icons::paint_icon(
                    &painter,
                    Rect::from_center_size(cell.center(), vec2(15.0, 15.0)),
                    c.icon,
                    if c.enabled { 1.0 } else { 0.35 },
                );
                for prefix in h.cell {
                    solidcraft_ui_registry::publish_target(&handle(prefix, c.id), cell);
                }
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, c.enabled, c.name));
                if resp.secondary_clicked() {
                    out.actions.push(Action::Menu { id: c.id.to_string(), at: resp.interact_pointer_pos().unwrap_or(cell.left_bottom()) });
                }
                if resp.clicked() && c.enabled {
                    out.actions.push(Action::Run { id: c.id.to_string(), rect: cell, place: Place::Cell });
                }
                drag_onto(&painter, &resp, c.id, None, gi, &mut out.actions);
                out.buttons.push(Drawn { group: gi, index, cell: true, whole: cell, rect: cell, response: resp, split: None, deferred: None });
            }
        }
        // The label row is also the group's menu button. The group area ends 1 px above the
        // card's bottom edge, so the highlight is CARD_INSET from the card's bottom edge (and
        // from its left edge in the first group): its corner is concentric with the card's.
        let lr = Rect::from_min_size(
            pos2(px + CARD_INSET, top + group_height - SECTION_HEIGHT - (CARD_INSET - 1.0)),
            vec2((width - 2.0 * CARD_INSET).max(0.0), SECTION_HEIGHT),
        );
        let lresp = ui.interact(lr, ui.scope_id().with(("panel", group.key)), Sense::click());
        solidcraft_ui_registry::publish_handle(&handle(h.section, group.key), lr.center());
        solidcraft_ui_registry::publish_target(&handle(h.section_target, group.key), lr);
        // The commands this group has only in its menu (not drawn as a button or cell).
        let drawn_ids: Vec<&str> = group
            .buttons
            .iter()
            .take(drawn)
            .flat_map(|b| if b.covers.is_empty() { vec![b.id] } else { b.covers.to_vec() })
            .chain(group.cells.iter().filter(|_| cells).map(|c| c.id))
            .collect();
        let hidden = |id: &str| group.commands.contains(&id) && !drawn_ids.contains(&id);
        let hidden_running = group.running.is_some_and(hidden);
        let hidden_reveal = row.reveal.filter(|r| hidden(r.id));
        let hot = lresp.hovered() && group.enabled;
        if hidden_running && !group.open {
            // Its command runs from the menu: the label shows it like a pressed button.
            painter.rect(lr, t.radius_sm, accent.soft, Stroke::new(1.0, accent.color), egui::StrokeKind::Inside);
            solidcraft_ui_registry::publish_rect(&handle(h.running, group.key), lr);
        } else if group.open || hot {
            painter.rect_filled(lr, t.radius_sm, if group.open { accent.soft } else { t.hover });
        }
        if let Some(r) = hidden_reveal.filter(|r| !r.preview) {
            paint_reveal(&painter, lr, &accent, r.strength, true);
            solidcraft_ui_registry::publish_rect(h.reveal, lr);
        }
        let open = group.open || hidden_running;
        let font = if open || hot { crate::ui_kit::type_face::semibold(t.font_caption) } else { FontId::proportional(t.font_caption) };
        let label_width = painter.layout_no_wrap(group.label.to_string(), font, t.text_dim).size().x;
        let ink = if open { accent.ink } else { t.text_dim };
        text(ui, lr.center() - vec2(if exit { 0.0 } else { 6.0 }, 0.0), group.label, t.font_caption, ink, open || hot);
        if !exit {
            dropdown_arrow(&painter, pos2(lr.center().x + label_width / 2.0 + 1.0, lr.center().y + 0.5), ink);
        }
        if lresp.clicked() && group.enabled {
            out.actions.push(Action::Section { group: gi });
        }
        let group_rect = Rect::from_min_max(pos2(px, top + 2.0), pos2(px + width, rect.bottom()));
        solidcraft_ui_registry::publish_rect(&handle(h.group, group.key), group_rect);
        // Dropped on the group's free space (all of a collapsed group): the command goes to its
        // end. Labels, gaps and margins append; buttons keep their insert-before target. The
        // current layer counts, so a menu covering the group is no drop target.
        let over_button = ui.input(|input| input.pointer.hover_pos()).is_some_and(|p| slots.iter().any(|slot| slot.contains(p)));
        if !over_button && ui.rect_contains_pointer(group_rect) && egui::DragAndDrop::payload::<DragPayload>(ui.ctx()).is_some() {
            crate::ui_kit::insertion_marker(&painter, group_rect.right() - 4.0, top + 4.0, top + 38.0);
            if ui.input(|input| input.pointer.any_released())
                && let Some(d) = egui::DragAndDrop::take_payload::<DragPayload>(ui.ctx())
            {
                out.actions.push(Action::Drop { id: d.id.clone(), group: gi, before: None });
            }
        }
        out.groups.push(GroupOut {
            group: gi,
            rect: group_rect,
            label: lr,
            label_response: lresp,
            collapsed,
            trimmed,
            buttons: drawn.min(group.buttons.len()),
            cells,
            slots,
            hot: group.enabled && !exit && (hot || collapsed_hovered),
            previewed: hidden_reveal.is_some_and(|r| r.preview),
        });
        px += width;
        painter.line_segment([pos2(px, top + 4.0), pos2(px, top + group_height - 3.0)], t.tertiary_border(t.border));
        px += PANEL_GAP;
    }
    if !overflow.is_empty() {
        let at = Rect::from_min_size(pos2(px.min(right - exit_width - 40.0), top + 4.0), vec2(38.0, button_height));
        let actions = &mut out.actions;
        ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
            let response = crate::ui_kit::Menu::button(ui, "»", |ui| {
                for &gi in &overflow {
                    let Some(group) = row.groups.get(gi) else { continue };
                    crate::ui_kit::Menu::button(ui, group.name, |ui| {
                        if more_rows(ui, gi) {
                            return;
                        }
                        for r in &group.more {
                            let response = if r.checked.is_some() {
                                let mut item = crate::ui_kit::MenuItem::new(r.id, r.label, r.icon);
                                item.shortcut = r.shortcut;
                                item.enabled = r.enabled;
                                item.checked = r.checked;
                                item.show(ui)
                            } else {
                                crate::ui_kit::command_menu_button(ui, r.id, r.label, r.icon, r.shortcut, r.enabled)
                            };
                            let response = if let Some(hint) = r.hint { response.backing_on_hover_text(hint) } else { response };
                            solidcraft_ui_registry::publish_handle(&handle(h.more_row, r.id), response.rect.center());
                            if response.clicked() {
                                actions.push(Action::Run { id: r.id.to_string(), rect: response.rect, place: Place::More });
                                ui.close();
                            }
                        }
                    });
                }
            });
            solidcraft_ui_registry::publish_target(h.more, response.response.rect);
            response.response.backing_on_hover_text(row.more_tip);
        });
    }
    out
}

/// The tab row's height.
pub const TAB_ROW_HEIGHT: f32 = 30.0;

/// Where the tab row publishes its parts: each prefix is followed by a tab's id (or alias) or an
/// action's id.
#[derive(Clone, Copy, Debug)]
pub struct TabHandles<'a> {
    /// A tab's rect as a target, and the centre of each of its aliases.
    pub tab: &'a str,
    /// A tab's rect.
    pub tab_rect: &'a str,
    /// The + button.
    pub add: &'a str,
    /// Each action button after the search field.
    pub action: &'a str,
}

/// One tab.
#[derive(Clone, Copy, Debug)]
pub struct Tab<'a> {
    /// Its name in handles and ids.
    pub id: &'a str,
    /// Its label, already translated.
    pub label: &'a str,
    pub accent: TabAccent,
    /// It stands for what is being edited (a sketch): set apart and drawn loudest when selected.
    pub contextual: bool,
    /// Other names it is published under (tabs it took over).
    pub aliases: &'a [&'a str],
}

/// A button after the search field, with its label; `ready` says why it can't run now.
#[derive(Clone, Copy, Debug)]
pub struct TabButton<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub ready: Result<(), &'a str>,
}

/// The tab row as the app describes it this frame.
pub struct Tabs<'a> {
    pub tabs: Vec<Tab<'a>>,
    /// The selected tab's id.
    pub active: &'a str,
    /// The + button after the last tab, with its hover text (None: no + button).
    pub add: Option<&'a str>,
    /// Whether the app's menu is open from this rect now: the + button then shows held.
    pub menu_open_from: &'a dyn Fn(Rect) -> bool,
    /// Room kept at the right for the search field (0: the field is elsewhere).
    pub search_width: f32,
    /// Buttons after the search field.
    pub buttons: Vec<TabButton<'a>>,
    /// The ribbon is a floating card: rounder tabs, concentric with its corners.
    pub floating: bool,
    /// The row's bottom edge as a 2 px line in this colour (a contextual tab is selected), or
    /// the quiet separator.
    pub underline: Option<Color32>,
    pub handles: TabHandles<'a>,
}

/// What the user chose on the tab row this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum TabAction {
    /// Tab `index` was clicked; `already`: it was the selected one.
    Select { index: usize, already: bool },
    /// A tab's menu, at `at`.
    Menu { at: Pos2 },
    /// The + button, at `rect`: open (or close) the app's menu from it.
    Add { rect: Rect },
    /// A button after the search field.
    Run { id: String },
}

/// The tab row drawn: what was chosen, and where the search field goes.
pub struct TabsOut {
    pub actions: Vec<TabAction>,
    pub search: Rect,
}

/// How one tab is painted: its fill, its label colour and an outline (`Color32::TRANSPARENT` for
/// none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStyle {
    pub fill: Color32,
    pub ink: Color32,
    pub outline: Color32,
}

/// A contextual tab is the loudest tab when it is selected: the solid accent pill. Unselected,
/// it looks like any unselected tab with its accent as an outline, so it still marks the context
/// without outshining the selected tab. Ordinary tabs use the soft accent when selected and the
/// hover colour when hovered.
pub fn tab_style(t: &Tokens, accent: TabAccent, contextual: bool, active: bool, hovered: bool) -> TabStyle {
    let idle = if hovered { t.hover } else { Color32::TRANSPARENT };
    let (fill, ink, outline) = match (contextual, active) {
        (true, true) => (accent.color, accent.on_fill(t), Color32::TRANSPARENT),
        (true, false) => (idle, t.text, accent.color),
        (false, true) => (accent.soft, accent.ink, Color32::TRANSPARENT),
        (false, false) => (idle, t.text, Color32::TRANSPARENT),
    };
    TabStyle { fill, ink, outline }
}

/// The width of a tab or tab-row button labelled `label`.
fn tab_width(ui: &egui::Ui, label: &str) -> f32 {
    let t = Tokens::get();
    ui.painter().layout_no_wrap(label.to_string(), crate::ui_kit::type_face::semibold(t.font_button), t.text).size().x + 24.0
}

/// Draws the tab row in `row`: the tabs (scrolled sideways when they don't fit), the + button,
/// room for the search field and the buttons after it.
pub fn tabs(ui: &mut egui::Ui, row_data: &Tabs, row: Rect) -> TabsOut {
    let t = Tokens::get();
    let h = &row_data.handles;
    let widths: Vec<f32> = row_data.tabs.iter().map(|tab| tab_width(ui, tab.label)).collect();
    let inset = CARD_INSET;
    let add_size = TAB_ROW_HEIGHT - 2.0 * inset;
    let tabs_width: f32 = widths.iter().sum::<f32>()
        + row_data.tabs.len().saturating_sub(1) as f32 * 2.0
        + row_data.tabs.iter().filter(|tab| tab.contextual).count() as f32 * 6.0
        + if row_data.add.is_some() { 2.0 + add_size } else { 0.0 };
    // The first tab and the search field sit the same distance from the card's top, left and right
    // edges, so their outer corners are concentric with the card's corners.
    let corner = if row_data.floating { t.radius_lg.saturating_sub(inset as u8).max(t.radius_sm / 2) } else { t.control_radius.saturating_sub(1) };
    let button_widths: Vec<f32> = row_data.buttons.iter().map(|b| tab_width(ui, b.label)).collect();
    let buttons_width = if row_data.buttons.is_empty() { 0.0 } else { button_widths.iter().sum::<f32>() + 2.0 * t.space_2 };
    let search = Rect::from_min_size(
        pos2(row.right() - row_data.search_width - inset - buttons_width, row.top() + inset),
        vec2(row_data.search_width, TAB_ROW_HEIGHT - 2.0 * inset),
    );
    let mut actions = Vec::new();
    let mut x = search.right() + t.space_2;
    for (b, width) in row_data.buttons.iter().zip(button_widths) {
        let rect = Rect::from_min_size(pos2(x, row.top() + inset), vec2(width, search.height()));
        let response = crate::ui_kit::button_at(ui, b.id, rect, b.label, crate::ui_kit::Button::Enabled(b.ready.is_ok()));
        solidcraft_ui_registry::publish_target(&handle(h.action, b.id), response.rect);
        if response.clicked() {
            actions.push(TabAction::Run { id: b.id.to_string() });
        }
        if let Err(reason) = b.ready {
            response.backing_on_disabled_hover_text(reason);
        }
        x += width + t.space_2;
    }
    // The selected tab is scrolled into view when it or the row's width changes.
    let changed = ui.ctx().data_mut(|d| {
        let id = ui.scope_id().with("visible_tab");
        let state = (row_data.active.to_string(), row.width());
        let changed = d.get_temp::<(String, f32)>(id) != Some(state.clone());
        d.insert_temp(id, state);
        changed
    });
    let end = if row_data.search_width > 0.0 { search.left() - 6.0 } else { row.right() - inset };
    let span = Rect::from_min_max(row.left_top() + vec2(inset, 0.0), pos2(end.max(row.left() + inset), row.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(span), |ui| {
        ui.set_clip_rect(span);
        egui::ScrollArea::horizontal()
            .id_salt("ribbon_tabs")
            .animated(false)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                let (all, _) = ui.allocate_exact_size(vec2(tabs_width, TAB_ROW_HEIGHT), Sense::hover());
                let mut x = all.left();
                for (index, (tab, width)) in row_data.tabs.iter().zip(widths).enumerate() {
                    if tab.contextual {
                        x += 6.0;
                    }
                    let tr = Rect::from_min_size(pos2(x, all.top() + inset), vec2(width, TAB_ROW_HEIGHT - 2.0 * inset));
                    let active = row_data.active == tab.id;
                    if active && changed {
                        ui.scroll_to_rect(tr, Some(egui::Align::Center));
                    }
                    let response = ui.interact(tr, ui.scope_id().with(("tab", tab.id)), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tab.label));
                    let style = tab_style(&t, tab.accent, tab.contextual, active, response.hovered());
                    ui.painter().rect(tr, corner, style.fill, Stroke::new(1.0, style.outline), egui::StrokeKind::Inside);
                    text(ui, tr.center(), tab.label, t.font_button, style.ink, active || tab.contextual);
                    if ui.clip_rect().contains(tr.center()) {
                        solidcraft_ui_registry::publish_target(&handle(h.tab, tab.id), tr);
                        solidcraft_ui_registry::publish_rect(&handle(h.tab_rect, tab.id), tr);
                        for alias in tab.aliases {
                            solidcraft_ui_registry::publish_handle(&handle(h.tab, alias), tr.center());
                        }
                    }
                    if response.clicked() {
                        actions.push(TabAction::Select { index, already: active });
                    }
                    if response.secondary_clicked() {
                        actions.push(TabAction::Menu { at: response.interact_pointer_pos().unwrap_or(tr.left_bottom()) });
                    }
                    x += width + 2.0;
                }
                if let Some(tip) = row_data.add {
                    let pr = Rect::from_min_size(pos2(x, all.top() + inset), vec2(add_size, add_size));
                    if (row_data.menu_open_from)(pr) && !ui.rect_contains_pointer(pr) {
                        ui.painter().rect_filled(pr, t.control_radius, t.hover);
                    }
                    let response = crate::ui_kit::quiet_icon_button_at(ui, pr, "plus", tip);
                    if ui.clip_rect().contains(pr.center()) {
                        solidcraft_ui_registry::publish_target(h.add, pr);
                    }
                    if response.clicked() {
                        // A click leaves no keyboard focus here, as on the tabs, so single-key
                        // shortcuts still work afterwards.
                        response.surrender_focus();
                        actions.push(TabAction::Add { rect: pr });
                    }
                }
            });
    });
    if let Some(color) = row_data.underline {
        ui.painter().line_segment([row.left_bottom() - vec2(0.0, 1.0), row.right_bottom() - vec2(0.0, 1.0)], Stroke::new(2.0, color));
    } else {
        t.secondary_line(
            ui.painter(),
            [row.left_bottom() - vec2(0.0, 0.5), row.right_bottom() - vec2(0.0, 0.5)],
            t.toolbar.lerp_to_gamma(t.border, 0.5),
        );
    }
    TabsOut { actions, search }
}

/// A group menu's row height.
pub const ROW_H: f32 = 30.0;

/// What a click on a group menu's row or grid icon asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
    /// Run the command, from `rect`.
    Run(Rect),
    /// Its menu, at the point.
    Menu(Pos2),
    /// Pin it on the ribbon, or take it off.
    TogglePin,
}

/// A row's hover text, shown beside the menu: one line, or a line with the command's help under
/// it.
#[derive(Clone, Copy, Debug)]
pub enum RowTip<'a> {
    Plain(&'a str),
    Help { text: &'a str, help: Option<&'a str> },
}

/// A command in a group's menu: a row of the list or an icon of the grid.
#[derive(Clone, Copy, Debug)]
pub struct MenuEntry<'a> {
    pub id: &'a str,
    /// Its label, already translated.
    pub label: &'a str,
    /// A kit icon name.
    pub icon: &'a str,
    pub shortcut: &'a str,
    pub enabled: bool,
    /// A check mark, on or off (None: none).
    pub checked: Option<bool>,
    /// It is pinned on the ribbon.
    pub pinned: bool,
    /// A row's hover text.
    pub tip: Option<RowTip<'a>>,
    /// The app paints a grid icon's background itself, after the menu ([`GridCell::background`]).
    pub app_paints: bool,
}

/// How a group's menus look and where they publish their parts.
#[derive(Clone, Copy, Debug)]
pub struct MenuStyle<'a> {
    /// The pin's hover text when the command is not pinned, and when it is, translated.
    pub pin_tip: &'a str,
    pub unpin_tip: &'a str,
    /// The search's mark on a row it points out.
    pub accent: TabAccent,
    /// Where a menu's rows keep their keyboard focus, the same in every menu that shares it.
    pub focus: egui::Id,
    /// Each entry's centre, under each of these prefixes followed by its id.
    pub entry: &'a [&'a str],
    /// An entry's pin, a hot grid icon, and what the search points out.
    pub pin: &'a str,
    pub hot: &'a str,
    pub reveal: &'a str,
}

impl MenuStyle<'_> {
    /// The id an entry keeps its keyboard focus by.
    pub fn focus_id(&self, id: &str) -> egui::Id {
        self.focus.with(id)
    }
}

/// A group heading inside a menu (Edges, Faces…; Line, Rectangle…), in the list and the grid: a
/// step below the menu's own header, written as named, not in capitals, so it reads as part of
/// the menu rather than above it.
pub fn heading_font() -> FontId {
    crate::ui_kit::type_face::semibold(Tokens::get().font_micro)
}

/// A group heading in a menu's list, `label` translated.
pub fn section_heading(ui: &mut egui::Ui, label: &str, width: f32) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    ui.painter().text(rect.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, label, heading_font(), t.text_dim);
}

/// The note under a menu (how pinning works), translated.
pub fn section_footer(ui: &mut egui::Ui, message: &str) {
    let t = Tokens::get();
    ui.separator();
    ui.add_space(4.0);
    ui.add(egui::Label::new(egui::RichText::new(message).size(t.font_small).color(t.text_dim)).wrap());
    ui.add_space(4.0);
}

/// How wide a menu's rows are for these (label, shortcut) pairs: the widest, at least 200.
pub fn menu_width<'a>(ui: &egui::Ui, entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> f32 {
    let t = Tokens::get();
    let font = FontId::proportional(t.font_button);
    entries
        .into_iter()
        .map(|(label, shortcut)| {
            let label = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x;
            let key = if shortcut.is_empty() {
                0.0
            } else {
                ui.painter().layout_no_wrap(shortcut.to_string(), FontId::monospace(t.font_button), t.text).size().x + 24.0
            };
            6.0 + 20.0 + 8.0 + label + key + 30.0
        })
        .fold(200.0, f32::max)
}

/// True once the pointer has rested on this row long enough for its tip. egui's shared tooltip
/// grace time lets every row after the first show its tip the moment the pointer reaches it;
/// timing each row's own hover restarts the delay for each newly hovered row instead (#10481).
fn tip_ready(ui: &egui::Ui, resp: &egui::Response) -> bool {
    let ctx = ui.ctx();
    let key = resp.id.with("sc_dropdown_tip");
    if !resp.hovered() {
        ctx.data_mut(|d| d.remove::<(f64, u64)>(key));
        return false;
    }
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let (mut began, seen) = ctx.data(|d| d.get_temp::<(f64, u64)>(key)).unwrap_or((now, 0));
    // A menu that closed and opened again under the pointer starts the delay afresh.
    if seen + 1 < pass {
        began = now;
    }
    ctx.data_mut(|d| d.insert_temp(key, (began, pass)));
    let delay = f64::from(ui.style().interaction.tooltip_delay);
    if now - began < delay {
        ctx.request_repaint_after_secs((delay - (now - began)) as f32);
        return false;
    }
    true
}

/// Show `add` as `resp`'s tip beside the menu rather than under the pointer: to the right of the
/// menu, or to its left when the right has no room, so the tip never covers the menu's rows
/// (#10481). Returns the tip's rect when it was shown.
pub fn menu_tip(ui: &egui::Ui, menu: Rect, resp: &egui::Response, add: impl FnOnce(&mut egui::Ui)) -> Option<Rect> {
    use crate::ui_kit::{Container, Surface};
    if !tip_ready(ui, resp) {
        return None;
    }
    let ctx = ui.ctx();
    let tip_id = resp.id.with("sc_dropdown_tip_area");
    // Last pass's size; the first pass assumes the tip fits on the right.
    let size = ctx.read_response(tip_id).map(|r| r.rect.size()).unwrap_or(egui::Vec2::ZERO);
    let screen = ctx.content_rect();
    let gap = 6.0;
    let x = if menu.right() + gap + size.x <= screen.right() { menu.right() + gap } else { (menu.left() - gap - size.x).max(screen.left()) };
    let y = resp.rect.top().min((screen.bottom() - size.y).max(screen.top()));
    let area = Container::new(ctx, Surface::Popup)
        .area(tip_id)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(pos2(x, y))
        .show(ctx, |ui| Container::new(ctx, Surface::Popup).show_frame(ui, Container::menu_frame(), ui.max_rect(), add));
    Some(area.inner.response.rect)
}

/// One command row of a group's menu: icon, label and shortcut, with the hover background under
/// the pointer or keyboard focus (like every menu), and a pin at its end. `revealed` is the pulse
/// strength when the search points this row out (#7287). Returns what was chosen, with the row's
/// response (for the app's drags and tips).
pub fn menu_row(ui: &mut egui::Ui, entry: &MenuEntry, width: f32, revealed: Option<f32>, style: &MenuStyle) -> (Option<MenuAction>, egui::Response) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    // Every row drags onto the ribbon (to pin it there); enabled rows also start their command.
    let sense = if entry.enabled { Sense::click_and_drag() } else { Sense::drag() };
    let resp = ui.interact(rect, style.focus_id(entry.id), sense);
    // Focus can settle on the following frame, after the key event has gone.
    // Keep the focused row visible even then in a menu taller than the window.
    if resp.has_focus() && !ui.clip_rect().contains_rect(rect) {
        resp.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    if resp.drag_started() {
        resp.dnd_set_drag_payload(DragPayload { id: entry.id.to_string(), from_menu: true });
    }
    // The pin at the row's end (above the row, so it takes its own clicks).
    let pr = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(20.0, 20.0));
    let pin = ui.interact(pr, ui.scope_id().with(("dropdown_pin", entry.id)), Sense::click());
    let pointer_on = (resp.hovered() || pin.hovered()) && !egui::DragAndDrop::has_any_payload(ui.ctx());
    // A checkable row announces its on/off state, since its label no longer says it.
    resp.widget_info(|| match entry.checked {
        Some(on) => egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled() && entry.enabled, on, entry.label),
        None => egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled() && entry.enabled, entry.label),
    });
    for prefix in style.entry {
        solidcraft_ui_registry::publish_handle(&handle(prefix, entry.id), rect.center());
    }
    let hot = entry.enabled && (pointer_on || resp.has_focus());
    let mut row = crate::ui_kit::MenuItem::new(entry.id, entry.label, entry.icon);
    row.enabled = entry.enabled;
    row.checked = entry.checked;
    row.highlighted = hot || revealed.is_some();
    row.shortcut = entry.shortcut;
    row.paint(ui, rect, &resp, 26.0);
    if let Some(strength) = revealed {
        paint_reveal(ui.painter(), rect, &style.accent, strength, false);
        solidcraft_ui_registry::publish_rect(style.reveal, rect);
        if !ui.clip_rect().contains_rect(rect) {
            ui.scroll_to_rect(rect, None);
        }
    }
    if hot {
        solidcraft_ui_registry::publish_handle(&handle(style.hot, entry.id), rect.center());
    }
    let p = ui.painter();
    if pointer_on || resp.has_focus() {
        if pin.hovered() {
            p.rect_filled(pr, t.radius_sm.saturating_sub(1), t.border.gamma_multiply(0.6));
        }
        crate::ui_kit::icons::glyphs::pin_icon(p, pr, entry.pinned, if entry.pinned { t.accent } else { t.text_dim });
        solidcraft_ui_registry::publish_handle(&handle(style.pin, entry.id), pr.center());
    }
    let tip = if entry.pinned { style.unpin_tip } else { style.pin_tip };
    // The menu's rect from the last pass: tips sit beside it, never over its rows (#10481).
    let menu = egui::containers::AreaState::load(ui.ctx(), ui.layer_id().id).map(|state| state.rect()).unwrap_or(rect);
    if pin.clicked() {
        return (Some(MenuAction::TogglePin), resp);
    }
    menu_tip(ui, menu, &pin, |ui| {
        ui.label(tip);
    });
    match entry.tip {
        Some(RowTip::Plain(text)) => {
            menu_tip(ui, menu, &resp, move |ui| {
                ui.label(text);
            });
        }
        Some(RowTip::Help { text, help }) => {
            menu_tip(ui, menu, &resp, move |ui| {
                ui.set_max_width(300.0);
                ui.label(text);
                if let Some(help) = help {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(help).weak());
                }
            });
        }
        None => {}
    }
    if resp.secondary_clicked() {
        let at = resp.interact_pointer_pos().unwrap_or(rect.left_bottom());
        return (Some(MenuAction::Menu(at)), resp);
    }
    if resp.clicked() && entry.enabled {
        return (Some(MenuAction::Run(rect)), resp);
    }
    (None, resp)
}

/// Up and Down move the keyboard focus through a list's enabled rows (`enabled`, their ids, in
/// order; from none focused: the first or the last), instead of egui's own spatial move. A
/// submenu draws before the rest of its parent: a key is handled once per pass among the menus
/// that share `style.focus`.
pub fn list_keys(ui: &egui::Ui, enabled: &[&str], style: &MenuStyle) {
    let (down, up) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp)));
    if !down && !up {
        return;
    }
    let handled = style.focus.with("key_pass");
    let pass = ui.ctx().cumulative_pass_nr();
    if ui.ctx().data(|d| d.get_temp::<u64>(handled)) == Some(pass) {
        return;
    }
    ui.ctx().data_mut(|d| d.insert_temp(handled, pass));
    if enabled.is_empty() {
        return;
    }
    let focused = ui.ctx().memory(|m| m.focused());
    let at = enabled.iter().position(|id| focused == Some(style.focus_id(id)));
    let n = enabled.len();
    let next = match (at, down) {
        (None, true) => 0,
        (None, false) => n - 1,
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
    };
    if let Some(id) = enabled.get(next) {
        ui.ctx().memory_mut(|m| {
            m.request_focus(style.focus_id(id));
            m.move_focus(egui::FocusDirection::None);
        });
    }
}

/// A grid menu's cell (#4739): the icon twice the size of a list row's, with room around it.
pub const GRID_CELL: f32 = 48.0;
const GRID_ICON: f32 = 32.0;
/// Columns a grid fills before a second row; a group with fewer commands is as wide as it needs.
pub const GRID_COLUMNS: usize = 5;
/// Rows at most: a bigger menu grows to the right instead (unless the window is too narrow).
pub const GRID_ROWS: usize = 3;
/// The height of a group's heading in a grid menu.
pub const GRID_HEADING: f32 = 24.0;
/// Icons side by side in a group's column, when groups stand side by side.
pub const GROUP_COLUMNS: usize = 2;

/// Where a grid menu puts things, relative to its top left: each entry's icon with its (row,
/// column) for the arrow keys, each group heading (where, how wide, its text), the separators
/// between groups side by side (their x), and the whole size.
#[derive(Debug)]
pub struct GridLayout<'a> {
    pub cells: Vec<(Rect, (usize, usize))>,
    pub headings: Vec<(Pos2, f32, &'a str)>,
    pub separators: Vec<f32>,
    pub size: egui::Vec2,
}

/// Lay out a grid menu, at most [`GRID_ROWS`] high, growing to the right. `grouped` is each
/// entry's id with the heading that starts its group. A menu of two or more headed groups puts
/// the groups side by side, each heading over a column of its icons, [`GROUP_COLUMNS`] across
/// (more for a group too big for three rows), when that fits in `room` (wide). Any other menu, or
/// one too wide for the window, fills rows: [`GRID_COLUMNS`] across, more when that would take
/// over three rows, fewer when the window is narrower; a heading starts a new row. `loose` names
/// the entries in no group: they stand apart after the groups, without a heading, so they stay
/// put when a group is left out.
pub fn grid_layout<'a>(grouped: &[(Option<&'a str>, &str)], loose: &[&str], room: f32) -> GridLayout<'a> {
    let mut groups: Vec<(Option<&'a str>, Vec<usize>)> = Vec::new();
    for (i, (heading, id)) in grouped.iter().enumerate() {
        let after_a_group = groups.last().is_some_and(|(h, _)| h.is_some());
        if heading.is_some() || groups.is_empty() || (after_a_group && loose.contains(id)) {
            groups.push((*heading, Vec::new()));
        }
        if let Some((_, items)) = groups.last_mut() {
            items.push(i);
        }
    }
    // A heading needs two icons' width; loose entries only as many columns as they fill.
    let group_columns = |heading: Option<&str>, n: usize| n.div_ceil(GRID_ROWS).max(if heading.is_some() { GROUP_COLUMNS } else { 1 });
    let headed = groups.iter().filter(|(h, _)| h.is_some()).count();
    let side_columns: usize = groups.iter().map(|(h, items)| group_columns(*h, items.len())).sum();
    let side_by_side = headed >= 2 && side_columns as f32 * GRID_CELL <= room;
    let mut cells = vec![(Rect::NOTHING, (0, 0)); grouped.len()];
    let mut headings = Vec::new();
    let mut separators = Vec::new();
    let cell = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(GRID_CELL, GRID_CELL));
    let size = if side_by_side {
        let (mut first_column, mut rows) = (0, 0);
        for (g, (heading, items)) in groups.iter().enumerate() {
            let columns = group_columns(*heading, items.len());
            let x = first_column as f32 * GRID_CELL;
            if g > 0 {
                separators.push(x);
            }
            if let Some(h) = heading {
                headings.push((pos2(x, 0.0), columns as f32 * GRID_CELL, *h));
            }
            for (k, i) in items.iter().enumerate() {
                let (row, column) = (k / columns, k % columns);
                if let Some(slot) = cells.get_mut(*i) {
                    *slot = (cell(x + column as f32 * GRID_CELL, GRID_HEADING + row as f32 * GRID_CELL), (row, first_column + column));
                }
            }
            rows = rows.max(items.len().div_ceil(columns));
            first_column += columns;
        }
        vec2(first_column as f32 * GRID_CELL, GRID_HEADING + rows as f32 * GRID_CELL)
    } else {
        let n = grouped.len();
        let fit = ((room / GRID_CELL).floor() as usize).max(1);
        let columns = n.div_ceil(GRID_ROWS).max(n.min(GRID_COLUMNS)).min(fit).max(1);
        let (mut y, mut first_row) = (0.0, 0);
        for (heading, items) in &groups {
            if let Some(h) = heading {
                headings.push((pos2(0.0, y), columns as f32 * GRID_CELL, *h));
                y += GRID_HEADING;
            }
            for (k, i) in items.iter().enumerate() {
                let (row, column) = (k / columns, k % columns);
                if let Some(slot) = cells.get_mut(*i) {
                    *slot = (cell(column as f32 * GRID_CELL, y + row as f32 * GRID_CELL), (first_row + row, column));
                }
            }
            let rows = items.len().div_ceil(columns);
            y += rows as f32 * GRID_CELL;
            first_row += rows;
        }
        vec2(columns as f32 * GRID_CELL, y)
    };
    GridLayout { cells, headings, separators, size }
}

/// An arrow key in a grid menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridKey {
    Left,
    Right,
    Up,
    Down,
}

/// Where an arrow key moves the focus in a grid menu, as an index into `enabled` (the enabled
/// entries, as indices into `cells`): Left and Right step through them in order, Up and Down to
/// the nearest one in the row above or below, all wrapping. From none focused, Right and Down go
/// to the first, Left and Up to the last.
pub fn grid_step(cells: &[(usize, usize)], enabled: &[usize], at: Option<usize>, key: GridKey) -> Option<usize> {
    let n = enabled.len();
    if n == 0 {
        return None;
    }
    let Some(at) = at.filter(|at| *at < n) else {
        return Some(if matches!(key, GridKey::Right | GridKey::Down) { 0 } else { n - 1 });
    };
    let cell = |k: usize| enabled.get(k).and_then(|i| cells.get(*i)).copied();
    match key {
        GridKey::Right => Some((at + 1) % n),
        GridKey::Left => Some((at + n - 1) % n),
        GridKey::Up | GridKey::Down => {
            let (row, column) = cell(at)?;
            let rows: Vec<usize> = (0..n).filter_map(|k| cell(k).map(|c| c.0)).collect();
            let target = if key == GridKey::Down {
                rows.iter().copied().filter(|r| *r > row).min().or_else(|| rows.iter().copied().min())
            } else {
                rows.iter().copied().filter(|r| *r < row).max().or_else(|| rows.iter().copied().max())
            }?;
            (0..n)
                .filter_map(|k| cell(k).filter(|c| c.0 == target).map(|c| (k, c.1.abs_diff(column))))
                .min_by_key(|(_, distance)| *distance)
                .map(|(k, _)| k)
        }
    }
}

/// Arrow keys move the keyboard focus through a grid menu's enabled icons ([`grid_step`]).
fn grid_keys(ui: &egui::Ui, entries: &[MenuEntry], cells: &[(usize, usize)], style: &MenuStyle) {
    let key = ui.input(|i| {
        [
            (egui::Key::ArrowLeft, GridKey::Left),
            (egui::Key::ArrowRight, GridKey::Right),
            (egui::Key::ArrowUp, GridKey::Up),
            (egui::Key::ArrowDown, GridKey::Down),
        ]
        .into_iter()
        .find(|(k, _)| i.key_pressed(*k))
        .map(|(_, key)| key)
    });
    let Some(key) = key else { return };
    let enabled: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.enabled).map(|(i, _)| i).collect();
    let focused = ui.ctx().memory(|m| m.focused());
    let at = enabled.iter().position(|i| entries.get(*i).is_some_and(|e| focused == Some(style.focus_id(e.id))));
    if let Some(e) = grid_step(cells, &enabled, at, key).and_then(|k| enabled.get(k)).and_then(|i| entries.get(*i)) {
        ui.ctx().memory_mut(|m| {
            m.request_focus(style.focus_id(e.id));
            m.move_focus(egui::FocusDirection::None);
        });
    }
}

/// A grid icon the menu drew, for the app's own part of it (hover text, drags, deferred paint).
pub struct GridCell {
    /// Its place among the entries.
    pub index: usize,
    pub rect: Rect,
    pub response: egui::Response,
    /// An `app_paints` entry's paint, for [`paint_deferred`].
    pub deferred: Option<Deferred>,
}

/// The grid drawn: the entry chosen and how, and each icon drawn.
pub struct GridOut {
    pub action: Option<(usize, MenuAction)>,
    pub cells: Vec<GridCell>,
}

/// How long the pointer rests on a grid icon before its corner pin can be clicked (seconds).
pub const PIN_DELAY: f64 = 2.0;

/// Whether the corner pin of grid icon `id` takes clicks: the pointer (`on`) has rested on the
/// icon for [`PIN_DELAY`]. One icon is timed at a time; the window redraws when the pin is due.
fn pin_armed(ui: &egui::Ui, key: egui::Id, id: &str, on: bool) -> bool {
    let now = ui.input(|i| i.time);
    let since = ui.ctx().data_mut(|d| {
        let rest = d.get_temp::<(String, f64)>(key).filter(|(at, _)| at == id);
        match (on, rest) {
            (true, Some((_, since))) => Some(since),
            (true, None) => {
                d.insert_temp(key, (id.to_string(), now));
                Some(now)
            }
            (false, Some(_)) => {
                d.remove::<(String, f64)>(key);
                None
            }
            (false, None) => None,
        }
    });
    let Some(since) = since else { return false };
    let left = PIN_DELAY - (now - since);
    if left > 0.0 {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left));
    }
    left <= 0.0
}

/// A group's menu drawn as a grid of large icons (#4739): the same entries, headings (translated)
/// and actions as the list, the names in the tooltips. `loose` as for [`grid_layout`].
pub fn grid(ui: &mut egui::Ui, entries: &[(Option<&str>, MenuEntry)], loose: &[&str], style: &MenuStyle) -> GridOut {
    // Wide enough for groups side by side, short of the window's edge.
    let room = ui.ctx().content_rect().right() - ui.cursor().left() - 24.0;
    let grouped: Vec<(Option<&str>, &str)> = entries.iter().map(|(heading, e)| (*heading, e.id)).collect();
    let layout = grid_layout(&grouped, loose, room);
    let width = layout.size.x.max(200.0);
    // A narrow grid sits in the middle of the menu's least width.
    let inset = ((width - layout.size.x) / 2.0).max(0.0);
    ui.set_width(width);
    let plain: Vec<MenuEntry> = entries.iter().map(|(_, e)| *e).collect();
    let keys: Vec<(usize, usize)> = layout.cells.iter().map(|(_, at)| *at).collect();
    grid_keys(ui, &plain, &keys, style);
    // Like the list, the grid scrolls when it would run past the window's bottom (#874).
    let height = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 64.0).max(GRID_CELL * 2.0);
    let mut out = GridOut { action: None, cells: Vec::new() };
    egui::ScrollArea::vertical().id_salt("panel_menu_grid").max_height(height).min_scrolled_height(height).show(ui, |ui| {
        let (area, _) = ui.allocate_exact_size(vec2(width, layout.size.y), Sense::hover());
        let origin = area.left_top() + vec2(inset, 0.0);
        let t = Tokens::get();
        for (at, w, heading) in &layout.headings {
            let clip = ui.clip_rect().intersect(Rect::from_min_size(origin + at.to_vec2(), vec2(*w, GRID_HEADING)));
            let painter = ui.painter().with_clip_rect(clip);
            // Centred over its group's icons.
            let centre = origin + at.to_vec2() + vec2(w / 2.0, GRID_HEADING / 2.0);
            painter.text(centre, egui::Align2::CENTER_CENTER, *heading, heading_font(), t.text_dim);
        }
        // A line between groups side by side, as between the ribbon's groups.
        for x in &layout.separators {
            let x = origin.x + x;
            ui.painter().line_segment([pos2(x, origin.y + 4.0), pos2(x, origin.y + layout.size.y - 4.0)], Stroke::new(1.0, t.border));
        }
        for (index, (entry, (rect, _))) in plain.iter().zip(&layout.cells).enumerate() {
            let rect = rect.translate(origin.to_vec2());
            let (action, cell) = grid_cell(ui, entry, rect, style);
            out.cells.push(GridCell { index, ..cell });
            if let Some(action) = action {
                out.action = Some((index, action));
            }
        }
    });
    out
}

/// One icon of a grid menu at `rect`: clicked, dragged or right-clicked like a list row; a pin in
/// its corner says it is on the ribbon. Returns what was chosen, with the icon drawn.
pub fn grid_cell(ui: &mut egui::Ui, entry: &MenuEntry, rect: Rect, style: &MenuStyle) -> (Option<MenuAction>, GridCell) {
    let t = Tokens::get();
    let sense = if entry.enabled { Sense::click_and_drag() } else { Sense::drag() };
    let resp = ui.interact(rect, style.focus_id(entry.id), sense);
    if resp.has_focus() && !ui.clip_rect().contains_rect(rect) {
        resp.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    if resp.drag_started() {
        resp.dnd_set_drag_payload(DragPayload { id: entry.id.to_string(), from_menu: true });
    }
    // The pin in the corner, as at a list row's end. It takes clicks (above the cell) only once
    // the pointer has rested on the icon for PIN_DELAY: until then a click anywhere runs the
    // command, so a quick click near the corner never pins or unpins it by accident.
    let pr = Rect::from_min_size(pos2(rect.right() - 15.0, rect.top()), vec2(15.0, 15.0));
    let dragging = egui::DragAndDrop::has_any_payload(ui.ctx());
    let armed = pin_armed(ui, style.focus.with("grid_pin_hover"), entry.id, ui.rect_contains_pointer(rect) && !dragging);
    let pin = armed.then(|| ui.interact(pr, ui.scope_id().with(("dropdown_pin", entry.id)), Sense::click()));
    let pin_hovered = pin.as_ref().is_some_and(egui::Response::hovered);
    if !entry.app_paints {
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled() && entry.enabled, entry.label));
    }
    for prefix in style.entry {
        solidcraft_ui_registry::publish_handle(&handle(prefix, entry.id), rect.center());
    }
    let pointer_on = (resp.hovered() || pin_hovered) && !dragging;
    let hot = entry.enabled && (pointer_on || resp.has_focus());
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    let cell = rect.shrink(2.0);
    let mut neutral = Vec::new();
    if hot {
        neutral.push(egui::Shape::rect_filled(cell, t.control_radius, t.accent_soft));
        solidcraft_ui_registry::publish_handle(&handle(style.hot, entry.id), rect.center());
    }
    if resp.has_focus() && !entry.app_paints {
        neutral.push(egui::Shape::rect_stroke(cell, t.control_radius, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside));
    }
    let neutral = egui::Shape::Vec(neutral);
    let deferred = if entry.app_paints {
        // Only responses and paint slots survive until the app paints them.
        let (background, label_slot) = (painter.add(egui::Shape::Noop), painter.add(egui::Shape::Noop));
        Some(Deferred { response: resp.clone(), painter: painter.clone(), rect: cell, background, label_slot, neutral })
    } else {
        painter.add(neutral);
        None
    };
    crate::ui_kit::icons::paint_icon(
        &painter,
        Rect::from_center_size(rect.center(), vec2(GRID_ICON, GRID_ICON)),
        entry.icon,
        if entry.enabled { 1.0 } else { 0.4 },
    );
    // A pinned entry always shows its pin, clear of the icon's own artwork; any other shows one
    // to pin it once it can be clicked.
    if entry.pinned || armed {
        if pin_hovered {
            painter.rect_filled(pr, 3.0, t.border.gamma_multiply(0.6));
        }
        crate::ui_kit::icons::glyphs::pin_icon(&painter, pr.shrink(2.5), entry.pinned, if entry.pinned { t.accent } else { t.text_dim });
    }
    if let Some(pin) = pin {
        solidcraft_ui_registry::publish_handle(&handle(style.pin, entry.id), pr.center());
        let tip = if entry.pinned { style.unpin_tip } else { style.pin_tip };
        if pin.backing_on_hover_text(tip).clicked() {
            return (Some(MenuAction::TogglePin), GridCell { index: 0, rect, response: resp, deferred });
        }
    }
    let action = if resp.secondary_clicked() {
        Some(MenuAction::Menu(resp.interact_pointer_pos().unwrap_or(rect.left_bottom())))
    } else if resp.clicked() && entry.enabled {
        Some(MenuAction::Run(rect))
    } else {
        None
    };
    (action, GridCell { index: 0, rect, response: resp, deferred })
}

/// A command being dragged: off a button or cell of the row, or out of a menu.
type DragPayload = crate::ui_kit::ReorderPayload<String>;

/// A command is being dragged along the ribbon or out of a menu.
pub fn dragging(ctx: &egui::Context) -> bool {
    egui::DragAndDrop::has_payload_of_type::<DragPayload>(ctx)
}

/// A row button's or cell's part in dragging: it starts a drag, shows where a command dragged
/// over a button (`marker`: its rect) goes, and takes one dropped on it.
fn drag_onto(painter: &egui::Painter, resp: &egui::Response, id: &str, marker: Option<Rect>, group: usize, actions: &mut Vec<Action>) {
    if resp.drag_started() {
        resp.dnd_set_drag_payload(DragPayload { id: id.to_string(), from_menu: false });
    }
    if let Some(at) = marker
        && let Some(dragged) = resp.dnd_hover_payload::<DragPayload>()
        && dragged.id != id
    {
        crate::ui_kit::insertion_marker(painter, at.left() - 1.0, at.top() + 2.0, at.bottom() - 2.0);
    }
    if let Some(dragged) = resp.dnd_release_payload::<DragPayload>()
        && dragged.id != id
    {
        actions.push(Action::Drop { id: dragged.id.clone(), group, before: Some(id.to_string()) });
    }
}

/// The end of a ribbon pass while a command is dragged: its icon (`icon_of` its id) follows the
/// pointer, crossed out when releasing it here removes it from the row: a row button dragged off
/// `ribbon` or onto the open `menu`. On release (with nothing `dropped` this pass) the drag ends;
/// returns the command removed.
pub fn drag_end(
    ui: &egui::Ui,
    ribbon: Rect,
    menu: Option<Rect>,
    dropped: bool,
    icon_of: &dyn Fn(&str) -> Option<String>,
    handles: &Handles,
) -> Option<String> {
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let (Some(d), Some(p)) = (egui::DragAndDrop::payload::<DragPayload>(ui.ctx()), pointer) else { return None };
    let leaves = !ribbon.contains(p) || menu.is_some_and(|menu| menu.contains(p));
    let removing = !d.from_menu && leaves;
    if let Some(icon) = icon_of(&d.id) {
        drag_ghost(ui, &icon, p, removing, handles);
    }
    if ui.input(|i| i.pointer.any_released()) && !dropped {
        egui::DragAndDrop::clear_payload(ui.ctx());
        if removing {
            return Some(d.id.clone());
        }
    }
    None
}

/// The dragged command's icon under the pointer, crossed out when releasing removes it.
fn drag_ghost(ui: &egui::Ui, icon: &str, at: Pos2, removing: bool, handles: &Handles) {
    let t = Tokens::get();
    let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("ribbon_drag_ghost")));
    let r = Rect::from_center_size(at + vec2(14.0, 16.0), vec2(30.0, 30.0));
    p.rect(r, t.radius_sm, t.toolbar.gamma_multiply(0.92), Stroke::new(1.0, if removing { t.error } else { t.accent }), egui::StrokeKind::Inside);
    crate::ui_kit::icons::paint_icon(&p, r.shrink(5.0), icon, f32::from(t.icon.a()) / 255.0);
    if removing {
        let s = Stroke::new(2.0, t.error);
        let x = Rect::from_center_size(r.right_top(), vec2(9.0, 9.0));
        p.circle_filled(x.center(), 7.0, t.toolbar);
        crate::ui_kit::icons::glyphs::cross(&p, x, s);
    }
    solidcraft_ui_registry::publish_handle(if removing { handles.drag_remove } else { handles.drag }, at);
}

/// A button or grid icon the app paints itself after every command of the pass has run, so a
/// state its click changed shows in the same frame: its response, the painter it was drawn
/// with (its layer, clip and opacity), its rect, its shape slots for background and label, and
/// its background as the row would have drawn it.
pub struct Deferred {
    pub response: egui::Response,
    pub painter: egui::Painter,
    pub rect: Rect,
    pub background: egui::layers::ShapeIdx,
    pub label_slot: egui::layers::ShapeIdx,
    pub neutral: egui::Shape,
}

/// How a deferred button looks now, as the app resolves it after its commands ran.
#[derive(Clone, Debug)]
pub struct DeferredLook {
    /// Its command id, in handles.
    pub id: String,
    /// Its effect is on: drawn pressed, its label semibold.
    pub selected: bool,
    /// Its tool runs (outlined in the accent) and it keeps a focus ring while focused: the
    /// pressed presentation is on.
    pub running: bool,
    pub focus_ring: bool,
    pub label: Option<String>,
    pub enabled: bool,
    /// Its accessible name, and whether it announces itself on or off.
    pub name: String,
    pub checkable: bool,
    pub accent: TabAccent,
}

/// Paints the deferred buttons as they look now. `namespace` prefixes what is published: each
/// pressed button (`<namespace>:pressed:<id>`), the focused one (`<namespace>:focus:<id>`) and
/// how many are pressed (`<namespace>:pressed`).
pub fn paint_deferred(ui: &egui::Ui, items: Vec<(Deferred, DeferredLook)>, namespace: &str) {
    let t = Tokens::get();
    let mut count = 0;
    for (paint, look) in items {
        let ink = if !look.enabled {
            t.text_dim
        } else if look.selected {
            t.selected_ink
        } else {
            t.text
        };
        let neutral = if look.running {
            egui::Shape::Rect(egui::epaint::RectShape::new(
                paint.rect,
                t.radius_sm,
                look.accent.soft,
                Stroke::new(1.0, look.accent.color),
                egui::StrokeKind::Inside,
            ))
        } else {
            paint.neutral
        };
        let background = if look.selected {
            count += 1;
            solidcraft_ui_registry::publish_target(&format!("{namespace}:pressed:{}", look.id), paint.rect);
            crate::ui_kit::persistent_button_shape(ui, &paint.response, paint.rect)
        } else if paint.response.has_focus() && look.focus_ring {
            egui::Shape::Vec(vec![neutral, crate::ui_kit::native_button_focus(ui, &paint.response, paint.rect)])
        } else {
            neutral
        };
        paint.painter.set(paint.background, background);
        if paint.response.has_focus() {
            solidcraft_ui_registry::publish_target(&format!("{namespace}:focus:{}", look.id), paint.rect);
        }
        if let Some(label) = &look.label {
            let galley = button_galley_weight(&paint.painter, label, paint.rect.width() - BUTTON_LABEL_PADDING, ink, look.selected);
            let at = pos2(paint.rect.center().x, label_top(paint.rect, galley.size().y));
            paint.painter.set(paint.label_slot, egui::Shape::galley(at, galley, ink));
        }
        paint.response.widget_info(|| {
            if look.checkable {
                egui::WidgetInfo::selected(egui::Role::Button, look.enabled, look.selected, &look.name)
            } else {
                egui::WidgetInfo::labeled(egui::Role::Button, look.enabled, &look.name)
            }
        });
    }
    solidcraft_ui_registry::publish_count(&format!("{namespace}:pressed"), count as f64);
}

#[cfg(test)]
#[path = "ribbon_tests.rs"]
mod tests;
