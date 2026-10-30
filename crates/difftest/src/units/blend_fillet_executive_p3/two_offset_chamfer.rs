//! tangency-labs/solidcore#880: an unequal (two-offset) chamfer of the rim a flat cut
//! leaves on a placed cylinder, through the public fillet session on the Rust side and
//! `SmConstantRadiusFS` with `pdOptSecondRadius` in the shim
//! (`blend_fillet_executive_p3_two_offset_chamfer.inc`). Both sides build the body, pick
//! the rim and, for `setbacks`, derive the two offsets from their own body.

use super::end_to_end::{section, status, topology};
use crate::{HarnessError, Output};
use serde::{Deserialize, Serialize};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::{Vec3, PI};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p5::{stitch_and_orient, transform, StitchAndOrientOptions};
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::edgeuse::p3::{evaluate_binormal_uv, BinormalOptions};
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;

/// The setbacks of the fork's `sides_offsets`: `along_up` on the face whose face use
/// normal leans most along `up`, `other` on the other face.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Setbacks {
    up: [f64; 3],
    along_up: f64,
    other: f64,
}

/// The case: a cylinder at the origin ([`Body`]) placed by `Transform` with `origin`,
/// `x_axis` and `y_axis`, minus the box `box_origin`, `box_size`; the `rim_nth` edge
/// (0 the first) whose middle is within 1e-6 of `rim_z` chamfered by `offsets` (first on
/// the blend edge use's face, second on its radial's) or by the offsets `setbacks`
/// derive; with `rim_all`, every such edge in one session, each by `offsets` (the whole
/// rim: `rim_nth` 0 and `offsets` required).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TwoOffsetChamfer {
    radius: f64,
    height: f64,
    origin: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    box_origin: [f64; 3],
    box_size: [f64; 3],
    rim_z: f64,
    #[serde(default)]
    offsets: Option<[f64; 2]>,
    #[serde(default)]
    setbacks: Option<Setbacks>,
    #[serde(default)]
    body: Body,
    #[serde(default)]
    rim_nth: u8,
    #[serde(default)]
    rim_all: bool,
}

/// How the cylinder at the origin is made.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Body {
    /// `SmApiCreateCylinder`.
    #[default]
    Cylinder,
    /// SolidCraft's extruded cylinder: the capped `SmApiCreateLinearSweep` along +Z of the
    /// two `SmApiCreateArc` half circles `seam_deg` to `seam_deg + 180` and
    /// `seam_deg - 180` to `seam_deg` (degrees).
    HalfArcSweep { seam_deg: f64 },
}

impl TwoOffsetChamfer {
    fn wire(&self) -> Result<Vec<f64>, HarnessError> {
        let (derive, [a, b], up) = match (self.offsets, self.setbacks) {
            (Some(offsets), None) => (0.0, offsets, [0.0; 3]),
            (None, Some(s)) => (1.0, [s.along_up, s.other], s.up),
            _ => {
                return Err(HarnessError::new(
                    "two_offset_chamfer needs exactly one of offsets and setbacks",
                ))
            }
        };
        let mut w = vec![self.radius, self.height];
        for v in [
            self.origin,
            self.x_axis,
            self.y_axis,
            self.box_origin,
            self.box_size,
        ] {
            w.extend(v);
        }
        w.extend([self.rim_z, derive, a, b]);
        w.extend(up);
        let (kind, seam) = match self.body {
            Body::Cylinder => (0.0, 0.0),
            Body::HalfArcSweep { seam_deg } => (1.0, seam_deg),
        };
        let all = f64::from(u8::from(self.rim_all));
        w.extend([kind, seam, f64::from(self.rim_nth), all]);
        Ok(w)
    }
}

// The shim's setup outcomes.
const NO_BODY: f64 = -1.0;
const NO_RIM: f64 = -2.0;
const NO_BLEND_EDGEUSE: f64 = -3.0;
const NO_OFFSETS: f64 = -4.0;

/// The record (see [`ffi::two_offset_chamfer`]) as an [`Output`]: `setup` (always 0: a
/// failed setup is a harness error), `rim_edge`, `chamfer_offsets` and the end-to-end
/// record's keys.
fn output(values: &[f64]) -> Result<Output, HarnessError> {
    let (&setup, rest) = values
        .split_first()
        .ok_or_else(|| HarnessError::new("missing setup code"))?;
    if ![0.0, NO_BODY, NO_RIM, NO_BLEND_EDGEUSE, NO_OFFSETS].contains(&setup) {
        return Err(HarnessError::new("invalid setup code"));
    }
    // Every case must build its body and reach the chamfer: a failed setup on either
    // side is a fixture error, not a compared outcome (two equal failures would pass).
    if setup != 0.0 {
        let what = if setup == NO_BODY {
            "no body (cylinder, transform or cut failed)"
        } else if setup == NO_RIM {
            "no edge at the rim height"
        } else if setup == NO_BLEND_EDGEUSE {
            "the rim has no blend edge use"
        } else {
            "no usable offsets"
        };
        return Err(HarnessError::new(format!(
            "two_offset_chamfer setup failed ({setup}): {what}"
        )));
    }
    let (head, rest) = rest
        .split_first_chunk::<3>()
        .ok_or_else(|| HarnessError::new("missing rim and offsets"))?;
    let [index, r0, r1] = *head;
    if !index.is_finite() || index < 0.0 || index.fract() != 0.0 {
        return Err(HarnessError::new("invalid rim index"));
    }
    let mut out = super::end_to_end::output(rest)?;
    out.ints.insert("setup".into(), vec![0]);
    out.ints.insert("rim_edge".into(), vec![index as i64]);
    out.values.insert("chamfer_offsets".into(), vec![r0, r1]);
    Ok(out)
}

pub(super) fn cpp(input: &TwoOffsetChamfer) -> Result<Output, HarnessError> {
    let wire = input.wire()?;
    let (values, step) = super::fresh_cpp(|| ffi::two_offset_chamfer(&wire))
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(super::with_point_step(output(&values)?, step))
}

pub(super) fn rust(input: &TwoOffsetChamfer) -> Result<Output, HarnessError> {
    let wire = input.wire()?;
    ffi::validate_two_offset_chamfer(&wire).map_err(|e| HarnessError::new(e.to_string()))?;
    let mut k = Kernel::default();
    match run(&mut k, input) {
        Ok(values) => Ok(super::with_point_step(
            output(&values)?,
            super::rust_point_step(&k),
        )),
        Err(error) => match Output::unported(&error) {
            Some(deferred) => Ok(deferred),
            None => Err(HarnessError::new(error.to_string())),
        },
    }
}

/// `outcome` for a kernel failure the shim reports as a setup code; an unported one
/// propagates (the case defers).
fn or_outcome<T>(result: SolidResult<T>, outcome: f64) -> SolidResult<Result<T, f64>> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(error @ SolidError::Unported { .. }) => Err(error),
        Err(_) => Ok(Err(outcome)),
    }
}

/// The shim's `cylinder_at_origin`.
fn cylinder_at_origin(k: &mut Kernel, input: &TwoOffsetChamfer) -> SolidResult<Brep> {
    use solidcore::curve::curve::Curve;
    use solidcore::primitive::api_curves::create_arc;
    use solidcore::primitive::api_primitives::create_cylinder;
    use solidcore::sweep::api_brep::create_linear_sweep;
    match input.body {
        Body::Cylinder => create_cylinder(k, Vec3::ZERO, input.radius, input.height),
        Body::HalfArcSweep { seam_deg } => {
            // (degrees within [-360, 360]: the second half circle as seam - 180 to seam)
            let r = input.radius;
            let a = Curve::from(create_arc(Vec3::ZERO, r, seam_deg, seam_deg + 180.0)?);
            let b = Curve::from(create_arc(Vec3::ZERO, r, seam_deg - 180.0, seam_deg)?);
            let up = Vec3::new(0.0, 0.0, 1.0);
            create_linear_sweep(k, &[Some(&a), Some(&b)], up, input.height, true)
        }
    }
}

/// The shim's `build`.
fn build(k: &mut Kernel, input: &TwoOffsetChamfer) -> SolidResult<Brep> {
    use solidcore::primitive::api_primitives::create_box;
    let mut cylinder = cylinder_at_origin(k, input)?;
    let placement = Axis2Placement::new(
        Vec3::from(input.origin),
        Vec3::from(input.x_axis),
        Vec3::from(input.y_axis),
    );
    transform(k, &mut cylinder, &placement, None, false)?;
    let [x, y, z] = input.box_size;
    let cut = create_box(k, Vec3::from(input.box_origin), x, y, z)?;
    solidcore::boolean::api_brep::boolean_difference(k, cylinder, cut)
}

/// The shim's `rim`: the `nth` edge (0 the first) with a curve whose middle lies within
/// 1e-6 of `z`.
fn rim(k: &Kernel, b: &Brep, z: f64, nth: u8) -> SolidResult<Option<(usize, EdgeId)>> {
    let mut nth = nth;
    for (index, e) in p1::edges(b, None)?.into_iter().enumerate() {
        let Some(curve) = edge::curve(b, e)? else {
            continue;
        };
        let p = match curve.evaluate_point(k, edge::interval(b, e)?.evaluate(0.5)) {
            Ok(p) => p,
            Err(error @ SolidError::Unported { .. }) => return Err(error),
            Err(_) => continue,
        };
        if (p.z - z).abs() < 1e-6 {
            if nth == 0 {
                return Ok(Some((index, e)));
            }
            nth -= 1;
        }
    }
    Ok(None)
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// The shim's `sides_offsets`; `None` where it returns false.
fn sides_offsets(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    u0: EdgeuseId,
    setbacks: Setbacks,
) -> SolidResult<Option<[f64; 2]>> {
    let Some(u1) = eu::radial(b, u0)? else {
        return Ok(None);
    };
    let t = edge::interval(b, e)?.evaluate(0.5);
    let options = BinormalOptions::default();
    let Ok(f0) = or_outcome(evaluate_binormal_uv(k, b, u0, t, false, &options), 0.0)? else {
        return Ok(None);
    };
    let Ok(f1) = or_outcome(evaluate_binormal_uv(k, b, u1, t, false, &options), 0.0)? else {
        return Ok(None);
    };
    let c = dot(f0.eval.binormal, f1.eval.binormal).clamp(-1.0, 1.0);
    let sn = (1.0 - c * c).max(0.0).sqrt();
    if sn.is_nan() || sn <= 1e-6 {
        return Ok(None);
    }
    let up = Vec3::from(setbacks.up);
    let (mut s0, mut s1) = (setbacks.along_up, setbacks.other);
    if dot(f1.eval.face_normal, up).abs() > dot(f0.eval.face_normal, up).abs() {
        std::mem::swap(&mut s0, &mut s1);
    }
    let r0 = (s1 - s0 * c) / sn;
    let r1 = (s0 - s1 * c) / sn;
    Ok((r0 > 0.0 && r1 > 0.0).then_some([r0, r1]))
}

/// The shim's `chamfer`, through the public session; the session is dropped before
/// the caller's `StitchAndOrient`.
fn chamfer(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[(EdgeId, EdgeuseId)],
    [r0, r1]: [f64; 2],
) -> SolidResult<()> {
    // `static constexpr double dTolFactor = 4.0` and `SM_DEG2RAD`, as SetFilletParameters
    let tolerances = edges
        .iter()
        .map(|&(e, _)| edge::tolerance(b, e))
        .collect::<SolidResult<Vec<f64>>>()?;
    let mut session = FilletSession::new(k, b)?;
    let generator = session.add_surface_generator(FilletSurfaceGenerator::Linear)?;
    // one solver per edge, in order, each loaded and then given the generator
    for (&(_, u0), tolerance) in edges.iter().zip(tolerances) {
        let solver = session.new_constant_radius_solver_with_second(
            k,
            u0,
            r0,
            r1,
            4.0 * tolerance,
            20.0 * (PI / 180.0),
            10.0 * (PI / 180.0),
        )?;
        session.load_solver(k, solver)?;
        session.set_solver_generator(solver, generator)?;
    }
    session.create_corners(k, None)?;
    for corner in session.fillet_corners()? {
        session.set_corner_bevel(corner, true)?;
    }
    session.do_filleting(k, None)
}

fn run(k: &mut Kernel, input: &TwoOffsetChamfer) -> SolidResult<Vec<f64>> {
    let mut body = match or_outcome(build(k, input), NO_BODY)? {
        Ok(body) => body,
        Err(outcome) => return Ok(vec![outcome]),
    };
    let Some((index, e)) = rim(k, &body, input.rim_z, input.rim_nth)? else {
        return Ok(vec![NO_RIM]);
    };
    let Some(u0) = edge::blend_edgeuse(k, &mut body, e)? else {
        return Ok(vec![NO_BLEND_EDGEUSE]);
    };
    let offsets = match (input.offsets, input.setbacks) {
        (Some(offsets), _) => Some(offsets),
        (None, Some(setbacks)) => sides_offsets(k, &mut body, e, u0, setbacks)?,
        (None, None) => None,
    };
    let Some(offsets) = offsets else {
        return Ok(vec![NO_OFFSETS]);
    };
    // with `rim_all`, every other edge `rim` would find after the first, in edge list order
    let mut edges = vec![(e, u0)];
    if input.rim_all {
        for nth in 1..=u8::MAX {
            let Some((_, other)) = rim(k, &body, input.rim_z, nth)? else {
                break;
            };
            let Some(other_eu) = edge::blend_edgeuse(k, &mut body, other)? else {
                return Ok(vec![NO_BLEND_EDGEUSE]);
            };
            edges.push((other, other_eu));
        }
    }
    let mut values = vec![0.0, index as f64];
    values.extend(offsets);
    let interval = edge::interval(&body, e)?;
    let mut before = vec![index as f64, interval.min(), interval.max(), 1.0];
    if let Some(curve) = edge::curve(&body, e)? {
        before.push(curve.type_code() as f64);
        for fraction in [0.17, 0.37, 0.83] {
            super::end_to_end::sample(
                &mut before,
                curve.evaluate_point(k, interval.evaluate(fraction)),
            )?;
        }
    }
    let result = chamfer(k, &mut body, &edges, offsets);
    if result.is_ok() {
        // StitchAndOrient's status is ignored, as SmApiSetBevelCorners ignores it.
        match stitch_and_orient(k, &mut body, &StitchAndOrientOptions::default()) {
            Ok(())
            | Err(
                SolidError::Upstream { .. }
                | SolidError::InvalidInput { .. }
                | SolidError::PortInvariant { .. },
            ) => {}
            Err(error) => return Err(error),
        }
    }
    let result_status = match result {
        Ok(()) => 0.0,
        Err(error) => status(error)?,
    };
    section(&mut values, &before);
    values.extend([
        result_status,
        p1::vertices(&body, None)?.len() as f64,
        p1::edges(&body, None)?.len() as f64,
        p1::faces(&body, None)?.len() as f64,
    ]);
    let mut topology_record = Vec::new();
    topology(&body, &mut topology_record)?;
    section(&mut values, &topology_record);
    let mut geometry = Vec::new();
    crate::util::primitive::append(k, &body, &mut geometry)?;
    section(&mut values, &geometry);
    // As the end-to-end record: no attribute is left on the body once the session is gone.
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attributes = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    match solidcore::mass::integrator::properties::brep_mass_properties(
        k,
        &mut body,
        1e-4,
        1.0,
        Vec3::ZERO,
        &attributes,
    ) {
        Ok(mass) => {
            values.extend([
                0.0,
                mass.properties.area,
                mass.properties.volume,
                mass.properties.mass,
            ]);
            for v in [mass.centroid, mass.inertia, mass.products] {
                values.extend([v.x, v.y, v.z]);
            }
        }
        Err(error) => values.push(status(error)?),
    }
    Ok(values)
}
