//! solidcore's sheet development ([`Develop`]): each the solidcore-ops call this crate's callers
//! made before the interface existed. A fold, a bend in space and a smooth piece are ops' own
//! values ([`ops::Fold`], [`ops::Bent`], [`ops::hermite::Smooth`]); the refusals are converted
//! variant for variant.

use solidcore_ops::develop as ops;
use solidcore_ops::develop::{BendOps as _, LapOps as _, Rigid2Ops as _, Rigid3Ops as _};
use solidcore_record::brep::Curve;
use solidcore_record::develop::{Anchor, Bead, BeadForm, BeadOutline, Bend, Development, Lap, Rigid2, Rigid3, Sheet, Surface};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::develop::{BeadAgainst, BeadRefusal, ContourRefusal, Develop, Folded, JointRefusal, PanelRefusal, Refusal, RuleRefusal};

use super::Solidcore;

/// The kernel's refusal of a layout or fold, as the interface's.
fn refusal(r: ops::Refusal) -> Refusal {
    match r {
        ops::Refusal::NoPanel(n) => Refusal::NoPanel(n),
        ops::Refusal::Panel { panel, why } => Refusal::Panel { panel, why: panel_refusal(why) },
        ops::Refusal::Joint { joint, why } => Refusal::Joint { joint, why: joint_refusal(why) },
        ops::Refusal::TooLarge => Refusal::TooLarge,
        ops::Refusal::Rule(r) => Refusal::Rule(rule_refusal(r)),
        ops::Refusal::Roots(n) => Refusal::Roots(n),
    }
}

fn panel_refusal(r: ops::PanelRefusal) -> PanelRefusal {
    match r {
        ops::PanelRefusal::RollRadius => PanelRefusal::RollRadius,
        ops::PanelRefusal::ConeHalfAngle => PanelRefusal::ConeHalfAngle,
        ops::PanelRefusal::ConeApex => PanelRefusal::ConeApex,
        ops::PanelRefusal::CutUnplaced => PanelRefusal::CutUnplaced,
        ops::PanelRefusal::ConeSpiral => PanelRefusal::ConeSpiral,
    }
}

fn joint_refusal(r: ops::JointRefusal) -> JointRefusal {
    match r {
        ops::JointRefusal::NoDirection => JointRefusal::NoDirection,
        ops::JointRefusal::NotUnit => JointRefusal::NotUnit,
        ops::JointRefusal::OnePanel => JointRefusal::OnePanel,
        ops::JointRefusal::BendRange => JointRefusal::BendRange,
        ops::JointRefusal::HalfTurnSharp => JointRefusal::HalfTurnSharp,
        ops::JointRefusal::ZoneEnds => JointRefusal::ZoneEnds,
        ops::JointRefusal::BendOnRoll => JointRefusal::BendOnRoll,
        ops::JointRefusal::RipPoints => JointRefusal::RipPoints,
        ops::JointRefusal::RipNotFinite => JointRefusal::RipNotFinite,
        ops::JointRefusal::RipLap => JointRefusal::RipLap,
        ops::JointRefusal::RipOffSurface => JointRefusal::RipOffSurface,
        ops::JointRefusal::RipOffOutline => JointRefusal::RipOffOutline,
        ops::JointRefusal::RipAgainstOutline => JointRefusal::RipAgainstOutline,
        ops::JointRefusal::TornEnd => JointRefusal::TornEnd,
        ops::JointRefusal::OutlineEnd => JointRefusal::OutlineEnd,
        ops::JointRefusal::OutlineFit => JointRefusal::OutlineFit,
        ops::JointRefusal::NoFrame => JointRefusal::NoFrame,
    }
}

fn rule_refusal(r: ops::RuleRefusal) -> RuleRefusal {
    match r {
        ops::RuleRefusal::Thickness => RuleRefusal::Thickness,
        ops::RuleRefusal::KFactor => RuleRefusal::KFactor,
    }
}

/// The kernel's refusal of a bead, as the interface's.
fn bead_refusal(r: ops::BeadRefusal) -> BeadRefusal {
    match r {
        ops::BeadRefusal::NotFinite => BeadRefusal::NotFinite,
        ops::BeadRefusal::NoLength => BeadRefusal::NoLength,
        ops::BeadRefusal::NotPositive => BeadRefusal::NotPositive,
        ops::BeadRefusal::Angle { angle } => BeadRefusal::Angle { angle },
        ops::BeadRefusal::Radius => BeadRefusal::Radius,
        ops::BeadRefusal::GentleBend { rise } => BeadRefusal::GentleBend { rise },
        ops::BeadRefusal::ShallowFlanks { angle, min_depth } => BeadRefusal::ShallowFlanks { angle, min_depth },
        ops::BeadRefusal::ShallowRamps { angle } => BeadRefusal::ShallowRamps { angle },
        ops::BeadRefusal::NoCrown { angle, min_width } => BeadRefusal::NoCrown { angle, min_width },
        ops::BeadRefusal::TooShort { length, min_length } => BeadRefusal::TooShort { length, min_length },
        ops::BeadRefusal::Crease { off } => BeadRefusal::Crease { off },
        ops::BeadRefusal::Unplaced => BeadRefusal::Unplaced,
        ops::BeadRefusal::CrownWidth { crown_width, width } => BeadRefusal::CrownWidth { crown_width, width },
        ops::BeadRefusal::Undecided => BeadRefusal::Undecided,
    }
}

fn bead_against(r: ops::BeadAgainst) -> BeadAgainst {
    match r {
        ops::BeadAgainst::Clear => BeadAgainst::Clear,
        ops::BeadAgainst::Meets => BeadAgainst::Meets,
        ops::BeadAgainst::Inside => BeadAgainst::Inside,
        ops::BeadAgainst::Around => BeadAgainst::Around,
    }
}

/// The kernel's refusal of a contour, as the interface's.
fn contour_refusal(r: ops::ContourRefusal) -> ContourRefusal {
    match r {
        ops::ContourRefusal::ZeroLength => ContourRefusal::ZeroLength,
        ops::ContourRefusal::TurnsBack => ContourRefusal::TurnsBack,
        ops::ContourRefusal::CornersMiss => ContourRefusal::CornersMiss,
        ops::ContourRefusal::LineTooShort => ContourRefusal::LineTooShort,
        ops::ContourRefusal::RadiusTooSmall => ContourRefusal::RadiusTooSmall,
        ops::ContourRefusal::ArcTooShort => ContourRefusal::ArcTooShort,
    }
}

impl Develop for Solidcore {
    type Fold = ops::Fold;
    type Bent = ops::Bent;
    type Smooth = ops::hermite::Smooth;
    const MAX_PANELS: usize = ops::MAX_PANELS;
    const MAX_JOINTS: usize = ops::MAX_JOINTS;
    const MIN_BEAD_RADIUS: f64 = ops::MIN_BEAD_RADIUS;
    const HERMITE_ROUNDING: f64 = ops::hermite::ROUNDING;

    fn develop(&self, sheet: &Sheet, anchors: &[(usize, Rigid2)]) -> Result<Development, Refusal> {
        ops::develop(sheet, anchors).map_err(refusal)
    }

    fn fold(&self, sheet: &Sheet, dev: &Development, roots: &[Rigid3]) -> Result<Folded<ops::Fold, ops::Bent>, Refusal> {
        let form = ops::fold(sheet, dev, roots).map_err(refusal)?;
        Ok(Folded { places: form.places.clone(), bends: form.bends.clone(), loops: form.loops.clone(), form })
    }

    fn crossings(&self, a: &[Seg2], b: &[Seg2]) -> Vec<(usize, f64, usize, f64, Vec2)> {
        ops::crossings(a, b)
    }

    fn offset_contour(&self, segs: &[Seg2], depth: f64, thickness: f64, flip: bool, smooth: f64) -> Result<Vec<Seg2>, ContourRefusal> {
        ops::offset_contour(segs, depth, thickness, flip, smooth).map_err(contour_refusal)
    }

    fn tear_slit(&self, from: Vec2, to: Vec2, depth: f64, ends: &Anchor, at: f64, outline: &Loop2, place: &Rigid2) -> Option<[Vec2; 2]> {
        ops::tear_slit(from, to, depth, ends, at, outline, place)
    }

    fn curve_through(&self, points: &[Vec2]) -> Vec<Seg2> {
        ops::curve_through(points)
    }

    fn fold_panel_point(&self, fold: &ops::Fold, i: usize, p: Vec2, depth: f64) -> Option<Vec3> {
        fold.panel_point(i, p, depth)
    }

    fn fold_surface_point(&self, fold: &ops::Fold, i: usize, q: Vec2, depth: f64) -> Option<Vec3> {
        fold.surface_point(i, q, depth)
    }

    fn fold_placement(&self, fold: &ops::Fold, i: usize) -> Option<Rigid2> {
        fold.placement(i)
    }

    fn fold_flat_point(&self, fold: &ops::Fold, i: usize, q: Vec2) -> Option<Vec2> {
        fold.flat_point(i, q)
    }

    fn fold_param(&self, fold: &ops::Fold, i: usize, p: Vec2) -> Option<Vec2> {
        fold.param(i, p)
    }

    fn fold_surface(&self, fold: &ops::Fold, i: usize) -> Option<Surface> {
        fold.surface(i)
    }

    fn fold_axis(&self, fold: &ops::Fold, i: usize) -> Option<(Vec3, Vec3)> {
        fold.axis(i)
    }

    fn fold_bead_form(&self, fold: &ops::Fold, b: &Bead, t: f64) -> Result<BeadForm, BeadRefusal> {
        fold.bead_form(b, t).map_err(bead_refusal)
    }

    fn fold_bead_outline(&self, fold: &ops::Fold, b: &Bead, t: f64, mirror: bool) -> Result<BeadOutline, BeadRefusal> {
        fold.bead_outline(b, t, mirror).map_err(bead_refusal)
    }

    fn fold_bead_note(&self, fold: &ops::Fold, b: &Bead, t: f64, mirror: bool, height: f64) -> Result<(Vec2, f64), BeadRefusal> {
        fold.bead_note(b, t, mirror, height).map_err(bead_refusal)
    }

    fn fold_line_in_chart(&self, fold: &ops::Fold, i: usize, t: f64, origin: Vec3, normal: Vec3, a: Vec3, b: Vec3) -> Option<(Vec2, Vec2, bool)> {
        fold.line_in_chart(i, t, origin, normal, a, b)
    }

    fn bent_point(&self, bent: &ops::Bent, p: Vec2, depth: f64) -> Vec3 {
        bent.point(p, depth)
    }

    fn bent_at(&self, bent: &ops::Bent, along: f64, phi: f64, depth: f64) -> Vec3 {
        bent.at(along, phi, depth)
    }

    fn bent_wrapped(&self, bent: &ops::Bent, p: Vec2) -> (f64, f64) {
        bent.wrapped(p)
    }

    fn bent_straight_across(&self, bent: &ops::Bent, a: Vec2, b: Vec2) -> bool {
        bent.straight_across(a, b)
    }

    fn bent_unwrapped(&self, bent: &ops::Bent, p: Vec3) -> Option<(Vec2, f64, Vec3)> {
        bent.unwrapped(p)
    }

    fn bent_axis(&self, bent: &ops::Bent) -> (Vec3, Vec3) {
        bent.axis()
    }

    fn bent_angle(&self, bent: &ops::Bent) -> f64 {
        bent.angle()
    }

    fn bent_flat_way(&self, bent: &ops::Bent) -> Vec2 {
        bent.flat_way()
    }

    fn rigid2_turn(&self, angle: f64) -> Rigid2 {
        <Rigid2 as ops::Rigid2Ops>::turn(angle)
    }

    fn rigid2_onto(&self, from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Rigid2> {
        <Rigid2 as ops::Rigid2Ops>::onto(from, from_dir, to, to_dir)
    }

    fn rigid2_angle(&self, m: &Rigid2) -> f64 {
        m.angle()
    }

    fn rigid2_vector(&self, m: &Rigid2, v: Vec2) -> Vec2 {
        m.vector(v)
    }

    fn rigid2_point(&self, m: &Rigid2, p: Vec2) -> Vec2 {
        m.point(p)
    }

    fn rigid2_then(&self, m: &Rigid2, next: &Rigid2) -> Rigid2 {
        m.then(next)
    }

    fn rigid2_inverse(&self, m: &Rigid2) -> Rigid2 {
        m.inverse()
    }

    fn rigid2_seg(&self, m: &Rigid2, s: &Seg2) -> Seg2 {
        m.seg(s)
    }

    fn rigid2_loop(&self, m: &Rigid2, l: &Loop2) -> Loop2 {
        m.loop2(l)
    }

    fn rigid2_region(&self, m: &Rigid2, r: &Region2) -> Region2 {
        m.region(r)
    }

    fn rigid3_frame(&self, origin: Vec3, x: Vec3, y: Vec3) -> Option<Rigid3> {
        <Rigid3 as ops::Rigid3Ops>::frame(origin, x, y)
    }

    fn rigid3_vector(&self, m: &Rigid3, v: Vec3) -> Vec3 {
        m.vector(v)
    }

    fn rigid3_point(&self, m: &Rigid3, p: Vec3) -> Vec3 {
        m.point(p)
    }

    fn rigid3_then(&self, m: &Rigid3, next: &Rigid3) -> Rigid3 {
        m.then(next)
    }

    fn rigid3_inverse(&self, m: &Rigid3) -> Rigid3 {
        m.inverse()
    }

    fn rigid3_angle_to(&self, m: &Rigid3, other: &Rigid3) -> f64 {
        m.angle_to(other)
    }

    fn bend_allowance(&self, bend: &Bend, thickness: f64, k_factor: f64) -> f64 {
        bend.allowance(thickness, k_factor)
    }

    fn bend_setback(&self, bend: &Bend, thickness: f64) -> f64 {
        bend.setback(thickness)
    }

    fn lap_cuts(&self, lap: &Lap, turn: f64, thickness: f64) -> (f64, f64) {
        lap.cuts(turn, thickness)
    }

    fn smooth_pieces(&self, curve: &Curve, span: [f64; 2]) -> Option<Vec<ops::hermite::Smooth>> {
        ops::hermite::smooth_pieces(curve, span)
    }

    fn smooth_span(&self, piece: &ops::hermite::Smooth) -> [f64; 2] {
        piece.span()
    }

    fn smooth_rates(&self, piece: &ops::hermite::Smooth) -> [f64; 4] {
        piece.rates()
    }

    fn smooth_slack(&self, piece: &ops::hermite::Smooth) -> f64 {
        piece.slack()
    }

    fn smooth_at(&self, piece: &ops::hermite::Smooth, t: f64) -> Option<(Vec3, Vec3)> {
        piece.at(t)
    }

    fn hermite_fourth(&self, c: [f64; 4], g: [f64; 4]) -> f64 {
        ops::hermite::fourth(c, g)
    }

    fn hermite_remainder(&self, h: f64, fourth: f64) -> f64 {
        ops::hermite::remainder(h, fourth)
    }

    fn hermite_nodes(&self, off: &dyn Fn(f64, f64) -> Option<f64>, span: [f64; 2], tol: f64) -> Option<Vec<f64>> {
        ops::hermite::nodes(off, span, tol)
    }

    fn hermite_cubic(&self, p0: Vec2, d0: Vec2, p3: Vec2, d3: Vec2, h: f64) -> Seg2 {
        ops::hermite::cubic(p0, d0, p3, d3, h)
    }

    fn bead_footprint(&self, b: &Bead, t: f64) -> Result<[Vec2; 4], BeadRefusal> {
        ops::bead_footprint(b, t).map_err(bead_refusal)
    }

    fn bead_against(&self, b: &Bead, t: f64, l: &Loop2, near: f64) -> Result<BeadAgainst, BeadRefusal> {
        ops::bead_against(b, t, l, near).map(bead_against).map_err(bead_refusal)
    }

    fn segment_in_region(&self, region: &Region2, a: Vec2, b: Vec2) -> Option<bool> {
        ops::segment_in_region(region, a, b)
    }

    fn bead_flank_angle(&self, depth: f64, width: f64, crown_width: f64) -> Result<f64, BeadRefusal> {
        ops::bead_flank_angle(depth, width, crown_width).map_err(bead_refusal)
    }
}
