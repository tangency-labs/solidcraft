//! Sheet development ([`Develop`]): a sheet metal part's surfaces laid out flat and folded back,
//! the motions and bend measures panels are placed with, beads, contour flanges and the cubic
//! pieces a curve is carried into the flat by. Sheets, developments, motions and beads are the
//! record's (`solidcore_record::develop`); a fold, a bend in space and a smooth piece of a curve
//! are the kernel's own forms ([`Develop::Fold`], [`Develop::Bent`], [`Develop::Smooth`]), held
//! by the router; the refusals are mirrored here.

use solidcore_record::brep::Curve;
use solidcore_record::develop::{Bead, BeadForm, BeadOutline, Bend, Development, Lap, Mismatch, Rigid2, Rigid3, Sheet, Surface};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};

use crate::Backend;

/// Why a sheet is not laid out ([`Develop::develop`]) or folded ([`Develop::fold`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// An anchor, a cut or the call names a panel the sheet does not have.
    NoPanel(usize),
    /// A panel the development cannot lay out, and why.
    Panel {
        /// The panel.
        panel: usize,
        /// Why.
        why: PanelRefusal,
    },
    /// A joint the development cannot lay out, and why.
    Joint {
        /// The joint.
        joint: usize,
        /// Why.
        why: JointRefusal,
    },
    /// More panels or joints than one development takes ([`Develop::MAX_PANELS`],
    /// [`Develop::MAX_JOINTS`]): a work limit, never a truncation.
    TooLarge,
    /// The sheet's thickness or K-factor, and which.
    Rule(RuleRefusal),
    /// The fold takes one place in space for each of the flat's pieces: how many it has.
    Roots(usize),
}

/// Why a panel is not laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelRefusal {
    /// A roll whose radius is 0 or less, or whose neutral surface would be (the material
    /// inward, the sheet thicker than the roll's radius allows).
    RollRadius,
    /// A cone whose rulings make 0 or less, or a quarter turn or more, with its axis.
    ConeHalfAngle,
    /// A cone whose neutral surface would pass its apex inside the panel.
    ConeApex,
    /// A cut drawn on a panel the flat has no place for.
    CutUnplaced,
    /// A line across both of a cone's parameters that more cubic pieces than a flat takes would
    /// be needed to follow within its distance (a panel far past any sheet's size): refused
    /// rather than followed less closely.
    ConeSpiral,
}

/// Why a joint is not laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointRefusal {
    /// An anchor's direction has no length where the layout reads it.
    NoDirection,
    /// An anchor's origin is not finite, or its direction is not a unit vector.
    NotUnit,
    /// A bend or a seam with both anchors on one panel (a rip may part a panel from itself).
    OnePanel,
    /// A bend that turns 0 or less, or more than a half turn, or round a radius below 0 or not
    /// finite.
    BendRange,
    /// A half turn anchored at its virtual sharp, which it has none of.
    HalfTurnSharp,
    /// A bend zone whose ends are not finite.
    ZoneEnds,
    /// A bend on the edge of a roll: not laid out yet.
    BendOnRoll,
    /// A rip without two or more points of its edge on each panel and a turn at each.
    RipPoints,
    /// A rip whose points, turns or curves are not finite.
    RipNotFinite,
    /// A rip that cuts less than 0, or a number that is not finite, from a side.
    RipLap,
    /// A rip's edge that leaves its panel's surface where it is cut back.
    RipOffSurface,
    /// A rip's edge that is not on its panel's outline.
    RipOffOutline,
    /// A rip's edge that runs against its panel's outline.
    RipAgainstOutline,
    /// A torn end of the zone that no spline of the most spans keeps within its tolerance of
    /// the curve it follows.
    TornEnd,
    /// A zone's end that follows its panels' outlines (`Relief::Outline`) where an outline runs
    /// along its tangent line at the end, or meets it again before it reaches the sharp.
    OutlineEnd,
    /// A zone's end that follows its panels' outlines where no cubic pieces are proved to follow
    /// an outline's image in the zone within the tolerance.
    OutlineFit,
    /// An anchor with no frame on its panel's surface.
    NoFrame,
}

/// Which of the sheet's rule values is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleRefusal {
    /// The thickness: not more than 0, or not finite.
    Thickness,
    /// The K-factor: not from 0 to 1.
    KFactor,
}

/// Why a bead cannot be made, with what would fit. An application words it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BeadRefusal {
    /// A size is not finite, or the sheet's thickness is not positive.
    NotFinite,
    /// Its centreline has no length.
    NoLength,
    /// Its depth, width or end length is not above 0.
    NotPositive,
    /// Its flanks' angle (radians) is not between 0° and 90°.
    Angle {
        /// The angle given.
        angle: f64,
    },
    /// A radius is less than [`Develop::MIN_BEAD_RADIUS`].
    Radius,
    /// Its gentlest bend (the smallest radius over the flatter of the flanks and the end ramps)
    /// rises `rise`, less than the kernel's least bend rise.
    GentleBend {
        /// How far the gentlest bend rises.
        rise: f64,
    },
    /// Too shallow for its radii at its flanks' angle (radians): at least `min_depth` deep.
    ShallowFlanks {
        /// The flanks' angle.
        angle: f64,
        /// The least depth that fits.
        min_depth: f64,
    },
    /// Too shallow for its radii on its end ramps, at their angle (radians).
    ShallowRamps {
        /// The ramps' angle.
        angle: f64,
    },
    /// Too narrow to leave a crown at its flanks' angle (radians): at least `min_width` wide.
    NoCrown {
        /// The flanks' angle.
        angle: f64,
        /// The least width that fits.
        min_width: f64,
    },
    /// Its centreline, `length` long, is too short for its two end ramps: at least `min_length`.
    TooShort {
        /// The centreline's length.
        length: f64,
        /// The least length that fits.
        min_length: f64,
    },
    /// A crease could not be traced within its tolerance: off by `off` mm.
    Crease {
        /// How far the trace was off.
        off: f64,
    },
    /// Its panel has no place on the folded sheet or in the flat.
    Unplaced,
    /// Its crown width is not at least 0 and less than its width.
    CrownWidth {
        /// The crown width given.
        crown_width: f64,
        /// The bead's width.
        width: f64,
    },
    /// An outline beside it could not be placed against its footprint.
    Undecided,
}

/// Where a bead's footprint lies against an outline on its panel ([`Develop::bead_against`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeadAgainst {
    /// Farther than the distance asked from the outline everywhere, outside it, and the outline
    /// outside the footprint.
    Clear,
    /// Within the distance of the outline somewhere: across it or along it.
    Meets,
    /// Farther than the distance from the outline everywhere, and held inside it.
    Inside,
    /// The outline lies wholly inside the footprint, farther than the distance from its edges.
    Around,
}

/// Why a contour profile has no surfaces at the depth asked: an application words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourRefusal {
    /// A piece of the profile has no length, so no direction to shift it across.
    ZeroLength,
    /// Two lines meet turning back on each other, so their shifted lines never meet.
    TurnsBack,
    /// At a sharp corner beside an arc, the shifted line and circle (or two circles) miss.
    CornersMiss,
    /// A line is shorter than the shifted corners at its ends leave it.
    LineTooShort,
    /// An arc's shifted radius is no larger than the sheet is thick where the material lies
    /// inside it (no larger than zero otherwise).
    RadiusTooSmall,
    /// An arc is shorter than the shifted corners at its ends leave it.
    ArcTooShort,
}

/// A developed sheet folded ([`Develop::fold`]): where each panel's frame and each bend lie in
/// space, the rings that do not close, and the kernel's own fold for the queries.
#[derive(Clone, Debug, PartialEq)]
pub struct Folded<F, B> {
    /// Where each panel's frame lies in space.
    pub places: Vec<Option<Rigid3>>,
    /// Where each joint's bend lies in space (none for a seam or a rip).
    pub bends: Vec<Option<B>>,
    /// Bends and seams that close a ring, where the fold reached both panels another way: how far
    /// the joint itself would put its `b` panel from where the ring put it.
    pub loops: Vec<Mismatch>,
    /// The kernel's own fold.
    pub form: F,
}

/// The sheet development of one kernel ([`crate::OpKind::Develop`]).
pub trait Develop: Backend {
    /// The kernel's own fold.
    type Fold: Clone + std::fmt::Debug + PartialEq + Send + Sync;
    /// The kernel's own bend in space.
    type Bent: Copy + std::fmt::Debug + PartialEq + Send + Sync;
    /// The kernel's own smooth piece of a curve.
    type Smooth: Clone + std::fmt::Debug + PartialEq + Send + Sync;
    /// The most panels one development takes.
    const MAX_PANELS: usize;
    /// The most joints one development takes.
    const MAX_JOINTS: usize;
    /// The smallest radius a bead takes (mm).
    const MIN_BEAD_RADIUS: f64;
    /// The share of the sizes a carried curve's bounds are rounded outward by.
    const HERMITE_ROUNDING: f64;

    /// The sheet laid out flat; `anchors` says where some panels' neutral charts lie in the flat.
    fn develop(&self, sheet: &Sheet, anchors: &[(usize, Rigid2)]) -> Result<Development, Refusal>;
    /// A developed sheet folded; `roots` says where the first panel of each of the development's
    /// pieces lies in space.
    fn fold(&self, sheet: &Sheet, dev: &Development, roots: &[Rigid3]) -> Result<Folded<Self::Fold, Self::Bent>, Refusal>;
    /// Where the edges of `a` cross those of `b`: each crossing's edge and parameter on either, and
    /// its point.
    fn crossings(&self, a: &[Seg2], b: &[Seg2]) -> Vec<(usize, f64, usize, f64, Vec2)>;
    /// The surfaces of a contour flange's profile `segs`, `depth` into the material: the material
    /// right of the travel, or left when `flip`; corners turning less than `smooth` radians meet on
    /// the common normal.
    fn offset_contour(&self, segs: &[Seg2], depth: f64, thickness: f64, flip: bool, smooth: f64) -> Result<Vec<Seg2>, ContourRefusal>;
    /// A curve through `points` in order: a line where they lie on one, else a clamped cubic spline
    /// through each.
    fn curve_through(&self, points: &[Vec2]) -> Vec<Seg2>;
    /// Where a point of panel `i`'s flat material goes, `depth` into the material.
    fn fold_panel_point(&self, fold: &Self::Fold, i: usize, p: Vec2, depth: f64) -> Option<Vec3>;
    /// The point of panel `i`'s surface at the parameters `q`, `depth` into the material.
    fn fold_surface_point(&self, fold: &Self::Fold, i: usize, q: Vec2, depth: f64) -> Option<Vec3>;
    /// Where panel `i`'s neutral chart lies in the flat.
    fn fold_placement(&self, fold: &Self::Fold, i: usize) -> Option<Rigid2>;
    /// Where the point of panel `i`'s surface at the parameters `q` lies in the flat.
    fn fold_flat_point(&self, fold: &Self::Fold, i: usize, q: Vec2) -> Option<Vec2>;
    /// The parameters of panel `i`'s surface at a point of its flat material.
    fn fold_param(&self, fold: &Self::Fold, i: usize, p: Vec2) -> Option<Vec2>;
    /// The surface panel `i` lies on.
    fn fold_surface(&self, fold: &Self::Fold, i: usize) -> Option<Surface>;
    /// A roll's axis: a point of it and its direction; `None` for a plane.
    fn fold_axis(&self, fold: &Self::Fold, i: usize) -> Option<(Vec3, Vec3)>;
    /// What bead `b` does to the folded solid of a sheet `t` thick: the footprint its panel's faces
    /// leave open, in the flat, and its faces in space.
    fn fold_bead_form(&self, fold: &Self::Fold, b: &Bead, t: f64) -> Result<BeadForm, BeadRefusal>;
    /// Bead `b` in the flat of a sheet `t` thick; with `mirror`, as a flat pattern is read.
    fn fold_bead_outline(&self, fold: &Self::Fold, b: &Bead, t: f64, mirror: bool) -> Result<BeadOutline, BeadRefusal>;
    /// Where a note `height` high along bead `b` stands in the flat: its anchor and its angle
    /// (radians).
    fn fold_bead_note(&self, fold: &Self::Fold, b: &Bead, t: f64, mirror: bool, height: f64) -> Result<(Vec2, f64), BeadRefusal>;
    /// Where a straight line from `a` to `b` on the plane through `origin` with unit `normal` lies
    /// in planar panel `i`'s chart: its ends, and whether the plane is the panel's far face.
    fn fold_line_in_chart(&self, fold: &Self::Fold, i: usize, t: f64, origin: Vec3, normal: Vec3, a: Vec3, b: Vec3) -> Option<(Vec2, Vec2, bool)>;
    /// Where a point of the bend's flat zone goes, `depth` into the material.
    fn bent_point(&self, bent: &Self::Bent, p: Vec2, depth: f64) -> Vec3;
    /// The point `along` the joint, turned `phi` round the bend from the `a` panel's tangent line,
    /// `depth` into the material.
    fn bent_at(&self, bent: &Self::Bent, along: f64, phi: f64, depth: f64) -> Vec3;
    /// A point of the flat zone as its length along the joint and its angle round the bend.
    fn bent_wrapped(&self, bent: &Self::Bent, p: Vec2) -> (f64, f64);
    /// Whether the flat line from `a` to `b` runs straight across the bend.
    fn bent_straight_across(&self, bent: &Self::Bent, a: Vec2, b: Vec2) -> bool;
    /// Where a point of space lies in the bend: its flat point, depth and the way deeper; `None` on
    /// the axis.
    fn bent_unwrapped(&self, bent: &Self::Bent, p: Vec3) -> Option<(Vec2, f64, Vec3)>;
    /// A point of the bend's axis and its direction.
    fn bent_axis(&self, bent: &Self::Bent) -> (Vec3, Vec3);
    /// How far the bend turns, in radians.
    fn bent_angle(&self, bent: &Self::Bent) -> f64;
    /// The joint's way in the flat, from the `a` panel's side.
    fn bent_flat_way(&self, bent: &Self::Bent) -> Vec2;
    /// The turn by `angle` radians about the origin.
    fn rigid2_turn(&self, angle: f64) -> Rigid2;
    /// The motion putting `from` on `to` and turning `from_dir` to `to_dir`; `None` when either
    /// direction has no length.
    fn rigid2_onto(&self, from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Rigid2>;
    /// The turn `m` makes, in radians.
    fn rigid2_angle(&self, m: &Rigid2) -> f64;
    /// The vector `v` turned by `m`.
    fn rigid2_vector(&self, m: &Rigid2, v: Vec2) -> Vec2;
    /// The point `p` moved by `m`.
    fn rigid2_point(&self, m: &Rigid2, p: Vec2) -> Vec2;
    /// `m`, then `next`.
    fn rigid2_then(&self, m: &Rigid2, next: &Rigid2) -> Rigid2;
    /// The motion undoing `m`.
    fn rigid2_inverse(&self, m: &Rigid2) -> Rigid2;
    /// The segment `s` moved by `m`.
    fn rigid2_seg(&self, m: &Rigid2, s: &Seg2) -> Seg2;
    /// The loop `l` moved by `m`.
    fn rigid2_loop(&self, m: &Rigid2, l: &Loop2) -> Loop2;
    /// The region `r` moved by `m`.
    fn rigid2_region(&self, m: &Rigid2, r: &Region2) -> Region2;
    /// The frame at `origin` whose axes are `x`, `y` and `x × y`; `None` unless `x` and `y` are
    /// unit vectors square to each other.
    fn rigid3_frame(&self, origin: Vec3, x: Vec3, y: Vec3) -> Option<Rigid3>;
    /// The vector `v` turned by `m`.
    fn rigid3_vector(&self, m: &Rigid3, v: Vec3) -> Vec3;
    /// The point `p` moved by `m`.
    fn rigid3_point(&self, m: &Rigid3, p: Vec3) -> Vec3;
    /// `m`, then `next`.
    fn rigid3_then(&self, m: &Rigid3, next: &Rigid3) -> Rigid3;
    /// The motion undoing `m`.
    fn rigid3_inverse(&self, m: &Rigid3) -> Rigid3;
    /// The angle of the turn from `m`'s to `other`'s, in radians.
    fn rigid3_angle_to(&self, m: &Rigid3, other: &Rigid3) -> f64;
    /// The flat width of the bend zone on a sheet `thickness` thick: its neutral arc's length,
    /// `θ (R + K T)`.
    fn bend_allowance(&self, bend: &Bend, thickness: f64, k_factor: f64) -> f64;
    /// How far each anchor lies from its panel's tangent line, toward the other panel, on a sheet
    /// `thickness` thick.
    fn bend_setback(&self, bend: &Bend, thickness: f64) -> f64;
    /// What each panel `(a, b)` is cut back from the edge, where the sheet turns `turn` across it
    /// and is `thickness` thick.
    fn lap_cuts(&self, lap: &Lap, turn: f64, thickness: f64) -> (f64, f64);
    /// The pieces of `curve` over `span` on which it is smooth, in order, each with bounds on its
    /// first four derivatives.
    fn smooth_pieces(&self, curve: &Curve, span: [f64; 2]) -> Option<Vec<Self::Smooth>>;
    /// The piece of the parameter.
    fn smooth_span(&self, piece: &Self::Smooth) -> [f64; 2];
    /// At least the length of each of the curve's first four derivatives on the piece.
    fn smooth_rates(&self, piece: &Self::Smooth) -> [f64; 4];
    /// At most how far a point of the piece, as read at, lies from the record's curve.
    fn smooth_slack(&self, piece: &Self::Smooth) -> f64;
    /// The curve's point and first derivative at `t`, from the piece's own form.
    fn smooth_at(&self, piece: &Self::Smooth, t: f64) -> Option<(Vec3, Vec3)>;
    /// A bound on the fourth derivative of a map of a curve, from bounds on the curve's and the
    /// map's.
    fn hermite_fourth(&self, c: [f64; 4], g: [f64; 4]) -> f64;
    /// How far a cubic over a step `h` lies at most from the curve it follows, its fourth
    /// derivative at most `fourth`.
    fn hermite_remainder(&self, h: f64, fourth: f64) -> f64;
    /// The parameters over `span` between which cubic pieces stand proved within `tol`, `off`
    /// bounding each piece's distance.
    fn hermite_nodes(&self, off: &dyn Fn(f64, f64) -> Option<f64>, span: [f64; 2], tol: f64) -> Option<Vec<f64>>;
    /// The cubic from `p0` at rate `d0` to `p3` at rate `d3` over a step `h`.
    fn hermite_cubic(&self, p0: Vec2, d0: Vec2, p3: Vec2, d3: Vec2, h: f64) -> Seg2;
    /// Bead `b`'s footprint in its panel's chart, counter-clockwise, on a sheet `t` thick.
    fn bead_footprint(&self, b: &Bead, t: f64) -> Result<[Vec2; 4], BeadRefusal>;
    /// Where bead `b`'s footprint lies against the outline `l`, within `near` counting as meeting
    /// it.
    fn bead_against(&self, b: &Bead, t: f64, l: &Loop2, near: f64) -> Result<BeadAgainst, BeadRefusal>;
    /// Whether the segment from `a` to `b` lies wholly in `region`; `None` where it cannot be
    /// decided.
    fn segment_in_region(&self, region: &Region2, a: Vec2, b: Vec2) -> Option<bool>;
    /// The flanks' angle (radians) of a bead `depth` deep and `width` wide leaving a crown
    /// `crown_width` wide.
    fn bead_flank_angle(&self, depth: f64, width: f64, crown_width: f64) -> Result<f64, BeadRefusal>;
}
