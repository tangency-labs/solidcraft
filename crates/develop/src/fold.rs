//! Folding a developed sheet: where each of its panels and bends lies in space, and where each
//! point of its flat pattern goes on the folded sheet, at any depth through its thickness.
//!
//! A panel's surface has a frame of its own. A plane is the frame's `z = 0`. A cylinder's or a
//! cone's axis is the frame's `z` axis, a cone's apex at its origin, and the angle round it runs
//! from `x` toward `y` where the material is inward and toward `-y` where it is outward, so that
//! `u x v` points to the material either way. A bend's metal is the ring sector round its axis
//! between its inside and outside radii, and its flat zone wraps onto it by its neutral arc.

use std::collections::VecDeque;

use solidcraft_geom::{Vec2, Vec3};

use crate::chart::Flatten;
use crate::{Anchor, Bend, DevelopError, Development, JointKind, Mismatch, Rigid2, Rigid3, Sheet, Surface};

/// A developed sheet folded.
#[derive(Clone, Debug, PartialEq)]
pub struct Fold {
    /// Where each panel's frame lies in space.
    pub places: Vec<Option<Rigid3>>,
    /// Where each joint's bend lies in space (none for a seam or a rip).
    pub bends: Vec<Option<Bent>>,
    /// Bends and seams that close a ring, where the fold reached both panels another way: how
    /// far the joint itself would put its `b` panel from where the ring put it.
    pub loops: Vec<Mismatch>,
    surfaces: Vec<Surface>,
    maps: Vec<Flatten>,
    /// From the flat frame back to each panel's neutral chart.
    unplace: Vec<Option<Rigid2>>,
    /// The angle halfway round each roll's region: a cone's flat sector is undone nearest it.
    middle: Vec<f64>,
}

/// A bend in space, and its flat zone wrapped onto it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bent {
    /// The `a` anchor's frame in space: `x` along the joint, `y` toward the material and `z`
    /// toward the `b` panel.
    frame: Rigid3,
    wrap: Wrap,
    angle: f64,
    /// The joint in the flat, from the `a` panel's side: its anchor's origin and direction.
    flat: (Vec2, Vec2),
}

/// A bend's metal in its `a` anchor's frame (`x` along the joint, `y` toward the material, `z`
/// toward the `b` panel).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wrap {
    /// 1 where the bend turns toward the material, so the sheet runs round its outside; -1 where
    /// it turns away, so the sheet runs round its inside.
    sign: f64,
    /// The sheet's radius round the axis.
    sheet: f64,
    setback: f64,
    /// The neutral arc's radius, which the flat zone's width wraps onto.
    neutral: f64,
}

impl Wrap {
    pub(crate) fn of(bend: &Bend, thickness: f64, k_factor: f64) -> Wrap {
        let toward = bend.toward_material;
        Wrap {
            sign: if toward { 1.0 } else { -1.0 },
            sheet: if toward { bend.radius + thickness } else { bend.radius },
            setback: bend.setback(thickness),
            neutral: bend.radius + k_factor * thickness,
        }
    }

    /// The point `along` the joint, turned `phi` round the bend from the `a` panel's tangent
    /// line, `depth` into the material. The axis lies the sheet's radius from the tangent line,
    /// on the material's side where the bend turns toward it.
    pub(crate) fn at(&self, along: f64, phi: f64, depth: f64) -> Vec3 {
        let rho = self.sheet - self.sign * depth;
        Vec3::new(along, self.sign * (self.sheet - rho * phi.cos()), rho * phi.sin() - self.setback)
    }
}

impl Bent {
    /// Where a point of the bend's flat zone goes, `depth` into the material.
    pub fn point(&self, p: Vec2, depth: f64) -> Vec3 {
        let (along, phi) = self.wrapped(p);
        self.at(along, phi, depth)
    }

    /// The point `along` the joint, turned `phi` round the bend from the `a` panel's tangent
    /// line, `depth` into the material.
    pub fn at(&self, along: f64, phi: f64, depth: f64) -> Vec3 {
        self.frame.point(self.wrap.at(along, phi, depth))
    }

    /// A point of the flat zone as its length along the joint and the angle it lies round the
    /// bend from the `a` panel's tangent line.
    pub fn wrapped(&self, p: Vec2) -> (f64, f64) {
        let (o, d) = self.flat;
        let q = p - o;
        let across = q.dot(Vec2::new(d.y, -d.x)) + self.wrap.setback;
        (q.dot(d), if self.wrap.neutral > 0.0 { across / self.wrap.neutral } else { 0.0 })
    }

    /// Where a point of space lies in the bend: the point of the flat zone whose fiber runs
    /// through it, how deep it lies into the material, and the way deeper (a unit vector). The
    /// inverse of [`Bent::point`]; `None` on the bend's axis.
    pub fn unwrapped(&self, p: Vec3) -> Option<(Vec2, f64, Vec3)> {
        let q = self.frame.inverse().point(p);
        let w = &self.wrap;
        // In the frame, a point turned `phi` at radius `rho` lies at (along, sign (sheet -
        // rho cos phi), rho sin phi - setback); deeper, `rho` falls (sign 1) or grows.
        let (c, s) = (w.sheet - w.sign * q.y, q.z + w.setback);
        let rho = c.hypot(s);
        if !(rho > 0.0) {
            return None;
        }
        let phi = s.atan2(c);
        let (o, d) = self.flat;
        let flat = o + d * q.x + Vec2::new(d.y, -d.x) * (phi * w.neutral - w.setback);
        let deeper = self.frame.vector(Vec3::new(0.0, phi.cos(), -w.sign * phi.sin()));
        Some((flat, (w.sheet - rho) * w.sign, deeper))
    }

    /// A point of the bend's axis and its direction, round which the sheet turns positively
    /// (right-handed) from the `a` panel to the `b` one.
    pub fn axis(&self) -> (Vec3, Vec3) {
        let w = &self.wrap;
        (self.frame.point(Vec3::new(0.0, w.sign * w.sheet, -w.setback)), self.frame.x * -w.sign)
    }

    /// How far the bend turns, in radians.
    pub fn angle(&self) -> f64 {
        self.angle
    }
}

impl Fold {
    /// Where a point of panel `i`'s flat material goes, `depth` into the material.
    pub fn panel_point(&self, i: usize, p: Vec2, depth: f64) -> Option<Vec3> {
        self.surface_point(i, self.param(i, p)?, depth)
    }

    /// The point of panel `i`'s surface at the parameters `q`, `depth` into the material.
    pub fn surface_point(&self, i: usize, q: Vec2, depth: f64) -> Option<Vec3> {
        let (local, _) = embed(self.surfaces.get(i)?, q, depth)?;
        Some(self.places.get(i).copied().flatten()?.point(local))
    }

    /// Where panel `i`'s neutral chart lies in the flat (a planar panel's chart is its neutral
    /// one).
    pub fn placement(&self, i: usize) -> Option<Rigid2> {
        self.unplace.get(i).copied().flatten().map(|p| p.inverse())
    }

    /// Where the point of panel `i`'s surface at the parameters `q` lies in the flat: the inverse
    /// of [`Fold::param`]. A roll's angle is taken within half a turn of its region's middle.
    pub fn flat_point(&self, i: usize, q: Vec2) -> Option<Vec2> {
        let q = match self.surfaces.get(i)? {
            Surface::Cylinder { .. } | Surface::Cone { .. } => {
                let middle = self.middle.get(i).copied().unwrap_or(0.0);
                Vec2::new(q.x, q.y + ((middle - q.y) / std::f64::consts::TAU).round() * std::f64::consts::TAU)
            }
            Surface::Plane | Surface::Spline(_) => q,
        };
        Some(self.unplace.get(i).copied().flatten()?.inverse().point(self.maps.get(i)?.point(q)))
    }

    /// The parameters of panel `i`'s surface at a point of its flat material.
    pub fn param(&self, i: usize, p: Vec2) -> Option<Vec2> {
        let neutral = self.unplace.get(i).copied().flatten()?.point(p);
        self.maps.get(i)?.param(neutral, self.middle.get(i).copied().unwrap_or(0.0))
    }

    /// A roll's axis: a point of it and its direction, round which the roll's angle turns
    /// positively (right-handed). `None` for a plane.
    pub fn axis(&self, i: usize) -> Option<(Vec3, Vec3)> {
        let place = self.places.get(i).copied().flatten()?;
        match *self.surfaces.get(i)? {
            Surface::Cylinder { material_inward, .. } | Surface::Cone { material_inward, .. } => {
                Some((place.shift, place.z * turn_sign(material_inward)))
            }
            _ => None,
        }
    }
}

/// Folds a developed sheet. `roots` says where the first panel of each of the development's
/// pieces lies in space; every other panel goes where the bends and seams that join it to that
/// one put it. A rip holds nothing: a piece goes where its root puts it.
pub fn fold(sheet: &Sheet, dev: &Development, roots: &[Rigid3]) -> Result<Fold, DevelopError> {
    if roots.len() != dev.pieces.len() {
        return Err(DevelopError::Roots(dev.pieces.len()));
    }
    let (t, k) = (sheet.thickness, sheet.k_factor);
    let mut maps = Vec::with_capacity(sheet.panels.len());
    let mut middle = Vec::with_capacity(sheet.panels.len());
    for (i, panel) in sheet.panels.iter().enumerate() {
        if matches!(panel.surface, Surface::Spline(_)) {
            return Err(DevelopError::Panel(i, "not supported yet: folding a free-form panel"));
        }
        maps.push(Flatten::of(&panel.surface, sheet).map_err(|e| DevelopError::Panel(i, e))?);
        middle.push(if panel.region.outer.segs.is_empty() {
            0.0
        } else {
            let b = panel.region.outer.polyline(1e-3).bounds();
            (b.min.y + b.max.y) / 2.0
        });
    }
    // Each joint's anchors as frames in their panels' frames, and where the joint puts the `b`
    // anchor's frame in the `a` one's.
    let mut held: Vec<Option<(Rigid3, Rigid3, Rigid3)>> = Vec::with_capacity(sheet.joints.len());
    for (j, joint) in sheet.joints.iter().enumerate() {
        let across = match &joint.kind {
            JointKind::Rip(_) => None,
            JointKind::Seam => Some(Rigid3::IDENTITY),
            JointKind::Bend(bend) => Some(bent_across(bend, t, k)),
        };
        held.push(match across {
            None => None,
            Some(across) => {
                let frame = |a: &Anchor| sheet.panels.get(a.panel).and_then(|p| anchor_frame(&p.surface, a));
                let (Some(fa), Some(fb)) = (frame(&joint.a), frame(&joint.b)) else {
                    return Err(DevelopError::Joint(j, "an anchor has no frame on its panel"));
                };
                Some((fa, fb, across))
            }
        });
    }
    // Where a joint puts one panel from the other: the `b` panel from the `a` one, or back.
    let carry = |(fa, fb, across): &(Rigid3, Rigid3, Rigid3), from_a: bool, at: &Rigid3| -> Rigid3 {
        if from_a { fb.inverse().then(across).then(fa).then(at) } else { fa.inverse().then(&across.inverse()).then(fb).then(at) }
    };

    let mut touching: Vec<Vec<usize>> = vec![Vec::new(); sheet.panels.len()];
    for (j, joint) in sheet.joints.iter().enumerate() {
        if held.get(j).is_some_and(Option::is_some) {
            for p in [joint.a.panel, joint.b.panel] {
                touching.get_mut(p).ok_or(DevelopError::NoPanel(p))?.push(j);
            }
        }
    }
    let mut places: Vec<Option<Rigid3>> = vec![None; sheet.panels.len()];
    let mut followed = vec![false; sheet.joints.len()];
    for (piece, root) in dev.pieces.iter().zip(roots) {
        let Some(&start) = piece.first() else { continue };
        *places.get_mut(start).ok_or(DevelopError::NoPanel(start))? = Some(*root);
        let mut queue = VecDeque::from([start]);
        while let Some(panel) = queue.pop_front() {
            let Some(at) = places.get(panel).copied().flatten() else { continue };
            for &j in touching.get(panel).map(Vec::as_slice).unwrap_or_default() {
                let (Some(joint), Some(Some(h)), Some(false)) = (sheet.joints.get(j), held.get(j), followed.get(j).copied()) else { continue };
                let other = if joint.a.panel == panel { joint.b.panel } else { joint.a.panel };
                if places.get(other).copied().flatten().is_some() {
                    continue;
                }
                if let Some(slot) = places.get_mut(other) {
                    *slot = Some(carry(h, joint.a.panel == panel, &at));
                }
                if let Some(f) = followed.get_mut(j) {
                    *f = true;
                }
                queue.push_back(other);
            }
        }
    }

    let mut loops = Vec::new();
    let mut bends = Vec::with_capacity(sheet.joints.len());
    for (j, joint) in sheet.joints.iter().enumerate() {
        let (a, b) = (places.get(joint.a.panel).copied().flatten(), places.get(joint.b.panel).copied().flatten());
        if let (Some(a), Some(b), Some(Some(h)), Some(false)) = (a, b, held.get(j), followed.get(j).copied()) {
            let fits = carry(h, true, &a);
            let at = h.1.shift;
            loops.push(Mismatch { joint: j, distance: b.point(at).dist(fits.point(at)), angle: b.angle_to(&fits) });
        }
        bends.push(match (&joint.kind, a, held.get(j)) {
            (JointKind::Bend(bend), Some(a), Some(Some((fa, ..)))) => {
                // Bends join planar panels only, whose neutral charts are their charts.
                let flat = dev.placements.get(joint.a.panel).copied().flatten();
                let flat = flat.and_then(|f| Some((f.point(joint.a.origin), f.vector(joint.a.dir).normalized()?)));
                flat.map(|flat| Bent { frame: fa.then(&a), wrap: Wrap::of(bend, t, k), angle: bend.angle, flat })
            }
            _ => None,
        });
    }
    Ok(Fold {
        places,
        bends,
        loops,
        surfaces: sheet.panels.iter().map(|p| p.surface.clone()).collect(),
        maps,
        unplace: dev.placements.iter().map(|p| p.map(|p| p.inverse())).collect(),
        middle,
    })
}

/// Where a bend puts its `b` anchor's frame in its `a` anchor's (`x` along the joint, `y` toward
/// the material, `z` toward the other panel): turned round the bend, at the `b` panel's tangent
/// line, less its setback back along the `b` panel.
fn bent_across(bend: &Bend, thickness: f64, k_factor: f64) -> Rigid3 {
    let w = Wrap::of(bend, thickness, k_factor);
    let (sin, cos) = bend.angle.sin_cos();
    let on = Vec3::new(0.0, w.sign * sin, cos);
    let up = Vec3::new(0.0, cos, -w.sign * sin);
    Rigid3 { x: Vec3::X, y: up, z: on, shift: w.at(0.0, bend.angle, 0.0) - on * w.setback }
}

/// An anchor's frame in its panel's frame: at its origin, `x` along it, `y` toward the
/// material and `z` across it, toward the other panel from the `a` anchor (into its own panel
/// from the `b` one).
fn anchor_frame(surface: &Surface, a: &Anchor) -> Option<Rigid3> {
    let (p, normal) = embed(surface, a.origin, 0.0)?;
    Rigid3::frame(p, tangent(surface, a.origin, a.dir)?, normal)
}

/// Which way a roll's angle turns round the axis of its frame.
fn turn_sign(material_inward: bool) -> f64 {
    if material_inward { 1.0 } else { -1.0 }
}

/// The point of a surface at the parameters `q`, `depth` into the material, in the surface's
/// frame, and the unit normal toward the material there. `None` for a free-form surface.
fn embed(surface: &Surface, q: Vec2, depth: f64) -> Option<(Vec3, Vec3)> {
    match *surface {
        Surface::Plane => Some((Vec3::new(q.x, q.y, depth), Vec3::Z)),
        Surface::Cylinder { radius, material_inward } => {
            let s = turn_sign(material_inward);
            let out = Vec3::new(q.y.cos(), s * q.y.sin(), 0.0);
            let normal = out * -s;
            Some((out * radius + Vec3::Z * q.x + normal * depth, normal))
        }
        Surface::Cone { half_angle, material_inward } => {
            let s = turn_sign(material_inward);
            let (sa, ca) = half_angle.sin_cos();
            let out = Vec3::new(q.y.cos(), s * q.y.sin(), 0.0);
            let normal = (out * ca - Vec3::Z * sa) * -s;
            Some(((out * sa + Vec3::Z * ca) * q.x + normal * depth, normal))
        }
        Surface::Spline(ref spline) => {
            // Its own points, and its normal from the way they run: `u x v` toward the material.
            let h = PARAM_STEP * (1.0 + q.x.abs().max(q.y.abs()));
            let at = |d: Vec2| spline.eval(q.x + d.x, q.y + d.y).ok();
            let su = at(Vec2::new(h, 0.0))? - at(Vec2::new(-h, 0.0))?;
            let sv = at(Vec2::new(0.0, h))? - at(Vec2::new(0.0, -h))?;
            let normal = su.cross(sv).normalized()?;
            Some((at(Vec2::ZERO)? + normal * depth, normal))
        }
    }
}

/// A step in a surface's parameters, as a fraction of their size, over which its directions and
/// its metric are measured where they are not worked out exactly.
const PARAM_STEP: f64 = 1e-6;

/// The first fundamental form of a surface at `q`, `[E, F, G]`: how the lengths of and the angle
/// between parameter directions there come out on the surface. Measured from the surface's own
/// points, so the same for every kind of surface.
pub(crate) fn metric(surface: &Surface, q: Vec2) -> Option<[f64; 3]> {
    let h = PARAM_STEP * (1.0 + q.x.abs().max(q.y.abs()));
    let at = |d: Vec2| embed(surface, q + d, 0.0).map(|(p, _)| p);
    let su = (at(Vec2::new(h, 0.0))? - at(Vec2::new(-h, 0.0))?) * (1.0 / (2.0 * h));
    let sv = (at(Vec2::new(0.0, h))? - at(Vec2::new(0.0, -h))?) * (1.0 / (2.0 * h));
    Some([su.dot(su), su.dot(sv), sv.dot(sv)])
}

/// The unit direction in space, in the surface's frame, of the parameter direction `dir` at `q`.
fn tangent(surface: &Surface, q: Vec2, dir: Vec2) -> Option<Vec3> {
    let along = match *surface {
        Surface::Plane => Vec3::new(dir.x, dir.y, 0.0),
        Surface::Cylinder { radius, material_inward } => {
            let s = turn_sign(material_inward);
            Vec3::Z * dir.x + Vec3::new(-q.y.sin(), s * q.y.cos(), 0.0) * (radius * dir.y)
        }
        Surface::Cone { half_angle, material_inward } => {
            let s = turn_sign(material_inward);
            let (sa, ca) = half_angle.sin_cos();
            let out = Vec3::new(q.y.cos(), s * q.y.sin(), 0.0);
            (out * sa + Vec3::Z * ca) * dir.x + Vec3::new(-q.y.sin(), s * q.y.cos(), 0.0) * (q.x * sa * dir.y)
        }
        Surface::Spline(_) => {
            let h = PARAM_STEP * (1.0 + q.x.abs().max(q.y.abs()));
            embed(surface, q + dir * h, 0.0)?.0 - embed(surface, q - dir * h, 0.0)?.0
        }
    };
    along.normalized()
}
