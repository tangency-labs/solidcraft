//! Differential runner for `query.face_p1`.
//! The curve API subset compares its status and all seeded optional outputs.
//! Other owned face/shell/region queries are still being ported; no completion
//! claim is made from this subset.

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::vector3d::Vec3;
use solidcore::query::face::p1;
use solidcore::SolidError;
use solidcore_ffi::units::query_face_p1 as ffi;

/// Differential coverage of the six `query.face_p1` operations.
pub struct Runner;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Parameter {
    Number(f64),
    Special(String),
}

impl Parameter {
    fn value(&self) -> Result<f64, HarnessError> {
        match self {
            Self::Number(x) => Ok(*x),
            Self::Special(x) if x == "NaN" => Ok(f64::NAN),
            Self::Special(x) if x == "+inf" => Ok(f64::INFINITY),
            Self::Special(x) if x == "-inf" => Ok(f64::NEG_INFINITY),
            Self::Special(_) => Err(HarnessError::new("unknown special parameter")),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Input {
    kind: i32,
    data: [f64; 12],
    parameter: Parameter,
    mask: i32,
}

fn output(status: i32, values: [f64; 9]) -> Output {
    Output::new()
        .int("status", i64::from(status))
        .values("out", values.to_vec())
}

impl UnitDiff for Runner {
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(600),
                ..ctx.clone()
            },
        )
    }

    fn id(&self) -> &'static str {
        "query.face_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("brep").is_some() {
            return run_box(case, true);
        }
        let input: Input = case.parse()?;
        let (status, values) = ffi::evaluate_curve(
            input.kind,
            &input.data,
            input.parameter.value()?,
            input.mask,
        )
        .map_err(|e| HarnessError::new(e.to_string()))?;
        Ok(output(status, values))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("brep").is_some() {
            return run_box(case, false);
        }
        let input: Input = case.parse()?;
        let [px, py, pz, dx, dy, dz, qx, qy, qz, ex, ey, ez] = input.data;
        let p = Vec3::new(px, py, pz);
        let d = Vec3::new(dx, dy, dz);
        let q = Vec3::new(qx, qy, qz);
        let e = Vec3::new(ex, ey, ez);
        let curve: Option<Curve> = match input.kind {
            0 => None,
            1 => Some(Line::from_points(p, q, 3).into()),
            2 => Some(HermiteCurve::new(p, d, q, e, 3).into()),
            _ => return Err(HarnessError::new("unknown curve kind")),
        };
        let mut values = [11., 13., 17., 19., 23., 29., 31., 37., 41.];
        match p1::evaluate_curve(curve.as_ref(), input.parameter.value()?) {
            Ok(result) => {
                for ((i, vector), destination) in
                    result.iter().enumerate().zip(values.chunks_exact_mut(3))
                {
                    if input.mask & (1 << i) != 0 {
                        destination.copy_from_slice(&[vector.x, vector.y, vector.z]);
                    }
                }
                Ok(output(0, values))
            }
            Err(SolidError::InvalidInput { .. }) => Ok(output(1007, values)),
            Err(SolidError::Upstream { code, .. }) => Ok(output(code, values)),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = match rng.below(5) {
            0 => 1e-200,
            1 => 1e-6,
            2 => 1.0,
            3 => 1e6,
            _ => 1e100,
        };
        let mut data = [0.0; 12];
        for item in &mut data {
            *item = rng.uniform(-100.0, 100.0) * scale;
        }
        serde_json::to_value(Input {
            kind: 1 + rng.below(2) as i32,
            data,
            parameter: Parameter::Number(rng.unit()),
            mask: 1 + rng.below(7) as i32,
        })
        .ok()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoxInput {
    brep: Value,
    target: u8,
    index: usize,
    tight: bool,
    use_default: bool,
}

fn run_box(case: &Case, cpp: bool) -> Result<Output, HarnessError> {
    use super::topology_edgeuse_p1::{fixture_input, import};
    use solidcore::geom::extent3d::Extent3d;
    use solidcore::kernel::Kernel;
    use solidcore::topology::{FaceId, RegionId, ShellId};
    use solidcore_ffi::units::topology_edgeuse_p1 as graph;
    let input: BoxInput = case.parse()?;
    let (path, block) = fixture_input(input.brep)?;
    if cpp {
        let values = ffi::bounding_box(
            input.target,
            input.index,
            input.tight,
            input.use_default,
            &path,
            &block,
        )
        .map_err(|e| HarnessError::new(e.to_string()))?;
        if values.len() != 7 {
            return Err(HarnessError::new("malformed native box record"));
        }
        let (status, rest) = values
            .split_first()
            .ok_or_else(|| HarnessError::new("missing native status"))?;
        return Ok(Output::new()
            .int("status", *status as i64)
            .values("out", rest.to_vec()));
    }
    let export = graph::eval(graph::Op::Export, &path, &block)
        .map_err(|e| HarnessError::new(e.to_string()))?;
    let brep = import(&export)?;
    let mut box_ = Extent3d::from_points(Vec3::new(-17., -19., -23.), Vec3::new(29., 31., 37.));
    let tight = input.tight && !input.use_default;
    let kernel = Kernel::default();
    let id = u32::try_from(input.index).map_err(|_| HarnessError::new("index too large"))?;
    let result = match input.target {
        0 => p1::calculate_bounding_box(&kernel, &brep, FaceId::from_index(id), tight, &mut box_),
        1 => p1::calculate_shell_bounding_box(
            &kernel,
            &brep,
            ShellId::from_index(id),
            tight,
            &mut box_,
        ),
        2 => p1::calculate_region_bounding_box(
            &kernel,
            &brep,
            RegionId::from_index(id),
            tight,
            &mut box_,
        ),
        _ => return Err(HarnessError::new("unknown box target")),
    };
    let status = match result {
        Ok(()) => 0,
        Err(SolidError::Upstream { code, .. }) => code,
        Err(SolidError::InvalidInput { .. }) => 1007,
        Err(e) => return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
    };
    let min = box_.min();
    let max = box_.max();
    Ok(Output::new()
        .int("status", i64::from(status))
        .values("out", vec![min.x, min.y, min.z, max.x, max.y, max.z]))
}
