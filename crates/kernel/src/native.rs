//! SolidCraft native, versioned binary geometry. No STEP conversion or display meshes.
//! Exact: a decoded body is the encoded one, number for number, for the same producer.
use crate::{Body, KernelError, Result, guard};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

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
    /// Saved by earlier versions (an orientation mesh's missing-face count); read and ignored.
    coarse_missing: Option<(f64, u64)>,
}
fn invalid(detail: impl std::fmt::Display) -> KernelError {
    KernelError::Invalid(format!("native B-rep: {detail}"))
}

/// Explicit topology, spatial geometry, construction certificates and history, exactly: the
/// body's own record and what it carries. Failure is a cache miss at the IO boundary, never a
/// failed document save.
pub fn encode_native_body(body: &Body) -> Result<Vec<u8>> {
    let _span = solidcraft_profile::span("kernel", "encode_native_body");
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
        if body.mesh.is_some() {
            return Err(invalid("mesh bodies are source blobs, not B-rep"));
        }
        if body.face_count() > MAX_ITEMS || body.edge_count() > MAX_ITEMS {
            return Err(invalid("native geometry budget"));
        }
        let runtime = Runtime { size: body.size, coarse_missing: None };
        let saved = SavedBody { solid: body.record().clone(), metadata: metadata::encode(body)?, runtime };
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

/// The schema 4 record of a solid body, in canonical order with its pcurves: the body itself.
/// Mesh bodies have none.
pub fn brep_record(body: &Body) -> Result<solidcraft_geom::brep::Brep> {
    let _span = solidcraft_profile::span("kernel", "brep_record");
    if body.mesh.is_some() {
        return Err(invalid("mesh bodies are source blobs, not B-rep"));
    }
    Ok(body.record().clone())
}

/// The body in SMLib (see [`Body::smlib`]), as an owned copy.
pub fn smlib_body(body: &Body) -> Result<solidcraft_smlib::Brep> {
    let _span = solidcraft_profile::span("kernel", "smlib_body");
    body.smlib()?.try_clone().map_err(|e| KernelError::Failed(format!("SMLib: {e}")))
}

/// Decode bounded untrusted data. Validate indices before topology extraction.
/// Only a caller that has checked who produced the data may treat it as evaluated geometry.
pub fn decode_native_body(bytes: &[u8]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "decode_native_body");
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
        let Runtime { size, .. } = saved.runtime;
        if !(size.is_finite() && size >= 0.0) {
            return Err(invalid("runtime values"));
        }
        let mut body = Body::from_record(saved.solid)?;
        if size > 0.0 {
            body.size = size;
        }
        metadata::apply(&mut body, saved.metadata)?;
        Ok(body)
    })
}

/// An SMLib body as a kernel body: SMLib's export mapped by `solidcraft_smlib` (canonical order,
/// radians, pcurves) is the body's record ([`Body::from_smlib_record`]).
pub fn body_from_smlib(brep: &solidcraft_smlib::Brep) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "body_from_smlib");
    body_from_smlib_ordered(brep).map(|(body, _)| body)
}

/// [`body_from_smlib`], with where each of SMLib's faces and edges is in the body's face and
/// edge order ([`crate::history`]).
pub(crate) fn body_from_smlib_ordered(brep: &solidcraft_smlib::Brep) -> Result<(Body, Option<solidcraft_smlib::history::Order>)> {
    let _span = solidcraft_profile::span("kernel", "body_from_smlib_ordered");
    let (record, order) = brep.to_native_ordered().map_err(|e| KernelError::Failed(e.to_string()))?;
    Ok((Body::from_smlib_record(record)?, Some(order)))
}

/// `body` in SMLib, with where each of SMLib's faces and edges is in the body's face and edge
/// order: SMLib's export of the imported body, checked to number every face, edge and vertex as
/// the body's record does. `None` where it does not.
pub(crate) fn smlib_body_ordered(body: &Body) -> Result<(solidcraft_smlib::Brep, Option<solidcraft_smlib::history::Order>)> {
    let brep = smlib_body(body)?;
    Ok((brep.try_clone().map_err(|e| KernelError::Failed(format!("SMLib: {e}")))?, numbering(body.record(), &brep)))
}

/// `record` in SMLib, imported as given (SMLib's own form; a record it refuses is an error that
/// names why), with where each of SMLib's faces and edges is in the record's order.
pub(crate) fn smlib_record_ordered(
    record: &solidcraft_geom::brep::Brep,
) -> Result<(solidcraft_smlib::Brep, Option<solidcraft_smlib::history::Order>)> {
    let _span = solidcraft_profile::span("kernel", "smlib_record_ordered");
    let brep = solidcraft_smlib::Brep::from_record(record, true).map_err(|e| KernelError::Failed(format!("SMLib: {e}")))?;
    let order = numbering(record, &brep);
    Ok((brep, order))
}

/// Where SMLib's faces and edges of `brep` (imported from `record`) are in the record's order:
/// `None` where SMLib's export does not number them as the record does.
fn numbering(record: &solidcraft_geom::brep::Brep, brep: &solidcraft_smlib::Brep) -> Option<solidcraft_smlib::history::Order> {
    match brep.to_native_ordered() {
        Ok((back, order)) => same_numbering(record, &back).then_some(order),
        Err(e) => {
            log::debug!(target: "smlib", "no order for an operand: {e}");
            None
        }
    }
}

/// Whether two records of one body number its faces, edges and vertices alike: the same loops of
/// the same edges, the same edge ends, and each vertex where the other's is.
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
#[cfg(test)]
mod smlib_import_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    use std::sync::Arc;
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
    fn a_kernel_body_past_the_file_item_budget_is_taken_and_a_file_of_it_refused() {
        // The box with its first edge a polyline of the file item budget's worth of points: a
        // record the kernel's own operations may make (#3992), and more than a file may carry.
        let body = crate::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        let mut record = body.record().clone();
        let [t0, t1] = record.edges[0].interval;
        let n = solidcraft_geom::brep::MAX_ITEMS;
        let points = (0..n).map(|i| [i as f64 * 1e-6, 0., 0.]).collect();
        record.edges[0].curve = solidcraft_geom::brep::Curve::Polyline { points };
        assert!(t0 >= 0.0 && t1 <= (n - 1) as f64);
        Body::from_record(record.clone()).unwrap();
        let saved = SavedBody { solid: record, metadata: metadata::encode(&body).unwrap(), runtime: Runtime { size: 0.0, coarse_missing: None } };
        let refused = validate_topology(&saved).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(refused.contains("item budget"), "{refused}");
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
}
