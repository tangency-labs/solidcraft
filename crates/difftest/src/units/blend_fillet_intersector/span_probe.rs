//! Diagnostic-only native test for `review_cases.json#72`, outside the corpus.
//!
//! Tags 1..=4 copy actual call arguments/results without extra evaluations.
//! Per-side bit identity against the untouched runner is the observer safety gate.
//! Internal intersection, Hermite and span calculations are not observed.

use super::*;
use solidcore::blend::fillet_custom_solver::{InitialValues, JacobianLoad};
use solidcore::blend::fillet_intersector::{
    FilStatus, FilletSolverOps as IntersectorOps, HandlingAlgorithmType, StepSolve,
};
use solidcore::blend::fillet_solver::p1::{
    FilletSolver, FilletSolverOps as SolverOps, FilletSolverType,
};
use solidcore::blend::fillet_solver::p3::StepSolveOutcome;
use solidcore::geom::extent1d::ExtentNd;
use solidcore::geom::matrix::Matrix;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::topology::Brep;
use solidcore::Result as KernelResult;
use std::io::Write;

#[derive(Debug)]
struct Event {
    tag: u32,
    values: Vec<f64>,
}

struct Observed {
    inner: ConstantRadiusAssistedFs,
    events: Vec<Event>,
}

// All unobserved members delegate to the existing concrete solver. In particular,
// do NOT replace SolverOps::point_on_plane_solve with the similarly named
// IntersectorOps member: StepSolve currently dispatches through SolverOps.
macro_rules! delegate_mut {
    ($trait:path; $(fn $name:ident($($arg:ident: $ty:ty),*) -> $result:ty;)*) => {
        $(fn $name(&mut self, $($arg: $ty),*) -> $result {
            <ConstantRadiusAssistedFs as $trait>::$name(&mut self.inner, $($arg),*)
        })*
    };
}
macro_rules! delegate_ref {
    ($trait:path; $(fn $name:ident($($arg:ident: $ty:ty),*) -> $result:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> $result {
            <ConstantRadiusAssistedFs as $trait>::$name(&self.inner, $($arg),*)
        })*
    };
}

fn result_status<T>(r: &KernelResult<T>) -> f64 {
    r.as_ref().err().map_or(0.0, status)
}

fn probe_error(reason: &'static str) -> SolidError {
    SolidError::invalid_input("difftest::blend_fillet_intersector::span_probe", reason)
}

impl SolverOps for Observed {
    delegate_ref! { SolverOps;
        fn solver() -> &FilletSolver;
        fn jacobian_size() -> KernelResult<usize>;
        fn offset_radii_can_change() -> KernelResult<bool>;
        fn setup_offset_extension(k: &Kernel, surface: &Surface, domain: &mut Extent2d) -> KernelResult<()>;
        fn can_make_analytic() -> KernelResult<bool>;
        fn type_code() -> KernelResult<i64>;
        fn is_kind_of(t: i64) -> KernelResult<bool>;
        fn solver_type() -> KernelResult<FilletSolverType>;
        fn constant_distance() -> KernelResult<f64>;
    }
    delegate_mut! { SolverOps;
        fn solver_mut() -> &mut FilletSolver;
        fn setup_offset_values(point: &TsectPnt, radius: Option<&mut f64>) -> KernelResult<()>;
        fn set_extension_flags(k: &Kernel, brep: &Brep, corners: &dyn solidcore::blend::fillet_solver::p1::ExtensionCorners) -> KernelResult<()>;
        fn load_jacobian(k: &Kernel, equations: &mut usize, parameters: &mut usize, load: &mut JacobianLoad<'_>, f: &mut [f64], jacobian: Option<&mut Matrix>) -> KernelResult<bool>;
        fn load_initial_values(k: &Kernel, brep: Option<&mut Brep>, do_surf2_calcs: bool, rail_index: usize, surface1: &mut OffsetSurface, surface1_index: usize, intervals: &mut ExtentNd, periodicities: &mut Vec<bool>, guess_t: &mut Vec<f64>) -> KernelResult<Option<InitialValues>>;
    }

    fn point_on_plane_solve(
        &mut self,
        k: &Kernel,
        brep: Option<&mut Brep>,
        origin: Vec3,
        normal: Vec3,
        d0: &Extent2d,
        d1: &Extent2d,
        uv0: Vec2,
        uv1: Vec2,
        point: &mut TsectPnt,
    ) -> KernelResult<bool> {
        let mut before = Vec::new();
        put_vec(&mut before, origin);
        put_vec(&mut before, normal);
        put_uv(&mut before, uv0);
        put_uv(&mut before, uv1);
        for slot in &self.inner.solver.surfaces {
            before.push(
                slot.as_ref()
                    .ok_or_else(|| probe_error("missing solver offset surface"))?
                    .surface
                    .offset_distance(),
            );
        }
        self.events.push(Event {
            tag: 1,
            values: before,
        });
        let result = SolverOps::point_on_plane_solve(
            &mut self.inner,
            k,
            brep,
            origin,
            normal,
            d0,
            d1,
            uv0,
            uv1,
            point,
        );
        let found = *result.as_ref().map_err(Clone::clone)?;
        let mut after = vec![result_status(&result), flag(found)];
        put_point(&mut after, point);
        self.events.push(Event {
            tag: 2,
            values: after,
        });
        result
    }
}

impl IntersectorOps for Observed {
    type FilletGeom = FilletGeomId;

    delegate_ref! { IntersectorOps;
        fn last_fillet_geom() -> KernelResult<FilletGeomId>;
        fn self_intersection_handler() -> KernelResult<Option<HandlingAlgorithmType>>;
        fn has_surface_generator() -> KernelResult<bool>;
        fn offset_radii_can_change() -> KernelResult<bool>;
    }
    delegate_mut! { IntersectorOps;
        fn compute_surface_values(k: &Kernel, uv: [Vec2; 2], point: &mut TsectPnt) -> KernelResult<()>;
        fn compute_uv_vectors(k: &Kernel, point: &mut TsectPnt) -> KernelResult<()>;
        fn point_on_plane_solve(k: &Kernel, origin: Vec3, normal: Vec3, d0: &Extent2d, d1: &Extent2d, uv0: Vec2, uv1: Vec2, point: &mut TsectPnt) -> KernelResult<bool>;
        fn set_surfaces_to_zero_offset() -> KernelResult<()>;
        fn reload_surface_offsets() -> KernelResult<()>;
        fn rail_surface_evaluate_1st_derivatives(k: &Kernel, rail: usize, uv: Vec2) -> KernelResult<(Vec3, Vec3, Vec3)>;
        fn set_status(value: FilStatus) -> KernelResult<()>;
        fn smooth_out_intersection(k: &Kernel, pseudo: &mut FilletBrep, geom: &FilletGeomId, curves: &mut Vec<BSplineCurve>, uv0: &mut Vec<BSplineCurve>, uv1: &mut Vec<BSplineCurve>) -> KernelResult<()>;
        fn create_fillet_surface(k: &Kernel, center: &mut BSplineCurve, curve0: &mut BSplineCurve, curve1: &mut BSplineCurve, points: &[TsectPnt], uv0: &mut BSplineCurve, uv1: &mut BSplineCurve) -> KernelResult<Option<BSplineSurface>>;
        fn set_fillet_surface(pseudo: &mut FilletBrep, geom: &FilletGeomId, surface: Option<BSplineSurface>) -> KernelResult<()>;
        fn set_center_line_curve(pseudo: &mut FilletBrep, geom: &FilletGeomId, curve: BSplineCurve) -> KernelResult<()>;
        fn set_rail_curve(pseudo: &mut FilletBrep, geom: &FilletGeomId, rail: usize, curve: BSplineCurve) -> KernelResult<()>;
        fn set_rail_mate_uv_curve(pseudo: &mut FilletBrep, geom: &FilletGeomId, rail: usize, curve: BSplineCurve) -> KernelResult<()>;
    }

    fn step_solve(
        &mut self,
        k: &Kernel,
        current: &TsectPnt,
        step: &mut f64,
        d0: &Extent2d,
        d1: &Extent2d,
        flags: &mut StepSolve,
        next: &mut TsectPnt,
    ) -> KernelResult<()> {
        let mut out = StepSolveOutcome::default();
        let result = FilletSolver::step_solve(self, k, None, current, step, d0, d1, next, &mut out);
        flags.found_answer = out.found_good_point;
        flags.clipped = out.clipped;
        flags.boundary_hit = out.boundary_hit;
        let mut values = vec![*step];
        put_point(&mut values, next);
        self.events.push(Event { tag: 3, values });
        result
    }

    fn compute_point_values(
        &mut self,
        k: &Kernel,
        uv: [Vec2; 2],
        d0: &Extent2d,
        d1: &Extent2d,
        dir: f64,
        point: &mut TsectPnt,
        previous: Option<&TsectPnt>,
    ) -> KernelResult<()> {
        let result = IntersectorOps::compute_point_values(
            &mut self.inner,
            k,
            uv,
            d0,
            d1,
            dir,
            point,
            previous,
        );
        let mut values = vec![result_status(&result)];
        put_point(&mut values, point);
        self.events.push(Event { tag: 4, values });
        result
    }
}

fn observed_rust(i: &Inputs) -> KernelResult<(Vec<f64>, Vec<Event>)> {
    // Same construction as rust_record, kept here so the ordinary runner does
    // not acquire an instrumentation branch, callback, new record or state.
    let k = Kernel::default();
    let a = rust_surface(&i.a).map_err(|_| probe_error("fixture surface construction failed"))?;
    let b =
        i.b.as_ref()
            .map(rust_surface)
            .transpose()
            .map_err(|_| probe_error("fixture surface construction failed"))?;
    let bref = b.as_ref().unwrap_or(&a);
    let s = &i.solver;
    let orient = s.orient.map(|f| if f { 1.0 } else { -1.0 });
    let [radius0, radius1] = s.radii;
    let [orientation0, orientation1] = orient;
    let off0 = rail_offset(&k, radius0 * orientation0, a.clone(), SurfaceId::Free(0))
        .map_err(|_| probe_error("fixture offset construction failed"))?;
    let off1 = rail_offset(
        &k,
        radius1 * orientation1,
        bref.clone(),
        SurfaceId::Free(u32::from(b.is_some())),
    )
    .map_err(|_| probe_error("fixture offset construction failed"))?;
    let mut inner = ConstantRadiusAssistedFs::from_parts(
        s.tol,
        s.ang_tol,
        s.tangency_tol,
        s.radii,
        [Some(off0), Some(off1)],
        orient,
        [None, None],
    );
    for slot in inner.solver.surfaces.iter_mut().flatten() {
        slot.surface.surface.set_allow_extension(true);
    }
    let mut pseudo = FilletBrep::new();
    inner
        .solver
        .add_constructor_fillet_geom(&k, &mut pseudo, FilletSolverId(0))?;
    let mut solver = Observed {
        inner,
        events: Vec::new(),
    };
    let mut fi: Fi<'_> = FilletIntersector::new(&k, &a, &ext(i.dom1), bref, &ext(i.dom2), None)?;
    fi.base.base.this_ang_tol_rad = i.ang_tol;
    fi.base.base.this_approx_tol_3d = i.approx_tol;
    fi.base.base.clipped_by_through_point = i.clipped;
    fi.base.base.curve_trace_direction = i.trace_dir;
    for p in &i.points {
        fi.base.base.alloc_point(
            rust_point(&point_values(p)).map_err(|_| probe_error("fixture point import failed"))?,
        );
    }
    let OpIn::Next {
        a: from,
        step,
        b: to,
    } = i.op
    else {
        return Err(probe_error("fixture must be Next"));
    };
    if i.clipped {
        return Err(probe_error(
            "probe requires Next without through-point clipping",
        ));
    }
    let out = fi.compute_next_point(&mut solver, &k, id(from), step, id(to))?;
    if !out.found_good_point || !out.clipped || out.boundary_hit {
        return Err(probe_error("unexpected Next outcome"));
    }
    let mut record = vec![
        2.0,
        0.0,
        flag(out.found_good_point),
        out.deviation_found
            .ok_or_else(|| probe_error("missing Next deviation"))?,
        out.angle_found_rad
            .ok_or_else(|| probe_error("missing Next angle"))?,
        flag(out.clipped),
        flag(out.boundary_hit),
    ];
    let si = &fi.base.base;
    let [domain0, domain1] = si.uv_domains;
    put_dom(&mut record, &domain0);
    put_dom(&mut record, &domain1);
    record.extend([
        si.this_ang_tol_rad,
        si.this_approx_tol_3d,
        flag(si.do_boundary_point_on_curve_test),
        flag(fi.curr_fillet_touch_boundary()),
        flag(fi.self_intersect()),
        flag(si.clipped_by_through_point),
        si.curve_trace_direction,
        flag(fi.is_kind_of(TYPE)),
        flag(fi.is_kind_of(ADV_TYPE)),
        flag(fi.is_kind_of(7)),
    ]);
    for slot in &solver.inner.solver.surfaces {
        record.push(
            slot.as_ref()
                .map_or(ffi::UNSET, |s| s.surface.offset_distance()),
        );
    }
    record.extend([si.curve_points().len() as f64, si.points().len() as f64]);
    for p in si.points() {
        put_point(&mut record, p);
    }
    Ok((record, solver.events))
}

fn take_count(raw: &[f64], maximum: usize) -> KernelResult<(usize, &[f64])> {
    let (&value, rest) = raw
        .split_first()
        .ok_or_else(|| probe_error("missing probe header"))?;
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=maximum as f64).contains(&value) {
        return Err(probe_error("invalid probe count or tag"));
    }
    Ok((value as usize, rest))
}

fn unpack_cpp(raw: &[f64]) -> KernelResult<(Vec<f64>, Vec<Event>)> {
    let (count, payload) = take_count(raw, raw.len())?;
    if count != 189 {
        return Err(probe_error("expected original 189-value C++ record"));
    }
    let record = payload
        .get(..count)
        .ok_or_else(|| probe_error("truncated original record"))?
        .to_vec();
    let mut rest = payload
        .get(count..)
        .ok_or_else(|| probe_error("truncated original record"))?;
    let mut events = Vec::new();
    while !rest.is_empty() {
        let (tag, payload) = take_count(rest, u32::MAX as usize)?;
        let tag = u32::try_from(tag).map_err(|_| probe_error("probe tag overflow"))?;
        let (len, payload) = take_count(payload, payload.len())?;
        events.push(Event {
            tag,
            values: payload
                .get(..len)
                .ok_or_else(|| probe_error("truncated checkpoint"))?
                .to_vec(),
        });
        rest = payload
            .get(len..)
            .ok_or_else(|| probe_error("truncated checkpoint"))?;
    }
    Ok((record, events))
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn label(tag: u32) -> KernelResult<&'static str> {
    Ok(match tag {
        1 => "observed.plane_input.origin_normal_uv_offsets",
        2 => "observed.plane_output.status_found_point80",
        3 => "observed.step_output.rescaled_step_point80",
        4 => "observed.point_values_output.status_point80",
        _ => return Err(probe_error("unknown checkpoint tag")),
    })
}

fn difference(cpp: &Event, rust: &Event) -> KernelResult<Option<Value>> {
    if cpp.tag != rust.tag || cpp.values.len() != rust.values.len() {
        return Err(probe_error("checkpoint shapes differ"));
    }
    let Some(component) = cpp
        .values
        .iter()
        .zip(&rust.values)
        .position(|(c, r)| c.to_bits() != r.to_bits())
    else {
        return Ok(None);
    };
    let c = cpp
        .values
        .get(component)
        .ok_or_else(|| probe_error("missing C++ component"))?;
    let r = rust
        .values
        .get(component)
        .ok_or_else(|| probe_error("missing Rust component"))?;
    Ok(Some(json!({
        "tag": cpp.tag, "checkpoint": label(cpp.tag)?, "component": component,
        "cpp": c, "rust": r, "cpp_bits": c.to_bits(), "rust_bits": r.to_bits(),
    })))
}

fn first_difference(cpp: &[Event], rust: &[Event]) -> KernelResult<Option<Value>> {
    if cpp.len() != rust.len() {
        return Err(probe_error("checkpoint counts differ"));
    }
    for (c, r) in cpp.iter().zip(rust) {
        if let Some(difference) = difference(c, r)? {
            return Ok(Some(difference));
        }
    }
    Ok(None)
}

#[test]
fn original_clipped_step_checkpoint_evidence() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../fixtures/blend.fillet_intersector/review_cases.json"
    ))
    .unwrap();
    let case = &fixture["cases"][72];
    assert_eq!(case["name"], "rv_next_clipped_step");
    let input: Inputs = serde_json::from_value(case["inputs"].clone()).unwrap();
    let plain_cpp = ffi::run(&ffi_program(&input)).unwrap();
    let plain_rust =
        rust_record(&Kernel::default(), &input).unwrap_or_else(|_| panic!("original Rust operation failed"));
    let (cpp_record, cpp_events) =
        unpack_cpp(&ffi::probe_next(&ffi_program(&input)).unwrap()).unwrap();
    let (rust_record, rust_events) = observed_rust(&input).unwrap();
    assert_eq!(plain_cpp.len(), 189);
    assert_eq!(plain_rust.len(), 189);
    assert_eq!(
        bits(&plain_cpp),
        bits(&cpp_record),
        "C++ observation changed original record"
    );
    assert_eq!(
        bits(&plain_rust),
        bits(&rust_record),
        "Rust observation changed original record"
    );
    let expected = [(1, 12), (2, 82), (3, 81), (4, 81)];
    for events in [&cpp_events, &rust_events] {
        assert_eq!(
            events
                .iter()
                .map(|e| (e.tag, e.values.len()))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(events.iter().flat_map(|e| &e.values).all(|v| v.is_finite()));
    }
    let first_observed = first_difference(&cpp_events, &rust_events).unwrap();
    // Direct stdout intentionally survives libtest's passing-test capture, so
    // canonical CI's existing native-feature test job retains the evidence. No
    // new tracing option, environment knob, file or corpus comparison is added.
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "FILLET_SPAN_PROBE {}", json!({
        "case": "review_cases.json#72 rv_next_clipped_step",
        "original_values": 189,
        "observer_records_bit_identical": true,
        "original_deviation": {"cpp": plain_cpp[3], "rust": plain_rust[3]},
        "first_observed_checkpoint_difference_not_internal_root_cause": first_observed,
        "limitation": "Actual calls only. Internal LocalCurveIntersect, Hermite and span calculations are not observed; checkpoint differences are not proof of an internal root cause."
    })).unwrap();
    for (c, r) in cpp_events.iter().zip(&rust_events) {
        writeln!(
            stdout,
            "FILLET_SPAN_PROBE {}",
            json!({
                "tag": c.tag, "checkpoint": label(c.tag).unwrap(),
                "cpp": c.values, "rust": r.values,
                "cpp_bits": bits(&c.values), "rust_bits": bits(&r.values),
            })
        )
        .unwrap();
    }
}
