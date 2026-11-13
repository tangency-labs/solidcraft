//! How closely a display or export mesh follows the exact surfaces: the chord tolerance and the
//! facet angle, and how the planner reads them for a body of a given size.

use crate::mesher::size::Turn;

/// The most one facet of the display mesh turns (radians): a curved face or edge is divided
/// until no facet spans more of it than this, however small its radius is beside the body.
/// 20 degrees: a quarter turn is drawn with at least five facets, a full circle with at least 18.
pub const DISPLAY_ANGLE: f64 = std::f64::consts::PI / 9.0;

/// The sag, as a fraction of the chord tolerance, below which the angle no longer divides a
/// facet: at [`DISPLAY_ANGLE`] and the application's tolerance of 1/1000 of the body, a radius
/// under about 1/15000 of the body gets fewer facets than the angle asks for, and one under
/// a millionth of it only those of the chord tolerance.
const DISPLAY_ANGLE_FLOOR: f64 = 1e-3;

/// The finest angle a display mesh is divided by (one degree).
const MIN_DISPLAY_ANGLE: f64 = std::f64::consts::PI / 180.0;

/// How closely a mesh follows the exact surfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshTolerance {
    /// The farthest a facet or an edge segment may stand off the surface or curve it stands for
    /// (mm). Not finite or not positive: 0.05 mm. Never finer than a millionth of the body.
    pub chord: f64,
    /// The most a facet may turn (radians, at least one degree) where its sag is above a
    /// thousandth of `chord`; `None`: `chord` alone.
    pub angle: Option<f64>,
}

impl MeshTolerance {
    /// `chord` alone.
    #[must_use]
    pub const fn chord(chord: f64) -> Self {
        MeshTolerance { chord, angle: None }
    }

    /// What a viewport draws: `chord`, and no facet turning more than [`DISPLAY_ANGLE`], so
    /// small radii on a large body look round too.
    #[must_use]
    pub const fn display(chord: f64) -> Self {
        MeshTolerance {
            chord,
            angle: Some(DISPLAY_ANGLE),
        }
    }

    /// The display mesh of a body of size `size` (its box diagonal): 1/1000 of it, at least
    /// 1 µm, at [`DISPLAY_ANGLE`].
    #[must_use]
    pub fn display_of(size: f64) -> Self {
        MeshTolerance::display((size * 1e-3).max(1e-3))
    }

    /// For export to a mesh format (STL, 3MF, OBJ) of a body of size `size`: 1/5000 of it, at
    /// least 1 µm, the chord alone.
    #[must_use]
    pub fn export_of(size: f64) -> Self {
        MeshTolerance::chord((size * 2e-4).max(1e-3))
    }

    /// The planner's chord tolerance and facet angle for a body of size `size`.
    pub(crate) fn rules(&self, size: f64) -> (f64, Option<Turn>) {
        let tol = if self.chord.is_finite() && self.chord > 0.0 {
            self.chord
        } else {
            0.05
        };
        let tol = tol.max(size * 1e-6);
        let turn = self
            .angle
            .filter(|a| a.is_finite() && *a > 0.0)
            .map(|a| Turn {
                angle: a.max(MIN_DISPLAY_ANGLE),
                floor: tol * DISPLAY_ANGLE_FLOOR,
            });
        (tol, turn)
    }

    /// A key that tells tolerances apart bit for bit, for keeping meshes made at each.
    #[must_use]
    pub fn key(&self) -> (u64, u64) {
        (
            self.chord.to_bits(),
            self.angle.map_or(u64::MAX, f64::to_bits),
        )
    }
}
