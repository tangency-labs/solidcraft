//! `SmApiSurfaceSurfaceFillet` on surfaces that may already belong to a face of a
//! sheet body (the API's `GetFace()` branch): the result B-rep and every sheet body
//! after the call, which upstream edits in place (the rails are merged into the base
//! faces' B-reps).
//!
//! C++: `solidcore_ffi::units::blend_fillet_executive_p3::surface_surface_bodies`.

use super::end_to_end::{section, status, take_section, topology};
use crate::{HarnessError, Output};
use serde::{Deserialize, Serialize};
use solidcore::blend::fillet_executive::api::{self, FilletSurfaceArg};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1;
use solidcore::topology::{Brep, FaceId};
use solidcore::Result as SolidResult;
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;

/// Who owns each surface: `owners[0]` 0 free, 1 the face of its own sheet body;
/// `owners[1]` 0 free, 1 its own sheet body, 2 a second face of surface 1's body,
/// 3 surface 1 itself (`second` is ignored).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SurfaceSurfaceBodies {
    /// First surface corners `u0v0, u1v0, u0v1, u1v1`.
    first: [[f64; 3]; 4],
    /// Second surface corners.
    second: [[f64; 3]; 4],
    owners: [u32; 2],
    radius1: f64,
    radius2: f64,
    tolerance: f64,
    xsect: u32,
    mirror: bool,
    complement: bool,
}
impl SurfaceSurfaceBodies {
    fn wire(&self) -> Vec<f64> {
        let mut out: Vec<f64> = self.first.iter().chain(&self.second).flatten().copied().collect();
        out.extend([
            f64::from(self.owners[0]),
            f64::from(self.owners[1]),
            self.radius1,
            self.radius2,
            self.tolerance,
            f64::from(self.xsect),
            f64::from(u8::from(self.mirror)),
            f64::from(u8::from(self.complement)),
        ]);
        out
    }
}

fn body_output(
    out: &mut Output,
    prefix: &str,
    values: &mut &[f64],
    with_counts: Option<&mut Vec<f64>>,
) -> Result<(), HarnessError> {
    let counts = values
        .get(..3)
        .ok_or_else(|| HarnessError::new("missing body counts"))?
        .to_vec();
    *values = values.get(3..).unwrap_or_default();
    let topology = take_section(values)?;
    if topology
        .iter()
        .chain(&counts)
        .any(|v| !v.is_finite() || v.fract() != 0.0)
    {
        return Err(HarnessError::new("invalid topology record"));
    }
    out.ints.insert(
        format!("{prefix}_topology"),
        topology.iter().map(|v| *v as i64).collect(),
    );
    out.values
        .insert(format!("{prefix}_geometry"), take_section(values)?.to_vec());
    match with_counts {
        Some(head) => head.extend(counts),
        None => {
            out.ints.insert(
                format!("{prefix}_vertex_edge_face_counts"),
                counts.iter().map(|v| *v as i64).collect(),
            );
        }
    }
    Ok(())
}

fn output(values: &[f64]) -> Result<Output, HarnessError> {
    let (head, mut rest) = values
        .split_at_checked(2)
        .ok_or_else(|| HarnessError::new("missing status"))?;
    let mut out = Output::new();
    let mut ints = head.to_vec();
    let has_result = *head.get(1).unwrap_or(&-1.0);
    if has_result == 1.0 {
        body_output(&mut out, "result", &mut rest, Some(&mut ints))?;
    } else if has_result != 0.0 {
        return Err(HarnessError::new("invalid result flag"));
    }
    let (&bodies, tail) = rest
        .split_first()
        .ok_or_else(|| HarnessError::new("missing body count"))?;
    rest = tail;
    if ![0.0, 1.0, 2.0].contains(&bodies) {
        return Err(HarnessError::new("invalid body count"));
    }
    for i in 0..bodies as usize {
        body_output(&mut out, &format!("body{i}"), &mut rest, None)?;
    }
    if !rest.is_empty() {
        return Err(HarnessError::new("trailing surface-surface record"));
    }
    if ints.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
        return Err(HarnessError::new("invalid status/counts"));
    }
    out.ints.insert(
        "api_status_result_vertex_edge_face_counts".into(),
        ints.iter().map(|v| *v as i64).collect(),
    );
    Ok(out)
}

/// The C++ record of [`SurfaceSurfaceBodies`].
pub(super) fn cpp(input: &SurfaceSurfaceBodies) -> Result<Output, HarnessError> {
    let (values, step) = super::fresh_cpp(|| ffi::surface_surface_bodies(&input.wire()))
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(super::with_point_step(output(&values)?, step))
}

fn body_record(k: &Kernel, body: &Brep, values: &mut Vec<f64>) -> SolidResult<()> {
    values.extend([
        p1::vertices(body, None)?.len() as f64,
        p1::edges(body, None)?.len() as f64,
        p1::faces(body, None)?.len() as f64,
    ]);
    let mut topology_record = Vec::new();
    topology(body, &mut topology_record)?;
    section(values, &topology_record);
    let mut geometry = Vec::new();
    crate::util::primitive::append(k, body, &mut geometry)?;
    section(values, &geometry);
    Ok(())
}

/// A sheet body holding one face per surface, as the shim's `CreateFaceFromSurface`
/// over the natural domain builds it.
fn sheet(k: &mut Kernel, surfaces: &[&Surface]) -> SolidResult<(Brep, Vec<FaceId>)> {
    let mut body = solidcore::topology::brep::p3::new_brep(
        k,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    )?;
    let mut faces = Vec::new();
    for surface in surfaces {
        faces.push(api::create_face_from_surface(k, &mut body, surface)?);
    }
    Ok((body, faces))
}

/// The Rust side of [`SurfaceSurfaceBodies`]: the surfaces and sheet bodies are built
/// as the shim builds them, then
/// `solidcore::blend::fillet_executive::api::surface_surface_fillet` runs on them.
pub(super) fn rust(input: &SurfaceSurfaceBodies) -> Result<Output, HarnessError> {
    ffi::validate_surface_surface_bodies(&input.wire())
        .map_err(|e| HarnessError::new(e.to_string()))?;
    let mut k = Kernel::default();
    let mut run = || -> SolidResult<Vec<f64>> {
        let surface = |c: &[[f64; 3]; 4]| {
            solidcore::nurbs::b_spline_surface::BSplineSurface::create_bilinear_surface(
                Vec3::from(c[0]),
                Vec3::from(c[1]),
                Vec3::from(c[2]),
                Vec3::from(c[3]),
            )
            .map(Surface::from)
        };
        let first = surface(&input.first)?;
        let second = surface(&input.second)?;
        let [owner1, owner2] = input.owners;
        let mut body1 = match (owner1, owner2) {
            (1, 2) => Some(sheet(&mut k, &[&first, &second])?),
            (1, _) => Some(sheet(&mut k, &[&first])?),
            _ => None,
        };
        let mut body2 = match owner2 {
            1 => Some(sheet(&mut k, &[&second])?),
            _ => None,
        };
        let face_at = |faces: &[FaceId], i: usize| {
            faces.get(i).copied().ok_or_else(|| {
                solidcore::SolidError::invalid_input("harness::surface_bodies", "sheet without face")
            })
        };
        let result = {
            let (arg1, arg2) = match (body1.as_mut(), body2.as_mut()) {
                (Some((b1, f1)), None) if owner2 == 2 => {
                    let faces = [face_at(f1, 0)?, face_at(f1, 1)?];
                    (
                        FilletSurfaceArg::Face { body: b1, face: faces[0] },
                        FilletSurfaceArg::FirstBodyFace(faces[1]),
                    )
                }
                (Some((b1, f1)), None) if owner2 == 3 => (
                    FilletSurfaceArg::Face { body: b1, face: face_at(f1, 0)? },
                    FilletSurfaceArg::FirstBodyFace(face_at(f1, 0)?),
                ),
                (Some((b1, f1)), Some((b2, f2))) => (
                    FilletSurfaceArg::Face { body: b1, face: face_at(f1, 0)? },
                    FilletSurfaceArg::Face { body: b2, face: face_at(f2, 0)? },
                ),
                (Some((b1, f1)), None) => (
                    FilletSurfaceArg::Face { body: b1, face: face_at(f1, 0)? },
                    FilletSurfaceArg::Free(&second),
                ),
                (None, Some((b2, f2))) => (
                    FilletSurfaceArg::Free(&first),
                    FilletSurfaceArg::Face { body: b2, face: face_at(f2, 0)? },
                ),
                (None, None) if owner2 == 3 => {
                    (FilletSurfaceArg::Free(&first), FilletSurfaceArg::Free(&first))
                }
                (None, None) => (FilletSurfaceArg::Free(&first), FilletSurfaceArg::Free(&second)),
            };
            api::surface_surface_fillet(
                &mut k,
                arg1,
                arg2,
                input.radius1,
                input.radius2,
                input.tolerance,
                input.xsect,
                input.mirror,
                input.complement,
            )
        };
        let mut values = Vec::new();
        match result {
            Ok(Some(body)) => {
                values.extend([0.0, 1.0]);
                body_record(&k, &body, &mut values)?;
            }
            Ok(None) => values.extend([0.0, 0.0]),
            Err(error) => values.extend([status(error)?, 0.0]),
        }
        let bodies: Vec<&Brep> = body1.iter().chain(&body2).map(|(b, _)| b).collect();
        values.push(bodies.len() as f64);
        for body in bodies {
            body_record(&k, body, &mut values)?;
        }
        Ok(values)
    };
    let result = run();
    let step = super::rust_point_step(&k);
    match result {
        Ok(values) => Ok(super::with_point_step(output(&values)?, step)),
        Err(error) => {
            Output::unported(&error).ok_or_else(|| HarnessError::new(error.to_string()))
        }
    }
}
