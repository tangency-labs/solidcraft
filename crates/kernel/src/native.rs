//! SolidCraft native, versioned binary geometry. No STEP conversion or display meshes.
//! Exact: a decoded body is the encoded one, number for number, for the same producer.
use crate::{Body, KernelError, Result, guard};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::sync::Arc;

mod adapter;
mod binary;
mod metadata;
mod preview;
pub use preview::native_preview_mesh;

// 01/02 were development-only prototypes; independent metadata records start at 03; 04 stores
// procedural intersections and placed revolutions exactly, and the measured runtime values; 05
// nests geometry schema 4 (regions, unbounded geometry, pcurves, canonical order).
const MAGIC: &[u8; 8] = b"SCBREP05";
// A body's own limits. A design's cache holds many bodies within the container's budgets, and
// decoding is linear in the bytes, so one large part (an imported casting) fits.
const MAX_BYTES: usize = 256 << 20;
const MAX_ITEMS: usize = 1_000_000;
const MAX_VALUES: usize = 20_000_000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedBody {
    solid: solidcraft_geom::brep::Brep,
    metadata: solidcraft_geom::brep::envelope::Metadata,
    runtime: Runtime,
}

/// What a body measured about itself that later operations read: kept so a decoded body works
/// to the same tolerances as the one encoded, without measuring again.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    /// [`Body::size`] as stored (0: not measured).
    size: f64,
    /// The orientation mesh's missing-face count at its tolerance.
    coarse_missing: Option<(f64, u64)>,
}
fn invalid(detail: impl std::fmt::Display) -> KernelError {
    KernelError::Invalid(format!("native B-rep: {detail}"))
}

/// Explicit topology, spatial geometry, construction certificates and history, exactly.
/// Failure is a cache miss at the IO boundary, never a failed document save.
pub fn encode_native_body(body: &Body) -> Result<Vec<u8>> {
    guard("encode native B-rep", || {
        for source in body.face_sources.iter().flat_map(|s| s.iter()).flatten() {
            let mut at = source;
            let mut depth = 0usize;
            while let crate::FaceSource::ShellPlanar { source, .. } = at {
                depth += 1;
                if depth > 32 {
                    return Err(invalid("face-source nesting"));
                }
                at = source;
            }
        }
        if body.face_count() > MAX_ITEMS
            || body.solid.edge_iter().take(MAX_ITEMS + 1).count() > MAX_ITEMS
            || body.mesh.as_ref().is_some_and(|m| m.positions.len() > MAX_ITEMS || m.triangles.len() > MAX_ITEMS)
        {
            return Err(invalid("native geometry budget"));
        }
        if body.mesh.is_some() {
            return Err(invalid("mesh bodies are source blobs, not B-rep"));
        }
        let runtime = Runtime { size: body.size, coarse_missing: body.coarse_missing.map(|(tol, n)| (tol, n as u64)) };
        let saved = SavedBody { solid: adapter::encode(body)?, metadata: metadata::encode(body)?, runtime };
        // Structural validation does not invoke another geometric reconstruction.
        validate_topology(&saved)?;
        let value = serde_json::to_value(saved).map_err(invalid)?;
        validate_value(&value, 0)?;
        if value_nodes(&value) > MAX_VALUES {
            return Err(invalid("value budget"));
        }
        let mut out = MAGIC.to_vec();
        binary::write(&value, &mut out, 0)?;
        Ok(out)
    })
}

/// The schema 4 record of a solid body, in canonical order with its pcurves: what another
/// backend (SMLib) builds the body from. Mesh bodies have none.
pub fn brep_record(body: &Body) -> Result<solidcraft_geom::brep::Brep> {
    guard("B-rep record", || {
        if body.mesh.is_some() {
            return Err(invalid("mesh bodies are source blobs, not B-rep"));
        }
        adapter::encode(body)
    })
}

/// The body in SMLib, built from its schema 4 record. The record is trusted (SMLib's healer is
/// skipped): the kernel's records keep SMLib's rules, and healing moved a cylinder's volume by
/// 9e-7 where the record as given is exact to 1e-7.
#[cfg(feature = "smlib")]
pub fn smlib_body(body: &Body) -> Result<solidcraft_smlib::Brep> {
    let record = tolerant_record(body)?;
    solidcraft_smlib::Brep::from_record(&record, true).map_err(|e| KernelError::Failed(format!("SMLib: {e}")))
}

/// [`brep_record`] for another kernel: where an edge lies a little farther from a face it bounds
/// than the body's working tolerance (truck's offsets leave edges some 3e-5 mm off their faces),
/// the record takes a tolerance ten, then a hundred times as large for the body, its edges and
/// vertices, as the kernel it goes to works to tolerances (SMLib's own are far larger). Past
/// that it is refused.
#[cfg(feature = "smlib")]
fn tolerant_record(body: &Body) -> Result<solidcraft_geom::brep::Brep> {
    guard("B-rep record", || {
        if body.mesh.is_some() {
            return Err(invalid("mesh bodies are source blobs, not B-rep"));
        }
        let working = crate::fill::face_tolerance(body);
        let mut last = None;
        for factor in [1.0, 10.0, 100.0] {
            match adapter::encode_within(body, working * factor)? {
                Ok(record) => return Ok(record),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| invalid("no record")))
    })
}

/// The truck solid of a schema 4 record, as a saved body's is built: tests compare records
/// made elsewhere (the STEP reader's) with truck's own import.
#[cfg(test)]
pub(crate) fn solid_of_record(record: solidcraft_geom::brep::Brep) -> Result<truck_modeling::Solid> {
    guard("decode a B-rep record", || adapter::decode(record))
}

/// Decode bounded untrusted data. Validate indices before topology extraction.
/// Only a caller that has checked who produced the data may treat it as evaluated geometry.
pub fn decode_native_body(bytes: &[u8]) -> Result<Body> {
    if bytes.len() > MAX_BYTES || bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err(invalid("version or size"));
    }
    guard("decode native B-rep", || {
        let mut input = bytes.get(8..).ok_or_else(|| invalid("header"))?;
        let mut remaining = MAX_VALUES;
        let value = binary::read(&mut input, 0, &mut remaining)?;
        if !input.is_empty() {
            return Err(invalid("trailing data"));
        }
        validate_value(&value, 0)?;
        let saved: SavedBody = serde_json::from_value(value).map_err(invalid)?;
        validate_topology(&saved)?;
        let (solid, numbering) = adapter::decode_numbered(saved.solid, adapter::Producer::Own)?;
        let Runtime { size, coarse_missing } = saved.runtime;
        if !(size.is_finite() && size >= 0.0) || coarse_missing.is_some_and(|(tol, n)| !tol.is_finite() || n > MAX_ITEMS as u64) {
            return Err(invalid("runtime values"));
        }
        let body = Body {
            solid: Arc::new(solid),
            size,
            volume: Some(std::sync::Arc::new(std::sync::Mutex::new(None))),
            coarse_missing: coarse_missing.map(|(tol, n)| (tol, n as usize)),
            mesh: None,
            color: None,
            paint: None,
            splits: None,
            face_sources: None,
            history: None,
            display_of: None,
            meshes: None,
            phased: false,
            tolerances: None,
            regularization_protected: false,
        };
        // A body made by another kernel keeps the tolerances its record saved for it.
        let tolerances = adapter::decoded_tolerances(&numbering, &body);
        let mut body = body.with_tolerances(tolerances);
        metadata::apply(&mut body, saved.metadata)?;
        Ok(body)
    })
}

/// An SMLib body as a kernel body, through its schema 4 record: SMLib's export mapped by
/// `solidcraft_smlib` (canonical order, radians, pcurves), then this adapter's decode of a
/// foreign record (analytic surfaces as truck's revolutions). The body is oriented as the record
/// says (`Body::oriented`), never by meshing it.
#[cfg(feature = "smlib")]
pub fn body_from_smlib(brep: &solidcraft_smlib::Brep) -> Result<Body> {
    body_from_smlib_ordered(brep).map(|(body, _)| body)
}

/// [`body_from_smlib`], with where each of SMLib's faces and edges is in the body's face and
/// edge order ([`crate::history`]): `None` where the body did not keep the record's order (a
/// rebuild that walked a face's edges the other way).
#[cfg(feature = "smlib")]
pub(crate) fn body_from_smlib_ordered(brep: &solidcraft_smlib::Brep) -> Result<(Body, Option<solidcraft_smlib::history::Order>)> {
    let (record, order) = brep.to_native_ordered().map_err(|e| KernelError::Failed(e.to_string()))?;
    guard("decode SMLib B-rep", || {
        let (solid, numbering) = adapter::decode_numbered(record, adapter::Producer::Foreign)?;
        let body = Body::oriented(solid)?;
        let kept = numbering.kept_by(&body.solid);
        // SMLib's approximated edges lie farther from their faces than the kernel's working
        // tolerance: the body keeps the tolerances the record gave them, so its own record does.
        let tolerances = adapter::decoded_tolerances(&numbering, &body);
        Ok((body.with_tolerances(tolerances), kept.then_some(order)))
    })
}

/// `body` in SMLib ([`smlib_body`], healed where SMLib's check refuses it as given), with where
/// each of SMLib's faces and edges is in the body's face and edge order: SMLib's export of the
/// imported body, checked to number every face, edge and vertex as the body's record does.
/// `None` where it does not.
#[cfg(feature = "smlib")]
pub(crate) fn smlib_body_ordered(body: &Body) -> Result<(solidcraft_smlib::Brep, Option<solidcraft_smlib::history::Order>)> {
    use solidcraft_smlib::{Brep, CheckLevel};
    let record = tolerant_record(body)?;
    let import = |trusted: bool| Brep::from_record(&record, trusted).map_err(|e| KernelError::Failed(format!("SMLib: {e}")));
    let valid = |b: &Brep| b.check(CheckLevel::All).is_ok_and(|c| c.is_valid());
    // A record SMLib's check refuses as given (a gap a little over the record's tolerance, a
    // hole listed before its face's outer loop) goes through SMLib's healer; healed, it is kept
    // only where SMLib then finds it valid.
    let trusted = import(true)?;
    let brep = if valid(&trusted) {
        trusted
    } else {
        match import(false) {
            Ok(healed) if valid(&healed) => healed,
            _ => trusted,
        }
    };
    let order = match brep.to_native_ordered() {
        Ok((back, order)) => same_numbering(&record, &back).then_some(order),
        Err(e) => {
            log::debug!(target: "smlib", "no order for an operand: {e}");
            None
        }
    };
    Ok((brep, order))
}

/// Whether two records of one body number its faces, edges and vertices alike: the same loops of
/// the same edges, the same edge ends, and each vertex where the other's is.
#[cfg(feature = "smlib")]
fn same_numbering(a: &solidcraft_geom::brep::Brep, b: &solidcraft_geom::brep::Brep) -> bool {
    use solidcraft_geom::brep::Loop;
    if a.faces.len() != b.faces.len() || a.edges.len() != b.edges.len() || a.vertices.len() != b.vertices.len() {
        return false;
    }
    let loops_alike = a.faces.iter().zip(&b.faces).all(|(fa, fb)| {
        fa.loops.len() == fb.loops.len()
            && fa.loops.iter().zip(&fb.loops).all(|pair| match pair {
                (Loop::Edges { edges: x }, Loop::Edges { edges: y }) => same_cycle(x, y),
                (Loop::Vertex { vertex: x }, Loop::Vertex { vertex: y }) => x == y,
                _ => false,
            })
    });
    let ends_alike = a.edges.iter().zip(&b.edges).all(|(x, y)| x.vertices == y.vertices);
    let at_same_place = a.vertices.iter().zip(&b.vertices).all(|(x, y)| {
        let d = (0..3).map(|k| (x.point[k] - y.point[k]).powi(2)).sum::<f64>().sqrt();
        d <= x.tolerance + y.tolerance + a.tolerance + b.tolerance
    });
    loops_alike && ends_alike && at_same_place
}

/// Whether two loops walk the same edges: in the same order from some edge on, or in the
/// opposite order with each edge the other way round. SMLib walks a face whose surface it turned
/// (a hole's wall) the other way, starting where it likes; its faces and edges are numbered
/// alike all the same.
#[cfg(feature = "smlib")]
fn same_cycle(x: &[solidcraft_geom::brep::OrientedEdge], y: &[solidcraft_geom::brep::OrientedEdge]) -> bool {
    let n = x.len();
    if n != y.len() {
        return false;
    }
    let Some(first) = x.first() else { return true };
    let at = |k: usize| y.get(k % n);
    (0..n).filter(|k| at(*k).is_some_and(|e| e.edge == first.edge)).any(|k| {
        let ahead = x.iter().enumerate().all(|(i, u)| at(k + i).is_some_and(|w| w.edge == u.edge && w.forward == u.forward));
        let back = x.iter().enumerate().all(|(i, u)| at(k + n - i).is_some_and(|w| w.edge == u.edge && w.forward != u.forward));
        ahead || back
    })
}

fn validate_topology(saved: &SavedBody) -> Result<()> {
    saved.solid.validate().map_err(invalid)?;
    let (faces, edges) = (saved.solid.faces.len(), saved.solid.edges.len());
    let saved = &saved.metadata;
    if saved.version != solidcraft_geom::brep::envelope::VERSION {
        return Err(invalid("metadata version"));
    }
    if saved.face_sources.as_ref().is_some_and(|s| s.len() != faces) {
        return Err(invalid("face source count"));
    }
    if let Some(history) = &saved.history
        && (history.faces.len() != faces
            || history.edges.len() != edges
            || history.operands as usize > MAX_ITEMS
            || history.edges.iter().flat_map(|e| &e.faces).any(|&f| f as usize >= faces))
    {
        return Err(invalid("history topology"));
    }
    if saved.color.is_some_and(|c| c.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))) {
        return Err(invalid("colour"));
    }
    if let Some(paint) = &saved.paint
        && (!paint.opacity.is_finite()
            || !(0.0..=1.0).contains(&paint.opacity)
            || paint.faces.iter().any(|p| {
                p.face as usize >= faces
                    || !p.opacity.is_finite()
                    || !(0.0..=1.0).contains(&p.opacity)
                    || p.color.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            }))
    {
        return Err(invalid("face paint"));
    }
    Ok(())
}

// The binary reader bounds allocation before deserialization. Geometry semantics
// are checked by the independent schema validator, not by backend serde internals.
fn validate_value(v: &Value, depth: usize) -> Result<()> {
    if depth > 48 {
        return Err(invalid("nesting"));
    }
    match v {
        Value::String(s) if s.len() > 1 << 20 => return Err(invalid("string budget")),
        Value::Number(n) if n.is_f64() && n.as_f64().is_none_or(|x| !x.is_finite() || x.abs() > 1e12) => return Err(invalid("non-finite metadata")),
        Value::Array(a) => {
            if a.len() > MAX_ITEMS {
                return Err(invalid("array budget"));
            }
            for x in a {
                validate_value(x, depth + 1)?;
            }
        }
        Value::Object(o) => {
            for x in o.values() {
                validate_value(x, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn value_nodes(v: &Value) -> usize {
    match v {
        Value::Array(a) => a.iter().fold(1usize, |n, v| n.saturating_add(value_nodes(v))),
        Value::Object(o) => o.values().fold(1usize, |n, v| n.saturating_add(value_nodes(v))),
        _ => 1,
    }
}
#[cfg(all(test, feature = "smlib"))]
mod smlib_import_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    fn equal(a: &Body, b: &Body) {
        assert_eq!(a.face_count(), b.face_count());
        assert_eq!(a.face_sources, b.face_sources);
        assert_eq!(a.history, b.history);
        assert_eq!(a.paint, b.paint);
        assert_eq!(a.color, b.color);
        assert_eq!(a.splits, b.splits);
        assert_eq!(a.regularization_protected, b.regularization_protected);
        assert_eq!(encode_native_body(a).unwrap(), encode_native_body(b).unwrap());
        let am = crate::measure(a).unwrap();
        let bm = crate::measure(b).unwrap();
        assert!((am.volume - bm.volume).abs() < 1e-7);
    }
    #[test]
    fn native_solids_keep_geometry_metadata_and_later_modelling() {
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let rounded = crate::fillet(&cube, &[Vec3::new(20., 0., 10.)], 2.).unwrap();
        let bore = crate::cylinder(Vec3::new(10., 10., -1.), Vec3::Z, 3., 12.).unwrap();
        let cut = crate::boolean(&rounded, &bore, crate::BoolOp::Cut).unwrap().unwrap();
        for body in [cube, rounded, cut] {
            let body = body.with_color(Some([0.1, 0.5, 0.9]));
            let bytes = encode_native_body(&body).unwrap();
            assert!(bytes.starts_with(MAGIC));
            let decoded = decode_native_body(&bytes).unwrap();
            equal(&body, &decoded);
            let a = crate::boolean(&body, &bore, crate::BoolOp::Cut).unwrap().unwrap();
            let b = crate::boolean(&decoded, &bore, crate::BoolOp::Cut).unwrap().unwrap();
            assert!((crate::measure(&a).unwrap().volume - crate::measure(&b).unwrap().volume).abs() < 1e-7);
        }
    }
    #[test]
    fn native_spheres_tori_and_open_faces_round_trip_without_conversion() {
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let surface = crate::copy_faces(&cube, &[Vec3::new(20., 15., 10.)]).unwrap();
        let sphere = crate::sphere(Vec3::ZERO, 12.).unwrap();
        let torus = crate::torus(Vec3::ZERO, 20., 4.).unwrap();
        for body in [surface, sphere, torus] {
            let encoded = encode_native_body(&body).unwrap();
            let restored = decode_native_body(&encoded).unwrap();
            assert_eq!(body.is_surface(), restored.is_surface());
            assert_eq!(body.face_count(), restored.face_count());
            assert_eq!(encoded, encode_native_body(&restored).unwrap());
            if !body.is_surface() {
                equal(&body, &restored);
            }
        }
    }

    #[test]
    fn box_and_fillet_have_stable_wire_records() {
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let rounded = crate::fillet(&cube, &[Vec3::new(20., 0., 10.)], 2.).unwrap();
        let records: Vec<_> = [cube, rounded]
            .iter()
            .map(|body| {
                let bytes = encode_native_body(body).unwrap();
                assert_eq!(&bytes[..8], b"SCBREP05");
                let mut raw = &bytes[8..];
                let value = binary::read(&mut raw, 0, &mut 2_000_000).unwrap();
                assert!(raw.is_empty());
                value
            })
            .collect();
        let actual = serde_json::json!(records);
        // An intended change to the wire records rewrites the fixture with NATIVE_BREP_BLESS=1.
        if std::env::var_os("NATIVE_BREP_BLESS").is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/native/fixtures/box-fillet-v5.json");
            std::fs::write(path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
        }
        let expected: Value = serde_json::from_str(include_str!("native/fixtures/box-fillet-v5.json")).unwrap();
        compare_wire(&expected, &actual);
    }

    fn compare_wire(expected: &Value, actual: &Value) {
        match (expected, actual) {
            (Value::Number(a), Value::Number(b)) if a.is_f64() && b.is_f64() => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                assert!((a - b).abs() <= 1e-10 * a.abs().max(1.), "wire number changed: {a} vs {b}");
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    compare_wire(a, b);
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
                for (key, a) in a {
                    compare_wire(a, &b[key]);
                }
            }
            _ => assert_eq!(expected, actual),
        }
    }

    #[test]
    fn malformed_native_geometry_is_bounded_and_never_panics() {
        let body = crate::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        let bytes = encode_native_body(&body).unwrap();
        for n in [0, 1, 7, 8, 9, bytes.len() - 1] {
            assert!(decode_native_body(&bytes[..n]).is_err());
        }
        for i in (0..bytes.len()).step_by((bytes.len() / 256).max(1)) {
            let mut bad = bytes.clone();
            bad[i] ^= 0xff;
            assert!(std::panic::catch_unwind(|| decode_native_body(&bad)).is_ok());
        }
        let mut raw = &bytes[8..];
        let value = binary::read(&mut raw, 0, &mut 2_000_000).unwrap();
        for key in ["vertices", "index"] {
            let mut v = value.clone();
            if key == "vertices" {
                v["solid"]["edges"][0]["vertices"] = serde_json::json!([u64::MAX, 0]);
            } else {
                v["solid"]["faces"][0]["loops"][0]["edges"][0]["edge"] = serde_json::json!(u64::MAX);
            }
            let mut bad = MAGIC.to_vec();
            binary::write(&v, &mut bad, 0).unwrap();
            assert!(decode_native_body(&bad).is_err());
        }
        let mut huge = MAGIC.to_vec();
        huge.push(7);
        huge.extend(u32::MAX.to_le_bytes());
        assert!(decode_native_body(&huge).is_err());
    }

    #[test]
    fn native_preserves_face_appearance_and_authored_boundaries() {
        let mut body = crate::box_solid(Vec3::new(1., 2., 3.), Vec3::new(10., 20., 30.))
            .unwrap()
            .with_paint(Some(crate::Paint { opacity: 0.7, faces: vec![crate::FacePaint { face: 2, color: [0.25, 0.5, 0.75], opacity: 0.6 }] }));
        body.splits = Some(Arc::new(vec![vec![Vec3::new(1., 2., 3.), Vec3::new(11., 2., 3.)]]));
        body.regularization_protected = true;
        let restored = decode_native_body(&encode_native_body(&body).unwrap()).unwrap();
        equal(&body, &restored);
    }

    /// A face's pcurves as the adapter writes them: its loops relative to its surface.
    fn pcurves_of(face: &truck_modeling::Face, tol: f64) -> Result<Vec<Vec<solidcraft_geom::brep::Curve2>>> {
        let record = adapter::face_alone(face, tol)?;
        Ok(record
            .loops
            .into_iter()
            .map(|l| match l {
                solidcraft_geom::brep::Loop::Edges { edges } => edges.into_iter().map(|e| e.pcurve).collect(),
                solidcraft_geom::brep::Loop::Vertex { .. } => Vec::new(),
            })
            .collect())
    }
    /// Every pcurve of every face of `body` validates as the schema validator does, spans its
    /// curve's range, lifts onto the surface within `tol` (64 samples and every span midpoint),
    /// and each wire closes in (u, v): consecutive uses meet, or meet across a pole (the gap
    /// lifts to one point). Returns the wires whose closure is a whole number of periods (a loop
    /// round a pole or the surface), as (face, wire).
    fn check_pcurves(body: &Body, tol: f64) -> Vec<(usize, usize)> {
        check_faces(&body.solid.face_iter().cloned().collect::<Vec<_>>(), tol)
    }
    fn check_faces(faces: &[truck_modeling::Face], tol: f64) -> Vec<(usize, usize)> {
        use truck_modeling::{BoundedCurve, MetricSpace, ParametricCurve, ParametricSurface};
        let mut winding = Vec::new();
        for (f, face) in faces.iter().enumerate() {
            let surface = face.surface();
            let all = pcurves_of(face, tol).unwrap_or_else(|e| panic!("face {f}: {e}"));
            let wires = face.absolute_boundaries().clone();
            assert_eq!(all.len(), wires.len());
            let lift = |uv: [f64; 2]| surface.subs(uv[0], uv[1]);
            let mut areas = Vec::new();
            for (w, (pcs, wire)) in all.iter().zip(&wires).enumerate() {
                let ring: Vec<[f64; 2]> = pcs
                    .iter()
                    .zip(wire.edge_iter())
                    .flat_map(|(pc, e)| {
                        let [a, b] = pc.interval().unwrap();
                        (0..16)
                            .map(move |k| if e.orientation() { a + (b - a) * k as f64 / 16. } else { b - (b - a) * k as f64 / 16. })
                            .map(|t| pc.eval(t).unwrap())
                    })
                    .collect();
                areas.push(
                    (0..ring.len())
                        .map(|i| {
                            let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
                            p[0] * q[1] - q[0] * p[1]
                        })
                        .sum::<f64>()
                        * 0.5,
                );
                assert_eq!(pcs.len(), wire.len());
                let mut ends = Vec::new();
                for (i, (pc, edge)) in pcs.iter().zip(wire.edge_iter()).enumerate() {
                    let at = format!("face {f} wire {w} edge {i}");
                    let (knots, count, values): (&Vec<f64>, usize, Vec<f64>) = match pc {
                        solidcraft_geom::brep::Curve2::Spline { knots, points } => (knots, points.len(), points.iter().flatten().copied().collect()),
                        solidcraft_geom::brep::Curve2::RationalSpline { knots, homogeneous } => {
                            assert!(homogeneous.iter().all(|h| h[2] >= 1e-12), "{at}: weight");
                            (knots, homogeneous.len(), homogeneous.iter().flatten().copied().collect())
                        }
                    };
                    let degree = knots.len() - count - 1;
                    assert!((2..=4096).contains(&count) && (1..=16).contains(&degree) && degree < count, "{at}: size");
                    assert!(knots.windows(2).all(|k| k[0] <= k[1]) && knots[degree] < knots[count], "{at}: knots");
                    assert!(values.iter().chain(knots).all(|x| x.is_finite() && x.abs() <= 1e12), "{at}: numbers");
                    let curve = edge.curve();
                    let (t0, t1) = curve.range_tuple();
                    assert_eq!(pc.interval(), Some([t0, t1]), "{at}: interval");
                    let mut ts: Vec<f64> = (0..=64).map(|k| t0 + (t1 - t0) * k as f64 / 64.).collect();
                    ts.extend(knots.windows(2).filter(|k| k[0] < k[1]).map(|k| 0.5 * (k[0] + k[1])));
                    for t in ts {
                        let uv = pc.eval(t).unwrap_or_else(|| panic!("{at}: eval at {t}"));
                        // Truck's intersection points are its Newton iterate, within 1e-6 mm of
                        // the intersection the pcurve follows.
                        let slack = if matches!(curve, truck_modeling::Curve::IntersectionCurve(_)) { 1e-6 } else { 0. };
                        let d = lift(uv).distance(curve.subs(t));
                        assert!(d <= tol + slack, "{at}: lift error {d} at {t}");
                    }
                    let (a, b) = (pc.eval(t0).unwrap(), pc.eval(t1).unwrap());
                    ends.push(if edge.orientation() { (a, b) } else { (b, a) });
                }
                let mut wraps = false;
                for k in 0..ends.len() {
                    let (end, start) = (ends[k].1, ends[(k + 1) % ends.len()].0);
                    let gap = (end[0] - start[0]).abs().max((end[1] - start[1]).abs());
                    if gap <= 1e-9 {
                        continue;
                    }
                    // A pole: the gap's ends and middle are one point in space.
                    let mid = [0.5 * (end[0] + start[0]), 0.5 * (end[1] + start[1])];
                    let one = lift(end).distance(lift(start)).max(lift(end).distance(lift(mid)));
                    if one <= tol {
                        continue;
                    }
                    let periods = [surface.u_period(), surface.v_period()];
                    let whole = (0..2).all(|d| {
                        let g = start[d] - end[d];
                        g.abs() <= 1e-9 || periods[d].is_some_and(|p| (g / p - (g / p).round()).abs() <= 1e-9)
                    });
                    assert!(
                        whole && lift(end).distance(lift(start)) <= tol,
                        "face {f} wire {w}: open between uses {k} and {}: {end:?} {start:?}",
                        (k + 1) % ends.len()
                    );
                    wraps = true;
                }
                if wraps {
                    winding.push((f, w));
                }
            }
            // Loops are relative to the surface: the outer loop (the most area) turns
            // counter-clockwise in (u, v), holes clockwise, whichever side the face uses.
            if !winding.iter().any(|(wf, _)| *wf == f) {
                let outer = (0..areas.len()).max_by(|a, b| areas[*a].abs().total_cmp(&areas[*b].abs())).unwrap();
                for (w, area) in areas.iter().enumerate() {
                    assert_eq!(*area > 0., w == outer, "face {f} wire {w}: turns the wrong way ({area})");
                }
            }
        }
        winding
    }

    #[test]
    fn pcurves_on_planes_are_the_curves_mapped_exactly() {
        use truck_modeling::{BoundedCurve, ParametricCurve, ParametricSurface};
        let cube = crate::box_solid(Vec3::new(-3., 1., 2.), Vec3::new(40., 30., 10.)).unwrap();
        assert!(check_pcurves(&cube, 1e-9).is_empty());
        // Bit for bit: each line's pcurve ends lift to the line's own ends.
        for face in cube.solid.face_iter() {
            let surface = face.surface();
            for (pcs, wire) in pcurves_of(face, 1e-9).unwrap().iter().zip(face.absolute_boundaries().clone()) {
                for (pc, edge) in pcs.iter().zip(wire.edge_iter()) {
                    let solidcraft_geom::brep::Curve2::Spline { points, .. } = pc else { panic!("a line is a degree-1 spline") };
                    let curve = edge.curve();
                    let (t0, t1) = curve.range_tuple();
                    assert_eq!(surface.subs(points[0][0], points[0][1]), curve.subs(t0));
                    assert_eq!(surface.subs(points[1][0], points[1][1]), curve.subs(t1));
                }
            }
        }
        // Arcs (rational) on the cylinder's caps.
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 5., 40.).unwrap();
        for face in cyl.solid.face_iter().filter(|f| matches!(f.surface(), truck_modeling::Surface::Plane(_))) {
            assert!(pcurves_of(face, 1e-9).unwrap().iter().flatten().all(|c| matches!(c, solidcraft_geom::brep::Curve2::RationalSpline { .. })));
        }
    }

    /// The kernel's own curved bodies: the cylinder (in halves), patch sphere, torus, a cone
    /// (apex), a revolved ball, a tube, the cylinder cut by a tilted box (intersection curves),
    /// a filleted box.
    fn curved_bodies() -> Vec<(&'static str, Body)> {
        use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        let turn = |outer: Loop2| crate::revolve(&Plane::XZ, &[Region2 { outer, holes: vec![] }], Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap();
        let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 5., 40.).unwrap();
        let block = crate::box_solid(Vec3::new(-20., -20., 20.), Vec3::new(20., 20., 60.)).unwrap();
        let block = crate::transform(&block, Vec3::ZERO, Vec3::new(0., 0., 20.), Vec3::X, 0.4).unwrap();
        let cut = crate::boolean(&cyl, &block, crate::BoolOp::Cut).unwrap().unwrap();
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let rounded = crate::fillet(&cube, &[Vec3::new(20., 0., 10.)], 2.).unwrap();
        let half = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::ZERO, radius: 6., start: -FRAC_PI_2, sweep: PI },
                Seg2::Line { a: Vec2::new(0., 6.), b: Vec2::new(0., -6.) },
            ],
        };
        vec![
            ("cylinder", cyl),
            ("sphere", crate::sphere(Vec3::ZERO, 12.).unwrap()),
            ("torus", crate::torus(Vec3::ZERO, 20., 4.).unwrap()),
            ("cone", turn(Loop2::polygon(&[Vec2::new(0., 0.), Vec2::new(5., 0.), Vec2::new(0., 10.)]))),
            ("ball", turn(half)),
            ("tube", turn(Loop2::polygon(&[Vec2::new(3., 0.), Vec2::new(5., 0.), Vec2::new(5., 4.), Vec2::new(3., 4.)]))),
            ("cut", cut),
            ("fillet", rounded),
        ]
    }

    #[test]
    fn pcurves_fit_curved_faces_within_tolerance() {
        for (name, body) in curved_bodies() {
            for tol in [1e-3, 1e-5, 1e-6] {
                let start = std::time::Instant::now();
                for face in body.solid.face_iter() {
                    pcurves_of(face, tol).unwrap();
                }
                let took = start.elapsed();
                let winding = check_pcurves(&body, tol);
                eprintln!("{name} at {tol}: {took:?}; loops round the surface: {winding:?}");
                assert!(winding.is_empty(), "{name}");
            }
        }
    }

    /// Faces with seams, as imported faces have them (the kernel's own revolutions come in
    /// pieces): a cylinder whose seam line is used twice between two full circles, each a closed
    /// edge on one vertex, and a sphere bounded by one meridian used twice from pole to pole.
    fn seam_faces() -> Vec<(&'static str, truck_modeling::Face)> {
        use crate::face_params::iso_curve;
        use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        use truck_modeling::{self as mt, ParametricSurface};
        let turn = |outer: Loop2| crate::revolve(&Plane::XZ, &[Region2 { outer, holes: vec![] }], Vec2::ZERO, Vec2::Y, TAU).unwrap().pop().unwrap();
        let tube = turn(Loop2::polygon(&[Vec2::new(3., 0.), Vec2::new(5., 0.), Vec2::new(5., 4.), Vec2::new(3., 4.)]));
        let wall = tube
            .solid
            .face_iter()
            .map(|f| f.surface())
            .find(|s| matches!(s, mt::Surface::RevolutedCurve(_)) && (s.subs(0., 0.).x.hypot(s.subs(0., 0.).y) - 5.).abs() < 1e-9)
            .unwrap();
        let (p0, p1) = (mt::Vertex::new(wall.subs(0., 0.)), mt::Vertex::new(wall.subs(1., 0.)));
        let seam = mt::Edge::new_unchecked(&p0, &p1, iso_curve(&wall, true, 0., 0., 1.).unwrap());
        let low = mt::Edge::new_unchecked(&p0, &p0, iso_curve(&wall, false, 0., 0., TAU).unwrap());
        let high = mt::Edge::new_unchecked(&p1, &p1, iso_curve(&wall, false, 1., 0., TAU).unwrap());
        let cylinder = mt::Face::new_unchecked(vec![mt::Wire::from(vec![seam.clone(), high.clone(), seam.inverse(), low.inverse()])], wall.clone());
        let band = mt::Face::new_unchecked(vec![mt::Wire::from(vec![low]), mt::Wire::from(vec![high.inverse()])], wall);
        let half = Loop2 {
            segs: vec![
                Seg2::Arc { center: Vec2::ZERO, radius: 6., start: -FRAC_PI_2, sweep: PI },
                Seg2::Line { a: Vec2::new(0., 6.), b: Vec2::new(0., -6.) },
            ],
        };
        let ball = turn(half);
        let round = ball.solid.face_iter().map(|f| f.surface()).find(|s| matches!(s, mt::Surface::RevolutedCurve(_))).unwrap();
        let (s0, s1) = (mt::Vertex::new(round.subs(0., 0.)), mt::Vertex::new(round.subs(1., 0.)));
        let meridian = mt::Edge::new_unchecked(&s0, &s1, iso_curve(&round, true, 0., 0., 1.).unwrap());
        let sphere = mt::Face::new_unchecked(vec![mt::Wire::from(vec![meridian.clone(), meridian.inverse()])], round);
        vec![("seam cylinder", cylinder), ("seam sphere", sphere), ("band", band)]
    }

    #[test]
    fn pcurves_of_seams_lie_a_period_apart() {
        use solidcraft_geom::brep::Curve2;
        for (name, face) in seam_faces() {
            for bad in [0., -1., f64::NAN, f64::INFINITY] {
                assert!(pcurves_of(&face, bad).is_err());
            }
            for tol in [1e-3, 1e-6] {
                // A face round the surface without a seam: each loop closes a period apart.
                if name == "band" {
                    assert_eq!(check_faces(std::slice::from_ref(&face), tol), [(0, 0), (0, 1)]);
                    continue;
                }
                assert!(check_faces(std::slice::from_ref(&face), tol).is_empty(), "{name}");
                let wire = &pcurves_of(&face, tol).unwrap()[0];
                let at = |c: &Curve2, f: f64| {
                    let [a, b] = c.interval().unwrap();
                    c.eval(a + (b - a) * f).unwrap()
                };
                // The seam's two uses: the same curve on either side of the period.
                let (first, second) = if name == "seam cylinder" { (&wire[0], &wire[2]) } else { (&wire[0], &wire[1]) };
                for f in [0., 0.3, 1.] {
                    let (a, b) = (at(first, f), at(second, f));
                    assert!((a[0] - b[0]).abs() < 1e-9 && ((b[1] - a[1]).abs() - std::f64::consts::TAU).abs() < 1e-9, "{name}: {a:?} {b:?}");
                }
                for c in wire {
                    let (lo, hi) = (0..=16).map(|k| at(c, k as f64 / 16.)[1]).fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
                    assert!(lo >= -1e-9 && hi <= std::f64::consts::TAU + 1e-9, "{name}: in the base period");
                }
            }
        }
    }

    /// With `SOLIDCRAFT_PCURVE_SVG=<dir>`, each test body's face loops in (u, v), one panel per
    /// face: the surface's base period framed, each edge use in its own colour with an arrow
    /// along the wire.
    #[test]
    fn pcurve_pictures() {
        let Ok(dir) = std::env::var("SOLIDCRAFT_PCURVE_SVG") else { return };
        let box_ = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let mut sets: Vec<(String, Vec<truck_modeling::Face>)> = vec![("box".into(), box_.solid.face_iter().cloned().collect())];
        sets.extend(curved_bodies().into_iter().map(|(n, b)| (n.to_string(), b.solid.face_iter().cloned().collect())));
        sets.extend(seam_faces().into_iter().map(|(n, f)| (n.to_string(), vec![f])));
        for (name, faces) in sets {
            let svg = draw(&name, &faces, 1e-5);
            std::fs::write(format!("{dir}/{}.svg", name.replace(' ', "-")), svg).unwrap();
        }
    }

    fn draw(name: &str, faces: &[truck_modeling::Face], tol: f64) -> String {
        use std::fmt::Write;
        use std::ops::Bound::{Excluded, Included};
        use truck_modeling::ParametricSurface;
        const W: f64 = 300.;
        const COLOURS: [&str; 8] = ["#d62728", "#1f77b4", "#2ca02c", "#9467bd", "#ff7f0e", "#17becf", "#e377c2", "#8c564b"];
        let cols = faces.len().min(4);
        let rows = faces.len().div_ceil(4);
        let mut out = String::new();
        write!(out, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" font-family="sans-serif" font-size="12"><rect width="100%" height="100%" fill="white"/><text x="10" y="20" font-size="16">{name}: face loops in (u, v)</text>"#, cols as f64 * W, rows as f64 * W + 30.).unwrap();
        for (f, face) in faces.iter().enumerate() {
            let (ox, oy) = ((f % 4) as f64 * W, (f / 4) as f64 * W + 30.);
            let surface = face.surface();
            let wires = pcurves_of(face, tol).unwrap();
            let lines: Vec<Vec<(usize, bool, Vec<[f64; 2]>)>> = wires
                .iter()
                .zip(face.absolute_boundaries().clone())
                .map(|(pcs, wire)| {
                    pcs.iter()
                        .zip(wire.edge_iter())
                        .enumerate()
                        .map(|(i, (c, e))| {
                            let [a, b] = c.interval().unwrap();
                            (i, e.orientation(), (0..=64).map(|k| c.eval(a + (b - a) * k as f64 / 64.).unwrap()).collect())
                        })
                        .collect()
                })
                .collect();
            // The frame: the base period along periodic directions, the surface's range along
            // bounded ones, else the loops' extent.
            let all: Vec<[f64; 2]> = lines.iter().flatten().flat_map(|l| l.2.clone()).collect();
            let ext = |d: usize| all.iter().map(|p| p[d]).fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
            let (ru, rv) = surface.parameter_range();
            let range = |r: (std::ops::Bound<f64>, std::ops::Bound<f64>), d: usize| match r {
                (Included(a) | Excluded(a), Included(b) | Excluded(b)) => (a, b),
                _ => ext(d),
            };
            let frame = if matches!(surface, truck_modeling::Surface::Plane(_)) { [ext(0), ext(1)] } else { [range(ru, 0), range(rv, 1)] };
            let view = |d: usize| {
                let (e, r) = (ext(d), frame[d]);
                let (lo, hi) = (e.0.min(r.0), e.1.max(r.1));
                let pad = (hi - lo).max(1e-9) * 0.08;
                (lo - pad, hi + pad)
            };
            let (vu, vv) = (view(0), view(1));
            let px = |p: [f64; 2]| (ox + 50. + (p[0] - vu.0) / (vu.1 - vu.0) * (W - 70.), oy + W - 30. - (p[1] - vv.0) / (vv.1 - vv.0) * (W - 50.));
            let kind = match surface {
                truck_modeling::Surface::Plane(_) => "plane",
                truck_modeling::Surface::BSplineSurface(_) => "B-spline",
                truck_modeling::Surface::NurbsSurface(_) => "NURBS",
                truck_modeling::Surface::RevolutedCurve(_) => "revolution",
            };
            let periodic = match (surface.u_period(), surface.v_period()) {
                (Some(_), _) => ", u periodic",
                (_, Some(_)) => ", v periodic",
                _ => "",
            };
            let (a, b) = (px([frame[0].0, frame[1].1]), px([frame[0].1, frame[1].0]));
            write!(out, r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#f4f4f4" stroke="#888" stroke-dasharray="4 3"/><text x="{}" y="{}">face {f}: {kind}{periodic}</text>"##, a.0, a.1, b.0 - a.0, b.1 - a.1, ox + 10., oy + 14.).unwrap();
            for (w, wire) in lines.iter().enumerate() {
                for (i, fwd, pts) in wire {
                    let colour = COLOURS[(i + 3 * w) % COLOURS.len()];
                    let path: Vec<String> = pts
                        .iter()
                        .map(|p| {
                            let q = px(*p);
                            format!("{:.2} {:.2}", q.0, q.1)
                        })
                        .collect();
                    write!(out, r#"<path d="M {}" fill="none" stroke="{colour}" stroke-width="2"/>"#, path.join(" L ")).unwrap();
                    // An arrow at the middle, along the wire.
                    let (m0, m1) = (px(pts[31]), px(pts[33]));
                    let (dx, dy) = if *fwd { (m1.0 - m0.0, m1.1 - m0.1) } else { (m0.0 - m1.0, m0.1 - m1.1) };
                    let len = dx.hypot(dy).max(1e-9);
                    let (ux, uy) = (dx / len * 9., dy / len * 9.);
                    let c = px(pts[32]);
                    write!(
                        out,
                        r#"<polygon points="{:.2},{:.2} {:.2},{:.2} {:.2},{:.2}" fill="{colour}"/>"#,
                        c.0 + ux,
                        c.1 + uy,
                        c.0 - ux - uy * 0.6,
                        c.1 - uy + ux * 0.6,
                        c.0 - ux + uy * 0.6,
                        c.1 - uy - ux * 0.6
                    )
                    .unwrap();
                    let s = px(if *fwd { pts[0] } else { pts[64] });
                    write!(
                        out,
                        r#"<circle cx="{:.2}" cy="{:.2}" r="2.5" fill="black"/><text x="{:.2}" y="{:.2}" fill="{colour}">{w}.{i}</text>"#,
                        s.0,
                        s.1,
                        c.0 + 6.,
                        c.1 - 6.
                    )
                    .unwrap();
                }
            }
            let (l0, l1) = (px([frame[0].0, frame[1].0]), px([frame[0].1, frame[1].1]));
            write!(out, r##"<text x="{:.2}" y="{:.2}" fill="#555">u {:.3}</text><text x="{:.2}" y="{:.2}" fill="#555" text-anchor="end">{:.3}</text><text x="{:.2}" y="{:.2}" fill="#555" text-anchor="end">v {:.3}</text><text x="{:.2}" y="{:.2}" fill="#555" text-anchor="end">{:.3}</text>"##,
                l0.0, oy + W - 12., frame[0].0, l1.0, oy + W - 12., frame[0].1, ox + 46., l0.1, frame[1].0, ox + 46., l1.1 + 10., frame[1].1).unwrap();
        }
        out.push_str("</svg>");
        out
    }
}
