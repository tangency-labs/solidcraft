//! Primitive p3 differential comparisons: the native offset ruled-surface helper
//! and real primitive entry points. The latter defer at unavailable topology;
//! status-only early exits are kept distinct from surface/curve comparisons.
#![allow(clippy::indexing_slicing)] // fixed fixture layout validated before indexing

use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::error::{Result, SolidError};
use solidcore::geom::attribute::TagData;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::context::MarkType;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::primitive_creation::{
    p1::PrimitiveCreation,
    p3::{self as api, PrimitiveAccess, RepeatedSweepOptions},
    SweepOptions,
};
use solidcore::surface::surface::Surface;
use solidcore::sweep::topology_sweep::{
    access::{EdgeRef, FaceRef, SweepAccess, SweepMergeId},
    SweepEntity,
};
use solidcore::topology::{Brep, BrepId, FaceId, Region, RegionId};
use solidcore_ffi::units::primitive_primitive_creation_p3 as ffi;

/// Differential runner.
pub struct Runner;
#[derive(Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    expected_rejection: bool,
    op: i32,
    data: Vec<f64>,
}
fn error(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.,
        _ => 1001.,
    }
}
fn curve(data: &[f64]) -> Result<BSplineCurve> {
    let [kind, x, y, z, a, b, c] = data else {
        return Err(SolidError::sm_err("fixture"));
    };
    if *kind == 0. {
        BSplineCurve::create_line_segment(3, Vec3::new(*x, *y, *z), Vec3::new(*a, *b, *c))
    } else {
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(Vec3::new(*x, *y, *z), Vec3::X, Vec3::Y),
            *a,
            *b,
            *c,
            NurbCircleParam::Quadratic,
        )
    }
}
fn samples(curve: &BSplineCurve, out: &mut Vec<f64>) -> Result<()> {
    let ivl = curve.natural_interval();
    out.extend([
        ivl.min(),
        ivl.max(),
        curve.degree()? as f64,
        curve.number_control_points()? as f64,
        f64::from(curve.is_rational()?),
    ]);
    for i in 0..=4 {
        for point in curve.evaluate(ivl.evaluate(f64::from(i) / 4.0), 1, true, false)? {
            out.extend(point.to_array());
        }
    }
    Ok(())
}
fn ruled(data: &[f64]) -> Result<Vec<f64>> {
    let k = Kernel::default();
    let mut offset = curve(&data[20..27])?;
    let mut generators = [curve(&data[27..34])?, curve(&data[34..41])?];
    let mut unmarried = [data[6] != 0., data[7] != 0.];
    let mut missing = data[5] != 0.;
    let intervals = [
        generators[0].natural_interval(),
        generators[1].natural_interval(),
    ];
    let rational = generators[0].is_rational()?;
    let map = (data[1] != 0.).then(|| {
        solidcore::curve::composite_curve::OffsetMapAttribute::new(
            0,
            data[0] != 0.,
            false,
            0,
            data[2] as u64,
            data[3] as u64,
            None,
            0.,
        )
    });
    let result = api::ruled_surface_from_offset(
        &k,
        &mut offset,
        map,
        &mut generators,
        intervals,
        rational,
        &mut unmarried,
        &mut missing,
        api::OffsetRuleOptions {
            tolerance: data[8],
            same_point_tolerance: data[9],
            offset: data[10],
            movement: Axis2Placement::new(
                Vec3::new(data[11], data[12], data[13]),
                Vec3::new(data[14], data[15], data[16]),
                Vec3::new(data[17], data[18], data[19]),
            ),
            surface_intersection: data[4] != 0.,
        },
    );
    if let Err(e) = &result {
        if e.is_unported() {
            return Err(e.clone());
        }
    }
    let mut out = vec![
        result.as_ref().err().map_or(0., error),
        f64::from(missing),
        f64::from(unmarried[0]),
        f64::from(unmarried[1]),
    ];
    samples(&offset, &mut out)?;
    for generator in &generators {
        samples(generator, &mut out)?;
    }
    out.push(f64::from(result.is_ok()));
    if let Ok((surface, map)) = result {
        let surface = Surface::from(surface);
        let domain = surface.natural_uv_domain();
        out.extend([
            domain.min().x,
            domain.min().y,
            domain.max().x,
            domain.max().y,
        ]);
        for u in 0..=4 {
            for v in 0..=4 {
                out.extend(
                    surface
                        .evaluate_point(&k, domain.evaluate(f64::from(u) / 4.0, f64::from(v) / 4.0))?
                        .to_array(),
                );
            }
        }
        out.push(f64::from(map.is_some()));
    }
    Ok(out)
}

// The native fixture is a fresh B-rep with only its infinite region. This adapter
// resolves exactly that state. Unsupported services fail explicitly if a case
// unexpectedly passes the current construction dependency; no topology is faked.
struct EmptyModel {
    k: Kernel,
    notification_records: std::collections::BTreeMap<
        solidcore::geom::attribute::ObjectKey,
        solidcore::topology::topology::Topology,
    >,
    notification_attributes: solidcore::geom::attribute::AttributeStore,
    b: Brep,
}
fn unavailable<T>() -> Result<T> {
    Err(SolidError::invalid_input(
        "primitive_p3_fixture",
        "fixture has no topology/service for this call",
    ))
}
impl EmptyModel {
    fn new() -> Result<Self> {
        let mut b = Brep::new();
        let region = b.add_region_record(Region {
            in_brep: true,
            ..Default::default()
        })?;
        b.set_infinite_region(Some(region));
        Ok(Self {
            k: Kernel::default(),
            notification_records: std::collections::BTreeMap::default(),
            notification_attributes: solidcore::geom::attribute::AttributeStore::default(),
            b,
        })
    }
}
impl SweepAccess for EmptyModel {
    fn brep(&self, _: BrepId) -> Result<&Brep> {
        Ok(&self.b)
    }
    fn brep_mut(&mut self, _: BrepId) -> Result<&mut Brep> {
        Ok(&mut self.b)
    }
    fn kernel(&self, _: BrepId) -> Result<&Kernel> {
        Ok(&self.k)
    }
    fn kernel_mut(&mut self, _: BrepId) -> Result<&mut Kernel> {
        Ok(&mut self.k)
    }
    fn with_brep_mut<T>(
        &mut self,
        id: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &dyn Fn(FaceId) -> solidcore::geom::object::ObjectRef,
            &mut solidcore::topology::edge::NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        if id != BrepId::from_index(0) {
            return unavailable();
        }
        // The fixture owns this face-key namespace, including future allocations.
        let face_objects = |face: FaceId| solidcore::geom::object::ObjectRef {
            type_code: solidcore::geom::assert_array::labels::FACE_TYPE,
            key: solidcore::geom::attribute::ObjectKey(100_000 + u64::from(face.index()) * 97),
        };
        let key_of = |entity: solidcore::EntityRef| {
            if let solidcore::EntityRef::Face(face) = entity {
                face_objects(face)
            } else {
                super::boolean_topology_intersector_p1::p9_entity_ref(entity)
            }
        };
        let mut notifications = solidcore::topology::edge::NotifyEnv {
            records: &mut self.notification_records,
            attributes: &mut self.notification_attributes,
            context: None,
            key_of: &key_of,
        };
        body(&mut self.k, &mut self.b, &face_objects, &mut notifications)
    }
    fn with_breps_mut<T>(
        &mut self,
        _source: BrepId,
        _destination: BrepId,
        _body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(FaceId) -> solidcore::geom::object::ObjectRef,
            &mut solidcore::topology::edge::NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        // This fixture has one B-rep; no distinct pair can be resolved.
        unavailable()
    }
    fn with_brep_notifications<T>(
        &mut self,
        id: BrepId,
        body: impl FnOnce(&mut Brep, &mut solidcore::topology::edge::NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        if id != BrepId::from_index(0) {
            return unavailable();
        }
        let key_of = |entity: solidcore::EntityRef| {
            if let solidcore::EntityRef::Face(face) = entity {
                solidcore::geom::object::ObjectRef {
                    type_code: solidcore::geom::assert_array::labels::FACE_TYPE,
                    key: solidcore::geom::attribute::ObjectKey(
                        100_000 + u64::from(face.index()) * 97,
                    ),
                }
            } else {
                super::boolean_topology_intersector_p1::p9_entity_ref(entity)
            }
        };
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut self.notification_records,
            attributes: &mut self.notification_attributes,
            context: Some(&mut self.k.context),
            key_of: &key_of,
        };
        body(&mut self.b, &mut env)
    }
    fn retain_hcr(&mut self, _: BrepId, _: solidcore::blend::hcr::Hcr) -> Result<BrepId> {
        unavailable()
    }
    fn hcr_session(&self, _: BrepId) -> Result<&solidcore::blend::hcr::Hcr> {
        unavailable()
    }
    fn new_brep(&mut self, _: BrepId) -> Result<BrepId> {
        unavailable()
    }
    fn delete_brep(&mut self, _: BrepId) {}
    fn delete_merge(&mut self, _: SweepMergeId) {}
    fn tag(&self, _: SweepEntity) -> Result<Option<TagData>> {
        unavailable()
    }
    fn add_tag(&mut self, _: SweepEntity, _: TagData) -> Result<()> {
        unavailable()
    }
    fn curve_tag(&self, _: EdgeRef) -> Result<Option<TagData>> {
        unavailable()
    }
    fn set_editing(&mut self, _: BrepId, value: bool) -> Result<bool> {
        let previous = self.b.editing_enabled();
        self.b.set_editing_enabled(value);
        Ok(previous)
    }
    fn lock_mark(&mut self, _: BrepId, _: i32) -> Result<MarkType> {
        unavailable()
    }
    fn unlock_mark(&mut self, _: BrepId, _: MarkType) -> Result<()> {
        unavailable()
    }
    fn is_marked(&self, _: SweepEntity, _: MarkType) -> Result<bool> {
        unavailable()
    }
    fn mark(&mut self, _: SweepEntity, _: MarkType) -> Result<()> {
        unavailable()
    }
    fn collect_faces(&mut self, _: FaceRef, _: MarkType, _: &mut Vec<FaceId>) -> Result<()> {
        unavailable()
    }
}
impl PrimitiveAccess for EmptyModel {
    fn delete_primitive_faces(&mut self, _: BrepId, _: &[FaceId]) -> Result<()> {
        unavailable()
    }
    fn with_primitive_brep<T>(
        &mut self,
        _: BrepId,
        body: impl FnOnce(&mut Kernel, &mut Brep) -> Result<T>,
    ) -> Result<T> {
        body(&mut self.k, &mut self.b)
    }
    fn axis_inside_face(&mut self, _: FaceRef, _: &Curve) -> Result<bool> {
        unavailable()
    }
}
fn entry(i: &Inputs) -> Result<Vec<f64>> {
    let data = &i.data;
    let mut model = EmptyModel::new()?;
    let mut primitive = PrimitiveCreation::new(Some(RegionId::from_index(0)), 0, false);
    let mut curves: Vec<_> = data[10..]
        .as_chunks::<7>()
        .0
        .iter()
        .map(|v| curve(v).map(|c| Some(Curve::from(c))))
        .collect::<Result<_>>()?;
    let b = BrepId::from_index(0);
    let mut starts = Vec::new();
    let mut sides = Vec::new();
    let mut ends = Vec::new();
    let result = match i.op {
        1 => primitive.create_repeated_linear_sweep(
            &mut model,
            b,
            &mut curves,
            Vec3::new(data[0], data[1], data[2]),
            data[3],
            RepeatedSweepOptions {
                repetitions: data[4] as u64,
                cap_ends: data[5] != 0.,
                test_continuity: data[6] != 0.,
            },
        ),
        2 => primitive.create_rotational_sweep(
            &mut model,
            b,
            &mut curves,
            Vec3::new(data[0], data[1], data[2]),
            Vec3::new(data[3], data[4], data[5]),
        ),
        3 => primitive.create_partial_rotational_sweep(
            &mut model,
            b,
            &mut curves,
            Vec3::new(data[0], data[1], data[2]),
            Vec3::new(data[3], data[4], data[5]),
            data[6],
            RepeatedSweepOptions {
                repetitions: data[7] as u64,
                cap_ends: data[8] != 0.,
                test_continuity: data[9] != 0.,
            },
        ),
        4 => {
            let profiles: Vec<_> = curves
                .iter()
                .flatten()
                .filter_map(|c| {
                    if let solidcore::curve::curve::CurveKind::BSpline(c) = c.kind() {
                        Some(c.clone())
                    } else {
                        None
                    }
                })
                .collect();
            primitive.create_skin_from_faces(
                &mut model,
                b,
                &[],
                &profiles,
                data[0],
                data[1] as usize,
                &mut sides,
            )
        }
        5 => primitive.create_curve_sweep(
            &mut model,
            b,
            &curves,
            &BSplineCurve::create_line_segment(3, Vec3::ZERO, 2. * Vec3::Z)?,
            None,
            None,
            data[0],
            SweepOptions {
                translational_sweep: true,
                ..Default::default()
            },
            data[1] != 0.,
            &mut starts,
            &mut sides,
            &mut ends,
        ),
        6 => primitive.create_swung_primitive(
            &mut model,
            b,
            &curves[..data[2] as usize],
            &curves[data[2] as usize..],
            data[0],
            data[1],
        ),
        7 => primitive.create_blend_primitive(
            &mut model,
            b,
            None,
            None,
            &mut None,
            api::BlendPrimitiveOptions {
                curvature: solidcore::surface::surface::p2::CurvatureType::from_code(
                    data[0] as u64,
                )
                .ok_or_else(|| SolidError::sm_err("fixture"))?,
                ends: solidcore::surface::surface::p2::BlendEndType::from_code(data[1] as u64)
                    .ok_or_else(|| SolidError::sm_err("fixture"))?,
                curves_direction: data[2] as u64,
                start_tangent_length: data[3],
                end_tangent_length: data[4],
            },
        ),
        _ => return unavailable(),
    };
    if let Err(e) = &result {
        if e.is_unported() {
            return Err(e.clone());
        }
    }
    Ok(vec![
        result.as_ref().err().map_or(0., error),
        f64::from(model.b.editing_enabled()),
        f64::from(model.k.context.doing_boolean()),
        starts.len() as f64,
        sides.len() as f64,
        ends.len() as f64,
    ])
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.primitive_creation_p3"
    }
    fn cases(
        &self,
        ctx: &crate::case::CaseContext,
    ) -> std::result::Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &crate::case::CaseContext {
                seeds: ctx.seeds.max(256),
                ..ctx.clone()
            },
        )
    }
    fn run_cpp(&self, case: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match ffi::eval(i.op, &i.data) {
            Ok(values) => Ok(Output::new().values("record", values)),
            Err(solidcore_ffi::FfiError::InvalidArgument(_)) if i.expected_rejection => {
                Ok(Output::rejected("UB-78: zero skin cross-sections"))
            }
            Err(solidcore_ffi::FfiError::Status(s)) if s == solidcore_ffi::SC_ERR_ARG => Ok(
                Output::rejected("native curve construction/sample preflight"),
            ),
            Err(e) => Err(HarnessError::new(e.to_string())),
        }
    }
    fn run_rust(&self, case: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        ffi::validate(i.op, &i.data).map_err(|e| HarnessError::new(e.to_string()))?;
        let result = if i.op == 0 { ruled(&i.data) } else { entry(&i) };
        match result {
            Ok(values) => Ok(Output::new().values("record", values)),
            Err(e) => {
                if let Some(deferred) = Output::unported(&e) {
                    Ok(deferred)
                } else {
                    Err(HarnessError::new(e.to_string()))
                }
            }
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = 10f64.powi(rng.range_i64(-6, 6) as i32);
        let corner = rng.below(3) == 0;
        let mut data = vec![
            f64::from(corner),
            1.,
            0.,
            1.,
            f64::from(rng.below(2) != 0),
            0.,
            1.,
            1.,
            scale * 1e-5,
            scale * 1e-8,
            scale,
            0.,
            0.,
            rng.uniform(-3., 3.) * scale,
            1.,
            0.,
            0.,
            0.,
            1.,
            0.,
        ];
        if rng.below(8) == 0 {
            data[1] = 0.;
        }
        if rng.below(8) == 0 {
            data[6] = 0.;
        }
        if rng.below(8) == 0 {
            data[5] = 1.;
        }
        if corner {
            data.extend([1., 2. * scale, 0., 0., scale, 0., 90.]);
        } else {
            data.extend([0., 0., scale, 0., 2. * scale, scale, 0.]);
        }
        data.extend([0., 0., 0., 0., 2. * scale, 0., 0.]);
        data.extend([0., 2. * scale, 0., 0., 2. * scale, 2. * scale, 0.]);
        serde_json::to_value(Inputs {
            op: 0,
            data,
            expected_rejection: false,
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::sweep::topology_sweep::{TopologySweep, TopologySweepOptions};

    #[test]
    fn face_box_default_resolves_the_selected_face_and_uses_the_normal_box() {
        let mut model = EmptyModel::new().unwrap();
        let shell = model.b.add_shell().unwrap();
        let _without_surface = model.b.add_face(shell).unwrap();
        let selected = model.b.add_face(shell).unwrap();
        let plane =
            solidcore::surface::plane::Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap();
        model.b.face_mut(selected).unwrap().surface = Some(Surface::from(plane));
        // A linear surface without trims has an empty normal box. The leading
        // face has no surface, so resolving face zero would fail this check.
        let bound = model
            .primitive_face_box((BrepId::from_index(0), selected))
            .unwrap();
        assert_eq!(bound, solidcore::geom::extent3d::Extent3d::new());
        assert!(matches!(
            model.primitive_face_box((BrepId::from_index(1), selected)),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    #[test]
    fn linear_cap_stitch_completes_for_empty_brep_and_restores_caller_state() {
        let mut model = EmptyModel::new().unwrap();
        let mut sweep = TopologySweep::new(TopologySweepOptions::default());
        for editing in [false, true] {
            for doing_boolean in [false, true] {
                model.b.set_editing_enabled(editing);
                model.k.context.set_doing_boolean(doing_boolean);
                let result = model.stitch_linear_caps(BrepId::from_index(0), &mut sweep, 0.125);
                assert!(result.is_ok(), "stitch result: {result:?}");
                assert_eq!(model.b.editing_enabled(), editing);
                assert_eq!(model.k.context.doing_boolean(), doing_boolean);
            }
        }
    }
}
