//! Differential checks of B-rep pointer reports and coincidence pairs against the
//! unchanged c9c5979 C++ reference. The Rust side builds a [`BrepIn`] source itself
//! ([`crate::own_builder`]) and reads an `.smb` file with the Rust reader
//! ([`crate::own_reader::brep_in_order`]), both under this runner's conventions. Only
//! the `SmBrepConstructor` sheet grids (`grid_faces`, `shared_boundaries`), whose
//! constructor is not ported, still export their topology, geometry ownership and
//! global list order for the Rust side to import. Post-call curve samples compare the
//! reversal repairs and lazily created UV curves made by the checks themselves.
use super::topology_edgeuse_p1::{brep_block_of, import_raw_with_types, path_of, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::assert_array::{AssertArray, ObjectKey as ReportKey};
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::validation;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopuseId, NamingScheme, VertexId,
};
use solidcore::{EntityRef, Kernel};
use solidcore_ffi::units::topology_brep_validation as ffi;

/// Native B-rep validation runner.
pub struct Runner;
#[derive(Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default = "empty_brep")]
    brep: BrepIn,
    #[serde(default)]
    grid_faces: Option<usize>,
    #[serde(default)]
    shared_boundaries: Option<u8>,
    #[serde(default)]
    coincidence: bool,
    #[serde(default = "level_two")]
    level: u8,
    #[serde(default = "yes")]
    walk: bool,
    #[serde(default = "default_tol")]
    tolerance: f64,
    #[serde(default = "yes")]
    find_all: bool,
    #[serde(default)]
    mutation: u8,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    value: f64,
}
fn empty_brep() -> BrepIn {
    BrepIn::Vertices { points: Vec::new() }
}
fn yes() -> bool {
    true
}
fn level_two() -> u8 {
    2
}
fn default_tol() -> f64 {
    -1.0
}
fn herr(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn info(e: EntityRef) -> (i64, u32) {
    match e {
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
    }
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
fn key(e: EntityRef) -> ObjectRef {
    let (t, i) = info(e);
    notify_key(t, i)
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
            .ok_or_else(|| herr("short validation protocol"))?;
        self.pos += 1;
        Ok(x)
    }
    fn count(&mut self) -> Result<usize, HarnessError> {
        let x = self.num()?;
        if !x.is_finite() || x < 0.0 || x.fract() != 0.0 || x >= usize::MAX as f64 {
            return Err(herr("bad validation count"));
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
            .ok_or_else(|| herr("short validation protocol"))?;
        self.pos = end;
        Ok(x)
    }
    fn list(&mut self) -> Result<Vec<usize>, HarnessError> {
        let n = self.count()?;
        let values = self.slice(n)?;
        values
            .iter()
            .map(|&x| {
                if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < usize::MAX as f64 {
                    Ok(x as usize)
                } else {
                    Err(herr("bad list index"))
                }
            })
            .collect()
    }
    fn owner(&mut self) -> Result<Option<ObjectRef>, HarnessError> {
        let t = self.num()? as i64;
        let i = self.num()? as i64;
        if t < 0 {
            return Ok(None);
        }
        Ok(Some(notify_key(t, i as u32)))
    }
}
fn args(i: &Inputs) -> [f64; 7] {
    [
        f64::from(i.level),
        f64::from(i.walk),
        i.tolerance,
        f64::from(i.find_all),
        f64::from(i.mutation),
        i.index as f64,
        i.value,
    ]
}
fn exported(i: &Inputs) -> Result<(String, Vec<f64>, Vec<f64>), HarnessError> {
    if let Some(mode) = i.shared_boundaries {
        let block = vec![101.0, f64::from(mode)];
        let export = ffi::eval(0, "", &block, args(i)).map_err(|e| herr(e.to_string()))?;
        return Ok((String::new(), block, export));
    }
    if let Some(count) = i.grid_faces {
        let block = vec![100.0, count as f64];
        let export = ffi::eval(0, "", &block, args(i)).map_err(|e| herr(e.to_string()))?;
        return Ok((String::new(), block, export));
    }
    let path = path_of(Some(&i.brep))?;
    let block = brep_block_of(Some(&i.brep), false, false)?;
    let export = ffi::eval(0, &path, &block, args(i)).map_err(|e| herr(e.to_string()))?;
    Ok((path, block, export))
}
fn imported(export: &[f64]) -> Result<Brep, HarnessError> {
    let mut rd = Reader {
        data: export,
        pos: 0,
    };
    if rd.num()? != 0.0 {
        return Err(herr("native fixture failed"));
    }
    let n = rd.count()?;
    let graph = rd.slice(n)?;
    let (mut b, types) = import_raw_with_types(graph)?;
    if (0..b.edges().len()).any(|e| types.inexact_curve(e).is_some())
        || (0..b.faces().len()).any(|f| types.inexact_surface(f).is_some())
    {
        return Err(herr("validation fixture needs exact geometry import"));
    }
    b.set_face_list(
        rd.list()?
            .into_iter()
            .map(|i| FaceId::from_index(i as u32))
            .collect(),
    )
    .map_err(|e| herr(e.to_string()))?;
    b.set_edge_list(
        rd.list()?
            .into_iter()
            .map(|i| EdgeId::from_index(i as u32))
            .collect(),
    )
    .map_err(|e| herr(e.to_string()))?;
    b.set_vertex_list(
        rd.list()?
            .into_iter()
            .map(|i| VertexId::from_index(i as u32))
            .collect(),
    )
    .map_err(|e| herr(e.to_string()))?;
    for index in 0..b.faces().len() {
        let f = FaceId::from_index(index as u32);
        let fu = rd.num()? as i64;
        b.face_mut(f).map_err(|e| herr(e.to_string()))?.faceuse =
            u32::try_from(fu).ok().map(FaceuseId::from_index);
        let owner = rd.owner()?;
        if let Some(s) = b
            .face_mut(f)
            .map_err(|e| herr(e.to_string()))?
            .surface
            .as_mut()
        {
            s.set_owner(owner);
        }
        b.sync_face_surface_tolerance(f)
            .map_err(|e| herr(e.to_string()))?;
    }
    for index in 0..b.edges().len() {
        let owner = rd.owner()?;
        if let Some(c) = b
            .edge_mut(EdgeId::from_index(index as u32))
            .map_err(|e| herr(e.to_string()))?
            .curve
            .as_mut()
        {
            c.set_owner(owner, None);
        }
    }
    b.set_naming(NamingScheme::Notify);
    if rd.pos != export.len() {
        return Err(herr("trailing export metadata"));
    }
    Ok(b)
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
/// The shim's build conventions (`sc_edgeuse_graph.h`), with the geometry owned under
/// this runner's notification identities ([`key`]), as [`imported`] names it.
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    prefix_lists: false,
    vertex_edge_gaps: false,
    vertex_lists_last: false,
    skip_edge_list: false,
    face_owner: &own_face_owner,
    edge_owner: &own_edge_owner,
    edgeuse_owner: &own_edgeuse_owner,
};
/// The Rust B-rep of a [`BrepIn`] source built by the Rust kernel
/// ([`crate::own_builder`]) or, for an `.smb` file, read by the Rust reader and
/// renumbered into the shim's traversal order ([`crate::own_reader::brep_in_order`]):
/// native lists, face uses and geometry owners under [`CONVENTIONS`], `Notify` naming as
/// [`imported`] sets it. A failed build is the shim's failed construction; a failed
/// read is a Rust divergence, reported as an error. `None` for the sources still
/// imported from the shim's export: the `SmBrepConstructor` sheet grids (`grid_faces`,
/// `shared_boundaries`), whose constructor is not ported.
fn own_brep(i: &Inputs) -> Result<Option<Result<Brep, Output>>, HarnessError> {
    use crate::own_builder::Built;
    if i.grid_faces.is_some() || i.shared_boundaries.is_some() {
        return Ok(None);
    }
    let file = matches!(i.brep, BrepIn::File { .. });
    let mut k = Kernel::default();
    Ok(Some(
        match crate::own_reader::brep_in_order(&mut k, &i.brep, false, false, &CONVENTIONS)? {
            Built::Brep(b) => {
                let mut b = *b;
                b.set_naming(NamingScheme::Notify);
                Ok(b)
            }
            Built::Failed(e) => Err(Output::unported(&e).unwrap_or_else(|| {
                if file {
                    Output::err(format!("rust read failed: {e}"))
                } else {
                    Output::rejected("native fixture construction failed")
                }
            })),
        },
    ))
}
fn orient(v: f64) -> solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    match v as u8 {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}
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
    match i.mutation {
        1 => {
            b.vertex_mut(VertexId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .tolerance = i.value;
        }
        2 => {
            b.edge_mut(EdgeId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .tolerance = i.value;
        }
        3 => {
            let f = FaceId::from_index(index);
            b.face_mut(f).map_err(|e| herr(e.to_string()))?.tolerance = i.value;
            b.sync_face_surface_tolerance(f)
                .map_err(|e| herr(e.to_string()))?;
        }
        4 => {
            b.faceuse_mut(FaceuseId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .orientation = orient(i.value);
        }
        5 => {
            b.loopuse_mut(LoopuseId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .orientation = orient(i.value);
        }
        6 => {
            b.vertex_mut(VertexId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .point
                .x += i.value;
        }
        7 => {
            if let Some(c) = b
                .edge_mut(EdgeId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .curve
                .as_mut()
            {
                c.set_owner(None, None);
            }
        }
        8 => {
            b.face_mut(FaceId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .surface = None;
        }
        9 => {
            b.edge_mut(EdgeId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .curve = None;
        }
        10 => {
            b.edgeuse_mut(EdgeuseId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .ccw = None;
        }
        _ => {}
    }
    Ok(())
}
fn text(out: &mut Vec<f64>, s: Option<&str>) {
    out.push(s.map_or(0, str::len) as f64);
    if let Some(s) = s {
        out.extend(s.bytes().map(f64::from));
    }
}
fn report_key(out: &mut Vec<f64>, k: Option<ReportKey>) {
    if let Some(k) = k {
        out.extend([(k.0 >> 32) as f64, (k.0 & 0xffff_ffff) as f64]);
    } else {
        out.extend([-1.0, -1.0]);
    }
}
fn rust_protocol(
    b: &Brep,
    status: i32,
    pairs: &[(EntityRef, EntityRef)],
    reports: &AssertArray,
) -> Result<Vec<f64>, HarnessError> {
    let mut out = vec![0.0, f64::from(status), pairs.len() as f64];
    for &(a, c) in pairs {
        let (t, i) = info(a);
        let (u, j) = info(c);
        out.extend([t as f64, f64::from(i), u as f64, f64::from(j)]);
    }
    out.push(reports.len() as f64);
    for r in reports.reports() {
        out.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            f64::from(r.ok),
        ]);
        report_key(&mut out, r.reporter);
        report_key(&mut out, r.owner);
        report_key(&mut out, r.other);
        out.extend([r.owner_type as f64, r.other_type as f64, r.tol3d, r.value]);
        text(&mut out, Some(r.name));
        text(&mut out, r.message.as_deref());
        text(&mut out, r.message_for_file);
        text(&mut out, r.owner_type_string);
        text(&mut out, r.other_type_string);
    }
    out.push(f64::from(reports.last_add()));
    out.push((b.edges().len() + b.edgeuses().len()) as f64);
    let kernel = Kernel::default();
    for curve in b
        .edges()
        .iter()
        .map(|(_, e)| e.curve.as_ref())
        .chain(b.edgeuses().iter().map(|(_, eu)| eu.uv_trim_curve.as_ref()))
    {
        let Some(curve) = curve else {
            out.push(-1.0);
            continue;
        };
        out.push(curve.type_code() as f64);
        let domain = curve.natural_interval();
        out.extend([domain.min(), domain.max()]);
        for i in 0..5 {
            match curve.evaluate_point(&kernel, domain.evaluate(f64::from(i) / 4.0)) {
                Ok(p) => out.extend([1000.0, p.x, p.y, p.z]),
                Err(e) if e.is_unported() => return Err(herr(e.to_string())),
                Err(_) => out.push(1001.0),
            }
        }
    }
    Ok(out)
}
fn output(protocol: &[f64]) -> Result<Output, HarnessError> {
    let mut rd = Reader {
        data: protocol,
        pos: 0,
    };
    if rd.num()? != 0.0 {
        return Ok(Output::rejected("native fixture construction failed"));
    }
    let status = rd.num()? as i64;
    let n = rd.count()?;
    let pairs: Vec<i64> = rd
        .slice(n.checked_mul(4).ok_or_else(|| herr("pair overflow"))?)?
        .iter()
        .map(|&x| x as i64)
        .collect();
    let n = rd.count()?;
    let mut ints = vec![n as i64];
    let mut numbers = Vec::new();
    for _ in 0..n {
        ints.extend(rd.slice(13)?.iter().map(|&x| x as i64));
        numbers.extend_from_slice(rd.slice(2)?);
        for _ in 0..5 {
            let n = rd.count()?;
            ints.push(n as i64);
            ints.extend(rd.slice(n)?.iter().map(|&x| x as i64));
        }
    }
    ints.push(rd.num()? as i64);
    let n = rd.count()?;
    let mut geometry = Vec::new();
    let mut values = Vec::new();
    geometry.push(n as i64);
    for _ in 0..n {
        let ty = rd.num()? as i64;
        geometry.push(ty);
        if ty < 0 {
            continue;
        }
        values.extend_from_slice(rd.slice(2)?);
        for _ in 0..5 {
            let s = rd.num()? as i64;
            geometry.push(s);
            if s == 1000 {
                values.extend_from_slice(rd.slice(3)?);
            }
        }
    }
    if rd.pos != protocol.len() {
        return Err(herr("trailing validation output"));
    }
    Ok(Output::new()
        .int("status", status)
        .ints("pairs", pairs)
        .ints("reports", ints)
        .values("report_values", numbers)
        .ints("geometry", geometry)
        .values("curve_samples", values))
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_validation"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (path, block, _) = exported(&i)?;
        output(
            &ffi::eval(if i.coincidence { 2 } else { 1 }, &path, &block, args(&i))
                .map_err(|e| herr(e.to_string()))?,
        )
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut b = match own_brep(&i)? {
            Some(Ok(b)) => b,
            Some(Err(o)) => return Ok(o),
            None => {
                let (_, _, export) = exported(&i)?;
                if export.first() != Some(&0.0) {
                    return Ok(Output::rejected("native fixture construction failed"));
                }
                imported(&export)?
            }
        };
        mutate(&mut b, &i)?;
        let kernel = Kernel::default();
        let mut reports = AssertArray::new();
        let run = if i.coincidence {
            validation::contains_coincident_topology(
                &kernel,
                &mut b,
                i.tolerance,
                i.find_all,
                Some(&mut reports),
                i.level,
                &key,
            )
            .map(|r| (i32::from(r.found), r.pairs))
        } else {
            validation::validate_pointers(
                &kernel,
                &mut b,
                Some(&mut reports),
                i.level,
                i.walk,
                &key,
            )
            .map(|s| (s, Vec::new()))
        };
        let (status, pairs) = match run {
            Ok(x) => x,
            Err(e) => return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        };
        output(&rust_protocol(&b, status, &pairs, &reports)?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let coincidence = rng.below(3) == 0;
        let brep = match rng.below(4) {
            0 => BrepIn::Box {
                origin: rng.point3(-10.0, 10.0),
                size: [
                    0.5 + rng.unit() * 4.0,
                    0.5 + rng.unit() * 4.0,
                    0.5 + rng.unit() * 4.0,
                ],
            },
            1 => BrepIn::Network,
            2 => BrepIn::Vertices {
                points: (0..(2 + rng.below(12)))
                    .map(|_| rng.point3(-2.0, 2.0))
                    .collect(),
            },
            _ => BrepIn::Points {
                vertices: vec![[0.0, 0.0, 0.0, 1e-6], [0.5, 0.0, 0.0, 1e-6]],
                lines: vec![[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1e-6]],
            },
        };
        let mutation = if coincidence { 0 } else { rng.below(11) as u8 };
        let value = if matches!(mutation, 4 | 5) {
            rng.below(3) as f64
        } else {
            [0.0, 1e-12, 1e-6, 0.1, 1.0, 1e3]
                .get(rng.below(6) as usize)
                .copied()?
        };
        serde_json::to_value(Inputs {
            brep,
            grid_faces: None,
            shared_boundaries: None,
            coincidence,
            level: rng.below(5) as u8,
            walk: rng.below(2) == 0,
            tolerance: 1e-6,
            find_all: rng.below(2) == 0,
            mutation,
            index: rng.below(100) as usize,
            value,
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_boundaries_reach_positive_edge_and_face_comparisons() {
        for (mode, expected_type) in [(1, 16002), (2, 16016)] {
            let case = Case {
                unit: "topology.brep_validation".into(),
                name: format!("positive_shared_boundaries_{mode}"),
                source: crate::CaseSource::Inline,
                inputs: serde_json::json!({"shared_boundaries": mode, "coincidence": true}),
            };
            let cpp = Runner
                .run_cpp(&case)
                .expect("native shared-boundary fixture");
            let rust = Runner
                .run_rust(&case)
                .expect("Rust shared-boundary fixture");
            for result in [&cpp, &rust] {
                assert!(result.error.is_none());
                assert!(result.rejected.is_none());
                assert!(result.deferred.is_none());
                assert!(result.ints["pairs"]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] == expected_type && p[2] == expected_type));
            }
            assert_eq!(cpp, rust);
        }
    }
}
