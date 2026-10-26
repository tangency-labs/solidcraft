//! The length of the selected edges, at the bottom left of the viewport (#6560): select one or
//! more model edges or sketch curves and their length shows without opening Measure. A circle
//! picked as the two kernel halves of its rim counts once.

use std::collections::BTreeSet;
use std::sync::Arc;

use egui::{FontId, Painter, Rect, pos2, vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::ModelState;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::theme::Tokens;

/// More picks than this aren't summed every time the selection changes.
const MAX_ITEMS: usize = 500;

/// The readout measured for a selection, kept while the selection and the model stay the same.
#[derive(Clone)]
struct Cached {
    selection: Vec<Sel>,
    state: Arc<ModelState>,
    text: Option<String>,
}

fn polyline_len(pts: &[Vec3]) -> f64 {
    pts.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).sum()
}

/// The total length of `selection` and how many edges or curves it is, when the selection is only
/// edges and sketch curves. Nothing for an empty selection or one with anything else in it.
pub(crate) fn measure(app: &SolveApp, st: &ModelState, selection: &[Sel]) -> Option<(f64, usize)> {
    if selection.is_empty() || selection.len() > MAX_ITEMS {
        return None;
    }
    let mut edges = BTreeSet::new();
    let mut count = 0;
    let mut total = 0.0;
    for sel in selection {
        match sel {
            Sel::Edge { body, index, .. } => {
                let m = st.body(body)?.mesh();
                // A circle's rim is one edge to the user, however many kernel edges make it.
                let rim = m.circular_rim(*index);
                let rim = if rim.is_empty() { vec![*index] } else { rim };
                if rim.iter().any(|i| edges.contains(&(body.as_str(), *i))) {
                    continue;
                }
                count += 1;
                for i in rim {
                    edges.insert((body.as_str(), i));
                    total += polyline_len(m.edges.get(i)?);
                }
            }
            Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. } => {
                let measured = app.session.measure_items(std::slice::from_ref(sel));
                total += measured.get("items")?.get(0)?.get("length_mm")?.as_f64()?;
                count += 1;
            }
            _ => return None,
        }
    }
    (total.is_finite() && count > 0).then_some((total, count))
}

/// The readout's text for the current selection, or nothing to show.
pub(crate) fn text(app: &SolveApp) -> Option<String> {
    let (length, count) = measure(app, &app.session.world_state(), &app.session.selection)?;
    let length = crate::prefs::show_mm(app, length, 1);
    Some(if count == 1 {
        crate::i18n::format("selection.length", &[("length", &length)])
    } else {
        crate::i18n::format("selection.length_total", &[("length", &length), ("count", &count.to_string())])
    })
}

/// Paints the readout in the bottom left corner of `rect`, above `below` (the analysis legend)
/// when that is shown. Not while a command dialog is open: Measure and the commands that take
/// edges show their own values.
pub(crate) fn paint(app: &SolveApp, painter: &Painter, rect: Rect, below: Option<Rect>) {
    if app.dialog.is_some() || app.session.selection.is_empty() {
        return;
    }
    let id = egui::Id::new("sc_selection_length");
    let state = app.session.world_state();
    let cached = painter.ctx().data(|d| d.get_temp::<Cached>(id));
    let text = match cached {
        Some(c) if c.selection == app.session.selection && Arc::ptr_eq(&c.state, &state) => c.text,
        _ => {
            let text = text(app);
            painter.ctx().data_mut(|d| d.insert_temp(id, Cached { selection: app.session.selection.clone(), state, text: text.clone() }));
            text
        }
    };
    let Some(text) = text else { return };
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text, FontId::proportional(t.font_body), t.text);
    let size = galley.size() + vec2(16.0, 8.0);
    let bottom = below.map_or(rect.bottom() - 12.0, |b| b.top() - 6.0);
    let r = Rect::from_min_size(pos2(rect.left() + 12.0, bottom - size.y), size);
    painter.rect(r, t.radius_sm, t.overlay, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    crate::scenario::publish_target("viewport:selection_length", r);
    painter.galley(r.left_center() + vec2(8.0, -galley.size().y / 2.0), galley, t.text);
}
