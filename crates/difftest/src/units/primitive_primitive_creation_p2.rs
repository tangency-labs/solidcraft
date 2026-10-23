//! Primitive p2 differential comparisons: the public `SmPrimitiveCreation` entry
//! points of the unit on a fresh B-rep with only its infinite region. Every record
//! compares the status, the B-rep's editing flag, the context's doing-boolean flag,
//! the four entity-map counts and the start, side and end face counts, plus the
//! skin's ownership flags, the taper's normal and whether the planar path returned
//! a B-rep. Construction reaches unported topology and defers.
#![allow(clippy::indexing_slicing)] // fixed fixture layout validated before indexing

use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::composite_curve::OffsetCornerType;
use solidcore::curve::curve::Curve;
use solidcore::error::{Result, SolidError};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::primitive_creation::{
    model::PrimitiveModel, p1::PrimitiveCreation, SweepOptions,
};
use solidcore::sweep::topology_sweep::access::SweepAccess;
use solidcore::topology::{Brep, BrepId, Region, RegionId};
use solidcore_ffi::units::primitive_primitive_creation_p2 as ffi;

/// Differential runner.
pub struct Runner;

#[derive(Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    expected_rejection: bool,
    op: i32,
    data: Vec<f64>,
}

/// The status code of an operation error, as the shim reports it.
fn error(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.,
        _ => 1001.,
    }
}

/// p1's curve record: a B-spline line or a quadratic circular arc.
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
fn curves(data: &[f64]) -> Result<Vec<Curve>> {
    data.as_chunks::<7>()
        .0
        .iter()
        .map(|record| curve(record).map(Curve::from))
        .collect()
}
fn frame(f: &[f64]) -> Axis2Placement {
    Axis2Placement::new(
        Vec3::new(f[0], f[1], f[2]),
        Vec3::new(f[3], f[4], f[5]),
        Vec3::new(f[6], f[7], f[8]),
    )
}
fn options(f: &[f64]) -> SweepOptions {
    SweepOptions {
        cap_ends: f[0] != 0.,
        translational_sweep: f[1] != 0.,
        move_profile_center_to_path: f[2] != 0.,
        orient_profile_perpendicular_to_path: f[3] != 0.,
        move_to_profile: f[4] != 0.,
    }
}

// The native fixture is a fresh B-rep with only its infinite region. The entry points
// run on it through the production adapter (`PrimitiveModel`), which also makes the
// scratch B-reps (`new SmBrep()`) and copies faces between them; the calls it cannot
// resolve yet fail as `Unported` with their unit.
fn fixture_brep(editing: bool) -> Result<Brep> {
    let mut b = Brep::new();
    let region = b.add_region_record(Region {
        in_brep: true,
        ..Default::default()
    })?;
    b.set_infinite_region(Some(region));
    b.set_editing_enabled(editing);
    Ok(b)
}
fn unavailable<T>() -> Result<T> {
    Err(SolidError::invalid_input(
        "primitive_p2_fixture",
        "fixture has no topology/service for this call",
    ))
}

/// Run one entry point on the fixture and build its record.
fn entry(i: &Inputs) -> Result<Vec<f64>> {
    let d = &i.data;
    let b = BrepId::from_index(0);
    let mut starts = Vec::new();
    let mut sides = Vec::new();
    let mut ends = Vec::new();
    let mut extra = Vec::new();
    if i.op == 9 {
        let mut k = Kernel::default();
        let mut model = PrimitiveModel::new(&mut k, fixture_brep(false)?);
        let profile_count = d[4] as usize;
        let profiles = curves(&d[6..6 + 7 * profile_count])?;
        let paths = curves(&d[6 + 7 * profile_count..])?;
        let result = PrimitiveCreation::create_sweep_along_planar_path(
            &mut model,
            b,
            &profiles,
            &paths,
            d[0],
            d[1] != 0.,
            d[2] != 0.,
            d[3] != 0.,
        );
        if let Err(e) = &result {
            if e.is_unported() {
                return Err(e.clone());
            }
        }
        return Ok(vec![
            result.as_ref().err().map_or(0., error),
            0.,
            f64::from(model.kernel(b)?.context.doing_boolean()),
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            f64::from(result.is_ok()),
        ]);
    }
    let mut k = Kernel::default();
    let mut model = PrimitiveModel::new(&mut k, fixture_brep(d[0] != 0.)?);
    let mut primitive = PrimitiveCreation::new(Some(RegionId::from_index(0)), 0, false);
    let result = match i.op {
        1 => primitive.create_partial_cone(
            &mut model,
            b,
            d[1],
            d[2],
            d[3],
            d[4],
            d[5],
            &frame(&d[6..15]),
        ),
        2 => primitive.create_partial_sphere(&mut model, b, d[1], d[2], d[3], &frame(&d[4..13])),
        3 => {
            primitive.create_partial_torus(&mut model, b, d[1], d[2], d[3], d[4], &frame(&d[5..14]))
        }
        4 => {
            let profile_count = d[4] as usize;
            let counts: Vec<usize> = d[5..5 + profile_count]
                .iter()
                .map(|c| *c as usize)
                .collect();
            let mut input: Vec<Option<Curve>> = curves(&d[5 + profile_count..])?
                .into_iter()
                .map(Some)
                .collect();
            let mut transferred = Vec::new();
            let result = primitive.create_skin_primitive(
                &mut model,
                b,
                &counts,
                &mut input,
                d[1],
                d[2] as usize,
                d[3] != 0.,
                &mut starts,
                &mut sides,
                &mut ends,
                &mut transferred,
            );
            // the native report has one flag per input curve
            extra.extend(
                (0..input.len()).map(|j| f64::from(transferred.get(j).copied().unwrap_or(false))),
            );
            result
        }
        5 | 6 => {
            let path = curve(&d[7..14])?;
            if i.op == 5 {
                primitive.create_curve_sweep_from_edges(
                    &mut model,
                    b,
                    &[],
                    &path,
                    None,
                    None,
                    d[1],
                    options(&d[2..7]),
                    &mut starts,
                    &mut sides,
                    &mut ends,
                )
            } else {
                primitive.create_curve_sweep_from_faces(
                    &mut model,
                    b,
                    &[],
                    &path,
                    None,
                    None,
                    d[1],
                    options(&d[2..7]),
                    &mut starts,
                    &mut sides,
                    &mut ends,
                )
            }
        }
        7 => {
            let path = curve(&d[5..12])?;
            primitive.create_pipe_sweep(
                &mut model,
                b,
                d[1],
                &path,
                d[2],
                d[3] != 0.,
                d[4] != 0.,
                &mut starts,
                &mut sides,
                &mut ends,
            )
        }
        8 => {
            let mut normal = Vec3::new(d[4], d[5], d[6]);
            let corner = match d[7] as i64 {
                0 => OffsetCornerType::LinearExtension,
                1 => OffsetCornerType::FilletCorner,
                _ => OffsetCornerType::LinearChamfer,
            };
            let result = primitive.create_taper_extrude(
                &mut model,
                b,
                curves(&d[8..])?,
                d[1],
                d[2],
                d[3] as i32,
                &mut normal,
                corner,
            );
            extra.extend(normal.to_array());
            result
        }
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
        primitive.maps.to_same.len() as f64,
        primitive.maps.to_higher.len() as f64,
        primitive.maps.from_same.len() as f64,
        primitive.maps.from_lower.len() as f64,
        starts.len() as f64,
        sides.len() as f64,
        ends.len() as f64,
    ];
    record.extend(extra);
    Ok(record)
}

/// A generated case near the unit's boundaries: each op, scale `1e-6..1e6`.
fn generated(rng: &mut Rng) -> (i32, Vec<f64>) {
    let scale = 10f64.powi(rng.range_i64(-6, 6) as i32);
    let edit = f64::from(rng.below(2) != 0);
    // the frame: origin, an X axis and a Y axis not parallel to it
    let mut f = vec![
        rng.uniform(-1., 1.) * scale,
        rng.uniform(-1., 1.) * scale,
        rng.uniform(-1., 1.) * scale,
        1.,
        rng.uniform(-0.5, 0.5),
        0.,
        0.,
        1.,
        rng.uniform(-0.5, 0.5),
    ];
    if rng.below(4) == 0 {
        f.splice(3..9, [0., 0., 1., 1., 0., 0.]);
    }
    // a turn near its boundaries: zero, the effective zero, a part turn, a full turn
    let start = [0., -45., 30.][rng.below(3) as usize];
    let turn = [
        0.,
        1e-13,
        2e-12,
        90.,
        360.,
        360. * (1. + 1e-13),
        360. * (1. + 1e-11),
        359.,
    ][rng.below(8) as usize];
    let size = |rng: &mut Rng| {
        [0., 1e-13, 5e-12, 1e-6, 1., 2.][rng.below(6) as usize]
            * if rng.below(2) == 0 { scale } else { 1. }
    };
    match rng.below(7) {
        0 => {
            let mut d = vec![edit, size(rng), size(rng), size(rng), start, start + turn];
            d.extend(&f);
            (1, d)
        }
        1 => {
            let mut d = vec![edit, size(rng), start, start + turn];
            d.extend(&f);
            (2, d)
        }
        2 => {
            let major = size(rng);
            let minor = if rng.below(4) == 0 { major } else { size(rng) };
            let mut d = vec![edit, major, minor, start, start + turn];
            d.extend(&f);
            (3, d)
        }
        3 => {
            // a skin of zero, one or two profiles of one circle each, at scale 1e-3 or
            // more (smaller skins take seconds to minutes natively, HANG-35)
            let side = scale.max(1e-3);
            let profiles = rng.below(3) as usize;
            let mut d = vec![
                edit,
                0.,
                1. + rng.below(3) as f64,
                f64::from(rng.below(2) != 0),
            ];
            d.push(profiles as f64);
            d.extend(std::iter::repeat_n(1., profiles));
            for p in 0..profiles {
                d.extend([1., 0., 0., p as f64 * side, side, 0., 360.]);
            }
            (4, d)
        }
        4 => {
            let mut d = vec![edit, scale * 1e-5];
            d.extend((0..5).map(|_| f64::from(rng.below(2) != 0)));
            d.extend([0., 0., 0., 0., 0., 0., scale]);
            (5 + rng.below(2) as i32, d)
        }
        5 => (
            7,
            vec![
                edit,
                size(rng),
                0.,
                f64::from(rng.below(2) != 0),
                f64::from(rng.below(2) != 0),
                0.,
                0.,
                0.,
                0.,
                0.,
                0.,
                scale,
            ],
        ),
        _ => {
            // a square taper (sides of at least 1e-3, UB-112) or a planar path
            if rng.below(2) == 0 {
                let side = scale.max(1e-3);
                let normal = [0., 1., -1.][rng.below(3) as usize];
                let mut d = vec![
                    edit,
                    scale,
                    rng.uniform(-30., 30.),
                    rng.below(4) as f64,
                    0.,
                    0.,
                    normal,
                    rng.below(3) as f64,
                ];
                let mut lines = vec![
                    [0., 0., 1., 0.],
                    [1., 0., 1., 1.],
                    [1., 1., 0., 1.],
                    [0., 1., 0., 0.],
                ];
                let pick = |rng: &mut Rng, [x0, y0, x1, y1]: [f64; 4]| {
                    if rng.below(2) == 0 {
                        [x0, y0, x1, y1]
                    } else {
                        [x1, y1, x0, y0]
                    }
                };
                // UB-50's chaining: a repeated side, either way (kept), a line with its
                // repeat alone (refused), a line back past a short first link (refused
                // for 3e-5, kept for 5e-5)
                match rng.below(4) {
                    0 => {
                        let side = lines[rng.below(4) as usize];
                        let repeat = pick(rng, side);
                        lines.push(repeat);
                    }
                    1 => lines = vec![[0., 0., 1., 0.], pick(rng, [0., 0., 1., 0.])],
                    2 => {
                        let short = [3e-5, 5e-5][rng.below(2) as usize] / side;
                        lines = vec![[0., 0., short, 0.], [short, 0., 1., 0.], [1., 0., 0., 0.]];
                    }
                    _ => {}
                }
                for [x0, y0, x1, y1] in lines {
                    d.extend([0., x0 * side, y0 * side, 0., x1 * side, y1 * side, 0.]);
                }
                (8, d)
            } else {
                // at scale 1e-3 or more: a 1e-4 path took over a minute natively
                let side = scale.max(1e-3);
                let mut d = vec![1e-5 * side, 1., 1., f64::from(rng.below(2) != 0), 1., 2.];
                d.extend([1., 0., 0., 0., 0.1 * side, 0., 360.]);
                d.extend([0., 0., 0., 0., 0., 0., side]);
                d.extend([0., 0., 0., side, side, 0., side]);
                (9, d)
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.primitive_creation_p2"
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
            Err(solidcore_ffi::FfiError::InvalidArgument(_)) if i.expected_rejection => Ok(
                Output::rejected("guarded native crash or hang (fixtures README)"),
            ),
            Err(solidcore_ffi::FfiError::Status(s)) if s == solidcore_ffi::SC_ERR_ARG => {
                Ok(Output::rejected("native curve construction"))
            }
            Err(e) => Err(HarnessError::new(e.to_string())),
        }
    }
    fn run_rust(&self, case: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        ffi::validate(i.op, &i.data).map_err(|e| HarnessError::new(e.to_string()))?;
        match entry(&i) {
            Ok(values) => Ok(Output::new().values("record", values)),
            Err(e) => {
                Output::unported(&e).map_or_else(|| Err(HarnessError::new(e.to_string())), Ok)
            }
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // as curve.curve_p1: draw again when the native guard refuses the input
        for _ in 0..64 {
            let (op, data) = generated(rng);
            if ffi::validate(op, &data).is_err() {
                continue;
            }
            return serde_json::to_value(Inputs {
                expected_rejection: false,
                op,
                data,
            })
            .ok();
        }
        None
    }
}
