//! Gear sets from a sketch: circles are pitch circles, lines are shafts.
//!
//! The rules (confirmed by the user, `GEAR_GENERATOR_NOTES.md`, "Sketch rules"): one sketch is
//! gear 1's plane. A circle is a gear's pitch circle (Ø = pitch Ø), its shaft through the centre
//! normal to the sketch unless a line says otherwise.
//!
//! | Mode | Picks | Sizes |
//! |---|---|---|
//! | single gear | one circle | pitch Ø |
//! | pair (spur/helical), cycloidal | two circles touching outside | both Ø; centre distance |
//! | sprocket / pulley | one or two circles | Ø; centre distance |
//! | internal | a circle touching inside a bigger one | ring Ø (and the pinion's) |
//! | planetary | sun, planet, ring: sun and ring concentric, the planet touching both | three Ø |
//! | rack, worm and wheel | circle + tangent line (worm: optional second parallel line) | line = pitch line |
//! | bevel, face gear | circle + line through its centre (shaft 2 seen from above) | Ø; shaft angle (a setting, default 90°) |
//! | bevel planetary | side view: origin dot (apex) + sun shaft line + first planet's shaft line from it | cone angle; sizes are settings |
//! | hypoid | circle (ring gear) + line crossing it off-centre (pinion shaft) | ring Ø; offset |
//! | crossed helical | circle + line missing it | Ø; offset |
//!
//! Everything is in one sketch (user, 2026-11-14): angles out of the sketch's plane (a bevel's,
//! hypoid's or crossed helicals' shaft angle) are settings, and repeated members (planets) are a
//! count, patterned from the one drawn.
//!
//! The window keeps a mode picker: [`slots`] says what a mode needs, [`measure`] checks the picks
//! against the mode's rule and measures them (an error says what is wrong and what it would
//! make instead), and [`recognize`] is only a hint ("these picks look like a hypoid"). Then
//! [`spec_for`] sizes a [`Spec`] of that mode from the measured diameters (standard modules where
//! one fits, an exact module otherwise) and [`axes_for`] gives the shafts in the world, for
//! [`crate::axes::solve`] to place the set on.
//!
//! The solver agent's `simple.rs` may take over `spec_for`; the rules and the measuring stay
//! here, shared by the window's drawing and the engine's `gear.create {sketch}`.
//!
//! [`read`] is the one reader of a whole drawing: with a tooth style (spur, helical,
//! herringbone) the drawing decides the set, one of the modes above or a gear train of any
//! number of circles (touching outside: a mesh; inside: a gear in a ring; concentric: gears on
//! one shaft), sized by [`train_spec`] and planned by [`crate::plan::train`].

use std::f64::consts::PI;

use crate::axes::{Axis, MODULES, MODULES_2, num};
use crate::geom::V2;
use crate::involute::{Involute, shift_sum_for_center};
use crate::plan::{Frame, Kind, SidePreset, Spec};

/// A circle of the sketch (its plane's coordinates, mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub c: V2,
    pub r: f64,
}

/// A straight line of the sketch, through `a` and `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: V2,
    pub b: V2,
}

impl Line {
    /// Unit direction from `a` to `b` (x when they coincide).
    pub fn dir(&self) -> V2 {
        let d = self.b - self.a;
        if d.len() > 1e-12 { d * (1.0 / d.len()) } else { V2::new(1.0, 0.0) }
    }
    /// The point of the (endless) line nearest `p`.
    pub fn foot(&self, p: V2) -> V2 {
        let u = self.dir();
        self.a + u * (p - self.a).dot(u)
    }
    /// How far `p` is from the (endless) line.
    pub fn distance(&self, p: V2) -> f64 {
        p.dist(self.foot(p))
    }
    pub fn len(&self) -> f64 {
        self.a.dist(self.b)
    }
}

/// What is drawn, in the sketch's plane.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Drawn {
    /// Sketch points (the origin dot).
    pub points: Vec<V2>,
    pub circles: Vec<Circle>,
    pub lines: Vec<Line>,
}

/// What a slot takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Point,
    Circle,
    Line,
}

/// One thing a mode needs from the sketch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub label: &'static str,
    pub role: Role,
    pub optional: bool,
    /// What it must be, for people.
    pub hint: &'static str,
}

const fn slot(label: &'static str, role: Role, optional: bool, hint: &'static str) -> Slot {
    Slot { label, role, optional, hint }
}

/// The picture a mode starts from, sized by `s` (the Gear Generator window's Simple mode shows it
/// before there is a sketch, and `gear.sketch` draws it as a native sketch): the origin dot at gear 1's centre
/// (a bevel's apex), and the circles and lines its rule wants.
pub fn template(k: Kind, s: &Spec) -> Drawn {
    let r = |z: u32| s.module * f64::from(z) / 2.0;
    let c = |x: f64, y: f64, r: f64| Circle { c: V2::new(x, y), r };
    let l = |ax: f64, ay: f64, bx: f64, by: f64| Line { a: V2::new(ax, ay), b: V2::new(bx, by) };
    let mut d = Drawn { points: vec![V2::ZERO], ..Drawn::default() };
    match k {
        Kind::Pair | Kind::Cycloidal => {
            let (r1, r2) = (r(s.teeth), r(s.teeth2));
            d.circles = vec![c(0.0, 0.0, r1), c(r1 + r2, 0.0, r2)];
        }
        Kind::Internal => {
            let ring = r(s.teeth);
            d.circles = vec![c(0.0, 0.0, ring), c(ring * 2.0 / 3.0, 0.0, ring / 3.0)];
        }
        Kind::Planetary => {
            let (rs, rp) = (r(s.teeth), r(s.teeth2));
            d.circles = vec![c(0.0, 0.0, rs), c(rs + rp, 0.0, rp), c(0.0, 0.0, rs + 2.0 * rp)];
        }
        Kind::Rack => {
            let r1 = r(s.teeth);
            d.circles = vec![c(0.0, 0.0, r1)];
            d.lines = vec![l(-1.6 * r1, -r1, 1.6 * r1, -r1)];
        }
        Kind::Worm => {
            let r1 = r(s.teeth);
            d.circles = vec![c(0.0, 0.0, r1)];
            d.lines = vec![l(-1.3 * r1, r1, 1.3 * r1, r1)];
        }
        Kind::Bevel | Kind::SpiralBevel => {
            let len = 1.6 * r(s.teeth.max(10));
            let a = s.shaft_angle.to_radians();
            d.lines = vec![l(0.0, 0.0, 0.0, -len), l(0.0, 0.0, len * a.sin(), -len * a.cos())];
        }
        // The same side view: the apex dot, the sun's shaft down from it, the first planet's
        // at the cone angle.
        Kind::BevelPlanetary => {
            let len = 1.6 * r(s.teeth.max(10));
            let a = s.shaft_angle.to_radians();
            d.lines = vec![l(0.0, 0.0, 0.0, -len), l(0.0, 0.0, len * a.sin(), -len * a.cos())];
        }
        Kind::FaceGear => {
            let rf = r(s.teeth2);
            d.circles = vec![c(0.0, 0.0, rf)];
            d.lines = vec![l(0.0, 0.0, 1.4 * rf, 0.0)];
        }
        Kind::Hypoid => {
            let ring = r(s.teeth2);
            let e = if s.offset > 0.0 { s.offset } else { 0.125 * 2.0 * ring };
            d.circles = vec![c(0.0, 0.0, ring)];
            d.lines = vec![l(-1.2 * ring, -e, 1.2 * ring, -e)];
        }
        Kind::CrossedHelical => {
            let b1 = (s.shaft_angle / 2.0).to_radians();
            let (r1, r2) = (r(s.teeth) / b1.cos(), r(s.teeth2) / (s.shaft_angle.to_radians() - b1).cos());
            d.circles = vec![c(0.0, 0.0, r1)];
            d.lines = vec![l(-1.5 * (r1 + r2), r1 + r2, 1.5 * (r1 + r2), r1 + r2)];
        }
        Kind::Sprocket => {
            let p = crate::sprocket::chain_by_id(s.chain).map(|c| c.pitch).unwrap_or(s.chain_pitch);
            d.circles = vec![c(0.0, 0.0, p / (std::f64::consts::PI / f64::from(s.teeth.max(6))).sin() / 2.0)];
        }
        Kind::Pulley => {
            let p = crate::pulley::belt_by_id(s.belt).map(|b| b.pitch).unwrap_or(2.0);
            d.circles = vec![c(0.0, 0.0, p * f64::from(s.teeth.max(6)) / std::f64::consts::PI / 2.0)];
        }
        _ => d.circles = vec![c(0.0, 0.0, r(s.teeth))],
    }
    d
}

/// The modes the sketch input covers (the rest are set up in the window's Advanced mode).
pub fn sketched(k: Kind) -> bool {
    !matches!(k, Kind::CycloidalDrive | Kind::Spline | Kind::Elliptical | Kind::Differential)
}

/// What mode `k` needs from the sketch, in order. The first is always the origin dot: the
/// set's anchor, about which [`axes_for`]'s turns go (left empty: gear 1's centre, or where a
/// bevel's shafts meet).
pub fn slots(k: Kind) -> Vec<Slot> {
    use Role::*;
    let origin = if matches!(k, Kind::Bevel | Kind::SpiralBevel) {
        slot("Origin (apex)", Point, true, "the dot where the two shafts meet: the pitch cones' apex")
    } else if k == Kind::BevelPlanetary {
        slot("Origin (apex)", Point, true, "the dot where the sun's and the planets' shafts meet: the pitch cones' apex")
    } else {
        slot("Origin", Point, true, "the set's anchor: turns go about it (empty: gear 1's centre)")
    };
    let mut all = vec![origin];
    all.extend(own_slots(k));
    all
}

fn own_slots(k: Kind) -> Vec<Slot> {
    use Role::*;
    match k {
        Kind::Pair | Kind::Cycloidal => vec![
            slot("Gear 1 pitch circle", Circle, false, "its Ø is gear 1's pitch Ø"),
            slot("Gear 2 pitch circle", Circle, false, "touching gear 1's circle outside"),
        ],
        Kind::Sprocket | Kind::Pulley => vec![
            slot("Pitch circle 1", Circle, false, "its Ø is the pitch Ø"),
            slot("Pitch circle 2", Circle, true, "the second one; the centre distance is between the centres"),
        ],
        Kind::Internal => vec![
            slot("Ring pitch circle", Circle, false, "the bigger circle: the ring gear's pitch Ø"),
            slot("Pinion pitch circle", Circle, true, "inside the ring's, touching it"),
        ],
        Kind::Planetary => vec![
            slot("Sun pitch circle", Circle, false, "at the ring's centre"),
            slot("Planet pitch circle", Circle, false, "touching the sun outside and the ring inside"),
            slot("Ring pitch circle", Circle, false, "round the sun, the same centre"),
        ],
        Kind::Rack => vec![
            slot("Pinion pitch circle", Circle, false, "the gear the rack meshes with"),
            slot("Rack pitch line", Line, false, "tangent to the circle; its length is the rack's"),
        ],
        Kind::Worm => vec![
            slot("Wheel pitch circle", Circle, false, "its Ø is the wheel's pitch Ø"),
            slot("Worm pitch line", Line, false, "tangent to the circle: the worm's shaft runs along it"),
            slot("Worm's far side", Line, true, "parallel to the pitch line, beyond it: the distance between them is the worm's pitch Ø"),
        ],
        // A side view, like a pinion and a spur gear drawn edge on: the angle between the lines
        // is the shaft angle; the sizes are settings (gear 1's pitch Ø and the ratio).
        Kind::Bevel | Kind::SpiralBevel => vec![
            slot("Gear 1 shaft", Line, false, "a line through the origin dot (the apex), seen from the side"),
            slot("Gear 2 shaft", Line, false, "a line from the dot: the angle between the two lines is the shaft angle"),
        ],
        // The same side view: the sun's shaft and one planet's; the other planets are patterned
        // round the sun's shaft.
        Kind::BevelPlanetary => vec![
            slot("Sun shaft", Line, false, "a line through the origin dot (the apex), seen from the side"),
            slot("First planet's shaft", Line, false, "a line from the dot: the angle between the two lines is the cone angle"),
        ],
        Kind::FaceGear => vec![
            slot("Face gear pitch circle", Circle, false, "its Ø is the face gear's pitch Ø"),
            slot("Pinion shaft", Line, false, "through the circle's centre"),
        ],
        Kind::Hypoid => vec![
            slot("Ring gear pitch circle", Circle, false, "its Ø is the ring gear's outer pitch Ø"),
            slot("Pinion shaft", Line, false, "crossing the circle off its centre: the offset is its distance from the centre"),
        ],
        Kind::CrossedHelical => vec![
            slot("Gear 1 pitch circle", Circle, false, "its Ø is gear 1's pitch Ø"),
            slot("Shaft 2", Line, false, "missing the circle: the offset is its distance from the centre"),
        ],
        // Single gears, and the modes only Advanced sets up: their one circle.
        _ => vec![slot("Pitch circle", Circle, false, "its Ø is the gear's pitch Ø")],
    }
}

/// Each slot's pick: an index into `Drawn::circles` or `Drawn::lines` (by its role); `None`
/// for an empty slot.
pub type Picks = Vec<Option<usize>>;

/// Fills the slots of mode `k` from what is drawn, in drawing order (the ring of a planetary
/// or internal set is the largest circle, a planetary sun the one at its centre).
pub fn auto_picks(k: Kind, d: &Drawn) -> Picks {
    let sl = slots(k);
    let mut picks: Picks = vec![None; sl.len()];
    let biggest = (0..d.circles.len()).max_by(|a, b| d.circles[*a].r.total_cmp(&d.circles[*b].r));
    let dot = (!d.points.is_empty()).then_some(0);
    match k {
        Kind::Planetary if d.circles.len() >= 3 => {
            let ring = biggest.unwrap_or(0);
            let rc = d.circles[ring];
            let sun = (0..d.circles.len()).filter(|i| *i != ring).min_by(|a, b| d.circles[*a].c.dist(rc.c).total_cmp(&d.circles[*b].c.dist(rc.c)));
            let planet = (0..d.circles.len()).find(|i| *i != ring && Some(*i) != sun);
            return vec![dot, sun, planet, Some(ring)];
        }
        Kind::Internal if d.circles.len() >= 2 => {
            let ring = biggest.unwrap_or(0);
            return vec![dot, Some(ring), (0..d.circles.len()).find(|i| *i != ring)];
        }
        _ => {}
    }
    let (mut pi, mut ci, mut li) = (0usize, 0usize, 0usize);
    for (p, s) in picks.iter_mut().zip(&sl) {
        match s.role {
            Role::Point if pi < d.points.len() => {
                *p = Some(pi);
                pi += 1;
            }
            Role::Circle if ci < d.circles.len() => {
                *p = Some(ci);
                ci += 1;
            }
            Role::Line if li < d.lines.len() => {
                *p = Some(li);
                li += 1;
            }
            _ => {}
        }
    }
    picks
}

/// How the picks lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Single,
    /// Two circles touching outside (or, for chains and belts, any two circles).
    Pair,
    Internal,
    Planetary,
    /// Circle + tangent line (rack, worm).
    Tangent,
    /// Circle + line through its centre (bevel, face gear).
    ThroughCentre,
    /// Circle + line crossing it off-centre (hypoid).
    Crossing,
    /// Circle + line missing it (crossed helical).
    Missing,
    /// Two lines meeting at the origin dot (a bevel's side view).
    SideView,
}

/// The picks measured for a mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub kind: Kind,
    pub shape: Shape,
    /// Gear 1's circle (the wheel, the ring gear of a hypoid, the sun, a single gear's), its
    /// mate's, and a planetary ring's.
    pub c1: Option<Circle>,
    pub c2: Option<Circle>,
    pub c3: Option<Circle>,
    /// The shaft line (or pitch line), and the worm's far-side line.
    pub line: Option<Line>,
    pub line2: Option<Line>,
    /// Pitch diameters: gear 1, its mate, a planetary ring (0: not drawn).
    pub d1: f64,
    pub d2: f64,
    pub d3: f64,
    pub center_distance: f64,
    /// Distance of the shaft line from gear 1's centre.
    pub offset: f64,
    /// The origin dot: as picked, else gear 1's centre or the apex.
    pub origin: V2,
    /// The side view's shaft angle (deg; `None`: the setting's) and apex.
    pub shaft_angle: Option<f64>,
    pub apex: Option<V2>,
    /// The worm's pitch Ø from its far-side line (0: not drawn).
    pub worm_diameter: f64,
    pub notes: Vec<String>,
}

impl Measured {
    fn new(kind: Kind, shape: Shape) -> Measured {
        Measured {
            kind,
            shape,
            c1: None,
            c2: None,
            c3: None,
            line: None,
            line2: None,
            d1: 0.0,
            d2: 0.0,
            d3: 0.0,
            center_distance: 0.0,
            offset: 0.0,
            origin: V2::ZERO,
            shaft_angle: None,
            apex: None,
            worm_diameter: 0.0,
            notes: Vec::new(),
        }
    }
}

/// Lengths this close (mm, plus this share of the size they are compared at) are equal: a
/// sketch constrained tangent or concentric is exact, a rough one within half a percent.
pub const TOLERANCE: f64 = 0.005;

fn near(a: f64, b: f64, size: f64) -> bool {
    (a - b).abs() <= 1e-6 + TOLERANCE * size.abs()
}

/// The direction along `l` from `from` towards its farther end.
fn ray(l: &Line, from: V2) -> V2 {
    let far = if l.a.dist(from) >= l.b.dist(from) { l.a } else { l.b };
    let d = far - from;
    if d.len() > 1e-12 { d * (1.0 / d.len()) } else { l.dir() }
}

/// The two lines' crossing point, or none for parallel ones.
fn crossing(l1: &Line, l2: &Line) -> Option<V2> {
    let (u, v) = (l1.dir(), l2.dir());
    let den = u.cross(v);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (l2.a - l1.a).cross(v) / den;
    Some(l1.a + u * t)
}

/// Checks the picks against mode `k`'s rule and measures them. The error says which pick is
/// missing or what is wrong with how they lie, and what that would make instead.
pub fn measure(k: Kind, d: &Drawn, picks: &[Option<usize>]) -> Result<Measured, String> {
    let dot = picks.first().copied().flatten().and_then(|i| d.points.get(i)).copied();
    let mut m = measure_own(k, d, picks.get(1..).unwrap_or(&[]), dot)?;
    m.origin = dot.or(m.apex).or(m.c1.map(|c| c.c)).unwrap_or(V2::ZERO);
    Ok(m)
}

/// [`measure`] of the slots after the origin dot.
fn measure_own(k: Kind, d: &Drawn, picks: &[Option<usize>], dot: Option<V2>) -> Result<Measured, String> {
    let sl = own_slots(k);
    let pick = |i: usize| picks.get(i).copied().flatten();
    let circle = |i: usize| -> Result<Option<Circle>, String> {
        match (pick(i).and_then(|j| d.circles.get(j)).copied(), sl.get(i)) {
            (Some(c), _) if c.r > 0.0 && c.r.is_finite() => Ok(Some(c)),
            (Some(_), _) => Err("a circle needs a radius greater than 0".into()),
            (None, Some(s)) if !s.optional => Err(format!("{}: pick or draw a circle ({})", s.label, s.hint)),
            _ => Ok(None),
        }
    };
    let line = |i: usize| -> Result<Option<Line>, String> {
        match (pick(i).and_then(|j| d.lines.get(j)).copied(), sl.get(i)) {
            (Some(l), _) if l.len() > 1e-9 => Ok(Some(l)),
            (Some(_), _) => Err("a line needs two different points".into()),
            (None, Some(s)) if !s.optional => Err(format!("{}: pick or draw a line ({})", s.label, s.hint)),
            _ => Ok(None),
        }
    };
    let need = |c: Option<Circle>| c.ok_or_else(|| "a circle is missing".to_string());
    match k {
        Kind::Pair | Kind::Cycloidal => {
            let (a, b) = (need(circle(0)?)?, need(circle(1)?)?);
            let dist = a.c.dist(b.c);
            if !near(dist, a.r + b.r, a.r.min(b.r)) {
                let how = if dist < a.r + b.r && near(dist, (a.r - b.r).abs(), a.r.min(b.r)) {
                    "one is inside the other: that is an internal pair".to_string()
                } else if dist < a.r + b.r {
                    format!("they overlap: centres {} mm apart, the radii add up to {} mm", num(dist), num(a.r + b.r))
                } else {
                    format!("they are apart: centres {} mm apart, the radii add up to {} mm", num(dist), num(a.r + b.r))
                };
                return Err(format!("the two pitch circles must touch outside; {how}. Move one, or type the centre distance"));
            }
            let mut m = Measured::new(k, Shape::Pair);
            (m.c1, m.c2, m.d1, m.d2, m.center_distance) = (Some(a), Some(b), 2.0 * a.r, 2.0 * b.r, dist);
            Ok(m)
        }
        Kind::Sprocket | Kind::Pulley => {
            let a = need(circle(0)?)?;
            let b = circle(1)?;
            let mut m = Measured::new(k, if b.is_some() { Shape::Pair } else { Shape::Single });
            (m.c1, m.d1) = (Some(a), 2.0 * a.r);
            if let Some(b) = b {
                let dist = a.c.dist(b.c);
                if dist <= a.r + b.r {
                    return Err(format!(
                        "the two circles overlap: a chain or belt drive needs them apart (centres {} mm apart, radii {} and {} mm)",
                        num(dist),
                        num(a.r),
                        num(b.r)
                    ));
                }
                (m.c2, m.d2, m.center_distance) = (Some(b), 2.0 * b.r, dist);
            }
            Ok(m)
        }
        Kind::Internal => {
            let ring = need(circle(0)?)?;
            let mut m = Measured::new(k, Shape::Internal);
            (m.c1, m.d1) = (Some(ring), 2.0 * ring.r);
            if let Some(p) = circle(1)? {
                if p.r >= ring.r {
                    return Err("the pinion's circle must be the smaller one, inside the ring's".into());
                }
                let dist = ring.c.dist(p.c);
                if !near(dist, ring.r - p.r, p.r) {
                    return Err(format!(
                        "the pinion's circle must touch the ring's inside: centres {} mm apart, the radii differ by {} mm",
                        num(dist),
                        num(ring.r - p.r)
                    ));
                }
                (m.c2, m.d2, m.center_distance) = (Some(p), 2.0 * p.r, dist);
                m.notes.push("the pinion is not built with the ring: make it as a single gear on its circle".into());
            }
            Ok(m)
        }
        Kind::Planetary => {
            let (sun, planet, ring) = (need(circle(0)?)?, need(circle(1)?)?, need(circle(2)?)?);
            if !near(sun.c.dist(ring.c), 0.0, sun.r) {
                return Err(format!("the sun's and the ring's circles must share a centre ({} mm apart)", num(sun.c.dist(ring.c))));
            }
            if !near(planet.c.dist(sun.c), sun.r + planet.r, planet.r) {
                return Err("the planet's circle must touch the sun's outside".into());
            }
            if !near(planet.c.dist(ring.c), ring.r - planet.r, planet.r) {
                return Err(format!(
                    "the planet's circle must touch the ring's inside: the ring Ø should be {} mm (sun + 2 × planet)",
                    num(2.0 * (sun.r + 2.0 * planet.r))
                ));
            }
            let mut m = Measured::new(k, Shape::Planetary);
            (m.c1, m.c2, m.c3) = (Some(sun), Some(planet), Some(ring));
            (m.d1, m.d2, m.d3, m.center_distance) = (2.0 * sun.r, 2.0 * planet.r, 2.0 * ring.r, sun.r + planet.r);
            Ok(m)
        }
        Kind::Rack | Kind::Worm => {
            let c = need(circle(0)?)?;
            let l = line(1)?.ok_or("a line is missing")?;
            let e = l.distance(c.c);
            if !near(e, c.r, c.r) {
                let what = if e < 1e-6 + TOLERANCE * c.r {
                    "it passes through the centre: that makes a bevel"
                } else if e < c.r {
                    "it crosses the circle: off-centre that makes a hypoid"
                } else {
                    "it misses the circle: that makes crossed helicals"
                };
                return Err(format!(
                    "the line must touch the circle (tangent): it is {} mm from the centre, the radius is {} mm; {what}",
                    num(e),
                    num(c.r)
                ));
            }
            let mut m = Measured::new(k, Shape::Tangent);
            (m.c1, m.line, m.d1, m.offset) = (Some(c), Some(l), 2.0 * c.r, e);
            if k == Kind::Worm
                && let Some(far) = line(2)?
            {
                if far.dir().cross(l.dir()).abs() > 1e-3 {
                    return Err("the worm's far-side line must be parallel to its pitch line".into());
                }
                let e2 = far.distance(c.c);
                if e2 <= e {
                    return Err("the worm's far-side line must lie beyond the pitch line, away from the wheel".into());
                }
                m.worm_diameter = e2 - e;
                m.line2 = Some(far);
            }
            if k == Kind::Rack {
                m.notes.push("only the rack is built: make the pinion as a spur gear on its circle".into());
            }
            Ok(m)
        }
        Kind::Bevel | Kind::SpiralBevel | Kind::BevelPlanetary => {
            let (l1, l2) = (line(0)?.ok_or("a line is missing")?, line(1)?.ok_or("a line is missing")?);
            let planetary = k == Kind::BevelPlanetary;
            let reach = l1.len().max(l2.len());
            let apex = match dot {
                Some(p) => {
                    let names = if planetary { ["the sun's", "the planet's"] } else { ["gear 1's", "gear 2's"] };
                    for (l, which) in [(&l1, names[0]), (&l2, names[1])] {
                        if !near(l.distance(p), 0.0, reach) {
                            return Err(format!(
                                "{which} shaft line must pass through the origin dot (the apex where the shafts meet); it is {} mm off",
                                num(l.distance(p))
                            ));
                        }
                    }
                    p
                }
                None => {
                    crossing(&l1, &l2).ok_or("the two shaft lines are parallel: a bevel's shafts meet at the apex (parallel shafts make a pair)")?
                }
            };
            let (r1, r2) = (ray(&l1, apex), ray(&l2, apex));
            let sigma = r1.cross(r2).abs().atan2(r1.dot(r2)).to_degrees();
            if !(5.0..=175.0).contains(&sigma) {
                return Err(format!(
                    "the shafts meet at {}°: {} 5° to 175°",
                    num(sigma),
                    if planetary { "a bevel planetary set needs" } else { "bevels need" }
                ));
            }
            let mut m = Measured::new(k, Shape::SideView);
            (m.line, m.line2, m.apex, m.shaft_angle) = (Some(l2), Some(l1), Some(apex), Some(sigma));
            Ok(m)
        }
        Kind::FaceGear => {
            let c = need(circle(0)?)?;
            let l = line(1)?.ok_or("a line is missing")?;
            let e = l.distance(c.c);
            if e > 1e-6 + TOLERANCE * c.r {
                let what = if e < c.r { "off-centre makes a hypoid" } else { "missing the circle makes crossed helicals" };
                return Err(format!("this line doesn't pass through the circle's centre ({} mm off): for a bevel it must; {what}", num(e)));
            }
            let mut m = Measured::new(k, Shape::ThroughCentre);
            (m.c1, m.line, m.d1) = (Some(c), Some(l), 2.0 * c.r);
            Ok(m)
        }
        Kind::Hypoid => {
            let c = need(circle(0)?)?;
            let l = line(1)?.ok_or("a line is missing")?;
            let e = l.distance(c.c);
            if e <= 1e-6 + TOLERANCE * c.r {
                return Err(
                    "the pinion shaft passes through the ring gear's centre: that is a spiral bevel (offset 0). Move it off the centre for a hypoid"
                        .into(),
                );
            }
            if e >= c.r {
                return Err(format!(
                    "the pinion shaft misses the ring gear's circle ({} mm from the centre, radius {} mm): a hypoid's shaft crosses it; further out makes crossed helicals",
                    num(e),
                    num(c.r)
                ));
            }
            let mut m = Measured::new(k, Shape::Crossing);
            (m.c1, m.line, m.d1, m.offset) = (Some(c), Some(l), 2.0 * c.r, e);
            Ok(m)
        }
        Kind::CrossedHelical => {
            let c = need(circle(0)?)?;
            let l = line(1)?.ok_or("a line is missing")?;
            let e = l.distance(c.c);
            if e <= c.r * (1.0 + TOLERANCE) + 1e-6 {
                return Err(format!(
                    "shaft 2 must miss the circle: it is {} mm from the centre, the radius is {} mm (crossing makes a hypoid, through the centre a bevel)",
                    num(e),
                    num(c.r)
                ));
            }
            let mut m = Measured::new(k, Shape::Missing);
            (m.c1, m.line, m.d1, m.offset) = (Some(c), Some(l), 2.0 * c.r, e);
            m.d2 = 2.0 * (e - c.r);
            Ok(m)
        }
        _ => {
            let c = need(circle(0)?)?;
            let mut m = Measured::new(k, Shape::Single);
            (m.c1, m.d1) = (Some(c), 2.0 * c.r);
            Ok(m)
        }
    }
}

/// The mode the drawing looks like (a hint only: the user's mode stays).
pub fn recognize(d: &Drawn) -> Option<Kind> {
    let order = [Kind::Planetary, Kind::Pair, Kind::Internal, Kind::Bevel, Kind::Worm, Kind::Hypoid, Kind::CrossedHelical, Kind::Spur];
    let (nc, nl) = (d.circles.len(), d.lines.len());
    order.into_iter().find(|k| {
        // Each row of the table by what it is drawn with.
        let drawn_as = match k {
            Kind::Planetary => nc >= 3,
            Kind::Pair | Kind::Internal => nc == 2,
            Kind::Spur => nc == 1 && nl == 0,
            Kind::Bevel => nc == 0 && nl >= 2,
            _ => nc == 1 && nl >= 1,
        };
        drawn_as && measure(*k, d, &auto_picks(*k, d)).is_ok()
    })
}

/// Whether two modes are the same family for a hint (spur, helical, herringbone; straight and
/// spiral bevels; rack and worm share a picture).
pub fn same_family(a: Kind, b: Kind) -> bool {
    let fam = |k: Kind| match k {
        Kind::Spur | Kind::Helical | Kind::Herringbone => 0,
        Kind::Bevel | Kind::SpiralBevel | Kind::FaceGear | Kind::BevelPlanetary => 1,
        Kind::Rack | Kind::Worm => 2,
        Kind::Pair | Kind::Cycloidal => 3,
        k => 10 + k as i32,
    };
    fam(a) == fam(b)
}

/// The module and tooth counts that give the diameters `ds` (each with its factor: pitch Ø =
/// module × teeth × factor, e.g. 1 / cos β): the largest standard module (ISO 54 series I,
/// then II) that gives each within half a percent with at least `zmin` teeth; otherwise the
/// exact module for the counts nearest a standard one (`false`).
pub fn fit_module(ds: &[(f64, f64)], zmin: u32) -> (f64, Vec<u32>, bool) {
    let counts = |m: f64| -> Option<(Vec<u32>, f64)> {
        let mut zs = Vec::new();
        let mut err = 0.0f64;
        for (d, f) in ds {
            let z = (d / (m * f)).round();
            if z < f64::from(zmin) || z > 1000.0 {
                return None;
            }
            err = err.max((m * f * z - d).abs() / d);
            zs.push(z as u32);
        }
        Some((zs, err))
    };
    for series in [&MODULES[..], &MODULES_2[..]] {
        let mut best: Option<(f64, Vec<u32>)> = None;
        for &m in series.iter().rev() {
            if let Some((zs, err)) = counts(m)
                && err <= TOLERANCE
            {
                best = Some((m, zs));
                break;
            }
        }
        if let Some((m, zs)) = best {
            return (m, zs, true);
        }
    }
    // No standard module: the nearest counts, and the module that makes their diameters add up
    // to the drawn ones exactly.
    let mut best: Option<(f64, Vec<u32>)> = None;
    for &m in MODULES.iter().chain(MODULES_2.iter()) {
        if let Some((zs, err)) = counts(m)
            && best.as_ref().is_none_or(|(e, _)| err < *e)
        {
            best = Some((err, zs));
        }
    }
    let zs = match best {
        Some((_, zs)) => zs,
        None => ds.iter().map(|_| zmin.max(1)).collect(),
    };
    let (sd, sz): (f64, f64) = ds.iter().zip(&zs).fold((0.0, 0.0), |(a, b), ((d, f), z)| (a + d, b + f * f64::from(*z)));
    (if sz > 0.0 { sd / sz } else { 1.0 }, zs, false)
}

// =============================================================================================
// Decision 1 (the user's diameter rule): the drawn pitch circle is kept. With a module, each gear
// takes the whole tooth count nearest its drawn diameter; the drawn centre distances are held
// exactly by profile shift, so the drawn circles are the operating pitch circles. Refused only
// where the shift needed leaves the teeth's limits (undercut, pointed tips), naming sizes that
// would work.

/// Teeth the automatic module gives the smallest gear at least ([`auto_module`]): the fewest a
/// 20° gear takes without undercut.
pub const AUTO_TEETH: u32 = 17;

/// The automatic module for gears of drawn pitch Ø `ds` (each with its factor, as in
/// [`fit_module`]): the largest ISO 54 series I module whose nearest whole counts give the
/// smallest gear [`AUTO_TEETH`] or more teeth (the others' diameters are reached by profile
/// shift). Where no standard module gives that (tiny gears): the largest giving `zmin` or more,
/// with a note (profile shift clears their undercut); failing that, the exact module of `zmin`
/// teeth on the smallest gear, with a note.
pub fn auto_module(ds: &[(f64, f64)], zmin: u32) -> (f64, Vec<u32>, Option<String>) {
    let counts = |m: f64| -> Vec<u32> { ds.iter().map(|(d, f)| (d / (m * f)).round().clamp(0.0, 1e6) as u32).collect() };
    let fewest = |zs: &[u32]| zs.iter().copied().min().unwrap_or(u32::MAX);
    for &m in MODULES.iter().rev() {
        let zs = counts(m);
        if fewest(&zs) >= AUTO_TEETH {
            return (m, zs, None);
        }
    }
    let zmin = zmin.max(3);
    for &m in MODULES.iter().rev() {
        let zs = counts(m);
        let few = fewest(&zs);
        if few >= zmin {
            return (
                m,
                zs,
                Some(format!(
                    "no standard module gives the smallest gear {AUTO_TEETH} teeth: module {} gives it {few}, cleared of undercut by profile shift",
                    num(m)
                )),
            );
        }
    }
    let small = ds.iter().map(|(d, f)| d / f).fold(f64::INFINITY, f64::min);
    let m = if small.is_finite() && small > 0.0 { small / f64::from(zmin) } else { 1.0 };
    (m, counts(m), Some(format!("too small for a standard module: module {} gives the smallest gear {zmin} teeth", num(m))))
}

/// The profile-shift limits of an external involute gear `g` (its own shift ignored):
/// `(lowest, highest, preferred)`.
/// - lowest: the practical undercut limit (teeth down to 5/6 of the fewest without undercut, as
///   DIN 3960 allows), but never above 0: a shift may not cut the teeth deeper than their count
///   already does;
/// - highest: a tip land of 0.2 modules left (beyond it the tips come to a point);
/// - preferred: the shift that just clears undercut (0 when there is none), within the two.
pub fn shift_limits(g: &Involute) -> (f64, f64, f64) {
    // x clear of undercut = ha − k, k = ha·z / (fewest teeth without undercut at no shift)
    // (= z·sin²αt / 2cos β); the practical limit ha − 1.2·k.
    let ha = g.rack.addendum;
    let fewest = Involute { shift: 0.0, ..*g }.min_teeth_no_undercut();
    let k = if fewest > 0.0 { ha * g.z() / fewest } else { f64::INFINITY };
    let lo = (ha - 1.2 * k).min(0.0);
    let land = |x: f64| {
        let h = Involute { shift: x, ..*g };
        h.thickness_at(h.ra()) - 0.2 * g.module
    };
    // The land narrows as the shift grows (from a modest negative shift up: far below, the
    // pitch thickness itself goes negative): searched from the lowest shift allowed, or −1.
    let (mut a, mut b) = (lo.max(-1.0), 3.0f64);
    let hi = if land(b) >= 0.0 {
        b
    } else if land(a) < 0.0 {
        a
    } else {
        for _ in 0..60 {
            let mid = 0.5 * (a + b);
            if land(mid) >= 0.0 {
                a = mid;
            } else {
                b = mid;
            }
        }
        a
    };
    let pref = (ha - k).max(0.0).min(hi).max(lo);
    (lo, hi, pref)
}

/// A mesh of a meshing group for [`group_shifts`]: gears `a` and `b` (indices into its gears;
/// `internal`: `b` is a ring with `a` inside it) and their drawn centre distance (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupMesh {
    pub a: usize,
    pub b: usize,
    pub internal: bool,
    pub dist: f64,
}

/// Why no shifts hold a stage's or group's drawn distances.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShiftFail {
    /// No profile shift puts gears `.0` and `.1` at their distance.
    Unreachable(usize, usize),
    /// The shifts needed leave the limits: `low` bounds the free shift from below, `high` from
    /// above (they cross), each a gear and whether its undercut (true) or pointed tips (false).
    Limits { low: (usize, bool), high: (usize, bool) },
}

/// The profile shifts (one per gear, in its order) for shifts that are linear in one free shift
/// t: gear k's is `lin[k].0 + lin[k].1 · t` (slope ±1, or 0 for a shift the distances fix). t is
/// chosen nearest every gear's preferred shift ([`shift_limits`]: clear of undercut, so the
/// smaller gears take the positive share) in the least-squares sense, within every gear's limits
/// (rings: none, preferring no shift).
///
/// First every gear is held clear of undercut (its preferred shift is its lowest), as the usual
/// split of a pair's shift sum does (x1 ≥ (z_min − z1)/z_min for the smaller gear, its mate the
/// rest): an even split would leave a small pinion undercut that its mate could have spared.
/// Where no free shift keeps them all clear (and their tips whole), within the practical limits.
fn choose_shifts(gears: &[Involute], ring: &[bool], lin: &[(f64, f64)]) -> Result<Vec<f64>, ShiftFail> {
    choose_shifts_within(gears, ring, lin, true).or_else(|_| choose_shifts_within(gears, ring, lin, false))
}

/// A pair's shift sum split between external gears `g1` and `g2` (their own shifts ignored) as
/// [`choose_shifts`] splits it: the smaller gear clear of undercut where its mate can spare it,
/// else nearest both gears' preferred shifts; evenly when no split keeps both within limits.
pub(crate) fn split_shift(g1: &Involute, g2: &Involute, sum: f64) -> (f64, f64) {
    match choose_shifts(&[*g1, *g2], &[false, false], &[(0.0, 1.0), (sum, -1.0)]) {
        Ok(x) => (x[0], x[1]),
        Err(_) => (sum / 2.0, sum / 2.0),
    }
}

/// [`choose_shifts`] with each gear's lowest shift its preferred one (`clear`: clear of
/// undercut) or its practical limit.
fn choose_shifts_within(gears: &[Involute], ring: &[bool], lin: &[(f64, f64)], clear: bool) -> Result<Vec<f64>, ShiftFail> {
    let (mut sum, mut den) = (0.0, 0.0);
    let mut lo: (f64, (usize, bool)) = (f64::NEG_INFINITY, (0, true));
    let mut hi: (f64, (usize, bool)) = (f64::INFINITY, (0, false));
    for (k, (g, &(a, s))) in gears.iter().zip(lin).enumerate() {
        if ring.get(k).copied().unwrap_or(false) {
            sum -= s * a;
            den += s * s;
            continue;
        }
        let (l, u, pref) = shift_limits(g);
        // (Clear: a hair over the shift that just clears it, so no rounding leaves it undercut.)
        let l = if clear && pref > 0.0 { (pref + 1e-6).min(u) } else { l };
        sum += s * (pref - a);
        den += s * s;
        if s == 0.0 {
            if a < l - 1e-9 {
                return Err(ShiftFail::Limits { low: (k, true), high: (k, true) });
            }
            if a > u + 1e-9 {
                return Err(ShiftFail::Limits { low: (k, false), high: (k, false) });
            }
            continue;
        }
        // l ≤ a + s·t ≤ u.
        let (from, to, from_cut) = if s > 0.0 { ((l - a) / s, (u - a) / s, true) } else { ((u - a) / s, (l - a) / s, false) };
        if from > lo.0 {
            lo = (from, (k, from_cut));
        }
        if to < hi.0 {
            hi = (to, (k, !from_cut));
        }
    }
    let want = if den > 0.0 && sum.is_finite() { sum / den } else { 0.0 };
    let t = if lo.0 <= hi.0 {
        want.clamp(lo.0, hi.0)
    } else if lo.0 - hi.0 <= 1e-9 {
        0.5 * (lo.0 + hi.0)
    } else {
        return Err(ShiftFail::Limits { low: lo.1, high: hi.1 });
    };
    Ok(lin.iter().map(|(a, s)| a + s * t).collect())
}

/// The profile shifts of a meshing group (decision 1): `gears` (their shifts ignored), which are
/// rings, and the `meshes` joining them (a tree from gear 0). Every mesh's drawn centre
/// distance is held exactly (outside: the shift sum, from the operating pressure angle; inside a
/// ring: the ring's shift less the gear's); the one shift left free is chosen by
/// [`choose_shifts`]. A group of one gear keeps no shift.
pub fn group_shifts(gears: &[Involute], ring: &[bool], meshes: &[GroupMesh]) -> Result<Vec<f64>, ShiftFail> {
    let n = gears.len();
    if n <= 1 {
        return Ok(vec![0.0; n]);
    }
    let at0 = |k: usize| Involute { shift: 0.0, ..gears[k] };
    let mut lin: Vec<Option<(f64, f64)>> = vec![None; n];
    lin[0] = Some((0.0, 1.0));
    let mut queue = vec![0];
    while let Some(i) = queue.pop() {
        let (ai, si) = lin[i].unwrap_or((0.0, 1.0));
        for m in meshes {
            let j = if m.a == i {
                m.b
            } else if m.b == i {
                m.a
            } else {
                continue;
            };
            if j >= n || lin[j].is_some() {
                continue;
            }
            let next = if m.internal {
                crate::plan::ring_shift_for_center(&at0(m.a), &at0(m.b), m.dist).map(|d| if j == m.b { (ai + d, si) } else { (ai - d, si) })
            } else {
                shift_sum_for_center(&at0(i), &at0(j), m.dist).map(|sum| (sum - ai, -si))
            };
            match next {
                Some(v) if v.0.is_finite() => {
                    lin[j] = Some(v);
                    queue.push(j);
                }
                _ => return Err(ShiftFail::Unreachable(m.a, m.b)),
            }
        }
    }
    let lin: Vec<(f64, f64)> = lin.into_iter().map(|v| v.unwrap_or((0.0, 0.0))).collect();
    choose_shifts(gears, ring, &lin)
}

/// "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What would work instead when the shifts can't hold the drawn distances at module `m` with
/// `zs` teeth (gears of drawn Ø and factor `ds`, named by `name`): up to three standard modules
/// nearest `m` giving every gear 8 or more teeth (the counts nearest its diameter), then up to
/// three single gears a tooth more or fewer at `m`, each one `works`.
fn other_sizes(ds: &[(f64, f64)], m: f64, zs: &[u32], name: &dyn Fn(usize) -> String, works: &dyn Fn(f64, &[u32]) -> bool) -> Vec<String> {
    let mut ideas = Vec::new();
    let mut cands: Vec<f64> = MODULES.iter().chain(MODULES_2.iter()).copied().filter(|x| (x - m).abs() > 1e-9 * m).collect();
    cands.sort_by(|a, b| (a / m).ln().abs().total_cmp(&(b / m).ln().abs()).then(b.total_cmp(a)));
    let mut found = 0;
    for mm in cands {
        if found == 3 {
            break;
        }
        let zz: Vec<u32> = ds.iter().map(|(d, f)| (d / (mm * f)).round().clamp(0.0, 1e6) as u32).collect();
        if zz.iter().all(|z| *z >= 8) && works(mm, &zz) {
            let list: Vec<String> = zz.iter().enumerate().map(|(k, z)| format!("{} z{z}", name(k))).collect();
            ideas.push(format!("module {} ({})", num(mm), list.join(", ")));
            found += 1;
        }
    }
    let mut found = 0;
    for k in 0..zs.len() {
        for z in [zs[k].saturating_sub(1), zs[k] + 1] {
            if found == 3 || z < 3 || z == zs[k] {
                continue;
            }
            let mut zz = zs.to_vec();
            zz[k] = z;
            if works(m, &zz) {
                // Or the drawing made to fit: the circle that gives that count at this module.
                let d = ds.get(k).map_or(0.0, |(_, f)| m * f64::from(z) * f);
                ideas.push(format!("{} with {z} teeth (or draw it Ø{})", name(k), num(d)));
                found += 1;
            }
        }
    }
    ideas
}

/// The refusal when no shifts hold `what` (e.g. "the drawn centre distances"): why, and `ideas`
/// ([`other_sizes`]).
fn shift_refusal(why: ShiftFail, name: &dyn Fn(usize) -> String, zs: &[u32], m: f64, what: &str, ideas: &[String]) -> String {
    let counts: Vec<String> = zs.iter().enumerate().map(|(k, z)| format!("{} z{z}", name(k))).collect();
    let text = match why {
        ShiftFail::Unreachable(a, b) => {
            format!("no profile shift puts {} and {} at {what} ({} at module {})", name(a), name(b), and_list(&counts), num(m))
        }
        ShiftFail::Limits { low, high } => {
            let says = |(k, cut): (usize, bool)| {
                if cut { format!("{} would be undercut", name(k)) } else { format!("{}'s tips would come to a point", name(k)) }
            };
            let mut list = vec![says(low)];
            if high != low {
                list.push(says(high));
            }
            format!("holding {what} needs more profile shift than these teeth take at module {} ({}): {}", num(m), and_list(&counts), and_list(&list))
        }
    };
    if ideas.is_empty() {
        format!("{text}: draw the circles nearer whole teeth at this module")
    } else {
        format!("{text}. These would work: {}", ideas.join(", "))
    }
}

/// One gear of a stage to size ([`size_stage`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageGear {
    /// Its drawn pitch Ø (mm): a bevel's, hypoid's or face gear's outer pitch Ø; a worm's pitch Ø.
    pub d: f64,
    /// Pitch Ø = module × teeth × factor: 1 / cos β for a crossed helical or helical gear, else 1.
    pub factor: f64,
    /// Teeth asked (Size by Teeth: the module follows from d ÷ (teeth × factor)); a worm's starts.
    pub teeth: Option<u32>,
}

impl StageGear {
    pub fn of(d: f64) -> StageGear {
        StageGear { d, factor: 1.0, teeth: None }
    }
}

/// What [`size_stage`] made of a stage: the module, and per gear its teeth, profile shift
/// coefficient and built pitch Ø; notes on what differs from the drawing.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StageSize {
    pub module: f64,
    pub teeth: Vec<u32>,
    pub shift: Vec<f64>,
    pub pitch_diameter: Vec<f64>,
    pub notes: Vec<String>,
}

/// Sizes one stage by decision 1, its gears as drawn (`gears`, gear 1 first) and `module` asked
/// (a module block's or the gear's own; `None`: by teeth asked, else [`auto_module`]). `base`
/// gives the pressure angle, rack, backlash, the shaft angle (bevel), a ratio (`teeth2 / teeth`,
/// where the drawing gives one gear) and the worm's starts.
/// - Pair / spur / helical / herringbone `[gear 1, gear 2]` (touching outside) and internal
///   `[ring, gear inside]`: as a train's meshing group ([`group_shifts`]).
/// - Rack `[pinion]`: the pitch line at the drawn radius by the pinion's shift.
/// - Worm `[wheel, worm]`: the wheel sized by the rule; the worm's Ø as drawn (its `teeth` the
///   starts); the drawn centre distance (Ø sum ÷ 2) by the wheel's shift.
/// - Crossed helical `[gear 1, gear 2]` (factors 1 / cos β of each): point contact, so the
///   centre distance is the pitch radii plus (x1 + x2)·m; the drawn one held by that sum.
/// - Bevel, spiral bevel, bevel planetary `[gear 1, gear 2?]`: the rule can't hold both drawn
///   Ø: the pitch cones meet at the apex, so the ratio sets the cone angles and a shift moves
///   nothing. Gear 1's drawn outer pitch Ø is kept (module = Ø ÷ its rounded teeth, standard or
///   not), gear 2 takes the teeth nearest its drawn Ø (or `base`'s ratio), with a note where the
///   ratio differs from the drawn one; shifts x1 = −x2 clear the smaller gear's undercut
///   (virtual teeth z / cos δ).
/// - Hypoid, face gear `[ring or face gear]`: likewise the drawn Ø kept (module = Ø ÷ rounded
///   teeth); the pinion's size is the settings'; no shift.
pub fn size_stage(kind: Kind, gears: &[StageGear], module: Option<f64>, base: &Spec) -> Result<StageSize, String> {
    let need = match kind {
        Kind::Pair | Kind::Spur | Kind::Helical | Kind::Herringbone | Kind::Internal | Kind::Worm | Kind::CrossedHelical => 2,
        _ => 1,
    };
    if gears.len() < need {
        return Err(format!("a {} stage needs {need} drawn gears", kind.label().to_lowercase()));
    }
    if gears.iter().any(|g| !(g.d > 0.0 && g.d.is_finite() && g.factor >= 1.0 && g.factor.is_finite())) {
        return Err("each gear needs a drawn pitch Ø greater than 0".into());
    }
    // The gears the module sizes: a worm's own Ø is free.
    let sized: &[StageGear] = match kind {
        Kind::Worm | Kind::Hypoid | Kind::FaceGear | Kind::Rack => &gears[..1],
        _ => gears,
    };
    let ds: Vec<(f64, f64)> = sized.iter().map(|g| (g.d, g.factor)).collect();
    let mut notes = Vec::new();
    let by_teeth = sized.iter().find_map(|g| g.teeth.filter(|z| *z > 0).map(|z| g.d / (g.factor * f64::from(z))));
    let m = match module.filter(|m| *m > 0.0 && m.is_finite()).or(by_teeth) {
        Some(m) => m,
        None => {
            let (m, _, note) = auto_module(&ds, 8);
            notes.extend(note);
            m
        }
    };
    let zs: Vec<u32> =
        sized.iter().map(|g| g.teeth.filter(|z| *z > 0).unwrap_or_else(|| (g.d / (m * g.factor)).round().clamp(3.0, 1e6) as u32)).collect();
    match stage_at(kind, gears, m, &zs, base, &mut notes) {
        Ok(mut s) => {
            s.notes.splice(0..0, notes);
            Ok(s)
        }
        Err(why) => {
            let name = |k: usize| format!("gear {}", k + 1);
            let ideas = other_sizes(&ds, m, &zs, &name, &|mm, zz| stage_at(kind, gears, mm, zz, base, &mut Vec::new()).is_ok());
            let what = if matches!(kind, Kind::Rack) { "the drawn pitch line" } else { "the drawn centre distance" };
            Err(shift_refusal(why, &name, &zs, m, what, &ideas))
        }
    }
}

/// [`size_stage`] at module `m` with teeth `zs` (one per gear the module sizes).
fn stage_at(kind: Kind, gears: &[StageGear], m: f64, zs: &[u32], base: &Spec, notes: &mut Vec<String>) -> Result<StageSize, ShiftFail> {
    let helix_of = |f: f64| (1.0 / f).clamp(-1.0, 1.0).acos().to_degrees();
    let inv = |m: f64, z: u32, f: f64| Spec { module: m, train: None, ..base.clone() }.involute(z.max(3), 0.0, helix_of(f));
    let built = |k: usize| m * gears[k].factor * f64::from(zs[k]);
    let mut out = StageSize { module: m, teeth: zs.to_vec(), ..StageSize::default() };
    match kind {
        Kind::Pair | Kind::Spur | Kind::Helical | Kind::Herringbone | Kind::Internal => {
            let internal = kind == Kind::Internal;
            // Internal: gear 1 is the ring, gear 2 inside it (the mesh's `b` is the ring).
            let gs = [inv(m, zs[0], gears[0].factor), inv(m, zs[1], gears[1].factor)];
            let (gs, ring, dist) = if internal {
                ([gs[1], gs[0]], [false, true], (gears[0].d - gears[1].d) / 2.0)
            } else {
                (gs, [false, false], (gears[0].d + gears[1].d) / 2.0)
            };
            let mut xs = group_shifts(&gs, &ring, &[GroupMesh { a: 0, b: 1, internal, dist }]).map_err(|e| if internal { swap(e) } else { e })?;
            if internal {
                xs.swap(0, 1);
            }
            out.shift = xs;
            out.pitch_diameter = vec![built(0), built(1)];
        }
        Kind::Rack => {
            let x = (gears[0].d - built(0)) / (2.0 * m);
            out.shift = choose_shifts(&[inv(m, zs[0], gears[0].factor)], &[false], &[(x, 0.0)])?;
            out.pitch_diameter = vec![built(0)];
        }
        Kind::Worm => {
            let x = (gears[0].d - built(0)) / (2.0 * m);
            let mut xs = choose_shifts(&[inv(m, zs[0], 1.0)], &[false], &[(x, 0.0)])?;
            xs.push(0.0);
            out.shift = xs;
            out.teeth.push(gears[1].teeth.filter(|z| *z > 0).unwrap_or(base.starts.max(1)));
            out.pitch_diameter = vec![built(0), gears[1].d];
        }
        Kind::CrossedHelical => {
            let sum = ((gears[0].d + gears[1].d) - (built(0) + built(1))) / (2.0 * m);
            let gs = [inv(m, zs[0], gears[0].factor), inv(m, zs[1], gears[1].factor)];
            out.shift = choose_shifts(&gs, &[false, false], &[(0.0, 1.0), (sum, -1.0)])?;
            out.pitch_diameter = vec![built(0), built(1)];
        }
        Kind::Bevel | Kind::SpiralBevel | Kind::BevelPlanetary => {
            let z1 = zs[0];
            let me = gears[0].d / f64::from(z1);
            if (me - m).abs() > 1e-9 * m {
                notes.push(format!(
                    "module {} keeps gear 1's drawn outer pitch Ø{} with {z1} teeth (a bevel's pitch cones fix its size: no shift reaches it)",
                    num(me),
                    num(gears[0].d)
                ));
            }
            let z2 = match gears.get(1) {
                Some(g) => g.teeth.filter(|z| *z > 0).unwrap_or_else(|| (g.d / me).round().clamp(3.0, 1e6) as u32),
                None => ((f64::from(z1) * f64::from(base.teeth2.max(1)) / f64::from(base.teeth.max(1))).round() as u32).max(3),
            };
            if let Some(g) = gears.get(1)
                && (me * f64::from(z2) - g.d).abs() > 1e-6
            {
                notes.push(format!(
                    "gear 2: Ø{} drawn, Ø{} built ({z2} teeth): the ratio is {z2}:{z1} = {:.4}, not the drawn {:.4}; the cone angles follow the teeth",
                    num(g.d),
                    num(me * f64::from(z2)),
                    f64::from(z2) / f64::from(z1),
                    g.d / gears[0].d
                ));
            }
            let sigma = base.shaft_angle.to_radians();
            let u = f64::from(z2) / f64::from(z1);
            let d1 = sigma.sin().atan2(u + sigma.cos());
            let virt = |z: u32, delta: f64| Involute { virtual_teeth: f64::from(z) / delta.cos().abs().max(1e-6), ..inv(me, z, 1.0) };
            let gs = [virt(z1, d1), virt(z2, sigma - d1)];
            out.module = me;
            out.teeth = vec![z1, z2];
            out.shift = choose_shifts(&gs, &[false, false], &[(0.0, 1.0), (0.0, -1.0)])?;
            out.pitch_diameter = vec![gears[0].d, me * f64::from(z2)];
        }
        _ => {
            // Hypoid, face gear and the rest: the drawn Ø kept with the rounded teeth.
            let me = gears[0].d / f64::from(zs[0]);
            if (me - m).abs() > 1e-9 * m {
                notes.push(format!("module {} keeps the drawn pitch Ø{} with {} teeth", num(me), num(gears[0].d), zs[0]));
            }
            out.module = me;
            out.shift = vec![0.0];
            out.pitch_diameter = vec![gears[0].d];
        }
    }
    Ok(out)
}

/// A [`ShiftFail`] of a two-gear stage solved in the other order.
fn swap(e: ShiftFail) -> ShiftFail {
    let o = |k: usize| 1 - k.min(1);
    match e {
        ShiftFail::Unreachable(a, b) => ShiftFail::Unreachable(o(a), o(b)),
        ShiftFail::Limits { low, high } => ShiftFail::Limits { low: (o(low.0), low.1), high: (o(high.0), high.1) },
    }
}

/// The spec of the measured mode: `base` (of the same kind) with the module and tooth counts
/// that give the drawn diameters, and the sizes the drawing gives (offset, centre distance,
/// shaft angle, worm Ø). What the drawing doesn't give (helix, face width, the mate's size by
/// ratio when only one circle is drawn, bores…) stays `base`'s.
pub fn spec_for(m: &Measured, base: &Spec) -> Result<(Spec, Vec<String>), String> {
    let mut s = base.clone();
    s.kind = m.kind;
    let mut notes = m.notes.clone();
    let inv_cos = |deg: f64| 1.0 / deg.to_radians().cos().max(1e-6);
    let helix = |k: Kind| if matches!(k, Kind::Spur) { 1.0 } else { inv_cos(s.helix) };
    let mut exact = |std: bool, m: f64| {
        if !std {
            notes.push(format!("no standard module gives these diameters: module {} gives them exactly", num(m)));
        }
    };
    let ratio = f64::from(base.teeth2.max(1)) / f64::from(base.teeth.max(1));
    match m.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => {
            let (md, z, std) = fit_module(&[(m.d1, helix(m.kind))], 8);
            exact(std, md);
            (s.module, s.teeth) = (md, z[0]);
        }
        Kind::Pair | Kind::Cycloidal => {
            let f = if m.kind == Kind::Pair { helix(Kind::Pair) } else { 1.0 };
            let (md, z, std) = fit_module(&[(m.d1, f), (m.d2, f)], if m.kind == Kind::Cycloidal { 6 } else { 8 });
            exact(std, md);
            (s.module, s.teeth, s.teeth2) = (md, z[0], z[1]);
            s.center_distance = 0.0;
        }
        Kind::Sprocket | Kind::Pulley => {
            let pitch = if m.kind == Kind::Sprocket {
                crate::sprocket::chain_by_id(s.chain).map(|c| c.pitch).unwrap_or(s.chain_pitch)
            } else {
                crate::pulley::belt_by_id(s.belt).map(|b| b.pitch).unwrap_or(2.0)
            };
            let teeth = |d: f64| -> u32 {
                let z = if m.kind == Kind::Sprocket { PI / (pitch / d).clamp(-1.0, 1.0).asin() } else { PI * d / pitch };
                z.round().clamp(6.0, 400.0) as u32
            };
            s.teeth = teeth(m.d1);
            s.teeth2 = if m.d2 > 0.0 { teeth(m.d2) } else { 0 };
            s.center_distance = m.center_distance;
        }
        Kind::Internal => {
            let (md, z, std) = fit_module(&[(m.d1, helix(Kind::Internal))], 20);
            exact(std, md);
            (s.module, s.teeth) = (md, z[0]);
        }
        Kind::Planetary => {
            let f = helix(Kind::Planetary);
            let (md, z, std) = fit_module(&[(m.d1, f), (m.d2, f)], 8);
            exact(std, md);
            (s.module, s.teeth, s.teeth2) = (md, z[0], z[1]);
            s.teeth3 = z[0] + 2 * z[1];
            if !(s.teeth + s.teeth3).is_multiple_of(s.planets.max(1)) {
                notes.push(format!(
                    "{} planets don't space evenly with {} + {} teeth: (sun + ring) ÷ planets must be whole",
                    s.planets, s.teeth, s.teeth3
                ));
            }
        }
        Kind::Bevel | Kind::SpiralBevel | Kind::BevelPlanetary => {
            // Drawn edge on: the angle is the drawing's; the sizes (module, teeth: gear 1's
            // pitch Ø and the ratio) are the settings'. The lines' lengths are not used.
            if let Some(a) = m.shaft_angle {
                s.shaft_angle = a;
            }
        }
        Kind::FaceGear => {
            let (md, z, std) = fit_module(&[(m.d1, 1.0)], 20);
            exact(std, md);
            s.module = md;
            s.teeth2 = z[0];
            s.teeth = ((f64::from(z[0]) / ratio.max(1.0)).round() as u32).clamp(6, z[0].saturating_sub(2).max(6));
            s.shaft_angle = 90.0;
        }
        Kind::Hypoid => {
            s.module = m.d1 / f64::from(s.teeth2.max(1));
            s.offset = m.offset;
        }
        Kind::CrossedHelical => {
            let sigma = s.shaft_angle;
            let b1 = if s.helix > 0.0 && s.helix < sigma { s.helix } else { sigma / 2.0 };
            if m.d2 <= 0.0 {
                return Err("shaft 2 must miss gear 1's circle for crossed helicals".into());
            }
            let (md, z, std) = fit_module(&[(m.d1, inv_cos(b1)), (m.d2, inv_cos(sigma - b1))], 8);
            exact(std, md);
            (s.module, s.teeth, s.teeth2, s.helix) = (md, z[0], z[1], b1);
        }
        Kind::Worm => {
            let (md, z, std) = fit_module(&[(m.d1, 1.0)], 20);
            exact(std, md);
            (s.module, s.teeth) = (md, z[0]);
            if m.worm_diameter > 0.0 {
                s.worm_diameter = m.worm_diameter;
            }
        }
        Kind::Rack => {
            let (md, z, std) = fit_module(&[(m.d1, 1.0)], 8);
            exact(std, md);
            s.module = md;
            s.teeth = z[0];
            let len = m.line.map(|l| l.len()).unwrap_or(0.0);
            s.rack_teeth = ((len / (PI * md)).round() as u32).clamp(1, 2000);
        }
        _ => {
            let (md, z, std) = fit_module(&[(m.d1, 1.0)], 6);
            exact(std, md);
            (s.module, s.teeth) = (md, z[0]);
        }
    }
    Ok((s, notes))
}

/// The Simple-mode request the measured drawing makes: `settings` (the form's own fields: face
/// width, helix, the shaft angle a single sketch can't show, a bevel's sizes, …) with what the
/// drawing gives laid over them, as one size driver and a ratio where the drawing has two sizes
/// (giving every drawn size would over-constrain [`crate::simple::solve`]).
pub fn simple_of(m: &Measured, settings: &crate::simple::Simple) -> crate::simple::Simple {
    let mut s = settings.clone();
    s.kind = Some(m.kind);
    let d = |v: f64| (v > 0.0).then_some(v);
    match m.kind {
        Kind::Pair => {
            (s.pitch_diameter, s.ratio, s.center_distance, s.pitch_diameter2) = (d(m.d1), Some(m.d2 / m.d1), None, None);
        }
        Kind::Cycloidal => {
            (s.pitch_diameter, s.ratio, s.center_distance) = (d(m.d1), Some(m.d1 / m.d2), None);
        }
        Kind::Sprocket | Kind::Pulley => {
            s.pitch_diameter = d(m.d1);
            s.ratio = (m.d2 > 0.0).then(|| m.d2 / m.d1);
            s.center_distance = d(m.center_distance);
        }
        Kind::Internal => {
            s.ring_diameter = d(m.d1);
            s.pitch_diameter = d(m.d2);
        }
        Kind::Planetary => {
            // Sun and ring drawn: the ratio with the ring fixed is 1 + ring ÷ sun.
            (s.pitch_diameter, s.ratio, s.ring_diameter, s.pitch_diameter2) = (d(m.d1), Some(1.0 + m.d3 / m.d1), None, None);
        }
        Kind::Rack => {
            s.pitch_diameter = d(m.d1);
            s.length = m.line.map(|l| l.len()).filter(|l| *l > 0.0);
        }
        Kind::Worm => {
            s.pitch_diameter2 = d(m.d1);
            if m.worm_diameter > 0.0 {
                s.pitch_diameter = Some(m.worm_diameter);
            }
            s.shaft_angle = Some(90.0);
        }
        Kind::Bevel | Kind::SpiralBevel | Kind::BevelPlanetary => {
            s.shaft_angle = m.shaft_angle.or(s.shaft_angle);
        }
        Kind::FaceGear => {
            s.pitch_diameter2 = d(m.d1);
            s.shaft_angle = Some(90.0);
        }
        Kind::Hypoid => {
            (s.pitch_diameter2, s.pitch_diameter, s.offset) = (d(m.d1), None, Some(m.offset));
        }
        Kind::CrossedHelical => {
            // Gear 2's size is what the drawn offset leaves beyond gear 1's circle.
            (s.pitch_diameter, s.pitch_diameter2, s.center_distance, s.ratio) = (d(m.d1), d(m.d2), Some(m.offset), None);
        }
        _ => s.pitch_diameter = d(m.d1),
    }
    s
}

// =============================================================================================
// One reader for every drawing: the set the whole sketch describes.

/// The tooth styles of parallel-axis gears: a drawing of circles in one of them is a gear set
/// (a single gear, a pair, a planetary set or a train), whatever its count.
pub fn is_style(k: Kind) -> bool {
    matches!(k, Kind::Spur | Kind::Helical | Kind::Herringbone)
}

/// What a whole drawing makes ([`read`]).
// A reading is made once per drawing and matched in many places, so boxing its big variant buys
// nothing worth the noise.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// One of the modes of the table at the top, measured from these picks (what [`measure`]
    /// gives): a single gear, a pair, a planetary set, a rack, a worm, a bevel...
    Mode { kind: Kind, picks: Picks, measured: Measured },
    /// A gear train of these circles (indices into `Drawn::circles`, gear 1 first), sized by
    /// [`train_spec`] and planned with the tooth style.
    Train { circles: Vec<usize> },
}

/// How two circles of a drawing lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    /// Touching outside: an external mesh.
    External,
    /// The smaller touching the bigger inside: a gear in a ring.
    Internal,
    /// The same centre: gears on one shaft.
    Concentric,
    Apart,
    /// The smaller inside the bigger, not touching.
    Inside,
    /// Crossing each other.
    Crossing,
}

/// How circles `a` and `b` lie (within [`TOLERANCE`]).
pub fn touch(a: &Circle, b: &Circle) -> Touch {
    let d = a.c.dist(b.c);
    let size = a.r.min(b.r);
    if near(d, 0.0, size) {
        Touch::Concentric
    } else if near(d, a.r + b.r, size) {
        Touch::External
    } else if near(d, (a.r - b.r).abs(), size) {
        Touch::Internal
    } else if d > a.r + b.r {
        Touch::Apart
    } else if d < (a.r - b.r).abs() {
        Touch::Inside
    } else {
        Touch::Crossing
    }
}

/// Reads the whole drawing as the set it describes. `style` is what the user chose: a tooth
/// style (spur, helical, herringbone; `None` reads circles as spur and lines as [`recognize`]
/// does) lets the drawing decide the set, while a family (pair, planetary, internal, rack, worm,
/// bevel, hypoid, cycloidal, sprocket...) is measured by its own rule, as before.
///
/// With a tooth style:
/// - circles only: one is a single gear; two touching outside a pair; a sun, a planet touching
///   it and a ring round the sun a planetary set (the planets are a count); anything else a
///   [`Reading::Train`] (meshes outside and inside rings, gears sharing a shaft, gears on their
///   own);
/// - one circle and a line: tangent a rack, through the centre a face gear, crossing it a hypoid,
///   missing it crossed helicals;
/// - lines meeting at a point (no circle): a bevel pair at the drawn angle (spiral for helical
///   and herringbone teeth).
pub fn read(style: Option<Kind>, d: &Drawn) -> Result<Reading, String> {
    let mode = |k: Kind| -> Result<Reading, String> {
        let picks = auto_picks(k, d);
        let measured = measure(k, d, &picks)?;
        Ok(Reading::Mode { kind: k, picks, measured })
    };
    if let Some(k) = style.filter(|k| !is_style(*k)) {
        if !sketched(k) {
            return Err(format!("a {} is not drawn from a sketch: give its numbers instead", k.label()));
        }
        return mode(k);
    }
    let (nc, nl) = (d.circles.len(), d.lines.len());
    let spiral = matches!(style, Some(Kind::Helical | Kind::Herringbone));
    if nl > 0 {
        if nc >= 2 {
            return Err("a train with a rack or a shaft line is not made yet: draw the gears' circles alone, or one circle with its line".into());
        }
        let k = match (nc, style) {
            // No style: what the drawing looks like, as before (a tangent line is a worm).
            (_, None) => recognize(d).ok_or(
                "these lines make no set: draw a circle and a line touching it (a rack or worm), two shaft lines meeting at a dot (bevels), or a circle with a line crossing or missing it (hypoid, crossed helicals)",
            )?,
            (0, _) if nl >= 2 => {
                if spiral {
                    Kind::SpiralBevel
                } else {
                    Kind::Bevel
                }
            }
            (0, _) => return Err("one line makes no set: draw a second shaft line from the same dot for bevels, or a pitch circle".into()),
            _ => {
                let (c, l) = (d.circles[0], d.lines[0]);
                let e = l.distance(c.c);
                if near(e, c.r, c.r) {
                    Kind::Rack
                } else if e < 1e-6 + TOLERANCE * c.r {
                    Kind::FaceGear
                } else if e < c.r {
                    Kind::Hypoid
                } else {
                    Kind::CrossedHelical
                }
            }
        };
        return mode(k);
    }
    let style = style.unwrap_or(Kind::Spur);
    match nc {
        0 => Err("draw the gears' pitch circles (or shaft lines)".into()),
        1 => mode(style),
        2 if touch(&d.circles[0], &d.circles[1]) == Touch::External => mode(Kind::Pair),
        3 if measure(Kind::Planetary, d, &auto_picks(Kind::Planetary, d)).is_ok() => mode(Kind::Planetary),
        _ => Ok(Reading::Train { circles: (0..nc).collect() }),
    }
}

/// The drawing with only the picked points, circles and lines (indices into `d`'s), in the
/// order given: what [`read`] reads when only some of the sketch is picked.
pub fn subset(d: &Drawn, points: &[usize], circles: &[usize], lines: &[usize]) -> Drawn {
    Drawn {
        points: points.iter().filter_map(|i| d.points.get(*i).copied()).collect(),
        circles: circles.iter().filter_map(|i| d.circles.get(*i).copied()).collect(),
        lines: lines.iter().filter_map(|i| d.lines.get(*i).copied()).collect(),
    }
}

/// Where a train drawn in the sketch goes (`Spec::placement`): `frame` (the sketch's plane) with
/// the set turned by `turn` (as [`axes_for`] turns shafts) about `origin` (the origin dot, or
/// gear 1's centre). The train's gear centres are the circles' own coordinates.
pub fn train_placement(origin: V2, frame: &Frame, turn: [f64; 3]) -> Frame {
    let o = [origin.x, origin.y, 0.0];
    let r = |v: [f64; 3]| rotate(v, turn);
    frame.compose(&Frame { origin: sub3(o, r(o)), x: r([1.0, 0.0, 0.0]), z: r([0.0, 0.0, 1.0]) })
}

/// `text` with the gears it names by number ("gear 3", "gears 2 and 3") named by `name` (from
/// the gear's index: "circle c3", "member 5").
pub fn rename_gears(text: &str, name: &dyn Fn(usize) -> String) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let digits = |w: &str| -> Option<(usize, String)> {
        let d: String = w.chars().take_while(char::is_ascii_digit).collect();
        let v: usize = d.parse().ok()?;
        Some((v.checked_sub(1)?, w[d.len()..].to_string()))
    };
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut k = 0;
    while k < words.len() {
        let w = words[k];
        // "(gear 4": the name after its bracket.
        let lead = w.len() - w.trim_start_matches(['(', '[']).len();
        match (&w[lead..], words.get(k + 1).and_then(|n| digits(n))) {
            ("gear", Some((g, rest))) => {
                out.push(format!("{}{}{rest}", &w[..lead], name(g)));
                k += 2;
            }
            ("gears", Some((g, rest))) if rest.is_empty() && words.get(k + 2) == Some(&"and") => match words.get(k + 3).and_then(|n| digits(n)) {
                Some((h, rest2)) => {
                    out.push(format!("{} and {}{rest2}", name(g), name(h)));
                    k += 4;
                }
                None => {
                    out.push(w.to_string());
                    k += 1;
                }
            },
            _ => {
                out.push(w.to_string());
                k += 1;
            }
        }
    }
    out.join(" ")
}

/// One circle of a train to size ([`size_train`]), with what is asked of its gear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrainCircle {
    pub circle: Circle,
    /// Where it lies along the shafts (mm), for a drivetrain drawn in 3D: circles mesh only
    /// with circles at the same place, and circles on one shaft at different places are a
    /// cluster. `None` (a flat drawing): circles on one shaft go on layers worked out here.
    pub z: Option<f64>,
    pub module: Option<f64>,
    pub teeth: Option<u32>,
    pub face_width: Option<f64>,
    /// It must be (true) or must not be (false) a ring gear (`None`: as the drawing says).
    pub ring: Option<bool>,
    /// Build a shaft on its axis, through it and the gears sharing its axis (its bore made to
    /// fit): `shaft_span` where the shaft runs along the axis (as `z`: a drawn shaft line's ends),
    /// else past the outer gears' faces; `shaft_diameter` its diameter, else half the smallest
    /// root diameter of the gears on it.
    pub shaft: bool,
    pub shaft_span: Option<(f64, f64)>,
    pub shaft_diameter: Option<f64>,
    /// Its helix angle (degrees, its own hand's sign ignored: hands alternate across meshes),
    /// and V (herringbone) teeth; `None`: the set's. Its meshing group shares them.
    pub helix: Option<f64>,
    pub herringbone: Option<bool>,
    /// Which side of its sketch plane (`z`) its face extends along the shafts: side A along +z,
    /// side B along −z (`None`: all on side A, its back face on `z`). A caller whose gear axis
    /// points along −z turns it round first ([`SidePreset::swapped`]). A custom split fixes the
    /// face width (`a + b`) over `face_width`.
    pub sides: Option<SidePreset>,
}

impl TrainCircle {
    /// A circle with nothing asked of it.
    pub fn of(circle: Circle) -> TrainCircle {
        TrainCircle {
            circle,
            z: None,
            module: None,
            teeth: None,
            face_width: None,
            ring: None,
            shaft: false,
            shaft_span: None,
            shaft_diameter: None,
            helix: None,
            herringbone: None,
            sides: None,
        }
    }
}

/// What [`size_train`] made of the circles: the spec (none when a member is wrong), notes, and
/// per gear what it became and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sized {
    pub spec: Option<Spec>,
    pub notes: Vec<String>,
    /// One per circle, in their order (`source` its index): module, teeth, pitch diameter and
    /// face width as sized (0 where an error left them unknown), meshes and cluster.
    pub members: Vec<crate::plan::SetMember>,
    /// The clusters: gears on one shaft joined as one part (indices into `members`).
    pub clusters: Vec<Vec<usize>>,
    /// One per circle: what is wrong with its gear.
    pub errors: Vec<Option<String>>,
    /// Why there is no spec: every member's error in one line, or one about the whole train.
    pub error: Option<String>,
    /// One per circle: its profile shift coefficient (the train's gears carry the same; 0 where
    /// an error left it unknown).
    pub shifts: Vec<f64>,
}

/// Sizes the train drawn as `circles` (gear 1 first: the driver) with tooth style `style`
/// ([`train_spec`] for circles with nothing asked of them):
/// - the circles' links: touching outside an external mesh, touching inside a gear in a ring,
///   the same centre gears on one shaft (a cluster: each meshing group on its own layer along
///   the shafts, or where `z` puts it); circles that cross must be on different layers, and a
///   circle touching nothing is an error (it would mesh with nothing);
/// - one module per meshing group (decision 1): the module a member asks (all its members agree;
///   a module block's reaches here as each member's), else the one the first gear asking teeth
///   gives (drawn Ø ÷ teeth, standard or not), else `settings.module`, else [`auto_module`] (the
///   standard module giving the smallest gear 17+ teeth; with `settings.exact` the module that
///   draws the group exactly); each gear the teeth it asks, else the count nearest its drawn Ø;
/// - the drawn centre distances held exactly by profile shift ([`group_shifts`]: the drawn
///   circles are the operating pitch circles), the free shift of each group spent clearing the
///   smaller gears' undercut; refused only where the shifts leave the teeth's limits
///   (undercut, pointed tips), naming modules and tooth counts that would work (`shifts`);
/// - helix hands opposite across outside meshes, the same inside rings and on a shaft.
///
/// What the drawing doesn't give stays `base`'s (pressure angle, rack, backlash, bores...);
/// `settings` gives the face width (default 10 modules; a member's own wins), helix angle
/// (default `base`'s, or 20° for helical teeth), gear 1's hand (`left_hand`), the module and
/// `exact`.
pub fn size_train(circles: &[TrainCircle], style: Kind, settings: &crate::simple::Simple, base: &Spec) -> Sized {
    use crate::plan::{LinkKind, SetMember, Train, TrainGear, TrainLink};
    let n = circles.len();
    let mut out = Sized { errors: vec![None; n], ..Sized::default() };
    let whole = |out: &mut Sized, e: String| {
        out.error = Some(e);
    };
    if n == 0 {
        whole(&mut out, "draw the gears' pitch circles".into());
        return out;
    }
    if n > crate::plan::train::MAX_GEARS {
        whole(&mut out, format!("a gear train takes {} gears at most", crate::plan::train::MAX_GEARS));
        return out;
    }
    if !is_style(style) {
        whole(&mut out, format!("a gear train's teeth are spur, helical or herringbone, not {}", style.label().to_lowercase()));
        return out;
    }
    let mut err: Vec<Option<String>> = vec![None; n];
    let fail = |err: &mut Vec<Option<String>>, i: usize, e: String| {
        if err[i].is_none() {
            err[i] = Some(e);
        }
    };
    let c: Vec<Circle> = circles.iter().map(|t| t.circle).collect();
    for (i, k) in c.iter().enumerate() {
        if !(k.r > 0.0 && k.r.is_finite() && k.c.is_finite()) || circles[i].z.is_some_and(|z| !z.is_finite()) {
            fail(&mut err, i, format!("gear {}: its circle needs a radius greater than 0", i + 1));
        }
    }
    let three_d = circles.iter().any(|t| t.z.is_some());
    let level = |i: usize, j: usize| match (circles[i].z, circles[j].z) {
        (Some(a), Some(b)) => near(a, b, c[i].r.min(c[j].r)),
        _ => true,
    };
    let mut notes = Vec::new();
    // How the circles lie.
    let mut links: Vec<TrainLink> = Vec::new();
    let mut crossing = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            if err[i].is_some() || err[j].is_some() {
                continue;
            }
            let (a, b) = (&c[i], &c[j]);
            match touch(a, b) {
                Touch::Concentric if level(i, j) && (three_d || near(a.r, b.r, a.r.min(b.r))) => {
                    let e = if near(a.r, b.r, a.r.min(b.r)) {
                        format!("gears {} and {}: their circles are the same circle", i + 1, j + 1)
                    } else {
                        format!(
                            "gears {} and {} are concentric in one plane: draw one further along the shaft (gears on one shaft are a cluster)",
                            i + 1,
                            j + 1
                        )
                    };
                    fail(&mut err, i, e.clone());
                    fail(&mut err, j, e);
                }
                Touch::Concentric => links.push(TrainLink { a: i, b: j, kind: LinkKind::Compound }),
                _ if !level(i, j) => {}
                Touch::External => links.push(TrainLink { a: i, b: j, kind: LinkKind::External }),
                Touch::Internal => {
                    let (inner, ring) = if a.r < b.r { (i, j) } else { (j, i) };
                    links.push(TrainLink { a: inner, b: ring, kind: LinkKind::Internal });
                }
                Touch::Crossing => crossing.push((i, j)),
                Touch::Apart | Touch::Inside => {}
            }
        }
    }
    // Ring gears: the outer circle of a gear touching inside. Nothing meshes on their outside.
    let ring: Vec<bool> = (0..n).map(|i| links.iter().any(|l| l.kind == LinkKind::Internal && l.b == i)).collect();
    for i in 0..n {
        match circles[i].ring {
            Some(true) if !ring[i] => {
                fail(&mut err, i, format!("gear {} is to be a ring gear (internal), but no gear's circle touches its circle inside", i + 1))
            }
            Some(false) if ring[i] => {
                fail(&mut err, i, format!("gear {} is a ring gear (a gear touches its circle inside), but it is not to be internal", i + 1))
            }
            _ => {}
        }
    }
    for l in &links {
        match l.kind {
            LinkKind::External if ring[l.a] || ring[l.b] => {
                let (r, other) = if ring[l.a] { (l.a, l.b) } else { (l.b, l.a) };
                let inside = links.iter().find(|k| k.kind == LinkKind::Internal && k.b == r).map_or(0, |k| k.a);
                let e = format!(
                    "gear {} is a ring gear (gear {} meshes inside it), but gear {} touches its outside: gears mesh only inside a ring",
                    r + 1,
                    inside + 1,
                    other + 1
                );
                fail(&mut err, r, e.clone());
                fail(&mut err, other, e);
            }
            LinkKind::Internal if ring[l.a] => {
                fail(&mut err, l.a, format!("gear {} is a ring gear inside ring gear {}: a ring can't mesh inside another", l.a + 1, l.b + 1))
            }
            _ => {}
        }
    }
    // Meshing groups (one module each): each gear's group is its lowest-numbered gear.
    let mut group: Vec<usize> = (0..n).collect();
    loop {
        let mut changed = false;
        for l in links.iter().filter(|l| l.kind != LinkKind::Compound) {
            let low = group[l.a].min(group[l.b]);
            for i in [l.a, l.b] {
                if group[i] != low {
                    group[i] = low;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for l in links.iter().filter(|l| l.kind == LinkKind::Compound) {
        if group[l.a] == group[l.b] {
            let e = format!(
                "gears {} and {} share a shaft (their circles are concentric) but also mesh through the train: they would lock",
                l.a + 1,
                l.b + 1
            );
            fail(&mut err, l.a, e.clone());
            fail(&mut err, l.b, e);
        }
    }
    // Their places along the shafts: as drawn in 3D (each group at its first gear's), or a
    // layer on for a group on a shaft shared with another.
    let mut layer: Vec<Option<i32>> = vec![None; n];
    if !three_d {
        for start in 0..n {
            if layer[group[start]].is_some() {
                continue;
            }
            layer[group[start]] = Some(0);
            let mut queue = vec![group[start]];
            while let Some(g) = queue.pop() {
                let here = layer[g].unwrap_or(0);
                for l in links.iter().filter(|l| l.kind == LinkKind::Compound) {
                    let (ga, gb) = (group[l.a], group[l.b]);
                    let other = if ga == g && gb != g {
                        gb
                    } else if gb == g && ga != g {
                        ga
                    } else {
                        continue;
                    };
                    match layer[other] {
                        None => {
                            layer[other] = Some(here + 1);
                            queue.push(other);
                        }
                        Some(v) if v == here => {
                            let e = format!(
                                "gears {} and {} share a shaft and would sit on the same layer: their groups are tied in a loop of shared shafts",
                                l.a + 1,
                                l.b + 1
                            );
                            fail(&mut err, l.a, e.clone());
                            fail(&mut err, l.b, e);
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    let layer_of = |i: usize| layer[group[i]].unwrap_or(0);
    for (i, j) in crossing {
        if three_d || layer_of(i) == layer_of(j) {
            let (a, b) = (c[i], c[j]);
            let e = format!(
                "gears {} and {}: their pitch circles cross (centres {} mm apart, radii {} and {} mm): make them touch to mesh, or move them apart",
                i + 1,
                j + 1,
                num(a.c.dist(b.c)),
                num(a.r),
                num(b.r)
            );
            fail(&mut err, i, e.clone());
            fail(&mut err, j, e);
        }
    }
    // A gear touching nothing touches no other gear.
    if n > 1 {
        for i in 0..n {
            if err[i].is_some() || links.iter().any(|l| l.a == i || l.b == i) {
                continue;
            }
            let gap = |j: usize| (c[i].c.dist(c[j].c) - c[i].r - c[j].r).abs();
            let e = match (0..n).filter(|j| *j != i).min_by(|a, b| gap(*a).total_cmp(&gap(*b))) {
                Some(j) => format!(
                    "gear {} touches no other gear: make it tangent to the gear it meshes with (Tangent constraint); it is {} mm from gear {}'s circle",
                    i + 1,
                    num(gap(j)),
                    j + 1
                ),
                None => format!("gear {} touches no other gear", i + 1),
            };
            fail(&mut err, i, e);
        }
    }
    // The helix: its size from the settings, its hand alternating across outside meshes.
    let helix = match style {
        Kind::Spur => 0.0,
        _ => settings.helix.filter(|h| *h != 0.0).map(f64::abs).unwrap_or(if base.helix != 0.0 { base.helix.abs() } else { 20.0 }),
    };
    if !(helix < 80.0) {
        whole(&mut out, "helix angle must be under 80°".into());
        out.errors = err;
        return out;
    }
    let first_hand = if settings.left_hand == Some(true) { -1.0 } else { 1.0 };
    let mut hand = vec![0.0f64; n];
    for start in 0..n {
        if hand[start] != 0.0 {
            continue;
        }
        hand[start] = first_hand;
        let mut queue = vec![start];
        while let Some(i) = queue.pop() {
            for l in &links {
                let j = if l.a == i {
                    l.b
                } else if l.b == i {
                    l.a
                } else {
                    continue;
                };
                if hand[j] == 0.0 {
                    hand[j] = if l.kind == LinkKind::External { -hand[i] } else { hand[i] };
                    queue.push(j);
                }
            }
        }
    }
    // Each meshing group's helix and V teeth: what its members ask (they must agree), else the
    // set's (a helix asked on spur teeth makes them helical). A cluster's gears may differ.
    let mut gh = vec![helix; n];
    let mut gv = vec![style == Kind::Herringbone; n];
    for g in (0..n).filter(|i| group[*i] == *i).collect::<Vec<_>>() {
        let members: Vec<usize> = (0..n).filter(|i| group[*i] == g).collect();
        let mut asked: Option<(usize, f64)> = None;
        let mut vee: Option<(usize, bool)> = None;
        for &i in &members {
            if let Some(h) = circles[i].helix {
                let h = h.abs();
                if !(h.is_finite() && h < 80.0) {
                    fail(&mut err, i, format!("gear {}: helix angle must be under 80°", i + 1));
                    continue;
                }
                match asked {
                    None => asked = Some((i, h)),
                    Some((j, hj)) if (h - hj).abs() > 1e-9 => fail(
                        &mut err,
                        i,
                        format!(
                            "gear {} asks a {}° helix, but gear {} it meshes with asks {}°: meshing gears share one helix angle (of opposite hands)",
                            i + 1,
                            num(h),
                            j + 1,
                            num(hj)
                        ),
                    ),
                    Some(_) => {}
                }
            }
            if let Some(v) = circles[i].herringbone {
                match vee {
                    None => vee = Some((i, v)),
                    Some((j, vj)) if v != vj => {
                        fail(&mut err, i, format!("gear {} and gear {} mesh: both have V (herringbone) teeth, or neither", i + 1, j + 1))
                    }
                    Some(_) => {}
                }
            }
        }
        for &i in &members {
            if let Some((_, h)) = asked {
                gh[i] = h;
            }
            if let Some((_, v)) = vee {
                gv[i] = v;
            }
            if gv[i] && gh[i] == 0.0 {
                fail(&mut err, i, format!("gear {}: V (herringbone) teeth need a helix angle", i + 1));
            }
        }
    }
    // One module per meshing group (decision 1): the module its members ask (a module block's
    // reaches here as each member's own; they must agree), else the one the first gear asking
    // teeth gives (Size by Teeth: drawn Ø ÷ teeth, standard or not), else `settings.module`,
    // else the automatic one ([`auto_module`]). Each gear takes the teeth it asks, else the
    // whole count nearest its drawn diameter; the drawn centre distances are then held by
    // profile shift (below), so the drawn circles are the operating pitch circles.
    let fs: Vec<f64> = gh.iter().map(|h| 1.0 / h.to_radians().cos()).collect();
    let mut module = vec![0.0f64; n];
    let mut teeth = vec![0u32; n];
    let roots: Vec<usize> = (0..n).filter(|i| group[*i] == *i).collect();
    let group_names = |members: &[usize]| -> String {
        let names: Vec<String> = members.iter().map(|i| (i + 1).to_string()).collect();
        format!("gear{} {}", if names.len() > 1 { "s" } else { "" }, and_list(&names))
    };
    for &g in &roots {
        let members: Vec<usize> = (0..n).filter(|i| group[*i] == g).collect();
        if members.iter().any(|i| err[*i].is_some() && c[*i].r <= 0.0) {
            continue;
        }
        let d = |i: usize| 2.0 * c[i].r;
        let f = fs[g];
        let mut asked: Option<(usize, f64)> = None;
        for &i in &members {
            let Some(m) = circles[i].module.filter(|m| *m > 0.0 && m.is_finite()) else { continue };
            match asked {
                None => asked = Some((i, m)),
                Some((j, mj)) if (m - mj).abs() > 1e-9 * mj => fail(
                    &mut err,
                    i,
                    format!(
                        "gear {} asks module {}, but gear {} it meshes with is module {}: meshing gears share one module",
                        i + 1,
                        num(m),
                        j + 1,
                        num(mj)
                    ),
                ),
                Some(_) => {}
            }
        }
        let by_teeth = members.iter().find_map(|&i| circles[i].teeth.filter(|z| *z > 0).map(|z| d(i) / (f * f64::from(z))));
        let fixed = asked.map(|a| a.1).or(by_teeth).or(settings.module.filter(|m| *m > 0.0 && m.is_finite()));
        let nearest = |m: f64| -> Vec<u32> {
            members.iter().map(|&i| circles[i].teeth.filter(|z| *z > 0).unwrap_or_else(|| (d(i) / (m * f)).round().clamp(1.0, 1e6) as u32)).collect()
        };
        let (m, zs) = match fixed {
            Some(m) => (m, nearest(m)),
            None => {
                let ds: Vec<(f64, f64)> = members.iter().map(|i| (d(*i), f)).collect();
                let (m, _, note) = auto_module(&ds, 8);
                let zs = nearest(m);
                if let Some(note) = note {
                    notes.push(format!("{}: {note}", group_names(&members)));
                }
                let (sd, sz): (f64, f64) = ds.iter().zip(&zs).fold((0.0, 0.0), |(a, b), ((d, f), z)| (a + d, b + f * f64::from(*z)));
                if settings.exact && sz > 0.0 {
                    let m = sd / sz;
                    notes.push(format!("{}: module {} (not a standard one) gives the drawn diameters", group_names(&members), num(m)));
                    (m, zs)
                } else {
                    (m, zs)
                }
            }
        };
        for (&i, z) in members.iter().zip(zs) {
            module[i] = m;
            teeth[i] = z;
        }
    }
    for i in 0..n {
        if err[i].is_none() && teeth[i] < 3 {
            let d = 2.0 * c[i].r;
            let most = (d / 3.0 * 10.0).floor() / 10.0;
            let fix = if most > 0.0 { format!("draw it bigger or use a smaller module (≤{})", num(most)) } else { "draw it bigger".to_string() };
            fail(
                &mut err,
                i,
                format!(
                    "gear {}: a Ø{} mm pitch circle gives {} {} at module {}, and a gear needs 3 or more; {fix}",
                    i + 1,
                    num(d),
                    teeth[i],
                    if teeth[i] == 1 { "tooth" } else { "teeth" },
                    num(module[i])
                ),
            );
        }
    }
    // The shifts that keep every drawn centre distance, per meshing group ([`group_shifts`]):
    // refused only where they leave the teeth's limits, naming sizes that would work.
    let face = settings.face_width.filter(|f| *f > 0.0).unwrap_or(10.0 * module.iter().copied().fold(0.0, f64::max));
    let probe = Spec { kind: style, helix, face_width: face, train: None, ..base.clone() };
    let inv0 = |i: usize, m: f64, z: u32| Spec { module: m.max(1e-6), helix: gh[i], ..probe.clone() }.involute(z.max(3), 0.0, hand[i] * gh[i]);
    let mut shift: Vec<Option<f64>> = vec![None; n];
    for &g in &roots {
        let members: Vec<usize> = (0..n).filter(|i| group[*i] == g).collect();
        if members.iter().any(|i| err[*i].is_some()) {
            continue;
        }
        let local = |i: usize| members.iter().position(|j| *j == i).unwrap_or(0);
        let meshes: Vec<GroupMesh> = links
            .iter()
            .filter(|l| l.kind != LinkKind::Compound && group[l.a] == g)
            .map(|l| GroupMesh { a: local(l.a), b: local(l.b), internal: l.kind == LinkKind::Internal, dist: c[l.a].c.dist(c[l.b].c) })
            .collect();
        let rings: Vec<bool> = members.iter().map(|i| ring[*i]).collect();
        let solve = |m: f64, zs: &[u32]| {
            let gs: Vec<crate::involute::Involute> = members.iter().zip(zs).map(|(&i, &z)| inv0(i, m, z)).collect();
            group_shifts(&gs, &rings, &meshes)
        };
        let zs: Vec<u32> = members.iter().map(|i| teeth[*i]).collect();
        let m = module[g];
        match solve(m, &zs) {
            Ok(xs) => {
                for (&i, x) in members.iter().zip(xs) {
                    shift[i] = Some(x);
                }
            }
            Err(why) => {
                let ds: Vec<(f64, f64)> = members.iter().map(|&i| (2.0 * c[i].r, fs[i])).collect();
                let name = |k: usize| format!("gear {}", members[k] + 1);
                let ideas = other_sizes(&ds, m, &zs, &name, &|m, zs| solve(m, zs).is_ok());
                let mut who: Vec<usize> = match why {
                    ShiftFail::Unreachable(a, b) => vec![members[a], members[b]],
                    ShiftFail::Limits { low, high } => vec![members[low.0], members[high.0]],
                };
                who.sort_unstable();
                who.dedup();
                let e = format!("{}: {}", group_names(&who), shift_refusal(why, &name, &zs, m, "the drawn centre distances", &ideas));
                for i in who {
                    fail(&mut err, i, e.clone());
                }
            }
        }
    }
    for i in 0..n {
        let built = module[i] * fs[i] * f64::from(teeth[i]);
        if err[i].is_none() && (built - 2.0 * c[i].r).abs() > 1e-6 {
            notes.push(format!(
                "gear {}: Ø{} drawn, Ø{} built ({} teeth at module {}); profile shift {:+.3} holds the drawn centre distances",
                i + 1,
                num(2.0 * c[i].r),
                num(built),
                teeth[i],
                num(module[i]),
                shift[i].unwrap_or(0.0)
            ));
        }
    }
    out.shifts = shift.iter().map(|x| x.unwrap_or(0.0)).collect();
    // Where each gear sits along the shafts. Worked out, a face width fits between the gears
    // drawn on one shaft (nine tenths of the nearest's spacing).
    let spacing = |i: usize| -> Option<f64> {
        let zi = circles[i].z?;
        links
            .iter()
            .filter(|l| l.kind == LinkKind::Compound && (l.a == i || l.b == i))
            .filter_map(|l| circles[if l.a == i { l.b } else { l.a }].z.map(|zj| (zj - zi).abs()))
            .filter(|d| *d > 1e-9)
            .reduce(f64::min)
    };
    let auto = |i: usize| -> f64 {
        if settings.face_width.is_some_and(|f| f > 0.0) {
            return face;
        }
        // A whole meshing group alike: the tightest of its gears' spacings.
        let tight = (0..n).filter(|j| group[*j] == group[i]).filter_map(spacing).reduce(f64::min);
        match tight {
            Some(d) if 0.9 * d < face => 0.9 * d,
            _ => face,
        }
    };
    // A custom split of the face fixes its width; the presets take the width worked out here.
    let faces: Vec<f64> = (0..n)
        .map(|i| circles[i].sides.and_then(SidePreset::width).or(circles[i].face_width).filter(|f| *f > 0.0).unwrap_or_else(|| auto(i)))
        .collect();
    // Each face's reach (side A along +z, side B along −z) from its sketch plane; nothing asked:
    // all on side A, as before sides existed.
    let sides: Vec<(f64, f64)> = (0..n).map(|i| circles[i].sides.unwrap_or(SidePreset::AOnly).sides(faces[i])).collect();
    // Layers of a flat drawing far enough apart for any gear's side A below any one's side B.
    let reach = sides.iter().fold((0.0f64, 0.0f64), |m, s| (m.0.max(s.0), m.1.max(s.1)));
    let step = faces.iter().copied().fold(face, f64::max).max(reach.0 + reach.1) * 1.1 + 1.0;
    let z_of = |i: usize| match circles[group[i]].z {
        Some(z) => z,
        None => f64::from(layer_of(i)) * step,
    };
    // Clusters: gears on one shaft (compound links), each with two or more.
    let mut cluster: Vec<Option<usize>> = vec![None; n];
    for start in 0..n {
        if cluster[start].is_some() || !links.iter().any(|l| l.kind == LinkKind::Compound && (l.a == start || l.b == start)) {
            continue;
        }
        let id = out.clusters.len();
        let mut list = vec![start];
        cluster[start] = Some(id);
        let mut k = 0;
        while k < list.len() {
            let i = list[k];
            for l in links.iter().filter(|l| l.kind == LinkKind::Compound) {
                let j = if l.a == i {
                    l.b
                } else if l.b == i {
                    l.a
                } else {
                    continue;
                };
                if cluster[j].is_none() {
                    cluster[j] = Some(id);
                    list.push(j);
                }
            }
            k += 1;
        }
        list.sort_unstable();
        out.clusters.push(list);
    }
    out.members = (0..n)
        .map(|i| SetMember {
            source: i,
            module: module[i],
            teeth: teeth[i],
            pitch_diameter: module[i] * fs[i] * f64::from(teeth[i]),
            face_width: faces[i],
            face_a: sides[i].0,
            face_b: sides[i].1,
            ring: ring[i],
            meshes_with: links
                .iter()
                .filter(|l| l.kind != LinkKind::Compound)
                .filter_map(|l| {
                    if l.a == i {
                        Some(l.b)
                    } else if l.b == i {
                        Some(l.a)
                    } else {
                        None
                    }
                })
                .collect(),
            cluster: cluster[i],
            body: None,
        })
        .collect();
    out.notes = notes;
    if err.iter().any(Option::is_some) {
        let mut seen = Vec::new();
        for e in err.iter().flatten() {
            if !seen.contains(e) {
                seen.push(e.clone());
            }
        }
        out.error = Some(seen.join("; "));
        out.errors = err;
        return out;
    }
    out.errors = err;
    // Shafts: one per axis that carries a gear asking for one (a cluster's gears share it).
    let mut shafts = Vec::new();
    let mut done = vec![false; n];
    for i in 0..n {
        if done[i] || ring[i] || !circles[i].shaft {
            continue;
        }
        let on: Vec<usize> = match cluster[i] {
            Some(k) => out.clusters[k].iter().copied().filter(|j| !ring[*j]).collect(),
            None => vec![i],
        };
        for &j in &on {
            done[j] = true;
        }
        let root = on.iter().map(|&j| module[j] * fs[j] * f64::from(teeth[j]) - 2.5 * module[j]).fold(f64::INFINITY, f64::min);
        let d = on
            .iter()
            .find_map(|j| circles[*j].shaft_diameter.filter(|d| *d > 0.0))
            .or((base.bore.diameter > 0.0).then_some(base.bore.diameter))
            .unwrap_or_else(|| (root.floor() / 2.0).max(1.0));
        let (lo, hi) = on
            .iter()
            .map(|&j| (z_of(j) - sides[j].1, z_of(j) + sides[j].0))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |a, b| (a.0.min(b.0), a.1.max(b.1)));
        let ext = (0.25 * d).max(2.0);
        let (from, to) = on
            .iter()
            .filter_map(|j| circles[*j].shaft_span)
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
            .map(|(a, b)| (a.min(b), a.max(b)))
            .unwrap_or((lo - ext, hi + ext));
        if to - from > 1e-6 {
            shafts.push(crate::plan::TrainShaft { center: c[i].c, from, to, diameter: d, gears: on });
        }
    }
    let gears: Vec<TrainGear> = (0..n)
        .map(|i| TrainGear {
            center: c[i].c,
            teeth: teeth[i],
            module: module[i],
            shift: shift[i].unwrap_or(0.0),
            helix: hand[i] * gh[i],
            herringbone: gv[i],
            ring: ring[i],
            z: z_of(i),
            face: faces[i],
            face_a: sides[i].0,
            face_b: sides[i].1,
            source: i,
        })
        .collect();
    out.spec = Some(Spec {
        kind: style,
        module: module[0],
        teeth: teeth[0],
        teeth2: teeth.get(1).copied().unwrap_or(teeth[0]),
        helix,
        shift: 0.0,
        shift2: 0.0,
        center_distance: 0.0,
        face_width: face,
        herringbone: style == Kind::Herringbone,
        coupon: false,
        train: Some(Train { gears, links, shafts }),
        ..base.clone()
    });
    out
}

/// Sizes the train drawn as `circles` with nothing asked of each gear ([`size_train`]): the
/// spec and notes, or why it can't be made (every gear's problem, each naming its gear).
pub fn train_spec(circles: &[Circle], style: Kind, settings: &crate::simple::Simple, base: &Spec) -> Result<(Spec, Vec<String>), String> {
    let sized = size_train(&circles.iter().map(|c| TrainCircle::of(*c)).collect::<Vec<_>>(), style, settings, base);
    match sized.spec {
        Some(spec) => Ok((spec, sized.notes)),
        None => Err(sized.error.unwrap_or_else(|| "the circles make no gear train".into())),
    }
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul3(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// The shafts of the measured set in the world (`frame`: the sketch's plane, its z the normal)
/// for [`crate::axes::solve`] with `spec` ([`spec_for`]'s): gear 1's (the hypoid's pinion's)
/// and gear 2's where the set has one. Circle modes turn about the circles' normals; a bevel's
/// shafts lie along its side view's lines; skew shafts lie in the sketch's plane, tilted out of
/// it by the shaft angle setting. Then the whole set is turned about the origin dot by `turn`
/// (degrees about the sketch's x, then y, then its normal), to stand in the plane wanted.
pub fn axes_for(m: &Measured, spec: &Spec, frame: &Frame, turn: [f64; 3]) -> (Axis, Option<Axis>) {
    let (a1, a2) = sketch_axes(m, spec);
    let o = [m.origin.x, m.origin.y, 0.0];
    let r = |v: [f64; 3]| rotate(v, turn);
    let place = |a: Axis| {
        let p = add3(r(sub3(a.origin, o)), o);
        Axis::new(frame.world(p), frame.dir(r(a.dir))).unwrap_or(a)
    };
    (place(a1), a2.map(place))
}

/// Where a rack drawn as a pinion circle and its pitch line goes: its pitch line on the drawn
/// line, from the line's first end along it, its teeth towards the pinion (in the world, turned
/// as [`axes_for`] turns the shafts). None for anything else.
pub fn rack_frame(m: &Measured, frame: &Frame, turn: [f64; 3]) -> Option<Frame> {
    if m.kind != Kind::Rack {
        return None;
    }
    let (l, c) = (m.line?, m.c1?);
    let x = l.dir();
    let foot = l.foot(c.c);
    let mut y = V2::new(-x.y, x.x);
    if (c.c - foot).dot(y) < 0.0 {
        y = y * -1.0;
    }
    let z = [0.0, 0.0, x.x * y.y - x.y * y.x];
    let o = [m.origin.x, m.origin.y, 0.0];
    let r = |v: [f64; 3]| rotate(v, turn);
    let origin = frame.world(add3(r(sub3([l.a.x, l.a.y, 0.0], o)), o));
    Some(Frame { origin, x: frame.dir(r([x.x, x.y, 0.0])), z: frame.dir(r(z)) })
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `v` turned `t[0]` degrees about x, then `t[1]` about y, then `t[2]` about z.
pub fn rotate(v: [f64; 3], t: [f64; 3]) -> [f64; 3] {
    let (sx, cx) = t[0].to_radians().sin_cos();
    let (sy, cy) = t[1].to_radians().sin_cos();
    let (sz, cz) = t[2].to_radians().sin_cos();
    let v = [v[0], cx * v[1] - sx * v[2], sx * v[1] + cx * v[2]];
    let v = [cy * v[0] + sy * v[2], v[1], -sy * v[0] + cy * v[2]];
    [cz * v[0] - sz * v[1], sz * v[0] + cz * v[1], v[2]]
}

/// [`axes_for`] in the sketch's own coordinates, before any turn.
fn sketch_axes(m: &Measured, spec: &Spec) -> (Axis, Option<Axis>) {
    let n = [0.0, 0.0, 1.0];
    let at = |p: V2, h: f64| [p.x, p.y, h];
    let along = |v: V2| [v.x, v.y, 0.0];
    let axis = |o: [f64; 3], d: [f64; 3]| Axis::new(o, d).unwrap_or(Axis { origin: o, dir: n });
    let c1 = m.c1.map(|c| c.c).unwrap_or(V2::ZERO);
    let sigma = spec.shaft_angle.to_radians();
    let gear1 = axis(at(c1, 0.0), n);
    let x_line = Line { a: c1, b: c1 + V2::new(1.0, 0.0) };
    match m.shape {
        Shape::Pair => (gear1, m.c2.map(|c| axis(at(c.c, 0.0), n))),
        Shape::ThroughCentre => {
            // A face gear: the pinion's axis crosses the face gear's a pinion pitch radius above
            // its pitch plane.
            let l = m.line.unwrap_or(x_line);
            let rp = spec.module * f64::from(spec.teeth) / 2.0;
            let u = along(ray(&l, c1));
            (gear1, Some(axis(add3(at(c1, rp), mul3(u, spec.module * f64::from(spec.teeth2) / 2.0)), u)))
        }
        Shape::SideView => {
            // Each shaft from the apex out along its line: the gear sits on that ray.
            let apex = m.apex.unwrap_or(m.origin);
            let (l1, l2) = (m.line2.unwrap_or(x_line), m.line.unwrap_or(x_line));
            let (r1, r2) = (ray(&l1, apex), ray(&l2, apex));
            let reach = l1.len().max(l2.len()).max(1.0);
            (axis(at(apex + r1 * reach, 0.0), along(r1)), Some(axis(at(apex + r2 * reach, 0.0), along(r2))))
        }
        Shape::Tangent if m.kind == Kind::Worm => {
            let l = m.line.unwrap_or(x_line);
            let foot = l.foot(c1);
            let out = if foot.dist(c1) > 1e-9 { (foot - c1) * (1.0 / foot.dist(c1)) } else { V2::new(-l.dir().y, l.dir().x) };
            let r1 = spec.module * f64::from(spec.teeth) / 2.0;
            let dw = if spec.worm_diameter > 0.0 { spec.worm_diameter } else { 10.0 * spec.module };
            (gear1, Some(axis(at(c1 + out * (r1 + dw / 2.0), 0.0), along(l.dir()))))
        }
        Shape::Crossing | Shape::Missing => {
            let l = m.line.unwrap_or(x_line);
            let shaft = axis(at(l.foot(c1), 0.0), add3(mul3(along(l.dir()), sigma.sin()), mul3(n, sigma.cos())));
            if m.kind == Kind::Hypoid {
                // The pinion turns about axis 1, the ring gear (the circle) about axis 2.
                (shaft, Some(gear1))
            } else {
                (gear1, Some(shaft))
            }
        }
        _ => (gear1, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axes::{self, Drive, Relation};

    fn circle(x: f64, y: f64, r: f64) -> Circle {
        Circle { c: V2::new(x, y), r }
    }
    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Line {
        Line { a: V2::new(ax, ay), b: V2::new(bx, by) }
    }
    fn drawn(circles: Vec<Circle>, lines: Vec<Line>) -> Drawn {
        Drawn { points: vec![], circles, lines }
    }
    fn measured(k: Kind, d: &Drawn) -> Result<Measured, String> {
        measure(k, d, &auto_picks(k, d))
    }
    /// Measures, sizes and solves the set on its shafts, and plans it.
    fn solved(k: Kind, d: &Drawn, base: Spec) -> (Measured, axes::Solved) {
        let m = measured(k, d).unwrap();
        let (spec, _) = spec_for(&m, &base).unwrap();
        let (a1, a2) = axes_for(&m, &spec, &Frame::at([0.0; 3]), [0.0; 3]);
        let s = axes::solve(&a1, a2.as_ref(), Some(k), &spec, &Drive::default()).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        crate::plan(&s.spec).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        (m, s)
    }

    #[test]
    fn one_circle_is_a_single_gear() {
        let d = drawn(vec![circle(0.0, 0.0, 20.0)], vec![]);
        assert_eq!(recognize(&d), Some(Kind::Spur));
        let (_, s) = solved(Kind::Spur, &d, Spec::defaults_for(Kind::Spur));
        assert!((s.spec.module * f64::from(s.spec.teeth) - 40.0).abs() < 0.2, "{} × {}", s.spec.module, s.spec.teeth);
    }

    #[test]
    fn touching_circles_are_a_pair() {
        let d = drawn(vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0)], vec![]);
        assert_eq!(recognize(&d), Some(Kind::Pair));
        let (m, s) = solved(Kind::Pair, &d, Spec::defaults_for(Kind::Pair));
        assert!((m.center_distance - 50.0).abs() < 1e-9);
        assert_eq!(f64::from(s.spec.teeth2) / f64::from(s.spec.teeth), 1.5);
        assert!(matches!(s.relation, Some(Relation::Parallel { distance, .. }) if (distance - 50.0).abs() < 1e-9));
    }

    #[test]
    fn apart_or_overlapping_circles_are_refused_with_why() {
        let d = drawn(vec![circle(0.0, 0.0, 20.0), circle(60.0, 0.0, 30.0)], vec![]);
        let e = measured(Kind::Pair, &d).unwrap_err();
        assert!(e.contains("must touch outside") && e.contains("apart"), "{e}");
        let d = drawn(vec![circle(0.0, 0.0, 20.0), circle(40.0, 0.0, 30.0)], vec![]);
        assert!(measured(Kind::Pair, &d).unwrap_err().contains("overlap"));
    }

    #[test]
    fn tolerance_takes_a_rough_drawing_but_not_a_wrong_one() {
        // 0.3 % off the touching distance: taken; 3 % off: refused.
        let d = drawn(vec![circle(0.0, 0.0, 20.0), circle(50.06, 0.0, 30.0)], vec![]);
        assert!(measured(Kind::Pair, &d).is_ok());
        let d = drawn(vec![circle(0.0, 0.0, 20.0), circle(50.6, 0.0, 30.0)], vec![]);
        assert!(measured(Kind::Pair, &d).is_err());
    }

    #[test]
    fn circle_inside_a_bigger_one_is_internal() {
        let d = drawn(vec![circle(0.0, 0.0, 60.0), circle(40.0, 0.0, 20.0)], vec![]);
        assert_eq!(recognize(&d), Some(Kind::Internal));
        let (m, _) = solved(Kind::Internal, &d, Spec::defaults_for(Kind::Internal));
        assert_eq!((m.d1, m.d2), (120.0, 40.0));
    }

    #[test]
    fn sun_planet_ring_are_planetary() {
        let d = drawn(vec![circle(0.0, 0.0, 18.0), circle(36.0, 0.0, 18.0), circle(0.0, 0.0, 54.0)], vec![]);
        assert_eq!(recognize(&d), Some(Kind::Planetary));
        let (_, s) = solved(Kind::Planetary, &d, Spec::defaults_for(Kind::Planetary));
        assert_eq!(s.spec.teeth3, s.spec.teeth + 2 * s.spec.teeth2);
        let bad = drawn(vec![circle(0.0, 0.0, 18.0), circle(36.0, 0.0, 18.0), circle(0.0, 0.0, 60.0)], vec![]);
        assert!(measured(Kind::Planetary, &bad).unwrap_err().contains("ring Ø should be"));
    }

    #[test]
    fn tangent_line_is_a_worm_or_a_rack() {
        let d = drawn(vec![circle(0.0, 0.0, 30.0)], vec![line(-40.0, 30.0, 40.0, 30.0)]);
        assert_eq!(recognize(&d), Some(Kind::Worm));
        let (_, s) = solved(Kind::Worm, &d, Spec::defaults_for(Kind::Worm));
        assert!(matches!(s.relation, Some(Relation::Skew { angle, .. }) if (angle - 90.0).abs() < 1e-6));
        let m = measured(Kind::Rack, &d).unwrap();
        let (spec, _) = spec_for(&m, &Spec::defaults_for(Kind::Rack)).unwrap();
        assert!((spec.rack_teeth as f64 - 80.0 / (PI * spec.module)).abs() <= 0.5, "{}", spec.rack_teeth);
        // A second parallel line beyond gives the worm's diameter.
        let d = drawn(vec![circle(0.0, 0.0, 30.0)], vec![line(-40.0, 30.0, 40.0, 30.0), line(-40.0, 50.0, 40.0, 50.0)]);
        assert_eq!(measured(Kind::Worm, &d).unwrap().worm_diameter, 20.0);
    }

    /// A side view: gear 1's shaft down −y from the dot at the origin, gear 2's at `deg` from it.
    fn side_view(deg: f64) -> Drawn {
        let a = deg.to_radians();
        Drawn { points: vec![V2::ZERO], circles: vec![], lines: vec![line(0.0, 0.0, 0.0, -50.0), line(0.0, 0.0, 50.0 * a.sin(), -50.0 * a.cos())] }
    }

    #[test]
    fn two_lines_from_the_dot_are_a_bevel_at_their_angle() {
        let d = side_view(90.0);
        assert_eq!(recognize(&d), Some(Kind::Bevel));
        let (m, s) = solved(Kind::Bevel, &d, Spec::defaults_for(Kind::Bevel));
        assert_eq!(m.shape, Shape::SideView);
        assert!((s.spec.shaft_angle - 90.0).abs() < 1e-6, "{}", s.spec.shaft_angle);
        // Gear 1 on its shaft below the apex, its axis along the drawn line.
        let f = s.frames[0];
        assert!(f.origin[1] < 0.0 && f.origin[0].abs() < 1e-6, "{:?}", f.origin);
        assert!((f.z[1].abs() - 1.0).abs() < 1e-9, "{:?}", f.z);
    }

    #[test]
    fn bevel_planetary_side_view_places_sun_and_first_planet() {
        use crate::plan::BevelPlanetMode;
        for (deg, mode, planets) in [(90.0, BevelPlanetMode::Differential, 4), (60.0, BevelPlanetMode::Single, 3), (45.0, BevelPlanetMode::Ring, 3)] {
            let d = side_view(deg);
            let base = Spec { bevel_planet_mode: mode, planets, teeth: 20, teeth2: 10, ..Spec::defaults_for(Kind::BevelPlanetary) };
            let (m, s) = solved(Kind::BevelPlanetary, &d, base);
            assert_eq!(m.shape, Shape::SideView);
            let p = crate::plan(&s.spec).unwrap();
            // The sun on its drawn shaft below the apex (sketch -y), its z pointing back up at it.
            let (sun, planet) = (p.bodies[0].frame, p.bodies[1].frame);
            assert!(sun.origin[1] < 0.0 && sun.origin[0].abs() < 1e-6 && (sun.z[1] - 1.0).abs() < 1e-9, "{deg}: {sun:?}");
            assert!(
                close(sun.origin, s.frames[0].origin) && close(planet.origin, s.frames[1].origin),
                "{deg}: {:?} against {:?}",
                (sun, planet),
                s.frames
            );
            // Planet 1 on the drawn line (in the sketch's plane), at the built cone angle.
            let built = crate::plan::bevel_planet::geometry(&s.spec).unwrap().sigma;
            let u = [-planet.z[0], -planet.z[1], -planet.z[2]];
            assert!(u[2].abs() < 1e-9 && u[0] > 0.0, "{deg}: {u:?}");
            assert!((u[1] + built.cos()).abs() < 1e-9, "{deg}: {u:?} at {}", s.spec.shaft_angle);
        }
        let mut off = side_view(90.0);
        off.points[0] = V2::new(5.0, 0.0);
        assert!(measured(Kind::BevelPlanetary, &off).unwrap_err().contains("the sun's"));
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
    }

    #[test]
    fn bevel_at_80_degrees_drawn_and_solved() {
        let d = side_view(80.0);
        let m = measured(Kind::Bevel, &d).unwrap();
        assert!((m.shaft_angle.unwrap() - 80.0).abs() < 1e-9);
        let (_, s) = solved(Kind::Bevel, &d, Spec::defaults_for(Kind::Bevel));
        assert!((s.spec.shaft_angle - 80.0).abs() < 1e-6, "{}", s.spec.shaft_angle);
        assert!(matches!(s.relation, Some(Relation::Intersecting { .. })));
    }

    #[test]
    fn bevel_lines_must_meet_at_the_dot() {
        let mut d = side_view(90.0);
        d.points[0] = V2::new(5.0, 0.0);
        assert!(measured(Kind::Bevel, &d).unwrap_err().contains("origin dot"));
        let parallel = Drawn { points: vec![], circles: vec![], lines: vec![line(0.0, 0.0, 0.0, 50.0), line(10.0, 0.0, 10.0, 50.0)] };
        assert!(measured(Kind::Bevel, &parallel).unwrap_err().contains("parallel"));
    }

    #[test]
    fn turns_about_the_dot_keep_the_set() {
        // A pair turned 90° about the sketch's x through a dot at gear 1's centre: the shafts lie
        // along −y, still 50 mm apart.
        let d = Drawn { points: vec![V2::ZERO], circles: vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0)], lines: vec![] };
        let m = measured(Kind::Pair, &d).unwrap();
        let (spec, _) = spec_for(&m, &Spec::defaults_for(Kind::Pair)).unwrap();
        let (a1, a2) = axes_for(&m, &spec, &Frame::at([0.0; 3]), [90.0, 0.0, 0.0]);
        assert!((a1.dir[1] + 1.0).abs() < 1e-9, "{:?}", a1.dir);
        assert!(a1.origin.iter().all(|v| v.abs() < 1e-9));
        let a2 = a2.unwrap();
        assert!((a2.origin[0] - 50.0).abs() < 1e-9);
        assert!(matches!(axes::relate(&a1, &a2), Relation::Parallel { distance, .. } if (distance - 50.0).abs() < 1e-9));
    }
    #[test]
    fn off_centre_line_crossing_is_a_hypoid() {
        let d = drawn(vec![circle(0.0, 0.0, 100.0)], vec![line(-120.0, -25.0, 120.0, -25.0)]);
        assert_eq!(recognize(&d), Some(Kind::Hypoid));
        let (m, s) = solved(Kind::Hypoid, &d, Spec::defaults_for(Kind::Hypoid));
        assert!((m.offset - 25.0).abs() < 1e-9);
        assert!((s.spec.offset - 25.0).abs() < 1e-6, "{}", s.spec.offset);
        assert!((s.spec.module * f64::from(s.spec.teeth2) - 200.0).abs() < 1e-6);
        let through = drawn(vec![circle(0.0, 0.0, 100.0)], vec![line(-120.0, 0.0, 120.0, 0.0)]);
        assert!(measured(Kind::Hypoid, &through).unwrap_err().contains("spiral bevel"));
    }

    #[test]
    fn line_missing_the_circle_is_crossed_helical() {
        let d = drawn(vec![circle(0.0, 0.0, 20.0)], vec![line(-60.0, 45.0, 60.0, 45.0)]);
        assert_eq!(recognize(&d), Some(Kind::CrossedHelical));
        let (m, s) = solved(Kind::CrossedHelical, &d, Spec::defaults_for(Kind::CrossedHelical));
        assert!((m.d2 - 50.0).abs() < 1e-9);
        assert!(matches!(s.relation, Some(Relation::Skew { offset, .. }) if (offset - 45.0).abs() < 1e-6));
    }

    #[test]
    fn missing_picks_say_what_to_draw() {
        let d = drawn(vec![circle(0.0, 0.0, 20.0)], vec![]);
        let e = measured(Kind::Pair, &d).unwrap_err();
        assert!(e.starts_with("Gear 2 pitch circle"), "{e}");
        let e = measured(Kind::Hypoid, &d).unwrap_err();
        assert!(e.contains("Pinion shaft"), "{e}");
    }

    /// Through the Simple-mode solver, as the window and the engine size a drawn set.
    fn through_simple(k: Kind, d: &Drawn, settings: crate::simple::Simple) -> axes::Solved {
        let m = measured(k, d).unwrap();
        let sol = crate::simple::solve(&simple_of(&m, &settings), &Spec::defaults_for(k));
        assert!(sol.error.is_none(), "{k:?}: {:?}", sol.error);
        let (a1, a2) = axes_for(&m, &sol.spec, &Frame::at([0.0; 3]), [0.0; 3]);
        let s = axes::solve(&a1, a2.as_ref(), Some(k), &sol.spec, &Drive::default()).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        crate::plan(&s.spec).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        s
    }

    #[test]
    fn every_row_sizes_through_the_simple_solver() {
        use crate::simple::Simple;
        let none = Simple::default;
        let s = through_simple(Kind::Pair, &drawn(vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0)], vec![]), none());
        assert!(matches!(s.relation, Some(Relation::Parallel { distance, .. }) if (distance - 50.0).abs() < 1e-9));
        through_simple(Kind::Spur, &drawn(vec![circle(0.0, 0.0, 20.0)], vec![]), none());
        through_simple(Kind::Planetary, &drawn(vec![circle(0.0, 0.0, 18.0), circle(36.0, 0.0, 18.0), circle(0.0, 0.0, 54.0)], vec![]), none());
        through_simple(Kind::Worm, &drawn(vec![circle(0.0, 0.0, 30.0)], vec![line(-40.0, 30.0, 40.0, 30.0)]), none());
        let h = through_simple(
            Kind::Hypoid,
            &drawn(vec![circle(0.0, 0.0, 100.0)], vec![line(-120.0, -25.0, 120.0, -25.0)]),
            Simple { shaft_angle: Some(90.0), ..none() },
        );
        assert!((h.spec.offset - 25.0).abs() < 1e-6);
        through_simple(
            Kind::CrossedHelical,
            &drawn(vec![circle(0.0, 0.0, 20.0)], vec![line(-60.0, 45.0, 60.0, 45.0)]),
            Simple { shaft_angle: Some(90.0), ..none() },
        );
    }

    #[test]
    fn bevel_at_80_degrees_through_the_simple_solver() {
        let s = through_simple(
            Kind::Bevel,
            &side_view(80.0),
            crate::simple::Simple { ratio: Some(1.5), pitch_diameter: Some(40.0), ..Default::default() },
        );
        assert!((s.spec.shaft_angle - 80.0).abs() < 1e-6, "{}", s.spec.shaft_angle);
        assert!((s.spec.module * f64::from(s.spec.teeth) - 40.0).abs() < 0.5, "{} × {}", s.spec.module, s.spec.teeth);
    }

    #[test]
    fn fit_prefers_the_largest_standard_module() {
        let (m, z, std) = fit_module(&[(40.0, 1.0), (60.0, 1.0)], 8);
        assert!(std);
        assert_eq!((m, z), (5.0, vec![8, 12]));
        // Ø9.1 is 1.1 % off every standard module's nearest count of 8 or more teeth.
        let (m, z, std) = fit_module(&[(9.1, 1.0)], 8);
        assert!(!std);
        assert!((m * f64::from(z[0]) - 9.1).abs() < 1e-9);
    }
}
