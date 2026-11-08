//! The live-link mesh: a body's [`PolyMesh`] as one JSON object with its arrays packed as
//! base64, for a program that follows the open design (`live.meshes`). The format is
//! `docs/formats/live-mesh.md`.
//!
//! Arrays are little-endian: a reader turns each into a typed array in one step instead of
//! parsing numbers. Positions are `f32` relative to `origin` (the centre of the mesh's box), so
//! a part far from the origin keeps its detail. The key names the content: two meshes with the
//! same key are the same, so a reader keeps what it has and asks only for keys it lacks.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcraft_doc::canvas::base64_encode;
use solidcraft_geom::Vec3;
use solidcraft_geom::poly_mesh::PolyMesh;

/// The format tag a live-link mesh carries.
pub const LIVE_MESH_FORMAT: &str = "solidcraft.live-mesh";
/// Its version; a reader refuses one it does not know.
pub const LIVE_MESH_VERSION: u32 = 1;

/// One encoded mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveMesh {
    /// Names the content (32 hex digits).
    pub key: String,
    pub json: Value,
    /// Bytes of the packed arrays, before base64.
    pub bytes: usize,
}

/// `m` encoded; `options` names how it was made (it goes into the key, so the same body made
/// another way has another key), `face_names` are the B-rep faces' persistent names by face
/// index, and `normals` says whether corner normals are sent.
pub fn encode(m: &PolyMesh, options: &str, face_names: &[String], normals: bool) -> LiveMesh {
    let big = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let (lo, hi) = m.positions.iter().fold((big, -big), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let origin = if m.positions.is_empty() { Vec3::ZERO } else { (lo + hi) * 0.5 };
    let positions = f32s(m.positions.iter().flat_map(|p| {
        let q = *p - origin;
        [q.x, q.y, q.z]
    }));
    let loop_normals = if normals { f32s(m.loop_normals.iter().flat_map(|n| [n.x, n.y, n.z])) } else { Vec::new() };
    let poly_sizes = u32s(m.poly_sizes.iter().copied());
    let loops = u32s(m.loops.iter().copied());
    let poly_face = u32s(m.poly_face.iter().copied());
    let edges = u32s(m.edges.iter().flatten().copied());
    let edge_brep = u32s(m.edge_brep.iter().copied());
    let edge_flags = m.edge_flags.clone();

    let mut h = Sha256::new();
    h.update(LIVE_MESH_FORMAT.as_bytes());
    h.update(LIVE_MESH_VERSION.to_le_bytes());
    h.update(options.as_bytes());
    h.update([0, u8::from(normals)]);
    for v in [origin.x, origin.y, origin.z] {
        h.update(v.to_le_bytes());
    }
    for part in [&positions, &loop_normals, &poly_sizes, &loops, &poly_face, &edges, &edge_flags, &edge_brep] {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part);
    }
    for name in face_names {
        h.update((name.len() as u64).to_le_bytes());
        h.update(name.as_bytes());
    }
    let key: String = h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect();

    let bytes = [&positions, &loop_normals, &poly_sizes, &loops, &poly_face, &edges, &edge_flags, &edge_brep].iter().map(|p| p.len()).sum();
    let mut json = json!({
        "format": LIVE_MESH_FORMAT,
        "version": LIVE_MESH_VERSION,
        "key": key,
        "origin": [origin.x, origin.y, origin.z],
        "counts": {
            "vertices": m.positions.len(),
            "polygons": m.poly_sizes.len(),
            "loops": m.loops.len(),
            "edges": m.edges.len(),
        },
        "positions": base64_encode(&positions),
        "poly_sizes": base64_encode(&poly_sizes),
        "loops": base64_encode(&loops),
        "poly_face": base64_encode(&poly_face),
        "edges": base64_encode(&edges),
        "edge_flags": base64_encode(&edge_flags),
        "edge_brep": base64_encode(&edge_brep),
        "face_names": face_names,
        "stats": {
            "triangles": m.stats.triangles,
            "quads": m.stats.quads,
            "ngons": m.stats.ngons,
            "open_edges": m.stats.open_edges,
        },
    });
    if normals && let Some(o) = json.as_object_mut() {
        o.insert("loop_normals".into(), Value::String(base64_encode(&loop_normals)));
    }
    LiveMesh { key, json, bytes }
}

fn f32s(values: impl Iterator<Item = f64>) -> Vec<u8> {
    values.flat_map(|v| (v as f32).to_le_bytes()).collect()
}

fn u32s(values: impl Iterator<Item = u32>) -> Vec<u8> {
    values.flat_map(u32::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::poly_mesh::{PolyStats, edge_flag};

    fn decode(s: &str) -> Vec<u8> {
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let digits: Vec<u32> = s.bytes().filter(|&c| c != b'=').map(|c| B64.iter().position(|&d| d == c).unwrap() as u32).collect();
        for chunk in digits.chunks(4) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (k, d)| n | (d << (18 - 6 * k)));
            out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
        }
        out
    }

    fn square() -> PolyMesh {
        PolyMesh {
            positions: vec![Vec3::new(100.0, 0.0, 0.0), Vec3::new(101.0, 0.0, 0.0), Vec3::new(101.0, 1.0, 0.0), Vec3::new(100.0, 1.0, 0.0)],
            poly_sizes: vec![4],
            loops: vec![0, 1, 2, 3],
            loop_normals: vec![Vec3::Z; 4],
            poly_face: vec![7],
            edges: vec![[0, 1]],
            edge_flags: vec![edge_flag::BREP | edge_flag::SHARP],
            edge_brep: vec![2],
            stats: PolyStats { triangles: 0, quads: 1, ngons: 0, open_edges: 4 },
        }
    }

    #[test]
    fn a_mesh_reads_back_from_its_arrays() {
        let m = encode(&square(), "ngons", &["F1:top".into()], true);
        let j = &m.json;
        assert_eq!((j["format"].as_str(), j["version"].as_u64()), (Some(LIVE_MESH_FORMAT), Some(1)));
        assert_eq!(j["origin"], json!([100.5, 0.5, 0.0]));
        let f32s = |k: &str| decode(j[k].as_str().unwrap()).chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>();
        let u32s = |k: &str| decode(j[k].as_str().unwrap()).chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>();
        assert_eq!(f32s("positions"), vec![-0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0]);
        assert_eq!(f32s("loop_normals").len(), 12);
        assert_eq!(u32s("poly_sizes"), vec![4]);
        assert_eq!(u32s("loops"), vec![0, 1, 2, 3]);
        assert_eq!(u32s("poly_face"), vec![7]);
        assert_eq!(u32s("edges"), vec![0, 1]);
        assert_eq!(u32s("edge_brep"), vec![2]);
        assert_eq!(decode(j["edge_flags"].as_str().unwrap()), vec![3]);
        assert_eq!(j["face_names"], json!(["F1:top"]));
        assert_eq!(j["counts"]["polygons"], 1);
        assert_eq!(m.bytes, 48 + 48 + 4 + 16 + 4 + 8 + 1 + 4);
    }

    #[test]
    fn the_key_follows_the_content() {
        let a = encode(&square(), "ngons", &[], true);
        assert_eq!(a.key.len(), 32);
        assert_eq!(a, encode(&square(), "ngons", &[], true), "the same mesh, the same key");
        assert_ne!(a.key, encode(&square(), "quads", &[], true).key, "made another way");
        assert_ne!(a.key, encode(&square(), "ngons", &[], false).key, "without normals");
        assert_ne!(a.key, encode(&square(), "ngons", &["F1".into()], true).key, "other face names");
        let mut moved = square();
        moved.positions[2].z = 1e-3;
        assert_ne!(a.key, encode(&moved, "ngons", &[], true).key);
        assert!(encode(&square(), "ngons", &[], false).json.get("loop_normals").is_none());
        let empty = encode(&PolyMesh::default(), "", &[], true);
        assert_eq!((empty.json["origin"].clone(), empty.bytes), (json!([0.0, 0.0, 0.0]), 0));
    }
}
