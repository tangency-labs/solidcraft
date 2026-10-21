//! Curve-sweep, swung, skinned and blended primitive orchestration.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`,
//! `source/SMLib/src/SmBSplineSurfaceNLib.cpp` (`CreateSwungSurface`).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation::create_curve_sweep`] | `CreateCurveSweep` |
//! | [`PrimitiveCreation::create_swung_primitive`] | `CreateSwungPrimitive` |
//! | [`PrimitiveCreation::create_skin_from_faces`] | `CreateSkinFromFaces` |
//! | [`PrimitiveCreation::create_blend_primitive`] | `CreateBlendPrimitive` |
//! | `swung_curve_scale` (private) | `CreateSwungPrimitive` scale initialization |
//! | `swung_surface` (private) | `CreateSwungSurface` virtual NURB acquisition |
//!
//! Deviations: typed service resolution replaces pointers. Output face lists
//! preserve partial results; owned geometry handles cleanup. Missing analytic
//! NURBs are generated on owned copies with caller context/owner. No recursion.

use super::{
    geometry::{prepare_curve_sweep, GeometryHandle},
    PrimitiveAccess,
};
use crate::curve::composite_curve::CompositeCurve;
use crate::curve::curve::p2::CurvePropertyType;
use crate::curve::curve::p7::PropertyAnalysisOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignore_status, Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::{BSplineSurface, SurfParamType};
use crate::nurbs::b_spline_surface_n_lib::p2::SkinnedSurfaceOptions;
use crate::nurbs::knots::Side;
use crate::primitive::primitive_creation::{p1::PrimitiveCreation, SweepOptions};
use crate::surface::surface::{
    p2::{BlendEndType, CurvatureType, DerivSurfDefinition},
    Surface,
};
use crate::sweep::topology_sweep::access::{FaceRef, RegionRef};
use crate::topology::{BrepId, EdgeuseId, FaceId};

const OP: &str = "primitive::primitive_creation::p3";

pub(super) fn target(primitive: &PrimitiveCreation, brep: BrepId) -> Result<RegionRef> {
    Ok((
        brep,
        primitive
            .region()
            .ok_or_else(|| SolidError::null_pointer(OP))?,
    ))
}
fn face_surface(a: &impl PrimitiveAccess, face: FaceRef) -> Result<&Surface> {
    crate::topology::face::p2::surface(a.brep(face.0)?, face.1)?
        .ok_or_else(|| SolidError::null_pointer(OP))
}

/// Parameters for `CreateBlendPrimitive`.
#[derive(Debug, Clone, Copy)]
pub struct BlendPrimitiveOptions {
    /// Requested blend continuity.
    pub curvature: CurvatureType,
    /// Smooth or cusped ends.
    pub ends: BlendEndType,
    /// 0 guesses directions; 1 matches starts; 2 matches start to end.
    pub curves_direction: u64,
    /// Starting takeoff length.
    pub start_tangent_length: f64,
    /// Ending takeoff length (divided by the start, including zero, upstream).
    pub end_tangent_length: f64,
}

// Keep the derived curve dispatch here: the upstream SM_CAST_PTR checks the
// inheritance chain without slicing SmLine/conics to their cached NURB base.
fn swung_curve_scale(
    kernel: &crate::kernel::Kernel,
    path: &Curve,
    profile: &Curve,
    approx_tol: f64,
) -> Result<f64> {
    let mut point = profile.evaluate_point(kernel, profile.natural_interval().min())?;
    let normal = Vec3::Y;
    let d = -point.dot(normal);
    let property = path.global_property_analysis(
        kernel,
        &profile.natural_interval(),
        CurvePropertyType::PlaneIntersection,
        approx_tol,
        &PropertyAnalysisOptions {
            property_value: Some(d),
            opt_vectors: &[normal],
            handle: Some(GeometryHandle(path.natural_interval())),
            ..Default::default()
        },
    )?;
    if let Some(solution) = property.solutions.get(0) {
        point = path.evaluate_point(kernel, solution.start.param(0)?)?;
    }
    Ok((1.0 / point.x).abs())
}

// CreateSwungSurface queries GetOrCreateGwNurbPointer on each original
// derived curve. Copying a lazy SmLine's empty base would lose its MakeNurb.
fn swung_nurb(kernel: &crate::kernel::Kernel, curve: &Curve) -> Result<BSplineCurve> {
    let base = super::geometry::bspline(curve)
        .ok_or_else(|| SolidError::invalid_input(OP, "non B-spline swung input"))?;
    if base.gw_nurb().is_some() {
        return Ok(base);
    }
    // GetOrCreateGwNurbPointer ignores MakeNurb's ordinary status. Keep the
    // caller's owner/context for analytic generation and propagate Unported.
    match curve.kind() {
        CurveKind::Line(line) => {
            let mut line = line.clone();
            ignore_status(line.make_nurb_owned(curve.owner()))?;
            Ok(line.base().clone())
        }
        CurveKind::Ellipse(ellipse) => {
            let mut ellipse = ellipse.clone();
            ignore_status(ellipse.make_nurb_owned(kernel, curve.owner()))?;
            Ok(ellipse.base().clone())
        }
        CurveKind::Parabola(parabola) => {
            let mut parabola = parabola.clone();
            ignore_status(parabola.make_nurb(curve.owner()))?;
            Ok(parabola.base().clone())
        }
        CurveKind::Hyperbola(hyperbola) => {
            let mut hyperbola = hyperbola.clone();
            ignore_status(hyperbola.make_nurb(curve.owner()))?;
            Ok(hyperbola.base().clone())
        }
        _ => Ok(base),
    }
}

fn swung_surface(
    kernel: &crate::kernel::Kernel,
    path: &Curve,
    profile: &Curve,
    scale: f64,
) -> Result<BSplineSurface> {
    // The native builder obtains section data before trajectory data.
    let profile = swung_nurb(kernel, profile)?;
    let path = swung_nurb(kernel, path)?;
    BSplineSurface::create_swung_surface(&profile, &path, scale)
}

impl PrimitiveCreation {
    /// Build swung surfaces for every B-spline-compatible path/profile pair.
    /// Only pair (0,0) initializes the scale, even if earlier slots were skipped.
    pub fn create_swung_primitive(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        paths: &[Option<Curve>],
        profiles: &[Option<Curve>],
        scale: f64,
        approx_tol: f64,
    ) -> Result<()> {
        self.init_entity_maps();
        let _ = target(self, brep)?;
        let mut curve_scale = 0.0;
        let mut faces = Vec::new();
        for (i, path) in paths.iter().enumerate() {
            let Some(path) = path.as_ref() else {
                continue;
            };
            if !path.is_kind_of(BSplineCurve::TYPE) {
                continue;
            }
            for (j, profile) in profiles.iter().enumerate() {
                let Some(profile) = profile.as_ref() else {
                    continue;
                };
                if !profile.is_kind_of(BSplineCurve::TYPE) {
                    continue;
                }
                if i == 0 && j == 0 {
                    curve_scale = swung_curve_scale(a.kernel(brep)?, path, profile, approx_tol)?;
                }
                let surface = swung_surface(a.kernel(brep)?, path, profile, scale * curve_scale)?;
                let surface = Surface::from(surface);
                let domain = surface.natural_uv_domain();
                a.surface_faces(brep, surface, domain, &mut faces)?;
            }
        }
        Ok(())
    }

    /// Loft matching loops of faces, or the supplied sequence of profile curves.
    /// `new_faces` is reset before any failing call, as upstream.
    pub fn create_skin_from_faces(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        faces: &[FaceRef],
        profiles: &[BSplineCurve],
        approx_tol: f64,
        degree: usize,
        new_faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        self.init_entity_maps();
        new_faces.clear();
        let _ = target(self, brep)?;
        let mut skin_curves = Vec::new();
        let mut layers = Vec::new();
        let mut layer_count = 0;
        for (i, &face) in faces.iter().enumerate() {
            let domain = crate::topology::face::p2::uv_domain(a.brep(face.0)?, face.1)?;
            face_surface(a, face)?.evaluate_normal(
                a.kernel(face.0)?,
                domain.evaluate(0.5, 0.5),
                Side::Left,
                Side::Left,
            )?;
            let composites = a.face_composites(face)?;
            if i == 0 {
                layer_count = composites.len();
            } else if layer_count != composites.len() {
                return Err(SolidError::sm_err(OP));
            }
            for (j, mut composite) in composites.into_iter().enumerate() {
                skin_curves.push(composite.make_composite_nurb()?);
                layers.push(j);
            }
        }
        if faces.is_empty() {
            skin_curves.extend_from_slice(profiles);
            layer_count = 1;
            layers.resize(skin_curves.len(), 0);
        }
        let mut made = Vec::new();
        for layer in 0..layer_count {
            let curves: Vec<_> = skin_curves
                .iter()
                .zip(&layers)
                .filter(|(_, l)| **l == layer)
                .map(|(c, _)| c.clone())
                .collect();
            let surface = BSplineSurface::create_skinned_surface(
                a.kernel(brep)?,
                &curves,
                SkinnedSurfaceOptions {
                    approx_tol,
                    degree,
                    deriv_surfs: Some([None, None]),
                    ..Default::default()
                },
            )?;
            let surface = Surface::from(surface);
            let domain = surface.natural_uv_domain();
            ignore_status(a.surface_faces(brep, surface, domain, &mut made))?;
            new_faces.extend_from_slice(&made);
        }
        Ok(())
    }

    /// Construct a blend and add its face, preserving the ignored face status.
    pub fn create_blend_primitive(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        first: Option<EdgeuseId>,
        second: Option<EdgeuseId>,
        output: &mut Option<FaceId>,
        options: BlendPrimitiveOptions,
    ) -> Result<()> {
        self.init_entity_maps();
        let _ = target(self, brep)?;
        let flag = match options.curvature {
            CurvatureType::NoCurvature => 1,
            CurvatureType::G3FromSurface => 3,
            _ => 2,
        };
        let mut derivatives = DerivSurfDefinition {
            curvature_type: options.curvature,
            blend_end_type: options.ends,
            start_tan_length: options.start_tangent_length,
            end_tan_scale: options.end_tangent_length / options.start_tangent_length,
            ..Default::default()
        };
        let surface = a.with_primitive_brep(brep, |kernel, model| {
            crate::topology::brep::p1::create_edge_edge_blend(
                kernel,
                flag,
                model,
                first,
                None, // Both edge uses belong to this primitive's B-rep.
                second,
                options.curves_direction,
                Some(&mut derivatives),
            )
        })?;
        a.set_editing(brep, true)?;
        let domain = surface.natural_uv_domain();
        ignore_status(a.surface_face(brep, surface, domain, output))?;
        Ok(())
    }

    /// Sweep copies of the profiles along the path, optionally scaling and capping.
    /// Only `side_faces` is reset; cap output slots retain their upstream lifetime.
    /// The scale reference retains its derived curve dispatch; path and scale use
    /// B-spline copies, as in the upstream constructor.
    pub fn create_curve_sweep(
        &mut self,
        a: &mut impl PrimitiveAccess,
        brep: BrepId,
        profiles: &[Option<Curve>],
        path: &BSplineCurve,
        scale_reference: Option<&Curve>,
        scale_curve: Option<&BSplineCurve>,
        approx_tol: f64,
        options: SweepOptions,
        test_continuity: bool,
        start_faces: &mut Vec<FaceId>,
        side_faces: &mut Vec<FaceId>,
        end_faces: &mut Vec<FaceId>,
    ) -> Result<()> {
        self.init_entity_maps();
        side_faces.clear();
        if profiles.is_empty() {
            return Err(SolidError::sm_err(OP));
        }
        let region = target(self, brep)?;
        let previous = a.kernel(brep)?.context.doing_boolean();
        a.kernel_mut(brep)?.context.set_doing_boolean(true);
        let result = (|| {
            let prepared = prepare_curve_sweep(
                a.kernel(brep)?,
                profiles,
                path,
                scale_reference,
                scale_curve,
                options,
                test_continuity,
            )?;
            let count = prepared.profiles.len();
            let curves = prepared.profiles.into_iter().map(Curve::from).collect();
            let composites = CompositeCurve::build_composites_from_curves(
                a.kernel(brep)?,
                curves,
                a.brep(brep)?.tolerance(),
                0.0,
            )?;
            for mut composite in composites.composites {
                let profile = composite.make_composite_nurb()?;
                let surface = BSplineSurface::create_sweep_surface(
                    &profile,
                    &prepared.path,
                    prepared.scale.as_ref(),
                    options.translational_sweep,
                    approx_tol,
                )?;
                let surface = Surface::from(surface);
                let domain = surface.natural_uv_domain();
                let mut made = Vec::new();
                ignore_status(a.surface_faces(brep, surface, domain, &mut made))?;
                side_faces.extend(made);
            }
            ignore_status(a.transform_primitive(brep, &prepared.rotation_back))?;
            let placement = if options.move_to_profile {
                &prepared.profile_back
            } else {
                &prepared.path_back
            };
            ignore_status(a.transform_primitive(brep, placement))?;
            if options.cap_ends {
                let mut starts = Vec::new();
                let mut ends = Vec::new();
                let mut direction = SurfParamType::U;
                let total = side_faces.len();
                for (i, &face) in side_faces.iter().enumerate() {
                    let surface = face_surface(a, (brep, face))?;
                    let domain = surface.natural_uv_domain();
                    if i < count {
                        direction = if options.translational_sweep {
                            SurfParamType::V
                        } else {
                            SurfParamType::U
                        };
                        let param = if options.translational_sweep {
                            domain.min().y
                        } else {
                            domain.min().x
                        };
                        let curve = surface
                            .create_iso_parametric_curve(
                                a.kernel(brep)?,
                                direction,
                                param,
                                approx_tol / 10.0,
                                None,
                                false,
                            )?
                            .curve;
                        starts.push(Some(curve));
                    }
                    if count > total || i >= total - count {
                        let param = if options.translational_sweep {
                            direction = SurfParamType::V;
                            domain.max().y
                        } else {
                            domain.max().x
                        };
                        let curve = surface
                            .create_iso_parametric_curve(
                                a.kernel(brep)?,
                                direction,
                                param,
                                approx_tol / 10.0,
                                None,
                                false,
                            )?
                            .curve;
                        ends.push(Some(curve));
                    }
                }
                a.planar_faces(region, &mut starts, approx_tol, start_faces)?;
                a.planar_faces(region, &mut ends, approx_tol, end_faces)?;
            }
            ignore_status(a.stitch_and_orient(brep))?;
            Ok(())
        })();
        a.kernel_mut(brep)?.context.set_doing_boolean(previous);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::extent1d::Extent1d;
    use crate::kernel::Kernel;

    // An imported line may have last-bit differences between its analytic data
    // and cached NURB. Virtual evaluations must continue to use the analytic data.
    fn imported_line(point: Vec3, vector: Vec3, scale: f64) -> Curve {
        let cached_point = Vec3::new(f64::from_bits(point.x.to_bits() + 2), point.y, point.z);
        let base =
            BSplineCurve::create_line_segment(3, cached_point, cached_point + scale * vector)
                .unwrap();
        Curve::from(Line::from_parts(
            base,
            point,
            vector,
            Extent1d::from_min_max(0.0, 1.0),
            scale,
            false,
        ))
    }

    #[test]
    fn swung_builder_materializes_lazy_line_nurbs() -> Result<()> {
        let kernel = Kernel::default();
        let p0 = Vec3::new(1.0, 0.0, 0.0);
        let p1 = Vec3::new(1.0, 0.0, 1.0);
        let t0 = Vec3::new(2.0, -1.0, 0.0);
        let t1 = Vec3::new(2.0, 1.0, 0.0);
        let profile = Line::create_line_segment(3, p0, p1, None)?;
        let path = Line::create_line_segment(3, t0, t1, None)?;
        let expected = Surface::from(BSplineSurface::create_swung_surface(
            profile.base(),
            path.base(),
            0.5,
        )?);
        let lazy_profile = Line::from_points(p0, p1, 3);
        let lazy_path = Line::from_points(t0, t1, 3);
        assert!(lazy_profile.base().gw_nurb().is_none());
        assert!(lazy_path.base().gw_nurb().is_none());
        // Each input must reach its own virtual MakeNurb, including when both
        // caches are empty. The old base copies fail before building a surface.
        for (profile, path) in [
            (lazy_profile.clone(), path.clone()),
            (profile, lazy_path.clone()),
            (lazy_profile, lazy_path),
        ] {
            let surface = Surface::from(swung_surface(
                &kernel,
                &Curve::from(path),
                &Curve::from(profile),
                0.5,
            )?);
            let domain = surface.natural_uv_domain();
            assert_eq!(domain, expected.natural_uv_domain());
            for i in 0..=4 {
                for j in 0..=4 {
                    let uv = domain.evaluate(f64::from(i) / 4.0, f64::from(j) / 4.0);
                    assert_eq!(
                        surface.evaluate_point(&kernel, uv)?,
                        expected.evaluate_point(&kernel, uv)?,
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn swung_builder_keeps_an_existing_cached_nurb() -> Result<()> {
        let kernel = Kernel::default();
        let profile = imported_line(Vec3::new(2.0, 0.0, 0.0), Vec3::Z, 1.0);
        let path = Curve::from(Line::from_points(Vec3::X, Vec3::X + Vec3::Y, 3));
        let surface = Surface::from(swung_surface(&kernel, &path, &profile, 1.0)?);
        let uv = surface.natural_uv_domain().min();
        let point = surface.evaluate_point(&kernel, uv)?;
        // Surface construction uses the existing NURB, even though scale
        // initialization above must use the original analytic evaluations.
        assert_eq!(point.x, f64::from_bits(2.0_f64.to_bits() + 2));
        assert_ne!(point.x, 2.0);
        Ok(())
    }

    #[test]
    fn swung_scale_keeps_analytic_profile_when_no_plane_intersection_exists() {
        let kernel = Kernel::default();
        let path = Curve::from(BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::X).unwrap());
        let profile = imported_line(Vec3::new(2.0, 2.0, 0.0), Vec3::Z, 1.0);
        assert_eq!(
            swung_curve_scale(&kernel, &path, &profile, 1e-5).unwrap(),
            0.5
        );
        let sliced = Curve::from(super::super::geometry::bspline(&profile).unwrap());
        assert_ne!(
            swung_curve_scale(&kernel, &path, &sliced, 1e-5).unwrap(),
            0.5
        );
    }

    #[test]
    fn swung_scale_keeps_analytic_path_after_plane_intersection() {
        let kernel = Kernel::default();
        let path = imported_line(Vec3::new(2.0, -1.0, 0.0), Vec3::Y, 2.0);
        let profile =
            Curve::from(BSplineCurve::create_line_segment(3, Vec3::X, Vec3::X + Vec3::Z).unwrap());
        assert_eq!(
            swung_curve_scale(&kernel, &path, &profile, 1e-5).unwrap(),
            0.5
        );
        let sliced = Curve::from(super::super::geometry::bspline(&path).unwrap());
        assert_ne!(
            swung_curve_scale(&kernel, &sliced, &profile, 1e-5).unwrap(),
            0.5
        );
    }
}
