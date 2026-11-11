//! Surface analysis as a view draws it: the shading mode and the colours it uses.

use solidcore_record::Vec3;
use solidcraft_color::AnalysisColors;

/// A way of shading the model's faces to judge their shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnalysisMode {
    /// Reflected stripes: `stripes` across the view.
    Zebra { stripes: f64 },
    /// Faces by draft angle (degrees) to `pull`.
    Draft { pull: Vec3, angle: f64 },
    /// Colour by curvature, reaching the tight end at 1 / `radius`.
    Curvature { radius: f64 },
    /// Mirror finish reflecting a studio.
    Environment,
    /// Reachable along `dir` (facing it, nothing above) or blocked.
    Access { dir: Vec3 },
}

/// A surface analysis shading a picture: the mode and its colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneAnalysis {
    pub mode: AnalysisMode,
    pub colors: AnalysisColors,
}
