//! Wavefront OBJ import: polygon meshes (`v`, `f` with any of `v`, `v/vt`, `v//vn`, `v/vt/vn`,
//! negative indices counting back), one mesh body per object (`o`, else group `g`). Polygons
//! are split into fans. Authored corner normals and smoothing groups are retained. Texture
//! coordinates and materials are not consumed. The file is
//! untrusted: counts are capped and bad lines become warnings.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

#[derive(Clone, Copy)]
struct Corner {
    vertex: u32,
    normal: Option<u32>,
}

struct Face {
    corners: [Corner; 3],
    smoothing: u32,
}

fn index(token: &str, len: usize) -> Option<u32> {
    let value: i64 = token.parse().ok()?;
    let len = i64::try_from(len).ok()?;
    let value = if value < 0 { len.checked_add(value)? } else { value.checked_sub(1)? };
    (0..len).contains(&value).then(|| u32::try_from(value).ok()).flatten()
}

use crate::threemf::MeshObject;
use crate::{IoError, Result};

/// Most vertices and triangles we read from one file.
const MAX_VERTICES: usize = 20_000_000;
const MAX_TRIANGLES: usize = 20_000_000;

/// Meshes of an OBJ file and what could not be read.
pub fn read_obj(bytes: &[u8], default_name: &str) -> Result<(Vec<MeshObject>, Vec<String>)> {
    let text = String::from_utf8_lossy(bytes);
    let mut verts: Vec<Vec3> = Vec::new();
    // Objects: name and polygons as vertex indices into `verts`.
    let mut objects: Vec<(String, Vec<Face>)> = vec![(default_name.to_string(), Vec::new())];
    let mut warnings: Vec<String> = Vec::new();
    let mut bad_lines = 0usize;
    let mut triangles = 0usize;
    let mut named_by_o = false;
    let mut normals = Vec::<Vec3>::new();
    let mut smoothing = 0u32;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f64> = it.take(3).filter_map(|x| x.parse::<f64>().ok()).collect();
                match c[..] {
                    [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite() => verts.push(Vec3::new(x, y, z)),
                    _ => {
                        // Keep the numbering: later faces count vertices by position.
                        verts.push(Vec3::new(f64::NAN, f64::NAN, f64::NAN));
                        bad_lines += 1;
                    }
                }
                if verts.len() > MAX_VERTICES {
                    return Err(IoError::Invalid(format!("OBJ with more than {MAX_VERTICES} vertices")));
                }
            }
            Some("vn") => {
                let c: Vec<f64> = it.take(3).filter_map(|x| x.parse::<f64>().ok()).collect();
                let normal = match c[..] {
                    [x, y, z] => Vec3::new(x, y, z).normalized().filter(|n| n.is_finite()),
                    _ => None,
                };
                normals.push(normal.unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN)));
                if normal.is_none() {
                    bad_lines += 1;
                }
                if normals.len() > MAX_VERTICES {
                    return Err(IoError::Invalid(format!("OBJ with more than {MAX_VERTICES} normals")));
                }
            }
            Some("s") => {
                smoothing = match it.next() {
                    Some("off" | "0") | None => 0,
                    Some("on") => 1,
                    Some(v) => v.parse::<u32>().unwrap_or(0),
                };
            }
            Some("o") | Some("g") => {
                let is_o = line.starts_with('o');
                let name = it.collect::<Vec<_>>().join(" ");
                if name.is_empty() || (!is_o && named_by_o) {
                    continue;
                }
                named_by_o |= is_o;
                match objects.last_mut() {
                    // An empty object takes the name instead of starting another.
                    Some((n, tris)) if tris.is_empty() => *n = name,
                    _ => objects.push((name, Vec::new())),
                }
            }
            Some("f") => {
                let corners: Option<Vec<Corner>> = it
                    .map(|tok| {
                        let mut fields = tok.split('/');
                        let vertex = index(fields.next()?, verts.len())?;
                        fields.next(); // UVs do not replace the saved procedural material mapping.
                        let normal = fields.next().filter(|v| !v.is_empty()).and_then(|v| index(v, normals.len()));
                        Some(Corner { vertex, normal })
                    })
                    .collect();
                match corners {
                    Some(p) if p.len() >= 3 && p.iter().all(|i| verts.get(i.vertex as usize).is_some_and(|v| v.is_finite())) => {
                        let added = p.len() - 2;
                        triangles = triangles.checked_add(added).ok_or_else(|| IoError::Invalid("OBJ triangle count overflow".into()))?;
                        if triangles > MAX_TRIANGLES {
                            return Err(IoError::Invalid(format!("OBJ with more than {MAX_TRIANGLES} triangles")));
                        }
                        if let Some((_, tris)) = objects.last_mut() {
                            for k in 1..p.len() - 1 {
                                tris.push(Face { corners: [p[0], p[k], p[k + 1]], smoothing });
                            }
                        }
                    }
                    _ => bad_lines += 1,
                }
                if triangles > MAX_TRIANGLES {
                    return Err(IoError::Invalid(format!("OBJ with more than {MAX_TRIANGLES} triangles")));
                }
            }
            _ => {}
        }
    }
    if bad_lines > 0 {
        warnings.push(format!("{bad_lines} vertex or face line(s) could not be read and were left out"));
    }
    let mut out = Vec::new();
    for (name, tris) in objects {
        if tris.is_empty() {
            continue;
        }
        // Each object keeps only its own vertices.
        let mut map = std::collections::HashMap::new();
        let mut positions = Vec::new();
        let mut triangles = Vec::with_capacity(tris.len());
        for t in &tris {
            let mut nt = [0u32; 3];
            for (k, i) in t.corners.iter().enumerate() {
                let next = u32::try_from(positions.len()).map_err(|_| IoError::Invalid("OBJ too large".into()))?;
                nt[k] = *map.entry(i.vertex).or_insert_with(|| {
                    positions.push(verts.get(i.vertex as usize).copied().unwrap_or(Vec3::ZERO));
                    next
                });
            }
            triangles.push(nt);
        }
        let mesh = solidcraft_geom::Mesh { positions, triangles, ..Default::default() };
        let annotated = tris.iter().any(|f| f.smoothing != 0 || f.corners.iter().any(|c| c.normal.is_some()));
        let mut corner_normals = Vec::new();
        if annotated {
            let groups: Vec<u32> = tris.iter().map(|f| f.smoothing).collect();
            corner_normals = mesh.crease_corner_normals(std::f64::consts::PI, Some(&groups)).map_err(IoError::Invalid)?;
            for (f, n) in tris.iter().zip(&mut corner_normals) {
                for (corner, shading) in f.corners.iter().zip(n) {
                    if let Some(normal) = corner.normal.and_then(|k| normals.get(k as usize)).copied().filter(|n| n.is_finite()) {
                        *shading = normal;
                    }
                }
            }
        }
        out.push(MeshObject { name, positions: mesh.positions, triangles: mesh.triangles, corner_normals, ..Default::default() });
    }
    if out.is_empty() {
        return Err(IoError::Invalid("OBJ has no faces".into()));
    }
    Ok((out, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_corner_normals_and_smoothing_groups_survive_import() {
        let vertices = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 -0.8660254037844386 0.5\n";
        let authored = format!("{vertices}vn 0 0 1\nvn 0 0.5 0.8660254037844386\nf 1//-2 2//-2 3//-2\nf 2//-1 1//-1 4//-1\n");
        let (objects, warnings) = read_obj(authored.as_bytes(), "corners").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(objects[0].positions.len(), 4);
        assert_eq!(objects[0].corner_normals.len(), 2);
        assert_eq!(objects[0].corner_normals[0], [Vec3::Z; 3]);
        assert!((objects[0].corner_normals[1][0] - Vec3::new(0., 0.5, 0.8660254037844386)).len() < 1e-12);
        let grouped = format!("{vertices}s 5\nf 1 2 3\nf 2 1 4\n");
        let (objects, _) = read_obj(grouped.as_bytes(), "smooth").unwrap();
        assert_eq!(objects[0].corner_normals[0][0], objects[0].corner_normals[1][1]);
        assert!(objects[0].corner_normals[0][0].y > 0.2);
        let separated = format!("{vertices}s 5\nf 1 2 3\ns off\nf 2 1 4\n");
        let (objects, _) = read_obj(separated.as_bytes(), "crease").unwrap();
        assert_eq!(objects[0].corner_normals[0][0], Vec3::Z);
        let old = format!("{vertices}f 1 2 3\nf 2 1 4\n");
        assert!(read_obj(old.as_bytes(), "flat").unwrap().0[0].corner_normals.is_empty());
    }

    #[test]
    fn objects_quads_and_negative_indices() {
        let src = "# cube\no Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
                   f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2/1 3/2 7/3 6/4\nf 3//1 4//1 8//1 7//1\nf -4 -8 -5 -1\n\
                   o Tri\nv 5 5 5\nv 6 5 5\nv 5 6 5\nf -3 -2 -1\n";
        let (objs, warnings) = read_obj(src.as_bytes(), "x").unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(objs.len(), 2);
        assert_eq!(objs[0].name, "Cube");
        assert_eq!(objs[0].triangles.len(), 12);
        assert_eq!(objs[0].positions.len(), 8);
        assert_eq!(objs[1].name, "Tri");
        assert_eq!(objs[1].triangles.len(), 1);
    }

    #[test]
    fn hostile_obj_never_panics() {
        for src in [
            "",
            "f 1 2 3",
            "v 1 2\nf 1 1 1",
            "v nan 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3",
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 99999999999999999999",
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -1 -2 -9",
            "o\ng\nf",
            "\u{0}\u{ff}v 1 1 1",
        ] {
            let r = std::panic::catch_unwind(|| read_obj(src.as_bytes(), "x").map(|(o, _)| o.len()));
            assert!(r.is_ok(), "{src:?}");
        }
        assert!(read_obj(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nf 1 2 x", "x").unwrap().1.len() == 1);
    }
}
