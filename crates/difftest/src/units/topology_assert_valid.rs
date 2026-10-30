//! Differential checks of `SmBrep::AssertValid` and the capi `smlib_brep_check`
//! sequence (`ValidatePointers`, then `AssertValid`) against the pinned C++ reference.
//! Both sides build the body themselves: the `sc_edgeuse_graph.h` sources with the Rust
//! kernel ([`crate::own_builder`]), `.smb` files with the Rust reader, and the
//! primitives with `SmApiCreate*` / `primitive::api_primitives`. Both are numbered in
//! the shim's traversal order, so the mutation selectors and every report identity name
//! the same entities. Report messages are compared after the shim replaces printed
//! edge addresses by the edge's report identity. No C++ state reaches the Rust side.
use super::topology_edgeuse_p1::{brep_block_of, path_of, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::assert_array::{obj_type_string, AssertArray, ObjectKey as ReportKey};
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::validation;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopuseId, NamingScheme, VertexId,
};
use solidcore::{EntityRef, Kernel, Vec3};
use solidcore_ffi::units::topology_assert_valid as ffi;

/// Native `AssertValid` runner.
pub struct Runner;

/// A primitive solid of `SmApiPrimitives.h`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Prim {
    Sphere { origin: [f64; 3], radius: f64 },
    Cylinder { origin: [f64; 3], radius: f64, height: f64 },
    Cone { origin: [f64; 3], radius_base: f64, radius_top: f64, height: f64 },
    Torus { origin: [f64; 3], major: f64, minor: f64 },
}

/// A body: a primitive or a `sc_edgeuse_graph.h` source.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Src {
    Prim(Prim),
    Brep(BrepIn),
}

#[derive(Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // independent serde case flags
struct Inputs {
    brep: Src,
    #[serde(default)]
    level: u8,
    #[serde(default = "yes")]
    walk: bool,
    /// Run the capi sequence instead of `AssertValid` alone.
    #[serde(default)]
    capi: bool,
    /// Run the level-2 tests one by one (shim op 2) instead of `AssertValid`.
    #[serde(default)]
    tests: bool,
    /// Run the capi sequence on the body's `SmApiBrepCopy` (shim op 3; Rust
    /// `api_brep_copy` under caller keys, then `check_brep` by the copy's naming),
    /// without mutation (#1584). Only the check's outcome is compared
    /// ([`copy_summary`]): the copy's report identities are its caller keys.
    #[serde(default)]
    copy: bool,
    /// With `copy`: check the result of an operation on the copy instead (shim op 4,
    /// `copy_operation`; #1631): 1 union with a 10 mm box at `value` on each axis, 2
    /// the same with that box's copy, 3 `fillet_edges` of edge `index` with radius
    /// `value`, 4 a shell of distance `value` with face `index` open.
    #[serde(default)]
    copy_op: u8,
    #[serde(default)]
    mutation: u8,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    value: f64,
}
fn yes() -> bool {
    true
}
fn herr(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn notify_key(t: i64, i: u32) -> ObjectRef {
    let kind = match t {
        16011 => 0,
        16012 => 1,
        16013 => 2,
        16022 => 3,
        16002 => 4,
        16004 => 5,
        16003 => 6,
        16005 => 7,
        16016 => 8,
        16006 => 9,
        16021 => 10,
        _ => 11,
    };
    ObjectRef {
        type_code: t,
        key: ObjectKey(1 + (kind << 32) + u64::from(i)),
    }
}
/// The geometry-owner identities of the shim's build conventions (notification keys).
fn key(e: EntityRef) -> ObjectRef {
    let (t, i) = match e {
        EntityRef::Brep(x) => (16011, x.index()),
        EntityRef::Region(x) => (16012, x.index()),
        EntityRef::Shell(x) => (16013, x.index()),
        EntityRef::Face(x) => (16002, x.index()),
        EntityRef::Faceuse(x) => (16022, x.index()),
        EntityRef::Loop(x) => (16003, x.index()),
        EntityRef::Loopuse(x) => (16004, x.index()),
        EntityRef::Edge(x) => (16016, x.index()),
        EntityRef::Edgeuse(x) => (16005, x.index()),
        EntityRef::Vertex(x) => (16021, x.index()),
        EntityRef::Vertexuse(x) => (16006, x.index()),
    };
    notify_key(t, i)
}
fn own_face_owner(f: FaceId) -> ObjectRef {
    key(EntityRef::Face(f))
}
fn own_edgeuse_owner(eu: EdgeuseId) -> ObjectRef {
    key(EntityRef::Edgeuse(eu))
}
fn own_edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    (
        key(EntityRef::Edge(e)),
        key(EntityRef::Brep(solidcore::topology::BrepId::from_index(0))),
    )
}
/// The shim's build conventions (`sc_edgeuse_graph.h`), geometry owned under [`key`].
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    prefix_lists: false,
    vertex_edge_gaps: false,
    vertex_lists_last: false,
    skip_edge_list: false,
    face_owner: &own_face_owner,
    edge_owner: &own_edge_owner,
    edgeuse_owner: &own_edgeuse_owner,
    // the runner records `Notify` itself, after the build
    naming: NamingScheme::Unknown,
};
fn block(src: &Src) -> Result<(String, Vec<f64>), HarnessError> {
    Ok(match src {
        Src::Prim(p) => (
            String::new(),
            match p {
                Prim::Sphere { origin, radius } => {
                    vec![200.0, origin[0], origin[1], origin[2], *radius]
                }
                Prim::Cylinder {
                    origin,
                    radius,
                    height,
                } => vec![201.0, origin[0], origin[1], origin[2], *radius, *height],
                Prim::Cone {
                    origin,
                    radius_base,
                    radius_top,
                    height,
                } => vec![
                    202.0,
                    origin[0],
                    origin[1],
                    origin[2],
                    *radius_base,
                    *radius_top,
                    *height,
                ],
                Prim::Torus {
                    origin,
                    major,
                    minor,
                } => vec![203.0, origin[0], origin[1], origin[2], *major, *minor],
            },
        ),
        Src::Brep(b) => (path_of(Some(b))?, brep_block_of(Some(b), false, false)?),
    })
}
fn args(i: &Inputs) -> [f64; 5] {
    [
        f64::from(i.level),
        f64::from(i.walk),
        f64::from(if i.copy && i.copy_op != 0 {
            i.copy_op
        } else {
            i.mutation
        }),
        i.index as f64,
        i.value,
    ]
}
/// The Rust B-rep of the source in the shim's traversal order, or the output of a
/// failed build.
fn own_brep(k: &mut Kernel, src: &Src) -> Result<Result<Brep, Output>, HarnessError> {
    use crate::own_builder::{build_any, finish, Built};
    let built = match src {
        Src::Brep(b) => build_any(k, b, false, false, &CONVENTIONS)?,
        Src::Prim(p) => {
            use solidcore::primitive::api_primitives as ap;
            let raw = match p {
                Prim::Sphere { origin, radius } => {
                    ap::create_sphere(k, Vec3::from_array(*origin), *radius)
                }
                Prim::Cylinder {
                    origin,
                    radius,
                    height,
                } => ap::create_cylinder(k, Vec3::from_array(*origin), *radius, *height),
                Prim::Cone {
                    origin,
                    radius_base,
                    radius_top,
                    height,
                } => ap::create_cone(
                    k,
                    Vec3::from_array(*origin),
                    *radius_base,
                    *radius_top,
                    *height,
                ),
                Prim::Torus {
                    origin,
                    major,
                    minor,
                } => ap::create_torus(k, Vec3::from_array(*origin), *major, *minor),
            };
            finish(k, raw, false, false, &CONVENTIONS)?
        }
    };
    Ok(match built {
        Built::Brep(b) => {
            let mut b = *b;
            b.set_naming(NamingScheme::Notify);
            // the copy keeps each UV trim curve's owner from the B-rep it was built in;
            // upstream's `SmEdgeuse::AssertValid` test 6 expects the edge use holding it
            // (`GetOwner() == this`), here under the same identities as the walk's
            let uses: Vec<_> = b.edgeuses().ids().collect();
            for eu in uses {
                let edgeuse = b.edgeuse_mut(eu).map_err(|e| herr(e.to_string()))?;
                if let Some(trim) = edgeuse.uv_trim_curve.as_mut() {
                    let _ = trim.set_owner(Some(key(EntityRef::Edgeuse(eu))), None);
                }
            }
            Ok(b)
        }
        Built::Failed(e) => Err(Output::unported(&e)
            .unwrap_or_else(|| Output::rejected("native fixture construction failed"))),
    })
}
fn orient(v: f64) -> solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    match v as u8 {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}
/// The shim's `Mutation` (kinds 1-10 of `topology_brep_validation`).
fn mutate(b: &mut Brep, i: &Inputs) -> Result<(), HarnessError> {
    let n = match i.mutation {
        1 | 6 => b.vertices().len(),
        2 | 7 | 9 => b.edges().len(),
        3 | 8 => b.faces().len(),
        4 => b.faceuses().len(),
        5 => b.loopuses().len(),
        10 => b.edgeuses().len(),
        _ => 0,
    };
    if n == 0 {
        return Ok(());
    }
    let index = (i.index % n) as u32;
    let e = |e: solidcore::SolidError| herr(e.to_string());
    match i.mutation {
        1 => b.vertex_mut(VertexId::from_index(index)).map_err(e)?.tolerance = i.value,
        2 => b.edge_mut(EdgeId::from_index(index)).map_err(e)?.tolerance = i.value,
        3 => {
            let f = FaceId::from_index(index);
            b.face_mut(f).map_err(e)?.tolerance = i.value;
            b.sync_face_surface_tolerance(f).map_err(e)?;
        }
        4 => {
            b.faceuse_mut(FaceuseId::from_index(index))
                .map_err(e)?
                .orientation = orient(i.value);
        }
        5 => {
            b.loopuse_mut(LoopuseId::from_index(index))
                .map_err(e)?
                .orientation = orient(i.value);
        }
        6 => b.vertex_mut(VertexId::from_index(index)).map_err(e)?.point.x += i.value,
        7 => {
            if let Some(c) = b.edge_mut(EdgeId::from_index(index)).map_err(e)?.curve.as_mut() {
                c.set_owner(None, None);
            }
        }
        8 => b.face_mut(FaceId::from_index(index)).map_err(e)?.surface = None,
        9 => b.edge_mut(EdgeId::from_index(index)).map_err(e)?.curve = None,
        10 => b.edgeuse_mut(EdgeuseId::from_index(index)).map_err(e)?.ccw = None,
        _ => {}
    }
    Ok(())
}
fn text(out: &mut Vec<f64>, s: &str) {
    out.push(s.len() as f64);
    out.extend(s.bytes().map(f64::from));
}
fn report_key(out: &mut Vec<f64>, k: Option<ReportKey>) {
    if let Some(k) = k {
        out.extend([(k.0 >> 32) as f64, (k.0 & 0xffff_ffff) as f64]);
    } else {
        out.extend([-1.0, -1.0]);
    }
}
/// The shim's `put_reports` of a Rust report list.
fn put_reports(out: &mut Vec<f64>, reports: &AssertArray) {
    out.push(reports.len() as f64);
    for r in reports.reports() {
        out.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            f64::from(r.ok),
        ]);
        report_key(out, r.reporter);
        report_key(out, r.owner);
        report_key(out, r.other);
        out.extend([r.owner_type as f64, r.other_type as f64, r.tol3d, r.value]);
        text(out, r.name);
        text(out, r.message.as_deref().unwrap_or(""));
        text(out, r.message_for_file.unwrap_or(""));
        text(out, r.owner_type_string.unwrap_or(""));
        text(out, r.other_type_string.unwrap_or(""));
        text(out, &r.format_log_message());
        text(out, obj_type_string(r.reporting_type));
    }
    out.push(f64::from(reports.last_add()));
}
struct Reader<'a> {
    data: &'a [f64],
    pos: usize,
}
impl Reader<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = *self
            .data
            .get(self.pos)
            .ok_or_else(|| herr("short AssertValid protocol"))?;
        self.pos += 1;
        Ok(x)
    }
    fn count(&mut self) -> Result<usize, HarnessError> {
        let x = self.num()?;
        if !x.is_finite() || x < 0.0 || x.fract() != 0.0 || x >= 1e9 {
            return Err(herr("bad AssertValid count"));
        }
        Ok(x as usize)
    }
    fn slice(&mut self, n: usize) -> Result<&[f64], HarnessError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| herr("protocol overflow"))?;
        let x = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| herr("short AssertValid protocol"))?;
        self.pos = end;
        Ok(x)
    }
    fn text(&mut self) -> Result<String, HarnessError> {
        let n = self.count()?;
        let bytes: Vec<u8> = self.slice(n)?.iter().map(|&x| x as u8).collect();
        String::from_utf8(bytes).map_err(|_| herr("bad report text"))
    }
}
/// One decoded report: its integer fields, values and texts.
struct Rec {
    ints: Vec<i64>,
    values: [f64; 2],
    texts: Vec<String>,
}
fn read_reports(rd: &mut Reader<'_>) -> Result<(Vec<Rec>, i64), HarnessError> {
    let n = rd.count()?;
    let mut out = Vec::new();
    for _ in 0..n {
        let ints = rd.slice(13)?.iter().map(|&x| x as i64).collect();
        let v = rd.slice(2)?;
        let values = [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0)];
        let mut texts = Vec::new();
        for _ in 0..7 {
            texts.push(rd.text()?);
        }
        out.push(Rec {
            ints,
            values,
            texts,
        });
    }
    let last_add = rd.num()? as i64;
    Ok((out, last_add))
}
/// The capi line of a decoded report.
fn line(r: &Rec) -> String {
    let test = r.ints.get(2).copied().unwrap_or(-1);
    let text = |i: usize| r.texts.get(i).map_or("", String::as_str);
    format!(
        "{}::Test #{} {}: {} [owner {}]",
        text(6),
        test,
        text(0),
        text(5),
        text(3)
    )
}
fn put(o: Output, prefix: &str, reports: &[Rec], last_add: i64) -> Output {
    let mut ints = vec![reports.len() as i64];
    let mut values = Vec::new();
    let mut texts = Vec::new();
    for r in reports {
        ints.extend(&r.ints);
        values.extend(r.values);
        for t in &r.texts {
            texts.push(t.len() as i64);
            texts.extend(t.bytes().map(i64::from));
        }
    }
    ints.push(last_add);
    o.ints(&format!("{prefix}reports"), ints)
        .values(&format!("{prefix}report_values"), values)
        .ints(&format!("{prefix}report_texts"), texts)
}
fn bytes(s: &str) -> Vec<i64> {
    s.bytes().map(i64::from).collect()
}
fn output(protocol: &[f64], capi: bool) -> Result<Output, HarnessError> {
    let mut rd = Reader {
        data: protocol,
        pos: 0,
    };
    if rd.num()? != 0.0 {
        return Ok(Output::rejected("native fixture construction failed"));
    }
    let o = if capi {
        let status = rd.num()? as i64;
        let (pointers, pointers_last) = read_reports(&mut rd)?;
        let assert = rd.num()? as i64;
        let (asserts, asserts_last) = read_reports(&mut rd)?;
        // the capi lines (smlib_brep_check)
        let mut lines = Vec::new();
        let pointer_failures = pointers.iter().any(|r| r.ints.get(4) == Some(&0));
        if status != 1000 && pointers.is_empty() {
            lines.push(format!("SmBrep::ValidatePointers failed with status {status}"));
        }
        lines.extend(pointers.iter().filter(|r| r.ints.get(4) == Some(&0)).map(line));
        if pointer_failures {
            lines.push("AssertValid skipped: ValidatePointers failed".to_string());
        } else {
            if assert == 0 && asserts.is_empty() {
                lines.push("SmBrep::AssertValid failed without a report".to_string());
            }
            lines.extend(asserts.iter().filter(|r| r.ints.get(4) == Some(&0)).map(line));
        }
        let o = Output::new()
            .int("pointers_status", status)
            .int("assert_valid", assert)
            .int("failure_count", lines.len() as i64)
            .ints("lines", bytes(&lines.join("\n")));
        let o = put(o, "pointer_", &pointers, pointers_last);
        put(o, "", &asserts, asserts_last)
    } else {
        let rtn = rd.num()? as i64;
        let (reports, last) = read_reports(&mut rd)?;
        put(Output::new().int("assert_valid", rtn), "", &reports, last)
    };
    if rd.pos != protocol.len() {
        return Err(herr("trailing AssertValid output"));
    }
    Ok(o)
}
/// The level-2 test results (shim op 2): per face tests 32, 33, 38 (`-1` without
/// surface), per edge test 13 and its max gap, per vertex test 1, its max gap and the
/// face of that gap (index, `-1` for none).
fn tests_output(protocol: &[f64]) -> Result<Output, HarnessError> {
    let mut rd = Reader {
        data: protocol,
        pos: 0,
    };
    if rd.num()? != 0.0 {
        return Ok(Output::rejected("native fixture construction failed"));
    }
    let (nf, ne, nv) = (rd.count()?, rd.count()?, rd.count()?);
    let faces: Vec<i64> = rd.slice(3 * nf)?.iter().map(|&x| x as i64).collect();
    let edges = rd.slice(2 * ne)?.to_vec();
    let vertices = rd.slice(3 * nv)?.to_vec();
    if rd.pos != protocol.len() {
        return Err(herr("trailing level-2 test output"));
    }
    let flags = |v: &[f64]| v.iter().step_by(2).map(|&x| x as i64).collect::<Vec<_>>();
    let gaps = |v: &[f64]| v.iter().skip(1).step_by(2).copied().collect::<Vec<_>>();
    Ok(Output::new()
        .ints("face_tests_32_33_38", faces)
        .ints("edge_test13", flags(&edges))
        .values("edge_test13_max_gap", gaps(&edges))
        .ints("vertex_test1", vertices.iter().step_by(3).map(|&x| x as i64).collect::<Vec<_>>())
        .values("vertex_test1_max_gap_3d", vertices.iter().skip(1).step_by(3).copied().collect::<Vec<_>>())
        .ints("vertex_test1_max_gap_face", vertices.iter().skip(2).step_by(3).map(|&x| x as i64).collect::<Vec<_>>()))
}

/// The Rust side of shim op 2, in its order (faces, then edges, then vertices: the UV
/// trim curves test 38 builds stay for the later tests on both sides).
fn level2_tests(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<Vec<f64>> {
    use solidcore::topology::face::assert_tests as fa;
    let faces: Vec<FaceId> = b.faces().ids().collect();
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    let vertices: Vec<VertexId> = b.vertices().ids().collect();
    let mut out = vec![0.0, faces.len() as f64, edges.len() as f64, vertices.len() as f64];
    let flag = |v: bool| if v { 1.0 } else { 0.0 };
    for f in faces {
        out.push(flag(fa::assert_test_face32(b, f)?));
        out.push(flag(fa::assert_test_face33(k, b, f)?));
        out.push(if b.face(f)?.surface.is_some() {
            flag(fa::assert_test_face38(k, b, f)?)
        } else {
            -1.0
        });
    }
    for e in edges {
        let (ok, gap) = solidcore::topology::edge::edge_form::assert_test_edge13(k, b, e)?;
        out.push(flag(ok));
        out.push(gap);
    }
    for v in vertices {
        let (ok, gap, face) = solidcore::topology::vertex::corner::assert_test_vertex1(k, b, v)?;
        out.push(flag(ok));
        out.push(gap);
        out.push(face.map_or(-1.0, |f| f64::from(f.index())));
    }
    Ok(out)
}

/// Why op 2 refuses mutations 8-10 (null face surface, edge curve, edge use link): the
/// level-2 tests dereference them unguarded in C++.
/// The capi check level of a case (levels 0 to 2).
fn capi_level(i: &Inputs) -> Result<validation::CheckLevel, HarnessError> {
    Ok(match i.level {
        0 => validation::CheckLevel::Level0,
        1 => validation::CheckLevel::Level1,
        2 => validation::CheckLevel::Level2,
        _ => return Err(herr("the capi check takes levels 0 to 2")),
    })
}
/// The shim's capi protocol of a Rust `check_brep` result, decoded as [`output`].
fn capi_output(check: solidcore::Result<validation::BrepCheck>) -> Result<Output, HarnessError> {
    let check = match check {
        Ok(c) => c,
        Err(e) => return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
    };
    let mut out = vec![0.0];
    out.push(f64::from(check.pointers_status));
    put_reports(&mut out, &check.pointer_reports);
    out.push(match check.assert_valid {
        None => -1.0,
        Some(v) => f64::from(v),
    });
    put_reports(&mut out, &check.reports);
    let o = output(&out, true)?;
    // the API's own lines must be the capi lines decoded above
    let lines = check.lines().join("\n");
    if o.ints.get("lines").map(Vec::as_slice) != Some(bytes(&lines).as_slice()) {
        return Ok(o.int("api_lines_differ", 1));
    }
    Ok(o)
}
/// The outcome of a copy case's capi check: statuses, the failure lines and the report
/// counts, without the reports' identities (the copy's are its caller keys).
fn copy_summary(o: Output) -> Output {
    let mut s = Output::new();
    s.error = o.error;
    s.deferred = o.deferred;
    s.rejected = o.rejected;
    for name in ["pointers_status", "assert_valid", "failure_count", "lines", "api_lines_differ"] {
        if let Some(v) = o.ints.get(name) {
            s = s.ints(name, v.clone());
        }
    }
    for name in ["pointer_reports", "reports"] {
        if let Some(&n) = o.ints.get(name).and_then(|v| v.first()) {
            s = s.int(&format!("{name}_count"), n);
        }
    }
    s
}
/// A copy case on the Rust side: `api_brep_copy` of `src` (named by the shim's [`key`])
/// under caller keys, then [`validation::check_brep`] by the copy's naming.
fn copy_check(k: &mut Kernel, src: &Brep, i: &Inputs) -> Result<Output, HarnessError> {
    if i.mutation != 0 || i.tests {
        return Err(herr("a copy case takes no mutation or level-2 test run"));
    }
    let level = capi_level(i)?;
    let copy_key = |e: EntityRef| {
        let r = key(e);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(r.key.0 | (1 << 40)),
        }
    };
    let mut state = solidcore::topology::brep::p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &copy_key;
    let copy =
        match solidcore::topology::brep::p3::api_brep_copy(k, &mut env, Some(src), &key) {
            Ok(c) => c,
            Err(e) => {
                return Ok(copy_summary(
                    Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())),
                ))
            }
        };
    let mut result = match copy_operation(k, copy, i) {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            return Ok(copy_summary(
                Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())),
            ))
        }
        Err(e) => return Err(e),
    };
    capi_output(validation::check_brep(k, &mut result, level)).map(copy_summary)
}
/// The operation of a copy case on the copy (`copy_operation` of the shim, through the
/// Rust API): the copy itself without one; the operation's error inside.
fn copy_operation(
    k: &mut Kernel,
    mut copy: Brep,
    i: &Inputs,
) -> Result<solidcore::Result<Brep>, HarnessError> {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::create_box;
    let v = i.value;
    Ok(match i.copy_op {
        0 => Ok(copy),
        1 | 2 => (|| {
            let mut other = create_box(k, Vec3::new(v, v, v), 10.0, 10.0, 10.0)?;
            if i.copy_op == 2 {
                let other_key = |e: EntityRef| {
                    let r = key(e);
                    ObjectRef {
                        type_code: r.type_code,
                        key: ObjectKey(r.key.0 | (2 << 40)),
                    }
                };
                let mut state =
                    solidcore::topology::brep::p2::ArenaNotifyState::new(&Brep::new());
                let mut env = state.env(None);
                env.key_of = &other_key;
                other = solidcore::topology::brep::p3::api_brep_copy(k, &mut env, Some(&other), &key)?;
            }
            boolean_union(k, copy, other)
        })(),
        3 => (|| {
            let edges = copy.edge_list().to_vec();
            let edge = *edges
                .get(i.index % edges.len().max(1))
                .ok_or_else(|| solidcore::SolidError::invalid_input("difftest", "no edge"))?;
            solidcore::blend::fillet_executive::api::fillet_edges(k, &mut copy, &[edge], v, 1, 1, 1.0)?;
            Ok(copy)
        })(),
        4 => shell_copy(k, copy, i),
        _ => return Err(herr("copy_op takes 0 to 4")),
    })
}
/// `SmApiShellBrepFull` of a copy case: the copy shelled by `value` (extended convex
/// edges, merged results, no offset solid) with face `index` of its face list open.
fn shell_copy(k: &mut Kernel, mut copy: Brep, i: &Inputs) -> solidcore::Result<Brep> {
    use super::offset_offset_executive_p2 as off;
    use solidcore::geom::attribute::AttributeStore;
    use solidcore::offset::offset_executive::p1::OffsetExecutive;
    use solidcore::offset::offset_executive::p2::{OffsetNotifications, OffsetOptions, ShellOptions};
    let faces = copy.face_list().to_vec();
    let open = *faces
        .get(i.index % faces.len().max(1))
        .ok_or_else(|| solidcore::SolidError::invalid_input("difftest", "no face"))?;
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    off::register_notification_brep(&mut records, &mut copy, off::NAMESPACE_SOURCE);
    let result_brep =
        off::result_notification_key(EntityRef::Brep(solidcore::topology::BrepId::from_index(0)));
    records.insert(result_brep.key, solidcore::topology::topology::Topology::new());
    let mut notifications = OffsetNotifications {
        records: &mut records,
        attributes: &mut attributes,
        original_key_of: &off::original_notification_key,
        result_key_of: &off::result_notification_key,
        original_curve_ref: &off::original_curve_key,
        result_curve_ref: &off::result_curve_key,
        boolean_source_lists: &off::boolean_source_lists,
        result_brep,
        merge_identities: Some(&off::MergeBodies),
    };
    OffsetExecutive::shell_brep(
        k,
        copy,
        ShellOptions {
            offset: OffsetOptions {
                distance: i.value,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: false,
            merge_sheet_side_walls: false,
        },
        &[open],
        &mut notifications,
    )
    .result
    .map(|mut b| {
        // as `api_shell_brep_full` does for its caller: the port's cached topology fields
        b.refresh_cached_topology();
        b
    })
}
const NULL_LINK_TESTS: &str = "the level-2 tests dereference the nulled link (crash in C++)";

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.assert_valid"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        if i.tests && i.mutation >= 8 {
            return Ok(Output::rejected(NULL_LINK_TESTS));
        }
        let (path, block) = block(&i.brep)?;
        let op = if i.tests {
            2
        } else if i.copy && i.copy_op != 0 {
            4
        } else if i.copy {
            3
        } else {
            u8::from(i.capi)
        };
        let out = ffi::eval(op, &path, &block, args(&i)).map_err(|e| herr(e.to_string()))?;
        if i.tests {
            return tests_output(&out);
        }
        if i.copy {
            return output(&out, true).map(copy_summary);
        }
        output(&out, i.capi)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        let mut b = match own_brep(&mut k, &i.brep)? {
            Ok(b) => b,
            Err(o) => return Ok(o),
        };
        if i.copy {
            return copy_check(&mut k, &b, &i);
        }
        mutate(&mut b, &i)?;
        if i.tests {
            return match level2_tests(&mut k, &mut b) {
                Ok(out) => tests_output(&out),
                Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
            };
        }
        if i.capi {
            let level = capi_level(&i)?;
            return capi_output(validation::check_brep_with_keys(&mut k, &mut b, level, &key));
        }
        let mut out = vec![0.0];
        let mut reports = AssertArray::new();
        let rtn = match validation::assert_valid(
            &mut k,
            &mut b,
            Some(&mut reports),
            i.level,
            i.walk,
            &key,
        ) {
            Ok(v) => v,
            Err(e) => return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        };
        out.push(f64::from(rtn));
        put_reports(&mut out, &reports);
        output(&out, false)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let o = |rng: &mut Rng| rng.point3(-5.0, 5.0);
        let brep = match rng.below(9) {
            0 => Src::Brep(BrepIn::Box {
                origin: o(rng),
                size: [
                    0.5 + rng.unit() * 4.0,
                    0.5 + rng.unit() * 4.0,
                    0.5 + rng.unit() * 4.0,
                ],
            }),
            1 => Src::Brep(BrepIn::Network),
            2 => Src::Brep(BrepIn::Vertices {
                points: (0..=rng.below(6)).map(|_| rng.point3(-2.0, 2.0)).collect(),
            }),
            3 => Src::Brep(BrepIn::Points {
                vertices: vec![[0.0, 0.0, 0.0, 1e-6], [0.5, 0.0, 0.0, 1e-6]],
                lines: vec![[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1e-6]],
            }),
            4 => Src::Prim(Prim::Sphere {
                origin: o(rng),
                radius: 0.5 + rng.unit() * 3.0,
            }),
            5 => Src::Prim(Prim::Cylinder {
                origin: o(rng),
                radius: 0.5 + rng.unit() * 3.0,
                height: 0.5 + rng.unit() * 3.0,
            }),
            6 => Src::Prim(Prim::Cone {
                origin: o(rng),
                radius_base: 0.5 + rng.unit() * 3.0,
                radius_top: rng.unit() * 0.4,
                height: 0.5 + rng.unit() * 3.0,
            }),
            7 => Src::Prim(Prim::Torus {
                origin: o(rng),
                major: 2.0 + rng.unit() * 3.0,
                minor: 0.2 + rng.unit() * 1.5,
            }),
            _ => Src::Brep(BrepIn::File {
                file: ["Solids/box.smb", "sphere.smb", "cone.smb", "disk.smb", "plane_square.smb"]
                    .get(rng.below(5) as usize)?
                    .to_string(),
                rebuild: false,
            }),
        };
        let mutation = rng.below(11) as u8;
        let value = if matches!(mutation, 4 | 5) {
            rng.below(3) as f64
        } else {
            [0.0, 1e-12, 1e-6, 0.1, 1.0, 1e3]
                .get(rng.below(6) as usize)
                .copied()?
        };
        let capi = rng.below(3) == 0;
        serde_json::to_value(Inputs {
            brep,
            level: if capi {
                rng.below(3) as u8
            } else {
                rng.below(5) as u8
            },
            walk: rng.below(4) != 0,
            capi,
            tests: false,
            copy: false,
            copy_op: 0,
            mutation,
            index: rng.below(100) as usize,
            value,
        })
        .ok()
    }
}
