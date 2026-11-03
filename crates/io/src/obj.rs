//! Wavefront OBJ import: polygon meshes (`v`, `f` with any of `v`, `v/vt`, `v//vn`, `v/vt/vn`,
//! negative indices counting back), one mesh body per object (`o`, else group `g`). Polygons
//! are split into fans. Texture coordinates, normals and materials are ignored. The file is
//! untrusted: counts are capped and bad lines become warnings.

use solidcore_record::Vec3;

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
    let mut objects: Vec<(String, Vec<[u32; 3]>)> = vec![(default_name.to_string(), Vec::new())];
    let mut warnings: Vec<String> = Vec::new();
    let mut bad_lines = 0usize;
    let mut triangles = 0usize;
    let mut named_by_o = false;
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
                let n = verts.len() as i64;
                let idx: Option<Vec<u32>> = it
                    .map(|tok| {
                        let i: i64 = tok.split('/').next()?.parse().ok()?;
                        let i = if i < 0 { n + i } else { i - 1 };
                        (0..n).contains(&i).then(|| u32::try_from(i).ok()).flatten()
                    })
                    .collect();
                match idx {
                    Some(p) if p.len() >= 3 && p.iter().all(|i| verts.get(*i as usize).is_some_and(|v| v.is_finite())) => {
                        if let Some((_, tris)) = objects.last_mut() {
                            for k in 1..p.len() - 1 {
                                tris.push([p[0], p[k], p[k + 1]]);
                            }
                            triangles += p.len() - 2;
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
        for t in tris {
            let mut nt = [0u32; 3];
            for (k, i) in t.iter().enumerate() {
                let next = u32::try_from(positions.len()).map_err(|_| IoError::Invalid("OBJ too large".into()))?;
                nt[k] = *map.entry(*i).or_insert_with(|| {
                    positions.push(verts.get(*i as usize).copied().unwrap_or(Vec3::ZERO));
                    next
                });
            }
            triangles.push(nt);
        }
        out.push(MeshObject { name, positions, triangles, ..Default::default() });
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
