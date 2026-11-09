//! The ribbon: groups of command buttons under a row of tabs. This module holds what the panel
//! row is laid out with: how groups fit a ribbon too narrow for all their buttons ([`fit`]), and
//! how a button's label is broken into lines and measured.

use egui::{Color32, FontId, Rect};

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
        Color32::WHITE,
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

#[cfg(test)]
#[path = "ribbon_tests.rs"]
mod tests;
