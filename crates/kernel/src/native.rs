//! SolidCraft native, versioned binary geometry. No STEP conversion or display meshes.
//! Exact: a decoded body is the encoded one, number for number, for the same producer.
use crate::{Body, KernelError, Result, guard};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::sync::Arc;

mod adapter;
mod binary;
mod metadata;
#[allow(dead_code)]
mod pcurve;
mod preview;
pub use preview::native_preview_mesh;

// 01/02 were development-only prototypes; independent metadata records start at 03; 04 stores
// procedural intersections and placed revolutions exactly, and the measured runtime values.
const MAGIC: &[u8; 8] = b"SCBREP04";
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
    /// [`Body::boundary_tol`]: the accuracy the file it was read from declares. Written only
    /// when there is one, so a body without it is stored as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_tol: Option<f64>,
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
        let runtime =
            Runtime { size: body.size, coarse_missing: body.coarse_missing.map(|(tol, n)| (tol, n as u64)), boundary_tol: body.boundary_tol };
        let saved = SavedBody { solid: adapter::encode(&body.solid)?, metadata: metadata::encode(body)?, runtime };
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
        let solid = adapter::decode(saved.solid)?;
        let Runtime { size, coarse_missing, boundary_tol } = saved.runtime;
        if !(size.is_finite() && size >= 0.0)
            || coarse_missing.is_some_and(|(tol, n)| !tol.is_finite() || n > MAX_ITEMS as u64)
            || boundary_tol.is_some_and(|t| !(t.is_finite() && t > 0.0))
        {
            return Err(invalid("runtime values"));
        }
        let mut body = Body {
            solid: Arc::new(solid),
            size,
            boundary_tol,
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
            regularization_protected: false,
        };
        metadata::apply(&mut body, saved.metadata)?;
        Ok(body)
    })
}

fn validate_topology(saved: &SavedBody) -> Result<()> {
    saved.solid.validate().map_err(invalid)?;
    let faces: usize = saved.solid.shells.iter().map(|s| s.faces.len()).sum();
    let edges: usize = saved.solid.shells.iter().map(|s| s.edges.len()).sum();
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
    /// A body read from a file keeps the accuracy the file declares through the model cache, so
    /// a reopened design plans its faces as the import did; one without it is stored as before.
    #[test]
    fn a_files_declared_accuracy_survives_the_model_cache() {
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(4., 3., 1.)).unwrap();
        let plain = encode_native_body(&cube).unwrap();
        assert_eq!(decode_native_body(&plain).unwrap().boundary_tol, None);
        let read = cube.with_boundary_tol(Some(1e-5));
        let bytes = encode_native_body(&read).unwrap();
        assert_ne!(bytes, plain);
        assert_eq!(decode_native_body(&bytes).unwrap().boundary_tol, Some(1e-5));
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
                assert_eq!(&bytes[..8], b"SCBREP04");
                let mut raw = &bytes[8..];
                let value = binary::read(&mut raw, 0, &mut 2_000_000).unwrap();
                assert!(raw.is_empty());
                value
            })
            .collect();
        let actual = serde_json::json!(records);
        let expected: Value = serde_json::from_str(include_str!("native/fixtures/box-fillet-v4.json")).unwrap();
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
                v["solid"]["shells"][0]["edges"][0]["vertices"] = serde_json::json!([u64::MAX, 0]);
            } else {
                v["solid"]["shells"][0]["faces"][0]["loops"][0][0]["edge"] = serde_json::json!(u64::MAX);
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

    /// Every pcurve of every face of `body` validates as the schema validator does, spans its
    /// curve's range, lifts onto the surface within `tol` (64 samples and every span midpoint),
    /// and each wire closes in (u, v): consecutive uses meet, or meet across a pole (the gap
    /// lifts to one point). Returns the wires whose closure is a whole number of periods (a loop
    /// round a pole or the surface), as (face, wire).
    fn check_pcurves(body: &Body, tol: f64) -> Vec<(usize, usize)> {
        use truck_modeling::{BoundedCurve, MetricSpace, ParametricCurve, ParametricSurface};
        let mut winding = Vec::new();
        for (f, face) in body.solid.face_iter().enumerate() {
            let surface = face.surface();
            let all = pcurve::pcurves(face, tol).unwrap_or_else(|e| panic!("face {f}: {e}"));
            let wires = face.boundaries();
            assert_eq!(all.len(), wires.len());
            let lift = |uv: [f64; 2]| surface.subs(uv[0], uv[1]);
            for (w, (pcs, wire)) in all.iter().zip(&wires).enumerate() {
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
                        let uv = pcurve::eval(pc, t).unwrap_or_else(|| panic!("{at}: eval at {t}"));
                        let d = lift(uv).distance(curve.subs(t));
                        assert!(d <= tol, "{at}: lift error {d} at {t}");
                    }
                    let (a, b) = (pcurve::eval(pc, t0).unwrap(), pcurve::eval(pc, t1).unwrap());
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
            for (pcs, wire) in pcurve::pcurves(face, 1e-9).unwrap().iter().zip(face.boundaries()) {
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
            assert!(pcurve::pcurves(face, 1e-9).unwrap().iter().flatten().all(|c| matches!(c, solidcraft_geom::brep::Curve2::RationalSpline { .. })));
        }
    }
}
