//! The router's sheet development ([`solidcraft_kernel_api::develop::Develop`]), as
//! `solidcraft_kernel::develop`: a sheet laid out flat and folded back, the record's motions,
//! bends and laps with their maths as methods ([`Rigid2Ops`], [`Rigid3Ops`], [`BendOps`],
//! [`LapOps`]), beads and contours, and the cubic pieces a curve is carried into the flat by
//! ([`hermite`]). A fold, a bend in space and a smooth piece hold the kernel's own form.

use solidcore_record::brep::Curve;
use solidcore_record::develop::{Bead, BeadForm, BeadOutline, Bend, Development, Lap, Mismatch, Rigid2, Rigid3, Sheet, Surface};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::develop::Develop;

pub use solidcraft_kernel_api::develop::{BeadAgainst, BeadRefusal, ContourRefusal, JointRefusal, PanelRefusal, Refusal, RuleRefusal};

use super::{Which, which};
use crate::backend::Solidcore;

/// The kernel that runs sheet development. This build holds solidcore only; a fold, a bend and a
/// smooth piece hold its forms.
fn kernel() -> Solidcore {
    match which(OpKind::Develop) {
        Which::Solidcore => Solidcore,
    }
}

/// The most panels one development takes.
pub const MAX_PANELS: usize = <Solidcore as Develop>::MAX_PANELS;
/// The most joints one development takes.
pub const MAX_JOINTS: usize = <Solidcore as Develop>::MAX_JOINTS;
/// The smallest radius a bead takes (mm).
pub const MIN_BEAD_RADIUS: f64 = <Solidcore as Develop>::MIN_BEAD_RADIUS;

/// The sheet laid out flat; `anchors` says where some panels' neutral charts lie in the flat.
///
/// # Errors
/// The kernel's [`Refusal`]: the panel or joint it cannot lay out, and why.
pub fn develop(sheet: &Sheet, anchors: &[(usize, Rigid2)]) -> Result<Development, Refusal> {
    kernel().develop(sheet, anchors)
}

/// A developed sheet folded; `roots` says where the first panel of each of the development's
/// pieces lies in space.
///
/// # Errors
/// The kernel's [`Refusal`]: a rule no sheet can have, a root for each piece missing, an anchor
/// with no frame on its panel.
pub fn fold(sheet: &Sheet, dev: &Development, roots: &[Rigid3]) -> Result<Fold, Refusal> {
    let f = kernel().fold(sheet, dev, roots)?;
    Ok(Fold { places: f.places, bends: f.bends.into_iter().map(|b| b.map(Bent)).collect(), loops: f.loops, form: f.form })
}

/// Where the edges of `a` cross those of `b`: each crossing's edge and parameter on either, and its
/// point.
#[must_use]
pub fn crossings(a: &[Seg2], b: &[Seg2]) -> Vec<(usize, f64, usize, f64, Vec2)> {
    kernel().crossings(a, b)
}

/// The surfaces of a contour flange's profile `segs`, `depth` into the material: the material right
/// of the travel, or left when `flip`; corners turning less than `smooth` radians meet on the
/// common normal.
///
/// # Errors
/// The kernel's refusal, as its kind.
pub fn offset_contour(segs: &[Seg2], depth: f64, thickness: f64, flip: bool, smooth: f64) -> Result<Vec<Seg2>, ContourRefusal> {
    kernel().offset_contour(segs, depth, thickness, flip, smooth)
}

/// A curve through `points` in order: a line where they lie on one, else a clamped cubic spline
/// through each.
#[must_use]
pub fn curve_through(points: &[Vec2]) -> Vec<Seg2> {
    kernel().curve_through(points)
}

/// Bead `b`'s footprint in its panel's chart, counter-clockwise, on a sheet `t` thick.
///
/// # Errors
/// The kernel's refusal, as its kind.
pub fn bead_footprint(b: &Bead, t: f64) -> Result<[Vec2; 4], BeadRefusal> {
    kernel().bead_footprint(b, t)
}

/// Where bead `b`'s footprint lies against the outline `l`, within `near` counting as meeting it.
///
/// # Errors
/// The kernel's refusal, as its kind.
pub fn bead_against(b: &Bead, t: f64, l: &Loop2, near: f64) -> Result<BeadAgainst, BeadRefusal> {
    kernel().bead_against(b, t, l, near)
}

/// Whether the segment from `a` to `b` lies wholly in `region`; `None` where it cannot be decided.
#[must_use]
pub fn segment_in_region(region: &Region2, a: Vec2, b: Vec2) -> Option<bool> {
    kernel().segment_in_region(region, a, b)
}

/// The flanks' angle (radians) of a bead `depth` deep and `width` wide leaving a crown
/// `crown_width` wide.
///
/// # Errors
/// The kernel's refusal, as its kind.
pub fn bead_flank_angle(depth: f64, width: f64, crown_width: f64) -> Result<f64, BeadRefusal> {
    kernel().bead_flank_angle(depth, width, crown_width)
}

/// A developed sheet folded ([`fold`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Fold {
    /// Where each panel's frame lies in space.
    pub places: Vec<Option<Rigid3>>,
    /// Where each joint's bend lies in space (none for a seam or a rip).
    pub bends: Vec<Option<Bent>>,
    /// Bends and seams that close a ring, where the fold reached both panels another way: how far
    /// the joint itself would put its `b` panel from where the ring put it.
    pub loops: Vec<Mismatch>,
    form: <Solidcore as Develop>::Fold,
}

impl Fold {
    /// Where a point of panel `i`'s flat material goes, `depth` into the material.
    #[must_use]
    pub fn panel_point(&self, i: usize, p: Vec2, depth: f64) -> Option<Vec3> {
        kernel().fold_panel_point(&self.form, i, p, depth)
    }

    /// The point of panel `i`'s surface at the parameters `q`, `depth` into the material.
    #[must_use]
    pub fn surface_point(&self, i: usize, q: Vec2, depth: f64) -> Option<Vec3> {
        kernel().fold_surface_point(&self.form, i, q, depth)
    }

    /// Where panel `i`'s neutral chart lies in the flat.
    #[must_use]
    pub fn placement(&self, i: usize) -> Option<Rigid2> {
        kernel().fold_placement(&self.form, i)
    }

    /// Where the point of panel `i`'s surface at the parameters `q` lies in the flat.
    #[must_use]
    pub fn flat_point(&self, i: usize, q: Vec2) -> Option<Vec2> {
        kernel().fold_flat_point(&self.form, i, q)
    }

    /// The parameters of panel `i`'s surface at a point of its flat material.
    #[must_use]
    pub fn param(&self, i: usize, p: Vec2) -> Option<Vec2> {
        kernel().fold_param(&self.form, i, p)
    }

    /// The surface panel `i` lies on.
    #[must_use]
    pub fn surface(&self, i: usize) -> Option<Surface> {
        kernel().fold_surface(&self.form, i)
    }

    /// A roll's axis: a point of it and its direction; `None` for a plane.
    #[must_use]
    pub fn axis(&self, i: usize) -> Option<(Vec3, Vec3)> {
        kernel().fold_axis(&self.form, i)
    }

    /// What bead `b` does to the folded solid of a sheet `t` thick: the footprint its panel's faces
    /// leave open, in the flat, and its faces in space.
    ///
    /// # Errors
    /// The kernel's [`BeadRefusal`]: a section that does not fit, or a panel with no place.
    pub fn bead_form(&self, b: &Bead, t: f64) -> Result<BeadForm, BeadRefusal> {
        kernel().fold_bead_form(&self.form, b, t)
    }

    /// Bead `b` in the flat of a sheet `t` thick; with `mirror`, as a flat pattern is read.
    ///
    /// # Errors
    /// The kernel's [`BeadRefusal`]: a section that does not fit, or a panel with no place.
    pub fn bead_outline(&self, b: &Bead, t: f64, mirror: bool) -> Result<BeadOutline, BeadRefusal> {
        kernel().fold_bead_outline(&self.form, b, t, mirror)
    }

    /// Where a note `height` high along bead `b` stands in the flat: its anchor and its angle
    /// (radians).
    ///
    /// # Errors
    /// The kernel's [`BeadRefusal`]: a section that does not fit, or a panel with no place.
    pub fn bead_note(&self, b: &Bead, t: f64, mirror: bool, height: f64) -> Result<(Vec2, f64), BeadRefusal> {
        kernel().fold_bead_note(&self.form, b, t, mirror, height)
    }

    /// Where a straight line from `a` to `b` on the plane through `origin` with unit `normal` lies
    /// in planar panel `i`'s chart: its ends, and whether the plane is the panel's far face.
    #[must_use]
    pub fn line_in_chart(&self, i: usize, t: f64, origin: Vec3, normal: Vec3, a: Vec3, b: Vec3) -> Option<(Vec2, Vec2, bool)> {
        kernel().fold_line_in_chart(&self.form, i, t, origin, normal, a, b)
    }
}

/// A bend in space, and its flat zone wrapped onto it ([`Fold::bends`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bent(<Solidcore as Develop>::Bent);

impl Bent {
    /// Where a point of the bend's flat zone goes, `depth` into the material.
    #[must_use]
    pub fn point(&self, p: Vec2, depth: f64) -> Vec3 {
        kernel().bent_point(&self.0, p, depth)
    }

    /// The point `along` the joint, turned `phi` round the bend from the `a` panel's tangent line,
    /// `depth` into the material.
    #[must_use]
    pub fn at(&self, along: f64, phi: f64, depth: f64) -> Vec3 {
        kernel().bent_at(&self.0, along, phi, depth)
    }

    /// A point of the flat zone as its length along the joint and its angle round the bend.
    #[must_use]
    pub fn wrapped(&self, p: Vec2) -> (f64, f64) {
        kernel().bent_wrapped(&self.0, p)
    }

    /// Whether the flat line from `a` to `b` runs straight across the bend.
    #[must_use]
    pub fn straight_across(&self, a: Vec2, b: Vec2) -> bool {
        kernel().bent_straight_across(&self.0, a, b)
    }

    /// Where a point of space lies in the bend: its flat point, depth and the way deeper; `None` on
    /// the axis.
    #[must_use]
    pub fn unwrapped(&self, p: Vec3) -> Option<(Vec2, f64, Vec3)> {
        kernel().bent_unwrapped(&self.0, p)
    }

    /// A point of the bend's axis and its direction.
    #[must_use]
    pub fn axis(&self) -> (Vec3, Vec3) {
        kernel().bent_axis(&self.0)
    }

    /// How far the bend turns, in radians.
    #[must_use]
    pub fn angle(&self) -> f64 {
        kernel().bent_angle(&self.0)
    }

    /// The joint's way in the flat, from the `a` panel's side.
    #[must_use]
    pub fn flat_way(&self) -> Vec2 {
        kernel().bent_flat_way(&self.0)
    }
}

/// The kernel's rigid motions of the plane on the record's [`Rigid2`].
pub trait Rigid2Ops: Sized {
    /// The turn by `angle` radians about the origin.
    fn turn(angle: f64) -> Self;
    /// The motion putting `from` on `to` and turning `from_dir` to `to_dir`; `None` when either
    /// direction has no length.
    fn onto(from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Self>;
    /// The turn this motion makes, in radians.
    fn angle(&self) -> f64;
    /// The vector `v` turned by this motion.
    fn vector(&self, v: Vec2) -> Vec2;
    /// The point `p` moved by this motion.
    fn point(&self, p: Vec2) -> Vec2;
    /// this motion, then `next`.
    fn then(&self, next: &Self) -> Self;
    /// The motion undoing this motion.
    fn inverse(&self) -> Self;
    /// The segment `s` moved by this motion.
    fn seg(&self, s: &Seg2) -> Seg2;
    /// The loop `l` moved by this motion.
    fn loop2(&self, l: &Loop2) -> Loop2;
    /// The region `r` moved by this motion.
    fn region(&self, r: &Region2) -> Region2;
}

impl Rigid2Ops for Rigid2 {
    fn turn(angle: f64) -> Rigid2 {
        kernel().rigid2_turn(angle)
    }
    fn onto(from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Rigid2> {
        kernel().rigid2_onto(from, from_dir, to, to_dir)
    }
    fn angle(&self) -> f64 {
        kernel().rigid2_angle(self)
    }
    fn vector(&self, v: Vec2) -> Vec2 {
        kernel().rigid2_vector(self, v)
    }
    fn point(&self, p: Vec2) -> Vec2 {
        kernel().rigid2_point(self, p)
    }
    fn then(&self, next: &Rigid2) -> Rigid2 {
        kernel().rigid2_then(self, next)
    }
    fn inverse(&self) -> Rigid2 {
        kernel().rigid2_inverse(self)
    }
    fn seg(&self, s: &Seg2) -> Seg2 {
        kernel().rigid2_seg(self, s)
    }
    fn loop2(&self, l: &Loop2) -> Loop2 {
        kernel().rigid2_loop(self, l)
    }
    fn region(&self, r: &Region2) -> Region2 {
        kernel().rigid2_region(self, r)
    }
}

/// The kernel's rigid motions of space on the record's [`Rigid3`].
pub trait Rigid3Ops: Sized {
    /// The motion to the frame at `origin` along `x` and `y`; `None` for axes it cannot square.
    fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Option<Self>;
    /// The vector `v` turned by this motion.
    fn vector(&self, v: Vec3) -> Vec3;
    /// The point `p` moved by this motion.
    fn point(&self, p: Vec3) -> Vec3;
    /// this motion, then `next`.
    fn then(&self, next: &Self) -> Self;
    /// The motion undoing this motion.
    fn inverse(&self) -> Self;
    /// The angle between the turns of this motion and `other`, in radians.
    fn angle_to(&self, other: &Self) -> f64;
}

impl Rigid3Ops for Rigid3 {
    fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Option<Rigid3> {
        kernel().rigid3_frame(origin, x, y)
    }
    fn vector(&self, v: Vec3) -> Vec3 {
        kernel().rigid3_vector(self, v)
    }
    fn point(&self, p: Vec3) -> Vec3 {
        kernel().rigid3_point(self, p)
    }
    fn then(&self, next: &Rigid3) -> Rigid3 {
        kernel().rigid3_then(self, next)
    }
    fn inverse(&self) -> Rigid3 {
        kernel().rigid3_inverse(self)
    }
    fn angle_to(&self, other: &Rigid3) -> f64 {
        kernel().rigid3_angle_to(self, other)
    }
}

/// The kernel's closed forms of a bend on the record's [`Bend`].
pub trait BendOps: Sized {
    /// The bend's allowance on a sheet `thickness` thick with `k_factor`.
    fn allowance(&self, thickness: f64, k_factor: f64) -> f64;
    /// The bend's setback on a sheet `thickness` thick.
    fn setback(&self, thickness: f64) -> f64;
}

impl BendOps for Bend {
    fn allowance(&self, thickness: f64, k_factor: f64) -> f64 {
        kernel().bend_allowance(self, thickness, k_factor)
    }
    fn setback(&self, thickness: f64) -> f64 {
        kernel().bend_setback(self, thickness)
    }
}

/// The kernel's cut-backs of a lap on the record's [`Lap`].
pub trait LapOps: Sized {
    /// What the lap cuts from each side at a joint turning `turn`, on a sheet `thickness` thick.
    fn cuts(&self, turn: f64, thickness: f64) -> (f64, f64);
}

impl LapOps for Lap {
    fn cuts(&self, turn: f64, thickness: f64) -> (f64, f64) {
        kernel().lap_cuts(self, turn, thickness)
    }
}

/// The cubic pieces a record curve is carried into the flat by, each proved within a tolerance
/// of it.
pub mod hermite {
    use solidcore_record::brep::Curve;
    use solidcore_record::{Seg2, Vec2, Vec3};
    use solidcraft_kernel_api::develop::Develop;

    use super::kernel;
    use crate::backend::Solidcore;

    /// The share of the sizes a carried curve's bounds are rounded outward by.
    pub const ROUNDING: f64 = <Solidcore as Develop>::HERMITE_ROUNDING;

    /// A piece of a curve's parameter over which the curve is smooth ([`smooth_pieces`]).
    #[derive(Clone, Debug, PartialEq)]
    pub struct Smooth(<Solidcore as Develop>::Smooth);

    impl Smooth {
        /// The piece of the parameter.
        #[must_use]
        pub fn span(&self) -> [f64; 2] {
            kernel().smooth_span(&self.0)
        }

        /// At least the length of each of the curve's first four derivatives on the piece.
        #[must_use]
        pub fn rates(&self) -> [f64; 4] {
            kernel().smooth_rates(&self.0)
        }

        /// At most how far a point of the piece, as read at, lies from the record's curve.
        #[must_use]
        pub fn slack(&self) -> f64 {
            kernel().smooth_slack(&self.0)
        }

        /// The curve's point and first derivative at `t`, from the piece's own form.
        #[must_use]
        pub fn at(&self, t: f64) -> Option<(Vec3, Vec3)> {
            kernel().smooth_at(&self.0, t)
        }
    }

    /// The pieces of `curve` over `span` on which it is smooth, in order, each with bounds on its
    /// first four derivatives.
    #[must_use]
    pub fn smooth_pieces(curve: &Curve, span: [f64; 2]) -> Option<Vec<Smooth>> {
        kernel().smooth_pieces(curve, span).map(|pieces| pieces.into_iter().map(Smooth).collect())
    }

    /// A bound on the fourth derivative of a map of a curve, from bounds on the curve's and the
    /// map's.
    #[must_use]
    pub fn fourth(c: [f64; 4], g: [f64; 4]) -> f64 {
        kernel().hermite_fourth(c, g)
    }

    /// How far a cubic over a step `h` lies at most from the curve it follows, its fourth
    /// derivative at most `fourth`.
    #[must_use]
    pub fn remainder(h: f64, fourth: f64) -> f64 {
        kernel().hermite_remainder(h, fourth)
    }

    /// The cubic from `p0` at rate `d0` to `p3` at rate `d3` over a step `h`.
    #[must_use]
    pub fn cubic(p0: Vec2, d0: Vec2, p3: Vec2, d3: Vec2, h: f64) -> Seg2 {
        kernel().hermite_cubic(p0, d0, p3, d3, h)
    }

    /// The parameters over `span` between which cubic pieces stand proved within `tol`, `off`
    /// bounding each piece's distance.
    pub fn nodes(off: impl Fn(f64, f64) -> Option<f64>, span: [f64; 2], tol: f64) -> Option<Vec<f64>> {
        kernel().hermite_nodes(&off, span, tol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::develop as ops;
    use solidcore_ops::develop::Rigid2Ops as _;

    #[test]
    fn routed_development_is_solidcores() {
        let turn = <Rigid2 as Rigid2Ops>::turn(0.5);
        assert_eq!(turn, <Rigid2 as ops::Rigid2Ops>::turn(0.5));
        let p = Vec2::new(3.0, 4.0);
        assert_eq!(Rigid2Ops::point(&turn, p), ops::Rigid2Ops::point(&turn, p));
        assert_eq!(Rigid2Ops::inverse(&turn), ops::Rigid2Ops::inverse(&turn));
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 1.0)];
        assert_eq!(curve_through(&pts), ops::curve_through(&pts));
        assert_eq!(bead_flank_angle(2.0, 10.0, 4.0).ok(), ops::bead_flank_angle(2.0, 10.0, 4.0).ok());
        // a refusal keeps its kind
        let refused = bead_flank_angle(2.0, 10.0, 12.0);
        assert!(matches!(refused, Err(BeadRefusal::CrownWidth { .. })), "{refused:?}");
        assert!(matches!(ops::bead_flank_angle(2.0, 10.0, 12.0), Err(ops::BeadRefusal::CrownWidth { .. })));
        assert_eq!(MIN_BEAD_RADIUS, ops::MIN_BEAD_RADIUS);
        assert_eq!(hermite::ROUNDING, ops::hermite::ROUNDING);
        assert_eq!(hermite::remainder(0.5, 3.0), ops::hermite::remainder(0.5, 3.0));
    }
}
