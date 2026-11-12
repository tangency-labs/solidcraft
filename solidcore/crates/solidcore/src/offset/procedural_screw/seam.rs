//! Continued partitions of coincident screw walls by their authored meridian.
//!
//! A constant source-angle trim is not a physical meridian after normal offset:
//! its generator has a tangential component. Preserve the original station's
//! physical half-plane instead. For generator coordinates (x,y), its correction
//! is q=tan(-phase/2)=-y/(sqrt(x*x+y*y)+x). Each wall's own rational station map
//! then gives its pcurve; no midpoint or nearest coincident edge selects a seam.
//! Interval Taylor coefficients bound the complete interpolation remainder.
//! Parameter error is propagated to model space as in Mukundan et al.,
//! *Tracing Surface Intersections with Validated ODE System Solver* (2004), §4,
//! <https://web.mit.edu/deslab/pubs/files/SM04.pdf>.
//! Deviations: affine invariant generators with an exact common endpoint station;
//! pcurve refinement is iterative, bounded by 4096 cells and depth 32.

use super::{
    bad, certificate, uv_curve, Curve, Ivl, Kernel, Point2, Point3, Result, ScrewOffsetSection,
    Surface, SurfaceIntersection, TsectCurveType, Vec3, Wall,
};
use crate::blend::fillet_section::tay::Tay;
use crate::surface::procedural_sweep::ScrewMotion;
use crate::surface::surface::SurfaceKind;

fn boxed(v: Vec3) -> [Ivl; 3] {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}
fn dot(a: [Ivl; 3], b: [Ivl; 3]) -> Ivl {
    a.into_iter()
        .zip(b)
        .fold(Ivl::from(0.0), |sum, (x, y)| sum + x * y)
}
use super::same_motion;
use crate::geom::extent1d::Extent1d;
use crate::nurbs::math::NOW;
fn controls(curve: &Curve) -> Result<Vec<[Ivl; 3]>> {
    let spline = curve
        .b_spline_base()
        .ok_or_else(|| bad("seam needs a bounded spline"))?;
    let rational = spline.is_rational()?;
    spline
        .control_points()?
        .iter()
        .map(|p| {
            let weight = if rational { p.w } else { 1.0 };
            if !weight.is_finite()
                || weight <= 0.0
                || ![p.x, p.y, p.z].iter().all(|v| v.is_finite())
            {
                return Err(bad("seam controls need finite positive weights"));
            }
            Ok([p.x, p.y, p.z].map(|x| {
                if weight == 1.0 {
                    Ivl::from(x)
                } else {
                    Ivl::from(x) / Ivl::from(weight)
                }
            }))
        })
        .collect()
}

/// Enclosures of the vertices of `curve`'s polygon over `interval`, when the curve
/// has degree one (a line, a polyline): its points at the interval's ends and at the
/// knots inside, each its homogeneous controls interpolated in interval arithmetic and
/// divided by the interpolated weight. Each span of such a curve is the segment between
/// its ends (a rational one too: a projective map of a segment, under positive weights),
/// so the polygon is the curve over the interval. `None` for any other degree.
fn polygon_over(curve: &Curve, interval: Extent1d) -> Result<Option<Vec<[Ivl; 3]>>> {
    let spline = curve
        .b_spline_base()
        .ok_or_else(|| bad("seam needs a bounded spline"))?;
    if spline.degree()? != 1 {
        return Ok(None);
    }
    let knots = spline.knot_values()?;
    // stored homogeneous: a rational point weighted, a non-rational one Euclidean of weight 1
    let homogeneous: Vec<[Ivl; 4]> = spline
        .control_points()?
        .iter()
        .map(|p| {
            let weight = if p.w == NOW { 1.0 } else { p.w };
            if !weight.is_finite()
                || weight <= 0.0
                || ![p.x, p.y, p.z].iter().all(|v| v.is_finite())
            {
                return Err(bad("seam controls need finite positive weights"));
            }
            Ok([p.x, p.y, p.z, weight].map(Ivl::from))
        })
        .collect::<Result<_>>()?;
    // degree one: control j sits at knot j + 1, its Greville abscissa
    let stations: Vec<f64> = (0..homogeneous.len())
        .map(|j| {
            knots
                .get(j + 1)
                .copied()
                .ok_or_else(|| bad("seam curve has too few knots"))
        })
        .collect::<Result<_>>()?;
    let (lo, hi) = (interval.min(), interval.max());
    let (Some(&first), Some(&last)) = (stations.first(), stations.last()) else {
        return Err(bad("empty seam support"));
    };
    if !(lo.is_finite() && hi.is_finite() && first <= lo && lo <= hi && hi <= last) {
        return Err(bad("seam interval lies outside its curve"));
    }
    let euclid = |h: [Ivl; 4]| -> Result<[Ivl; 3]> {
        let [x, y, z, w] = h;
        if w.lo <= 0.0 {
            return Err(bad("seam controls need finite positive weights"));
        }
        Ok([x / w, y / w, z / w])
    };
    let at = |t: f64| -> Result<[Ivl; 3]> {
        for (j, pair) in stations.windows(2).enumerate() {
            let &[a, b] = pair else { continue };
            if !(a < b && a <= t && t <= b) {
                continue;
            }
            let (Some(&p), Some(&q)) = (homogeneous.get(j), homogeneous.get(j + 1)) else {
                break;
            };
            let s = (Ivl::from(t) - Ivl::from(a)) / (Ivl::from(b) - Ivl::from(a));
            let r = Ivl::from(1.0) - s;
            return euclid(std::array::from_fn(|i| {
                p.get(i).copied().unwrap_or(Ivl::whole()) * r
                    + q.get(i).copied().unwrap_or(Ivl::whole()) * s
            }));
        }
        Err(bad("seam interval lies outside its curve"))
    };
    let mut points = vec![at(lo)?];
    for (&g, &h) in stations.iter().zip(&homogeneous) {
        if lo < g && g < hi {
            points.push(euclid(h)?);
        }
    }
    points.push(at(hi)?);
    Ok(Some(points))
}

/// An authored station and physical half-plane, including a rigorous enclosure
/// of its normalized frame. The old cylinder meridian is separately certified
/// against this plane; floating-coordinate equality is never assumed.
pub(in crate::offset) struct Separator {
    station: f64,
    radial: Vec3,
    origin: Vec3,
    normal: [Ivl; 3],
    direction: [Ivl; 3],
    source: ([Vec3; 2], [f64; 2], f64),
    motion: ScrewMotion,
}

/// A complete parent meridian enclosure, including the physical radius and
/// displacement to the authored meridian along a screw orbit.
#[derive(Clone, Copy, Debug)]
pub(in crate::offset) struct CylinderCorner {
    pub radius: Ivl,
    pub meridian_error: f64,
}

impl CylinderCorner {
    pub fn hull(self, other: Self) -> Self {
        Self {
            radius: self.radius.hull(other.radius),
            meridian_error: self.meridian_error.max(other.meridian_error),
        }
    }
}

impl Separator {
    pub fn new(first: &Surface, second: &Surface) -> Result<Option<Self>> {
        let (Some(a), Some(b)) = (first.procedural_sweep(), second.procedural_sweep()) else {
            return Ok(None);
        };
        let (a, b) = (a.definition(), b.definition());
        let (Some(source), Some(other)) =
            (a.retained_affine_source()?, b.retained_affine_source()?)
        else {
            return Ok(None);
        };
        let (Some(ma), Some(mb)) = (a.screw(), b.screw()) else {
            return Ok(None);
        };
        if source != other || source.2 != 0.0 || !same_motion(ma, mb) {
            return Ok(None);
        }
        let station = match (
            a.stations().first(),
            a.stations().last(),
            b.stations().first(),
            b.stations().last(),
        ) {
            (_, Some(&al), Some(&bf), _) if al == bf => al,
            (Some(&af), _, _, Some(&bl)) if af == bl => af,
            _ => return Ok(None),
        };
        let radial = source.0[0] - ma.origin();
        let section = ScrewOffsetSection::new(source.0[0], source.0[1], ma, 0.0)?;
        let (e, f, _) = section.meridian_frame(radial)?;
        let (s, c) = certificate::sin_cos(Ivl::from(station))?;
        let normal = std::array::from_fn(|i| {
            f.get(i).copied().unwrap_or(Ivl::whole()) * c
                - e.get(i).copied().unwrap_or(Ivl::whole()) * s
        });
        let direction = std::array::from_fn(|i| {
            e.get(i).copied().unwrap_or(Ivl::whole()) * c
                + f.get(i).copied().unwrap_or(Ivl::whole()) * s
        });
        Ok(Some(Self {
            station,
            radial,
            origin: ma.origin(),
            normal,
            direction,
            source,
            motion: ma.clone(),
        }))
    }

    /// Selection precedes construction: an unchanged pair needs no new seam,
    /// and unequal offsets are a different (noncoincident) geometric class.
    pub fn supports_targets(&self, first: &Surface, second: &Surface) -> Result<bool> {
        let (Some(a), Some(b)) = (first.procedural_sweep(), second.procedural_sweep()) else {
            return Ok(false);
        };
        let (Some(a), Some(b)) = (
            a.definition().retained_affine_source()?,
            b.definition().retained_affine_source()?,
        ) else {
            return Ok(false);
        };
        Ok(a == b
            && a.0 == self.source.0
            && a.1 == self.source.1
            && a.2.is_finite()
            && a.2 != self.source.2)
    }

    fn point_plane(&self, point: [Ivl; 3], tolerance: f64) -> Result<f64> {
        let [x, y, z] = point;
        let [ox, oy, oz] = boxed(self.origin);
        let relative = [x - ox, y - oy, z - oz];
        if dot(self.direction, relative).lo <= 0.0 || dot(self.normal, relative).mag() > tolerance {
            return Err(bad(
                "authored seam is outside its certified meridian half-plane",
            ));
        }
        Ok(dot(self.normal, relative).mag())
    }

    /// The part of `curve` an edge uses, over `interval`, lies in the half-plane
    /// within `tolerance`. A curve of degree one is its polygon over the interval
    /// ([`polygon_over`]): a line keeps no more than its edge's segment, however
    /// far its stored support runs (a record's line is unbounded, and the kernel's
    /// runs on across the axis). Any other curve is held by all its controls:
    /// positive rational weights make every point a convex combination of the
    /// Euclidean controls, so these inequalities cover the entire curve.
    pub fn certify_curve(&self, curve: &Curve, interval: Extent1d, tolerance: f64) -> Result<()> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(bad("invalid seam tolerance"));
        }
        let points = match polygon_over(curve, interval)? {
            Some(points) => points,
            None => controls(curve)?,
        };
        if points.len() < 2 {
            return Err(bad("empty seam support"));
        }
        for point in points {
            self.point_plane(point, tolerance)?;
        }
        Ok(())
    }

    pub fn certify_source_uv(&self, surface: &Surface, uv: &Curve, tolerance: f64) -> Result<()> {
        let definition = surface
            .procedural_sweep()
            .ok_or_else(|| bad("missing original seam definition"))?
            .definition();
        let (a, b, swapped) = definition.definition_map();
        if a.into_iter().chain(b).any(|v| !v.is_finite()) || b.contains(&0.0) {
            return Err(bad("singular seam chart"));
        }
        let mut turn = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
        let mut profile = turn;
        for [x, y, _] in controls(uv)? {
            let (w, t) = if swapped { (y, x) } else { (x, y) };
            turn = turn.hull(Ivl::from(a[1]) + Ivl::from(b[1]) * t);
            profile = profile.hull(Ivl::from(a[0]) + Ivl::from(b[0]) * w);
        }
        let angles = definition.mathematical_turn_range(turn)?;
        let source = (profile - Ivl::from(self.source.1[0]))
            / (Ivl::from(self.source.1[1]) - Ivl::from(self.source.1[0]));
        let section =
            ScrewOffsetSection::new(self.source.0[0], self.source.0[1], &self.motion, 0.0)?;
        let bounds = section.meridian_bounds(source.lo, source.hi, self.radial)?;
        let error = (Ivl::from(bounds.radius.hi)
            * Ivl::from((angles - Ivl::from(self.station) + bounds.phase).mag())
            + Ivl::from(section.unit_axis_orbit_deviation(source, angles)?)
            + Ivl::from(definition.runtime_evaluation_allowance()?))
        .hi;
        if !error.is_finite() || error > tolerance {
            return Err(bad(
                "original wall trim does not certify its authored station",
            ));
        }
        Ok(())
    }

    /// Positive-weight pcurve controls enclose its complete angular range.
    /// The cylinder's normalized polar polynomial is bounded on every covered
    /// span and over the full affine/clamped height range, including pcurve and
    /// checked point-evaluation rounding. No constant-angle equality is assumed.
    /// Whether `surface` is a literal cylinder coaxial with this separator's screw: the parent
    /// [`Self::certify_cylinder_uv`] can certify. A tapered cone, or a cylinder on another axis, is
    /// not one.
    pub fn is_coaxial_cylinder(&self, surface: &Surface) -> Result<bool> {
        let SurfaceKind::Cone(cone) = surface.kind() else {
            return Ok(false);
        };
        Ok(cone
            .coaxial_radius_enclosure(self.origin, self.motion.axis())?
            .is_some())
    }

    pub fn certify_cylinder_uv(
        &self,
        k: &Kernel,
        surface: &Surface,
        uv: &Curve,
        tolerance: f64,
    ) -> Result<CylinderCorner> {
        let SurfaceKind::Cone(cone) = surface.kind() else {
            return Err(bad("seam endpoint has no cylinder parent"));
        };
        let radius = cone
            .coaxial_radius_enclosure(self.origin, self.motion.axis())?
            .ok_or_else(|| bad("seam parent is not a proved literal coaxial cylinder"))?;
        let cap = cone
            .cylinder_runtime_allowance(k)?
            .ok_or_else(|| bad("uncertified cylinder seam parent"))?;
        let points = controls(uv)?;
        let spline = uv
            .b_spline_base()
            .ok_or_else(|| bad("missing cylinder parent pcurve"))?;
        let raw = spline.control_points()?;
        let [p0, p1] = raw else {
            return Err(bad("cylinder parent needs one linear rational span"));
        };
        let rational = spline.is_rational()?;
        let ideal = [p0, p1].map(|p| {
            [
                Ivl::from(p.x),
                Ivl::from(p.y),
                Ivl::from(if rational { p.w } else { 1.0 }),
            ]
        });
        // The ideal is the literal stored homogeneous line. The shared bound
        // encloses its actual basis, weighted sum and rational division, so the
        // rectangle covers both the mathematical trace and evaluated UV points.
        let rounding = certificate::mapped_error(uv, &[ideal], ([0.0; 2], [1.0; 2], false))?;
        let swapped = cone.canonical().swap_uv;
        let mut angular: Option<Ivl> = None;
        let mut axial: Option<Ivl> = None;
        for [x, y, _] in points {
            let (angle, height) = if swapped { (y, x) } else { (x, y) };
            angular = Some(angular.map_or(angle, |prior| prior.hull(angle)));
            axial = Some(axial.map_or(height, |prior| prior.hull(height)));
        }
        let angular = angular.ok_or_else(|| bad("empty cylinder separator pcurve"))?;
        let [u_error, v_error] = rounding;
        let error = if swapped { v_error } else { u_error };
        let angular = angular + Ivl::new(-error, error);
        let domain = cone.rev().base().natural_uv_domain();
        let height = if swapped {
            domain.u_interval()
        } else {
            domain.v_interval()
        };
        let height_error = if swapped { u_error } else { v_error };
        let axial = axial.ok_or_else(|| bad("empty cylinder separator pcurve"))?
            + Ivl::new(-height_error, height_error);
        let height = Ivl::new(height.min(), height.max()).hull(axial);
        let patch = if swapped {
            [height, angular]
        } else {
            [angular, height]
        };
        let point = cone
            .cylinder_patch_enclosure(k, patch)?
            .ok_or_else(|| bad("unsupported cylinder separator chart"))?;
        let error = Ivl::new(-cap, cap);
        let plane_error = self.point_plane(point.map(|v| v + error), tolerance)?;
        // Positive half-plane: |angle| <= 2*|sin(angle)|. The orbit
        // displacement is bounded by r*|angle| + |pitch/(2pi)|*|angle|;
        // this includes the axial phase change, not just the radial chord.
        let rise = Ivl::from(self.motion.pitch().abs())
            / Ivl::checked(std::f64::consts::TAU, std::f64::consts::TAU);
        let meridian_error = (Ivl::from(2.0)
            * Ivl::from(plane_error)
            * (Ivl::from(1.0) + rise / Ivl::from(radius.lo)))
        .hi;
        if !meridian_error.is_finite() {
            return Err(bad("cylinder parent meridian enclosure overflowed"));
        }
        Ok(CylinderCorner {
            radius,
            meridian_error,
        })
    }

    /// The corner where a seam ends inside one literal coaxial cylinder face, with no edge
    /// of the cylinder at its endpoint (a thread's crest on stock whose own seam lies at
    /// another angle): the meridian half-plane cuts the cylinder in one ruling at its
    /// radius, wherever the face's angular range lies, so the seam meets it there and no
    /// parent edge has to be certified in the half-plane. The cylinder's evaluated points
    /// lie within its checked runtime allowance of the literal one, so its radius
    /// enclosure is widened by that allowance; with no parent edge there is no meridian
    /// discrepancy.
    pub fn certify_cylinder_face(&self, k: &Kernel, surface: &Surface) -> Result<CylinderCorner> {
        let SurfaceKind::Cone(cone) = surface.kind() else {
            return Err(bad("seam endpoint has no cylinder parent"));
        };
        let radius = cone
            .coaxial_radius_enclosure(self.origin, self.motion.axis())?
            .ok_or_else(|| bad("seam parent is not a proved literal coaxial cylinder"))?;
        let cap = cone
            .cylinder_runtime_allowance(k)?
            .ok_or_else(|| bad("uncertified cylinder seam parent"))?;
        if !cap.is_finite() || cap < 0.0 {
            return Err(bad("uncertified cylinder seam parent"));
        }
        Ok(CylinderCorner {
            radius: radius + Ivl::new(-cap, cap),
            meridian_error: 0.0,
        })
    }

    fn target<'a>(
        &self,
        surface: &'a Surface,
        domain: &crate::geom::extent2d::Extent2d,
    ) -> Result<Wall<'a>> {
        let wall =
            Wall::new(surface, domain)?.ok_or_else(|| bad("missing continued screw seam wall"))?;
        let source = wall
            .definition
            .retained_affine_source()?
            .ok_or_else(|| bad("missing continued seam source"))?;
        let motion = wall
            .definition
            .screw()
            .ok_or_else(|| bad("missing continued seam motion"))?;
        if source.0 != self.source.0
            || source.1 != self.source.1
            || !same_motion(motion, &self.motion)
        {
            return Err(bad("continued seam changed its exact generator or motion"));
        }
        Ok(wall)
    }

    pub fn construct(
        &self,
        k: &Kernel,
        first: (&Surface, &crate::geom::extent2d::Extent2d),
        second: (&Surface, &crate::geom::extent2d::Extent2d),
        corners: [CylinderCorner; 2],
        tolerance: f64,
    ) -> Result<SurfaceIntersection> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(bad("invalid paired seam tolerance"));
        }
        if !self.supports_targets(first.0, second.0)? {
            return Err(bad("seam targets are not a moved coincident pair"));
        }
        let a = self.target(first.0, first.1)?;
        let b = self.target(second.0, second.1)?;
        if a.definition.retained_affine_source()? != b.definition.retained_affine_source()? {
            return Err(bad("coincident seam targets have different signed offsets"));
        }
        let range = Ivl::new(
            a.profile.lo.max(b.profile.lo),
            a.profile.hi.min(b.profile.hi),
        );
        if range.lo >= range.hi {
            return Err(bad("continued seam has no common profile support"));
        }
        let corner_error = self
            .certify_corners(&a, range, corners)?
            .max(self.certify_corners(&b, range, corners)?);
        let (ua, ea) = self.pcurve(&a, range, tolerance * 0.5)?;
        let (ub, eb) = self.pcurve(&b, range, tolerance * 0.5)?;
        // ea+eb bounds the two stored wall traces relative to the same ideal
        // meridian. corner_error separately bounds differing literal parent
        // radii and polar meridians; no parent discrepancy is called equality.
        let deviation = (Ivl::from(ea) + Ivl::from(eb) + Ivl::from(corner_error)).hi;
        if deviation > tolerance {
            return Err(bad("paired meridian seam exceeds its tolerance"));
        }
        let mut out = SurfaceIntersection::default();
        super::append_pair(
            k,
            &a,
            ua,
            ub,
            TsectCurveType::RegionBoundary,
            deviation,
            &mut out,
        )?;
        Ok(out)
    }

    /// A fixed nonzero radial derivative and strict endpoint signs prove,
    /// by the intermediate value theorem, exactly one root for each literal
    /// cylinder radius. Disjoint radius enclosures order the two corners.
    fn certify_corners(
        &self,
        wall: &Wall<'_>,
        range: Ivl,
        corners: [CylinderCorner; 2],
    ) -> Result<f64> {
        let radii = corners.map(|corner| corner.radius);
        let [r0, r1] = radii;
        if radii
            .iter()
            .any(|r| !r.lo.is_finite() || !r.hi.is_finite() || r.lo <= 0.0 || r.lo > r.hi)
            || !(r0.hi < r1.lo || r1.hi < r0.lo)
        {
            return Err(bad("seam cylinder corners have no strict radial order"));
        }
        let whole = wall
            .section
            .meridian_bounds(range.lo, range.hi, self.radial)?;
        if whole.radius_rate.mig() <= 0.0 {
            return Err(bad("seam cylinder roots have no monotone radial chart"));
        }
        let left = wall
            .section
            .meridian_bounds(range.lo, range.lo, self.radial)?
            .radius;
        let right = wall
            .section
            .meridian_bounds(range.hi, range.hi, self.radial)?
            .radius;
        let rate = Ivl::from(whole.radius_rate.mig());
        let dr = Ivl::from(whole.radius_rate.mag());
        // Meridian height is generator axial height minus h*phase: its
        // derivative includes the seam's axial phase correction.
        let dg = Ivl::from(whole.height_rate.mag());
        let speed = (dr * dr + dg * dg).sqrt();
        let mut error = 0.0_f64;
        for corner in corners {
            let radius = corner.radius;
            if !corner.meridian_error.is_finite() || corner.meridian_error < 0.0 {
                return Err(bad("invalid cylinder corner discrepancy"));
            }
            let bracketed = if whole.radius_rate.lo > 0.0 {
                left.hi < radius.lo && radius.hi < right.lo
            } else {
                right.hi < radius.lo && radius.hi < left.lo
            };
            if !bracketed {
                return Err(bad(
                    "continued seam does not strictly bracket its cylinder corner",
                ));
            }
            let radial_gap = (Ivl::from(radius.hi) - Ivl::from(radius.lo)) / rate * speed;
            error = error.max((radial_gap + Ivl::from(2.0) * Ivl::from(corner.meridian_error)).hi);
        }
        if !error.is_finite() {
            return Err(bad("cylinder corner discrepancy overflowed"));
        }
        Ok(error)
    }

    fn chart(&self, wall: &Wall<'_>, range: Ivl) -> Result<(f64, f64)> {
        let bounds = wall.section.meridian_family_bounds(
            range.lo,
            range.hi,
            self.radial,
            Ivl::from(
                wall.definition
                    .retained_affine_source()?
                    .ok_or_else(|| bad("missing seam offset"))?
                    .2,
            ),
        )?;
        if bounds.oriented_area.lo <= 0.0 || bounds.section.radius_rate.mig() <= 0.0 {
            return Err(bad("seam regularity or radial order is unproved"));
        }
        let angle = Ivl::from(self.station) - bounds.section.phase;
        wall.definition
            .stations()
            .windows(2)
            .find_map(|p| match p {
                [a, b] if *a <= angle.lo && angle.hi <= *b => Some((*a, *b)),
                _ => None,
            })
            .ok_or_else(|| bad("meridian seam crosses an unpartitioned rational station"))
    }

    fn jet(&self, wall: &Wall<'_>, range: Ivl, span: (f64, f64)) -> Result<Tay<Ivl>> {
        let constant = |x| Tay::of_terms(x, Ivl::from(0.0));
        let q = wall.section.meridian_half_angle_jet(range, self.radial)?;
        let width = Ivl::from(span.1) - Ivl::from(span.0);
        let mid = (Ivl::from(span.0) + Ivl::from(span.1)) / Ivl::from(2.0);
        let phase = certificate::tan((Ivl::from(self.station) - mid) / Ivl::from(2.0))?;
        let denominator = constant(Ivl::from(1.0)) - constant(phase) * q;
        if denominator.at(0).lo <= 0.0 {
            return Err(bad("meridian seam inverse has no positive denominator"));
        }
        Ok(constant(mid)
            + constant(width / (Ivl::from(2.0) * certificate::tan(width / Ivl::from(4.0))?))
                * (q + constant(phase))
                / denominator)
    }

    fn pcurve(&self, wall: &Wall<'_>, range: Ivl, tolerance: f64) -> Result<(Curve, f64)> {
        let span = self.chart(wall, range)?;
        let mut pending = vec![(range, 0_u32)];
        let mut cells = Vec::new();
        let mut budget = 4096_usize;
        let [_, speed] = wall.section.speed_upper_bounds(range)?;
        let width = Ivl::from(span.1) - Ivl::from(span.0);
        let rate = Ivl::from(4.0) * certificate::tan(width / Ivl::from(4.0))? / width;
        while let Some((cell, depth)) = pending.pop() {
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| bad("seam pcurve exhausted its cell budget"))?;
            let jet = self.jet(wall, cell, span)?;
            let dx = Ivl::from(cell.hi) - Ivl::from(cell.lo);
            let error = (Ivl::from(jet.at(2).mag()) * dx * dx / Ivl::from(4.0)).hi;
            if (Ivl::from(error) * Ivl::from(speed) * rate).hi <= tolerance * 0.5 {
                cells.push((cell, error));
                continue;
            }
            let mid = cell.mid();
            if depth >= 32 || !(cell.lo < mid && mid < cell.hi) {
                return Err(bad("meridian seam interpolation is unresolved"));
            }
            pending.push((Ivl::new(mid, cell.hi), depth + 1));
            pending.push((Ivl::new(cell.lo, mid), depth + 1));
        }
        let mut points = Vec::new();
        let mut knots = Vec::new();
        let mut ideal = Vec::new();
        let mut error = 0.0_f64;
        for (cell, local) in cells {
            let mut endpoints = Vec::new();
            for x in [cell.lo, cell.hi] {
                let w = Ivl::from(wall.source[0])
                    + (Ivl::from(wall.source[1]) - Ivl::from(wall.source[0])) * Ivl::from(x);
                let t = self.jet(wall, Ivl::from(x), span)?.at(0);
                let uv = wall
                    .definition
                    .face_parameters(Point2::new(w.mid(), t.mid()))?;
                points.push(Point3::new(uv.x, uv.y, 0.0));
                endpoints.push(certificate::point(w, t, Ivl::from(1.0)));
            }
            let [p, q] = endpoints.as_slice() else {
                return Err(bad("missing seam endpoint"));
            };
            ideal.push([*p, *q]);
            knots.push(cell.lo);
            error = error.max(local);
        }
        knots.push(range.hi);
        let multiplicities = vec![2; knots.len()];
        let curve = uv_curve(&points, None, &knots, &multiplicities)?;
        let deviation = certificate::wall_error_with_deviation(wall, &curve, &ideal, [0.0, error])?;
        if !deviation.is_finite() || deviation > tolerance {
            return Err(bad("stored meridian pcurve exceeds its tolerance"));
        }
        Ok((curve, deviation))
    }
}
