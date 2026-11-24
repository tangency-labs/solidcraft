//! Runner for primitive p3: the offset ruled-surface helper and the primitive entry
//! points through the production arena adapter, ported from upstream; checked against
//! its golden. Successful builds record owning-list topology counts and sampled vertex/edge/face geometry;
//! unavailable continuations remain deferred and early exits retain their status.
#![allow(clippy::indexing_slicing)] // the record layout is checked by `layout_ok` first

use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::error::{Result, SolidError};
#[cfg(test)]
use solidcore::geom::attribute::TagData;
use solidcore::geom::axis2_placement::Axis2Placement;
#[cfg(test)]
use solidcore::geom::context::MarkType;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
#[cfg(test)]
use solidcore::primitive::primitive_creation::p3::PrimitiveAccess;
use solidcore::primitive::primitive_creation::{
    model::PrimitiveModel,
    p1::PrimitiveCreation,
    p3::{self as api, RepeatedSweepOptions},
    SweepOptions,
};
use solidcore::surface::surface::Surface;
use solidcore::sweep::topology_sweep::access::SweepAccess;
#[cfg(test)]
use solidcore::sweep::topology_sweep::{
    access::{EdgeRef, FaceRef, SweepMergeId},
    SweepEntity,
};
use solidcore::topology::Brep;
#[cfg(test)]
use solidcore::topology::{BrepId, FaceId, Region};

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
        e if solidcore::error::upstream_status(e).is_some() => {
            f64::from(solidcore::error::upstream_status(e).unwrap_or(-1))
        }
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
/// Whether `d` has the record layout of op `op` (its lengths and the swung profile
/// count), so that [`ruled`] and [`entry`] can index it.
fn layout_ok(op: i32, d: &[f64]) -> bool {
    match op {
        0 => d.len() == 41,
        6 => {
            d.len() >= 10
                && d[2].fract() == 0.0
                && (0.0..=((d.len() - 10) / 7) as f64).contains(&d[2])
        }
        1..=7 => d.len() >= 10,
        _ => false,
    }
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
        let domain = surface.natural_uv_domain_with_nurb(&k)?;
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
                        .evaluate_point(
                            &k,
                            domain.evaluate(f64::from(u) / 4.0, f64::from(v) / 4.0),
                        )?
                        .to_array(),
                );
            }
        }
        out.push(f64::from(map.is_some()));
    }
    Ok(out)
}

// Minimal caller used only by the default-adapter regression tests below.
// Differential entry points use the production PrimitiveModel.
#[cfg(test)]
struct EmptyModel {
    k: Kernel,
    notification_records: solidcore::topology::topology::NotifyRecords,
    notification_attributes: solidcore::geom::attribute::AttributeStore,
    // `SmAObject` tag attributes of the swept topology (none on the fresh B-rep).
    tags: std::collections::BTreeMap<SweepEntity, TagData>,
    b: Brep,
}
fn unavailable<T>() -> Result<T> {
    Err(SolidError::invalid_input(
        "primitive_p3_fixture",
        "fixture has no topology/service for this call",
    ))
}
// The fixture owns this face-key namespace, including future allocations.
#[cfg(test)]
fn fixture_face(face: FaceId) -> solidcore::geom::object::ObjectRef {
    solidcore::geom::object::ObjectRef {
        type_code: solidcore::geom::assert_array::labels::FACE_TYPE,
        key: solidcore::geom::attribute::ObjectKey(100_000 + u64::from(face.index()) * 97),
    }
}
#[cfg(test)]
fn fixture_key(entity: solidcore::EntityRef) -> solidcore::geom::object::ObjectRef {
    if let solidcore::EntityRef::Face(face) = entity {
        fixture_face(face)
    } else {
        super::boolean_topology_intersector_p1::p9_entity_ref(entity)
    }
}
#[cfg(test)]
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
            notification_records: solidcore::topology::topology::NotifyRecords::new(),
            notification_attributes: solidcore::geom::attribute::AttributeStore::default(),
            tags: std::collections::BTreeMap::default(),
            b,
        })
    }
}
#[cfg(test)]
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
        let mut notifications = solidcore::topology::edge::NotifyEnv {
            records: &mut self.notification_records,
            attributes: &mut self.notification_attributes,
            context: None,
            key_of: &fixture_key,
        };
        body(&mut self.k, &mut self.b, &fixture_face, &mut notifications)
    }
    fn with_breps_mut<T>(
        &mut self,
        _source: BrepId,
        _destination: BrepId,
        _body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(solidcore::EntityRef) -> solidcore::geom::object::ObjectRef,
            &dyn Fn(FaceId) -> solidcore::geom::object::ObjectRef,
            Option<&mut solidcore::geom::context::Context>,
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
        let mut env = solidcore::topology::edge::NotifyEnv {
            records: &mut self.notification_records,
            attributes: &mut self.notification_attributes,
            context: Some(&mut self.k.context),
            key_of: &fixture_key,
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
    fn new_merge(
        &mut self,
        _: BrepId,
        _: BrepId,
        _: BrepId,
        _: f64,
        _: f64,
    ) -> Result<SweepMergeId> {
        Err(SolidError::invalid_input(
            "primitive fixture",
            "no merge engine",
        ))
    }
    fn merge_operands(&self, _: SweepMergeId) -> Result<(BrepId, BrepId)> {
        Err(SolidError::invalid_input(
            "primitive fixture",
            "no merge engine",
        ))
    }
    fn merge_relations(
        &mut self,
        _: SweepMergeId,
    ) -> Result<&mut solidcore::boolean::topology_intersector::p1::Relations> {
        Err(SolidError::invalid_input(
            "primitive fixture",
            "no merge engine",
        ))
    }
    fn piecewise_merge(&mut self, _: SweepMergeId, _: bool, _: bool) -> Result<BrepId> {
        Err(SolidError::invalid_input(
            "primitive fixture",
            "no merge engine",
        ))
    }
    fn union(&mut self, _: SweepMergeId) -> Result<BrepId> {
        Err(SolidError::invalid_input(
            "primitive fixture",
            "no merge engine",
        ))
    }
    fn copy_faces(
        &mut self,
        source: BrepId,
        _: &[FaceId],
        destination: BrepId,
        _: SweepMergeId,
        _: &mut solidcore::sweep::topology_sweep::access::BTreeEdgeMap,
    ) -> Result<()> {
        self.brep(source)?;
        self.brep(destination)?;
        Err(SolidError::invalid_input("fixture", "no merge engine"))
    }
    fn delete_merge(&mut self, _: SweepMergeId) {}
    fn tag(&self, e: SweepEntity) -> Result<Option<TagData>> {
        Ok(self.tags.get(&e).copied())
    }
    fn add_tag(&mut self, e: SweepEntity, tag: TagData) -> Result<()> {
        self.tags.insert(e, tag);
        Ok(())
    }
    fn curve_tag(&self, edge: EdgeRef) -> Result<Option<TagData>> {
        // The fixture's curves carry no tag attributes.
        self.brep(edge.0)?
            .edge(edge.1)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer("primitive_p3_fixture"))?;
        Ok(None)
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
#[cfg(test)]
impl PrimitiveAccess for EmptyModel {
    fn delete_primitive_faces(&mut self, id: BrepId, faces: &[FaceId]) -> Result<()> {
        if id != BrepId::from_index(0) {
            return unavailable();
        }
        // `SmTopologyIntersector::DeleteFaces` under the fixture's own face keys.
        solidcore::boolean::merge::p4::KernelManifoldIntersector::delete_faces_with_keys(
            &mut self.k,
            &mut self.b,
            faces,
            &mut self.notification_records,
            &mut self.notification_attributes,
            &fixture_key,
        )
    }
    fn with_primitive_brep<T>(
        &mut self,
        _: BrepId,
        body: impl FnOnce(&mut Kernel, &mut Brep) -> Result<T>,
    ) -> Result<T> {
        body(&mut self.k, &mut self.b)
    }
}
// Keep evaluation errors in the record; missing ported dispatch remains an
// honest deferred result rather than being normalized to an upstream error.
fn geometry_point(out: &mut Vec<f64>, value: Result<Vec3>) -> Result<()> {
    match value {
        Ok(point) => {
            out.push(0.0);
            out.extend(point.to_array());
        }
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => out.push(error(&e)),
    }
    Ok(())
}

// Native owning-list order, with actual evaluated geometry on successful builds.
// Failed operations still compare their partial owning-list counts without attempting
// to evaluate geometry the operation did not finish constructing.
fn construction_geometry(k: &Kernel, b: &Brep, success: bool, out: &mut Vec<f64>) -> Result<()> {
    out.extend([
        b.face_list().len() as f64,
        b.edge_list().len() as f64,
        b.vertex_list().len() as f64,
    ]);
    if !success {
        return Ok(());
    }
    #[cfg(test)]
    crate::certify::written(b);
    for &id in b.vertex_list() {
        out.push(1.0); // Native owning lists contain non-null vertices.
        out.extend(b.vertex(id)?.point.to_array());
    }
    for &id in b.edge_list() {
        let edge = b.edge(id)?;
        out.push(f64::from(edge.curve.is_some()));
        if let Some(curve) = &edge.curve {
            out.extend([edge.interval.min(), edge.interval.max()]);
            for i in 0..=4 {
                geometry_point(
                    out,
                    curve.evaluate_point(k, edge.interval.evaluate(f64::from(i) / 4.0)),
                )?;
            }
        }
    }
    for &id in b.face_list() {
        let face = b.face(id)?;
        out.push(f64::from(face.surface.is_some()));
        if let Some(surface) = &face.surface {
            let domain = face.uv_domain;
            out.extend([
                domain.min().x,
                domain.min().y,
                domain.max().x,
                domain.max().y,
            ]);
            for u in 0..=2 {
                for v in 0..=2 {
                    geometry_point(
                        out,
                        surface.evaluate_point(
                            k,
                            domain.evaluate(f64::from(u) / 2.0, f64::from(v) / 2.0),
                        ),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn entry(i: &Inputs) -> Result<Vec<f64>> {
    let data = &i.data;
    // Match the native SmBrep constructor and Pc3Probe(region, 0, false).
    // The production model resolves the real topology services; the minimal
    // EmptyModel below is retained only for focused default-adapter tests.
    let mut kernel = Kernel::default();
    let brep = solidcore::topology::brep::p3::new_brep(
        &mut kernel,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    )?;
    let mut primitive = PrimitiveCreation::new(brep.infinite_region(), 0, false);
    let mut model = PrimitiveModel::new(&mut kernel, brep);
    let mut curves: Vec<_> = data[10..]
        .as_chunks::<7>()
        .0
        .iter()
        .map(|v| curve(v).map(|c| Some(Curve::from(c))))
        .collect::<Result<_>>()?;
    let b = PrimitiveModel::MAIN;
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
    let mut record = vec![
        result.as_ref().err().map_or(0., error),
        f64::from(model.brep(b)?.editing_enabled()),
        f64::from(model.kernel(b)?.context.doing_boolean()),
        starts.len() as f64,
        sides.len() as f64,
        ends.len() as f64,
    ];
    construction_geometry(
        model.kernel(b)?,
        model.brep(b)?,
        result.is_ok(),
        &mut record,
    )?;
    Ok(record)
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
    fn run_rust(&self, case: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let result = if !layout_ok(i.op, &i.data) {
            Err(SolidError::invalid_input(
                "difftest::primitive_p3",
                "record layout",
            ))
        } else if i.op == 0 {
            ruled(&i.data)
        } else {
            entry(&i)
        };
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
    fn production_constructions_record_nonempty_geometry() {
        let context = crate::CaseContext {
            root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            seeds: 0,
        };
        let cases = context.fixture_cases(Runner.id()).unwrap();
        for (name, counts) in [
            ("linear_repeated_wire", [2.0, 7.0, 6.0]),
            ("partial_rotation_line", [1.0, 4.0, 4.0]),
            ("skin_two_lines", [1.0, 4.0, 4.0]),
            ("curve_sweep_line", [1.0, 4.0, 4.0]),
            ("swung_one_path_profile", [1.0, 4.0, 4.0]),
            ("zero_curve_sweep_approximation_tolerance", [1.0, 4.0, 4.0]),
            (
                "negative_curve_sweep_approximation_tolerance",
                [1.0, 4.0, 4.0],
            ),
            ("negative_skin_approximation_tolerance", [1.0, 4.0, 4.0]),
            ("negative_swung_tolerance_with_profile", [1.0, 4.0, 4.0]),
        ] {
            let case = cases.iter().find(|case| case.name == name).unwrap();
            let inputs: Inputs = case.parse().unwrap();
            let rust = entry(&inputs).unwrap();
            assert_eq!(&rust[6..9], &counts, "{name}: topology");
            assert_eq!(rust[0], 0.0, "{name}: construction success");
            // Includes actual vertex positions, curve intervals/points and face
            // domains/points, rather than an empty or status-only success.
            assert!(rust.len() > 100, "{name}: geometry was not recorded");
        }
    }

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

/// tangency-labs/solidcraft#14425 (WR-448's `revolve_analytic_profile_faces` retired): the
/// rotational rows it moved, replayed and checked against the exact solid of revolution.
///
/// Every moved row's profile lies in a plane containing the turn axis. The faces of the
/// solid of revolution then lie on the turned carriers of the profile curves (a segment's
/// line, an arc's circle) and on the two end planes, the profile's plane turned through 0
/// and `theta`. Turning a point `P` about the axis keeps its distance to a turned carrier,
/// so with `h = a . (P - c)`, `r = |P - c - h a|` and `u` a unit vector normal to `a`, the
/// distance from either fold `c + h a +- r u` to the profile curve's carrier bounds `P`'s
/// distance to that carrier turned.
#[cfg(test)]
mod moved_rows {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
    use super::*;
    use crate::interval::{sin_cos_deg, Iv};

    /// The rows #14425 moved whose samples moved (all profiles in a plane through the axis).
    const SAMPLE_ROWS: [&str; 11] = [
        "capped_rot_half_disc_axis_on_chord_270",
        "capped_rot_half_disc_axis_on_chord_90",
        "capped_rot_half_disc_axis_y_outside_270",
        "capped_rot_half_disc_axis_y_outside_90",
        "capped_rot_square_axis_diag_vertex_touch_270",
        "capped_rot_square_axis_diag_vertex_touch_90",
        "capped_rot_square_axis_y_on_edge_270",
        "capped_rot_square_axis_y_on_edge_90",
        "capped_rot_square_axis_y_outside_270",
        "capped_rot_square_axis_y_outside_90",
        "partial_rotation_line",
    ];

    fn case(name: &str) -> Inputs {
        let ctx = crate::CaseContext {
            root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            seeds: 0,
        };
        let cases = ctx.fixture_cases(Runner.id()).unwrap();
        cases
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name}"))
            .parse()
            .unwrap()
    }

    /// An enclosed vector.
    type V = [Iv; 3];

    fn v(p: Vec3) -> V {
        [Iv::exact(p.x), Iv::exact(p.y), Iv::exact(p.z)]
    }
    fn sub(a: V, b: V) -> V {
        std::array::from_fn(|n| a[n].sub(b[n]))
    }
    fn add(a: V, b: V) -> V {
        std::array::from_fn(|n| a[n].add(b[n]))
    }
    fn scale(a: V, s: Iv) -> V {
        a.map(|x| x.mul(s))
    }
    fn dot(a: V, b: V) -> Iv {
        a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
    }
    fn cross(a: V, b: V) -> V {
        [
            a[1].mul(b[2]).sub(a[2].mul(b[1])),
            a[2].mul(b[0]).sub(a[0].mul(b[2])),
            a[0].mul(b[1]).sub(a[1].mul(b[0])),
        ]
    }
    fn unit(a: V) -> V {
        let len = dot(a, a).sqrt();
        a.map(|x| x.div(len))
    }

    /// A profile curve as the fixture gives it: a segment, or an arc about `centre` in the
    /// `xy` plane of radius `radius` from angle `from` to `to` (degrees, from x).
    #[derive(Clone, Copy)]
    enum Piece {
        Segment(Vec3, Vec3),
        Arc { centre: Vec3, radius: f64, from: f64, to: f64 },
    }

    impl Piece {
        fn new(v: &[f64]) -> Self {
            if v[0] == 0. {
                Self::Segment(Vec3::new(v[1], v[2], v[3]), Vec3::new(v[4], v[5], v[6]))
            } else {
                Self::Arc { centre: Vec3::new(v[1], v[2], v[3]), radius: v[4], from: v[5], to: v[6] }
            }
        }

        /// An enclosure of the distance from `q` to the piece's carrier (a segment's line,
        /// or an arc's whole circle).
        fn distance(&self, q: V) -> Iv {
            match *self {
                Self::Segment(p0, p1) => {
                    let d = sub(v(p1), v(p0));
                    let w = sub(q, v(p0));
                    let c = cross(w, d);
                    dot(c, c).sqrt().div(dot(d, d).sqrt())
                }
                Self::Arc { centre, radius, .. } => {
                    let w = sub(q, v(centre));
                    let radial = w[0].mul(w[0]).add(w[1].mul(w[1])).sqrt().sub(Iv::exact(radius));
                    radial.mul(radial).add(w[2].mul(w[2])).sqrt()
                }
            }
        }

        /// The piece's defining points (segment ends; an arc's centre and the circle's
        /// point along x).
        fn points(&self) -> [Vec3; 2] {
            match *self {
                Self::Segment(p0, p1) => [p0, p1],
                Self::Arc { centre, radius, .. } => [centre, centre + Vec3::new(radius, 0.0, 0.0)],
            }
        }

        /// Points of the piece itself (in floating point): a segment's ends, an arc's ends
        /// and midpoint.
        fn on(&self) -> Vec<Vec3> {
            match *self {
                Self::Segment(p0, p1) => vec![p0, p1],
                Self::Arc { centre, radius, from, to } => [from, 0.5 * (from + to), to]
                    .iter()
                    .map(|deg| {
                        let t = deg.to_radians();
                        centre + Vec3::new(radius * t.cos(), radius * t.sin(), 0.0)
                    })
                    .collect(),
            }
        }
    }

    /// A face carrier: a profile piece turned (by index), or the profile plane turned
    /// through 0 (`Cap(0)`) or `theta` (`Cap(1)`).
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Carrier {
        Side(usize),
        Cap(usize),
    }

    /// Each moved row's record is its body's (the record rebuilt from the body is bitwise
    /// the same), the body is sound (no defect in the soundness check; a manifold solid for
    /// a capped profile, a sheet for `partial_rotation_line`), every face's boundary (its
    /// edges' samples and vertices) is in the swept sector (for a profile across the axis,
    /// the sector or its half-turn), and each face
    /// of the body is assigned the one carrier (a profile piece
    /// turned, or an end plane) that its samples, its edges' samples and its vertices are
    /// all on, each within its own entity's tolerance (the face's, the edge's, the
    /// vertex's); no two faces share a carrier, except that profile segments mirrored
    /// across the axis turn to one cone (each segment's ends are on the other's turned
    /// carrier), which then carries as many faces at most as it has segments; an end plane
    /// of a profile on one side of the axis carries only its half-plane
    /// (`capped_rot_square_axis_diag_vertex_touch_*`: two faces on the cone of sides 0
    /// and 3). The distances are enclosed in interval
    /// arithmetic from the exact inputs (the unit axis, the fold direction `u` and the
    /// end planes' normals, with `sin`/`cos` enclosed, all derived from them), so the
    /// bounds are certain; the tolerances are the body's own, so no allowance is chosen
    /// here. A face sample is taken across the face's parameter box, so it is on the
    /// face's surface though not always inside its trim.
    #[test]
    fn every_moved_record_is_the_exact_solid_of_revolution_sampled() {
        for name in SAMPLE_ROWS {
            let i = case(name);
            let d = &i.data;
            let c = Vec3::new(d[0], d[1], d[2]);
            let a = unit(v(Vec3::new(d[3], d[4], d[5])));
            let theta = d[6];
            let pieces: Vec<Piece> = d[10..].chunks(7).map(Piece::new).collect();
            // `u`: the unit direction from the axis to the profile point farthest from it
            // (chosen in floating point, computed enclosed)
            let off = |p: Vec3| {
                let w = sub(v(p), v(c));
                sub(w, scale(a, dot(a, w)))
            };
            let far = pieces
                .iter()
                .flat_map(Piece::on)
                .max_by(|p, q| dot(off(*p), off(*p)).lo.total_cmp(&dot(off(*q), off(*q)).lo))
                .unwrap();
            let u = unit(off(far));
            let n0 = cross(a, u);
            let normals = [0.0, theta].map(|deg| {
                let (s, co) = sin_cos_deg(deg);
                add(scale(n0, co), scale(cross(a, n0), s))
            });
            // the end half-planes' directions: `u` turned through 0 and `theta`
            let dirs = [0.0, theta].map(|deg| {
                let (s, co) = sin_cos_deg(deg);
                add(scale(u, co), scale(n0, s))
            });
            // whether the profile reaches across the axis (a segment end certainly on the
            // far side of it from `u`); the arcs here close on a segment, so they do not
            let straddles = pieces.iter().any(|piece| match piece {
                Piece::Segment(p0, p1) => [p0, p1].iter().any(|&&q| dot(off(q), u).hi < 0.0),
                Piece::Arc { .. } => false,
            });
            // `p` is in the swept sector within `tol`: a point at angle `phi` from `u` and
            // distance `r` from the axis has `normals[0] . w = r sin(phi)` and
            // `normals[1] . w = r sin(phi - theta)`, so the sector `0 <= phi <= theta` is
            // where the first is not negative and the second not positive (both, up to a
            // half turn; either, past it). A profile across the axis also sweeps the
            // opposite sector, where `w` turned a half turn (`-w`) is in this one.
            let in_sector = |p: Vec3, tol: f64| {
                let w = sub(v(p), v(c));
                let within = |w: V| {
                    let (from_start, to_end) = (dot(normals[0], w), dot(normals[1], w));
                    let (s, e) = (from_start.lo >= -tol, to_end.hi <= tol);
                    if theta <= 180.0 { s && e } else { s || e }
                };
                within(w) || (straddles && within(w.map(|x| Iv::ZERO.sub(x))))
            };
            crate::certify::watch_written();
            let record = entry(&i).unwrap();
            let mut bodies = crate::certify::take_written();
            assert_eq!(record[0], 0.0, "{name}: a success");
            assert_eq!(bodies.len(), 1, "{name}");
            let mut b = bodies.remove(0);
            {
                let mut k = Kernel::default();
                let found = solidcore::topology::brep::validation::soundness(&mut k, &mut b).unwrap();
                assert!(found.check.lines().is_empty(), "{name}: {:?}", found.check.lines());
                // a capped profile turns to a solid; `partial_rotation_line` to a sheet
                assert_eq!(found.manifold, name.starts_with("capped_"), "{name}: a manifold solid");
            }
            let k = Kernel::default();
            let mut again = Vec::new();
            construction_geometry(&k, &b, true, &mut again).unwrap();
            let _ = crate::certify::take_written();
            let tail = &record[record.len() - again.len()..];
            for (n, (r, x)) in tail.iter().zip(&again).enumerate() {
                assert_eq!(r.to_bits(), x.to_bits(), "{name}: record value {n}");
            }
            let distance = |carrier: Carrier, p: Vec3| -> Iv {
                let w = sub(v(p), v(c));
                match carrier {
                    Carrier::Side(n) => {
                        let h = dot(a, w);
                        let r = {
                            let o = sub(w, scale(a, h));
                            dot(o, o).sqrt()
                        };
                        let fold = |sg: f64| {
                            add(add(v(c), scale(a, h)), scale(u, r.mul(Iv::exact(sg))))
                        };
                        let (dp, dm) = (pieces[n].distance(fold(1.0)), pieces[n].distance(fold(-1.0)));
                        if dp.hi <= dm.hi { dp } else { dm }
                    }
                    Carrier::Cap(n) => dot(normals[n], w),
                }
            };
            let mut carriers: Vec<Carrier> = (0..pieces.len()).map(Carrier::Side).collect();
            carriers.extend([Carrier::Cap(0), Carrier::Cap(1)]);
            let mut used = Vec::new();
            for &f in b.face_list() {
                let face = b.face(f).unwrap();
                let surface = face.surface.as_ref().unwrap();
                let domain = face.uv_domain;
                let mut pts = Vec::new();
                for s in 0..=2 {
                    for t in 0..=2 {
                        let uv = domain.evaluate(f64::from(s) / 2.0, f64::from(t) / 2.0);
                        pts.push((surface.evaluate_point(&k, uv).unwrap(), face.tolerance));
                    }
                }
                for e in solidcore::topology::face::p4::edges(&b, f, None, false, None).unwrap() {
                    let er = b.edge(e).unwrap();
                    let curve = er.curve.as_ref().unwrap();
                    for s in 0..=4 {
                        let p = curve.evaluate_point(&k, er.interval.evaluate(f64::from(s) / 4.0));
                        pts.push((p.unwrap(), er.tolerance));
                    }
                }
                for vx in solidcore::topology::face::p4::vertices(&b, f, None, false, None).unwrap() {
                    let vr = b.vertex(vx).unwrap();
                    pts.push((vr.point, vr.tolerance));
                }
                // the face's boundary (its edges' samples and vertices, after the face's
                // own samples) bounds it, so it is what must be in the sector
                for &(p, tol) in &pts[9..] {
                    assert!(in_sector(p, tol), "{name}: face {f:?} point {p:?} outside the sector");
                }
                // an end plane of a profile on one side of the axis carries only its
                // half-plane: the points on the side of `dirs`
                let on_half = |cr: Carrier, p: Vec3, tol: f64| match cr {
                    Carrier::Cap(n) if !straddles => dot(dirs[n], sub(v(p), v(c))).lo >= -tol,
                    _ => true,
                };
                let found: Vec<Carrier> = carriers
                    .iter()
                    .copied()
                    .filter(|&cr| {
                        pts.iter().all(|&(p, tol)| distance(cr, p).mag() <= tol)
                            && pts[9..].iter().all(|&(p, tol)| on_half(cr, p, tol))
                    })
                    .collect();
                assert!(!found.is_empty(), "{name}: face {f:?} on no carrier");
                // Several carriers only when they are one surface: profile segments mirrored
                // across the axis turn to the same cone, so each one's ends are on every
                // other's turned carrier (within the face's tolerance).
                for &x in &found {
                    for &y in &found {
                        let (Carrier::Side(n), Carrier::Side(m)) = (x, y) else {
                            assert_eq!(x, y, "{name}: face {f:?} on carriers {found:?}");
                            continue;
                        };
                        if n == m {
                            continue;
                        }
                        assert!(
                            matches!(pieces[m], Piece::Segment(..))
                                && pieces[m]
                                    .points()
                                    .iter()
                                    .all(|&q| distance(Carrier::Side(n), q).mag() <= face.tolerance),
                            "{name}: face {f:?} on carriers {found:?}"
                        );
                    }
                }
                // No more faces on one surface than the carriers that make it up.
                let shared = used.iter().filter(|&&c| c == found[0]).count();
                assert!(shared < found.len(), "{name}: faces beyond {found:?}");
                used.push(found[0]);
            }
            assert!(!used.is_empty(), "{name}");
        }
    }

    /// `full_rotation_line`'s one moved value, the B-rep's editing state after the sweep:
    /// the sweep restores the caller's state, so every successful rotational row (whole or
    /// part turn) ends with the editing state of the fresh B-rep it was given.
    #[test]
    fn a_rotational_sweep_leaves_the_callers_editing_state() {
        let mut k = Kernel::default();
        let fresh = solidcore::topology::brep::p3::new_brep(
            &mut k,
            solidcore::geom::tol::USE_DEFAULT,
            solidcore::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        let want = f64::from(fresh.editing_enabled());
        let ctx = crate::CaseContext {
            root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            seeds: 0,
        };
        let mut seen = 0;
        for c in ctx.fixture_cases(Runner.id()).unwrap() {
            let i: Inputs = c.parse().unwrap();
            if !(i.op == 2 || i.op == 3) || !layout_ok(i.op, &i.data) {
                continue;
            }
            let record = entry(&i).unwrap();
            if record[0] == 0.0 {
                assert_eq!(record[1], want, "{}", c.name);
                seen += 1;
            }
        }
        assert!(seen > 10, "{seen}");
        assert_eq!(
            entry(&case("full_rotation_line")).unwrap()[..2],
            [0.0, want],
            "full_rotation_line"
        );
    }
}
