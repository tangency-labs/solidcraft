//! The viewport's frame-rate readout (Preferences ▸ Graphics ▸ "Frame rate") and its details
//! (#2548): a graph of the last frames' times with the triangles drawn over it, a histogram of
//! frame times, and what the GPU drew. Click the readout to show or hide the details.

use std::collections::VecDeque;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::SolveApp;
use crate::i18n::tr;
use crate::theme::Tokens;

/// Weight of the newest frame in the running mean (roughly the last 20 frames count).
const NEWEST: f64 = 0.05;

/// A gap between frames longer than this (seconds) means the view stood still: nothing changed,
/// so nothing was drawn.
const IDLE_AFTER: f64 = 1.0;

/// Frames the graph and the histogram keep: about four seconds at 60 frames per second.
pub const HISTORY: usize = 240;

/// The histogram's buckets: frames up to each time (ms); the last takes everything slower.
/// 17 and 34 ms hold the 60 and 30 FPS frames with a display's usual jitter.
pub const BUCKETS: [f32; 9] = [4.0, 8.0, 12.0, 17.0, 25.0, 34.0, 50.0, 100.0, f32::INFINITY];

/// One frame of the history.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Time since the previous frame (ms).
    pub ms: f32,
    /// What the GPU drew in it.
    pub triangles: u64,
    /// The model's scene was rebuilt for it (the design or what is shown changed).
    pub rebuilt: bool,
}

/// Frames per second, averaged over recent frames, and the last frames for the details.
#[derive(Clone, Debug, Default)]
pub struct FrameRate {
    mean_dt: f64,
    /// The last gap between frames was a pause: the view is idle.
    idle: bool,
    samples: VecDeque<Sample>,
    /// The model scene's key at the last frame, to mark the frames that rebuilt it.
    scene_key: u64,
}

impl FrameRate {
    /// Takes the time since the previous frame, in seconds. A gap over a second means the view
    /// was idle (it draws only when something changes): the mean starts again from the next
    /// frames, and the history keeps the frames before the pause. Anything that isn't a positive
    /// finite time is left out. `true` when it counted as a frame.
    pub fn record(&mut self, dt: f64) -> bool {
        if !dt.is_finite() || dt <= 0.0 {
            return false;
        }
        if dt > IDLE_AFTER {
            (self.mean_dt, self.idle) = (0.0, true);
            return false;
        }
        self.mean_dt = if self.mean_dt > 0.0 { self.mean_dt + NEWEST * (dt - self.mean_dt) } else { dt };
        self.idle = false;
        true
    }

    /// Adds a frame to the history, dropping the oldest past [`HISTORY`].
    pub fn push(&mut self, sample: Sample) {
        if self.samples.len() >= HISTORY {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// The last frames, oldest first.
    pub fn samples(&self) -> impl ExactSizeIterator<Item = &Sample> {
        self.samples.iter()
    }

    /// The averaged rate; `None` before the first frame was recorded and while the view is idle.
    pub fn fps(&self) -> Option<f64> {
        (self.mean_dt > 0.0).then(|| 1.0 / self.mean_dt)
    }

    /// The readout's text: rate and the matching time per frame, or 0 while the view is idle.
    pub fn label(&self) -> Option<String> {
        if self.idle {
            return Some("0 FPS".to_string());
        }
        self.fps().map(|fps| format!("{fps:.0} FPS · {:.1} ms", 1000.0 * self.mean_dt))
    }

    /// The mean time (ms) of the slowest 1% of the history's frames (at least one), and the
    /// slowest frame; `None` without a history.
    pub fn slowest(&self) -> Option<(f32, f32)> {
        let mut ms: Vec<f32> = self.samples.iter().map(|s| s.ms).collect();
        ms.sort_by(|a, b| b.total_cmp(a));
        let worst = *ms.first()?;
        let n = (ms.len() / 100).max(1);
        Some((ms.iter().take(n).sum::<f32>() / n as f32, worst))
    }

    /// How many of the history's frames fall in each of [`BUCKETS`].
    pub fn histogram(&self) -> [u32; BUCKETS.len()] {
        let mut counts = [0; BUCKETS.len()];
        for s in &self.samples {
            let i = BUCKETS.iter().position(|&up_to| s.ms <= up_to).unwrap_or(BUCKETS.len() - 1);
            if let Some(c) = counts.get_mut(i) {
                *c += 1;
            }
        }
        counts
    }
}

/// `1234567` as `1 234 567`: counts read at a glance.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out
}

/// A frame time's colour: within 60 FPS, within 30 FPS, slower (with the buckets' allowance for
/// jitter).
fn budget_colour(t: &Tokens, ms: f32) -> Color32 {
    if ms <= 17.0 {
        t.accent
    } else if ms <= 34.0 {
        t.warning
    } else {
        t.error
    }
}

/// The readout's distance from the view's edges and from the panes beside it.
const MARGIN: f32 = 12.0;

/// `readout` moved left until none of the `panes` covering the view overlaps it, `MARGIN` clear
/// of the leftmost of them; it stops at the view's left edge.
fn beside_panes(mut readout: Rect, view: Rect, panes: &[Rect]) -> Rect {
    for _ in 0..panes.len().min(32) {
        let Some(pane) = panes.iter().find(|p| p.is_finite() && p.is_positive() && p.intersects(readout)) else { break };
        let left = (pane.left() - MARGIN - readout.width()).max(view.left() + MARGIN);
        if left >= readout.left() {
            break;
        }
        readout = readout.translate(vec2(left - readout.left(), 0.0));
    }
    readout
}

/// Records this frame and paints the readout in the view's bottom-right corner, left of a pane over
/// that corner, with the details above it when they are on. The readout asks for no frames of its
/// own, so it shows how often the view is really drawn and leaves an idle window idle (a readout
/// that redrew every frame kept the GPU busy on an untouched window); the details show the frames
/// the view drew. Once the frames stop, one more frame a little over [`IDLE_AFTER`] later shows the
/// view idle.
pub(crate) fn paint(app: &mut SolveApp, ui: &egui::Ui, painter: &egui::Painter, rect: Rect) {
    if !app.preferences.show_frame_rate {
        app.frame_rate = FrameRate::default();
        return;
    }
    let ctx = ui.ctx();
    let drawn = app.viewport.draw_stats.as_ref().and_then(|s| s.lock().ok().map(|s| *s)).unwrap_or_default();
    if app.frame_rate.record(f64::from(ctx.input(|i| i.unstable_dt))) {
        let ms = ctx.input(|i| i.unstable_dt) * 1000.0;
        let rebuilt = app.frame_rate.scene_key != app.viewport.key;
        app.frame_rate.push(Sample { ms, triangles: drawn.triangles, rebuilt });
    }
    app.frame_rate.scene_key = app.viewport.key;
    if app.frame_rate.fps().is_some() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(IDLE_AFTER + 0.1));
    }
    let Some(text) = app.frame_rate.label() else { return };
    let t = Tokens::get();
    let font = FontId::monospace(t.font_body);
    let galley = painter.layout_no_wrap(text, font.clone(), t.text);
    let size = galley.size() + vec2(16.0, 8.0);
    let r = Rect::from_min_size(pos2(rect.right() - MARGIN - size.x, rect.bottom() - MARGIN - size.y), size);
    // Just left of a pane over the view's right side (the Assistant, a dialog), not under it (#6755).
    let mut r = beside_panes(r, rect, &crate::shell::covered_rects(app));
    // Above the navigation bar where a narrow view brings the two together.
    if let Some(nav) = app.viewport.layout.as_ref().map(|l| l.navigation)
        && r.intersects(nav.expand(4.0))
    {
        r = r.translate(vec2(0.0, nav.top() - 6.0 - r.bottom()));
    }
    let id = egui::Id::unique("sc_frame_rate");
    let clicked = ui.interact(r, id, Sense::click()).on_hover_text(tr("perf.click_for_details")).clicked();
    if clicked {
        app.preferences.frame_rate_details = !app.preferences.frame_rate_details;
    }
    painter.rect(r, t.radius_sm, t.overlay, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    crate::scenario::publish_target("viewport:frame_rate", r);
    painter.galley(r.center() - galley.size() / 2.0, galley, t.text);
    if app.preferences.frame_rate_details {
        details(app, painter, rect, r, drawn, &font);
    }
}

/// The details above the readout `below`, flush with its right edge: the frame-time graph with the
/// triangles over it, the histogram and what the GPU drew.
fn details(app: &SolveApp, painter: &egui::Painter, view: Rect, below: Rect, drawn: crate::gpu::DrawStats, font: &FontId) {
    let t = Tokens::get();
    let (pad, row, label_h, graph_h, hist_h) = (8.0, 16.0, 12.0, 56.0, 40.0);
    let lines = 7.0 + 0.8; // seven rows and the adapter's name
    let w = 300.0_f32.min(below.right() - view.left() - MARGIN);
    let h = pad * 2.0 + row + label_h + graph_h + 6.0 + hist_h + row + lines * row;
    if w < 160.0 || h > view.height() - below.height() - 30.0 {
        return;
    }
    let panel = Rect::from_min_size(pos2(below.right() - w, below.top() - 6.0 - h), vec2(w, h));
    painter.rect(panel, t.radius_sm, t.dialog_bg, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    crate::scenario::publish_handle("viewport:frame_details", panel.center());
    let inner = panel.shrink(pad);
    let small = FontId::monospace(t.font_small);
    let text = |pos: egui::Pos2, align: Align2, s: &str, font: &FontId, c: Color32| {
        painter.text(pos, align, s, font.clone(), c);
    };
    let mut y = inner.top();

    // Heading: the slowest frames, which the mean hides.
    let heading = match app.frame_rate.slowest() {
        Some((low, worst)) => format!("{} {:.0} FPS · {} {worst:.1} ms", tr("perf.low_1"), 1000.0 / low.max(0.01), tr("perf.worst")),
        None => String::new(),
    };
    text(pos2(inner.left(), y), Align2::LEFT_TOP, &heading, font, t.text);
    y += row;

    // The frame-time graph: one bar per frame, newest on the right, the 60 and 30 FPS budgets as
    // dashed lines, the triangles drawn as a solid line over it and frames that rebuilt the scene
    // marked with a tick.
    let graph = Rect::from_min_size(pos2(inner.left(), y + label_h), vec2(inner.width(), graph_h));
    let samples: Vec<Sample> = app.frame_rate.samples().copied().collect();
    let top_ms = samples.iter().map(|s| s.ms).fold(33.3_f32 * 1.25, f32::max).min(250.0);
    let y_of = |ms: f32| graph.bottom() - graph.height() * (ms / top_ms).clamp(0.0, 1.0);
    let bar = graph.width() / HISTORY as f32;
    let first = HISTORY.saturating_sub(samples.len()) as f32;
    for (i, s) in samples.iter().enumerate() {
        let x = graph.left() + (first + i as f32) * bar;
        let r = Rect::from_min_max(pos2(x, y_of(s.ms)), pos2(x + bar.max(1.0), graph.bottom()));
        painter.rect_filled(r, 0.0, budget_colour(&t, s.ms));
        if s.rebuilt {
            painter.line_segment([pos2(x, graph.top()), pos2(x, graph.top() + 5.0)], Stroke::new(1.0, t.text));
        }
    }
    for (ms, label) in [(16.7, "60 FPS"), (33.3, "30 FPS")] {
        let gy = y_of(ms);
        painter.extend(egui::Shape::dashed_line(&[pos2(graph.left(), gy), pos2(graph.right(), gy)], Stroke::new(1.0, t.text_dim), 3.0, 3.0));
        text(pos2(graph.left() + 2.0, gy - 1.0), Align2::LEFT_BOTTOM, label, &small, t.text_dim);
    }
    let most = samples.iter().map(|s| s.triangles).max().unwrap_or(0);
    if most > 0 {
        let pts: Vec<egui::Pos2> = samples
            .iter()
            .enumerate()
            .map(|(i, s)| {
                pos2(graph.left() + (first + i as f32 + 0.5) * bar, graph.bottom() - graph.height() * 0.75 * (s.triangles as f32 / most as f32))
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.5, t.text.gamma_multiply(0.8))));
        text(pos2(graph.left(), graph.top() - 1.0), Align2::LEFT_BOTTOM, &format!("— {} {}", grouped(most), tr("perf.triangles")), &small, t.text);
    }
    text(pos2(graph.right(), graph.top() - 1.0), Align2::RIGHT_BOTTOM, &format!("↑ {top_ms:.0} ms"), &small, t.text_dim);
    y = graph.bottom() + 6.0;

    // The histogram: how many frames took each span of time.
    let counts = app.frame_rate.histogram();
    let total = counts.iter().sum::<u32>().max(1) as f32;
    let cell = inner.width() / BUCKETS.len() as f32;
    let hist = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), hist_h));
    let mut from = 0.0_f32;
    for (i, (&n, &up_to)) in counts.iter().zip(&BUCKETS).enumerate() {
        let x = hist.left() + i as f32 * cell;
        let share = n as f32 / total;
        let top = hist.bottom() - hist.height() * share;
        let colour = budget_colour(&t, if up_to.is_finite() { up_to } else { from + 1.0 });
        painter.rect_filled(Rect::from_min_max(pos2(x + 1.0, top), pos2(x + cell - 1.0, hist.bottom())), 1.0, colour);
        if n > 0 {
            let pct = if share < 0.01 { "<1%".to_string() } else { format!("{:.0}%", share * 100.0) };
            text(pos2(x + cell / 2.0, top - 1.0), Align2::CENTER_BOTTOM, &pct, &small, t.text_dim);
        }
        let label = if up_to.is_finite() { format!("{up_to:.0}") } else { format!("{from:.0}+") };
        text(pos2(x + cell / 2.0, hist.bottom() + 1.0), Align2::CENTER_TOP, &label, &small, t.text_dim);
        from = up_to;
    }
    y = hist.bottom() + row;

    // What the GPU drew last frame, and the work around it.
    let mut kv = |k: &str, v: String| {
        text(pos2(inner.left(), y), Align2::LEFT_TOP, k, font, t.text_dim);
        text(pos2(inner.right(), y), Align2::RIGHT_TOP, &v, font, t.text);
        y += row;
    };
    kv(tr("perf.triangles"), grouped(drawn.triangles));
    kv(tr("perf.line_segments"), grouped(drawn.line_segments));
    kv(tr("perf.draw_calls"), grouped(u64::from(drawn.draw_calls)));
    let px = view.size() * painter.ctx().pixels_per_point();
    let msaa = app.viewport.gpu.map_or(1, |g| g.samples);
    let ss = if drawn.supersampled { " · SSAA" } else { "" };
    kv(tr("perf.viewport"), format!("{:.0}×{:.0} · MSAA {msaa}×{ss}", px.x, px.y));
    kv(tr("perf.ui_time"), format!("{:.1} ms", app.frame_ms));
    kv(tr("perf.scene_build"), format!("{:.1} ms", app.viewport.build_ms));
    // The adapter's name can be long: on a line of its own, cut to the panel's width.
    let (backend, adapter) = app.viewport.adapter.split_once(" · ").unwrap_or(("CPU", ""));
    kv(tr("perf.renderer"), backend.to_string());
    let room = (inner.width() / 6.0) as usize;
    let adapter: String =
        if adapter.chars().count() > room { adapter.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { adapter.to_string() };
    text(pos2(inner.right(), y - 2.0), Align2::RIGHT_TOP, &adapter, &small, t.text_dim);
}

#[cfg(test)]
mod tests {
    use super::{BUCKETS, FrameRate, HISTORY, Sample, grouped};

    #[test]
    fn steady_frames_read_their_rate() {
        let mut f = FrameRate::default();
        assert_eq!(f.fps(), None);
        for _ in 0..200 {
            f.record(1.0 / 60.0);
        }
        assert!((f.fps().unwrap_or(0.0) - 60.0).abs() < 1e-6);
        assert_eq!(f.label().as_deref(), Some("60 FPS · 16.7 ms"));
    }

    #[test]
    fn hostile_times_are_left_out_and_a_pause_reads_idle() {
        let mut f = FrameRate::default();
        for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(!f.record(dt));
        }
        assert_eq!(f.fps(), None);
        assert_eq!(f.label(), None);
        assert!(f.record(0.01));
        assert!((f.fps().unwrap_or(0.0) - 100.0).abs() < 1e-6);
        // Nothing drawn for 3 s: the view was idle, and the next frames start a new mean.
        assert!(!f.record(3.0));
        assert_eq!(f.fps(), None);
        assert_eq!(f.label().as_deref(), Some("0 FPS"));
        f.record(1.0 / 60.0);
        assert!((f.fps().unwrap_or(0.0) - 60.0).abs() < 1e-6);
    }

    /// The readout must not keep an untouched window drawing: with it on, a frame asks for the
    /// next one only after the idle gap (to show 0 FPS), and the idle frame asks for none.
    #[test]
    fn the_readout_asks_for_no_frames_of_its_own() {
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.preferences.show_frame_rate = true;
        let ctx = crate::fonts::test_context();
        let mut frame = |time: f64| {
            let raw = egui::RawInput { time: Some(time), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter().clone();
                super::paint(&mut app, ui, &painter, rect);
            });
            out.textures_delta.clear();
            out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay)
        };
        for i in 0..10 {
            frame(f64::from(i) / 60.0);
        }
        let busy = frame(10.0 / 60.0);
        assert!(busy.is_some_and(|d| d.as_secs_f64() > 1.0), "a running readout asked for a frame after {busy:?}");
        let idle = frame(2.0);
        assert!(idle.is_some_and(|d| d.as_secs_f64() > 3600.0), "an idle readout asked for a frame after {idle:?}");
        assert_eq!(app.frame_rate.label().as_deref(), Some("0 FPS"));
    }

    #[test]
    fn the_mean_follows_a_slower_view() {
        let mut f = FrameRate::default();
        f.record(1.0 / 120.0);
        for _ in 0..300 {
            f.record(1.0 / 30.0);
        }
        assert!((f.fps().unwrap_or(0.0) - 30.0).abs() < 0.1);
    }

    /// The details keep the last [`HISTORY`] frames, and the slowest 1% and the histogram show a
    /// stutter that the mean hides (#2548).
    #[test]
    fn the_history_shows_stutters_the_mean_hides() {
        let mut f = FrameRate::default();
        assert_eq!(f.slowest(), None);
        assert_eq!(f.histogram(), [0; BUCKETS.len()]);
        for i in 0..1000 {
            let ms = if i % 100 == 0 { 120.0 } else { 10.0 };
            f.record(f64::from(ms) / 1000.0);
            f.push(Sample { ms, triangles: i, rebuilt: false });
        }
        assert_eq!(f.samples().len(), HISTORY);
        assert_eq!(f.samples().last().map(|s| s.triangles), Some(999), "the newest frame is last");
        assert!(f.fps().unwrap_or(0.0) > 50.0, "the mean barely moves");
        let (low, worst) = f.slowest().unwrap_or_default();
        assert_eq!((low, worst), (120.0, 120.0), "the stutters are the slowest 1%");
        let h = f.histogram();
        assert_eq!(h.iter().sum::<u32>(), HISTORY as u32);
        assert_eq!((h[2], h[8]), (HISTORY as u32 - 2, 2), "10 ms frames up to 12 ms, two stutters over 100 ms");
        assert_eq!(BUCKETS.last(), Some(&f32::INFINITY), "the last bucket takes every slower frame");
    }

    /// Clicking the readout shows the details and clicking it again hides them; the choice is a
    /// preference, and `ui.inspect` carries what the GPU drew.
    #[test]
    fn clicking_the_readout_shows_and_hides_the_details() {
        use crate::scenario::handle_at;
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"show_frame_rate": true}));
        h.frames(4);
        let readout = handle_at("viewport:frame_rate").expect("the readout is drawn");
        assert!(handle_at("viewport:frame_details").is_none());
        h.call("ui.click", json!({"x": readout.x, "y": readout.y}));
        h.frames(2);
        assert!(h.app.preferences.frame_rate_details);
        assert!(handle_at("viewport:frame_details").is_some(), "the details are drawn");
        assert!(h.app.frame_rate.samples().len() >= 2, "frames are kept for the graph");
        let inspect = h.call("ui.inspect", json!({}));
        assert!(inspect["result"].get("draw").is_some() && inspect["result"].get("adapter").is_some(), "{inspect}");
        let readout = handle_at("viewport:frame_rate").expect("the readout is still drawn");
        h.call("ui.click", json!({"x": readout.x, "y": readout.y}));
        h.frames(2);
        assert!(!h.app.preferences.frame_rate_details);
        assert!(handle_at("viewport:frame_details").is_none());
    }

    /// Panes over the readout push it left, a pane-free corner keeps it, and hostile or
    /// view-wide panes never push it out of the view (#6755).
    #[test]
    fn the_readout_moves_left_of_the_panes_over_it() {
        use egui::{Rect, pos2};
        let view = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 600.0));
        let readout = Rect::from_min_max(pos2(850.0, 560.0), pos2(988.0, 588.0));
        assert_eq!(super::beside_panes(readout, view, &[]), readout);
        let top = Rect::from_min_max(pos2(700.0, 0.0), pos2(1000.0, 100.0));
        assert_eq!(super::beside_panes(readout, view, &[top]), readout, "a pane above leaves it in the corner");
        let chat = Rect::from_min_max(pos2(600.0, 40.0), pos2(992.0, 593.0));
        let dialog = Rect::from_min_max(pos2(380.0, 500.0), pos2(590.0, 580.0));
        let moved = super::beside_panes(readout, view, &[chat, dialog]);
        assert_eq!((moved.right(), moved.size()), (dialog.left() - 12.0, readout.size()), "left of both");
        let nan = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1000.0, 600.0));
        assert_eq!(super::beside_panes(readout, view, &[nan]), readout);
        let everything = view.expand(10.0);
        assert_eq!(super::beside_panes(readout, view, &[everything]).left(), 12.0, "stops at the view's left edge");
    }

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1\u{2009}000");
        assert_eq!(grouped(12_345_678), "12\u{2009}345\u{2009}678");
    }
}
