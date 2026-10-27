//! Runner for unit `topology.proto_topology_p2`: the second part of `SmProtoTopology.cpp`
//! (imprinting vertex and edge definitions, resolving proto vertices and proto edges,
//! creating, finding, combining and removing proto topology, and the updates after an
//! edge or face split).
//!
//! C++: the proto scripts of `solidcore_ffi::units::topology_proto_topology_p1` (shim
//! `harness/cpp/shim/topology_proto_topology_p1.cpp`, ops 91 to 108,
//! [`proto_op_p2`](solidcore_ffi::units::topology_proto_topology_p1::proto_op_p2)). Rust:
//! `solidcore::topology::proto_topology::p2`.
//!
//! The proto-script engine is unit `topology.proto_topology_p1`'s
//! ([`super::topology_proto_topology_p1`]): a script builds proto topology with its ops
//! and then runs the ops of this unit, which [`run_op`] interprets against the port. Both
//! sides produce the same flat record: per op its status and outputs, then the whole state
//! (every definition, proto object and pool curve, including the curves the calls copy).
//! An op the shim refuses because it is undefined upstream ends the record with `-2`; the
//! port reports those as [`SolidError::InvalidInput`], which the interpreter turns into the
//! same `-2`, so a refusal on one side only is a mismatch. The two conservative guards of
//! the shim (a duplicated entry in the list a resolve deletes from, zero-length intervals
//! where a reparameterisation can follow, plan/upstream-issues.md HANG-12) are mirrored
//! here explicitly, as is its `Imprint` guard for a merge into a bare region, face or
//! edge stand-in, whose `GetBrep` is `NULL` ([`imprint_reaches_missing_brep`]).
//!
//! The pool's bare edges and vertices carry the interval and zone tolerance the shim gives
//! its stand-ins (`ObjIn::EdgeIvl`, `ObjIn::VertexTol`); the pool answers the port's
//! [`ProtoBrep`](solidcore::topology::proto_topology::ProtoBrep) queries with them.
//!
//! `UpdateFaces` runs on real faces: the fixtures and [`real_face_update_cases`] build the
//! old and new face of a split in an owning B-rep on both sides; the shim refuses it on
//! the random scripts' bare-face stand-ins, and so does the port (the same input guard).
//!
//! Deferred: everything past `SmBrep::MakeEdgeInFace` (unit `topology.brep_p5`), the
//! B-rep walks of `sm_AreConnected` and point classifications on bare topology (units
//! `topology.edge`, `topology.vertex`, `topology.face_p2`), and
//! `SmProtoVertex::ContainsPoint` (unit `query.face_p2`).

use serde_json::{json, Value};

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::intersect::curve_class::{CurveClassification, CurveInterval};
use solidcore::topology::proto_topology::p1::{ProtoEdgeId, VertexDefId};
use solidcore::SolidError;
use solidcore_ffi::units::topology_proto_topology_p1::{
    self as ffi, pool as pk, proto_op as p, proto_op_p2 as q,
};

use super::topology_proto_topology_p1::{
    b, build_curve, cat, cpp, cpp_input, dir, gen_curve, gen_scale, gen_tol, line, opv, pcv, rp,
    rust, shim_accepts, status, CurveIn, Fail, ObjIn, ProtoRun, Rd, Script, H,
};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `topology.proto_topology_p2` runner.
pub struct Runner;

// ---- interpreting the ops of this unit --------------------------------------------------------

/// The value of a port call: `Some` on success; `None` with the status set on an upstream
/// error, or with `skip` set for undefined behaviour upstream (the shim refuses those);
/// `Unported` defers the case.
fn res<T>(r: solidcore::Result<T>, status_out: &mut f64, skip: &mut bool) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(o)));
            }
            if matches!(e, SolidError::InvalidInput { .. }) {
                *skip = true;
            } else {
                *status_out = status(&e);
            }
            Ok(None)
        }
    }
}

/// The shim's guard of `SmVertexDefinition::Imprint`, mirrored from the inputs: its
/// `MergeIntoObject` calls `GetBrep()->...` on a region, face or edge, and the pool's bare
/// stand-ins have no B-rep (upstream dereferences `NULL`); a vertex merges, after which
/// the manager is reached through the proto vertex (`NULL` without one). The shim checks
/// both classifications before the call, so the whole op is refused, as here.
fn imprint_reaches_missing_brep(r: &ProtoRun, d: VertexDefId) -> bool {
    use solidcore::intersect::point_class::ClassObjectKind as K;
    let Some(v) = r.mgr.vertex_def(d) else {
        return false;
    };
    for which in [1, 2] {
        let Some(H(_, kind)) = v.brep_point_classif(which).object() else {
            break;
        };
        match kind {
            K::Region | K::Face | K::Edge => return true,
            K::Vertex if v.proto_vertex().is_none() => return true,
            K::Vertex => {}
            _ => break,
        }
    }
    false
}

/// The objects calls made get the next ids of their kinds (the shim numbers them after the
/// op in the order the port makes them).
pub(super) fn sync_counts(r: &mut ProtoRun) {
    use solidcore::topology::proto_topology::p1::{EdgeDefId, ProtoFaceId, ProtoVertexId};
    while r.mgr.vertex_def(VertexDefId::from_index(r.n_vd)).is_some() {
        r.n_vd += 1;
    }
    while r.mgr.edge_def(EdgeDefId::from_index(r.n_ed)).is_some() {
        r.n_ed += 1;
    }
    while r.mgr.proto_vertex(ProtoVertexId::from_index(r.n_pv)).is_some() {
        r.n_pv += 1;
    }
    while r.mgr.proto_edge(ProtoEdgeId::from_index(r.n_pe)).is_some() {
        r.n_pe += 1;
    }
    while r.mgr.proto_face(ProtoFaceId::from_index(r.n_pf)).is_some() {
        r.n_pf += 1;
    }
}

/// A curve interval `c uv tol a b pc` (the shim's `read_ci`).
fn read_ci(r: &ProtoRun, rd: &mut Rd<'_>) -> Option<(CurveClassification<H>, CurveInterval<H>)> {
    let c = rd.idx();
    let uv = rd.idx();
    let tol = rd.num();
    let a = rd.num();
    let bb = rd.num();
    let mid = r.read_pc(rd);
    let curve_ok = |j: i64| j == -1 || r.pool.is_curve(j);
    let mid = mid?;
    if !curve_ok(c) || !curve_ok(uv) || rd.bad {
        return None;
    }
    let k = &r.k;
    let mut cc = CurveClassification::new(k);
    cc.set_curve((c != -1).then(|| r.pool.h(c)), false);
    cc.set_uv_curve1((uv != -1).then(|| r.pool.h(uv)));
    cc.set_src_zone_tol_3d(zone_tol3d(k, tol));
    let mut ci = CurveInterval::with_interval(k, &Extent1d::from_min_max(a, bb), Some(&cc));
    ci.mid = mid;
    Some((cc, ci))
}

fn zero_len(e: &Extent1d) -> bool {
    e.min() == e.max()
}

fn has_dup<T: PartialEq>(v: &[T]) -> bool {
    v.iter().enumerate().any(|(i, x)| v.iter().take(i).any(|y| y == x))
}

/// Whether the mid tangent of the proto edge evaluates (the shim's `pe_tangent_ok`).
fn pe_tangent_ok(k: &solidcore::kernel::Kernel, r: &ProtoRun, pe: ProtoEdgeId) -> Result<bool, Fail> {
    match r.mgr.proto_edge_evaluate_mid(k, &r.pool, pe, false, true) {
        Ok(_) => Ok(true),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(false),
        },
    }
}

/// Whether the mid tangent of the definition evaluates (the shim's `ed_tangent_ok`).
fn ed_tangent_ok(k: &solidcore::kernel::Kernel, r: &ProtoRun, ed: solidcore::topology::proto_topology::p1::EdgeDefId) -> Result<bool, Fail> {
    let Some(d) = r.mgr.edge_def(ed) else {
        return Ok(false);
    };
    match d.evaluate_mid(k, &r.pool, false, true) {
        Ok(_) => Ok(true),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(false),
        },
    }
}

fn unit_segment(curve: &solidcore::curve::curve::Curve, dim: u64) -> bool {
    use solidcore::curve::curve::CurveKind;
    let CurveKind::BSpline(bs) = curve.kind() else { return false; };
    let Some(nurb) = bs.gw_nurb() else { return false; };
    curve.dim() == dim && matches!(bs.degree(), Ok(1))
        && matches!(bs.number_control_points(), Ok(2)) && matches!(bs.is_rational(), Ok(false))
        && nurb.knots() == [0.0, 0.0, 1.0, 1.0]
        && nurb.cpts().iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
}

fn face_update_edges_ok(r: &ProtoRun, which: i32, old_face: H) -> Result<bool, Fail> {
    use solidcore::intersect::point_class::ClassGeometry;
    if r.n_ed == 0 { return Ok(true); }
    let (Some(ed_id), Some(pe_id), Some(pv0), Some(pv1)) = (r.ed(0), r.pe(0), r.pv(0), r.pv(1)) else {
        return Ok(false);
    };
    if r.n_ed != 1 || r.n_pe != 1 || r.n_pv != 2 || r.first_ed_final_initialized != 3
        || r.mgr.proto_edges() != [pe_id] || r.mgr.proto_vertices() != [pv0, pv1] {
        return Ok(false);
    }
    let (Some(ed), Some(pe)) = (r.mgr.edge_def(ed_id), r.mgr.proto_edge(pe_id)) else { return Ok(false); };
    if pe.start_proto_vertex() != Some(pv0) || pe.end_proto_vertex() != Some(pv1)
        || pe.edge_definitions() != [ed_id] || ed.proto_edge() != Some(pe_id)
        || pe.xsect_interval().min() != 0.0 || pe.xsect_interval().max() != 1.0 {
        return Ok(false);
    }
    for vertex in [pv0, pv1] {
        if r.mgr.proto_vertex(vertex).is_none_or(|pv| pv.proto_edges() != [pe_id]) {
            return Ok(false);
        }
    }
    let (Some(other), Some(c3), Some(uv)) = (ed.src_topo(3 - which), ed.xsect_curve_3d(), ed.xsect_uv_curve(which)) else {
        return Ok(false);
    };
    if ed.src_topo(which) != Some(old_face) || !r.pool.is(other.0 as i64, pk::REGION)
        || c3 == uv || ed.xsect_uv_curve(3 - which).is_some()
        || ed.xsect_interval().min() != 0.0 || ed.xsect_interval().max() != 1.0 {
        return Ok(false);
    }
    let (Some(c3), Some(uv)) = (r.pool.curve(c3), r.pool.curve(uv)) else { return Ok(false); };
    if !unit_segment(c3, 3) || !unit_segment(uv, 2) { return Ok(false); }
    // The native preflight excludes its DropPoint fallback. A Rust-only ordinary
    // evaluation failure must fail the harness rather than exclude this fixture.
    let point = super::topology_proto_topology_p1::st(uv.evaluate_point(&r.k, 0.5))?;
    match point {
        Ok(point) if point.x.is_finite() && point.y.is_finite() => Ok(true),
        _ => Err(super::topology_proto_topology_p1::harness(
            "admitted UpdateFaces UV segment did not evaluate to a finite midpoint",
        ).into()),
    }
}

/// Runs one op of this unit (its code already read) against the port, as the shim's
/// `run_p2_op` runs it against upstream.
#[allow(clippy::too_many_lines)]
pub(super) fn run_op(
    r: &mut ProtoRun,
    rd: &mut Rd<'_>,
    op: i64,
    out: &mut Vec<f64>,
    st: &mut f64,
    skip: &mut bool,
) -> Result<(), Fail> {
    if !r.mgr_alive {
        *skip = true;
    }
    let mut ok = true;
    match op {
        q::VD_IMPRINT => {
            let d = r.vd(rd.idx());
            let Some(d) = d.filter(|_| !*skip) else {
                *skip = true;
                return Ok(());
            };
            if imprint_reaches_missing_brep(r, d) {
                *skip = true;
                return Ok(());
            }
            let _ = res(r.mgr.imprint_vertex_definition(&mut r.k, &mut r.pool, d), st, skip)?;
            if let Some(v) = r.mgr.vertex_def(d) {
                out.push(r.pool.id(v.final_vertex(1)));
                out.push(r.pool.id(v.final_vertex(2)));
            }
        }
        q::ED_IMPRINT => {
            let d = r.ed(rd.idx());
            let Some(d) = d.filter(|_| !*skip) else {
                *skip = true;
                return Ok(());
            };
            let _ = res(r.mgr.imprint_edge_definition(&r.k, &mut r.pool, d), st, skip)?;
            if let Some(e) = r.mgr.edge_def(d) {
                out.push(r.pool.id(e.final_edge(1)));
                out.push(r.pool.id(e.final_edge(2)));
            }
        }
        q::PV_RESOLVE => {
            let v = r.pv(rd.idx());
            let Some(v) = v.filter(|_| !*skip) else {
                *skip = true;
                return Ok(());
            };
            let vds = r
                .mgr
                .proto_vertex(v)
                .map(|x| x.vertex_definitions().to_vec())
                .unwrap_or_default();
            if has_dup(&vds) {
                *skip = true;
                return Ok(());
            }
            let _ = res(r.mgr.resolve_proto_vertex(&r.k, &r.pool, v), st, skip)?;
        }
        q::PV_ADD_VD_PCS => {
            let v = r.pv(rd.idx());
            let pc1 = r.read_pc(rd);
            let pc2 = r.read_pc(rd);
            let (Some(v), Some(mut pc1), Some(mut pc2)) = (v, pc1, pc2) else {
                *skip = true;
                return Ok(());
            };
            if *skip {
                return Ok(());
            }
            let got = res(
                r.mgr
                    .add_vertex_definition_from_pcs(&r.k, &r.pool, v, &mut pc1, &mut pc2),
                st,
                skip,
            )?;
            sync_counts(r);
            out.push(r.vd_id(got.map(|g| g.0)));
            out.push(b(got.is_some_and(|g| g.1)));
            r.put_pc(out, &pc1);
            r.put_pc(out, &pc2);
        }
        q::PE_FIND_MATCH | q::PE_ADD_ED_CIS => {
            let e = r.pe(rd.idx());
            let c1 = read_ci(r, rd);
            let c2 = read_ci(r, rd);
            let (Some(e), Some((cc1, ci1)), Some((cc2, ci2))) = (e, c1, c2) else {
                *skip = true;
                return Ok(());
            };
            if *skip {
                return Ok(());
            }
            if op == q::PE_FIND_MATCH {
                let found = res(
                    r.mgr
                        .find_matching_edge_definition(&r.k, &r.pool, e, &cc1, &ci1, &ci2),
                    st,
                    skip,
                )?;
                out.push(r.ed_id(found.flatten()));
                out.push(0.0);
                return Ok(());
            }
            // HANG-12 guard (the shim's)
            let pe = r.mgr.proto_edge(e);
            if pe.is_some_and(|x| x.num_edge_definitions() > 0 && zero_len(&x.xsect_interval())) {
                *skip = true;
                return Ok(());
            }
            let _ = res(
                r.mgr
                    .add_edge_definition_from_intervals(&r.k, &mut r.pool, e, &cc1, &ci1, &cc2, &ci2),
                st,
                skip,
            )?;
            sync_counts(r);
        }
        q::PE_ADD_ED => {
            let e = r.pe(rd.idx());
            let d = r.ed(rd.idx());
            let (Some(e), Some(d)) = (e, d) else {
                *skip = true;
                return Ok(());
            };
            if *skip {
                return Ok(());
            }
            let pe = r.mgr.proto_edge(e);
            if pe.is_some_and(|x| x.num_edge_definitions() > 0 && zero_len(&x.xsect_interval())) {
                *skip = true;
                return Ok(());
            }
            let _ = res(r.mgr.add_edge_definition(&r.k, &mut r.pool, e, d), st, skip)?;
        }
        q::PE_COMBINE => {
            let e = r.pe(rd.idx());
            let o = r.pe(rd.idx());
            let (Some(e), Some(o)) = (e, o) else {
                *skip = true;
                return Ok(());
            };
            if *skip {
                return Ok(());
            }
            if e != o {
                // the shim's conservative guard
                let (ne, ivl) = r
                    .mgr
                    .proto_edge(e)
                    .map_or((0, Extent1d::new()), |x| {
                        (x.num_edge_definitions(), x.xsect_interval())
                    });
                let others = r
                    .mgr
                    .proto_edge(o)
                    .map(|x| x.edge_definitions().to_vec())
                    .unwrap_or_default();
                let no = others.len();
                if no >= 1 && (ne > 0 || no >= 2) {
                    if ne > 0 && (zero_len(&ivl) || !pe_tangent_ok(&r.k, r, e)?) {
                        *skip = true;
                    }
                    for d in others {
                        let short = r.mgr.edge_def(d).is_some_and(|x| zero_len(&x.xsect_interval()));
                        if short || !ed_tangent_ok(&r.k, r, d)? {
                            *skip = true;
                        }
                    }
                }
                if *skip {
                    return Ok(());
                }
            }
            let _ = res(r.mgr.combine_proto_edge(&r.k, &mut r.pool, e, o), st, skip)?;
        }
        q::PE_RESOLVE => {
            let e = r.pe(rd.idx());
            let Some(e) = e.filter(|_| !*skip) else {
                *skip = true;
                return Ok(());
            };
            let eds = r
                .mgr
                .proto_edge(e)
                .map(|x| x.edge_definitions().to_vec())
                .unwrap_or_default();
            if has_dup(&eds) {
                *skip = true;
                return Ok(());
            }
            let Some(deleted) = res(r.mgr.resolve_proto_edge(&r.k, &mut r.pool, e), st, skip)? else {
                return Ok(());
            };
            // a curve freed twice, or freed while a kept definition uses it (the shim's)
            let kept: Vec<H> = r
                .mgr
                .proto_edge(e)
                .map(|x| x.edge_definitions().to_vec())
                .unwrap_or_default()
                .iter()
                .filter_map(|&d| r.mgr.edge_def(d))
                .flat_map(|d| [d.xsect_curve_3d(), d.xsect_uv_curve(1), d.xsect_uv_curve(2)])
                .flatten()
                .collect();
            if has_dup(&deleted) || deleted.iter().any(|c| kept.contains(c)) {
                *skip = true;
                return Ok(());
            }
            let mut ids: Vec<f64> = deleted.iter().map(|&c| r.pool.id(Some(c))).collect();
            ids.sort_by(f64::total_cmp);
            for c in deleted {
                r.pool.kill(c);
            }
            out.push(ids.len() as f64);
            out.extend(ids);
            sync_counts(r);
        }
        q::MGR_CREATE_VD | q::MGR_CREATE_PV => {
            let pc1 = r.read_pc(rd);
            let pc2 = r.read_pc(rd);
            let (Some(mut pc1), Some(mut pc2)) = (pc1, pc2) else {
                *skip = true;
                return Ok(());
            };
            if *skip {
                return Ok(());
            }
            if op == q::MGR_CREATE_VD {
                let vd = res(
                    r.mgr
                        .create_vertex_definition(&r.k, &r.pool, &mut pc1, &mut pc2),
                    st,
                    skip,
                )?;
                sync_counts(r);
                out.push(r.vd_id(vd));
            } else {
                let pv = res(r.mgr.create_proto_vertex(&r.k, &r.pool, &mut pc1, &mut pc2), st, skip)?;
                sync_counts(r);
                out.push(r.pv_id(pv.flatten()));
            }
            r.put_pc(out, &pc1);
            r.put_pc(out, &pc2);
        }
        q::MGR_FIND_OR_CREATE_PV => {
            let pt = rd.vec();
            if *skip {
                return Ok(());
            }
            let got = res(r.mgr.find_or_create_proto_vertex(pt), st, skip)?;
            sync_counts(r);
            out.push(r.pv_id(got.map(|g| g.0)));
            out.push(b(got.is_some_and(|g| g.1)));
        }
        q::MGR_FIND_PE => {
            let v1 = r.pv_or_null(rd.idx(), &mut ok);
            let v2 = r.pv_or_null(rd.idx(), &mut ok);
            let pc1 = r.read_pc(rd);
            let pc2 = r.read_pc(rd);
            let c = r.curve_or_null(rd.idx(), &mut ok);
            let a = rd.num();
            let bb = rd.num();
            let (Some(pc1), Some(pc2)) = (pc1, pc2) else {
                *skip = true;
                return Ok(());
            };
            if *skip || !ok {
                *skip = true;
                return Ok(());
            }
            let dom = Extent1d::from_min_max(a, bb);
            let got = res(
                r.mgr
                    .find_proto_edge(&r.k, &r.pool, v1, v2, &pc1, &pc2, c, &dom),
                st,
                skip,
            )?;
            out.push(r.pe_id(got.and_then(|g| g.0)));
            out.push(b(got.is_some_and(|g| g.1)));
        }
        q::MGR_CREATE_PE => {
            let v1 = r.pv_or_null(rd.idx(), &mut ok);
            let v2 = r.pv_or_null(rd.idx(), &mut ok);
            let c1 = read_ci(r, rd);
            let c2 = read_ci(r, rd);
            let (Some((cc1, ci1)), Some((cc2, ci2))) = (c1, c2) else {
                *skip = true;
                return Ok(());
            };
            if *skip || !ok || cc1.curve().is_none() {
                *skip = true;
                return Ok(());
            }
            let pe = res(
                r.mgr
                    .create_proto_edge(&r.k, &mut r.pool, v1, v2, &cc1, &ci1, &cc2, &ci2),
                st,
                skip,
            )?;
            sync_counts(r);
            out.push(r.pe_id(pe));
        }
        q::MGR_REMOVE_ED => {
            let d = r.ed(rd.idx());
            let flag = rd.flag();
            let Some(d) = d.filter(|_| !*skip) else {
                *skip = true;
                return Ok(());
            };
            let _ = res(r.mgr.remove_edge_definition(d, flag), st, skip)?;
        }
        q::MGR_UPDATE_EDGES => {
            let w = rd.idx();
            let edge_or_null = |r: &ProtoRun, j: i64, ok: &mut bool| -> Option<H> {
                if j == -1 {
                    return None;
                }
                if !r.pool.is(j, pk::EDGE) {
                    *ok = false;
                    return None;
                }
                Some(r.pool.h(j))
            };
            let old = edge_or_null(r, rd.idx(), &mut ok);
            let n = rd.idx();
            let mut edges = Vec::new();
            for _ in 0..n.clamp(0, 4) {
                match edge_or_null(r, rd.idx(), &mut ok) {
                    Some(x) => edges.push(x),
                    None => ok = false,
                }
            }
            let j = rd.idx();
            let vd = if j == -1 {
                None
            } else {
                let x = r.vd(j);
                if x.is_none() {
                    ok = false;
                }
                x
            };
            if *skip || !ok || !(1..=2).contains(&w) || !(0..=4).contains(&n) {
                *skip = true;
                return Ok(());
            }
            let _ = res(
                r.mgr
                    .update_edges(&r.k, &mut r.pool, w as i32, old, &edges, vd),
                st,
                skip,
            )?;
        }
        q::MGR_UPDATE_FACES => {
            let w = rd.idx();
            let fo = rd.idx();
            let fne = rd.idx();
            let se = rd.idx();
            let oe = rd.idx();
            let ed = rd.idx();
            if *skip {
                return Ok(());
            }
            let face = |j: i64| r.pool.is(j, pk::FACE).then(|| r.pool.h(j));
            let edge = |j: i64| r.pool.is(j, pk::EDGE).then(|| r.pool.h(j));
            match (face(fo), face(fne)) {
                (Some(f1), Some(f2)) if (1..=2).contains(&w) => {
                    if !r.pool.is_owned_face(f1, w)
                        || !r.pool.is_owned_face(f2, w)
                        || se != -1
                        || oe != -1
                        || ed != -1
                        || r.n_pf != 0
                    {
                        // the shim's guard: bare-face stand-ins have no B-rep or loops
                        // for `UpdateFaces` to walk, and splits with edges or proto faces
                        // need fixtures the shim does not build; it refuses the op
                        *skip = true;
                        return Ok(());
                    }
                    if !face_update_edges_ok(r, w as i32, f1)? {
                        *skip = true;
                        return Ok(());
                    }
                    let (se, oe, ed) = (edge(se), edge(oe), r.ed(ed));
                    let _ = res(
                        r.mgr
                            .update_faces(&r.k, &mut r.pool, w as i32, f1, f2, se, oe, ed),
                        st,
                        skip,
                    )?;
                    if !*skip {
                        r.pool.put_owned_face(f1, out)?;
                        r.pool.put_owned_face(f2, out)?;
                    }
                }
                _ => *skip = true,
            }
        }
        q::MGR_CAN_COMBINE_VDS => {
            let d1 = r.vd(rd.idx());
            let d2 = r.vd(rd.idx());
            let (Some(d1), Some(d2)) = (d1, d2) else {
                *skip = true;
                return Ok(());
            };
            if *skip {
                return Ok(());
            }
            match res(r.mgr.can_combine_vertex_definitions(&r.k, &r.pool, d1, d2), st, skip)? {
                Some(c) => out.push(c as f64),
                None => out.push(0.0),
            }
        }
        _ => return Err(Fail::Harness(HarnessError::new("proto op"))),
    }
    Ok(())
}

// ---- generation -------------------------------------------------------------------------------

/// Generator state: the pool layout and the creation counts of the script so far.
struct Gen {
    scale: f64,
    /// Pool curves with their natural domains.
    curves: Vec<(i64, [f64; 2])>,
    surfs: Vec<i64>,
    vertices: Vec<i64>,
    /// Edges with their curve and interval: pairs `(old, new)` split one curve.
    edges: Vec<(i64, i64, [f64; 2])>,
    splits: Vec<(i64, i64)>,
    faces: Vec<i64>,
    /// Regions and loops (topology for definitions).
    others: Vec<i64>,
    /// Regions only (the class of a point classification).
    regions: Vec<i64>,
    n_vd: i64,
    n_ed: i64,
    n_pv: i64,
    n_pe: i64,
}

impl Gen {
    fn pick(rng: &mut Rng, n: i64) -> f64 {
        if n == 0 || rng.chance(0.03) {
            if rng.chance(0.5) {
                -1.0
            } else {
                n as f64
            }
        } else {
            rng.below(n as u64) as f64
        }
    }
    fn of(rng: &mut Rng, v: &[i64]) -> f64 {
        rng.choose(v).map_or(-1.0, |&x| x as f64)
    }
    fn curve(&self, rng: &mut Rng) -> (f64, [f64; 2]) {
        rng.choose(&self.curves)
            .map_or((-1.0, [0.0, 1.0]), |&(c, d)| (c as f64, d))
    }
    fn sub(rng: &mut Rng, d: [f64; 2]) -> [f64; 2] {
        match rng.below(10) {
            0..=3 => d,
            4 => {
                // a sliver at one end
                let w = (d[1] - d[0]) * 1e-9;
                [d[1] - w, d[1]]
            }
            _ => {
                let a = rng.uniform(0.0, 0.6);
                let bb = rng.uniform(a + 0.05, 1.0);
                [d[0] + (d[1] - d[0]) * a, d[0] + (d[1] - d[0]) * bb]
            }
        }
    }
    fn point(&self, rng: &mut Rng) -> [f64; 3] {
        let p = rp(rng, 10.0);
        [p[0] * self.scale, p[1] * self.scale, p[2] * self.scale]
    }
    /// A topology object for definitions: regions and loops mostly (comparable), edges,
    /// vertices and faces sometimes.
    fn topo(&self, rng: &mut Rng) -> f64 {
        match rng.below(10) {
            0..=5 => Self::of(rng, &self.others),
            6..=7 => {
                let e: Vec<i64> = self.edges.iter().map(|x| x.0).collect();
                Self::of(rng, &e)
            }
            8 => Self::of(rng, &self.vertices),
            _ => Self::of(rng, &self.faces),
        }
    }
    /// A point classification: on a region (comparable topology), a curve or surface,
    /// sometimes on other topology or nothing.
    fn pc(&self, rng: &mut Rng) -> Vec<f64> {
        let tol = gen_tol(rng, self.scale);
        let gap = if rng.chance(0.4) { 0.0 } else { rng.uniform(0.0, 0.05) * self.scale };
        match rng.below(20) {
            0..=5 => {
                let o = Self::of(rng, &self.regions);
                pcv(101.0, o, tol, f64::from(rng.below(8) as u32), rng.unit(), rng.unit(), rng.unit(), gap)
            }
            6..=11 => {
                let (c, d) = self.curve(rng);
                let t = d[0] + (d[1] - d[0]) * rng.uniform(-0.05, 1.05);
                let flags = if rng.chance(0.9) { 1.0 } else { 0.0 } + if rng.chance(0.5) { 4.0 } else { 0.0 };
                pcv(106.0, c, tol, flags, t, 0.0, 0.0, gap)
            }
            12..=14 => {
                let s = Self::of(rng, &self.surfs);
                let flags = if rng.chance(0.95) { 2.0 } else { 0.0 } + if rng.chance(0.5) { 4.0 } else { 0.0 };
                pcv(107.0, s, tol, flags, 0.0, rng.uniform(-0.1, 1.1), rng.uniform(-0.1, 1.1), gap)
            }
            15..=16 => {
                let (e, _, d) = rng.choose(&self.edges).copied().unwrap_or((-1, -1, [0.0, 1.0]));
                let t = d[0] + (d[1] - d[0]) * rng.uniform(-0.1, 1.1);
                pcv(103.0, e as f64, tol, 1.0 + 4.0 * f64::from(u8::from(rng.chance(0.7))), t, 0.0, 0.0, gap)
            }
            17 => pcv(104.0, Self::of(rng, &self.vertices), tol, 4.0, 0.0, 0.0, 0.0, gap),
            18 => pcv(102.0, Self::of(rng, &self.faces), tol, 2.0, rng.unit(), rng.unit(), 0.0, gap),
            _ => pcv(*rng.choose(&[100.0, 110.0]).unwrap_or(&100.0), -1.0, tol, 0.0, 0.0, 0.0, 0.0, gap),
        }
    }
    /// A point classification on a curve or surface mostly (positions are found).
    fn pc_geom(&self, rng: &mut Rng) -> Vec<f64> {
        let tol = gen_tol(rng, self.scale);
        let gap = if rng.chance(0.4) { 0.0 } else { rng.uniform(0.0, 0.05) * self.scale };
        match rng.below(10) {
            0..=5 => {
                let (c, d) = self.curve(rng);
                let t = d[0] + (d[1] - d[0]) * rng.uniform(-0.05, 1.05);
                let flags = if rng.chance(0.85) { 1.0 } else { 0.0 } + if rng.chance(0.5) { 4.0 } else { 0.0 };
                pcv(106.0, c, tol, flags, t, 0.0, 0.0, gap)
            }
            6..=8 => {
                let s = Self::of(rng, &self.surfs);
                pcv(107.0, s, tol, 2.0, 0.0, rng.uniform(-0.1, 1.1), rng.uniform(-0.1, 1.1), gap)
            }
            _ => self.pc(rng),
        }
    }
    /// A curve interval `c uv tol a b pc`, mostly on a pool curve with a region midpoint.
    fn ci(&self, rng: &mut Rng, c: f64, d: [f64; 2]) -> Vec<f64> {
        let uv = if rng.chance(0.25) { self.curve(rng).0 } else { -1.0 };
        let tol = gen_tol(rng, self.scale);
        let mid = if rng.chance(0.9) {
            pcv(101.0, Self::of(rng, &self.regions), tol, 0.0, 0.0, 0.0, 0.0, 0.0)
        } else {
            self.pc(rng)
        };
        cat(&[&[c, uv, tol, d[0], d[1]], &mid])
    }
}

/// A random proto script: a pool of curves (lines, B-splines, circles, ellipses, Hermite
/// curves), a plane and B-spline surface, bare vertices and edges with tolerances (some
/// edges in pairs splitting one curve), regions and loops; proto topology built with the
/// ops of unit `topology.proto_topology_p1`; then ops of this unit.
#[allow(clippy::too_many_lines)]
fn gen_script(rng: &mut Rng) -> Script {
    let scale = gen_scale(rng);
    let mut objects: Vec<ObjIn> = Vec::new();
    let mut g = Gen {
        scale,
        curves: Vec::new(),
        surfs: Vec::new(),
        vertices: Vec::new(),
        edges: Vec::new(),
        splits: Vec::new(),
        faces: Vec::new(),
        others: Vec::new(),
        regions: Vec::new(),
        n_vd: 0,
        n_ed: 0,
        n_pv: 0,
        n_pe: 0,
    };
    let push_curve = |objects: &mut Vec<ObjIn>, g: &mut Gen, c: CurveIn| -> i64 {
        let d = build_curve(&c).map_or([0.0, 1.0], |cv| {
            let d = cv.natural_interval();
            [d.min(), d.max()]
        });
        let id = objects.len() as i64;
        g.curves.push((id, d));
        objects.push(ObjIn::Curve(c));
        id
    };
    for _ in 0..2 + rng.below(3) {
        let c = gen_curve(rng, scale);
        push_curve(&mut objects, &mut g, c);
    }
    // a near-duplicate of the first curve (same-curve checks between definitions)
    if rng.chance(0.5) {
        if let Some(ObjIn::Curve(c)) = objects.first().cloned() {
            push_curve(&mut objects, &mut g, c);
        }
    }
    let plane = objects.len() as i64;
    objects.push(ObjIn::Plane {
        o: g.point(rng),
        n: dir(rng),
    });
    g.surfs.push(plane);
    for _ in 0..2 {
        g.vertices.push(objects.len() as i64);
        objects.push(if rng.chance(0.5) {
            ObjIn::Vertex
        } else {
            ObjIn::VertexTol(gen_tol(rng, scale))
        });
    }
    // split pairs: old edge [a, m] and new edge [m, b] of one edge curve
    for _ in 0..=rng.below(2) {
        let (c, d) = g.curve(rng);
        if c < 0.0 {
            continue;
        }
        let m = d[0] + (d[1] - d[0]) * rng.uniform(0.2, 0.8);
        let tol = if rng.chance(0.5) { 1e-6 * scale } else { gen_tol(rng, scale) };
        let old = objects.len() as i64;
        objects.push(ObjIn::EdgeIvl {
            c: c as usize,
            a: d[0],
            b: m,
            tol,
        });
        let new = objects.len() as i64;
        let (na, nb) = if rng.chance(0.85) { (m, d[1]) } else { (d[0], d[1]) };
        objects.push(ObjIn::EdgeIvl {
            c: c as usize,
            a: na,
            b: nb,
            tol: if rng.chance(0.7) { tol } else { gen_tol(rng, scale) },
        });
        g.edges.push((old, c as i64, [d[0], m]));
        g.edges.push((new, c as i64, [na, nb]));
        g.splits.push((old, new));
    }
    if rng.chance(0.3) {
        let (c, _) = g.curve(rng);
        if c >= 0.0 {
            g.edges.push((objects.len() as i64, c as i64, [-1e20, 1e20]));
            objects.push(ObjIn::Edge(c as usize));
        }
    }
    g.faces.push(objects.len() as i64);
    objects.push(ObjIn::Face(plane as usize));
    for i in 0..3 {
        g.others.push(objects.len() as i64);
        if i < 2 || rng.chance(0.5) {
            g.regions.push(objects.len() as i64);
            objects.push(ObjIn::Region);
        } else {
            objects.push(ObjIn::Loop);
        }
    }
    let approx_tol = match rng.below(10) {
        0 => 1e-3 * scale,
        1 => 0.1 * scale,
        _ => 1e-5 * scale.max(1.0),
    };
    let capture = match rng.below(4) {
        0 => 0.0,
        1 => 1e-3 * scale,
        _ => rng.uniform(1e-4, 1.0) * scale,
    };

    let mut ops: Vec<Vec<f64>> = Vec::new();
    // proto vertices
    for _ in 0..2 + rng.below(3) {
        let mut v = vec![p::PV_NEW as f64];
        v.extend(g.point(rng));
        v.push(gen_tol(rng, scale));
        ops.push(v);
        g.n_pv += 1;
        if rng.chance(0.85) {
            ops.push(vec![p::MGR_ADD_PV as f64, (g.n_pv - 1) as f64]);
        }
    }
    // vertex definitions at them
    for _ in 0..rng.below(4) {
        let pv = Gen::pick(rng, g.n_pv);
        let mut v = vec![p::VD_NEW as f64];
        v.extend(g.pc(rng));
        v.extend(g.pc(rng));
        v.extend(g.point(rng));
        v.push(gen_tol(rng, scale));
        v.push(if rng.chance(0.8) { rng.uniform(1e-9, 1e-3) * scale } else { 0.0 });
        v.push(pv);
        ops.push(v);
        g.n_vd += 1;
        if pv >= 0.0 && pv < g.n_pv as f64 {
            ops.push(vec![p::PV_ADD_VD as f64, pv, (g.n_vd - 1) as f64]);
        }
    }
    // proto edges with definitions (some on the split edges)
    for _ in 0..=rng.below(3) {
        let s = Gen::pick(rng, g.n_pv);
        let e = if rng.chance(0.15) { s } else { Gen::pick(rng, g.n_pv) };
        let (c, d) = g.curve(rng);
        let dd = Gen::sub(rng, d);
        ops.push(vec![p::PE_NEW as f64, s, e, dd[0], dd[1]]);
        g.n_pe += 1;
        let pe = (g.n_pe - 1) as f64;
        if rng.chance(0.9) {
            ops.push(vec![p::MGR_ADD_PE as f64, pe]);
        }
        if s >= 0.0 && s < g.n_pv as f64 {
            ops.push(vec![p::PV_ADD_PE as f64, s, pe]);
        }
        if e >= 0.0 && e < g.n_pv as f64 && e != s {
            ops.push(vec![p::PV_ADD_PE as f64, e, pe]);
        }
        for _ in 0..=rng.below(2) {
            let s1 = g.topo(rng);
            let s2 = g.topo(rng);
            let (c3, d3) = if rng.chance(0.85) { (c, dd) } else {
                let (c3, d3) = g.curve(rng);
                (c3, Gen::sub(rng, d3))
            };
            // an edge source on its own curve over the edge's interval
            let (c3, d3, s1) = if rng.chance(0.25) {
                match rng.choose(&g.edges).copied() {
                    Some((ed, ec, ei)) if ei[0] > -1e19 => (ec as f64, ei, ed as f64),
                    _ => (c3, d3, s1),
                }
            } else {
                (c3, d3, s1)
            };
            let uv1 = if rng.chance(0.15) { g.curve(rng).0 } else { -1.0 };
            ops.push(vec![
                p::ED_NEW as f64,
                c3,
                d3[0],
                d3[1],
                s1,
                s2,
                gen_tol(rng, scale),
                uv1,
                -1.0,
                pe,
            ]);
            g.n_ed += 1;
            ops.push(vec![p::PE_ADD_ED as f64, pe, (g.n_ed - 1) as f64]);
        }
    }

    // the ops of this unit; those that defer the whole case come rarely
    let n_ops = 2 + rng.below(8);
    for _ in 0..n_ops {
        let code = match rng.below(100) {
            0..=11 => q::MGR_CREATE_PE,
            12..=21 => q::PE_ADD_ED_CIS,
            22..=27 => q::PE_FIND_MATCH,
            28..=33 => q::PE_ADD_ED,
            34..=40 => q::PE_COMBINE,
            41..=50 => q::PE_RESOLVE,
            51..=56 => q::MGR_CREATE_VD,
            57..=60 => q::MGR_CREATE_PV,
            61..=65 => q::MGR_FIND_PE,
            66 => q::MGR_FIND_OR_CREATE_PV,
            67..=72 => q::MGR_REMOVE_ED,
            73..=82 => q::MGR_UPDATE_EDGES,
            83..=88 => q::MGR_CAN_COMBINE_VDS,
            89..=92 => q::PV_RESOLVE,
            93..=95 => q::PV_ADD_VD_PCS,
            96..=97 => q::ED_IMPRINT,
            98 => q::VD_IMPRINT,
            _ => q::MGR_UPDATE_FACES,
        };
        let mut v = vec![code as f64];
        match code {
            q::MGR_CREATE_PE => {
                let (c, d) = g.curve(rng);
                let dd = Gen::sub(rng, d);
                v.push(Gen::pick(rng, g.n_pv));
                v.push(Gen::pick(rng, g.n_pv));
                let c1 = g.ci(rng, c, dd);
                let c2c = if rng.chance(0.9) { c } else { g.curve(rng).0 };
                let c2 = g.ci(rng, c2c, dd);
                v.extend(c1);
                v.extend(c2);
                g.n_pe += 1;
                g.n_ed += 1;
            }
            q::PE_ADD_ED_CIS | q::PE_FIND_MATCH => {
                let (c, d) = g.curve(rng);
                let dd = Gen::sub(rng, d);
                v.push(Gen::pick(rng, g.n_pe));
                v.extend(g.ci(rng, c, dd));
                v.extend(g.ci(rng, c, dd));
                if code == q::PE_ADD_ED_CIS {
                    g.n_ed += 1;
                }
            }
            q::PE_ADD_ED => {
                v.push(Gen::pick(rng, g.n_pe));
                v.push(Gen::pick(rng, g.n_ed));
            }
            q::PE_COMBINE => {
                v.push(Gen::pick(rng, g.n_pe));
                v.push(Gen::pick(rng, g.n_pe));
            }
            q::PE_RESOLVE => v.push(Gen::pick(rng, g.n_pe)),
            q::MGR_CREATE_VD | q::MGR_CREATE_PV => {
                v.extend(g.pc_geom(rng));
                v.extend(g.pc_geom(rng));
                g.n_vd += 1;
                g.n_pv += i64::from(code == q::MGR_CREATE_PV);
            }
            q::MGR_FIND_PE => {
                let (c, d) = g.curve(rng);
                let dd = Gen::sub(rng, d);
                v.push(Gen::pick(rng, g.n_pv));
                v.push(Gen::pick(rng, g.n_pv));
                v.extend(g.pc(rng));
                v.extend(g.pc(rng));
                v.extend([c, dd[0], dd[1]]);
            }
            q::MGR_FIND_OR_CREATE_PV => {
                v.extend(g.point(rng));
                g.n_pv += 1;
            }
            q::MGR_REMOVE_ED => {
                v.push(Gen::pick(rng, g.n_ed));
                v.push(f64::from(u8::from(rng.chance(0.6))));
            }
            q::MGR_UPDATE_EDGES => {
                let w = if rng.chance(0.99) { f64::from(1 + rng.below(2) as u32) } else { 3.0 };
                let (old, new) = rng.choose(&g.splits).copied().unwrap_or((-1, -1));
                let (old, new) = (old as f64, new as f64);
                v.push(w);
                match rng.below(10) {
                    0 => v.extend([-1.0, 0.0]),
                    1 => v.extend([old, 1.0, old]),
                    2 => v.extend([old, 2.0, new, new]),
                    3..=5 => v.extend([old, 2.0, old, new]),
                    _ => v.extend([old, 2.0, new, old]),
                }
                v.push(Gen::pick(rng, g.n_vd));
            }
            q::MGR_CAN_COMBINE_VDS => {
                v.push(Gen::pick(rng, g.n_vd));
                v.push(Gen::pick(rng, g.n_vd));
            }
            q::PV_RESOLVE => v.push(Gen::pick(rng, g.n_pv)),
            q::PV_ADD_VD_PCS => {
                v.push(Gen::pick(rng, g.n_pv));
                v.extend(g.pc(rng));
                v.extend(g.pc(rng));
                g.n_vd += 1;
            }
            q::ED_IMPRINT => v.push(Gen::pick(rng, g.n_ed)),
            q::VD_IMPRINT => v.push(Gen::pick(rng, g.n_vd)),
            _ => {
                let f = Gen::of(rng, &g.faces);
                let e: Vec<i64> = g.edges.iter().map(|x| x.0).collect();
                v.extend([1.0, f, f, Gen::of(rng, &e), -1.0, Gen::pick(rng, g.n_ed)]);
            }
        }
        ops.push(v);
    }
    Script::Proto {
        objects,
        approx_tol,
        capture,
        override_tol: rng.uniform(-1.0, 1.0),
        ops,
    }
}

// ---- real-face updates ------------------------------------------------------------------------

/// Generated `UpdateFaces` scripts on real owned faces, beside the random scripts.
pub const REAL_FACE_CASES: usize = 200;

/// `UpdateFaces` scripts on real faces, [`REAL_FACE_CASES`] of them from fixed seeds: a
/// bilinear patch (planar or twisted, at a random scale) twice, the old and the new face of
/// a split of its domain (along `u` or `v`, with a common boundary, an overlap or a gap)
/// created in one operand with an initial domain and sometimes shifted trims, then
/// `UpdateFaces` with either proto vertices whose definitions classify on a face of the
/// split (inside either face, on or near the split line, outside both) or the one proto
/// edge with a direct UV segment the shim admits (inside, across or along the split).
/// Inputs stay within the shim's `UpdateFaces` guard, so both sides run the update.
fn real_face_update_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for i in 0..REAL_FACE_CASES {
        let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p2/real-face", i));
        if let Ok(v) = serde_json::to_value(real_face_script(&mut rng)) {
            out.push((format!("real_face_gen{i:03}"), v));
        }
    }
    out
}

/// One generated real-face `UpdateFaces` script (see [`real_face_update_cases`]).
#[allow(clippy::too_many_lines)]
fn real_face_script(rng: &mut Rng) -> Script {
    use super::topology_proto_topology_p1::{BSplineIn, SurfIn, NOW};
    let s = match rng.below(4) {
        0 => 1.0,
        1 => 1e3,
        2 => 1e-3,
        _ => 10f64.powf(rng.uniform(-2.0, 2.0)),
    };
    // P(u, v) = (u s, v s, u v z s): planar or twisted
    let z = if rng.chance(0.3) { rng.uniform(-0.5, 0.5) } else { 0.0 };
    let eval = |u: f64, v: f64| [u * s, v * s, u * v * z * s];
    let corner = |u: f64, v: f64| {
        let p = eval(u, v);
        [p[0], p[1], p[2], 1.0]
    };
    let patch = || {
        ObjIn::Bsurf(SurfIn {
            oob: false,
            net: vec![
                vec![corner(0.0, 0.0), corner(0.0, 1.0)],
                vec![corner(1.0, 0.0), corner(1.0, 1.0)],
            ],
            p: 1,
            q: 1,
            u: vec![0.0, 0.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
        })
    };
    // the split: the low side ends at `a`, the high side starts at `bb`
    let along_v = rng.chance(0.5);
    let a = rng.uniform(0.15, 0.85);
    let bb = match rng.below(10) {
        0..=4 => a,
        5..=8 => a - rng.uniform(0.0, a.min(0.3)),
        _ => a + rng.uniform(0.0, (1.0 - a).min(0.1)),
    };
    let rect = |lo: f64, hi: f64| {
        if along_v {
            [0.0, lo, 1.0, hi]
        } else {
            [lo, 0.0, hi, 1.0]
        }
    };
    let (low, high) = (rect(0.0, a), rect(bb, 1.0));
    let (old_dom, new_dom) = if rng.chance(0.75) { (low, high) } else { (high, low) };
    let initial = |rng: &mut Rng, own: [f64; 4]| match rng.below(10) {
        0..=5 => [-1.0, -1.0, 2.0, 2.0],
        6..=7 => own,
        _ => [
            -rng.uniform(0.0, 2.0),
            -rng.uniform(0.0, 2.0),
            1.0 + rng.uniform(0.0, 2.0),
            1.0 + rng.uniform(0.0, 2.0),
        ],
    };
    let shift = |rng: &mut Rng, p: f64| {
        if rng.chance(p) {
            [rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)]
        } else {
            [0.0, 0.0]
        }
    };
    let which: u8 = if rng.chance(0.5) { 1 } else { 2 };
    let mut objects = vec![patch(), patch()];
    for (surface, domain, p_shift) in [(0, old_dom, 0.15), (1, new_dom, 0.1)] {
        let initial_domain = initial(rng, domain);
        let trim_shift = shift(rng, p_shift);
        objects.push(ObjIn::TrimmedFace {
            operand: which,
            surface,
            domain,
            initial_domain,
            trim_shift,
        });
    }
    objects.push(ObjIn::Region);
    let (old_face, new_face, region) = (2.0, 3.0, 4.0);
    // a parameter across the split: inside either side, on or near it, or outside both
    let param = |rng: &mut Rng| match rng.below(10) {
        0..=3 => rng.uniform(-0.3, 1.3),
        4 => a,
        5 => bb,
        6 => a + rng.uniform(-1e-6, 1e-6),
        7 => rng.uniform(0.0, 1.0),
        _ => {
            if rng.chance(0.5) {
                rng.uniform(0.0, a)
            } else {
                rng.uniform(bb, 1.0)
            }
        }
    };
    let across = |rng: &mut Rng| rng.uniform(-0.2, 1.2);
    let uv_at = |t: f64, c: f64| if along_v { (c, t) } else { (t, c) };
    let tol = |rng: &mut Rng| match rng.below(3) {
        0 => 0.0,
        1 => 1e-4 * s,
        _ => rng.uniform(1e-7, 1e-3) * s,
    };
    let mut ops: Vec<Vec<f64>> = Vec::new();
    if rng.chance(0.6) {
        // proto vertices with definitions classified on a face of the split
        for i in 0..=rng.below(3) {
            let (u, v) = uv_at(param(rng), across(rng));
            let pt = eval(u, v);
            let ztol = tol(rng);
            let pv = i as f64;
            ops.push(opv(p::PV_NEW, &cat(&[&pt, &[ztol]])));
            ops.push(opv(p::MGR_ADD_PV, &[pv]));
            let face = if rng.chance(0.85) { old_face } else { new_face };
            let gap = if rng.chance(0.7) { 0.0 } else { rng.uniform(0.0, 1e-3) * s };
            let flags = if gap == 0.0 { 2.0 } else { 6.0 };
            let on_face = pcv(102.0, face, ztol, flags, 0.0, u, v, gap);
            let in_region = pcv(101.0, region, ztol, 0.0, 0.0, 0.0, 0.0, 0.0);
            let (pc1, pc2) = if which == 1 {
                (on_face, in_region)
            } else {
                (in_region, on_face)
            };
            ops.push(opv(p::VD_NEW, &cat(&[&pc1, &pc2, &pt, &[ztol, 0.0, pv]])));
            ops.push(opv(p::PV_ADD_VD, &[pv, pv]));
        }
    } else {
        // the one proto edge with a direct UV segment on the old face
        let (t0, t1, c0, c1) = loop {
            let (t0, t1) = match rng.below(4) {
                // along the split line
                0 => {
                    let t = if rng.chance(0.5) { a } else { bb };
                    (t, t)
                }
                // across the split
                1 => (rng.uniform(-0.2, a), rng.uniform(bb, 1.2)),
                _ => (param(rng), param(rng)),
            };
            let (c0, c1) = (across(rng), across(rng));
            if (t1 - t0).abs() + (c1 - c0).abs() > 0.05 {
                break (t0, t1, c0, c1);
            }
        };
        let (u0, v0) = uv_at(t0, c0);
        let (u1, v1) = uv_at(t1, c1);
        let (p0, p1) = (eval(u0, v0), eval(u1, v1));
        let segment = |dim: u64, p0: [f64; 3], p1: [f64; 3]| {
            ObjIn::Curve(CurveIn::Bspline(BSplineIn {
                dim,
                oob: false,
                cpts: vec![[p0[0], p0[1], p0[2], NOW], [p1[0], p1[1], p1[2], NOW]],
                p: 1,
                u: vec![0.0, 0.0, 1.0, 1.0],
            }))
        };
        let (c3, uv) = (objects.len() as f64, objects.len() as f64 + 1.0);
        objects.push(segment(3, p0, p1));
        objects.push(segment(2, [u0, v0, 0.0], [u1, v1, 0.0]));
        let (tol0, tol1, ed_tol) = (tol(rng), tol(rng), tol(rng));
        ops.push(opv(p::PV_NEW, &cat(&[&p0, &[tol0]])));
        ops.push(opv(p::PV_NEW, &cat(&[&p1, &[tol1]])));
        ops.push(opv(p::MGR_ADD_PV, &[0.0]));
        ops.push(opv(p::MGR_ADD_PV, &[1.0]));
        ops.push(opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]));
        ops.push(opv(p::MGR_ADD_PE, &[0.0]));
        ops.push(opv(p::PV_ADD_PE, &[0.0, 0.0]));
        ops.push(opv(p::PV_ADD_PE, &[1.0, 0.0]));
        let (s1, s2, uv1, uv2) = if which == 1 {
            (old_face, region, uv, -1.0)
        } else {
            (region, old_face, -1.0, uv)
        };
        ops.push(opv(p::ED_NEW, &[c3, 0.0, 1.0, s1, s2, ed_tol, uv1, uv2, 0.0]));
        ops.push(opv(p::PE_ADD_ED, &[0.0, 0.0]));
        ops.push(opv(p::ED_SET_FINAL, &[0.0, 1.0, -1.0]));
        ops.push(opv(p::ED_SET_FINAL, &[0.0, 2.0, -1.0]));
    }
    ops.push(opv(
        q::MGR_UPDATE_FACES,
        &[f64::from(which), old_face, new_face, -1.0, -1.0, -1.0],
    ));
    Script::Proto {
        objects,
        approx_tol: 1e-5 * s,
        capture: 1e-3 * s,
        override_tol: -1.0,
        ops,
    }
}

// ---- cases built in code ----------------------------------------------------------------------

/// Edge cases: the life cycle of a proto edge (create, add, find, combine, resolve,
/// remove), the edge split updates (vertex definitions moving, trims at the calling proto
/// vertex, definitions moving by their midpoint drops), degenerate proto edges, combining
/// vertex definitions, at unit, huge and tiny scales; every upstream error return.
#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (sname, s) in [("unit", 1.0), ("huge", 1e4), ("tiny", 1e-4)] {
        // pool: see edge_objects
        let pt = |x: f64| [x * s, 0.0, 0.0];
        let reg = |o: f64| pcv(101.0, o, 1e-6 * s, 0.0, 0.0, 0.0, 0.0, 0.0);
        let ci = |c: f64, a: f64, b2: f64, mid: &[f64]| cat(&[&[c, -1.0, 1e-6 * s, a, b2], mid]);
        let base = vec![
            opv(p::PV_NEW, &cat(&[&pt(0.0), &[1e-5 * s]])),
            opv(p::PV_NEW, &cat(&[&pt(2.0), &[1e-5 * s]])),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::MGR_ADD_PV, &[1.0]),
        ];
        // create, find, add again, add another source pair, combine, resolve, remove
        let mut ops = base.clone();
        ops.extend([
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 1.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(q::PE_FIND_MATCH, &cat(&[&[0.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(q::PE_ADD_ED_CIS, &cat(&[&[0.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(q::PE_ADD_ED_CIS, &cat(&[&[0.0], &ci(4.0, 0.0, 2.0, &reg(0.0)), &ci(4.0, 0.0, 2.0, &reg(2.0))])),
            opv(q::MGR_FIND_PE, &cat(&[&[0.0, 1.0], &reg(0.0), &reg(1.0), &[3.0, 0.0, 2.0]])),
            opv(q::MGR_CREATE_PE, &cat(&[&[1.0, 0.0], &ci(4.0, 0.5, 1.5, &reg(0.0)), &ci(4.0, 0.5, 1.5, &reg(1.0))])),
            opv(q::PE_COMBINE, &[0.0, 1.0]),
            opv(q::PE_RESOLVE, &[0.0]),
            opv(q::MGR_REMOVE_ED, &[0.0, 1.0]),
            opv(q::MGR_REMOVE_ED, &[2.0, 1.0]),
        ]);
        out.push((format!("edge_pe_life_{sname}"), ops));

        // combining a proto edge with three definitions moves all three (WR-95 was the
        // c9c5979 walk that skipped every second one)
        let mut ops = base.clone();
        ops.extend([
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 1.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(p::PE_NEW, &[0.0, 1.0, 0.0, 2.0]),
            opv(p::MGR_ADD_PE, &[1.0]),
            opv(p::PV_ADD_PE, &[0.0, 1.0]),
            opv(p::PV_ADD_PE, &[1.0, 1.0]),
            opv(p::ED_NEW, &[3.0, 0.0, 2.0, 0.0, 2.0, 1e-6 * s, -1.0, -1.0, 1.0]),
            opv(p::PE_ADD_ED, &[1.0, 1.0]),
            opv(p::ED_NEW, &[4.0, 0.0, 2.0, 1.0, 2.0, 1e-6 * s, -1.0, -1.0, 1.0]),
            opv(p::PE_ADD_ED, &[1.0, 2.0]),
            opv(p::ED_NEW, &[3.0, 0.0, 2.0, 0.0, 1.0, 2e-6 * s, -1.0, -1.0, 1.0]),
            opv(p::PE_ADD_ED, &[1.0, 3.0]),
            opv(q::PE_COMBINE, &[0.0, 1.0]),
        ]);
        out.push((format!("edge_pe_combine_every_other_{sname}"), ops));

        // the same curve twice from the same sources: resolve drops the second
        let mut ops = base.clone();
        ops.extend([
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 1.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(p::ED_NEW, &[4.0, 0.0, 2.0, 0.0, 1.0, 1e-6 * s, -1.0, -1.0, 0.0]),
            opv(q::PE_ADD_ED, &[0.0, 1.0]),
            opv(q::PE_RESOLVE, &[0.0]),
        ]);
        out.push((format!("edge_pe_resolve_same_{sname}"), ops));

        // a closed proto edge: long curve with far ends (a vertex definition is added at
        // the far end) and a short one (definitions dropped)
        let mut ops = vec![
            opv(p::PV_NEW, &cat(&[&pt(0.0), &[1e-5 * s]])),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::VD_NEW, &cat(&[&pcv(106.0, 3.0, 1e-6 * s, 1.0, 0.0, 0.0, 0.0, 0.0), &reg(0.0), &pt(0.0), &[1e-6 * s, 1e-7 * s, 0.0]])),
            opv(p::PV_ADD_VD, &[0.0, 0.0]),
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 0.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(q::PE_RESOLVE, &[0.0]),
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 0.0], &ci(3.0, 0.0, 1e-9, &reg(0.0)), &ci(3.0, 0.0, 1e-9, &reg(1.0))])),
            opv(q::PE_RESOLVE, &[1.0]),
            opv(q::PV_RESOLVE, &[0.0]),
        ];
        out.push((format!("edge_pe_closed_{sname}"), ops.clone()));
        // the circle closes on itself
        ops.truncate(4);
        ops.extend([
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 0.0], &ci(5.0, 0.0, 360.0, &reg(0.0)), &ci(5.0, 0.0, 360.0, &reg(1.0))])),
            opv(q::PE_RESOLVE, &[0.0]),
        ]);
        out.push((format!("edge_pe_closed_circle_{sname}"), ops));

        // an edge split at its middle: definitions on the old edge move by their midpoint,
        // vertex definitions by their parameter, definitions at the calling proto vertex
        // are trimmed at the split point
        let edge_pc = |t: f64| pcv(103.0, 6.0, 1e-6 * s, 1.0, t, 0.0, 0.0, 0.0);
        let ops = vec![
            opv(p::PV_NEW, &cat(&[&pt(1.0), &[1e-5 * s]])),
            opv(p::PV_NEW, &cat(&[&pt(2.0), &[1e-5 * s]])),
            opv(p::PV_NEW, &cat(&[&pt(0.0), &[1e-5 * s]])),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::MGR_ADD_PV, &[1.0]),
            opv(p::MGR_ADD_PV, &[2.0]),
            // the calling definition, and one on the old edge past the split
            opv(p::VD_NEW, &cat(&[&reg(0.0), &reg(1.0), &pt(1.0), &[1e-6 * s, 1e-7 * s, 0.0]])),
            opv(p::PV_ADD_VD, &[0.0, 0.0]),
            opv(p::VD_NEW, &cat(&[&edge_pc(1.5), &reg(1.0), &pt(1.5), &[1e-6 * s, 1e-7 * s, 1.0]])),
            opv(p::PV_ADD_VD, &[1.0, 1.0]),
            opv(p::VD_NEW, &cat(&[&edge_pc(0.5), &reg(1.0), &pt(0.5), &[1e-6 * s, 1e-7 * s, 2.0]])),
            opv(p::PV_ADD_VD, &[2.0, 2.0]),
            // a proto edge from the calling proto vertex over [0, 2] (trimmed), one on the
            // old edge over [1.2, 1.8] (moves), one on the old edge over [0.2, 0.8] (stays)
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 1.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(q::MGR_CREATE_PE, &cat(&[&[1.0, 2.0], &ci(4.0, 1.2, 1.8, &edge_pc(1.5)), &ci(4.0, 1.2, 1.8, &reg(1.0))])),
            opv(q::MGR_CREATE_PE, &cat(&[&[2.0, 1.0], &ci(4.0, 0.2, 0.8, &edge_pc(0.5)), &ci(4.0, 0.2, 0.8, &reg(1.0))])),
            opv(q::MGR_UPDATE_EDGES, &[1.0, 6.0, 2.0, 6.0, 7.0, 0.0]),
            opv(q::MGR_UPDATE_EDGES, &[2.0, 6.0, 2.0, 7.0, 6.0, 0.0]),
        ];
        out.push((format!("edge_update_edges_{sname}"), ops));

        // vertex definitions: same objects in both (larger gap goes), not connected,
        // null sources, then a resolve and the imprint error returns
        let ops = vec![
            opv(p::PV_NEW, &cat(&[&pt(1.0), &[1e-5 * s]])),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::VD_NEW, &cat(&[&reg(0.0), &reg(1.0), &pt(1.0), &[1e-6 * s, 2e-7 * s, 0.0]])),
            opv(p::VD_NEW, &cat(&[&reg(0.0), &reg(1.0), &pt(1.0), &[1e-6 * s, 1e-7 * s, 0.0]])),
            opv(p::VD_NEW, &cat(&[&reg(0.0), &pcv(106.0, 3.0, 1e-6 * s, 1.0, 1.0, 0.0, 0.0, 0.0), &pt(1.0), &[1e-6 * s, 1e-7 * s, 0.0]])),
            opv(p::PV_ADD_VD, &[0.0, 0.0]),
            opv(p::PV_ADD_VD, &[0.0, 1.0]),
            opv(p::PV_ADD_VD, &[0.0, 2.0]),
            opv(q::MGR_CAN_COMBINE_VDS, &[0.0, 1.0]),
            opv(q::MGR_CAN_COMBINE_VDS, &[1.0, 0.0]),
            opv(q::MGR_CAN_COMBINE_VDS, &[0.0, 2.0]),
            opv(q::PV_ADD_VD_PCS, &cat(&[&[0.0], &reg(0.0), &reg(1.0)])),
            opv(q::PV_ADD_VD_PCS, &cat(&[&[0.0], &pcv(106.0, 3.0, 1e-6 * s, 1.0, 1.0, 0.0, 0.0, 0.0), &reg(1.0)])),
            opv(q::PV_RESOLVE, &[0.0]),
            opv(q::MGR_CREATE_VD, &cat(&[&pcv(106.0, 3.0, 1e-6 * s, 1.0, 0.5, 0.0, 0.0, 1e-7 * s), &pcv(106.0, 5.0, 1e-6 * s, 1.0, 10.0, 0.0, 0.0, 0.0)])),
            opv(q::MGR_CREATE_VD, &cat(&[&pcv(106.0, 3.0, 1e-6 * s, 1.0, 0.5, 0.0, 0.0, 0.0), &pcv(107.0, 9.0, 1e-6 * s, 2.0, 0.0, 0.3 * s, 0.4 * s, 0.0)])),
            opv(q::MGR_CREATE_PV, &cat(&[&pcv(106.0, 3.0, 1e-6 * s, 1.0, 0.5, 0.0, 0.0, 0.0), &reg(0.0)])),
        ];
        out.push((format!("edge_vertex_defs_{sname}"), ops));
        // a proto vertex made where none is listed (with one listed, the containment test
        // is not ported)
        let ops = vec![
            opv(p::PV_NEW, &cat(&[&pt(1.0), &[1e-5 * s]])),
            opv(q::MGR_FIND_OR_CREATE_PV, &pt(3.0)),
        ];
        out.push((format!("edge_find_or_create_pv_{sname}"), ops));

        // vertex definition imprint: the merge's error returns (no object, a curve)
        let ops = vec![
            opv(p::PV_NEW, &cat(&[&pt(1.0), &[1e-5 * s]])),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::VD_NEW, &cat(&[&pcv(100.0, -1.0, 1e-6 * s, 0.0, 0.0, 0.0, 0.0, 0.0), &reg(1.0), &pt(1.0), &[1e-6 * s, 1e-7 * s, 0.0]])),
            opv(p::VD_NEW, &cat(&[&pcv(106.0, 3.0, 1e-6 * s, 1.0, 1.0, 0.0, 0.0, 0.0), &reg(1.0), &pt(1.0), &[1e-6 * s, 1e-7 * s, 0.0]])),
            opv(p::PV_ADD_VD, &[0.0, 0.0]),
            opv(p::PV_ADD_VD, &[0.0, 1.0]),
            opv(q::VD_IMPRINT, &[0.0]),
            opv(q::VD_IMPRINT, &[1.0]),
        ];
        out.push((format!("edge_vd_imprint_errors_{sname}"), ops));

        // edge definition imprint: no final vertices (SM_ERR_NULL_POINTER), and the
        // proto edge interval / definition interval reparameterisation (reversed)
        let ops = vec![
            opv(p::PV_NEW, &cat(&[&pt(0.0), &[1e-5 * s]])),
            opv(p::PV_NEW, &cat(&[&pt(2.0), &[1e-5 * s]])),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::MGR_ADD_PV, &[1.0]),
            opv(q::MGR_CREATE_PE, &cat(&[&[0.0, 1.0], &ci(3.0, 0.0, 2.0, &reg(0.0)), &ci(3.0, 0.0, 2.0, &reg(1.0))])),
            opv(q::ED_IMPRINT, &[0.0]),
            opv(p::ED_NEW, &[4.0, 0.0, 2.0, 6.0, 7.0, 1e-6 * s, -1.0, -1.0, -1.0]),
            opv(p::ED_REVERSE, &[1.0]),
            opv(q::PE_ADD_ED, &[0.0, 1.0]),
            opv(q::PE_RESOLVE, &[0.0]),
        ];
        out.push((format!("edge_ed_imprint_reparam_{sname}"), ops));
    }
    out.into_iter()
        .map(|(name, ops)| {
            let scale = if name.ends_with("huge") {
                1e4
            } else if name.ends_with("tiny") {
                1e-4
            } else {
                1.0
            };
            (
                name,
                json!(Script::Proto {
                    objects: edge_objects(scale),
                    approx_tol: 1e-5 * scale.max(1.0),
                    capture: 1e-3 * scale,
                    override_tol: -1.0,
                    ops,
                }),
            )
        })
        .collect()
}

/// The pool of the edge cases at `scale` (see [`edge_cases`]).
fn edge_objects(s: f64) -> Vec<ObjIn> {
    vec![
        ObjIn::Region,
        ObjIn::Region,
        ObjIn::Loop,
        ObjIn::Curve(line([0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, 2.0])),
        ObjIn::Curve(line([0.0, 1e-9 * s, 0.0], [s, 0.0, 0.0], [0.0, 2.0])),
        ObjIn::Curve(CurveIn::Conic(super::topology_proto_topology_p1::ConicIn {
            center: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            domain: [0.0, 360.0],
            rx: s,
            ry: None,
            inside_out: false,
        })),
        ObjIn::EdgeIvl {
            c: 3,
            a: 0.0,
            b: 1.0,
            tol: 1e-6 * s,
        },
        ObjIn::EdgeIvl {
            c: 3,
            a: 1.0,
            b: 2.0,
            tol: 1e-6 * s,
        },
        ObjIn::VertexTol(1e-6 * s),
        ObjIn::Plane {
            o: [0.0, 0.0, 0.0],
            n: [0.0, 0.0, 1.0],
        },
    ]
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.proto_topology_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let s = gen_script(rng);
            if ffi::validate(&cpp_input(&s)).is_err() || !shim_accepts(&s) {
                continue;
            }
            return serde_json::to_value(s).ok();
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases().into_iter().chain(real_face_update_cases()) {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::topology_proto_topology_p1::{rust_run, SKIP};
    use super::*;

    /// Generated scripts are accepted by the ffi checks, and most of them are comparable
    /// (not deferred).
    #[test]
    fn generated_scripts_are_mostly_comparable() {
        let mut rng = Rng::new(23);
        let (mut total, mut deferred) = (0, 0);
        for _ in 0..200 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
        }
        assert!(total > 190, "{total}");
        assert!(deferred * 2 < total, "{deferred} of {total} deferred");
    }

    /// Every generated real-face script reaches `UpdateFaces` and runs it (no op refused,
    /// nothing deferred).
    #[test]
    fn real_face_scripts_run_update_faces() {
        for (name, v) in real_face_update_cases() {
            let s: Script = serde_json::from_value(v).unwrap();
            let Script::Proto { ops, .. } = &s else { panic!("{name}") };
            let Ok(out) = rust_run(&s) else {
                panic!("{name} did not run");
            };
            let mut at = 0;
            for o in ops {
                assert!(at + 2 < out.len(), "{name} ended early");
                assert!(out[at + 1] != SKIP, "{name} refused op {}", o[0]);
                at += 3 + out[at + 2] as usize;
            }
        }
    }

    /// Prints how often each op of this unit runs, is refused or ends in a deferral, and how
    /// many generated scripts are deferred (`cargo test -p difftest --features cpp --lib
    /// p2_coverage -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid"]
    fn p2_coverage() {
        let mut ran: BTreeMap<i64, usize> = BTreeMap::new();
        let mut skipped: BTreeMap<i64, usize> = BTreeMap::new();
        let mut deferred = 0;
        let mut total = 0;
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p2", i));
            let Some(v) = Runner.generate(&mut rng) else { continue };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            let Script::Proto { ops, .. } = &s else { continue };
            let Ok(out) = ffi::run(&cpp_input(&s)) else { continue };
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
            let mut at = 0;
            for o in ops {
                let code = o[0] as i64;
                if at + 1 >= out.len() {
                    break;
                }
                if out[at + 1] == SKIP {
                    *skipped.entry(code).or_default() += 1;
                    if std::env::var("PT_VERBOSE").is_ok() {
                        println!("skip op {code} in case {i}");
                    }
                    break;
                }
                *ran.entry(code).or_default() += 1;
                let n = out[at + 2] as usize;
                at += 3 + n;
            }
        }
        println!("scripts {total}, deferred {deferred}");
        for c in q::FIRST..=q::LAST {
            println!(
                "{c:3} ran {:5} skipped {:5}",
                ran.get(&c).unwrap_or(&0),
                skipped.get(&c).unwrap_or(&0)
            );
        }
    }

    /// Writes each candidate script to `PT_FILE` before the shim runs it, for every
    /// generated case (a debugging aid for shim crashes).
    #[test]
    #[ignore = "debugging aid; needs PT_FILE"]
    fn p2_candidates() {
        let f = std::env::var("PT_FILE").unwrap();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p2", i));
            for _ in 0..64 {
                let s = gen_script(&mut rng);
                if ffi::validate(&cpp_input(&s)).is_err() {
                    continue;
                }
                std::fs::write(&f, serde_json::to_string(&s).unwrap()).unwrap();
                eprintln!("case {i}");
                if shim_accepts(&s) {
                    break;
                }
            }
            let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p2", i));
            if let Some(v) = Runner.generate(&mut rng) {
                let s: Script = serde_json::from_value(v).unwrap();
                let _ = rust_run(&s);
            }
        }
    }

    /// Prints edge case `PT_NAME` with both records (`PT_NAME=edge_update_edges_unit cargo
    /// test -p difftest --features cpp --lib p2_dump_edge -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs PT_NAME"]
    fn p2_dump_edge() {
        let name = std::env::var("PT_NAME").unwrap();
        let (_, v) = edge_cases().into_iter().find(|(n, _)| *n == name).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
        let s: Script = serde_json::from_value(v).unwrap();
        println!("cpp  {:?}", ffi::run(&cpp_input(&s)));
        match rust_run(&s) {
            Ok(o) => println!("rust {o:?}"),
            Err(Fail::Deferred(_)) => println!("rust deferred"),
            Err(Fail::Harness(e)) => println!("rust harness {e}"),
        }
    }

    /// Prints generated case `PT_CASE` with both records (`PT_CASE=6 cargo test -p
    /// difftest --features cpp --lib p2_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs PT_CASE"]
    fn p2_dump_case() {
        let i: usize = std::env::var("PT_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p2", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
        let s: Script = serde_json::from_value(v).unwrap();
        println!("cpp  {:?}", ffi::run(&cpp_input(&s)));
        match rust_run(&s) {
            Ok(o) => println!("rust {o:?}"),
            Err(Fail::Deferred(_)) => println!("rust deferred"),
            Err(Fail::Harness(e)) => println!("rust harness {e}"),
        }
    }
}
