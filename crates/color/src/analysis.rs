//! The colours a surface analysis is shaded in.

/// Surface analysis colours, straight sRGB 0..1 as the shader takes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnalysisColors {
    /// Draft Analysis: at least the draft angle, between, at most minus the angle.
    pub draft: [[f32; 3]; 3],
    /// Accessibility Analysis: reachable, blocked.
    pub access: [[f32; 3]; 2],
    /// Curvature Map: flat, the middle of the scale, at or below the analysis radius. Dark blue
    /// to light yellow in one straight blend, so the lightness rises the whole way and the scale
    /// reads with hue lost too (#975).
    pub curvature: [[f32; 3]; 3],
    /// Stripe the negative draft and blocked faces too, so colour is not the only cue.
    pub hatch: bool,
}

impl AnalysisColors {
    /// The light theme's colours (no colour vision mode): what headless pictures use.
    pub const DEFAULT: AnalysisColors = AnalysisColors {
        draft: [[0.20, 0.75, 0.30], [0.95, 0.80, 0.15], [0.85, 0.22, 0.20]],
        access: [[0.20, 0.75, 0.30], [0.85, 0.22, 0.20]],
        curvature: [[0.08, 0.12, 0.45], [0.53, 0.51, 0.425], [0.98, 0.90, 0.40]],
        hatch: false,
    };
}

impl Default for AnalysisColors {
    fn default() -> Self {
        AnalysisColors::DEFAULT
    }
}
