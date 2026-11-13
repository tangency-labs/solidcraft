//! The last completed UI pass that submitted a Render image, not a presentation acknowledgement.
//! Fixed metadata only: the receipt owns no textures, pixels, handles, hashes or history.

use egui::{Context, Painter, Rect};
use serde::Serialize;
use solidcraft_engine::render::job::{FrameInfo, JobId, PreviewInfo, adaptive::AdaptivePreview};

use crate::SolveApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Preview,
    Final,
    DenoiseMask,
    LivePreview,
    LiveFinal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Source {
    pub job: JobId,
    pub scene_generation: u64,
    pub width: u32,
    pub height: u32,
    pub kind: Kind,
    pub frame_sequence: Option<u32>,
    /// Fixed sampling: completed SPP. Adaptive sampling: absolute complete iteration;
    /// `adaptive` supplies the captured pilot/production interpretation.
    pub samples_per_pixel: Option<u32>,
    pub adaptive: Option<AdaptivePreview>,
}
impl Source {
    pub(super) fn image(frame: &FrameInfo, preview: Option<&PreviewInfo>) -> Self {
        Self {
            job: frame.job,
            scene_generation: frame.scene_generation,
            width: frame.width,
            height: frame.height,
            kind: if preview.is_some() { Kind::Preview } else { Kind::Final },
            frame_sequence: preview.map(|info| info.frame_sequence),
            samples_per_pixel: preview.map(|info| info.samples_per_pixel),
            adaptive: preview.and_then(|info| info.adaptive),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Receipt {
    #[serde(flatten)]
    source: Source,
    /// These identify the UI pass that issued the image shape. A logic-only update
    /// does not refresh them. They do not acknowledge compositor/GPU presentation.
    egui_frame_nr: u64,
    egui_pass_nr: u64,
}

#[derive(Default)]
pub(super) struct State {
    candidate: Option<Receipt>,
    completed: Option<Receipt>,
}
impl State {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
    fn begin(&mut self) {
        self.candidate = None;
    }
    pub(super) fn record(&mut self, painter: &Painter, image: Rect, source: Source) {
        if !painter.is_visible() || painter.opacity() <= 0.0 || !image.is_finite() || !image.intersect(painter.clip_rect()).is_positive() {
            return;
        }
        self.candidate =
            Some(Receipt { source, egui_frame_nr: painter.ctx().cumulative_frame_nr(), egui_pass_nr: painter.ctx().cumulative_pass_nr() });
    }
    fn finish(&mut self, ctx: &Context) {
        // Layout may request a discard after the image was submitted. Keep the last
        // completed pass until a non-discarded pass publishes its candidate (or None).
        if !ctx.will_discard() {
            self.completed = self.candidate;
        }
        self.candidate = None;
    }
}

pub(crate) fn begin(app: &mut SolveApp) {
    app.render_workspace.displayed.begin();
}

pub(crate) fn finish(app: &mut SolveApp, ctx: &Context) {
    app.render_workspace.displayed.finish(ctx);
}

pub(super) fn inspect(app: &SolveApp) -> Option<Receipt> {
    if !super::active(app) || app.home.open || super::comparison_active(app) {
        return None;
    }
    let receipt = app.render_workspace.displayed.completed?;
    let live = matches!(receipt.source.kind, Kind::LivePreview | Kind::LiveFinal);
    (live != app.render_workspace.view.result).then_some(receipt)
}
