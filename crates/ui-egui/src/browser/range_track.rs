//! A range on a track, as in an audio editor. The track draws how much of the cloud lies at each
//! value (its bars) under the range. A thin strip above shows the whole span, with the part the
//! track shows (its loupe) framed: the strip is the same range widget, its range being the loupe.
//!
//! Gestures: drag a range's end or its band to move them; drag elsewhere to draw a new range;
//! Shift-click to bring the nearer end to the pointer; double-click the band to frame the loupe on
//! it, elsewhere to ask for the whole (`Outcome::whole`); the wheel or a pinch zooms the loupe
//! around the pointer, a sideways scroll moves it. An end snaps to a value where the colour
//! changes when that value stands alone at this zoom (nothing else within twice the reach), and
//! a whole-numbered property's ends to whole numbers; ⌥ moves ten times slower, without
//! snapping. With the keyboard, once the track is touched: ← and → bring the part touched last to
//! the previous or next change (one pixel with ⌥ or where there is none), Space asks to play it.
//! The ends and the length are fields too; a field's value may be typed relative, as `+30`.

use egui::{Color32, Mesh, Pos2, Rect, Sense, Stroke, Ui, vec2};

use crate::ui_kit;

use crate::theme::Tokens;

/// The part of a range a gesture moves, and that playing moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Part {
    Low,
    #[default]
    High,
    Band,
}

/// How much lies at each value of an axis: `slots[s][b]`, stacked in slot order, slot `s` in
/// `colours[s]`; or, with `ramp`, one slot painted at each value with the ramp over that range.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Bars {
    pub axis: [f64; 2],
    pub slots: Vec<Vec<f64>>,
    pub colours: Vec<Color32>,
    pub ramp: Option<[f64; 2]>,
}

/// What a track shows and snaps to.
pub(super) struct Track<'a> {
    /// Names the track among all of them.
    pub key: &'a str,
    pub range: [f64; 2],
    pub span: [f64; 2],
    pub integer: bool,
    pub overview: Option<&'a Bars>,
    pub loupe: Option<&'a Bars>,
    /// Values the ends snap to and the arrows step through, lowest first.
    pub changes: &'a [f64],
    /// A value moving on its own (a range playing), kept in the loupe.
    pub follow: Option<f64>,
}

/// What a frame's input asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Outcome {
    /// The range moved.
    pub range: Option<[f64; 2]>,
    /// The whole: a double click beside the range.
    pub whole: bool,
    /// Play or stop: Space.
    pub play: bool,
    /// The range was touched by a gesture, a field or a key.
    pub touched: bool,
}

/// What a drag moves: an end, the band, or a new range from where it started.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Low,
    High,
    Band,
    New(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    /// On the strip (moving the loupe) or the track (moving the range).
    strip: bool,
    grab: Grab,
    /// The range when it started, and the value under the pointer now (slowed by ⌥).
    start: [f64; 2],
    at: f64,
    from: f64,
    last_x: f32,
}

/// What the track keeps between frames.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    loupe: Option<[f64; 2]>,
    drag: Option<Drag>,
    last: Part,
    /// Where an end snapped during the drag.
    snapped: Option<f64>,
}

/// The loupe's narrowest width, as a fraction of the span.
const MIN_LOUPE: f64 = 1e-6;
/// Zoom per point of wheel scroll.
const ZOOM: f64 = 0.004;
/// How close to an end a press grabs it, and an end snaps, in points.
const REACH: f32 = 6.0;
/// ⌥'s slowing.
const FINE: f64 = 0.1;
const STRIP: f32 = 12.0;
const RULER: f32 = 14.0;
const BARS: f32 = 40.0;

fn id(key: &str) -> egui::Id {
    egui::Id::unique(("sc_range_track", key))
}

fn state(ctx: &egui::Context, key: &str) -> State {
    ctx.data(|d| d.get_temp(id(key))).unwrap_or_default()
}

fn whole(span: [f64; 2]) -> [f64; 2] {
    [span[0], span[1].max(span[0] + 1e-9)]
}

/// The part of `span` a track shows now.
pub(super) fn loupe(ctx: &egui::Context, key: &str, span: [f64; 2]) -> [f64; 2] {
    state(ctx, key).loupe.map_or(whole(span), |l| fit(l, whole(span)))
}

/// The part of a track's range touched last.
pub(super) fn last_part(ctx: &egui::Context, key: &str) -> Part {
    state(ctx, key).last
}

/// `l` moved and narrowed to lie within `span`, at least its narrowest.
fn fit([a, b]: [f64; 2], span: [f64; 2]) -> [f64; 2] {
    let full = span[1] - span[0];
    let w = (b - a).clamp(full * MIN_LOUPE, full);
    let left = (a + (b - a - w) / 2.0).clamp(span[0], span[1] - w);
    [left, left + w]
}

/// The change nearest `v` within `r`, when no other change is within `2r` of it.
fn lone_change(changes: &[f64], v: f64, r: f64) -> Option<f64> {
    let i = changes.partition_point(|c| *c < v);
    let near = [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|k| changes.get(k).map(|c| (k, *c)));
    let (k, c) = near.min_by(|a, b| (a.1 - v).abs().total_cmp(&(b.1 - v).abs()))?;
    let alone = k.checked_sub(1).and_then(|j| changes.get(j)).is_none_or(|p| c - p > 2.0 * r) && changes.get(k + 1).is_none_or(|n| n - c > 2.0 * r);
    ((c - v).abs() <= r && alone).then_some(c)
}

/// The changes standing alone at this zoom within `[a, b]`, at most `n`.
fn lone_changes(changes: &[f64], [a, b]: [f64; 2], r: f64, n: usize) -> Vec<f64> {
    let (i, j) = (changes.partition_point(|c| *c < a), changes.partition_point(|c| *c <= b));
    (i..j)
        .filter(|k| {
            let c = changes.get(*k).copied().unwrap_or(f64::NAN);
            k.checked_sub(1).and_then(|p| changes.get(p)).is_none_or(|p| c - p > 2.0 * r) && changes.get(k + 1).is_none_or(|q| q - c > 2.0 * r)
        })
        .filter_map(|k| changes.get(k).copied())
        .take(n)
        .collect()
}

/// The next change after `v` (`dir` 1) or before it (-1).
fn next_change(changes: &[f64], v: f64, dir: f64) -> Option<f64> {
    let eps = v.abs().max(1.0) * 1e-12;
    if dir > 0.0 {
        changes.get(changes.partition_point(|c| *c <= v + eps)).copied()
    } else {
        changes.partition_point(|c| *c < v - eps).checked_sub(1).and_then(|k| changes.get(k)).copied()
    }
}

/// Bar and tick steps: 1, 2 or 5 times a power of ten, at least `min` apart.
fn nice_step(min: f64) -> f64 {
    let k = 10f64.powf(min.max(1e-300).log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * k).find(|s| *s >= min).unwrap_or(10.0 * k)
}

/// Paint `bars` within `rect`, showing `axis`; each column scaled to the tallest shown.
fn paint_bars(painter: &egui::Painter, rect: Rect, axis: [f64; 2], bars: &Bars, fill: Color32, ink: Color32) {
    painter.rect_filled(rect, 2.0, fill);
    let n = bars.slots.first().map_or(0, Vec::len);
    if n == 0 || !(bars.axis[1] > bars.axis[0]) {
        return;
    }
    let width = (bars.axis[1] - bars.axis[0]) / n as f64;
    let x_of = |v: f64| rect.left() + ((v - axis[0]) / (axis[1] - axis[0])) as f32 * rect.width();
    let total = |b: usize| bars.slots.iter().filter_map(|s| s.get(b)).sum::<f64>();
    let shown: Vec<usize> = (0..n)
        .filter(|b| {
            let lo = bars.axis[0] + *b as f64 * width;
            lo + width >= axis[0] && lo <= axis[1]
        })
        .collect();
    let peak = shown.iter().map(|b| total(*b)).fold(0.0, f64::max);
    if !(peak > 0.0) {
        return;
    }
    let mut mesh = Mesh::default();
    for b in shown {
        let lo = bars.axis[0] + b as f64 * width;
        let (x0, x1) = (x_of(lo).max(rect.left()), x_of(lo + width).min(rect.right()).max(x_of(lo).max(rect.left()) + 1.0));
        let mut y = rect.bottom();
        for (s, slot) in bars.slots.iter().enumerate() {
            let h = (slot.get(b).copied().unwrap_or(0.0) / peak) as f32 * (rect.height() - 2.0);
            if h <= 0.0 {
                continue;
            }
            let colour = match bars.ramp {
                Some([a, z]) if z > a => {
                    crate::theme::model_color(solidcraft_engine::doc::point_cloud::height_ramp((lo + width / 2.0 - a) / (z - a)))
                }
                _ => bars.colours.get(s).copied().unwrap_or(ink),
            };
            mesh.add_colored_rect(Rect::from_min_max(Pos2::new(x0, y - h), Pos2::new(x1, y)), colour);
            y -= h;
        }
    }
    painter.add(mesh);
}

/// The range within `span` after this frame's input, and what else the input asks for.
pub(super) fn range_track(ui: &mut Ui, track: &Track) -> Outcome {
    let t = Tokens::get();
    let ctx = ui.ctx().clone();
    let mut st = state(&ctx, track.key);
    let span = whole(track.span);
    let mut out = Outcome::default();
    let mut range = track.range;
    let mut view = st.loupe.map_or(span, |l| fit(l, span));
    if let Some(f) = track.follow
        && (f < view[0] || f > view[1])
    {
        let w = view[1] - view[0];
        view = fit([f - 0.1 * w, f + 0.9 * w], span);
        st.loupe = Some(view);
    }

    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP + RULER + BARS + 4.0), Sense::click_and_drag());
    let inner = rect.shrink2(vec2(REACH, 0.0));
    let strip = Rect::from_min_size(inner.min, vec2(inner.width(), STRIP));
    let ruler = Rect::from_min_size(Pos2::new(inner.left(), strip.bottom() + 2.0), vec2(inner.width(), RULER));
    let bars = Rect::from_min_size(Pos2::new(inner.left(), ruler.bottom() + 2.0), vec2(inner.width(), BARS));
    // Named for agents and tests: the strip and the bars.
    crate::scenario::publish_rect(&format!("range_track:{}:strip", track.key), strip);
    crate::scenario::publish_rect(&format!("range_track:{}:bars", track.key), bars);
    let on_strip = |p: Pos2| p.y < strip.bottom() + 1.0;
    let axis_of = |strip: bool, view: [f64; 2]| if strip { span } else { view };
    let x_of = |axis: [f64; 2], v: f64| inner.left() + ((v - axis[0]) / (axis[1] - axis[0])) as f32 * inner.width();
    let v_of = |axis: [f64; 2], x: f32| axis[0] + f64::from((x - inner.left()) / inner.width().max(1.0)) * (axis[1] - axis[0]);
    let per_point = (view[1] - view[0]) / f64::from(inner.width().max(1.0));
    let reach = f64::from(REACH) * per_point;
    let mods = ui.input(|i| i.modifiers);
    let snap = |v: f64, fine: bool, snapped: &mut Option<f64>| {
        let s = if fine { None } else { lone_change(track.changes, v, reach) };
        *snapped = s;
        let v = s.unwrap_or(v);
        if track.integer { v.round() } else { v }
    };
    let grab_at = |strip: bool, x: f32, view: [f64; 2], range: [f64; 2]| {
        let (axis, [a, b]) = (axis_of(strip, view), if strip { view } else { range });
        let (xa, xb) = (x_of(axis, a), x_of(axis, b));
        if (x - xa).abs().min((x - xb).abs()) <= REACH {
            Some(if (x - xa).abs() < (x - xb).abs() { Grab::Low } else { Grab::High })
        } else if x > xa && x < xb {
            Some(Grab::Band)
        } else {
            None
        }
    };

    // Zoom and move the loupe.
    if response.hovered()
        && let Some(p) = response.hover_pos()
    {
        let (zoom, sideways, pinch) = crate::navigation::axis_wheel(&ctx, rect.height());
        if sideways != 0.0 {
            let d = -f64::from(sideways) * per_point;
            view = fit([view[0] + d, view[1] + d], span);
            st.loupe = Some(view);
        }
        if zoom != 0.0 || pinch != 1.0 {
            let k = (-f64::from(zoom) * ZOOM).exp() / f64::from(pinch);
            let at = v_of(axis_of(on_strip(p), view), p.x).clamp(view[0], view[1]);
            view = fit([at - (at - view[0]) * k, at + (view[1] - at) * k], span);
            st.loupe = Some(view);
        }
    }
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        response.request_focus();
        let strip = on_strip(p);
        let axis = axis_of(strip, view);
        let v = v_of(axis, p.x);
        let start = if strip { view } else { range };
        let grab = match grab_at(strip, p.x, view, range) {
            Some(g) => g,
            None if mods.shift => {
                if (v - start[0]).abs() < (v - start[1]).abs() {
                    Grab::Low
                } else {
                    Grab::High
                }
            }
            None => Grab::New(if strip { v } else { snap(v, mods.alt, &mut st.snapped) }),
        };
        if !strip {
            st.last = match grab {
                Grab::Low => Part::Low,
                Grab::Band => Part::Band,
                Grab::High | Grab::New(_) => Part::High,
            };
            out.touched = true;
        }
        st.drag = Some(Drag { strip, grab, start, at: v, from: v, last_x: p.x });
    }
    if response.dragged()
        && let (Some(mut d), Some(p)) = (st.drag, response.interact_pointer_pos())
    {
        let axis = if d.strip { span } else { view };
        let scale = (axis[1] - axis[0]) / f64::from(inner.width().max(1.0));
        d.at += f64::from(p.x - d.last_x) * scale * if mods.alt { FINE } else { 1.0 };
        d.last_x = p.x;
        let mut snapped = None;
        let mut put = |v: f64| if d.strip { v } else { snap(v.clamp(span[0], span[1]), mods.alt, &mut snapped) };
        let [a, b] = d.start;
        let moved = match d.grab {
            Grab::Band => {
                let w = b - a;
                let mut shift = d.at - d.from;
                if !d.strip {
                    // The band snaps by whichever end lands on a change.
                    let (la, lb) = (put(a + shift) - (a + shift), put(b + shift) - (b + shift));
                    shift += if la.abs() > 0.0 && (lb == 0.0 || la.abs() <= lb.abs()) { la } else { lb };
                }
                let lo = (a + shift).clamp(span[0], span[1] - w);
                [lo, lo + w]
            }
            Grab::New(anchor) => {
                let v = put(d.at);
                [anchor.min(v), anchor.max(v)]
            }
            Grab::Low | Grab::High => {
                let v = put(d.at);
                let other = if d.grab == Grab::Low { b } else { a };
                // An end dragged past the other becomes the other.
                let low = v < other;
                if !d.strip {
                    st.last = if low { Part::Low } else { Part::High };
                }
                d.grab = if low { Grab::Low } else { Grab::High };
                d.start = if low { [a, other] } else { [other, b] };
                [v.min(other), v.max(other)]
            }
        };
        st.snapped = snapped;
        if d.strip {
            view = fit(moved, span);
            st.loupe = Some(view);
        } else {
            range = moved;
            out.touched = true;
        }
        st.drag = Some(d);
    }
    if response.drag_stopped() {
        st.drag = None;
        st.snapped = None;
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        response.request_focus();
        let strip = on_strip(p);
        let v = v_of(axis_of(strip, view), p.x);
        if strip && grab_at(true, p.x, view, range).is_none() {
            // A click beside the loupe brings it there.
            let w = view[1] - view[0];
            view = fit([v - w / 2.0, v + w / 2.0], span);
            st.loupe = Some(view);
        } else if !strip && mods.shift && grab_at(false, p.x, view, range).is_none() {
            let v = snap(v, mods.alt, &mut st.snapped);
            let low = (v - range[0]).abs() < (v - range[1]).abs();
            range = if low { [v.min(range[1]), range[1]] } else { [range[0], v.max(range[0])] };
            st.last = if low { Part::Low } else { Part::High };
            out.touched = true;
        }
    }
    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        if on_strip(p) {
            view = span;
            st.loupe = None;
        } else if grab_at(false, p.x, view, range) == Some(Grab::Band) {
            let m = (range[1] - range[0]) * 0.08;
            view = fit([range[0] - m, range[1] + m], span);
            st.loupe = Some(view);
        } else {
            out.whole = true;
            out.touched = true;
        }
    }
    // The keyboard, while the track has it.
    if response.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(response.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() }));
        let (left, right, space) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) || i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) || i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
            )
        });
        out.play = space;
        if left || right {
            let dir = if right { 1.0 } else { -1.0 };
            let step = |v: f64| {
                let n = if mods.alt { None } else { next_change(track.changes, v, dir) };
                let v = n.unwrap_or(v + dir * per_point);
                let v = if track.integer && n.is_none() { (v + dir * 0.5).round() } else { v };
                v.clamp(span[0], span[1])
            };
            range = match st.last {
                Part::Low => [step(range[0]).min(range[1]), range[1]],
                Part::High => [range[0], step(range[1]).max(range[0])],
                Part::Band => {
                    let lead = if right { range[1] } else { range[0] };
                    let w = range[1] - range[0];
                    let lo = (range[0] + step(lead) - lead).clamp(span[0], span[1] - w);
                    [lo, lo + w]
                }
            };
            let moving = match st.last {
                Part::Low => range[0],
                Part::High => range[1],
                Part::Band => {
                    if right {
                        range[1]
                    } else {
                        range[0]
                    }
                }
            };
            if moving < view[0] || moving > view[1] {
                let w = view[1] - view[0];
                view = fit([moving - w / 2.0, moving + w / 2.0], span);
                st.loupe = Some(view);
            }
            out.touched = true;
        }
    }

    // The pointer says what a press would take: an end, the band, or a new range beside them.
    if let Some(p) = response.hover_pos().or_else(|| st.drag.and_then(|_| response.interact_pointer_pos())) {
        let icon = match st.drag.map(|d| d.grab) {
            Some(Grab::Band) => egui::CursorIcon::Grabbing,
            Some(_) => egui::CursorIcon::ResizeHorizontal,
            None => match grab_at(on_strip(p), p.x, view, range) {
                Some(Grab::Band) => egui::CursorIcon::Grab,
                Some(_) => egui::CursorIcon::ResizeHorizontal,
                None if on_strip(p) => egui::CursorIcon::PointingHand,
                None => egui::CursorIcon::Crosshair,
            },
        };
        ctx.set_cursor_icon(icon);
    }

    // The strip: the whole span, the range, and the loupe framed.
    let painter = ui.painter_at(rect);
    let empty = Bars::default();
    paint_bars(&painter, strip, span, track.overview.unwrap_or(&empty), t.control_fill, t.text_dim);
    let band = |r: Rect, axis: [f64; 2], [a, b]: [f64; 2]| {
        Rect::from_min_max(Pos2::new(x_of(axis, a).max(r.left()), r.top()), Pos2::new(x_of(axis, b).min(r.right()), r.bottom()))
    };
    painter.rect_filled(band(strip, span, range), 0.0, t.form_primary.gamma_multiply(0.35));
    let frame = band(strip, span, view);
    let shade = t.panel.gamma_multiply(0.7);
    painter.rect_filled(Rect::from_min_max(strip.min, Pos2::new(frame.left(), strip.bottom())), 0.0, shade);
    painter.rect_filled(Rect::from_min_max(Pos2::new(frame.right(), strip.top()), strip.max), 0.0, shade);
    painter.rect_stroke(frame, 1.0, Stroke::new(1.5, t.text), egui::StrokeKind::Inside);

    // The ruler: steps of 1, 2 or 5 times a power of ten, at least 60 points apart.
    let step = nice_step(60.0 * per_point);
    let decimals = (-step.log10().floor()).clamp(0.0, 9.0) as usize;
    let font = egui::FontId::proportional(t.font_micro);
    let mut v = (view[0] / step).ceil() * step;
    for _ in 0..64 {
        if v > view[1] {
            break;
        }
        let x = x_of(view, v);
        painter.line_segment([Pos2::new(x, ruler.bottom() - 5.0), Pos2::new(x, ruler.bottom())], Stroke::new(1.0, t.border));
        painter.text(Pos2::new(x + 2.0, ruler.top()), egui::Align2::LEFT_TOP, format!("{v:.decimals$}"), font.clone(), t.text_dim);
        v += step;
    }
    for c in lone_changes(track.changes, view, reach, 512) {
        let x = x_of(view, c);
        painter.line_segment([Pos2::new(x, ruler.bottom() - 3.0), Pos2::new(x, ruler.bottom() + 1.0)], Stroke::new(1.0, t.warning));
    }

    // The bars, what lies outside the range shaded, and the range's ends.
    paint_bars(&painter, bars, view, track.loupe.unwrap_or(&empty), t.control_fill, t.text_dim);
    let r = band(bars, view, range);
    painter.rect_filled(Rect::from_min_max(bars.min, Pos2::new(r.left(), bars.bottom())), 0.0, shade);
    painter.rect_filled(Rect::from_min_max(Pos2::new(r.right(), bars.top()), bars.max), 0.0, shade);
    painter.rect_filled(r, 0.0, t.form_primary.gamma_multiply(0.18));
    for (v, part) in [(range[0], Part::Low), (range[1], Part::High)] {
        if v < view[0] || v > view[1] {
            continue;
        }
        let x = x_of(view, v);
        let width = if response.has_focus() && st.last == part { 3.0 } else { 2.0 };
        painter.line_segment([Pos2::new(x, bars.top()), Pos2::new(x, bars.bottom())], Stroke::new(width, t.form_primary));
        painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(x - 5.0, bars.top()), Pos2::new(x + 5.0, bars.top()), Pos2::new(x, bars.top() + 6.0)],
            t.form_primary,
            Stroke::NONE,
        ));
    }
    if let (Some(s), Some(_)) = (st.snapped, st.drag) {
        let x = x_of(view, s);
        painter.line_segment([Pos2::new(x, bars.top()), Pos2::new(x, bars.bottom())], Stroke::new(1.0, t.warning));
    }
    if response.has_focus() && st.last == Part::Band {
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, t.form_primary), egui::StrokeKind::Inside);
    }

    // The ends and the length as numbers.
    let decimals = if track.integer { 0 } else { (-(per_point.max(1e-12)).log10()).ceil().clamp(0.0, 6.0) as usize };
    let speed = if track.integer { 1.0 } else { per_point };
    ui.horizontal(|ui| {
        let field = |ui: &mut Ui, v: &mut f64| ui_kit::track_number(ui, v, speed, decimals).changed();
        let (mut lo, mut hi) = (range[0], range[1]);
        let mut length = hi - lo;
        let changed_lo = field(ui, &mut lo);
        ui.label("…");
        let changed_hi = field(ui, &mut hi);
        ui.weak("↔");
        let changed_length = field(ui, &mut length);
        let round = |v: f64| if track.integer { v.round() } else { v };
        if changed_lo {
            range = [round(lo).min(range[1]), range[1]];
            st.last = Part::Low;
        } else if changed_hi {
            range = [range[0], round(hi).max(range[0])];
            st.last = Part::High;
        } else if changed_length {
            range = [range[0], range[0] + round(length).max(0.0)];
            st.last = Part::High;
        }
        if changed_lo || changed_hi || changed_length {
            out.touched = true;
        }
    });

    ctx.data_mut(|d| d.insert_temp(id(track.key), st));
    out.range = (range != track.range).then_some(range);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_snaps_only_when_it_stands_alone_at_this_zoom() {
        let changes = [0.0, 10.0, 11.0, 30.0];
        assert_eq!(lone_change(&changes, 29.0, 2.0), Some(30.0));
        assert_eq!(lone_change(&changes, 10.4, 2.0), None, "10 and 11 are too close to tell apart");
        assert_eq!(lone_change(&changes, 10.4, 0.45), Some(10.0));
        assert_eq!(lone_changes(&changes, [0.0, 40.0], 2.0, 10), vec![0.0, 30.0]);
        assert_eq!(
            (next_change(&changes, 10.0, 1.0), next_change(&changes, 10.0, -1.0), next_change(&changes, 30.0, 1.0)),
            (Some(11.0), Some(0.0), None)
        );
        assert_eq!(nice_step(3.2), 5.0);
        assert_eq!(nice_step(0.07), 0.1);
    }
}
