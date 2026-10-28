//! The window while the app gets ready (#1210). The window stays hidden until its first frame is
//! drawn, so the setup that is slow on some computers (fonts, the 3D view's shaders, the recovery
//! scan, a design to open) runs in later frames, one step each, behind a light "Starting" page:
//! the first frame only paints that page, and each later one runs the next step and names the one
//! after it. A step can also wait for work on another thread (the 3D view's shader compile), so
//! the window keeps drawing and answering (moving, minimizing) while it runs.
//!
//! The steps run from the app's logic, which eframe keeps calling for a minimized or occluded window
//! without painting it. Nobody sees the page then, so the rest of the setup runs at once: the app
//! is ready when the window comes back, as it was when all of it ran before the window opened.

use std::collections::VecDeque;

use egui::{Align2, FontId, Id, Sense, Stroke, ViewportCommand, vec2};

use crate::theme::Tokens;
use crate::ui_kit::{Container, Surface};

/// How often the page is drawn while a step waits for its worker (see [`Starting::wait_frame`]).
const WAIT_FRAME: std::time::Duration = std::time::Duration::from_millis(100);

/// One step of the setup, run on its own frame with what it sets up (`T`, the app); `true` once it
/// is done, `false` to be asked again next frame.
pub type Step<T> = Box<dyn FnMut(&mut T, &egui::Context) -> bool>;

/// The steps still to run. `T` is what they set up: the desktop app's window state.
pub struct Starting<T> {
    steps: VecDeque<(&'static str, Step<T>)>,
    /// Whether a frame showed the page yet: no step runs before it has.
    shown: bool,
    total: usize,
    wait_frame: std::time::Duration,
}

impl<T> Default for Starting<T> {
    fn default() -> Self {
        Self { steps: VecDeque::new(), shown: false, total: 0, wait_frame: WAIT_FRAME }
    }
}

impl<T> Starting<T> {
    /// A step, after the ones already added. `label` (a message id; empty for none) says on the
    /// page what is being done while the step runs.
    #[must_use]
    pub fn then(self, label: &'static str, step: impl FnOnce(&mut T, &egui::Context) + 'static) -> Self {
        let mut step = Some(step);
        self.until(label, move |target, ctx| {
            if let Some(step) = step.take() {
                step(target, ctx);
            }
            true
        })
    }

    /// A step that is asked once a frame until it says it is done: one that waits for a worker
    /// thread, keeping the window drawing meanwhile.
    #[must_use]
    pub fn until(mut self, label: &'static str, step: impl FnMut(&mut T, &egui::Context) -> bool + 'static) -> Self {
        self.steps.push_back((label, Box::new(step)));
        self.total += 1;
        self
    }

    /// How often the page is drawn while a step waits; longer where drawing takes the CPU the
    /// waited-for work needs (a software renderer).
    #[must_use]
    pub fn wait_frame(mut self, every: std::time::Duration) -> Self {
        self.wait_frame = every;
        self
    }

    /// Whether every step has run.
    pub fn done(&self) -> bool {
        self.steps.is_empty()
    }

    /// The start of a frame: on the first one nothing runs (the page goes up first); on each
    /// later one, the next step; every step left while the window is minimized or occluded.
    /// Returns whether the app is still starting, so this frame shows the page again.
    pub fn logic(&mut self, target: &mut T, ctx: &egui::Context) -> bool {
        let hidden = ctx.input(|i| i.viewport().visible() == Some(false));
        if !self.shown && !hidden {
            self.shown = true;
            ctx.request_repaint();
            return !self.done();
        }
        self.shown = true;
        let mut waiting = false;
        while let Some((_, step)) = self.steps.front_mut() {
            if !step(target, ctx) {
                waiting = true;
                break;
            }
            self.steps.pop_front();
            if !hidden {
                break;
            }
        }
        if self.done() {
            return false;
        }
        if waiting {
            // A few frames a second keep the bar moving; drawing flat out would take the GPU (or a
            // software renderer's CPU) from the work being waited for. Input still draws at once.
            ctx.request_repaint_after(self.wait_frame);
        } else {
            ctx.request_repaint();
        }
        true
    }

    /// The page: the name, a progress bar and what the next step does, in the theme's colours.
    /// Dragging it moves the window (there may be no OS title bar yet).
    pub fn ui(&self, ui: &mut egui::Ui) {
        let t = Tokens::get();
        let label = self.steps.front().map_or("", |(label, _)| *label);
        let done = self.total - self.steps.len();
        Container::new(ui.ctx(), Surface::Viewport).central().frame(Container::new(ui.ctx(), Surface::Viewport).edge_frame(t.panel)).show(ui, |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(rect, Id::new("starting-drag"), Sense::click_and_drag());
            if response.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
            }
            let painter = ui.painter();
            let c = rect.center();
            painter.text(c - vec2(0.0, 28.0), Align2::CENTER_BOTTOM, "SolidCraft", FontId::proportional(t.font_display), t.text);
            // Steps done of all of them; a step that runs long holds the bar where it is.
            let bar = egui::Rect::from_center_size(c, vec2(240.0, 4.0));
            painter.rect_filled(bar, 2.0, t.border);
            let share = if self.total == 0 { 1.0 } else { done as f32 / self.total as f32 };
            let filled = bar.width() * share.clamp(0.0, 1.0);
            painter.rect_filled(egui::Rect::from_min_size(bar.min, vec2(filled, bar.height())), 2.0, t.accent);
            // A light that runs along the rest of the bar: the app is busy, not stuck.
            let rest = bar.width() - filled;
            if rest > 24.0 {
                let phase = (ui.input(|i| i.time) % 1.4 / 1.4) as f32;
                let x = bar.min.x + filled + (rest - 24.0) * phase;
                painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x, bar.min.y), vec2(24.0, bar.height())), 2.0, t.accent_soft);
            }
            if !label.is_empty() {
                painter.text(c + vec2(0.0, 20.0), Align2::CENTER_TOP, crate::i18n::tr(label), FontId::proportional(t.font_body), t.text_dim);
            }
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(output: &egui::FullOutput) -> Vec<String> {
        let mut out = Vec::new();
        for shape in &output.shapes {
            if let egui::Shape::Text(t) = &shape.shape {
                out.push(t.galley.text().to_string());
            }
        }
        out
    }

    fn three_steps() -> Starting<Vec<&'static str>> {
        Starting::default()
            .then("", |log: &mut Vec<&'static str>, _| log.push("fonts"))
            .then("starting.view", |log: &mut Vec<&'static str>, _| log.push("view"))
            .then("starting.recovery", |log: &mut Vec<&'static str>, _| log.push("recovery"))
    }

    /// What eframe does for a window it does not paint: the app's logic alone, nothing presented.
    fn logic_only(ctx: &egui::Context, starting: &mut Starting<Vec<&'static str>>, log: &mut Vec<&'static str>, info: egui::ViewportInfo) -> bool {
        let mut input = egui::RawInput::default();
        input.viewports.insert(egui::ViewportId::ROOT, info);
        let mut still = false;
        let _ = ctx.run_logic(&input, |ctx| still = starting.logic(log, ctx));
        still
    }

    fn frame(ctx: &egui::Context, starting: &mut Starting<Vec<&'static str>>, log: &mut Vec<&'static str>) -> (bool, Vec<String>) {
        let mut still = false;
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 1000.0))), ..Default::default() },
            |ui| {
                still = starting.logic(log, ui.ctx());
                if still {
                    starting.ui(ui);
                }
            },
        );
        output.textures_delta.clear();
        (still, texts(&output))
    }

    /// The first frame (the one that shows the window) is drawn before any step runs, however
    /// slow; each later frame runs one step and names the next.
    #[test]
    fn the_first_frame_shows_the_page_before_any_step_runs() {
        let ctx = egui::Context::default();
        let mut starting = three_steps();
        let mut log = Vec::new();

        let (still, painted) = frame(&ctx, &mut starting, &mut log);
        assert!(still);
        assert!(log.is_empty(), "no step ran before the first frame: {log:?}");
        assert!(painted.iter().any(|t| t == "SolidCraft"), "{painted:?}");
        assert!(ctx.has_requested_repaint(), "the next frame comes without input");

        let (still, painted) = frame(&ctx, &mut starting, &mut log);
        assert!(still);
        assert_eq!(log, ["fonts"]);
        assert!(painted.iter().any(|t| t == "Preparing the 3D view…"), "the page names the step the next frame runs: {painted:?}");

        let (still, painted) = frame(&ctx, &mut starting, &mut log);
        assert!(still);
        assert_eq!(log, ["fonts", "view"]);
        assert!(painted.iter().any(|t| t == "Checking for unsaved work…"), "{painted:?}");

        let (still, painted) = frame(&ctx, &mut starting, &mut log);
        assert!(!still, "the app draws this frame itself");
        assert!(starting.done());
        assert_eq!(log, ["fonts", "view", "recovery"]);
        assert!(painted.is_empty(), "{painted:?}");
    }

    #[test]
    fn no_steps_means_the_app_draws_from_the_second_frame() {
        let ctx = egui::Context::default();
        let mut starting = Starting::default();
        let mut log = Vec::new();
        assert!(!frame(&ctx, &mut starting, &mut log).0);
        assert!(!frame(&ctx, &mut starting, &mut log).0);
    }

    /// A minimized or occluded window gets the app's logic but no frames: the setup finishes
    /// there, at once, instead of waiting for the window to be painted again.
    #[test]
    fn a_window_nobody_sees_finishes_the_setup_without_frames() {
        for (minimized, occluded) in [(Some(true), None), (Some(false), Some(true))] {
            let info = egui::ViewportInfo { minimized, occluded, ..Default::default() };
            // From the launch (a window started minimized) …
            let ctx = egui::Context::default();
            let mut starting = three_steps();
            let mut log = Vec::new();
            assert!(!logic_only(&ctx, &mut starting, &mut log, info.clone()));
            assert_eq!(log, ["fonts", "view", "recovery"]);
            // … and after the page was shown, then the window minimized or covered.
            let mut starting = three_steps();
            let mut log = Vec::new();
            assert!(frame(&ctx, &mut starting, &mut log).0);
            assert!(log.is_empty());
            assert!(!logic_only(&ctx, &mut starting, &mut log, info));
            assert_eq!(log, ["fonts", "view", "recovery"]);
            assert!(starting.done());
        }
    }

    /// The steps follow the app's logic, not presented frames: logic calls alone finish the setup.
    #[test]
    fn the_setup_moves_on_with_logic_alone() {
        let ctx = egui::Context::default();
        let mut starting = three_steps();
        let mut log = Vec::new();
        let mut calls = 0;
        while logic_only(&ctx, &mut starting, &mut log, egui::ViewportInfo::default()) {
            calls += 1;
            assert!(calls < 10, "the setup never finished: {log:?}");
        }
        assert_eq!(log, ["fonts", "view", "recovery"]);
    }

    /// A step that waits for a worker (the 3D view's shader compile) is asked each frame, and the
    /// page keeps drawing and naming it meanwhile: the window answers while the work runs.
    #[test]
    fn a_step_that_waits_keeps_the_page_drawing() {
        let ctx = egui::Context::default();
        let mut polls = 0;
        let mut starting = Starting::default()
            .until("starting.view", move |log: &mut Vec<&'static str>, _| {
                polls += 1;
                if polls < 3 {
                    return false;
                }
                log.push("view");
                true
            })
            .then("starting.recovery", |log: &mut Vec<&'static str>, _| log.push("recovery"));
        let mut log = Vec::new();
        assert!(frame(&ctx, &mut starting, &mut log).0);
        for _ in 0..2 {
            let (still, painted) = frame(&ctx, &mut starting, &mut log);
            assert!(still);
            assert!(log.is_empty(), "nothing after the waiting step ran: {log:?}");
            assert!(painted.iter().any(|t| t == "Preparing the 3D view…"), "{painted:?}");
            assert!(ctx.has_requested_repaint(), "frames go on while the step waits");
        }
        let (still, painted) = frame(&ctx, &mut starting, &mut log);
        assert!(still);
        assert_eq!(log, ["view"]);
        assert!(painted.iter().any(|t| t == "Checking for unsaved work…"), "{painted:?}");
        assert!(!frame(&ctx, &mut starting, &mut log).0);
        assert_eq!(log, ["view", "recovery"]);
    }

    /// Minimized, a waiting step is asked at each of eframe's logic ticks; the steps after it run
    /// in the same tick it finishes.
    #[test]
    fn a_window_nobody_sees_waits_without_spinning_then_finishes() {
        let ctx = egui::Context::default();
        let mut polls = 0;
        let mut starting = Starting::default()
            .until("starting.view", move |log: &mut Vec<&'static str>, _| {
                polls += 1;
                if polls < 2 {
                    return false;
                }
                log.push("view");
                true
            })
            .then("starting.recovery", |log: &mut Vec<&'static str>, _| log.push("recovery"));
        let mut log = Vec::new();
        let minimized = egui::ViewportInfo { minimized: Some(true), ..Default::default() };
        assert!(logic_only(&ctx, &mut starting, &mut log, minimized.clone()), "still waiting");
        assert!(log.is_empty());
        assert!(!logic_only(&ctx, &mut starting, &mut log, minimized));
        assert_eq!(log, ["view", "recovery"]);
    }
}
