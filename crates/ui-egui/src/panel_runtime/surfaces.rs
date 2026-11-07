//! Captured, paint-only surface readability. Geometry and authored theme intent stay unchanged.
use std::sync::Arc;

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
    authored: Arc<Tokens>,
    fallback: Arc<Tokens>,
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
            *previous.authored == authored
                && previous.raw_fills == raw_fills
                && previous.force_opaque == force_opaque
                && previous.requested_bits == requested_bits
                && previous.dark == dark
        }) {
            return previous.clone();
        }
        let fallback = theme::panel_fallback_tokens(authored);
        let invalid = requested_opacity.is_some_and(|alpha| !alpha.is_finite() || !(0.0..=1.0).contains(&alpha));
        let resolve = |fill: Color32, opaque: bool| {
            if invalid {
                return fallback_appearance(&fallback, ContrastFailure::InvalidTokens);
            }
            let requested = if opaque || force_opaque { 1.0 } else { requested_opacity.unwrap_or(f32::from(fill.a()) / 255.0) };
            resolve_appearance(fill, &authored, &fallback, requested)
        };
        let opaque = resolve(raw_fills[0], true);
        let docked = resolve(raw_fills[0], false);
        let floating = resolve(raw_fills[1], false);
        Self { authored: Arc::new(authored), fallback: Arc::new(fallback), raw_fills, force_opaque, requested_bits, dark, opaque, docked, floating }
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

fn fallback_appearance(tokens: &Tokens, reason: ContrastFailure) -> HostSurfaceAppearance {
    HostSurfaceAppearance { fill: tokens.panel, contrast: ContrastOutcome::RetainReadableFallback { reason } }
}

fn resolve_appearance(fill: Color32, tokens: &Tokens, fallback: &Tokens, requested: f32) -> HostSurfaceAppearance {
    match theme::panel_surface_fill(fill, tokens, requested) {
        Some(fill) => HostSurfaceAppearance { fill, contrast: ContrastOutcome::Readable { effective_background_alpha: f32::from(fill.a()) / 255.0 } },
        None => fallback_appearance(fallback, ContrastFailure::UnreadableAtFullOpacity),
    }
}
