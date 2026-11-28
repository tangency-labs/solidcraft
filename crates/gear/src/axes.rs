//! Axis-first gear design: start from the two shaft axes, and work out the gears.
//!
//! [`relate`] says how two axes lie (coincident, parallel, intersecting or skew, with the
//! distance, angle, apex or common perpendicular); [`candidates`] says which gear families fit
//! that and why the others don't; [`solve`] sizes a set from the axes plus a few driving values
//! (ratio or tooth counts, and a module where the axes leave one free) and places it so gear 1
//! turns about the first axis and gear 2 about the second.
//!
//! What the axes fix, per relation:
//!
//! - parallel, distance a: a gear pair. Module and teeth are picked (ISO 54 modules) so the
//!   reference centre distance is at or just under a, and the profile shifts take up the rest
//!   exactly. Cycloidal (clock) gears take the module that gives a exactly.
//! - coincident: a planetary set or a cycloidal drive about the axis (and any single gear).
//! - intersecting at Σ: straight or spiral bevels with their pitch cone apex at the meeting
//!   point; sized by the module or by the outer cone distance.
//! - skew at Σ, offset E: crossed helical gears (helix angles solved so the pitch cylinders
//!   touch across E), a worm and wheel (Σ = 90°: the worm's pitch diameter solved), or hypoid
//!   gears (throat radii solved); all meet on the common perpendicular.

use std::f64::consts::FRAC_PI_2;

use crate::involute::shift_sum_for_center;
use crate::plan::{Fact, Frame, Kind, Spec, Unit};

type V = [f64; 3];

#[inline]
pub(crate) fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub(crate) fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub(crate) fn mul(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
#[inline]
pub(crate) fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// The feet of the common perpendicular of the lines through `p1` along `d1` and `p2` along
/// `d2`, or None for parallel lines.
fn feet(p1: V, d1: V, p2: V, d2: V) -> Option<(V, V)> {
    let w = sub(p1, p2);
    let (a, b, c, d, e) = (dot(d1, d1), dot(d1, d2), dot(d2, d2), dot(d1, w), dot(d2, w));
    let den = a * c - b * b;
    if den.abs() < 1e-12 * a * c {
        return None;
    }
    let t1 = (b * e - c * d) / den;
    let t2 = (a * e - b * d) / den;
    Some((add(p1, mul(d1, t1)), add(p2, mul(d2, t2))))
}

#[inline]
pub(crate) fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub(crate) fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
pub(crate) fn unit(a: V) -> Option<V> {
    let l = len(a);
    (l.is_finite() && l > 1e-12).then(|| mul(a, 1.0 / l))
}

/// Directions closer than this (sine of the angle between them) are parallel.
const PARALLEL: f64 = 1e-6;
/// Lines nearer than this (mm) meet.
const TOUCH: f64 = 1e-6;
/// A worm needs its axes square within this (degrees).
const SQUARE: f64 = 0.01;

/// ISO 54 series I modules, with the usual small ones below 1 (DIN 780).
pub const MODULES: [f64; 25] =
    [0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 32.0, 40.0, 50.0];
/// ISO 54 series II modules (second choice).
pub const MODULES_2: [f64; 17] = [1.125, 1.375, 1.75, 2.25, 2.75, 3.5, 4.5, 5.5, 7.0, 9.0, 11.0, 14.0, 18.0, 22.0, 28.0, 36.0, 45.0];

/// A shaft axis: a point on it and its unit direction, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub origin: [f64; 3],
    pub dir: [f64; 3],
}

impl Axis {
    /// World Z through the origin.
    pub const Z: Axis = Axis { origin: [0.0; 3], dir: [0.0, 0.0, 1.0] };

    /// Whether `other` is the same line (either way round): its origin within 1e-6 of this
    /// line, its direction parallel (unit directions).
    pub fn coaxial(&self, other: &Axis) -> bool {
        let d = sub(other.origin, self.origin);
        let along = dot(d, self.dir);
        let off = sub(d, mul(self.dir, along));
        len(off) < 1e-6 && dot(self.dir, other.dir).abs() > 1.0 - 1e-9
    }

    /// The axis through `origin` along `dir` (any length); `None` when `dir` has no direction or
    /// a number isn't finite.
    pub fn new(origin: [f64; 3], dir: [f64; 3]) -> Option<Axis> {
        let dir = unit(dir)?;
        origin.iter().all(|v| v.is_finite()).then_some(Axis { origin, dir })
    }

    /// The point on the axis nearest `p`.
    pub fn foot(&self, p: [f64; 3]) -> [f64; 3] {
        add(self.origin, mul(self.dir, dot(sub(p, self.origin), self.dir)))
    }

    /// How far `p` is from the axis.
    pub fn distance(&self, p: [f64; 3]) -> f64 {
        len(sub(p, self.foot(p)))
    }

    /// Whether a line through `origin` along `dir` is this axis (either way round).
    pub fn holds(&self, origin: [f64; 3], dir: [f64; 3], tol: f64) -> bool {
        self.distance(origin) <= tol && len(cross(self.dir, dir)) <= tol.max(1e-9)
    }
}

/// How two axes lie. Angles in degrees, between the directions as given (0 to 180).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Relation {
    /// One line.
    Coincident,
    /// Parallel, `distance` apart: `foot1` is axis 1's origin, `foot2` the point of axis 2
    /// nearest it.
    Parallel { distance: f64, foot1: [f64; 3], foot2: [f64; 3] },
    /// Meeting at `apex`.
    Intersecting { angle: f64, apex: [f64; 3] },
    /// Neither: `offset` apart along the common perpendicular from `foot1` (on axis 1) to
    /// `foot2` (on axis 2).
    Skew { angle: f64, offset: f64, foot1: [f64; 3], foot2: [f64; 3] },
}

impl Relation {
    pub fn name(&self) -> &'static str {
        match self {
            Relation::Coincident => "coincident",
            Relation::Parallel { .. } => "parallel",
            Relation::Intersecting { .. } => "intersecting",
            Relation::Skew { .. } => "skew",
        }
    }

    /// A line for people: "parallel, 40 mm apart".
    pub fn describe(&self) -> String {
        match self {
            Relation::Coincident => "the same line (coaxial)".into(),
            Relation::Parallel { distance, .. } => format!("parallel, {} mm apart", num(*distance)),
            Relation::Intersecting { angle, .. } => format!("intersecting at {}°", num(fold(*angle))),
            Relation::Skew { angle, offset, .. } => format!("skew at {}°, offset {} mm", num(fold(*angle)), num(*offset)),
        }
    }
}

/// An angle between lines, folded to 0…90.
fn fold(deg: f64) -> f64 {
    if deg > 90.0 { 180.0 - deg } else { deg }
}

#[inline]
pub(crate) fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// How `a1` and `a2` lie.
pub fn relate(a1: &Axis, a2: &Axis) -> Relation {
    let c = cross(a1.dir, a2.dir);
    let s = len(c);
    let angle = s.atan2(dot(a1.dir, a2.dir)).to_degrees();
    if s < PARALLEL {
        let foot2 = a2.foot(a1.origin);
        let distance = len(sub(foot2, a1.origin));
        return if distance < TOUCH { Relation::Coincident } else { Relation::Parallel { distance, foot1: a1.origin, foot2 } };
    }
    // The nearest points of the two lines.
    let w = sub(a1.origin, a2.origin);
    let b = dot(a1.dir, a2.dir);
    let (d, e) = (dot(a1.dir, w), dot(a2.dir, w));
    let den = 1.0 - b * b;
    let foot1 = add(a1.origin, mul(a1.dir, (b * e - d) / den));
    let foot2 = add(a2.origin, mul(a2.dir, (e - b * d) / den));
    let offset = len(sub(foot2, foot1));
    if offset < TOUCH { Relation::Intersecting { angle, apex: mul(add(foot1, foot2), 0.5) } } else { Relation::Skew { angle, offset, foot1, foot2 } }
}

/// Whether a gear family suits a pair of axes.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub kind: Kind,
    pub fits: bool,
    /// The family to reach for first with these axes.
    pub natural: bool,
    /// Why not, or how it is placed.
    pub reason: String,
}

/// Every gear type against a relation (`None`: only one axis is given).
pub fn candidates(rel: Option<&Relation>) -> Vec<Candidate> {
    Kind::ALL.iter().map(|k| candidate(*k, rel)).collect()
}

fn lie(rel: &Relation) -> &'static str {
    match rel {
        Relation::Coincident => "the axes are the same line",
        Relation::Parallel { .. } => "the axes are parallel",
        Relation::Intersecting { .. } => "the axes intersect",
        Relation::Skew { .. } => "the axes are skew",
    }
}

fn candidate(kind: Kind, rel: Option<&Relation>) -> Candidate {
    let yes = |natural: bool, reason: &str| Candidate { kind, fits: true, natural, reason: reason.into() };
    let no = |reason: String| Candidate { kind, fits: false, natural: false, reason };
    let single = "a single gear: it goes on axis 1";
    let Some(rel) = rel else {
        return if kind.is_set() && !matches!(kind, Kind::Planetary | Kind::CycloidalDrive | Kind::Spline) {
            yes(false, "gear 1 goes on axis 1; pick a second axis to have the set sized to both")
        } else {
            yes(kind == Kind::Spur, if kind.is_set() { "built about axis 1" } else { single })
        };
    };
    match kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone | Kind::Internal | Kind::Rack => yes(false, single),
        Kind::Planetary | Kind::CycloidalDrive | Kind::Spline => match rel {
            Relation::Coincident => yes(kind == Kind::Planetary, "built about the shared axis"),
            _ => no(format!("{}: this set is coaxial, it needs input and output on one line", lie(rel))),
        },
        Kind::Pair | Kind::Cycloidal => match rel {
            Relation::Parallel { .. } => yes(kind == Kind::Pair, "sized to the distance between the axes"),
            Relation::Coincident => no("the axes are the same line: a pair needs them apart".into()),
            _ => no(format!("{}: a pair needs parallel axes", lie(rel))),
        },
        Kind::Sprocket | Kind::Pulley => match rel {
            Relation::Parallel { .. } => yes(false, "spaced to the axes; the chain or belt rounds the distance to whole links or teeth"),
            Relation::Coincident => no("the axes are the same line: a chain or belt drive needs them apart".into()),
            _ => no(format!("{}: a chain or belt drive needs parallel axes", lie(rel))),
        },
        Kind::Elliptical => match rel {
            Relation::Parallel { .. } => yes(false, "the module is scaled to the distance between the axes"),
            Relation::Coincident => no("the axes are the same line: an elliptical pair needs them apart".into()),
            _ => no(format!("{}: an elliptical pair needs parallel axes", lie(rel))),
        },
        Kind::FaceGear => match rel {
            Relation::Intersecting { angle, .. } if (fold(*angle) - 90.0).abs() <= SQUARE => {
                yes(false, "the pinion's axis crosses the face gear's above its teeth")
            }
            Relation::Intersecting { angle, .. } => no(format!("the axes meet at {}°: a face gear needs 90°", num(fold(*angle)))),
            _ => no(format!("{}: a face gear needs intersecting axes", lie(rel))),
        },
        Kind::Bevel | Kind::SpiralBevel => match rel {
            Relation::Intersecting { angle, .. } if (5.0..=175.0).contains(angle) => yes(kind == Kind::Bevel, "pitch cones meet where the axes do"),
            Relation::Intersecting { .. } => no("the axes meet at under 5°: too flat for bevels".into()),
            _ => no(format!("{}: bevels need intersecting axes", lie(rel))),
        },
        Kind::BevelPlanetary => match rel {
            Relation::Intersecting { angle, .. } if (5.0..=175.0).contains(angle) => {
                yes(false, "the first planet's shaft meets the sun's at the apex; the other planets are patterned round the sun")
            }
            Relation::Intersecting { .. } => no("the axes meet at under 5°: too flat for bevels".into()),
            _ => no(format!("{}: a bevel planetary set needs the planet's shaft to meet the sun's", lie(rel))),
        },
        Kind::Differential => no("a differential is designed from its own settings (axle, ratio, style), not from two shafts".into()),
        Kind::Hypoid | Kind::CrossedHelical | Kind::Worm => match rel {
            Relation::Skew { angle, .. } => {
                let square = (angle - 90.0).abs() <= SQUARE;
                match kind {
                    Kind::Worm if !square => no(format!("the axes cross at {}°: a worm and wheel need 90°", num(fold(*angle)))),
                    Kind::Worm => yes(true, "the worm's diameter is solved to the offset"),
                    _ if !(5.0..=175.0).contains(angle) => no("the axes cross at under 5°".into()),
                    Kind::CrossedHelical => yes(!square, "helix angles are solved to the offset"),
                    _ => yes(false, "throat radii are solved to the offset"),
                }
            }
            _ => no(format!("{}: this set needs skew axes (crossing without meeting)", lie(rel))),
        },
    }
}

/// The family to use when none is asked for.
pub fn natural(rel: Option<&Relation>) -> Kind {
    candidates(rel).into_iter().find(|c| c.natural).map(|c| c.kind).unwrap_or(Kind::Spur)
}

/// What drives the solve besides the axes and the spec's own numbers.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Drive {
    /// Gear 2's teeth ÷ gear 1's (worm: wheel teeth ÷ starts; cycloidal: wheel ÷ pinion).
    /// `None`: the spec's tooth counts as they are.
    pub ratio: Option<f64>,
    /// The module may be chosen (otherwise the spec's is kept).
    pub module_free: bool,
    /// Gear 1's and gear 2's tooth counts may be chosen.
    pub teeth_free: bool,
    pub teeth2_free: bool,
    /// The worm's starts may be chosen.
    pub starts_free: bool,
    /// Bevels: the outer cone distance (apex to the large ends' pitch circles), instead of the
    /// module.
    pub cone_distance: Option<f64>,
    /// Put gear 1 (gear 2) on the other side: bevels sit on the other ray from the apex; skew
    /// sets take the supplementary shaft angle; a parallel or single gear faces the other way.
    pub flip1: bool,
    pub flip2: bool,
    /// Gear 1 on axis 2 and gear 2 on axis 1.
    pub swap: bool,
    /// Parallel, coaxial and single gears: how far along axis 1 from its origin the set sits.
    pub along: f64,
}

/// Which of the spec's numbers the axes (and the solve) decided, for a form to show read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Derived {
    pub module: bool,
    pub teeth: bool,
    pub teeth2: bool,
    pub center_distance: bool,
    pub shaft_angle: bool,
    pub offset: bool,
    pub helix: bool,
    pub worm_diameter: bool,
    pub starts: bool,
    pub shift: bool,
}

/// A set sized and placed on its axes.
#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    /// The set, with its placement.
    pub spec: Spec,
    pub relation: Option<Relation>,
    /// Where gear 1 and gear 2 turn: each frame's origin is on its axis and its z along it
    /// (the same frame twice for a single gear or a coaxial set).
    pub frames: [Frame; 2],
    /// What the axes fixed and what was chosen.
    pub facts: Vec<Fact>,
    /// How the choices were made, for people.
    pub notes: Vec<String>,
    pub derived: Derived,
}

fn fact(label: &str, value: f64, unit: Unit) -> Fact {
    Fact { label: label.into(), value, unit }
}

/// Some direction square to `z`: world X where it can be, so world Z gives the world frame.
pub(crate) fn square_to(z: V) -> V {
    let seed = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    unit(sub(seed, mul(z, dot(seed, z)))).unwrap_or([1.0, 0.0, 0.0])
}

fn flipped(d: V, flip: bool) -> V {
    if flip { mul(d, -1.0) } else { d }
}

/// Sizes and places a gear set on its axes.
///
/// `kind`: the family (`None`: the natural one for the axes). `base` gives every number the
/// axes and `drive` don't decide (pressure angle, face width, bores…) and the tooth counts and
/// module where they are not free. Errors say what doesn't fit and what would.
pub fn solve(axis1: &Axis, axis2: Option<&Axis>, kind: Option<Kind>, base: &Spec, drive: &Drive) -> Result<Solved, String> {
    let (a1, a2) = match (axis2, drive.swap) {
        (Some(a2), true) => (*a2, Some(*axis1)),
        (a2, _) => (*axis1, a2.copied()),
    };
    if let Some(r) = drive.ratio
        && !(r.is_finite() && r > 0.0)
    {
        return Err("the ratio must be greater than 0".into());
    }
    let relation = a2.as_ref().map(|a2| relate(&a1, a2));
    let kind = kind.unwrap_or_else(|| natural(relation.as_ref()));
    let c = candidate(kind, relation.as_ref());
    if !c.fits {
        return Err(c.reason);
    }
    let mut spec = Spec { kind, origin: [0.0; 3], center_distance: 0.0, ..base.clone() };
    let mut out =
        Solved { spec: spec.clone(), relation, frames: [Frame::at([0.0; 3]); 2], facts: Vec::new(), notes: Vec::new(), derived: Derived::default() };
    // A single gear, a coaxial set, or a set with only its first axis: on axis 1.
    let on_axis_1 = |out: &mut Solved, spec: Spec| {
        let z = flipped(a1.dir, drive.flip1);
        let place = Frame { origin: add(a1.origin, mul(a1.dir, drive.along)), x: square_to(z), z };
        out.frames = [place; 2];
        out.spec = Spec { placement: Some(place), ..spec };
    };
    let sets_both = kind.is_set() && !matches!(kind, Kind::Planetary | Kind::CycloidalDrive | Kind::Spline);
    let (Some(a2), Some(rel), true) = (a2, relation, sets_both) else {
        spec.center_distance = base.center_distance;
        if let Some(r) = drive.ratio
            && sets_both
        {
            let (z1, z2) = pick_teeth(r, &spec, drive, kind);
            out.derived.teeth = z1 != spec.teeth;
            out.derived.teeth2 = z2 != spec.teeth2;
            (spec.teeth, spec.teeth2) = (z1, z2);
        }
        on_axis_1(&mut out, spec);
        return Ok(out);
    };
    match rel {
        // No two-axis set fits coincident axes (the candidates turn them down first).
        Relation::Coincident => return Err("the axes are the same line: a pair needs them apart".into()),
        Relation::Parallel { distance, foot1, foot2 } => {
            out.facts.push(fact("axis distance", distance, Unit::Mm));
            if kind == Kind::Cycloidal {
                cycloidal(distance, &mut spec, drive, &mut out);
            } else if matches!(kind, Kind::Sprocket | Kind::Pulley) {
                wrapped(distance, &mut spec, drive, &mut out);
            } else if kind == Kind::Elliptical {
                elliptical(distance, &mut spec, &mut out);
            } else {
                pair(distance, &mut spec, drive, &mut out)?;
            }
            let z = flipped(a1.dir, drive.flip1);
            let slide = mul(a1.dir, drive.along);
            let x = unit(sub(foot2, foot1)).unwrap_or_else(|| square_to(z));
            let place = Frame { origin: add(foot1, slide), x, z };
            out.frames = [place, Frame { origin: add(foot2, slide), x, z }];
            spec.placement = Some(place);
        }
        Relation::Intersecting { apex, .. } if kind == Kind::FaceGear => {
            // The face gear (gear 1) turns about axis 1 below the apex, its teeth facing it; the
            // pinion's axis (axis 2) crosses the face gear's a pitch radius above the pitch plane.
            if let Some(r) = drive.ratio {
                let (z1, z2) = pick_teeth(r, &spec, drive, kind);
                out.derived.teeth = drive.teeth_free && z1 != spec.teeth;
                out.derived.teeth2 = drive.teeth2_free && z2 != spec.teeth2;
                (spec.teeth, spec.teeth2) = (z1, z2);
            }
            out.derived.shaft_angle = true;
            let r1 = spec.module * f64::from(spec.teeth) / 2.0;
            out.facts.push(fact("pinion axis above the pitch plane", r1, Unit::Mm));
            let along = |a: &Axis, flip: bool| {
                let t = dot(sub(a.origin, apex), a.dir);
                flipped(if t < -TOUCH { mul(a.dir, -1.0) } else { a.dir }, flip)
            };
            // Gear 1 sits on the ray from the apex its axis's origin is on; its frame's z points
            // back up to the apex. The pinion lies along the ray of axis 2.
            let t1 = mul(along(&a1, drive.flip1), -1.0);
            let s2 = along(&a2, drive.flip2);
            let x = unit(sub(s2, mul(t1, dot(s2, t1)))).unwrap_or_else(|| square_to(t1));
            let place = Frame { origin: sub(apex, mul(t1, r1)), x, z: t1 };
            let local2 = Frame { origin: [0.0, 0.0, r1], x: [0.0, 0.0, -1.0], z: [1.0, 0.0, 0.0] };
            out.frames = [place, place.compose(&local2)];
            out.notes.push(format!(
                "{} pinion teeth, {} face gear teeth (ratio {})",
                spec.teeth,
                spec.teeth2,
                num(f64::from(spec.teeth2) / f64::from(spec.teeth))
            ));
            spec.placement = Some(place);
        }
        Relation::Intersecting { apex, .. } if kind == Kind::BevelPlanetary => {
            // Axis 1 is the sun's shaft, axis 2 the first planet's; each member on the ray from
            // the apex its axis's origin is on. The builder's frame: the apex at the origin, the
            // sun below it on -z, planet 1 in the xz plane.
            let side = |a: &Axis, flip: bool| {
                let along = dot(sub(a.origin, apex), a.dir);
                flipped(if along < -TOUCH { mul(a.dir, -1.0) } else { a.dir }, flip)
            };
            let (s1, s2) = (side(&a1, drive.flip1), side(&a2, drive.flip2));
            let sigma = len(cross(s1, s2)).atan2(dot(s1, s2));
            if !(5.0..=175.0).contains(&sigma.to_degrees()) {
                return Err(format!("the shafts meet at {}°: a bevel planetary set needs 5° to 175°", num(sigma.to_degrees())));
            }
            spec.shaft_angle = sigma.to_degrees();
            spec.first_azimuth = 0.0;
            out.derived.shaft_angle = true;
            out.facts.push(fact("cone angle (from the axes)", spec.shaft_angle, Unit::Deg));
            if drive.ratio.is_some() {
                let (z1, z2) = pick_teeth(drive.ratio.unwrap_or(1.0), &spec, drive, kind);
                out.derived.teeth = drive.teeth_free;
                out.derived.teeth2 = drive.teeth2_free;
                (spec.teeth, spec.teeth2) = (z1, z2);
            }
            let g = crate::plan::bevel_planet::geometry(&spec)?;
            if let Some(re) = drive.cone_distance {
                if !(re.is_finite() && re > 0.0) {
                    return Err("the cone distance must be greater than 0".into());
                }
                spec.module = 2.0 * re * g.delta_sun.sin() / f64::from(spec.teeth);
                out.derived.module = true;
                out.notes.push(format!("module {} from the cone distance {} mm", num(spec.module), num(re)));
            }
            let g = crate::plan::bevel_planet::geometry(&spec)?;
            if (g.sigma - sigma).abs() > 1e-4 {
                out.notes.push(format!(
                    "the planets sit at {}° to the sun's shaft (not the axes' {}°): the second sun's or ring's whole teeth set the cone angle",
                    num(g.sigma.to_degrees()),
                    num(sigma.to_degrees())
                ));
            }
            out.facts.push(fact("outer cone distance", g.re, Unit::Mm));
            let t1 = mul(s1, -1.0);
            let x = unit(sub(s2, mul(t1, dot(s2, t1)))).unwrap_or_else(|| square_to(t1));
            let place = Frame { origin: apex, x, z: t1 };
            let (h1, h2, bs) = (g.re * g.delta_sun.cos(), g.re * g.delta_planet.cos(), g.sigma);
            let sun = Frame { origin: [0.0, 0.0, -h1], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
            let planet = Frame { origin: [bs.sin() * h2, 0.0, -bs.cos() * h2], x: [bs.cos(), 0.0, bs.sin()], z: [-bs.sin(), 0.0, bs.cos()] };
            out.frames = [place.compose(&sun), place.compose(&planet)];
            spec.placement = Some(place);
        }
        Relation::Intersecting { apex, .. } => {
            // Each gear sits on one ray from the apex: the side its axis's origin is on, or
            // along the axis's direction when the origin is the apex itself.
            let side = |a: &Axis, flip: bool| {
                let along = dot(sub(a.origin, apex), a.dir);
                flipped(if along < -TOUCH { mul(a.dir, -1.0) } else { a.dir }, flip)
            };
            let (s1, s2) = (side(&a1, drive.flip1), side(&a2, drive.flip2));
            let sigma = len(cross(s1, s2)).atan2(dot(s1, s2));
            if !(5.0..=175.0).contains(&sigma.to_degrees()) {
                return Err(format!("the shafts meet at {}°: bevels need 5° to 175°", num(sigma.to_degrees())));
            }
            spec.shaft_angle = sigma.to_degrees();
            out.derived.shaft_angle = true;
            out.facts.push(fact("shaft angle (from the axes)", spec.shaft_angle, Unit::Deg));
            if drive.ratio.is_some() {
                let (z1, z2) = pick_teeth(drive.ratio.unwrap_or(1.0), &spec, drive, kind);
                out.derived.teeth = drive.teeth_free;
                out.derived.teeth2 = drive.teeth2_free;
                (spec.teeth, spec.teeth2) = (z1, z2);
            }
            let (z1, z2) = (f64::from(spec.teeth), f64::from(spec.teeth2));
            let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
            let d2 = sigma - d1;
            if let Some(re) = drive.cone_distance {
                if !(re.is_finite() && re > 0.0) {
                    return Err("the cone distance must be greater than 0".into());
                }
                spec.module = 2.0 * re * d1.sin() / z1;
                out.derived.module = true;
                out.notes.push(format!("module {} from the cone distance {} mm", num(spec.module), num(re)));
            }
            let re = spec.module * z1 / 2.0 / d1.sin();
            out.facts.push(fact("outer cone distance", re, Unit::Mm));
            let (h1, h2) = (re * d1.cos(), re * d2.cos());
            // The builder's own frame: gear 1 on +z with the apex h1 up it, gear 2's axis
            // towards the apex at (−sin Σ, 0, cos Σ).
            let (t1, t2) = (mul(s1, -1.0), mul(s2, -1.0));
            let x = unit(sub(mul(t1, sigma.cos()), t2)).unwrap_or_else(|| square_to(t1));
            let place = Frame { origin: sub(apex, mul(t1, h1)), x, z: t1 };
            let local2 = Frame {
                origin: [sigma.sin() * h2, 0.0, h1 - sigma.cos() * h2],
                x: [sigma.cos(), 0.0, sigma.sin()],
                z: [-sigma.sin(), 0.0, sigma.cos()],
            };
            out.frames = [place, place.compose(&local2)];
            spec.placement = Some(place);
        }
        Relation::Skew { offset, foot1, foot2, .. } => {
            // The builders' own frame: gear 1 on +z, gear 2's centre the offset along +x, its
            // axis turned Σ about x from z towards y.
            let z = flipped(a1.dir, drive.flip1);
            let x = mul(sub(foot2, foot1), 1.0 / offset);
            let y = cross(z, x);
            let d2 = if dot(a2.dir, y) < 0.0 { mul(a2.dir, -1.0) } else { a2.dir };
            let sigma = dot(d2, y).atan2(dot(d2, z));
            out.facts.push(fact("offset (from the axes)", offset, Unit::Mm));
            out.facts.push(fact("shaft angle (from the axes)", sigma.to_degrees(), Unit::Deg));
            out.derived.shaft_angle = true;
            out.derived.offset = true;
            let sigma = match kind {
                Kind::Worm => {
                    worm(offset, &mut spec, drive, &mut out)?;
                    FRAC_PI_2
                }
                _ => {
                    if !(5.0..=175.0).contains(&sigma.to_degrees()) {
                        return Err(format!("the axes cross at {}°: this set needs 5° to 175°", num(sigma.to_degrees())));
                    }
                    // Crossed helicals past square: the mirror set (left-hand helices) at the
                    // acute angle, gear 2's axis the same line pointing the other way.
                    spec.shaft_angle = sigma.to_degrees();
                    let solve_at = if kind == Kind::CrossedHelical && sigma > FRAC_PI_2 + 1e-9 { sigma - std::f64::consts::PI } else { sigma };
                    skew(offset, solve_at, &mut spec, drive, &mut out)?;
                    sigma
                }
            };
            let world = Frame { origin: foot1, x, z };
            if kind == Kind::Hypoid {
                // The hypoid builder places its own pinion and ring gear (the pinion off the
                // ring gear's axis by the offset, at the shaft angle): find where they come out
                // unplaced, and move the pair so the pinion's axis is axis 1 and the gear's is
                // axis 2. Both pairs of axes are skew by the same offset and angle, so one rigid
                // motion fits both.
                // Skew axes have a handedness: the set as built may be the mirror of the pair
                // asked for, and then the other hand of spiral (the mirrored set) fits.
                let wy = cross(world.z, world.x);
                let sense = |d2: V, y: V, z: V| dot(d2, y) * dot(d2, z);
                let want = sense(a2.dir, wy, world.z);
                let mut tries = 0;
                let (l1, l2, g1, lx, ly, lz) = loop {
                    let local = crate::plan(&Spec { placement: None, ..spec.clone() })?;
                    let (l1, l2) = (local.bodies[0].frame, local.bodies[1].frame);
                    let (g1, g2) = feet(l1.origin, l1.z, l2.origin, l2.z).ok_or("the hypoid's own axes don't cross skew")?;
                    let lx = mul(sub(g2, g1), 1.0 / dot(sub(g2, g1), sub(g2, g1)).sqrt());
                    let lz = l1.z;
                    let ly = cross(lz, lx);
                    let have = sense(l2.z, ly, lz);
                    if tries == 0 && want * have < -1e-9 {
                        spec.left_hand = !spec.left_hand;
                        out.notes.push(format!(
                            "{}-hand spiral pinion: the hand that fits how these axes cross",
                            if spec.left_hand { "left" } else { "right" }
                        ));
                        tries += 1;
                        continue;
                    }
                    break (l1, l2, g1, lx, ly, lz);
                };
                // The rotation taking the local triad (lx, ly, lz) to the world one (x, y, z).
                let turn = |v: V| -> V { add(add(mul(world.x, dot(lx, v)), mul(wy, dot(ly, v))), mul(world.z, dot(lz, v))) };
                let place = Frame { origin: sub(world.origin, turn(g1)), x: turn([1.0, 0.0, 0.0]), z: turn([0.0, 0.0, 1.0]) };
                out.frames = [place.compose(&l1), place.compose(&l2)];
                spec.placement = Some(place);
            } else {
                let local2 = Frame { origin: [offset, 0.0, 0.0], x: [-1.0, 0.0, 0.0], z: [0.0, sigma.sin(), sigma.cos()] };
                out.frames = [world, world.compose(&local2)];
                spec.placement = Some(world);
            }
        }
    }
    out.spec = spec;
    Ok(out)
}

/// Tooth counts for a ratio where the axes leave the size free (bevels, skew sets): the counts
/// that are not free stay; free ones give the ratio as nearly as whole numbers can, gear 1
/// staying near the spec's count.
fn pick_teeth(ratio: f64, spec: &Spec, drive: &Drive, kind: Kind) -> (u32, u32) {
    let clamp = |z: f64| z.round().clamp(3.0, 2000.0) as u32;
    // Cycloidal gears count the wheel first: its ratio is wheel ÷ pinion.
    let ratio = if kind == Kind::Cycloidal { 1.0 / ratio } else { ratio };
    match (drive.teeth_free, drive.teeth2_free) {
        (false, false) => (spec.teeth, spec.teeth2),
        (false, true) => (spec.teeth, clamp(f64::from(spec.teeth) * ratio)),
        (true, false) => (clamp(f64::from(spec.teeth2) / ratio), spec.teeth2),
        (true, true) => {
            let floor = if kind == Kind::Cycloidal { 6 } else { 12 };
            let want = f64::from(spec.teeth.max(floor));
            let mut best = (f64::MAX, spec.teeth, spec.teeth2);
            for z1 in floor..=120u32 {
                let z2 = clamp(f64::from(z1) * ratio);
                if z2 < floor {
                    continue;
                }
                let err = (f64::from(z2) / f64::from(z1) / ratio).ln().abs();
                let score = 400.0 * err + 0.02 * (f64::from(z1) - want).abs();
                if score < best.0 - 1e-12 {
                    best = (score, z1, z2);
                }
            }
            (best.1, best.2)
        }
    }
}

/// A gear pair across a fixed centre distance.
///
/// Free values are searched over the standard modules and the tooth sums whose reference
/// centre distance is at or just under `a` (within 0.8 module, so the shift sum stays small and
/// positive), scoring ratio error first, then small pinions (under 17 teeth), needless fine
/// teeth (pinion over 24, so the coarsest module that keeps about 17 teeth wins), the shift needed, and second-choice modules. The profile shifts then
/// reach `a` exactly, split evenly unless gear 1's shift is given.
fn pair(a: f64, spec: &mut Spec, drive: &Drive, out: &mut Solved) -> Result<(), String> {
    let cb = spec.helix.to_radians().cos();
    let ratio = drive.ratio.unwrap_or(f64::from(spec.teeth2) / f64::from(spec.teeth));
    let (free1, free2) = (drive.teeth_free, drive.teeth2_free);
    let modules: Vec<(f64, bool)> = if drive.module_free {
        MODULES.iter().map(|m| (*m, false)).chain(MODULES_2.iter().map(|m| (*m, true))).collect()
    } else {
        vec![(spec.module, false)]
    };
    let mut best: Option<(f64, f64, u32, u32)> = None;
    for (m, second) in modules {
        let ideal = 2.0 * a * cb / m;
        let sums: Vec<u32> = if free1 || free2 {
            let f = (ideal + 1e-9).floor();
            [f, f - 1.0].into_iter().filter(|s| *s >= 2.0 && *s < 4001.0).map(|s| s as u32).collect()
        } else {
            vec![spec.teeth + spec.teeth2]
        };
        for sum in sums {
            let firsts: Vec<u32> = match (free1, free2) {
                (true, true) => {
                    let c = f64::from(sum) / (1.0 + ratio);
                    vec![c.floor() as u32, c.ceil() as u32]
                }
                (false, _) => vec![spec.teeth],
                (true, false) => vec![sum.saturating_sub(spec.teeth2)],
            };
            for z1 in firsts {
                let z2 = sum.saturating_sub(z1);
                let small = z1.min(z2);
                if small < 6 {
                    continue;
                }
                let gap = (a - m * f64::from(z1 + z2) / (2.0 * cb)) / m;
                let searched = drive.module_free || free1 || free2;
                if searched && !(-1e-9..=0.8).contains(&gap) {
                    continue;
                }
                let small = f64::from(small);
                let score = 40.0 * (f64::from(z2) / f64::from(z1) / ratio).ln().abs()
                    + 0.5 * gap.abs()
                    + 0.25 * (17.0 - small).max(0.0)
                    + 0.05 * (small - 24.0).max(0.0)
                    + if second { 0.15 } else { 0.0 };
                if best.is_none_or(|b| score < b.0 - 1e-12) {
                    best = Some((score, m, z1, z2));
                }
            }
        }
    }
    let (m, z1, z2) = match best {
        Some((_, m, z1, z2)) => (m, z1, z2),
        // Fixed teeth that no standard module brings near the distance: the exact module.
        None if drive.module_free && !free1 && !free2 => {
            let m = 2.0 * a * cb / f64::from(spec.teeth + spec.teeth2);
            out.notes.push(format!(
                "no standard module brings {} + {} teeth to {} mm: using the exact module {}",
                spec.teeth,
                spec.teeth2,
                num(a),
                num(m)
            ));
            (m, spec.teeth, spec.teeth2)
        }
        None => {
            return Err(format!(
                "no teeth fit a centre distance of {} mm with module {}: the tooth sum would be {}; use a smaller module",
                num(a),
                num(spec.module),
                num(2.0 * a * cb / spec.module)
            ));
        }
    };
    out.derived.module = drive.module_free;
    out.derived.teeth = free1;
    out.derived.teeth2 = free2;
    out.derived.center_distance = true;
    (spec.module, spec.teeth, spec.teeth2) = (m, z1, z2);
    let g1 = spec.involute(z1, 0.0, spec.helix);
    let g2 = spec.involute(z2, 0.0, -spec.helix);
    let reference = g1.r() + g2.r();
    let Some(sum) = shift_sum_for_center(&g1, &g2, a) else {
        return Err(format!(
            "module {} with {} + {} teeth has a reference centre distance of {} mm, too far from the axes' {} mm to reach by profile shift; change the teeth or the module",
            num(m),
            z1,
            z2,
            num(reference),
            num(a)
        ));
    };
    if !(-0.6..=1.6).contains(&sum) {
        out.notes.push(format!(
            "the profile shift sum {} is large: teeth nearer {} would suit this distance better",
            num(sum),
            num(2.0 * a * cb / m)
        ));
    }
    // An exact fit needs no working distance (and no rounding noise in the shift sum).
    spec.center_distance = if (a - reference).abs() < 1e-9 { 0.0 } else { a };
    let sum = if spec.center_distance == 0.0 { 0.0 } else { sum };
    if spec.shift == 0.0 {
        // The smaller gear clear of undercut where its mate can spare it.
        spec.shift = crate::sketch_rules::split_shift(&g1, &g2, sum).0;
        if spec.shift.abs() < 1e-12 {
            spec.shift = 0.0;
        }
        out.derived.shift = true;
    }
    spec.shift2 = sum - spec.shift;
    out.facts.push(fact("module (chosen)", m, Unit::None));
    out.facts.push(fact("reference centre distance", reference, Unit::Mm));
    out.facts.push(fact("profile shift sum", sum, Unit::None));
    out.notes.push(format!(
        "module {}, {} + {} teeth (ratio {}); profile shifts take up {} mm",
        num(m),
        z1,
        z2,
        num(f64::from(z2) / f64::from(z1)),
        num(a - reference)
    ));
    Ok(())
}

/// Cycloidal (clock) gears across a fixed distance: they take no shift, so the module is the
/// one that gives the distance exactly.
fn cycloidal(a: f64, spec: &mut Spec, drive: &Drive, out: &mut Solved) {
    if let Some(r) = drive.ratio {
        let (z1, z2) = pick_teeth(r, spec, drive, Kind::Cycloidal);
        out.derived.teeth = drive.teeth_free;
        out.derived.teeth2 = drive.teeth2_free;
        (spec.teeth, spec.teeth2) = (z1, z2);
    }
    spec.module = 2.0 * a / f64::from(spec.teeth + spec.teeth2);
    out.derived.module = true;
    out.derived.center_distance = true;
    out.facts.push(fact("module (from the distance)", spec.module, Unit::None));
    out.notes.push(format!("cycloidal teeth take no shift: module {} gives the distance exactly", num(spec.module)));
}

/// Chain sprockets or timing pulleys on two parallel shafts: the distance is asked of the
/// chain or belt, which takes the nearest whole number of links or teeth.
fn wrapped(a: f64, spec: &mut Spec, drive: &Drive, out: &mut Solved) {
    if let Some(r) = drive.ratio {
        let (z1, z2) = pick_teeth(r, spec, drive, spec.kind);
        out.derived.teeth = drive.teeth_free && z1 != spec.teeth;
        out.derived.teeth2 = drive.teeth2_free && z2 != spec.teeth2;
        (spec.teeth, spec.teeth2) = (z1, z2);
    }
    if spec.teeth2 == 0 {
        spec.teeth2 = spec.teeth;
        out.derived.teeth2 = true;
    }
    spec.center_distance = a;
    out.derived.center_distance = true;
    let what = if spec.kind == Kind::Sprocket { "links" } else { "belt teeth" };
    out.notes.push(format!(
        "centre distance {} mm asked of the drive: it takes the nearest whole number of {what}, so gear 2 may sit a little off axis 2",
        num(a)
    ));
}

/// An elliptical pair on two parallel shafts: its centre distance grows with the module, so
/// the module is the one that gives the distance.
fn elliptical(a: f64, spec: &mut Spec, out: &mut Solved) {
    let unit_pair = crate::elliptic::EllipticPair {
        teeth: spec.teeth,
        module: 1.0,
        eccentricity: spec.ellipse_eccentricity,
        pressure_angle: spec.pressure_angle.to_radians(),
        addendum: spec.rack.addendum,
        dedendum: spec.rack.dedendum,
        backlash: 0.0,
    };
    let per_module = unit_pair.pitch().center;
    if per_module > 0.0 && per_module.is_finite() {
        spec.module = a / per_module;
        out.derived.module = true;
        out.facts.push(fact("module (from the distance)", spec.module, Unit::None));
        out.notes.push(format!("module {} gives the distance exactly", num(spec.module)));
    }
    out.derived.center_distance = true;
}

/// A worm and wheel across a fixed centre distance: the wheel (gear 1) on axis 1, the worm on
/// axis 2. The worm's pitch diameter is what is left of the distance after the wheel; a free
/// module is the standard one that brings the worm nearest 10 modules across.
fn worm(a: f64, spec: &mut Spec, drive: &Drive, out: &mut Solved) -> Result<(), String> {
    if let Some(r) = drive.ratio {
        if drive.starts_free {
            // The fewest starts that give the wheel about 20 teeth or more.
            spec.starts = (1..=6u32).find(|s| r * f64::from(*s) >= 19.5).unwrap_or(6);
            out.derived.starts = true;
        }
        if drive.teeth_free {
            spec.teeth = (r * f64::from(spec.starts)).round().clamp(3.0, 2000.0) as u32;
            out.derived.teeth = true;
        }
    }
    let zw = f64::from(spec.teeth);
    let diameter = |m: f64| 2.0 * (a - spec.shift * m) - m * zw;
    if drive.module_free {
        let q = |m: f64| diameter(m) / m;
        let all = MODULES.iter().chain(MODULES_2.iter()).copied();
        let best = all.filter(|m| (6.0..=20.0).contains(&q(*m))).min_by(|x, y| (q(*x) - 10.0).abs().total_cmp(&(q(*y) - 10.0).abs()));
        spec.module = match best {
            Some(m) => m,
            None => {
                let m = 2.0 * a / (zw + 10.0 + 2.0 * spec.shift);
                out.notes.push(format!("no standard module gives a usable worm at this offset: using the exact module {}", num(m)));
                m
            }
        };
        out.derived.module = true;
    }
    let m = spec.module;
    let d1 = diameter(m);
    if d1 <= 2.0 * spec.rack.dedendum * m + 0.5 * m {
        return Err(format!(
            "module {} with a {}-tooth wheel leaves no room for the worm across {} mm (the wheel alone is {} mm across); use a smaller module or fewer teeth",
            num(m),
            spec.teeth,
            num(a),
            num(m * zw)
        ));
    }
    spec.worm_diameter = d1;
    out.derived.worm_diameter = true;
    out.derived.center_distance = true;
    out.facts.push(fact("module (chosen)", m, Unit::None));
    out.facts.push(fact("worm pitch diameter (solved)", d1, Unit::Mm));
    out.notes.push(format!(
        "axial module {}, {} wheel teeth, {} start(s); worm pitch Ø {} (q = {})",
        num(m),
        spec.teeth,
        spec.starts,
        num(d1),
        num(d1 / m)
    ));
    Ok(())
}

/// Crossed helical and hypoid gears across a fixed offset `e` at shaft angle `sigma`.
///
/// Their pitch cylinders (throats) touch when e = m/2 · (z1 / cos β1 + z2 / cos(Σ − β1)), which
/// has a least value over β1; a free module is the largest standard one whose least value is
/// within the offset, so the helix angles stay as moderate as the offset allows. β1 is then
/// solved on the side of that minimum nearer Σ/2 (the hypoid builder solves its own angles from
/// the offset, with the pinion's the larger).
fn skew(e: f64, sigma: f64, spec: &mut Spec, drive: &Drive, out: &mut Solved) -> Result<(), String> {
    let kind = spec.kind;
    if drive.ratio.is_some() {
        let (z1, z2) = pick_teeth(drive.ratio.unwrap_or(1.0), spec, drive, kind);
        out.derived.teeth = drive.teeth_free;
        out.derived.teeth2 = drive.teeth2_free;
        (spec.teeth, spec.teeth2) = (z1, z2);
    }
    if kind == Kind::Hypoid {
        // A spiral hypoid takes the offset and shaft angle as they are: its size (the ring
        // gear's diameter, so its module) stays as set, and its spiral angles come from its own
        // ISO 23509 design.
        spec.offset = e;
        let ring = spec.module * f64::from(spec.teeth2);
        out.facts.push(fact("ring gear outer pitch diameter", ring, Unit::Mm));
        out.notes.push(format!("{} + {} teeth on a Ø{} ring gear; offset {} mm from the axes", spec.teeth, spec.teeth2, num(ring), num(e)));
        let _ = sigma;
        return Ok(());
    }
    let (z1, z2) = (f64::from(spec.teeth), f64::from(spec.teeth2));
    let limit = 80f64.to_radians();
    // The hypoid builder keeps both angles positive; crossed helicals may take either hand.
    let (lo, hi) = if kind == Kind::Hypoid {
        (1e-3_f64.max(sigma - limit), (sigma - 1e-3).min(limit))
    } else {
        ((sigma - limit).max(-limit) + 1e-6, (sigma + limit).min(limit) - 1e-6)
    };
    if lo >= hi {
        return Err(format!("the axes cross at {}°: too far from square for these gears", num(sigma.to_degrees())));
    }
    // Per unit module, without the shifts.
    let span = |b: f64| 0.5 * (z1 / b.cos() + z2 / (sigma - b).cos());
    let (mut a, mut b) = (lo, hi);
    for _ in 0..200 {
        let (m1, m2) = (a + (b - a) / 3.0, b - (b - a) / 3.0);
        if span(m1) < span(m2) { b = m2 } else { a = m1 }
    }
    let b_min = 0.5 * (a + b);
    let shifts = if kind == Kind::Hypoid { 0.0 } else { spec.shift + spec.shift2 };
    let least = span(b_min) + shifts;
    if least <= 0.0 {
        return Err("the profile shifts leave no centre distance".into());
    }
    // The hypoid builder only takes the solution with the pinion's larger angle, which a module
    // much under the largest can't reach.
    let floor = if kind == Kind::Hypoid { e / (span(hi) + shifts) } else { 0.0 };
    if drive.module_free {
        let all = MODULES.iter().chain(MODULES_2.iter()).copied();
        spec.module = match all.filter(|m| m * least <= e * (1.0 + 1e-12) && *m >= floor).max_by(f64::total_cmp) {
            Some(m) => m,
            None => {
                let m = e / least * (1.0 - 1e-9);
                out.notes.push(format!("no standard module spans this offset with these teeth: using the exact module {}", num(m)));
                m
            }
        };
        out.derived.module = true;
    }
    let m = spec.module;
    if m * least > e * (1.0 + 1e-9) {
        return Err(format!(
            "module {} with {} + {} teeth needs the axes at least {} mm apart, and they are {} mm; use module {} or less, or fewer teeth",
            num(m),
            spec.teeth,
            spec.teeth2,
            num(m * least),
            num(e),
            num(e / least)
        ));
    }
    out.facts.push(fact("module (chosen)", m, Unit::None));
    out.facts.push(fact("smallest offset for these teeth", m * least, Unit::Mm));
    if kind == Kind::Hypoid {
        // The builder solves its spiral angles from the offset.
        if m < floor * (1.0 - 1e-12) {
            return Err(format!("module {} is too small for an offset of {} mm with these teeth: the spiral angle would pass 80°", num(m), num(e)));
        }
        spec.offset = e;
        out.notes.push(format!("normal module {}, {} + {} teeth; the throat radii span the offset", num(m), spec.teeth, spec.teeth2));
        return Ok(());
    }
    let target = e / m - shifts;
    let towards_hi = sigma / 2.0 >= b_min;
    let solve_on = |up: bool| -> Option<f64> {
        let (mut near, mut far) = if up { (b_min, hi) } else { (b_min, lo) };
        if span(far) < target {
            return None;
        }
        for _ in 0..200 {
            let mid = 0.5 * (near + far);
            if span(mid) < target { near = mid } else { far = mid }
        }
        Some(0.5 * (near + far))
    };
    let Some(b1) = solve_on(towards_hi).or_else(|| solve_on(!towards_hi)) else {
        return Err(format!(
            "module {} is too small for an offset of {} mm with these teeth: a helix angle would pass 80°; use a larger module or more teeth",
            num(m),
            num(e)
        ));
    };
    spec.helix = b1.to_degrees();
    out.derived.helix = true;
    out.facts.push(fact("gear 1 helix angle (solved)", spec.helix, Unit::Deg));
    out.facts.push(fact("gear 2 helix angle (solved)", (sigma - b1).to_degrees(), Unit::Deg));
    out.notes.push(format!(
        "normal module {}, {} + {} teeth; helix angles {}° and {}° bring the pitch cylinders together across the offset",
        num(m),
        spec.teeth,
        spec.teeth2,
        num(spec.helix),
        num((sigma - b1).to_degrees())
    ));
    Ok(())
}
