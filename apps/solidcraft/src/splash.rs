//! Desktop startup splash screen: shown in the root viewport before transitioning into the main
//! SolidCraft workspace window.

use std::time::{Duration, Instant};

const SPLASH_PNG: &[u8] = include_bytes!("../../../assets/branding/desktop/source/splash.png");
const MAX_WIDTH: u32 = 1024;
const MAX_HEIGHT: u32 = 512;

/// Logical size of the borderless startup splash viewport (exact 2:1 aspect ratio of 1024x512).
pub const SIZE: [f32; 2] = [640.0, 320.0];
/// Logical size of the main SolidCraft window once the splash screen transitions.
pub const MAIN_SIZE: [f32; 2] = [1280.0, 800.0];
/// Minimum logical size of the main SolidCraft window.
pub const MAIN_MIN_SIZE: [f32; 2] = [960.0, 600.0];
/// Default time the startup splash screen stays up before opening the main window.
const DEFAULT_DURATION_MS: u64 = 3500;

/// Configure `builder` as the borderless startup splash window when `splash` is true, or as the
/// main SolidCraft workspace window otherwise.
pub fn configure_viewport(builder: egui::ViewportBuilder, custom_titlebar: bool, splash: bool) -> egui::ViewportBuilder {
    if splash {
        builder
            .with_inner_size(SIZE)
            .with_min_inner_size(SIZE)
            .with_max_inner_size(SIZE)
            .with_resizable(false)
            .with_decorations(false)
            .with_titlebar_buttons_shown(false)
            .with_fullscreen(false)
            .with_maximized(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
    } else {
        builder.with_inner_size(MAIN_SIZE).with_min_inner_size(MAIN_MIN_SIZE).with_decorations(!custom_titlebar)
    }
}

/// Active desktop startup splash screen state.
pub struct Splash {
    started_at: Instant,
    duration: Duration,
    texture: Option<egui::TextureHandle>,
    frames: u32,
}

impl Splash {
    /// Start the splash screen when `enabled` is true.
    pub fn new(enabled: bool) -> Option<Self> {
        if !enabled {
            return None;
        }
        let ms = std::env::var("SOLIDCRAFT_SPLASH_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|ms| ms.clamp(100, 30_000))
            .unwrap_or(DEFAULT_DURATION_MS);
        Some(Self { started_at: Instant::now(), duration: Duration::from_millis(ms), texture: None, frames: 0 })
    }

    /// Fraction of the splash screen timer elapsed (`0.0..=1.0`).
    pub fn progress(&self, now: Instant) -> f32 {
        let total = self.duration.as_secs_f32();
        if total <= 0.0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.started_at).as_secs_f32() / total).clamp(0.0, 1.0)
    }

    /// Whether the splash screen should close on this frame (timer elapsed after at least two
    /// presented frames, or the user pressed Escape).
    pub fn should_finish(&self, now: Instant, clicked_or_key: bool) -> bool {
        self.frames >= 2 && (clicked_or_key || now.saturating_duration_since(self.started_at) >= self.duration)
    }

    /// Advance the splash screen for one frame; returns `true` while the splash screen remains open
    /// and `false` once it has transitioned the viewport into the main SolidCraft window.
    pub fn tick(&mut self, ctx: &egui::Context, custom_titlebar: bool) -> bool {
        self.frames = self.frames.saturating_add(1);
        if self.frames == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(SIZE[0], SIZE[1])));
        }
        #[cfg(target_os = "macos")]
        if self.frames <= 2
            && let Some(mtm) = objc2::MainThreadMarker::new()
        {
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            if self.frames == 1 {
                let size = objc2_foundation::NSSize::new(f64::from(SIZE[0]), f64::from(SIZE[1]));
                for window in app.windows() {
                    window.setContentMinSize(size);
                    window.setContentMaxSize(size);
                    window.setContentSize(size);
                    window.center();
                }
            }
            app.activate();
        }
        let dismiss_input = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let now = Instant::now();
        if self.should_finish(now, dismiss_input) {
            self.texture = None;
            transition_to_main_window(ctx, custom_titlebar);
            return false;
        }
        let remaining = self.duration.saturating_sub(now.saturating_duration_since(self.started_at));
        ctx.request_repaint_after(remaining.min(Duration::from_millis(16)));
        true
    }

    /// Draw the splash artwork preserving its exact 2:1 (`1024 × 512`) aspect ratio with a bottom
    /// loading progress bar.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let tokens = solidcraft_ui_egui::theme::Tokens::dark();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, tokens.app_bar);

        let texture = self.texture.get_or_insert_with(|| {
            let image = solidcraft_ui_egui::help::decode_png(SPLASH_PNG, MAX_WIDTH, MAX_HEIGHT)
                .unwrap_or_else(|| egui::ColorImage::filled([1, 1], tokens.app_bar));
            ui.ctx().load_texture("solidcraft_splash", image, egui::TextureOptions::LINEAR)
        });
        let aspect = MAX_WIDTH as f32 / MAX_HEIGHT as f32;
        let (w, h) = if rect.width() / rect.height().max(1.0) > aspect {
            (rect.height() * aspect, rect.height())
        } else {
            (rect.width(), rect.width() / aspect)
        };
        let image_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h));
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture.id(), image_rect, uv, egui::Color32::WHITE);

        let progress = self.progress(Instant::now());
        let bar_height = 24.0;
        let bar_rect = egui::Rect::from_min_max(egui::pos2(image_rect.left(), image_rect.bottom() - bar_height), image_rect.max);
        painter.rect_filled(bar_rect, 0.0, tokens.app_bar.gamma_multiply(0.84));

        let pad_x = tokens.space_12;
        let text_y = bar_rect.center().y - 1.5;
        painter.text(
            egui::pos2(bar_rect.left() + pad_x, text_y),
            egui::Align2::LEFT_CENTER,
            solidcraft_ui_egui::i18n::tr("splash.starting"),
            egui::FontId::proportional(tokens.font_small),
            tokens.text,
        );
        painter.text(
            egui::pos2(bar_rect.right() - pad_x, text_y),
            egui::Align2::RIGHT_CENTER,
            format!("v{}  \u{00b7}  {:>3.0}%", solidcraft_ui_egui::help::VERSION, (progress * 100.0).round()),
            egui::FontId::proportional(tokens.font_small),
            tokens.text_dim,
        );

        let progress_height = 3.0;
        let track_rect = egui::Rect::from_min_max(egui::pos2(image_rect.left(), image_rect.bottom() - progress_height), image_rect.max);
        painter.rect_filled(track_rect, 0.0, tokens.border);
        let fill_width = image_rect.width() * progress;
        let fill_rect = egui::Rect::from_min_max(track_rect.min, egui::pos2(track_rect.left() + fill_width, track_rect.bottom()));
        painter.rect_filled(fill_rect, 0.0, tokens.accent);

        painter.rect_stroke(image_rect, 0.0, egui::Stroke::new(1.0, tokens.border), egui::StrokeKind::Inside);
    }
}

/// Transition the root viewport from the borderless splash screen to the main SolidCraft window.
fn transition_to_main_window(ctx: &egui::Context, custom_titlebar: bool) {
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(16384.0, 16384.0)));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(MAIN_MIN_SIZE[0], MAIN_MIN_SIZE[1])));
    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!custom_titlebar));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MAIN_SIZE[0], MAIN_SIZE[1])));
    if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
        let center = outer.center();
        let x = (center.x - MAIN_SIZE[0] * 0.5).max(20.0);
        let y = (center.y - MAIN_SIZE[1] * 0.5).max(40.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let min_size = objc2_foundation::NSSize::new(f64::from(MAIN_MIN_SIZE[0]), f64::from(MAIN_MIN_SIZE[1]));
        let max_size = objc2_foundation::NSSize::new(16384.0, 16384.0);
        let main_size = objc2_foundation::NSSize::new(f64::from(MAIN_SIZE[0]), f64::from(MAIN_SIZE[1]));
        for window in app.windows() {
            window.setStyleMask(
                objc2_app_kit::NSWindowStyleMask::Titled
                    | objc2_app_kit::NSWindowStyleMask::Closable
                    | objc2_app_kit::NSWindowStyleMask::Miniaturizable
                    | objc2_app_kit::NSWindowStyleMask::Resizable
                    | objc2_app_kit::NSWindowStyleMask::FullSizeContentView,
            );
            window.setTitlebarAppearsTransparent(true);
            window.setTitleVisibility(objc2_app_kit::NSWindowTitleVisibility::Hidden);
            window.setContentMinSize(min_size);
            window.setContentMaxSize(max_size);
            window.setContentSize(main_size);
            window.center();
            for button in [
                objc2_app_kit::NSWindowButton::CloseButton,
                objc2_app_kit::NSWindowButton::MiniaturizeButton,
                objc2_app_kit::NSWindowButton::ZoomButton,
            ] {
                if let Some(btn) = window.standardWindowButton(button) {
                    btn.setHidden(false);
                }
            }
        }
        app.activate();
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_splash_png_decodes_at_expected_dimensions() {
        let image = solidcraft_ui_egui::help::decode_png(SPLASH_PNG, MAX_WIDTH, MAX_HEIGHT).expect("bundled splash PNG must decode");
        assert_eq!(image.size, [MAX_WIDTH as usize, MAX_HEIGHT as usize]);
    }

    #[test]
    fn splash_state_tracks_progress_and_dismisses_on_input_or_timer() {
        assert!(Splash::new(false).is_none());
        let mut splash = Splash::new(true).expect("enabled splash");
        let start = splash.started_at;
        assert_eq!(splash.progress(start), 0.0);
        assert!(!splash.should_finish(start + Duration::from_secs(10), true), "waits for at least 2 frames");
        splash.frames = 2;
        assert!(splash.should_finish(start, true), "click or key dismisses after 2 frames");
        assert!(!splash.should_finish(start + Duration::from_millis(100), false));
        assert!(splash.should_finish(start + splash.duration, false));
        assert_eq!(splash.progress(start + splash.duration), 1.0);
    }

    #[test]
    fn configure_viewport_sets_splash_or_main_geometry() {
        let splash_vp = configure_viewport(egui::ViewportBuilder::default(), false, true);
        assert_eq!(splash_vp.inner_size, Some(egui::vec2(SIZE[0], SIZE[1])));
        assert_eq!(splash_vp.resizable, Some(false));
        assert_eq!(splash_vp.decorations, Some(false));

        let main_vp = configure_viewport(egui::ViewportBuilder::default(), false, false);
        assert_eq!(main_vp.inner_size, Some(egui::vec2(MAIN_SIZE[0], MAIN_SIZE[1])));
        assert_eq!(main_vp.min_inner_size, Some(egui::vec2(MAIN_MIN_SIZE[0], MAIN_MIN_SIZE[1])));
        assert_eq!(main_vp.decorations, Some(true));
    }
}
