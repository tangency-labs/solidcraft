//! `SmApiFilletPreview` cases: the untrimmed fillet surfaces of a constant circular
//! fillet, on a box or one of the end-to-end runner's sources, without filleting.
//!
//! A case is `{"fillet_preview": {"dimensions" | "source", "edges", "radius"}}`: a box at
//! the origin (`SmApiCreateBox`) or a [`Source`], owning-list edge indices (an empty list
//! is the API's `SM_ERR_INVALID_INPUT`) and the radius. Both sides record the API status,
//! the body's vertex, edge and face counts after the call, then on success each preview
//! surface: `has`, its type, natural UV domain and a 5 x 5 `EvaluatePoint` grid over it
//! (`status [x y z]`). The C++ runs in a fresh child process and both sides report the
//! `sdStaticStep` latch ([`super::POINT_STEP`]), as the other real-solid cases.

use super::end_to_end::{build_source, Source};
use crate::{HarnessError, Output, Rng};
use serde::{Deserialize, Serialize};
use solidcore::blend::fillet_executive::api::fillet_preview;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1;
use solidcore::topology::{Brep, EdgeId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;

/// Samples per direction of the evaluation grid (as the shim's `PREVIEW_GRID`).
const GRID: usize = 5;

/// Generated preview cases per run (their own seed stream, `preview0000`, ...).
pub(super) const MIN_GENERATED: usize = 64;

/// One `SmApiFilletPreview` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FilletPreview {
    /// The box at the origin (`SmApiCreateBox`), unless `source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dimensions: Option<[f64; 3]>,
    /// A non-box body instead of the box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<Source>,
    /// Selected edges by index into `GetEdges`.
    edges: Vec<usize>,
    /// `dRadius`.
    radius: f64,
}

impl FilletPreview {
    /// The shim's wire: source kind, its parameters, radius, edge count and indices.
    fn wire(&self) -> Result<Vec<f64>, HarnessError> {
        let mut out = match (&self.source, self.dimensions) {
            (None, Some(size)) => {
                let mut v = vec![0.0];
                v.extend(size);
                v.extend([0.0; ffi::SOURCE_PARAMS - 3]);
                v
            }
            (Some(source), None) => {
                let (kind, params) = source.wire();
                let mut v = vec![kind];
                v.extend(params);
                v
            }
            _ => {
                return Err(HarnessError::new(
                    "preview: exactly one of dimensions and source",
                ))
            }
        };
        out.push(self.radius);
        out.push(self.edges.len() as f64);
        out.extend(self.edges.iter().map(|&e| e as f64));
        ffi::validate_fillet_preview(&out, self.source.as_ref().and_then(Source::path).is_some())
            .map_err(|e| HarnessError::new(e.to_string()))?;
        Ok(out)
    }
}

/// The C++ record, in a fresh child process.
pub(super) fn cpp(input: &FilletPreview) -> Result<Output, HarnessError> {
    let wire = input.wire()?;
    let path = input.source.as_ref().and_then(Source::path);
    let (values, step) = super::fresh_cpp(|| ffi::fillet_preview(&wire, path))
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(super::with_point_step(
        Output::new().values("preview", values),
        step,
    ))
}

/// The upstream status of a Rust error (`-1` for one upstream cannot return).
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// `vertices edges faces` of `b` (the shim's `counts`).
fn counts(out: &mut Vec<f64>, b: &Brep) -> solidcore::Result<()> {
    out.push(p1::vertices(b, None)?.len() as f64);
    out.push(p1::edges(b, None)?.len() as f64);
    out.push(p1::faces(b, None)?.len() as f64);
    Ok(())
}

/// One preview surface (the shim's `preview_surface`).
///
/// # Errors
///
/// Those of the natural domain (`GetNaturalUVDomain`, which makes a missing NURB).
fn surface(out: &mut Vec<f64>, k: &Kernel, s: Option<&Surface>) -> solidcore::Result<()> {
    let Some(s) = s else {
        out.push(0.0);
        return Ok(());
    };
    out.push(1.0);
    out.push(s.type_code() as f64);
    let d = s.natural_uv_domain_with_nurb(k)?;
    let (u0, v0, u1, v1) = (d.min().x, d.min().y, d.max().x, d.max().y);
    out.extend([u0, v0, u1, v1]);
    for i in 0..GRID {
        for j in 0..GRID {
            let u = u0 + (u1 - u0) * (i as f64 / (GRID - 1) as f64);
            let v = v0 + (v1 - v0) * (j as f64 / (GRID - 1) as f64);
            match s.evaluate_point(k, Point2::new(u, v)) {
                Ok(p) => out.extend([0.0, p.x, p.y, p.z]),
                Err(e) => out.push(status(&e)),
            }
        }
    }
    Ok(())
}

/// The Rust record, on a fresh kernel.
pub(super) fn rust(input: &FilletPreview) -> Result<Output, HarnessError> {
    input.wire()?;
    let mut k = Kernel::default();
    let body = match (&input.source, input.dimensions) {
        (None, Some([x, y, z])) => {
            solidcore::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, x, y, z)
        }
        (Some(source), None) => build_source(&mut k, source)?,
        _ => {
            return Err(HarnessError::new(
                "preview: exactly one of dimensions and source",
            ))
        }
    };
    // a body the shim cannot build is its SC_ERR_ARG, a harness error
    let mut body = match body {
        Ok(body) => body,
        Err(e) => match Output::unported(&e) {
            Some(deferred) => return Ok(deferred),
            None => return Err(HarnessError::new(format!("preview source: {e}"))),
        },
    };
    let all = p1::edges(&body, None).map_err(|e| HarnessError::new(e.to_string()))?;
    let selected = input
        .edges
        .iter()
        .map(|&i| {
            all.get(i)
                .copied()
                .ok_or_else(|| HarnessError::new("preview: edge index out of range"))
        })
        .collect::<Result<Vec<EdgeId>, _>>()?;
    let result = fillet_preview(&mut k, &mut body, &selected, input.radius);
    let out = match result {
        Ok(surfaces) => {
            let mut out = vec![0.0];
            counts(&mut out, &body).map_err(|e| HarnessError::new(e.to_string()))?;
            out.push(surfaces.len() as f64);
            for s in &surfaces {
                if let Err(e) = surface(&mut out, &k, s.as_ref()) {
                    return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
                }
            }
            Output::new().values("preview", out)
        }
        Err(e) => {
            if let Some(deferred) = Output::unported(&e) {
                return Ok(deferred);
            }
            let mut out = vec![status(&e)];
            counts(&mut out, &body).map_err(|e| HarnessError::new(e.to_string()))?;
            out.push(0.0);
            Output::new().values("preview", out)
        }
    };
    Ok(super::with_point_step(out, super::rust_point_step(&k)))
}

/// A generated case: a box (sometimes long and thin) with one to four of its edges, or
/// none (`SM_ERR_INVALID_INPUT`), at a radius from small to about the box's half size,
/// sometimes degenerate: zero, negative or up to 1.5 times the box's smallest side
/// (`SmApiFilletPreview` does not check the radius).
pub(super) fn generate(rng: &mut Rng) -> FilletPreview {
    let scale = match rng.below(8) {
        0 => rng.uniform(0.05, 0.5),
        1 => rng.uniform(50.0, 500.0),
        _ => rng.uniform(2.0, 20.0),
    };
    let dimensions = [
        scale * rng.uniform(0.5, 2.0),
        scale * rng.uniform(0.5, 2.0),
        scale * rng.uniform(0.5, 2.0),
    ];
    let smallest = dimensions.iter().copied().fold(f64::INFINITY, f64::min);
    let n = if rng.chance(0.05) {
        0
    } else {
        1 + rng.below(4) as usize
    };
    let mut edges: Vec<usize> = Vec::new();
    while edges.len() < n {
        let e = rng.below(12) as usize;
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    let radius = match rng.below(10) {
        0 => 0.0,
        1 => -smallest * rng.uniform(0.02, 0.45),
        2 | 3 => smallest * rng.uniform(0.45, 1.5),
        _ => smallest * rng.uniform(0.02, 0.45),
    };
    FilletPreview {
        dimensions: Some(dimensions),
        source: None,
        edges,
        radius,
    }
}
