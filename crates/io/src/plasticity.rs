//! Reader for Plasticity's `.plasticity` files, for interoperability (opens them as mesh bodies).
//!
//! Layout (observed in users' own files; no Plasticity code): the magic `plasticity`, a u32
//! version (1), a u32 total length, then glTF-style chunks `[u32 length][type][body]` with types
//! `JSON` and `BIN\0`. The JSON lists `nodes` (items, groups, empties, measurements), `items`
//! with a cached tessellation (`lods[0].mesh`) next to the exact geometry, `meshes` whose
//! triangle primitive (mode 4) carries `indices`, `POSITION`, `NORMAL` and `groups` ((start,
//! count) index ranges, one per face), and `bufferViews` into the binary chunk. Lengths are in
//! metres.
//!
//! The exact geometry is a Parasolid transmit blob, which is not read: solids come from the
//! cached tessellation, refined back to smooth curved faces ([`crate::mesh_refine`]). Curves
//! (no cached tessellation) and measurements are skipped and reported. Embedded objects (STL
//! scans placed by an empty) are read as meshes.

use serde_json::Value;
use solidcore_record::Vec3;

use crate::{IoError, Result, threemf::MeshObject};

const M_TO_MM: f64 = 1000.0;

/// One body of the file.
pub struct PlasticityItem {
    pub object: MeshObject,
    /// Hidden in Plasticity (itself or a group it is in).
    pub hidden: bool,
    /// The groups it is in, outermost first, joined with ` / `.
    pub group: Option<String>,
}

pub struct PlasticityFile {
    pub generator: String,
    pub items: Vec<PlasticityItem>,
    pub skipped_curves: usize,
    pub skipped_measurements: usize,
}

/// Do these bytes start like a Plasticity file?
pub fn is_plasticity(bytes: &[u8]) -> bool {
    bytes.len() > 26 && bytes.starts_with(b"plasticity")
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn bad(msg: impl Into<String>) -> IoError {
    IoError::Invalid(format!("Plasticity file: {}", msg.into()))
}

struct Views<'a> {
    bin: &'a [u8],
    views: &'a [Value],
}

impl Views<'_> {
    fn bytes(&self, i: &Value) -> Result<&[u8]> {
        let v = i.as_u64().and_then(|i| self.views.get(usize::try_from(i).ok()?)).ok_or_else(|| bad("bad buffer view"))?;
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let len = v.get("byteLength").and_then(Value::as_u64).ok_or_else(|| bad("buffer view without a length"))? as usize;
        self.bin.get(off..off.checked_add(len).ok_or_else(|| bad("bad buffer view"))?).ok_or_else(|| bad("buffer view past the end"))
    }
    fn f32s(&self, i: &Value) -> Result<Vec<f32>> {
        Ok(self.bytes(i)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }
    fn u32s(&self, i: &Value) -> Result<Vec<u32>> {
        Ok(self.bytes(i)?.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }
    /// Indices in the view's `type` (`uint8`, `uint16` or the default `uint32`).
    fn indices(&self, i: &Value) -> Result<Vec<u32>> {
        let ty =
            i.as_u64().and_then(|i| self.views.get(usize::try_from(i).ok()?)).and_then(|v| v.get("type")).and_then(Value::as_str).unwrap_or("uint32");
        let b = self.bytes(i)?;
        Ok(match ty {
            "uint8" => b.iter().map(|&x| u32::from(x)).collect(),
            "uint16" => b.as_chunks::<2>().0.iter().map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect(),
            _ => self.u32s(i)?,
        })
    }
}

/// Read a `.plasticity` file. `refine` rebuilds smooth curved faces from the coarse cached mesh.
pub fn read_plasticity(bytes: &[u8], refine: bool) -> Result<PlasticityFile> {
    if !is_plasticity(bytes) {
        return Err(bad("not a Plasticity file"));
    }
    let version = u32_at(bytes, 10).ok_or_else(|| bad("truncated"))?;
    if version != 1 {
        return Err(bad(format!("unsupported version {version}")));
    }
    let (mut json, mut bin): (Option<Value>, &[u8]) = (None, &[]);
    let mut o = 18usize;
    while o + 8 <= bytes.len() {
        let len = u32_at(bytes, o).ok_or_else(|| bad("truncated chunk"))? as usize;
        let ty = bytes.get(o + 4..o + 8).unwrap_or(&[]);
        let end = (o + 8).saturating_add(len).min(bytes.len());
        let body = bytes.get(o + 8..end).unwrap_or(&[]);
        if ty == b"JSON" {
            json = Some(serde_json::from_slice(body).map_err(|e| bad(format!("bad JSON: {e}")))?);
        } else if ty.starts_with(b"BIN") {
            bin = body;
        }
        o = (o + 8).saturating_add(len);
    }
    let json = json.ok_or_else(|| bad("no JSON chunk"))?;
    let empty = Vec::new();
    let arr = |k: &str| json.get(k).and_then(Value::as_array).unwrap_or(&empty);
    let views = Views { bin, views: arr("bufferViews") };
    let nodes = arr("nodes");

    // node -> the group node containing it
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let group_node: std::collections::HashMap<u64, usize> =
        nodes.iter().enumerate().filter_map(|(i, n)| n.get("group").and_then(Value::as_u64).map(|g| (g, i))).collect();
    for (gi, g) in arr("groups").iter().enumerate() {
        let Some(&gn) = group_node.get(&(gi as u64)) else { continue };
        for c in g.get("children").and_then(Value::as_array).unwrap_or(&empty) {
            if let Some(slot) = c.as_u64().and_then(|c| parent.get_mut(c as usize)) {
                *slot = Some(gn);
            }
        }
    }
    let flag = |n: &Value, k: &str| n.get(k).and_then(Value::as_bool).unwrap_or(false);
    let group_path = |ni: usize| {
        let (mut names, mut hidden, mut at, mut steps) = (Vec::new(), false, parent[ni], 0);
        while let Some(p) = at {
            let n = &nodes[p];
            hidden |= flag(n, "hidden") || flag(n, "invisible");
            if let Some(name) = n.get("name").and_then(Value::as_str) {
                names.insert(0, name.to_string());
            }
            at = parent[p];
            steps += 1;
            if steps > nodes.len() {
                break; // a cycle in a damaged file
            }
        }
        ((!names.is_empty()).then(|| names.join(" / ")), hidden)
    };

    let mut out = PlasticityFile {
        generator: json.get("asset").and_then(|a| a.get("generator")).and_then(Value::as_str).unwrap_or("Plasticity").to_string(),
        items: Vec::new(),
        skipped_curves: 0,
        skipped_measurements: 0,
    };
    let mut unnamed = 0;
    for (ni, n) in nodes.iter().enumerate() {
        if n.get("measurement").is_some() {
            out.skipped_measurements += 1;
            continue;
        }
        let (group, hidden_by_group) = group_path(ni);
        let hidden = flag(n, "hidden") || flag(n, "invisible") || hidden_by_group;
        let mut name = n.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_string);
        if let Some(item) = n.get("item").and_then(Value::as_u64) {
            let mesh = arr("items")
                .get(item as usize)
                .and_then(|it| it.get("lods"))
                .and_then(|l| l.get(0))
                .and_then(|l| l.get("mesh"))
                .and_then(Value::as_u64);
            let tri = mesh
                .and_then(|m| arr("meshes").get(m as usize))
                .and_then(|m| m.get("primitives"))
                .and_then(Value::as_array)
                .and_then(|ps| ps.iter().find(|p| p.get("mode").and_then(Value::as_u64) == Some(4) && p.get("indices").is_some()));
            let Some(tri) = tri else {
                out.skipped_curves += 1;
                continue;
            };
            let attrs = tri.get("attributes").ok_or_else(|| bad("primitive without attributes"))?;
            let pos = views.f32s(attrs.get("POSITION").ok_or_else(|| bad("mesh without positions"))?)?;
            let normals = match attrs.get("NORMAL") {
                Some(v) => Some(views.f32s(v)?),
                None => None,
            };
            let idx = views.indices(tri.get("indices").unwrap_or(&Value::Null))?;
            let groups = match tri.get("groups") {
                Some(g) => views.u32s(g)?,
                None => vec![0, idx.len() as u32],
            };
            let tri_count = idx.len() / 3;
            let mut tri_face = vec![0u32; tri_count];
            for (f, pair) in groups.as_chunks::<2>().0.iter().enumerate() {
                let (s, c) = (pair[0] as usize, pair[1] as usize);
                let range = (s / 3).min(tri_count)..((s + c) / 3).min(tri_count);
                tri_face[range].fill(f as u32);
            }
            let vert = |v: u32, a: &[f32], k: f64| -> Option<Vec3> {
                let i = v as usize * 3;
                Some(Vec3::new(f64::from(*a.get(i)?) * k, f64::from(*a.get(i + 1)?) * k, f64::from(*a.get(i + 2)?) * k))
            };
            let mut corners = Vec::with_capacity(tri_count * 3);
            let mut corner_normals = Vec::with_capacity(tri_count * 3);
            for &v in idx.iter().take(tri_count * 3) {
                corners.push(vert(v, &pos, M_TO_MM).ok_or_else(|| bad("index out of range"))?);
                if let Some(nn) = &normals {
                    corner_normals.push(vert(v, nn, 1.0).unwrap_or(Vec3::new(0.0, 0.0, 1.0)));
                }
            }
            let refined = if refine && normals.is_some() { crate::mesh_refine::refine(&corners, &corner_normals, &tri_face) } else { None };
            let (positions, triangles) = match refined {
                Some(r) => (r.positions, r.triangles),
                None => {
                    let (pts, ids) = crate::mesh_refine::weld(&corners);
                    let tris = ids
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
                        .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
                        .collect();
                    (pts, tris)
                }
            };
            let name = name.take().unwrap_or_else(|| {
                unnamed += 1;
                format!("Solid{unnamed}")
            });
            out.items.push(PlasticityItem { object: MeshObject { name, positions, triangles, ..Default::default() }, hidden, group });
        } else if let Some(e) = n.get("empty").and_then(Value::as_u64) {
            // an empty placing an embedded object (a mesh such as an STL scan)
            let Some(e) = arr("empties").get(e as usize) else { continue };
            if e.get("type").and_then(Value::as_str) != Some("Object") {
                continue;
            }
            let uri = e.get("object");
            let Some(ob) = arr("objects").iter().find(|x| x.get("uri") == uri) else { continue };
            let Some(view) = ob.get("bufferView") else { continue };
            let Ok(tris) = crate::read_stl(views.bytes(view)?) else { continue };
            // column-major 4×4, then metres to millimetres
            let m: Vec<f64> = n
                .get("transform")
                .and_then(Value::as_array)
                .filter(|a| a.len() == 16)
                .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                .unwrap_or_else(|| vec![1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]);
            let apply = |p: &[f32; 3]| {
                let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                Vec3::new(
                    (m[0] * x + m[4] * y + m[8] * z + m[12]) * M_TO_MM,
                    (m[1] * x + m[5] * y + m[9] * z + m[13]) * M_TO_MM,
                    (m[2] * x + m[6] * y + m[10] * z + m[14]) * M_TO_MM,
                )
            };
            let corners: Vec<Vec3> = tris.iter().flatten().map(apply).collect();
            let (positions, ids) = crate::mesh_refine::weld(&corners);
            let triangles = ids
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
                .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
                .collect();
            let name = name.take().unwrap_or_else(|| "Object".into());
            out.items.push(PlasticityItem { object: MeshObject { name, positions, triangles, ..Default::default() }, hidden, group });
        }
    }
    if out.items.is_empty() {
        return Err(bad("no solids or meshes to open (curves and measurements are not read)"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal file: one item, a unit triangle in metres with one face group.
    fn tiny() -> Vec<u8> {
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u32, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        for g in [0u32, 3] {
            bin.extend_from_slice(&g.to_le_bytes());
        }
        let json = serde_json::json!({
            "asset": {"generator": "test"},
            "nodes": [{"item": 0, "name": "Tri"}, {"measurement": 0}, {"item": 1}],
            "items": [{"lods": [{"mesh": 0}]}, {"lods": []}],
            "meshes": [{"primitives": [{"mode": 4, "indices": 1, "attributes": {"POSITION": 0}, "groups": 2}]}],
            "bufferViews": [{"byteOffset": 0, "byteLength": 36}, {"byteOffset": 36, "byteLength": 12}, {"byteOffset": 48, "byteLength": 8}]
        })
        .to_string();
        let mut out = b"plasticity".to_vec();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(json.as_bytes());
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn reads_items_in_millimetres_and_reports_what_it_skips() {
        let f = read_plasticity(&tiny(), true).unwrap();
        assert_eq!(f.generator, "test");
        assert_eq!(f.items.len(), 1);
        assert_eq!((f.skipped_curves, f.skipped_measurements), (1, 1));
        let o = &f.items[0].object;
        assert_eq!(o.name, "Tri");
        assert_eq!(o.triangles.len(), 1);
        assert!(o.positions.iter().any(|p| (p.x - 10.0).abs() < 1e-6));
    }

    #[test]
    fn damaged_files_are_errors_not_panics() {
        let good = tiny();
        assert!(read_plasticity(b"not a plasticity file at all", true).is_err());
        for cut in [20, 30, good.len() / 2, good.len() - 10] {
            let _ = read_plasticity(&good[..cut], true);
        }
    }
}
