//! What a perforation field's holes take out of a sheet made from surfaces (#6748, #732). The
//! holes are not cut into the body: the sheet's physical properties take them off its measure.
//! A hole lies where its field's grid puts it on the developed flat, clear of the bends the sheet
//! had when the field was made (see [`SurfaceSheet::perforation_holes`]). On a flat panel it is a
//! cylinder through the thickness. A bend made after the field runs through it, as a press brake
//! bends perforated stock, and the hole bends with it: its part in the bend is the bend's own map
//! of that part of the flat, and the rest lies on the two panels. A hole beside a bend's end, level
//! with its zone, is on a panel, and one on a tab a cut left of a zone rides flat with the tab's
//! panel.
//!
//! Each part is integrated in the bend's coordinates: along the joint, across the zone and through
//! the thickness. A bend's map is linear along the joint, and its volume grows linearly through
//! the thickness, so a three-point Gauss rule is exact in both. Across the zone the hole's chord
//! is r cos θ where w = c + r sin θ, which leaves an analytic integrand; composite Gauss–Legendre
//! takes it to round-off. Nothing is tessellated.
//!
//! A hexagonal or slotted hole on a flat panel, and every hole of a sheet laid out flat, is the
//! prism of its outline through the thickness, its area, centroid and second moments in closed
//! form. One that a bend runs through is split, as a round one is, into its parts on the two
//! panels and in the bend, each its own map of its part of the flat. Each part's integrals over
//! the flat become integrals round its outline by Green's theorem: the outline's lines and arcs
//! are cut where they cross the zone's edges, and the zone's edges themselves add nothing, since
//! they run along the joint.

use std::f64::consts::TAU;

use solidcore_record::{Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::prelude::*;
use solidcraft_develop::{Bent, Rigid2, Rigid3, Surface};
use solidcraft_geom::bspline::GAUSS_8;
use solidcraft_geom::inertia::{self, Tensor3};
use solidcraft_geom::prelude::*;

use crate::sheet_surface_perforation_shapes::GRAZE;
use crate::{DocError, PerforationShape, Result, ShapedPerforation, SurfaceSheet};

/// What one hole takes out of the body, in the sheet's frame: its volume, the area its walls add
/// to the body's faces less the area it opens in them, its centroid, and its inertia about that
/// centroid at unit density.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HoleMass {
    pub(crate) volume: f64,
    pub(crate) area: f64,
    pub(crate) centroid: Vec3,
    pub(crate) inertia: Tensor3,
}

/// Three-point Gauss–Legendre nodes and weights on [−1, 1]: exact up to degree five.
const GAUSS_3: [(f64, f64); 3] = [(-0.774_596_669_241_483_4, 5.0 / 9.0), (0.0, 8.0 / 9.0), (0.774_596_669_241_483_4, 5.0 / 9.0)];

/// Gauss panels across a hole's part in a bend, and round its walls there.
const PANELS: usize = 8;

/// Every hole of `sk`'s perforation fields as what it takes out of the body. Refused: a field's
/// numbers that make no grid, and (not supported yet) a hole on a roll, a hole two bends reach,
/// and one a bend's end runs through.
pub(crate) fn hole_masses(sk: &SurfaceSheet) -> Result<Vec<HoleMass>> {
    let invalid = || DocError::Invalid(format!("invalid perforation physical properties for `{}`", sk.body));
    let t = sk.thickness;
    if !(t.is_finite() && t > 0.0) {
        return Err(invalid());
    }
    // Each field's numbers before its grid is laid out: a pitch no wider than a hole makes none,
    // and holes of a shape or rows of a pitch of their own leave room between them.
    let shown = sk.shown();
    for p in &shown.perforations {
        if !([p.diameter, p.pitch, p.margin].iter().all(|v| v.is_finite()) && p.diameter > 0.0 && p.pitch > p.diameter && p.margin >= 0.0) {
            return Err(invalid());
        }
        let field = shown.shaped_perforation(p);
        if !field.is_plain() {
            field.check_spacing()?;
        }
    }
    // The holes in the flat as the sheet lays it out: the flat as it is read, mirrored back. A
    // hexagon with a corner up and a slot along the rows are their own mirror images.
    let mut holes: Vec<(Vec2, ShapedPerforation)> = Vec::new();
    for p in &shown.perforations {
        let field = shown.shaped_perforation(p);
        holes.extend(shown.try_perforation_holes(p)?.into_iter().map(|h| (Vec2::new(h.x, -h.y), field)));
    }
    if holes.is_empty() {
        return Ok(Vec::new());
    }
    // Laid out flat, the flat is one plane, the sheet's own flat frame (never a panel's, which a
    // cut can take away): every hole goes through it, its rows along the flat's X.
    if sk.unfolded {
        return Ok(holes.iter().map(|(c, field)| through(&sk.flat, *c, Vec2::new(1.0, 0.0), field, t)).collect());
    }
    let dev = sk.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
    let fold = crate::sheet_surface_convert::fold_onto(&sk.sheet, &dev, &sk.places)?;
    let frames = crate::sheet_surface_cuts::frames(sk, &dev)?;
    let mut bends = Vec::with_capacity(dev.zones.len());
    for z in &dev.zones {
        let Some(bent) = fold.bends.get(z.joint).copied().flatten() else { continue };
        // The pieces the cuts left of the zone held by one panel: each runs along that panel's
        // tangent line, as the flat sorted them.
        let mut tabs = Vec::new();
        for tab in sk.bends.iter().filter(|b| b.joint == z.joint).flat_map(|b| &b.tabs) {
            let panel = match (crate::sheet_surface::along(tab, z.near), crate::sheet_surface::along(tab, z.far)) {
                (true, false) => z.panels[0],
                (false, true) => z.panels[1],
                _ => return Err(invalid()),
            };
            tabs.push((tab.clone(), panel));
        }
        bends.extend(Zone::new(z, bent, tabs));
    }
    // Each panel's material in the flat, and the loose pieces with the panels they lie in.
    let material: Vec<(usize, Region2)> = dev
        .panels
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref().map(|p| (i, p.region.clone())))
        .chain(sk.loose.iter().filter_map(|l| dev.placements.get(l.panel).copied().flatten().map(|at| (l.panel, at.region(&l.region)))))
        .collect();
    // Panel `i`: where its chart lies in space, and where the flat lays the chart.
    let panel = |i: usize| -> Result<(Rigid3, Rigid2)> {
        if sk.sheet.panels.get(i).is_none_or(|panel| panel.surface != Surface::Plane) {
            return Err(DocError::Invalid(format!("not supported yet: perforation physical properties of a hole on a roll of `{}`", sk.body)));
        }
        match (frames.get(i).copied().flatten(), dev.placements.get(i).copied().flatten()) {
            (Some(frame), Some(flat)) => Ok((frame, flat)),
            _ => Err(invalid()),
        }
    };
    let mut out = Vec::with_capacity(holes.len());
    for (c, field) in holes {
        // On one flat panel (or a tab of one) the hole goes through it; one bend bends it.
        let on_panel = |i: usize| -> Result<HoleMass> {
            let (frame, flat) = panel(i)?;
            let back = flat.inverse();
            Ok(through(&frame, back.point(c), back.vector(Vec2::new(1.0, 0.0)), &field, t))
        };
        let reached: Vec<(&Zone, Reach)> = bends.iter().filter_map(|z| Some((z, z.reach(c, &field)?))).collect();
        match reached.as_slice() {
            [] => {
                let (i, _) = material.iter().find(|(_, g)| g.contains(c)).ok_or_else(invalid)?;
                out.push(on_panel(*i)?);
            }
            // A tab is laid out with its panel and folds with it: the panel's chart carried on.
            [(_, Reach::Tab(i))] => out.push(on_panel(*i)?),
            [(zone, Reach::Bends)] => {
                if !zone.holds_along(c, &field) {
                    return Err(DocError::Invalid(format!("not supported yet: a perforation hole across the end of a bend of `{}`", sk.body)));
                }
                let (a, b) = (panel(zone.panels[0])?, panel(zone.panels[1])?);
                out.push(match field.shape {
                    PerforationShape::Round => zone.bent_hole(c, field.field.diameter / 2.0, t, a, b)?,
                    PerforationShape::Hex | PerforationShape::Slot { .. } => zone.bent_outline(c, &field, t, a, b)?,
                });
            }
            _ => return Err(DocError::Invalid(format!("not supported yet: a perforation hole two bends reach on `{}`", sk.body))),
        }
    }
    Ok(out)
}

/// A hole of `field` through a flat panel whose chart lies in space at `frame`, round the chart's
/// point `at`, its rows along the chart's unit vector `along`: a round hole's cylinder, else the
/// prism of its outline.
fn through(frame: &Rigid3, at: Vec2, along: Vec2, field: &ShapedPerforation, t: f64) -> HoleMass {
    match field.shape {
        PerforationShape::Round => cylinder(frame, at, field.field.diameter / 2.0, t),
        PerforationShape::Hex | PerforationShape::Slot { .. } => prism(frame, at, along, field, t),
    }
}

/// A hexagonal or slotted hole of `field` through a flat panel whose chart lies in space at
/// `frame`: the prism of its outline round the chart's point `at`, its rows along the chart's unit
/// vector `along`, `t` deep along the frame's `z`. Its area and second moments are its outline's,
/// in closed form, and its centroid is the outline's centre, mid-depth.
fn prism(frame: &Rigid3, at: Vec2, along: Vec2, field: &ShapedPerforation, t: f64) -> HoleMass {
    let face = field.hole_area();
    let volume = face * t;
    let (xx, yy) = field.hole_second_moments();
    let depth = volume * t * t / 12.0;
    // About the hole's own axes: along its rows, across them, and through the sheet.
    let own = [[t * yy + depth, 0.0, 0.0], [0.0, t * xx + depth, 0.0], [0.0, 0.0, t * (xx + yy)]];
    let (u, v) = (frame.vector(Vec3::new(along.x, along.y, 0.0)), frame.vector(Vec3::new(-along.y, along.x, 0.0)));
    let turn = [[u.x, v.x, frame.z.x], [u.y, v.y, frame.z.y], [u.z, v.z, frame.z.z]];
    HoleMass {
        volume,
        area: field.hole_perimeter() * t - 2.0 * face,
        centroid: frame.point(Vec3::new(at.x, at.y, t / 2.0)),
        inertia: inertia::transformed(&own, &turn),
    }
}

/// A hole of radius `r` through a flat panel whose chart lies in space at `frame`: a cylinder
/// round the chart's point `at`, `t` deep along the frame's `z`.
fn cylinder(frame: &Rigid3, at: Vec2, r: f64, t: f64) -> HoleMass {
    let disk = std::f64::consts::PI * r * r;
    let volume = disk * t;
    let transverse = volume * (3.0 * r * r + t * t) / 12.0;
    let own = [[transverse, 0.0, 0.0], [0.0, transverse, 0.0], [0.0, 0.0, volume * r * r / 2.0]];
    HoleMass {
        volume,
        area: 2.0 * std::f64::consts::PI * r * t - 2.0 * disk,
        centroid: frame.point(Vec3::new(at.x, at.y, t / 2.0)),
        inertia: inertia::transformed(&own, &axes(frame)),
    }
}

/// The rotation of `frame` as a matrix: its axes as columns.
fn axes(frame: &Rigid3) -> Tensor3 {
    let [x, y, z] = [frame.x, frame.y, frame.z];
    [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]]
}

/// A bend's zone in the flat with the bend in space: the zone's edge on its `a` panel's side (its
/// start and the way along it), the way across toward the `b` panel, how wide the zone is across
/// (the bend's allowance) and the radius that width wraps onto, the panels on its two sides, the
/// bend, the zone's region of the flat, and the tabs the cuts left of it with the panel each rides
/// with.
struct Zone {
    start: Vec2,
    along: Vec2,
    across: Vec2,
    span: (f64, f64),
    width: f64,
    neutral: f64,
    panels: [usize; 2],
    bent: Bent,
    region: Region2,
    tabs: Vec<(Region2, usize)>,
}

/// What a bend's zone is to a hole that reaches into it.
enum Reach {
    /// The zone bends the hole with it.
    Bends,
    /// The hole is on a tab of the zone, which lies flat with panel `.0`.
    Tab(usize),
}

impl Zone {
    fn new(z: &solidcraft_develop::Zone, bent: Bent, tabs: Vec<(Region2, usize)>) -> Option<Zone> {
        let start = z.near[0];
        let along = (z.near[1] - z.near[0]).normalized()?;
        let left = Vec2::new(-along.y, along.x);
        let across = if (z.far[0] - start).dot(left) < 0.0 { -left } else { left };
        let width = (z.far[0] - start).dot(across);
        let neutral = width / bent.angle().abs();
        if !(width > 0.0 && neutral.is_finite() && neutral > 0.0) {
            return None;
        }
        // Along the joint, where both of the zone's edges run.
        let a = |p: Vec2| (p - start).dot(along);
        let span = (a(z.near[0]).max(a(z.far[0])), a(z.near[1]).min(a(z.far[1])));
        Some(Zone { start, along, across, span, width, neutral, panels: z.panels, bent, region: z.region.clone(), tabs })
    }

    /// A point of the flat as its distance along the zone's `a` edge and across toward its `b`
    /// edge.
    fn coords(&self, p: Vec2) -> (f64, f64) {
        ((p - self.start).dot(self.along), (p - self.start).dot(self.across))
    }

    /// What the zone is to the hole of `field` round `c`: nothing when the hole keeps clear of the
    /// zone's region, its ends included, as one beside an end of the bend, level with its zone,
    /// does; the tab it is on when that is a tab of the zone; else the bend. A hole may graze the
    /// region by [`GRAZE`], as the grid keeps it: it then keeps clear (see
    /// [`ShapedPerforation::overlaps`]). (A hole lies in the sheet's material, clear of its cuts, so
    /// the part of it in the zone's region is on the zone's material: a piece that bends, or a
    /// tab.)
    fn reach(&self, c: Vec2, field: &ShapedPerforation) -> Option<Reach> {
        if !field.overlaps(c, &self.region) {
            return None;
        }
        Some(self.tabs.iter().find(|(tab, _)| field.overlaps(c, tab)).map_or(Reach::Bends, |(_, panel)| Reach::Tab(*panel)))
    }

    /// Whether the hole of `field` round `c` lies between the zone's ends, along the joint, to
    /// within what it may graze.
    fn holds_along(&self, c: Vec2, field: &ShapedPerforation) -> bool {
        let (a, _) = self.coords(c);
        let e = field.support(self.along);
        a - e >= self.span.0 - GRAZE && a + e <= self.span.1 + GRAZE
    }

    /// Where the line or arc `s` crosses the zone's edges (across it, 0 and its width): its
    /// parameters there, with its ends, in order. Between two of them it lies in one part. None
    /// for any other kind of segment, which a hole's outline doesn't have.
    fn crossings(&self, s: &Seg2) -> Option<Vec<f64>> {
        let w = |p: Vec2| (p - self.start).dot(self.across);
        let mut u = vec![0.0, 1.0];
        for edge in [0.0, self.width] {
            match *s {
                Seg2::Line { a, b } => {
                    let (wa, wb) = (w(a), w(b));
                    if (wa - edge) * (wb - edge) < 0.0 {
                        u.push((edge - wa) / (wb - wa));
                    }
                }
                // Across the zone the arc runs w(centre) + radius cos(θ − φ), φ the way across.
                Seg2::Arc { center, radius, start, sweep } => {
                    let k = (edge - w(center)) / radius;
                    if k.abs() < 1.0 && sweep != 0.0 {
                        let (phi, half) = (self.across.angle(), k.acos());
                        for theta in [phi + half, phi - half] {
                            for turns in -2..=2 {
                                let v = (theta + f64::from(turns) * TAU - start) / sweep;
                                if v > 0.0 && v < 1.0 {
                                    u.push(v);
                                }
                            }
                        }
                    }
                }
                // A hole's outline is lines and arcs; any other kind isn't cut here.
                _ => return None,
            }
        }
        u.sort_by(f64::total_cmp);
        Some(u)
    }

    /// The hexagonal or slotted hole of `field` round `c` that the bend runs through, `t` deep:
    /// its part in the zone as the bend maps it, and its parts on the `a` and `b` panels (each
    /// panel's frame in space and where the flat lays its chart). Each part is what the pieces of
    /// the outline in it bound (see [`Sum::piece`]).
    fn bent_outline(&self, c: Vec2, field: &ShapedPerforation, t: f64, a: (Rigid3, Rigid2), b: (Rigid3, Rigid2)) -> Result<HoleMass> {
        let unread = || DocError::Invalid("a perforation hole in a bend can't be read".into());
        let (ca, cw) = self.coords(c);
        let (axis_point, axis) = self.bent.axis();
        let flat = |(frame, at): (Rigid3, Rigid2)| {
            move |p: Vec2, depth: f64| {
                let q = at.inverse().point(p);
                Some((frame.point(Vec3::new(q.x, q.y, depth)), 1.0))
            }
        };
        let bend = |p: Vec2, depth: f64| {
            let x = self.bent.point(p, depth);
            // The volume of the zone grows with the distance from the axis, against the neutral
            // radius its width wraps onto.
            let rho = (x - axis_point).cross(axis).len();
            Some((x, rho / self.neutral))
        };
        let (on_a, on_b) = (flat(a), flat(b));
        // The part a point across the zone lies in.
        let part = |w: f64| -> &dyn Fn(Vec2, f64) -> Option<(Vec3, f64)> {
            if w < 0.0 {
                &on_a
            } else if w > self.width {
                &on_b
            } else {
                &bend
            }
        };
        // About the hole's middle, mid-depth, so that its moments keep their digits.
        let middle = part(cw)(c, t / 2.0).map(|(x, _)| x).ok_or_else(unread)?;
        let at = |a: f64, w: f64| c + self.along * (a - ca) + self.across * (w - cw);
        let coords = |p: Vec2| self.coords(p);
        // The outline runs counter-clockwise in the flat; along and across the zone, the other
        // way where those two turn the flat over.
        let turn = self.along.cross(self.across);
        let mut sum = Sum::default();
        for s in &field.hole_loop(c).segs {
            let cuts = self
                .crossings(s)
                .ok_or_else(|| DocError::Invalid("not supported yet: a perforation hole in a bend whose outline is not lines and arcs".into()))?;
            for (&u0, &u1) in cuts.iter().zip(cuts.iter().skip(1)) {
                if !(u1 > u0) {
                    continue;
                }
                let (_, w) = self.coords(s.point_at((u0 + u1) / 2.0));
                let piece = Piece { seg: s, span: (u0, u1), along: self.along, across: self.across, turn };
                sum.piece(part(w), &at, &coords, &piece, ca, t, middle).ok_or_else(unread)?;
            }
        }
        sum.hole(middle)
    }

    /// The hole of radius `r` round `c` that the bend runs through, `t` deep: its part in the zone
    /// as the bend maps it, and its parts on the `a` and `b` panels (each panel's frame in space
    /// and where the flat lays its chart).
    fn bent_hole(&self, c: Vec2, r: f64, t: f64, a: (Rigid3, Rigid2), b: (Rigid3, Rigid2)) -> Result<HoleMass> {
        let (ca, cw) = self.coords(c);
        let (axis_point, axis) = self.bent.axis();
        let flat = |(frame, at): (Rigid3, Rigid2)| {
            move |p: Vec2, depth: f64| {
                let q = at.inverse().point(p);
                Some((frame.point(Vec3::new(q.x, q.y, depth)), 1.0))
            }
        };
        let bend = |p: Vec2, depth: f64| {
            let x = self.bent.point(p, depth);
            // The volume of the zone grows with the distance from the axis, against the neutral
            // radius its width wraps onto.
            let rho = (x - axis_point).cross(axis).len();
            Some((x, rho / self.neutral))
        };
        let mut sum = Sum::default();
        let parts: [(f64, f64, &dyn Fn(Vec2, f64) -> Option<(Vec3, f64)>); 3] =
            [(cw - r, 0.0, &flat(a)), (0.0, self.width, &bend), (self.width, cw + r, &flat(b))];
        // About the hole's middle, mid-depth, so that its moments keep their digits.
        let middle = parts
            .iter()
            .find(|(w0, w1, _)| cw >= *w0 && cw <= *w1)
            .and_then(|(_, _, map)| map(c, t / 2.0))
            .map(|(x, _)| x)
            .ok_or_else(|| DocError::Invalid("a perforation hole in a bend can't be read".into()))?;
        let at = |a: f64, w: f64| c + self.along * (a - ca) + self.across * (w - cw);
        for (w0, w1, map) in parts {
            let (w0, w1) = (w0.max(cw - r), w1.min(cw + r));
            if w1 - w0 <= r * 1e-12 {
                continue;
            }
            sum.part(map, &at, (ca, cw), r, t, (w0, w1), middle)
                .ok_or_else(|| DocError::Invalid("a perforation hole in a bend can't be read".into()))?;
        }
        sum.hole(middle)
    }
}

/// The moments a hole's parts add up to: volume, its first and second moments about a point,
/// the area it opens in the body's two faces, and the area of its walls.
#[derive(Default)]
struct Sum {
    volume: f64,
    first: Vec3,
    second: Tensor3,
    faces: f64,
    walls: f64,
}

impl Sum {
    /// Adds the part of the hole of radius `r` round (`ca`, `cw`) (along and across the zone) that
    /// lies across from `w0` to `w1`, through the thickness `t`, as `map` takes the flat point
    /// `at(a, w)` and a depth into space and gives the volume's stretch there. Moments about
    /// `middle`. None where the map can't place a point.
    fn part(
        &mut self,
        map: &dyn Fn(Vec2, f64) -> Option<(Vec3, f64)>,
        at: &dyn Fn(f64, f64) -> Vec2,
        (ca, cw): (f64, f64),
        r: f64,
        t: f64,
        (w0, w1): (f64, f64),
        middle: Vec3,
    ) -> Option<()> {
        // Across: w = cw + r sin θ, so the chord at w is 2 r cos θ and dw = r cos θ dθ.
        let angle = |w: f64| ((w - cw) / r).clamp(-1.0, 1.0).asin();
        let (th0, th1) = (angle(w0), angle(w1));
        for (theta, weight) in panels(th0, th1) {
            let (sin, cos) = theta.sin_cos();
            let (w, half) = (cw + r * sin, r * cos);
            let dw = weight * r * cos;
            for (xa, wa) in GAUSS_3 {
                let a = ca + half * xa;
                let da = wa * half;
                let p = at(a, w);
                for (xd, wd) in GAUSS_3 {
                    let depth = t / 2.0 * (1.0 + xd);
                    let (x, stretch) = map(p, depth)?;
                    let dv = stretch * dw * da * wd * t / 2.0;
                    let d = x - middle;
                    self.volume += dv;
                    self.first += d * dv;
                    for (row, di) in self.second.iter_mut().zip([d.x, d.y, d.z]) {
                        for (cell, dj) in row.iter_mut().zip([d.x, d.y, d.z]) {
                            *cell += di * dj * dv;
                        }
                    }
                }
                // The faces it opens: the sheet's surface and the face a thickness in.
                for depth in [0.0, t] {
                    self.faces += map(p, depth)?.1 * dw * da;
                }
            }
        }
        // Its walls: round the hole's edge, where it lies across from w0 to w1, through the
        // thickness. A wall's way round the edge is stretched across by the volume's stretch, and
        // its way through the thickness is not.
        let (s0, s1) = (((w0 - cw) / r).clamp(-1.0, 1.0).asin(), ((w1 - cw) / r).clamp(-1.0, 1.0).asin());
        let pi = std::f64::consts::PI;
        for (b0, b1) in [(s0, s1), (pi - s1, pi - s0)] {
            for (beta, weight) in panels(b0, b1) {
                let (sin, cos) = beta.sin_cos();
                let p = at(ca + r * cos, cw + r * sin);
                for (xd, wd) in GAUSS_8 {
                    let depth = t / 2.0 * (1.0 + xd);
                    let (_, stretch) = map(p, depth)?;
                    self.walls += weight * r * (sin * sin + (stretch * cos).powi(2)).sqrt() * wd * t / 2.0;
                }
            }
        }
        Some(())
    }

    /// Adds the part of a hole that a piece of its outline bounds in one part of the flat, `t`
    /// deep, as `map` takes the flat point `at(a, w)` (along and across the zone) and a depth
    /// into space and gives the volume's stretch there; `coords` gives a flat point's (a, w).
    /// Moments about `middle`. By Green's theorem a region's integral of F da dw is its outline's
    /// of G dw, G the integral of F along the joint from the hole's centre line `ca`. Every map is
    /// of degree one along the joint and through the thickness, and so is its stretch, so F is of
    /// degree at most three in each and three-point Gauss gives G, and the thickness, exactly;
    /// composite Gauss–Legendre runs along the piece, a line by its parameter and an arc by its
    /// angle. The walls round the piece are stretched across by the volume's stretch, and through
    /// the thickness not. None where the map can't place a point.
    fn piece(
        &mut self,
        map: &dyn Fn(Vec2, f64) -> Option<(Vec3, f64)>,
        at: &dyn Fn(f64, f64) -> Vec2,
        coords: &dyn Fn(Vec2) -> (f64, f64),
        piece: &Piece,
        ca: f64,
        t: f64,
        middle: Vec3,
    ) -> Option<()> {
        let (u0, u1) = piece.span;
        for (u, weight) in panels(u0, u1) {
            let p = piece.seg.point_at(u);
            let (pa, pw) = coords(p);
            let d = piece.seg.derivative(u);
            let (da, dw) = (d.dot(piece.along), d.dot(piece.across));
            let half = (pa - ca) / 2.0;
            for (xa, wa) in GAUSS_3 {
                let q = at(ca + half * (1.0 + xa), pw);
                // This node's share of G dw, the outline's turn included.
                let g = wa * half * dw * weight * piece.turn;
                for (xd, wd) in GAUSS_3 {
                    let depth = t / 2.0 * (1.0 + xd);
                    let (x, stretch) = map(q, depth)?;
                    let dv = stretch * g * wd * t / 2.0;
                    let r = x - middle;
                    self.volume += dv;
                    self.first += r * dv;
                    for (row, ri) in self.second.iter_mut().zip([r.x, r.y, r.z]) {
                        for (cell, rj) in row.iter_mut().zip([r.x, r.y, r.z]) {
                            *cell += ri * rj * dv;
                        }
                    }
                }
                // The faces it opens: the sheet's surface and the face a thickness in.
                for depth in [0.0, t] {
                    self.faces += map(q, depth)?.1 * g;
                }
            }
            for (xd, wd) in GAUSS_8 {
                let depth = t / 2.0 * (1.0 + xd);
                let (_, stretch) = map(p, depth)?;
                self.walls += weight * (da * da + (dw * stretch).powi(2)).sqrt() * wd * t / 2.0;
            }
        }
        Some(())
    }

    /// The hole these parts make, its moments taken about `middle`.
    fn hole(&self, middle: Vec3) -> Result<HoleMass> {
        let v = self.volume;
        if !(v.is_finite() && v > 0.0) {
            return Err(DocError::Invalid("a perforation hole in a bend has no volume".into()));
        }
        let shift = self.first / v;
        // Second moments about the centroid, then the inertia: tr(S) E - S.
        let s = [shift.x, shift.y, shift.z];
        let mut second = self.second;
        for (i, row) in second.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell -= v * s[i] * s[j];
            }
        }
        let trace = second[0][0] + second[1][1] + second[2][2];
        let mut inertia = [[0.0; 3]; 3];
        for (i, row) in inertia.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = if i == j { trace } else { 0.0 } - second[i][j];
            }
        }
        Ok(HoleMass { volume: v, area: self.walls - self.faces, centroid: middle + shift, inertia })
    }
}

/// A piece of a hole's outline in one part of the flat: its segment, between which of its
/// parameters, the ways along and across the zone, and how the outline turns in those (1
/// counter-clockwise, −1 clockwise).
struct Piece<'a> {
    seg: &'a Seg2,
    span: (f64, f64),
    along: Vec2,
    across: Vec2,
    turn: f64,
}

/// Composite Gauss–Legendre on [`PANELS`] equal panels from `a` to `b`: each node and its weight.
fn panels(a: f64, b: f64) -> impl Iterator<Item = (f64, f64)> {
    let step = (b - a) / PANELS as f64;
    (0..PANELS).flat_map(move |k| {
        let mid = a + step * (k as f64 + 0.5);
        GAUSS_8.into_iter().map(move |(x, w)| (mid + step / 2.0 * x, w * step / 2.0))
    })
}
