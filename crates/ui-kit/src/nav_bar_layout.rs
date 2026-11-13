//! The navigation bar's layout: where each stored slot sits on a canvas or in the customiser's
//! stage, and what folds into More. The host passes which slots its canvas allows and how wide
//! each label is; [`crate::nav_bar`] draws the result.
use crate::navigation_bar::{self, Labels, MenuDisplay, NavigationBar, Position, Slot};
use crate::{
    NAV_BORDER, NAV_BOTTOM, NAV_CHEVRON, NAV_EDGE, NAV_GAP, NAV_LABEL_GAP, NAV_LABEL_PADDING, NAV_MENU_GAP, NAV_MENU_LEFT, NAV_MENU_RIGHT,
    NAV_PADDING, NAV_SEPARATOR, NAV_STAGE_HEIGHT, NAV_STAGE_INSET, NavigationEdge, NavigationKind, NavigationMetrics,
};
use egui::{Rect, pos2, vec2};

/// A measured bar: its outline, the slots on it with their buttons, the slots folded into More
/// and More's button, and whether labels and menu values fit.
#[derive(Clone, Debug, PartialEq)]
pub struct BarLayout {
    pub rect: Rect,
    pub slots: Vec<(&'static Slot, Rect)>,
    pub overflow: Vec<&'static Slot>,
    pub more: Rect,
    pub labels: bool,
    pub values: bool,
}

/// The bar `prefs` on `content`, with the stored slots `shown` keeps (those the canvas allows)
/// and each slot's label or menu value `label_width` wide. Drop menu values, then labels, before
/// moving a suffix into More. All rectangles use logical UI points.
pub fn measure(content: Rect, prefs: &NavigationBar, shown: impl Fn(&Slot) -> bool, label_width: impl Fn(&Slot) -> f32) -> BarLayout {
    measure_row(content, prefs, shown, label_width, false)
}

fn measure_row(content: Rect, prefs: &NavigationBar, shown: impl Fn(&Slot) -> bool, label_width: impl Fn(&Slot) -> f32, editing: bool) -> BarLayout {
    let metrics = NavigationMetrics::for_size(prefs.size);
    let button = prefs.size.points();
    let inset = NAV_BORDER + NAV_PADDING;
    let mut result = BarLayout { rect: Rect::NOTHING, slots: Vec::new(), overflow: Vec::new(), more: Rect::NOTHING, labels: false, values: true };
    let height = metrics.height();
    let available = content.width() - 2.0 * NAV_EDGE;
    if !content.is_finite() || available < button + 2.0 * inset || content.height() < height + NAV_BOTTOM {
        return result;
    }
    let slots: Vec<_> =
        prefs.slots.iter().take(navigation_bar::MAX_SLOTS).filter_map(|id| navigation_bar::find(id)).filter(|slot| shown(slot)).collect();
    if slots.is_empty() {
        return result;
    }
    let width = |slot: &Slot, labels, values| {
        if slot.kind == NavigationKind::Separator {
            NAV_SEPARATOR
        } else if slot.kind == NavigationKind::Menu {
            metrics.icon
                + if editing { 2.0 * NAV_BORDER } else { 0.0 }
                + NAV_MENU_LEFT
                + NAV_MENU_RIGHT
                + NAV_CHEVRON
                + NAV_MENU_GAP
                + if values && prefs.menu_display(slot.id) == MenuDisplay::IconAndValue { label_width(slot).max(0.0) + NAV_MENU_GAP } else { 0.0 }
        } else if labels {
            (metrics.icon + NAV_LABEL_GAP + label_width(slot).max(0.0) + 2.0 * NAV_LABEL_PADDING).max(metrics.button)
        } else {
            metrics.button
        }
    };
    let total = |labels, values| {
        slots.iter().map(|slot| width(slot, labels, values)).sum::<f32>() + NAV_GAP * slots.len().saturating_sub(1) as f32 + 2.0 * inset
    };
    result.labels = prefs.labels == Labels::Labels;
    if total(result.labels, true) > available {
        result.values = false;
    }
    if total(result.labels, result.values) > available {
        result.labels = false;
    }
    let mut visible = slots.len();
    let mut row_width = total(result.labels, result.values);
    if row_width > available {
        visible = 0;
        row_width = metrics.button + 2.0 * inset;
        for slot in &slots {
            let next = row_width + width(slot, false, false) + NAV_GAP;
            if next > available {
                break;
            }
            row_width = next;
            visible += 1;
        }
        result.overflow.extend(slots.iter().skip(visible).copied());
    }
    let left = match prefs.position {
        Position::Centre => content.center().x - row_width / 2.0,
        Position::Left => content.left() + NAV_EDGE,
        Position::Right => content.right() - NAV_EDGE - row_width,
    };
    // The same gap from either edge of the view.
    let top = match prefs.edge {
        NavigationEdge::Bottom => content.bottom() - height - NAV_BOTTOM,
        NavigationEdge::Top => content.top() + NAV_BOTTOM,
    };
    result.rect = Rect::from_min_size(pos2(left, top), vec2(row_width, height));
    let mut x = left + inset;
    for slot in slots.iter().take(visible) {
        let w = width(slot, result.labels, result.values);
        result.slots.push((*slot, Rect::from_min_size(pos2(x, result.rect.top() + inset), vec2(w, metrics.button))));
        x += w + NAV_GAP;
    }
    if !result.overflow.is_empty() {
        result.more = Rect::from_min_size(pos2(x, result.rect.top() + inset), vec2(metrics.button, metrics.button));
    }
    result
}

/// The bar `prefs` in the customiser's `stage`. Editing retains every stored catalogue slot,
/// including those the canvas would hide.
pub fn editor_layout(stage: Rect, prefs: &NavigationBar, label_width: impl Fn(&Slot) -> f32) -> BarLayout {
    let virtual_canvas = Rect::from_min_size(stage.min, vec2(8192.0, NAV_STAGE_HEIGHT));
    let mut bar = measure_row(virtual_canvas, prefs, |_| true, label_width, true);
    if !bar.rect.is_positive() {
        let metrics = NavigationMetrics::for_size(prefs.size);
        bar.rect = Rect::from_min_size(stage.min, vec2(metrics.button + 2.0 * (NAV_PADDING + NAV_BORDER), metrics.height()));
    }
    let x = match prefs.position {
        Position::Left => stage.left() + NAV_BORDER + NAV_STAGE_INSET,
        Position::Centre => stage.center().x - bar.rect.width() / 2.0,
        Position::Right => stage.right() - NAV_BORDER - NAV_STAGE_INSET - bar.rect.width(),
    };
    // The same inset from the top or bottom edge as from the side, as in the live view.
    let y = match prefs.edge {
        NavigationEdge::Top => stage.top() + NAV_BORDER + NAV_STAGE_INSET,
        NavigationEdge::Bottom => stage.bottom() - NAV_BORDER - NAV_STAGE_INSET - bar.rect.height(),
    };
    let delta = pos2(x, y) - bar.rect.min;
    bar.rect = bar.rect.translate(delta);
    for (_, rect) in &mut bar.slots {
        *rect = rect.translate(delta);
    }
    if bar.more.is_positive() {
        bar.more = bar.more.translate(delta);
    }
    bar
}
