//! Runner for unit `topology.edgeuse_p3`. C++: `solidcore_ffi::units::topology_edgeuse_p3`.
//! Rust: `solidcore::topology::edgeuse::p3`.
//!
//! The B-reps are those of unit `topology.edgeuse_p1` (upstream `.smb` files, wire edges,
//! faces from B-spline surfaces and real `SmPlane`s, the wire network of `prog_test`
//! `my_test_topology`): the C++ side builds a real `SmBrep` per call, the Rust side
//! builds a non-file source itself ([`crate::own_builder`]) and for an `.smb` file
//! imports the graph the shim exports ([`super::topology_edgeuse_p1::import`]), and the
//! ported functions run on both. Edge use arguments are selectors taken modulo the edge
//! use count.
//!
//! `CreateUVTrimCurve` and every path that has to make a UV trim curve run
//! `SmSurface::DropCurve` (the plane override or the general drop, unit
//! `intersect.surface_drop_lib_p3`) and are compared with the paths with existing UV
//! trim curves, the 3D evaluation, the null dereferences and the wire edge uses; the
//! cases that reach a unit not ported yet (the face's `CreateUVTrimCurves`, B-rep
//! makers, surface kinds, the curve classifier) are deferred. The state compared after an op is that of
//! `topology.edgeuse_p2`'s `put_eu_state` for the edge use and its mate, the edge's
//! interval and tolerance, then the full records of the edge use's and its mate's UV trim
//! curves ([`crate::util::trim_curve`]: type, degree, control points, weights, knots,
//! interval and samples, a failed sample as its status); `CreateUVTrimCurve`'s result
//! curve is compared with the same record. A unit-local auxiliary export carries the complete initial
//! maximum edge/face gap cache, so `GetOrCreateUVTrimCurve` compares cached distances too.
//! `EvaluateBinormal` builds the face's trimmed surface cache; on the tiny adversarial
//! surfaces that tree grows to about 2 million nodes on both sides (2 014 255 in the C++
//! for the `1e-4`-scale faces), below the port's `MAX_SUBDIVISION_NODES` bound; a case
//! past that bound would be deferred to `surface.surface_cache_p3`.
//!
//! Known mismatches (`known_mismatches/topology.edgeuse_p3.toml`): `EvaluateBinormal` with `bUse3DOnly` on
//! `Endcap.smb` (a file without UV trim curves). `SmBrep::ReadFromFile`'s healer leaves
//! an `SmTrimSrfCache` without a tree on each face's surface; `EvaluateBinormal`
//! reuses it, and its tree is built later with the surface's owner cleared
//! (`GlobalPointSolve`), so no `CreateUVTrimCurves` runs. The Rust B-rep keeps no
//! per-surface cache, so the port builds the face's trimmed cache and attaches the UV
//! trim curves. The graph export does not carry the cache's existence.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::kernel::Kernel;
use solidcore::surface::surface::p2::DropCurveFail;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p3 as eu3;
use solidcore::topology::{Brep, EdgeuseId, ObjectHandle};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;
use solidcore_ffi::units::topology_edgeuse_p3::{self as ffi, Op};

use super::topology_edgeuse_p1::{
    brep_block_of, domain, gen_brep, gen_surface, harness, import_with_types, path_of,
    seed_cache_tree_states, sel, BrepIn, SrfIn, FILES,
};
use super::topology_edgeuse_p2::{flag, inexact_geometry, put_eu_state, Setup};
use crate::util::trim_curve::put_trim_curve;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Create {
        eu: i64,
        want_dev: bool,
        has_fail: bool,
    },
    Binormal {
        eu: i64,
        frac: f64,
        use_3d: bool,
        want_uv_vector: bool,
    },
    Normalized {
        eu: i64,
        t: f64,
        parameter_space: bool,
        want_deriv: bool,
        ok_make: bool,
    },
    GetOrCreate {
        eu: i64,
        ok_make: bool,
        want_dist: bool,
        want_dev: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

impl OpIn {
    fn eu(&self) -> i64 {
        match self {
            Self::Create { eu, .. }
            | Self::Binormal { eu, .. }
            | Self::Normalized { eu, .. }
            | Self::GetOrCreate { eu, .. } => *eu,
        }
    }
}

/// The case's B-rep: the C++ export, the imported Rust B-rep and the selected edge
/// use; `Err(output)` when the B-rep cannot be built or has no edge use.
type CaseSetup = (Setup, Option<ffi::PreparedBrep>);

/// The case's setup. `prepare` (the C++ side) also keeps the shim's prepared B-rep;
/// without it (the Rust side) a shim-built source is built by the Rust kernel
/// ([`crate::own_builder`], its cached gaps computed by its own `init_gaps` walk) and
/// only an `.smb` file's graph and gap exports are imported.
fn setup(i: &Inputs, prepare: bool) -> Result<Result<CaseSetup, Output>, HarnessError> {
    use crate::own_builder::{build, counts, failed_output, Built};
    let path = path_of(Some(&i.brep))?;
    let block = brep_block_of(Some(&i.brep), i.validate_only, i.init_gaps)?;
    if !prepare {
        let mut k = Kernel::default();
        match build(&mut k, &i.brep, i.validate_only, i.init_gaps)? {
            Some(Built::Brep(b)) => {
                let brep = *b;
                let s = sel(i.op.eu(), counts(&brep)[6]);
                if s < 0 {
                    return Ok(Err(Output::err("no_entity_of_that_kind")));
                }
                let eu = EdgeuseId::from_index(u32::try_from(s).map_err(|_| harness("index"))?);
                return Ok(Ok((
                    Setup {
                        path,
                        block,
                        brep,
                        eu,
                        types: super::topology_edgeuse_p1::ImportTypes::default(),
                    },
                    None,
                )));
            }
            Some(Built::Failed(e)) => return Ok(Err(failed_output(&e))),
            None => {}
        }
    }
    let prepared = prepare
        .then(|| ffi::PreparedBrep::new(&path, &block))
        .transpose()
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let ex = crate::export_memo::export(
        "topology_edgeuse_p1",
        &[&path],
        &block,
        || match &prepared {
            Some(prepared) => Ok(prepared.export().to_vec()),
            None => ffi1::eval(ffi1::Op::Export, &path, &block),
        },
    )
    .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(c) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let s = sel(i.op.eu(), n_eu);
    if s < 0 {
        return Ok(Err(Output::err("no_entity_of_that_kind")));
    }
    let (mut brep, types) = import_with_types(&ex)?;
    let gaps = crate::export_memo::export("topology_edgeuse_p3_gaps", &[&path], &block, || {
        ffi::eval(Op::ExportGaps, &path, &block)
    })
    .map_err(|e| harness(format!("c++ gap export: {e}")))?;
    import_gaps(&mut brep, &gaps, n_eu)?;
    let eu = EdgeuseId::from_index(u32::try_from(s).map_err(|_| harness("index"))?);
    Ok(Ok((
        Setup {
            path,
            block,
            brep,
            eu,
            types,
        },
        prepared,
    )))
}

/// Restore input cache fields, without invoking or copying the tested operation.
fn import_gaps(brep: &mut Brep, data: &[f64], count: usize) -> Result<(), HarnessError> {
    let expected = count
        .checked_mul(5)
        .and_then(|n| n.checked_add(2))
        .ok_or_else(|| harness("gap snapshot overflow"))?;
    if data.first() != Some(&0.0) || data.get(1) != Some(&(count as f64)) || data.len() != expected
    {
        return Err(harness("invalid gap snapshot"));
    }
    for (index, values) in data
        .get(2..)
        .ok_or_else(|| harness("gap records"))?
        .as_chunks::<5>()
        .0
        .iter()
        .enumerate()
    {
        let &[owner, gap_3d, edge_t, u, v] = values;
        if owner.fract() != 0.0 || owner < -1.0 || owner >= count as f64 {
            return Err(harness("invalid gap owner"));
        }
        let id = EdgeuseId::from_index(u32::try_from(index).map_err(|_| harness("gap index"))?);
        brep.edgeuse_mut(id)
            .map_err(|e| harness(e.to_string()))?
            .max_edge_face_gap = solidcore::heal::gap::EdgeFaceGap {
            edgeuse: (owner >= 0.0).then(|| EdgeuseId::from_index(owner as u32)),
            gap_3d,
            edge_t,
            face_uv: solidcore::math::Point2::new(u, v),
        };
    }
    Ok(())
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (s, prepared) = match setup(i, true)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = vec![f64::from(s.eu.index())];
    let op = match &i.op {
        OpIn::Create {
            want_dev, has_fail, ..
        } => {
            input.extend([flag(*want_dev), flag(*has_fail)]);
            Op::Create
        }
        OpIn::Binormal {
            frac,
            use_3d,
            want_uv_vector,
            ..
        } => {
            input.extend([*frac, flag(*use_3d), flag(*want_uv_vector)]);
            Op::Binormal
        }
        OpIn::Normalized {
            t,
            parameter_space,
            want_deriv,
            ok_make,
            ..
        } => {
            input.extend([
                *t,
                flag(*parameter_space),
                flag(*want_deriv),
                flag(*ok_make),
            ]);
            Op::Normalized
        }
        OpIn::GetOrCreate {
            ok_make,
            want_dist,
            want_dev,
            ..
        } => {
            input.extend([flag(*ok_make), flag(*want_dist), flag(*want_dev)]);
            Op::GetOrCreate
        }
    };
    let prepared = prepared
        .as_ref()
        .ok_or_else(|| harness("missing prepared fixture"))?;
    let out = ffi::eval_prepared(op, prepared, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(output_of(body))
}

/// The output of an op body (after the build status), as both sides build it.
fn output_of(body: &[f64]) -> Output {
    let first = body.first().copied().unwrap_or(f64::NAN);
    if first == NULL_DEREF {
        return Output::err("null_deref");
    }
    Output::new()
        .int("status", first as i64)
        .values("state", body.get(1..).unwrap_or(&[]).to_vec())
}

// ---- Rust side --------------------------------------------------------------------------

/// The status a failed Rust op reports (`None`: not an upstream status).
fn status_of(e: &SolidError) -> Option<f64> {
    match e {
        SolidError::Upstream { code, .. } => Some(f64::from(*code)),
        _ => None,
    }
}

/// The tail every op reports: the edge use's and its mate's state and the edge state.
fn put_states(k: &Kernel, o: &mut Vec<f64>, b: &Brep, eu: EdgeuseId) -> Result<(), SolidError> {
    let mate = eu1::mate(b, eu).ok().flatten();
    put_eu_state(k, o, b, Some(eu))?;
    put_eu_state(k, o, b, mate)?;
    match eu1::edge(b, eu)? {
        Some(e) => {
            let r = b.edge(e)?;
            o.extend([r.interval.min(), r.interval.max(), r.tolerance]);
        }
        None => o.extend([0.0; 3]),
    }
    put_trim_curve(k, o, b.edgeuse(eu)?.uv_trim_curve.as_ref())?;
    match mate {
        Some(m) => put_trim_curve(k, o, b.edgeuse(m)?.uv_trim_curve.as_ref()),
        None => put_trim_curve(k, o, None),
    }
}

/// The Rust outcome of an op body; `Err` carries a [`SolidError`] that is not an
/// upstream status (unported, null dereference).
#[allow(clippy::too_many_lines)]
fn run_rust_op(op: &OpIn, b: &mut Brep, eu: EdgeuseId) -> Result<Vec<f64>, SolidError> {
    let k = Kernel::default();
    // a failing upstream status is reported with the state, like the shim
    let finish = |b: &Brep, mut v: Vec<f64>| -> Result<Vec<f64>, SolidError> {
        put_states(&k, &mut v, b, eu)?;
        Ok(v)
    };
    match op {
        OpIn::Create {
            want_dev, has_fail, ..
        } => {
            if eu1::faceuse(b, eu)?.is_none() {
                // NER(pFU) before anything else
                return match eu3::create_uv_trim_curve(&k, b, eu, *want_dev, None) {
                    Ok(_) => Err(SolidError::invalid_input(
                        "difftest",
                        "wire edge use made a curve",
                    )),
                    Err(e) => status_of(&e).map(|s| vec![s]).ok_or(e),
                };
            }
            let mut rec = has_fail.then(DropCurveFail::<ObjectHandle>::default);
            let r = eu3::create_uv_trim_curve(&k, b, eu, *want_dev, rec.as_mut());
            let mut v = Vec::new();
            match r {
                Ok(c) => {
                    v.extend([
                        0.0,
                        c.max_dist_to_surf,
                        c.max_deviation.filter(|_| *want_dev).unwrap_or(-1.0),
                    ]);
                    put_trim_curve(&k, &mut v, Some(&c.curve))?;
                }
                Err(e) => {
                    v.push(status_of(&e).ok_or(e)?);
                    v.extend([-1.0, -1.0]);
                    put_trim_curve(&k, &mut v, None)?;
                }
            }
            v.push(
                rec.as_ref()
                    .map_or(-1.0, |f| f64::from(f.create_path_type as i32)),
            );
            v.push(flag(rec.as_ref().is_some_and(|f| f.brep.is_some())));
            finish(b, v)
        }
        OpIn::GetOrCreate {
            ok_make,
            want_dist,
            want_dev,
            ..
        } => {
            let r = eu3::get_or_create_uv_trim_curve(&k, b, eu, *want_dist, *want_dev, *ok_make)?;
            let status = match &r.status {
                Ok(()) => 0.0,
                Err(e) => status_of(e).unwrap_or(1001.0),
            };
            let has_curve = eu1::uv_trim_curve(b, eu)?.0.is_some();
            finish(
                b,
                vec![
                    status,
                    if *want_dist { r.dist_to_surf } else { -2.0 },
                    if *want_dev { r.deviation } else { -2.0 },
                    flag(has_curve),
                ],
            )
        }
        OpIn::Binormal {
            frac,
            use_3d,
            want_uv_vector,
            ..
        } => {
            let t = match eu1::edge(b, eu)? {
                Some(e) => b.edge(e)?.interval.evaluate(*frac),
                None => *frac,
            };
            let options = eu3::BinormalOptions {
                want_uv_vector: *want_uv_vector,
            };
            let mut v = Vec::new();
            match eu3::evaluate_binormal_uv(&k, b, eu, t, *use_3d, &options) {
                Ok(r) => {
                    v.push(0.0);
                    for p in [
                        r.eval.point,
                        r.eval.binormal,
                        r.eval.tangent,
                        r.eval.face_normal,
                    ] {
                        v.extend([p.x, p.y, p.z]);
                    }
                    v.extend([r.eval.uv.x, r.eval.uv.y]);
                    let d = r.uv_vector.map_or([0.0; 2], |d| [d.x, d.y]);
                    v.extend(d);
                }
                Err(e) => v.push(status_of(&e).ok_or(e)?),
            }
            finish(b, v)
        }
        OpIn::Normalized {
            t,
            parameter_space,
            want_deriv,
            ok_make,
            ..
        } => {
            let mut v = Vec::new();
            match eu3::normalized_evaluate(&k, b, eu, *t, *parameter_space, *want_deriv, *ok_make) {
                Ok(r) => {
                    v.push(0.0);
                    v.extend([r.point.x, r.point.y, r.point.z]);
                    let d = r.first_deriv.unwrap_or(Vec3::new(0.0, 0.0, 0.0));
                    v.extend([d.x, d.y, d.z]);
                }
                Err(e) => v.push(status_of(&e).ok_or(e)?),
            }
            finish(b, v)
        }
    }
}

/// The operation of the port's surface cache subdivision bound.
const SUBDIVIDE_OP: &str = "surface::surface_cache::p3::SurfaceCache::subdivide_to_tolerances";

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let (mut s, _) = match setup(i, false)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    if let Some(o) = inexact_geometry(&s) {
        return Ok(o);
    }
    // the face surfaces' caches as the C++ build left them
    seed_cache_tree_states(&s.brep, &s.path, &s.block)?;
    match run_rust_op(&i.op, &mut s.brep, s.eu) {
        Ok(body) => Ok(output_of(&body)),
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Ok(o);
            }
            match e {
                // the port's bound on the surface cache tree (`MAX_SUBDIVISION_NODES`),
                // reached through the face's trimmed surface cache that `EvaluateBinormal`
                // builds; upstream subdivides on (no fixture reaches it since the bound
                // became 2^22; plan/findings.md, surface.surface_cache_p3)
                SolidError::InvalidInput { operation, reason }
                    if operation == SUBDIVIDE_OP && reason.contains("MAX_SUBDIVISION_NODES") =>
                {
                    Ok(Output::deferred(
                        "surface.surface_cache_p3",
                        "SmSurfaceCache::SubdivideToTolerances past MAX_SUBDIVISION_NODES (port bound)",
                    ))
                }
                SolidError::InvalidInput { .. } => Ok(Output::err("null_deref")),
                e => Err(harness(format!("rust: {e}"))),
            }
        }
    }
}

// ---- generation -------------------------------------------------------------------------

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(20) {
        0..=3 => OpIn::Create {
            eu: gen_sel(rng),
            want_dev: rng.chance(0.5),
            has_fail: rng.chance(0.5),
        },
        4..=9 => OpIn::Binormal {
            eu: gen_sel(rng),
            frac: *rng
                .choose(&[0.0, 0.1, 0.34, 0.5, 0.9, 1.0, 1.1, -0.1])
                .unwrap_or(&0.5),
            use_3d: rng.chance(0.5),
            want_uv_vector: rng.chance(0.4),
        },
        10..=16 => OpIn::Normalized {
            eu: gen_sel(rng),
            t: rng.uniform(-0.1, 1.1),
            parameter_space: rng.chance(0.6),
            want_deriv: rng.chance(0.5),
            ok_make: rng.chance(0.6),
        },
        _ => OpIn::GetOrCreate {
            eu: gen_sel(rng),
            ok_make: rng.chance(0.7),
            want_dist: rng.chance(0.5),
            want_dev: rng.chance(0.5),
        },
    }
}

/// A full-circle NURBS cylinder (radius `r`, height `h`) closed and periodic in U
/// (`along_u`) or in V.
fn cylinder_surface(r: f64, h: f64, centre: [f64; 3], along_u: bool) -> Option<SrfIn> {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring = [
        (r, 0.0, 1.0),
        (r, r, w),
        (0.0, r, 1.0),
        (-r, r, w),
        (-r, 0.0, 1.0),
        (-r, -r, w),
        (0.0, -r, 1.0),
        (r, -r, w),
        (r, 0.0, 1.0),
    ];
    let pt = |(x, y, w): (f64, f64, f64), z: f64| {
        [
            (centre[0] + x) * w,
            (centre[1] + y) * w,
            (centre[2] + z) * w,
            w,
        ]
    };
    let ck = [
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    let lk = [0.0, 0.0, 1.0, 1.0];
    let v = if along_u {
        let net: Vec<_> = ring.iter().map(|&c| vec![pt(c, 0.0), pt(c, h)]).collect();
        serde_json::json!({"net": net, "p": 2, "q": 1, "u": ck, "v": lk})
    } else {
        let net = vec![
            ring.iter().map(|&c| pt(c, 0.0)).collect::<Vec<_>>(),
            ring.iter().map(|&c| pt(c, h)).collect::<Vec<_>>(),
        ];
        serde_json::json!({"net": net, "p": 1, "q": 2, "u": lk, "v": ck})
    };
    serde_json::from_value(v).ok().map(SrfIn::Bspline)
}

/// A face whose edges are `SmCrvOnSurf`s: on an open B-spline surface, or on a periodic
/// cylinder closed in U or in V (seam edges).
fn gen_crv_on_surf_face(rng: &mut Rng) -> BrepIn {
    let scale = *rng.choose(&[0.01, 1.0, 1.0, 3.0, 100.0]).unwrap_or(&1.0);
    let centre = rng.point3(-2.0, 2.0).map(|c| c * scale);
    let surface = match rng.below(3) {
        0 => cylinder_surface(scale, scale * rng.uniform(0.2, 3.0), centre, true),
        1 => cylinder_surface(scale, scale * rng.uniform(0.2, 3.0), centre, false),
        _ => None,
    };
    let surface = surface.unwrap_or_else(|| loop {
        let s = gen_surface(rng);
        if matches!(s, SrfIn::Bspline(_)) {
            break s;
        }
    });
    let [u0, v0, u1, v1] = domain(&surface);
    let dom = if rng.chance(0.3) {
        let (du, dv) = (u1 - u0, v1 - v0);
        [
            u0 + du * rng.uniform(0.0, 0.3),
            v0 + dv * rng.uniform(0.0, 0.3),
            u1 - du * rng.uniform(0.0, 0.3),
            v1 - dv * rng.uniform(0.0, 0.3),
        ]
    } else {
        [u0, v0, u1, v1]
    };
    BrepIn::CrvOnSurfFace { surface, dom }
}

/// B-reps of generated cases: mostly upstream files (their faces carry real UV trim
/// curves), plus the constructed kinds of `topology.edgeuse_p1`.
fn gen_brep_p3(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.12) {
        return gen_crv_on_surf_face(rng);
    }
    if rng.chance(0.6) {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
            rebuild: rng.chance(0.25),
        }
    } else {
        gen_brep(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.edgeuse_p3"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = gen_op(rng);
        let i = Inputs {
            brep: gen_brep_p3(rng),
            validate_only: rng.chance(0.15),
            init_gaps: rng.chance(0.5),
            op,
        };
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_distance_is_observed_instead_of_masked() {
        let i: Inputs = serde_json::from_value(serde_json::json!({
            "brep": {"kind": "file", "file": "Mouse.smb", "rebuild": true},
            "init_gaps": true, "op": "get_or_create", "eu": 0,
            "ok_make": false, "want_dist": true, "want_dev": false
        }))
        .unwrap();
        let cpp = run_cpp(&i).unwrap();
        let rust = run_rust(&i).unwrap();
        assert!(crate::compare::compare(&cpp, &rust, &[]).pass());
        let (mut s, _) = setup(&i, false).unwrap().unwrap();
        assert!(eu1::uv_trim_curve(&s.brep, s.eu).unwrap().0.is_some());
        let cached = s.brep.edgeuse_mut(s.eu).unwrap();
        assert!(cached.max_edge_face_gap.is_init());
        cached.max_edge_face_gap.gap_3d += 0.125;
        let changed = output_of(&run_rust_op(&i.op, &mut s.brep, s.eu).unwrap());
        assert!(!crate::compare::compare(&cpp, &changed, &[]).pass());
        assert_ne!(changed.values["state"][0], -2.0);
    }

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(7);
        for _ in 0..50 {
            let op = gen_op(&mut rng);
            let i = Inputs {
                brep: BrepIn::Network,
                validate_only: false,
                init_gaps: true,
                op,
            };
            let v = serde_json::to_value(&i).unwrap();
            let back: Inputs = serde_json::from_value(v).unwrap();
            assert_eq!(back.op.eu(), i.op.eu());
        }
    }
}
