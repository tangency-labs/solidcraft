//! Captured, paint-only surface readability. Geometry and authored theme intent stay unchanged.
use egui::Color32;
use solidcraft_engine::panels::{ContrastFailure, ContrastOutcome, PanelSurface};

use crate::theme::{self, Tokens};

#[derive(Clone, Debug, PartialEq)]
pub struct HostSurfaceAppearance {
    pub fill: Color32,
    pub contrast: ContrastOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostSurfaceStyle {
    authored: Tokens,
    fallback: Tokens,
    raw_fills: [Color32; 2],
    force_opaque: bool,
    requested_bits: Option<u32>,
    dark: bool,
    opaque: HostSurfaceAppearance,
    docked: HostSurfaceAppearance,
    floating: HostSurfaceAppearance,
}

impl HostSurfaceStyle {
    /// `requested_opacity` is normalized opacity, not translucency or percent. None
    /// retains the actual shared frame's effective theme/blur alpha.
    pub fn capture(ctx: &egui::Context, force_opaque: bool, requested_opacity: Option<f32>, previous: Option<&Self>) -> Self {
        let authored = Tokens::get();
        let raw_fills = [crate::ui_kit::panel_host_frame(ctx, false).fill, crate::ui_kit::panel_host_frame(ctx, true).fill];
        let requested_bits = requested_opacity.map(f32::to_bits);
        let dark = theme::is_dark();
        if let Some(previous) = previous.filter(|previous| {
            previous.authored == authored
                && previous.raw_fills == raw_fills
                && previous.force_opaque == force_opaque
                && previous.requested_bits == requested_bits
                && previous.dark == dark
        }) {
            return previous.clone();
        }
        let fallback = fallback_palette(authored);
        let invalid = requested_opacity.is_some_and(|alpha| !alpha.is_finite() || !(0.0..=1.0).contains(&alpha));
        let resolve = |fill: Color32, opaque: bool| {
            if invalid {
                return fallback_appearance(fallback, ContrastFailure::InvalidTokens);
            }
            let requested = if opaque || force_opaque { 1.0 } else { requested_opacity.unwrap_or(f32::from(fill.a()) / 255.0) };
            resolve_appearance(fill, authored, fallback, requested)
        };
        Self {
            authored,
            fallback,
            raw_fills,
            force_opaque,
            requested_bits,
            dark,
            opaque: resolve(raw_fills[0], true),
            docked: resolve(raw_fills[0], false),
            floating: resolve(raw_fills[1], false),
        }
    }

    pub fn appearance(&self, surface: PanelSurface, floating: bool) -> &HostSurfaceAppearance {
        if surface == PanelSurface::Opaque || self.force_opaque {
            &self.opaque
        } else if floating {
            &self.floating
        } else {
            &self.docked
        }
    }

    pub fn palette(&self, appearance: &HostSurfaceAppearance) -> &Tokens {
        if matches!(appearance.contrast, ContrastOutcome::RetainReadableFallback { .. }) { &self.fallback } else { &self.authored }
    }
}

fn fallback_palette(mut authored: Tokens) -> Tokens {
    // Bypass custom/published themes. These existing built-in pairs have their own
    // contrast tests. Copy only surface/ink roles to retain captured layout metrics.
    let known = if theme::is_dark() { Tokens::dark() } else { Tokens::light() };
    authored.panel = known.file_background.to_opaque();
    authored.panel_header = authored.panel;
    authored.text = known.file_primary_ink;
    authored.form_ink = known.file_primary_ink;
    authored.dialog_label = known.file_primary_ink;
    authored.text_dim = known.file_dim;
    authored.form_dim = known.file_dim;
    authored
}

fn fallback_appearance(tokens: Tokens, reason: ContrastFailure) -> HostSurfaceAppearance {
    HostSurfaceAppearance { fill: tokens.panel, contrast: ContrastOutcome::RetainReadableFallback { reason } }
}

fn resolve_appearance(fill: Color32, tokens: Tokens, fallback: Tokens, requested: f32) -> HostSurfaceAppearance {
    let [r, g, b, _] = fill.to_srgba_unmultiplied();
    if !surface_readable(Color32::from_rgb(r, g, b), &tokens) {
        return fallback_appearance(fallback, ContrastFailure::UnreadableAtFullOpacity);
    }
    // Every candidate is the exact quantized fill the renderer receives. Round up
    // so the effective alpha can never undercut the caller's requested opacity.
    let first = (requested * 255.0).ceil() as u8;
    for alpha in first..=255 {
        let candidate = Color32::from_rgba_unmultiplied(r, g, b, alpha);
        if surface_readable(candidate, &tokens) {
            return HostSurfaceAppearance {
                fill: candidate,
                contrast: ContrastOutcome::Readable { effective_background_alpha: f32::from(alpha) / 255.0 },
            };
        }
    }
    fallback_appearance(fallback, ContrastFailure::UnreadableAtFullOpacity)
}

fn luminance(color: Color32) -> f64 {
    let decode = solidcraft_engine::geom::srgb::decode8;
    0.2126 * decode(color.r()) + 0.7152 * decode(color.g()) + 0.0722 * decode(color.b())
}

pub(super) fn surface_readable(fill: Color32, tokens: &Tokens) -> bool {
    let (low, _) = composite_bounds(fill, Color32::BLACK);
    let (_, high) = composite_bounds(fill, Color32::WHITE);
    let (background_low, background_high) = (luminance(low), luminance(high));
    [tokens.text, tokens.text_dim, tokens.form_ink, tokens.form_dim, tokens.dialog_label].into_iter().all(|ink| {
        // RGBA inks remain authored. Conservative intervals include every possible
        // composited text luminance, including an interior contrast minimum.
        let ink_low = luminance(composite_bounds(ink, low).0);
        let ink_high = luminance(composite_bounds(ink, high).1);
        if ink_low <= background_high && ink_high >= background_low {
            false
        } else if ink_low > background_high {
            (ink_low + 0.05) / (background_high + 0.05) >= 4.5
        } else {
            (background_low + 0.05) / (ink_high + 0.05) >= 4.5
        }
    })
}

/// Enclose the actual stored-premultiplied source-over, without unpremultiplying
/// and rounding the source again. egui-wgpu's renderer uses One/OneMinusSrcAlpha;
/// its two WGSL entry points blend gamma codes or decoded premultiplied codes.
/// Include both framebuffer formats and the shader's <half-byte source dither,
/// then round outward to cover framebuffer quantization. This intentionally
/// overestimates uncertainty instead of promising threshold contrast falsely.
pub(super) fn composite_bounds(source: Color32, background: Color32) -> (Color32, Color32) {
    use solidcraft_engine::geom::srgb::{decode, encode};
    if source.a() == 255 {
        // Flat opaque integer colours survive the shader's <0.5-byte dither and
        // final nearest-byte conversion unchanged on either framebuffer format.
        return (source, source);
    }
    let remain = 1.0 - f64::from(source.a()) / 255.0;
    let channel = |source: u8, background: u8| {
        let source = f64::from(source);
        let background = f64::from(background);
        let low_source = (source - 0.5).max(0.0);
        let high_source = (source + 0.5).min(255.0);
        let gamma_low = low_source + background * remain;
        let gamma_high = high_source + background * remain;
        let linear_low = encode(decode(low_source / 255.0) + decode(background / 255.0) * remain) * 255.0;
        let linear_high = encode(decode(high_source / 255.0) + decode(background / 255.0) * remain) * 255.0;
        (gamma_low.min(linear_low).floor().clamp(0.0, 255.0) as u8, gamma_high.max(linear_high).ceil().clamp(0.0, 255.0) as u8)
    };
    let (r0, r1) = channel(source.r(), background.r());
    let (g0, g1) = channel(source.g(), background.g());
    let (b0, b1) = channel(source.b(), background.b());
    (Color32::from_rgb(r0, g0, b0), Color32::from_rgb(r1, g1, b1))
}
