//! A cone or cylinder continued naturally past one side of a face's parameter box: the
//! analytic itself over a wider STEP domain, so a sheet extended past a side keeps its kind.
//!
//! Not upstream: SolidCraft's composition (`cone_continued`), moved into the kernel so the
//! modelling layer computes no geometry (docs/layers.md §1). It is an exact class at bound 0
//! (owner rule 13564): the result is a [`Cone`] with the source's placement, radius and
//! semi-angle (its label), and its NURBS form is proved on that one implicit surface
//! ([`AnalyticSupport::check`]); a continuation is refused unless both hold.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::nurbs::math::gamma;
use crate::surface::cone::checked::HeightMap;
use crate::surface::cone::Cone;
use crate::surface::continued::{at_most_a_turn, step_rows, widen};
use crate::surface::extend::{Continuity, ExtendRefusal};
use crate::surface::implicit::AnalyticSupport;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::cone::Cone::continued";

impl Cone {
    /// Continue a full cylinder whose native height map is literally `h(v)=v`,
    /// retaining both that map and the original rational polar converter.
    ///
    /// The ordinary analytic extension already constructs the continued net on
    /// the cylinder, but returns a bare B-spline. Rebuilding a canonical cylinder
    /// from its enlarged box would change the polar parameters. Here only the
    /// height interval changes, identically in STEP and native coordinates; the
    /// real affine conversion remains `a+(v-a)/(b-a)*(b-a)=v`. Placement, radius,
    /// polar coefficients and conversion metadata are copied exactly. Thus every
    /// retained UV trim still names the same mathematical point. Native arithmetic
    /// is covered by the cylinder's checked runtime point allowance.
    ///
    /// A cylinder read back from a record has its STEP height from its net's end rows
    /// and its native height from its knots, which can differ by rounding (an M10
    /// thread's root, `[0, 1.6894999999999998]` against `[0, 1.6895]`): its height map is
    /// then an affine map a few ulps from the identity. Its continuation keeps that map:
    /// the new STEP ends are the source's converter applied to the new native ends, and
    /// the two maps' exact coefficients ([`HeightMap::exact_coefficients`]) are enclosed
    /// over the whole continued height. The continuation is refused unless the farthest
    /// they move a native height along the axis is within the cylinder's checked runtime
    /// allowance, the bound its checked evaluation already holds a native point to; so
    /// every retained UV trim names a point of the source's within that same allowance.
    ///
    /// `None` denotes a different parameterization or an unproved analytic label,
    /// before construction. A failed continuation is an error, not a fallback.
    pub(crate) fn extended_cylinder_keeping_parameters(
        &self,
        k: &Kernel,
        distance: f64,
    ) -> Result<Option<Self>> {
        const OP_MAP: &str = "surface::cone::Cone::extended_cylinder_keeping_parameters";
        if !distance.is_finite() || distance <= 0.0 {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "invalid cylinder continuation distance",
            ));
        }
        let step = self.rev.step_uv_domain();
        let native = self.rev.base.natural_uv_domain();
        let (angle, height) = if self.rev.swap_uv {
            (native.v_interval(), native.u_interval())
        } else {
            (native.u_interval(), native.v_interval())
        };
        if self.semi_angle_deg != 0.0
            || self.inside_out
            || step.u_min() != 0.0
            || step.u_max() != 360.0
            || !self.rev.is_bounded()?
            || self.rev.polar_curve().is_none()
            || !matches!(
                self.rev.gen_curve.as_ref().map(Curve::kind),
                Some(CurveKind::Line(_))
            )
        {
            return Ok(None);
        }
        let mut exact_source = self.clone();
        exact_source.rev = self.rev.clone_exact();
        let source = Surface::from(exact_source);
        if !AnalyticSupport::label_stands(k, &source)? {
            return Ok(None);
        }
        let extension = source.extended_sides_keeping_parameters(
            k,
            &BoxSide::ALL,
            Continuity::Natural,
            distance,
        )?;
        let SurfaceKind::BSpline(form) = extension.surface.into_kind() else {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "cylinder height continuation has no extended net",
            ));
        };
        let extended = form.natural_uv_domain();
        let (new_angle, new_height) = if self.rev.swap_uv {
            (extended.v_interval(), extended.u_interval())
        } else {
            (extended.u_interval(), extended.v_interval())
        };
        if new_angle != angle
            || !new_height.min().is_finite()
            || !new_height.max().is_finite()
            || new_height.min() >= height.min()
            || new_height.max() <= height.max()
        {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "cylinder continuation changed its angular chart or failed to extend its height",
            ));
        }
        // the identity height map stays the identity; any other keeps its own map, the
        // source's converter giving the new STEP ends
        let new_step = if height == step.v_interval() {
            new_height
        } else {
            let at = |h: f64| -> Result<f64> {
                let native = if self.rev.swap_uv {
                    Point2::new(h, angle.min())
                } else {
                    Point2::new(angle.min(), h)
                };
                Ok(self.convert_uv_from_nurbs_to_step(k, native)?.y)
            };
            let (low, high) = (at(new_height.min())?, at(new_height.max())?);
            if !(low.is_finite() && high.is_finite() && low < high) {
                return Err(SolidError::invalid_input(
                    OP_MAP,
                    "cylinder height continuation has no ordered STEP height",
                ));
            }
            Extent1d::from_min_max(low, high)
        };
        let mut result = self.clone();
        result.rev = self.rev.clone_exact();
        result.rev.base = form;
        result.rev.anal_uv_domain =
            Extent2d::from_bounds(step.u_min(), new_step.min(), step.u_max(), new_step.max());
        let Some(CurveKind::Line(generator)) = result.rev.gen_curve.as_mut().map(Curve::kind_mut)
        else {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "cylinder continuation lost its linear generator",
            ));
        };
        generator.adjust_step_interval(&new_step)?;
        if generator.natural_interval() != new_step {
            return Err(SolidError::invalid_input(
                OP_MAP,
                "cylinder generator did not retain its height interval",
            ));
        }
        if new_step != new_height {
            result.certify_height_map_kept(k, self, new_height)?;
        }
        AnalyticSupport::check(k, &Surface::from(result.clone()))?;
        Ok(Some(result))
    }

    /// That this continuation's height map moves no native height of `native` along the
    /// axis farther from where `source`'s map puts it than this cylinder's checked
    /// runtime allowance: both maps' exact affine coefficients enclosed, their difference
    /// bounded over the whole interval, times the axis's length.
    fn certify_height_map_kept(&self, k: &Kernel, source: &Self, native: Extent1d) -> Result<()> {
        const OP_KEPT: &str = "surface::cone::Cone::extended_cylinder_keeping_parameters";
        let [o0, s0] = HeightMap::new(k, source)?.exact_coefficients();
        let [o1, s1] = HeightMap::new(k, self)?.exact_coefficients();
        let span = Ivl::new(native.min(), native.max());
        let shift = ((o1 - o0) + (s1 - s0) * span).mag();
        let axis = self.canonical().origin.z_axis();
        let length = (Ivl::from(axis.x) * Ivl::from(axis.x)
            + Ivl::from(axis.y) * Ivl::from(axis.y)
            + Ivl::from(axis.z) * Ivl::from(axis.z))
        .sqrt();
        let moved = (Ivl::from(shift) * length).hi;
        let cap = self.cylinder_runtime_allowance(k)?.ok_or_else(|| {
            SolidError::invalid_input(OP_KEPT, "continued cylinder has no checked allowance")
        })?;
        if !moved.is_finite() || moved > cap {
            return Err(SolidError::invalid_input(
                OP_KEPT,
                "cylinder continuation moved its height map past its checked allowance",
            ));
        }
        Ok(())
    }

    /// The cone continued naturally `distance` past side `side` of the face box `dom` (in the
    /// cone's NURBS parameters): its STEP domain widened at that side, by the angle
    /// `distance / r` (degrees) where the side runs along the axis (`r` the radius at the
    /// side's middle), or by the height `distance cos(semi-angle)` where it runs round it
    /// (so the slant length added is `distance`). The rest of the domain is `dom`'s.
    ///
    /// Which STEP coordinate the side holds constant follows from the parametrisation (the
    /// NURBS u is the height when the cone's uv is swapped), not from a measurement. A
    /// sweep that comes out over a turn by no more than the rounding of the sum is a full
    /// turn.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a distance not finite and positive, a side along the axis at a
    /// radius not positive, a sweep proved over a turn, a height past the apex (from
    /// [`Cone::adjust_step_uv_domain`]), or a result whose label or NURBS form is not the
    /// source's surface; those of the parameter conversions and of
    /// [`AnalyticSupport::check`].
    pub fn continued(
        &self,
        k: &Kernel,
        dom: &Extent2d,
        side: BoxSide,
        distance: f64,
    ) -> Result<Self> {
        if !(distance.is_finite() && distance > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "the distance is not finite and positive",
            ));
        }
        // the source an exact class first: a form moved off its label is refused, not remade
        AnalyticSupport::check(k, &Surface::from(self.clone()))?;
        let step = |uv| self.convert_uv_from_nurbs_to_step(k, uv);
        let swap = self.canonical().swap_uv;
        let rows = step_rows(&step, dom, side, swap)?;
        let (mut angle, mut height) = (rows.sweep, rows.generator);
        // the side holds the NURBS u constant for a u side; that is the height when swapped
        let in_height = side.is_u() == swap;
        if in_height {
            let theta = self.semi_angle_deg().to_radians();
            let old = height;
            let high = widen(&mut height, rows.at, distance * theta.cos());
            // not past the apex, proved: the radius at both ends above its rounding (`base +
            // tan h`: the product and sum's, gamma_8, and the tangent's own error from the
            // angle's rounding to radians, (1 + tan^2) |d theta| |h|, d theta a few ulps of it)
            let tan = theta.tan();
            let rounding = |h: f64| {
                gamma(8) * (self.radius(0.0).abs() + (tan * h).abs())
                    + (1.0 + tan * tan) * gamma(4) * theta.abs() * h.abs()
            };
            let past = height.iter().any(|&h| {
                let r = self.radius(h);
                r.is_nan() || r <= rounding(h)
            });
            if past {
                // the slant length from the side to where the radius meets twice its rounding,
                // the radius shrinking by |sin(theta)| per unit of slant (theta < 0 narrows upward)
                let end = if high { old[1] } else { old[0] };
                let room = (self.radius(end) - 2.0 * rounding(end)) / theta.sin().abs();
                return Err(SolidError::ExtendRefused {
                    operation: OP,
                    refusal: ExtendRefusal::PastApex,
                    distance: (room > 0.0).then_some(room * (1.0 - gamma(8))),
                });
            }
        } else {
            let r = self.radius(rows.mid.y);
            if r.is_nan() || r <= 0.0 {
                return Err(SolidError::invalid_input(
                    OP,
                    "the side runs along the axis at a radius not positive",
                ));
            }
            let old = angle;
            let high = widen(&mut angle, rows.at, (distance / r).to_degrees());
            at_most_a_turn(&mut angle, old, high, r, OP)?;
        }
        let wider = Extent2d::from_bounds(angle[0], height[0], angle[1], height[1]);
        let mut cone = self.clone();
        cone.adjust_step_uv_domain(k, &wider)?;
        // bound 0: the source's label, and the result's form on its implicit surface
        let was = AnalyticSupport::of(&Surface::from(self.clone()));
        let now = AnalyticSupport::check(k, &Surface::from(cone.clone()))?;
        if was != Some(now) {
            return Err(SolidError::invalid_input(
                OP,
                "the continuation is not the source's surface",
            ));
        }
        Ok(cone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector2d::Point2;
    use crate::geom::vector3d::Vec3;

    fn k() -> Kernel {
        Kernel::default()
    }

    /// A cone over the STEP box `[a0, a1] x [h0, h1]`, with its NURBS box.
    fn cone(bot: f64, top: f64, h: f64, a: [f64; 2], hs: [f64; 2], swap: bool) -> (Cone, Extent2d) {
        let k = k();
        let pl = Axis2Placement::new(Vec3::new(1.0, -2.0, 0.5), Vec3::X, Vec3::Y);
        let mut c = Cone::create_canonical(&k, &pl, bot, top, h, swap, false).unwrap();
        c.adjust_step_uv_domain(&k, &Extent2d::from_bounds(a[0], hs[0], a[1], hs[1]))
            .unwrap();
        let dom = Surface::from(c.clone())
            .natural_uv_domain_with_nurb(&k)
            .unwrap();
        (c, dom)
    }

    fn step_box(c: &Cone) -> Extent2d {
        Surface::from(c.clone())
            .step_uv_domain_with_nurb(&k())
            .unwrap()
    }

    /// The side of `dom` that holds the STEP angle (`in_height` false) or height constant
    /// at its high or low end.
    fn side_of(c: &Cone, dom: &Extent2d, in_height: bool, high: bool) -> BoxSide {
        let k = k();
        BoxSide::ALL
            .into_iter()
            .find(|s| {
                let a = c
                    .convert_uv_from_nurbs_to_step(&k, s.point(dom, 0.0))
                    .unwrap();
                let z = c
                    .convert_uv_from_nurbs_to_step(&k, s.point(dom, 1.0))
                    .unwrap();
                let held = if in_height { (a.y, z.y) } else { (a.x, z.x) };
                let sb = step_box(c);
                let end = if in_height {
                    if high {
                        sb.max().y
                    } else {
                        sb.min().y
                    }
                } else if high {
                    sb.max().x
                } else {
                    sb.min().x
                };
                (held.0 - end).abs() < 1e-9 && (held.1 - end).abs() < 1e-9
            })
            .unwrap()
    }

    #[test]
    fn a_cylinder_continued_round_its_axis_by_the_arc() {
        // radius 2 over a quarter turn: pi more of arc is a half turn
        for swap in [false, true] {
            let (c, dom) = cone(2.0, 2.0, 3.0, [0.0, 90.0], [0.0, 3.0], swap);
            let s = side_of(&c, &dom, false, true);
            let d = c.continued(&k(), &dom, s, std::f64::consts::PI).unwrap();
            let b = step_box(&d);
            assert!((b.min().x - 0.0).abs() < 1e-12 && (b.max().x - 180.0).abs() < 1e-12);
            assert!((b.min().y - 0.0).abs() < 1e-12 && (b.max().y - 3.0).abs() < 1e-12);
            assert!(d.is_cylinder());
        }
    }

    #[test]
    fn a_cone_continued_along_its_axis_by_the_slant_length() {
        // radius 1 to 2 over height 3: tan(semi) = 1/3
        let (c, dom) = cone(1.0, 2.0, 3.0, [0.0, 90.0], [0.0, 3.0], false);
        let s = side_of(&c, &dom, true, true);
        let d = c.continued(&k(), &dom, s, 2.0).unwrap();
        let b = step_box(&d);
        let dh = 2.0 * 3.0 / 10f64.sqrt();
        assert!((b.max().y - (3.0 + dh)).abs() < 1e-12);
        // the slant from the old top to the new along a generator is the distance
        let at = |h: f64| d.evaluate_step_point(Point2::new(45.0, h)).unwrap();
        assert!((at(3.0 + dh).distance(at(3.0)) - 2.0).abs() < 1e-12);
        // the radius grows as the source's: r(h) = 1 + h / 3
        assert!((d.radius(3.0 + dh) - (1.0 + (3.0 + dh) / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn the_low_end_grows_down() {
        let (c, dom) = cone(1.0, 2.0, 3.0, [10.0, 100.0], [1.0, 3.0], true);
        let s = side_of(&c, &dom, true, false);
        let d = c.continued(&k(), &dom, s, 0.5).unwrap();
        let b = step_box(&d);
        assert!((b.min().y - (1.0 - 0.5 * 3.0 / 10f64.sqrt())).abs() < 1e-12);
        assert!((b.max().y - 3.0).abs() < 1e-12);
        let s = side_of(&c, &dom, false, false);
        let d = c.continued(&k(), &dom, s, 0.5).unwrap();
        let b = step_box(&d);
        // r at the side's middle (height 2) is 5/3
        assert!((b.min().x - (10.0 - (0.5 * 3.0 / 5.0f64).to_degrees())).abs() < 1e-12);
    }

    #[test]
    fn past_the_apex_or_over_a_turn_is_refused() {
        let (c, dom) = cone(1.0, 2.0, 3.0, [0.0, 90.0], [0.0, 3.0], false);
        // the apex is 3 below the origin: a slant of 4 reaches past it
        let s = side_of(&c, &dom, true, false);
        // refused naming the slant to the apex less its rounding, which works (REVIEW2 6)
        let named = |e: SolidError, want: ExtendRefusal| match e {
            SolidError::ExtendRefused {
                refusal,
                distance: Some(d),
                ..
            } if refusal == want => d,
            e => panic!("{e:?}"),
        };
        let d = named(
            c.continued(&k(), &dom, s, 4.0).unwrap_err(),
            ExtendRefusal::PastApex,
        );
        assert!(d < 10f64.sqrt() && d > 10f64.sqrt() * (1.0 - 1e-12), "{d}");
        assert!(c.continued(&k(), &dom, s, d).is_ok());
        let (c, dom) = cone(1.0, 1.0, 3.0, [0.0, 270.0], [0.0, 3.0], false);
        let s = side_of(&c, &dom, false, true);
        let d = named(
            c.continued(&k(), &dom, s, 2.0).unwrap_err(),
            ExtendRefusal::OverATurn,
        );
        assert!((d - std::f64::consts::FRAC_PI_2).abs() < 1e-14, "{d}");
        assert!(c.continued(&k(), &dom, s, d).is_ok());
        // exactly to a full turn is one
        let d = c
            .continued(&k(), &dom, s, std::f64::consts::FRAC_PI_2)
            .unwrap();
        let b = step_box(&d);
        assert_eq!(b.max().x - b.min().x, 360.0);
        assert!(c.continued(&k(), &dom, s, 0.0).is_err());
        assert!(c.continued(&k(), &dom, s, f64::NAN).is_err());
    }

    #[test]
    fn exactly_to_the_apex_is_refused_not_decided_at_a_tolerance() {
        // radius 1 to 2 over height 3: the apex is a slant of sqrt10 below the bottom
        let (c, dom) = cone(1.0, 2.0, 3.0, [0.0, 90.0], [0.0, 3.0], false);
        let s = side_of(&c, &dom, true, false);
        assert!(c.continued(&k(), &dom, s, 10f64.sqrt()).is_err());
        // short of it by much more than the radius's rounding: built
        let d = c
            .continued(&k(), &dom, s, 10f64.sqrt() * (1.0 - 1e-9))
            .unwrap();
        assert!(d.radius(step_box(&d).min().y) > 0.0);
    }

    #[test]
    fn a_moved_form_is_refused() {
        // the NURBS form moved off the label: no longer an exact class
        let (mut c, dom) = cone(1.0, 2.0, 3.0, [0.0, 90.0], [0.0, 3.0], false);
        let s = side_of(&c, &dom, true, true);
        let d = c.continued(&k(), &dom, s, 1.0).unwrap();
        let mut moved = d.clone();
        let shift = Axis2Placement::new(Vec3::new(0.0, 0.0, 1e-6), Vec3::X, Vec3::Y);
        moved.rev.base.transform(&shift, None).unwrap();
        assert!(AnalyticSupport::check(&k(), &Surface::from(moved)).is_err());
        // a source whose label differs from its form gives no continuation
        c.base_radius += 1e-6;
        assert!(c.continued(&k(), &dom, s, 1.0).is_err());
    }
}
