//! Exact screw-offset definitions and their certified support nets (#2724).
//!
//! New nets are constructed from the offset generator, never by offsetting an old
//! display net. Continued supports preserve the original generator parameter map.
//! Deviations: no upstream counterpart. Arc refinement is iterative and bounded by
//! the screw builder's 12,000-station budget; no recursion.

use std::f64::consts::TAU;

use crate::blend::fillet_section::interval::Ivl;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::math::{Point2, Vec3};
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::surface::surface::Surface;
use crate::sweep::screw::MAX_STATIONS;

use super::offset_profile::{affine_source, OffsetProfile};
use super::screw_offset::ScrewOffsetSection;
use super::{lift_error_bound, Motion, ProceduralSweep};

const OP: &str = "surface::procedural_sweep::offset";

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn finite_interval(range: [f64; 2]) -> bool {
    let [a, b] = range;
    a.is_finite() && b.is_finite() && a < b
}

/// Refines from the analytical error, before a net is constructed. Old stations
/// remain present, preserving angular seams and each original span's parameters.
fn refine_stations(
    stations: &[f64],
    original: &[f64],
    pitch: f64,
    target: f64,
) -> Result<Vec<f64>> {
    let first = stations
        .first()
        .copied()
        .ok_or_else(|| bad("empty screw stations"))?;
    let mut result = vec![first];
    let lambda = Ivl::from(pitch.abs()) / Ivl::from(TAU);
    for pair in stations.windows(2) {
        let [a, b] = *pair else {
            continue;
        };
        if !finite_interval([a, b]) {
            return Err(bad("invalid screw station interval"));
        }
        // Subdividing with newly unit-normalised arc weights changes theta(t).
        // Existing spans must retain that parameter map, including their old trims.
        let existing = original
            .binary_search_by(|x| x.total_cmp(&a))
            .is_ok_and(|i| original.get(i + 1).copied() == Some(b));
        if existing {
            result.push(b);
            continue;
        }
        let mut pieces = 1_usize;
        loop {
            let width = (b - a) / pieces as f64;
            let bound = (lambda * Ivl::from(lift_error_bound((width * 0.25).tan()))).hi;
            if width * 0.5 <= super::MAX_HALF_ANGLE && bound.is_finite() && bound <= target {
                break;
            }
            pieces = pieces
                .checked_mul(2)
                .filter(|&n| n as f64 <= MAX_STATIONS)
                .ok_or_else(|| bad("the screw lift cannot meet its bounded station budget"))?;
        }
        if result
            .len()
            .checked_add(pieces)
            .is_none_or(|n| n as f64 > MAX_STATIONS + 1.0)
        {
            return Err(bad("the screw offset exceeds its station budget"));
        }
        for i in 1..=pieces {
            result.push(if i == pieces {
                b
            } else {
                a + (b - a) * i as f64 / pieces as f64
            });
        }
    }
    Ok(result)
}

fn extended_stations(stations: &[f64], range: [f64; 2]) -> Result<Vec<f64>> {
    if !finite_interval(range) {
        return Err(bad("invalid continued turn interval"));
    }
    let (Some(&first), Some(&last)) = (stations.first(), stations.last()) else {
        return Err(bad("empty screw stations"));
    };
    let mut result = Vec::new();
    if range[0] < first {
        result.push(range[0]);
    }
    result.extend_from_slice(stations);
    if range[1] > last {
        result.push(range[1]);
    }
    Ok(result)
}

impl ProceduralSweep {
    /// Whether this motion is invariant under one fixed screw group. A constant
    /// radial shift only relocates its generator; a varying radial law is a
    /// different geometric class and keeps the general surface-operation path.
    #[must_use]
    pub fn is_screw_invariant(&self) -> bool {
        self.screw().is_some_and(|motion| {
            motion.law().is_none_or(|law| {
                law.offsets()
                    .first()
                    .is_some_and(|first| law.offsets().iter().all(|v| v == first))
            })
        })
    }

    /// Whether both definitions are the same exact screw surface with the same
    /// face parameters wherever their turn intervals overlap. Fitted profiles and
    /// support bounds may differ; the retained source, distance, motion, affine
    /// parameter map and every shared rational arc span must agree exactly.
    /// This is suitable for preserving pcurves across a support extension.
    #[must_use]
    pub fn same_screw_parameterization(&self, other: &Self) -> bool {
        let (Some(a), Some(b)) = (self.screw(), other.screw()) else {
            return false;
        };
        if a != b || self.map != other.map {
            return false;
        }
        let source = |s: &Self| {
            let (profile, distance) = s.offset.as_ref().map_or((&s.profile, 0.0), |offset| {
                (&offset.source, offset.distance)
            });
            super::super::procedural_blend::convert::curve_record(profile)
                .ok()
                .map(|record| (record, distance))
        };
        let (Some(a), Some(b)) = (source(self), source(other)) else {
            return false;
        };
        if a != b {
            return false;
        }
        let (Some(&af), Some(&al), Some(&bf), Some(&bl)) = (
            self.stations.first(),
            self.stations.last(),
            other.stations.first(),
            other.stations.last(),
        ) else {
            return false;
        };
        let lo = af.max(bf);
        let hi = al.min(bl);
        if lo >= hi {
            return false;
        }
        let intersects = |pair: &&[f64]| matches!(pair, [a,b] if *a < hi && *b > lo);
        self.stations
            .windows(2)
            .filter(intersects)
            .eq(other.stations.windows(2).filter(intersects))
    }

    /// The exact affine screw section and its original profile parameter interval.
    /// `x=(w-w0)/(w1-w0)` maps that interval to the section's line parameter.
    /// `None` identifies a different geometric class, never an approximation:
    /// a varying radial law, a non-affine source, or rational source controls
    /// (which the runtime certificate cannot take as exact).
    ///
    /// # Errors
    /// Those of inspecting the source or constructing its exact section.
    pub fn screw_offset_section(&self) -> Result<Option<(ScrewOffsetSection, [f64; 2])>> {
        let Some(motion) = self.screw() else {
            return Ok(None);
        };
        if motion.law().is_some_and(|law| {
            law.offsets()
                .first()
                .is_none_or(|first| law.offsets().iter().any(|x| x != first))
        }) {
            return Ok(None);
        }
        // The exact section must agree with the runtime certificate, which
        // refuses rational source controls (equal weights included).
        if !self.checked_source_class()? {
            return Ok(None);
        }
        if let Some(offset) = &self.offset {
            return offset.section(motion).map(Some);
        }
        let Some(([start, end], domain)) = affine_source(&self.profile)? else {
            return Ok(None);
        };
        Ok(Some((
            ScrewOffsetSection::new(start, end, motion, 0.0)?,
            domain,
        )))
    }

    /// Whether this definition retains an exact normal-offset source.
    #[must_use]
    pub const fn is_normal_offset(&self) -> bool {
        self.offset.is_some()
    }

    /// Converts definition coordinates back to the face's parameter coordinates.
    ///
    /// # Errors
    /// Refuses a nonfinite or singular parameter map.
    pub fn face_parameters(&self, wt: Point2) -> Result<Point2> {
        let [a, c] = self.map.a;
        let [b, d] = self.map.b;
        if ![a, b, c, d, wt.x, wt.y].iter().all(|x| x.is_finite()) || b == 0.0 || d == 0.0 {
            return Err(bad(
                "a screw definition requires a finite nonsingular parameter map",
            ));
        }
        let p = (wt.x - a) / b;
        let q = (wt.y - c) / d;
        if !p.is_finite() || !q.is_finite() {
            return Err(bad("screw parameter mapping overflowed"));
        }
        Ok(if self.map.swapped {
            Point2::new(q, p)
        } else {
            Point2::new(p, q)
        })
    }

    /// The net parameter for a physical screw angle, inverse to [`Self::turn_at`].
    ///
    /// # Errors
    /// Refuses a non-screw definition, an angle outside its stations or invalid data.
    pub fn parameter_at_angle(&self, angle: f64) -> Result<f64> {
        if self.screw().is_none() || !angle.is_finite() {
            return Err(bad("invalid screw angle"));
        }
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(bad("empty screw stations"));
        };
        if angle < first || angle > last {
            return Err(bad("angle outside the screw support"));
        }
        if angle == first || angle == last {
            return Ok(angle);
        }
        let i = self
            .stations
            .partition_point(|&t| t <= angle)
            .saturating_sub(1);
        let (Some(&a), Some(&b)) = (self.stations.get(i), self.stations.get(i + 1)) else {
            return Err(bad("missing screw span"));
        };
        let mid = 0.5 * (a + b);
        let tau = ((b - a) * 0.25).tan();
        Ok(mid + 0.5 * (b - a) * ((angle - mid) * 0.5).tan() / tau)
    }

    fn definition_box(&self, face: &Extent2d) -> Result<Extent2d> {
        if self.map.b.iter().any(|b| !b.is_finite() || *b == 0.0) {
            return Err(bad("invalid screw support parameter map"));
        }
        let u = Ivl::new(face.u_min(), face.u_max());
        let v = Ivl::new(face.v_min(), face.v_max());
        let (p, q) = if self.map.swapped { (v, u) } else { (u, v) };
        let w = Ivl::from(self.map.a[0]) + Ivl::from(self.map.b[0]) * p;
        let t = Ivl::from(self.map.a[1]) + Ivl::from(self.map.b[1]) * q;
        let result = Extent2d::from_bounds(w.lo, t.lo, w.hi, t.hi);
        if !finite_interval([result.u_min(), result.u_max()])
            || !finite_interval([result.v_min(), result.v_max()])
        {
            return Err(bad("invalid screw support parameter domain"));
        }
        Ok(result)
    }

    /// Offsets this exact screw wall along the face's oriented parameter normal.
    /// The source's normal is used even when the current face has been reversed,
    /// swapped or reparameterised. The returned surface carries its exact definition.
    ///
    /// # Errors
    /// Refuses unsupported non-affine or non-invariant sections, nonfinite distance,
    /// and a tolerance not certified within the bounded section/station budgets.
    pub fn normal_offset(
        &self,
        net: &BSplineSurface,
        distance: f64,
        tolerance: f64,
    ) -> Result<Surface> {
        if !distance.is_finite() {
            return Err(bad("a screw offset distance must be finite"));
        }
        self.definition_box(&net.natural_uv_domain())?;
        // A trim or reparameterisation does not change the retained support.
        // Mapping its rounded face endpoints back would create artificial ulp-wide
        // end spans. Build on the definition's own exact profile/station bounds;
        // the original face loops still select the part of this support they use.
        let profile = self.profile.natural_interval();
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(bad("empty screw support stations"));
        };
        let domain = Extent2d::from_bounds(profile.min(), first, profile.max(), last);
        let [b0, b1] = self.map.b;
        let orientation = b0.signum() * b1.signum() * if self.map.swapped { -1.0 } else { 1.0 };
        let distance = self.offset.as_ref().map_or(0.0, |s| s.distance) + orientation * distance;
        self.offset_support(domain, distance, tolerance)
    }

    fn offset_support(&self, domain: Extent2d, distance: f64, tolerance: f64) -> Result<Surface> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(bad("a screw net tolerance must be positive and finite"));
        }
        if self.screw_offset_section()?.is_none() {
            return Err(bad(
                "normal offsets require an invariant affine screw section",
            ));
        }
        let Motion::Screw(original_motion) = &self.motion else {
            return Err(bad("a path sweep is not a screw offset"));
        };
        let mut motion = original_motion.clone();
        let mut source = self
            .offset
            .as_ref()
            .map_or_else(|| self.profile.copy(), |s| s.source.copy());
        // A constant radial shift is absorbed once into the retained source; the
        // resulting motion has no radial shift, so C is never displaced twice.
        if let Some(law) = motion.law.take() {
            let first = law
                .offsets()
                .first()
                .copied()
                .ok_or_else(|| bad("empty radial law"))?;
            if law.offsets().iter().any(|&d| d != first) {
                return Err(bad("a varying radial law has no invariant offset section"));
            }
            let placement = Axis2Placement::from_stored(
                law.direction() * first,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            );
            source.transform(&placement, None)?;
        }
        let stations = extended_stations(&self.stations, [domain.v_min(), domain.v_max()])?;
        let stations =
            refine_stations(&stations, &self.stations, motion.pitch(), tolerance * 0.25)?;
        let half = stations
            .windows(2)
            .fold(0.0_f64, |largest, pair| match pair {
                [a, b] => largest.max((b - a) * 0.5),
                _ => largest,
            });
        // Allocate independent profile and lift budgets up front. The complete
        // result is accepted only after every error term is re-certified below.
        let budget = (Ivl::from(tolerance) * Ivl::from(0.25)).lo;
        if !(budget > 0.0 && budget.is_finite()) {
            return Err(bad("no positive finite offset construction budget"));
        }
        let (profile, offset) = OffsetProfile::build(
            &source,
            &motion,
            distance,
            [domain.u_min(), domain.u_max()],
            budget,
        )?;
        let mut definition = Self::create(&profile, motion, &stations, half)?;
        definition.invalidate_runtime_allowance();
        definition.offset = Some(offset);
        definition.map = self.map;
        let (expected, pieces) = super::refined_net::build_refined(&definition, &profile, budget)?;
        let mut net = expected.copy();
        let [a0, a1] = self.map.a;
        let [b0, b1] = self.map.b;
        let original = net.natural_uv_domain();
        let p = [(original.u_min() - a0) / b0, (original.u_max() - a0) / b0];
        let q = [(original.v_min() - a1) / b1, (original.v_max() - a1) / b1];
        if b0 < 0.0 {
            net.reverse(SurfParamType::U)?;
        }
        if b1 < 0.0 {
            net.reverse(SurfParamType::V)?;
        }
        net.reparameterize(&Extent2d::from_bounds(
            p[0].min(p[1]),
            q[0].min(q[1]),
            p[0].max(p[1]),
            q[0].max(q[1]),
        ))?;
        if self.map.swapped {
            net.swap_uv()?;
        }
        definition.bound = super::refined_net::certify_refined(&definition, &net, pieces)?;
        if definition.bound > tolerance {
            return Err(bad(
                "the complete screw offset net is not within its requested tolerance",
            ));
        }
        let mut surface = Surface::from(net);
        surface.set_sweep_definition(definition);
        Ok(surface)
    }
}
