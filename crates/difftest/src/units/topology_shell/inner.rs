//! `SmShell::IsInnerShellOf` cases (#1419, #1473): every pair of shells of bodies with
//! internal voids, built from boxes and spheres with Booleans on both sides
//! (`SmApiCreateBox`/`SmApiCreateSphere`/`SmApiBoolean` in the shim
//! `topology_shell_inner.cpp`, [`create_box`]/[`create_sphere`]/[`boolean`] in Rust).
//!
//! A case is `{"inner_shell_of": [[op, kind, x, y, z, a, b, c], ...]}` (see
//! [`ffi::inner_shell_of`]). The record is `n_regions (n_shells)*n_regions`, then per
//! shell (region and shell list order) `IsInnerShellOf(NULL)` and `IsInnerShellOf(other)`
//! for every other shell. No C++ state reaches the Rust side: the Rust kernel builds the
//! same bodies itself.

use serde_json::json;

use solidcore::boolean::api_brep::boolean;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::topology::brep::p1;
use solidcore::topology::shell::is_inner_shell_of;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::topology_shell as ffi;

use crate::{Case, HarnessError, Output};

/// One step: `[op, kind, x, y, z, a, b, c]`.
type Step = [f64; 8];

/// Whether `case` is an `IsInnerShellOf` case.
pub(super) fn is_inner(case: &Case) -> bool {
    case.inputs.get("inner_shell_of").is_some()
}

fn steps_of(case: &Case) -> Result<Vec<Step>, HarnessError> {
    let v = case
        .inputs
        .get("inner_shell_of")
        .cloned()
        .ok_or_else(|| HarnessError::new("no inner_shell_of steps"))?;
    serde_json::from_value(v).map_err(|e| HarnessError::new(e.to_string()))
}

/// The C++ side.
pub(super) fn run_cpp(case: &Case) -> Result<Output, HarnessError> {
    let steps = steps_of(case)?;
    let mut input = vec![steps.len() as f64];
    for s in &steps {
        input.extend(s);
    }
    let v = ffi::inner_shell_of(&input)
        .map_err(|e| HarnessError::new(format!("c++ IsInnerShellOf: {e}")))?;
    Ok(Output::new().values("out", v))
}

fn primitive(k: &mut Kernel, s: &Step) -> Result<Brep, SolidError> {
    let [_, kind, x, y, z, a, b, c] = *s;
    let o = Vec3::new(x, y, z);
    if kind == 1.0 {
        create_box(k, o, a, b, c)
    } else {
        create_sphere(k, o, a)
    }
}

fn rust_record(steps: &[Step]) -> Result<Vec<f64>, SolidError> {
    let mut k = Kernel::default();
    let mut body: Option<Brep> = None;
    for s in steps {
        let p = primitive(&mut k, s)?;
        body = Some(match body {
            None => p,
            Some(b) => {
                let op = BooleanOperation::from_code(s[0] as i32).ok_or(
                    SolidError::InvalidInput {
                        operation: "difftest::topology_shell::inner",
                        reason: "step operation",
                    },
                )?;
                boolean(&mut k, b, p, op)?.result
            }
        });
    }
    let mut b = body.ok_or(SolidError::InvalidInput {
        operation: "difftest::topology_shell::inner",
        reason: "no steps",
    })?;
    let regions = p1::regions(&b, None)?;
    let mut o = vec![regions.len() as f64];
    let mut shells = Vec::new();
    for r in regions {
        let sh = b.region(r)?.shells.clone();
        o.push(sh.len() as f64);
        shells.extend(sh);
    }
    for (i, &s) in shells.iter().enumerate() {
        o.push(f64::from(is_inner_shell_of(&mut k, &mut b, s, None)?));
        for (j, &t) in shells.iter().enumerate() {
            if i != j {
                o.push(f64::from(is_inner_shell_of(&mut k, &mut b, s, Some(t))?));
            }
        }
    }
    Ok(o)
}

/// The Rust side.
pub(super) fn run_rust(case: &Case) -> Result<Output, HarnessError> {
    let steps = steps_of(case)?;
    match rust_record(&steps) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(e) => Output::unported(&e)
            .ok_or_else(|| HarnessError::new(format!("rust IsInnerShellOf: {e}"))),
    }
}

fn bx(op: f64, x: f64, y: f64, z: f64, a: f64, b: f64, c: f64) -> Step {
    [op, 1.0, x, y, z, a, b, c]
}

fn sph(op: f64, x: f64, y: f64, z: f64, r: f64) -> Step {
    [op, 2.0, x, y, z, r, 0.0, 0.0]
}

/// The built-in bodies: solids with one or two box or sphere cavities, a void inside an
/// island inside a void, plain primitives, at 1e-3, 1 and 1e3 scales.
pub(super) fn cases(unit: &str) -> Vec<Case> {
    const UNION: f64 = 1.0;
    const DIFF: f64 = 3.0;
    let bodies: Vec<(&str, Vec<Step>)> = vec![
        ("box", vec![bx(0.0, 0.0, 0.0, 0.0, 20.0, 20.0, 20.0)]),
        ("sphere", vec![sph(0.0, 0.0, 0.0, 0.0, 10.0)]),
        (
            "box_minus_box",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 20.0, 20.0, 20.0),
                bx(DIFF, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0),
            ],
        ),
        (
            "box_minus_sphere",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 20.0, 20.0, 20.0),
                sph(DIFF, 10.0, 10.0, 10.0, 4.0),
            ],
        ),
        (
            "sphere_minus_box",
            vec![
                sph(0.0, 0.0, 0.0, 0.0, 20.0),
                bx(DIFF, -3.0, -4.0, -5.0, 6.0, 7.0, 8.0),
            ],
        ),
        (
            "box_minus_two_boxes",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 40.0, 20.0, 20.0),
                bx(DIFF, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0),
                bx(DIFF, 25.0, 5.0, 5.0, 10.0, 10.0, 10.0),
            ],
        ),
        (
            "box_minus_box_and_sphere",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 40.0, 20.0, 20.0),
                bx(DIFF, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0),
                sph(DIFF, 30.0, 10.0, 10.0, 4.0),
            ],
        ),
        (
            "island_in_void",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 30.0, 30.0, 30.0),
                bx(DIFF, 5.0, 5.0, 5.0, 20.0, 20.0, 20.0),
                bx(UNION, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0),
            ],
        ),
        (
            "void_in_island_in_void",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 40.0, 40.0, 40.0),
                bx(DIFF, 5.0, 5.0, 5.0, 30.0, 30.0, 30.0),
                bx(UNION, 10.0, 10.0, 10.0, 20.0, 20.0, 20.0),
                bx(DIFF, 15.0, 15.0, 15.0, 10.0, 10.0, 10.0),
            ],
        ),
        (
            "two_separate_boxes",
            vec![
                bx(0.0, 0.0, 0.0, 0.0, 10.0, 10.0, 10.0),
                bx(UNION, 20.0, 0.0, 0.0, 10.0, 10.0, 10.0),
            ],
        ),
        (
            "off_centre_cavity",
            vec![
                bx(0.0, -10.0, -7.0, -3.0, 20.0, 14.0, 6.0),
                bx(DIFF, -9.0, -6.0, -2.0, 1.0, 1.0, 1.0),
            ],
        ),
    ];
    let mut v = Vec::new();
    for scale in [1.0, 1e-1, 1e2] {
        for (name, steps) in &bodies {
            let scaled: Vec<Step> = steps
                .iter()
                .map(|s| {
                    let mut t = *s;
                    for x in t.iter_mut().skip(2) {
                        *x *= scale;
                    }
                    t
                })
                .collect();
            let index = v.len();
            v.push(Case {
                unit: unit.to_string(),
                name: format!("inner_shell_of_{name}_x{scale:e}"),
                source: crate::CaseSource::Fixture {
                    path: "built-in: IsInnerShellOf cases".to_string(),
                    index,
                    upstream_ref: None,
                },
                inputs: json!({ "inner_shell_of": scaled }),
            });
        }
    }
    v
}

