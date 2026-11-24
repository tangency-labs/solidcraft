//! Surface analysis as a view draws it: the shading mode and the colours it uses.

use solidcore_record::Vec3;
use solidcraft_color::AnalysisColors;

/// A way of shading the model's faces to judge their shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnalysisMode {
    /// Reflected stripes: `stripes` across the view, reflections of light bars along
    /// `direction` (world; +Z stands them upright around the viewer), each white for `width` of
    /// its period (0.05 to 0.95; 0.5 is half white, half black).
    Zebra { stripes: f64, direction: Vec3, width: f64 },
    /// Faces by draft angle (degrees) to `pull`.
    Draft { pull: Vec3, angle: f64 },
    /// Colour by curvature, reaching the tight end at 1 / `radius`.
    Curvature { radius: f64 },
    /// Mirror finish reflecting a studio.
    Environment,
    /// Reachable along `dir` (facing it, nothing above) or blocked.
    Access { dir: Vec3 },
    /// Colour by a value given at each mesh vertex ([`crate::SceneMesh::values`]: a measured
    /// curvature, say), on the curvature colours' ramp from `low` (flat's colour) through the
    /// middle to `high` (tight's): with `low = -high`, zero is the middle colour, so a signed map
    /// reads its sign at a glance. A mesh without values is coloured as [`Self::Curvature`] does
    /// at a radius of 1 mm.
    Values { low: f64, high: f64 },
}

/// A surface analysis shading a picture: the mode and its colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneAnalysis {
    pub mode: AnalysisMode,
    pub colors: AnalysisColors,
}
