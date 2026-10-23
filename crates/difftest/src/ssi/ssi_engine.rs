//! The `ssi` engine: the `ssi` crate (`crates/ssi`, a surface/surface intersection of its
//! own, not a port of upstream) on one corpus case, as the crate's corpus runner
//! (`crates/ssi/src/bin/ssi_corpus_runner`) runs it. Each surface becomes the crate's
//! exact patch (`ssi::shapes` for the analytic kinds, the net as given for a NURBS), the
//! pair is intersected with `ssi::intersect_bodies`, and after the timer stops the curves
//! are filled in to [`DENSIFY_TURN`] by `ssi::densify`. The curves are written as marched
//! (the crate has no curve type to sample at 4001 parameters), each point with both
//! patches' parameters and each patch evaluated there; the points are the touch points
//! and then the curves' vertices (crossings and meetings). No C++ is involved, so this
//! engine runs without the `cpp` feature.

use std::time::Instant;

use ::ssi::{densify, intersect_bodies, shapes, Intersection, Options, Patch};

use super::case::{SsiCase, SurfaceSpec};
use super::raw::{RawCurve, RawOutput, SM_SUCCESS};
use super::EngineFailure;

/// `SM_ERR`: the status of a refusal or failure, whose code and detail are in
/// `status_name`.
const SM_ERR: i64 = 1001;

/// The most a filled-in curve turns between points (radians), as the crate's runner
/// fills them, so a chord stays within the corpus' coverage tolerance.
pub const DENSIFY_TURN: f64 = 0.01;

/// A corpus surface as the crate's exact patch.
fn patch(spec: &SurfaceSpec) -> Result<Patch, String> {
    Ok(match spec {
        SurfaceSpec::Plane {
            origin,
            xaxis,
            yaxis,
            u,
            v,
        } => shapes::plane(*origin, *xaxis, *yaxis, *u, *v),
        SurfaceSpec::Cylinder {
            origin,
            xaxis,
            yaxis,
            radius,
            angle,
            height,
        } => shapes::cylinder(*origin, *xaxis, *yaxis, *radius, *angle, *height),
        SurfaceSpec::Cone {
            apex,
            xaxis,
            yaxis,
            half_angle_deg,
            height,
        } => shapes::cone(*apex, *xaxis, *yaxis, *half_angle_deg, *height),
        SurfaceSpec::Sphere {
            center,
            xaxis,
            yaxis,
            radius,
            lon,
            lat,
        } => shapes::sphere(*center, *xaxis, *yaxis, *radius, *lon, *lat),
        SurfaceSpec::Torus {
            center,
            xaxis,
            yaxis,
            major_radius,
            minor_radius,
            major,
            minor,
        } => shapes::torus(
            *center,
            *xaxis,
            *yaxis,
            *major_radius,
            *minor_radius,
            *major,
            *minor,
        ),
        SurfaceSpec::Nurbs {
            degree: [p, q],
            knots_u,
            knots_v,
            nu,
            nv,
            points,
            weights,
        } => {
            let count = nu.checked_mul(*nv);
            if *nu == 0 || *nv == 0 || count != Some(points.len()) || count != Some(weights.len()) {
                return Err("nurbs: bad sizes".into());
            }
            if Some(knots_u.len()) != nu.checked_add(p + 1)
                || Some(knots_v.len()) != nv.checked_add(q + 1)
            {
                return Err("nurbs: bad knots".into());
            }
            Patch {
                u_degree: *p,
                v_degree: *q,
                u_count: *nu,
                v_count: *nv,
                poles: points.clone(),
                weights: Some(weights.clone()),
                u_knots: knots_u.clone(),
                v_knots: knots_v.clone(),
                analytic: None,
            }
        }
    })
}

/// The patch at (u, v), NaN where its counts and knots do not let it be evaluated (as
/// the crate's runner writes it).
fn at(patch: &Patch, u: f64, v: f64) -> [f64; 3] {
    patch.evaluate(u, v).unwrap_or([f64::NAN; 3])
}

/// Writes the curves and points of an intersection into `out`.
fn write_found(out: &mut RawOutput, found: &Intersection, a: &Patch, b: &Patch) {
    for curve in &found.curves {
        let mut points = curve.points.clone();
        if curve.closed {
            points.extend(curve.points.first().copied());
        }
        out.curves.push(RawCurve {
            ty: "n/a".into(),
            deviation: f64::NAN,
            xyz: points.iter().map(|q| q.p).collect(),
            uv1: points.iter().map(|q| [q.a.u, q.a.v]).collect(),
            s1_at_uv: points.iter().map(|q| at(a, q.a.u, q.a.v)).collect(),
            uv2: points.iter().map(|q| [q.b.u, q.b.v]).collect(),
            s2_at_uv: points.iter().map(|q| at(b, q.b.u, q.b.v)).collect(),
            ..RawCurve::default()
        });
    }
    out.points = found
        .points
        .iter()
        .map(|q| q.p)
        .chain(found.vertices.iter().copied())
        .collect();
}

/// The `ssi` crate's intersection of the case's two surfaces. A refusal (a contact the
/// crate has not built, `BOOLEAN_TANGENT`) or failure is status `SM_ERR` with its code
/// and detail as `status_name`; trim boxes are not supported (no corpus case has one).
pub fn run_ssi(case: &SsiCase, tol: Option<f64>) -> Result<RawOutput, EngineFailure> {
    let [(spec_a, dom_a), (spec_b, dom_b)] = &case.surfaces;
    if dom_a.is_some() || dom_b.is_some() {
        return Err(EngineFailure::Build(
            "ssi: surface trim boxes are not supported".into(),
        ));
    }
    let a = patch(spec_a).map_err(EngineFailure::Build)?;
    let b = patch(spec_b).map_err(EngineFailure::Build)?;
    let domain = |p: &Patch| {
        p.domain().ok_or_else(|| {
            EngineFailure::Build("ssi: a patch's knots are too few for its poles".into())
        })
    };
    let (domain1, domain2) = (domain(&a)?, domain(&b)?);
    let options = tol.map_or_else(Options::default, |t| shapes::resolving(t, &[&a, &b]));
    let (body_a, body_b) = (std::slice::from_ref(&a), std::slice::from_ref(&b));
    let t0 = Instant::now();
    let mut result = intersect_bodies(body_a, body_b, &options);
    let time_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if let Ok(found) = &mut result {
        densify(found, body_a, body_b, DENSIFY_TURN)
            .map_err(|e| EngineFailure::Other(e.to_string()))?;
    }
    let mut out = RawOutput {
        case: case.id.clone(),
        engine: "ssi".into(),
        status: SM_SUCCESS,
        domain1,
        domain2,
        approx_tol: tol,
        time_ms,
        ..RawOutput::default()
    };
    match result {
        Ok(found) => write_found(&mut out, &found, &a, &b),
        Err(e) => {
            out.status = SM_ERR;
            out.status_name = Some(format!("{} ({e})", e.code()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::ssi::geom::P3;
    use crate::ssi::score::score_case;

    /// The unit sphere cut by the plane z = 0.5: one circle of radius sqrt(3)/2.
    fn sphere_plane() -> SsiCase {
        let r = 0.75f64.sqrt();
        let pts: Vec<P3> = (0..4001)
            .map(|i| {
                let t = std::f64::consts::TAU * f64::from(i) / 4000.0;
                [r * t.cos(), r * t.sin(), 0.5]
            })
            .collect();
        let v = json!({
            "id": "t", "class": "1 transversal", "cov_tol": 1e-3,
            "surfaces": [
                {"type": "plane", "origin": [0,0,0.5], "xaxis": [1,0,0], "yaxis": [0,1,0], "u": [-2,2], "v": [-2,2]},
                {"type": "sphere", "center": [0,0,0], "xaxis": [1,0,0], "yaxis": [0,1,0], "radius": 1, "lon": [0,360], "lat": [-90,90]}
            ],
            "truth": [
                {"kind": "plane", "point": [0,0,0.5], "normal": [0,0,1]},
                {"kind": "sphere", "center": [0,0,0], "radius": 1}
            ],
            "expected": {"branches": [{"closed": true, "kind": "crossing", "points": pts}],
                         "isolated_points": [], "singular_points": [], "coincident_regions": []}
        });
        SsiCase::parse(&v).unwrap()
    }

    #[test]
    fn sphere_cut_by_a_plane_passes() {
        let case = sphere_plane();
        let out = run_ssi(&case, Some(1e-6)).unwrap();
        assert_eq!(out.status, SM_SUCCESS, "{:?}", out.status_name);
        assert_eq!(out.curves.len(), 1);
        let s = score_case(&case, Ok(&out), Some(1e-5), 0.0, &BTreeMap::new());
        assert_eq!(s.verdict, "PASS", "{:?}", s.flags);
    }

    #[test]
    fn bad_nurbs_is_a_build_error() {
        let spec = SurfaceSpec::Nurbs {
            degree: [1, 1],
            knots_u: vec![0.0, 0.0, 1.0, 1.0],
            knots_v: vec![0.0, 0.0, 1.0],
            nu: 2,
            nv: 2,
            points: vec![[0.0; 3]; 4],
            weights: vec![1.0; 4],
        };
        assert_eq!(patch(&spec).err().as_deref(), Some("nurbs: bad knots"));
    }
}
