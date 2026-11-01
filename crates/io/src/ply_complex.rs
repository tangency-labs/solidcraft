//! A PLY file read with everything a point cloud can hold: its vertices with all their
//! properties. Positions are `x`, `y`, `z` and colours `red`, `green`, `blue`; every other scalar
//! property becomes a column named as in the file, whole-numbered when its type is an integer. A
//! comment `property: 0 name, 1 name` names the values of a whole-numbered property. Every
//! `stride`th vertex is kept.

use solidcraft_doc::point_cloud::{Column, Complex, Floats, MAX_COLUMNS, Names};

use crate::ply::{Element, Kind, Rows, Scalar, header};
use crate::{IoError, Result};

fn invalid(m: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("PLY: {m}"))
}

/// What a PLY file holds, positions in the file's units.
pub(crate) struct PlyCloud {
    pub positions: Vec<[f64; 3]>,
    pub colors: Option<Vec<[u8; 3]>>,
    pub complex: Complex,
    /// Vertices in the file.
    pub vertices: u64,
}

fn integer(s: Scalar) -> bool {
    !matches!(s, Scalar::F32 | Scalar::F64)
}

fn named(e: &Element, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|n| e.properties.iter().position(|p| p.name.eq_ignore_ascii_case(n) && matches!(p.kind, Kind::Scalar(_))))
}

/// The columns an element's scalar properties fill, leaving out those at `taken`.
fn columns_of(e: &Element, taken: &[usize]) -> Vec<(usize, Column)> {
    e.properties
        .iter()
        .enumerate()
        .filter(|(i, _)| !taken.contains(i))
        .filter_map(|(i, p)| match p.kind {
            Kind::Scalar(s) => Some((i, Column { name: p.name.clone(), integer: integer(s), values: Floats::default() })),
            Kind::List(..) => None,
        })
        .collect()
}

/// Read a PLY file's vertices, every `stride`th one of those beyond `max_points`.
pub(crate) fn read_ply_cloud(bytes: &[u8], max_points: usize) -> Result<PlyCloud> {
    let h = header(bytes)?;
    let vertex = h.elements.iter().find(|e| e.name == "vertex").ok_or_else(|| invalid("no `vertex` element"))?;
    let stride = vertex.count.div_ceil(max_points.max(1) as u64).max(1);
    let xyz = [named(vertex, &["x"]), named(vertex, &["y"]), named(vertex, &["z"])];
    let [Some(x), Some(y), Some(z)] = xyz else { return Err(invalid("the vertices have no `x`, `y` and `z`")) };
    let rgb =
        [named(vertex, &["red", "r", "diffuse_red"]), named(vertex, &["green", "g", "diffuse_green"]), named(vertex, &["blue", "b", "diffuse_blue"])];
    let rgb = match rgb {
        [Some(r), Some(g), Some(b)] => Some([r, g, b]),
        _ => None,
    };
    let mut taken = vec![x, y, z];
    taken.extend(rgb.into_iter().flatten());
    let mine = columns_of(vertex, &taken);
    if mine.len() > MAX_COLUMNS {
        return Err(invalid(format!("more than {MAX_COLUMNS} properties")));
    }
    let body = bytes.len().saturating_sub(h.body) as u64;
    let mut rows = Rows::new(&h, bytes);
    let mut out = PlyCloud { positions: Vec::new(), colors: rgb.map(|_| Vec::new()), complex: Complex::default(), vertices: vertex.count };
    let mut columns: Vec<(usize, Vec<f32>, Column)> = mine.into_iter().map(|(i, c)| (i, Vec::new(), c)).collect();
    let mut row = vec![0.0f64; vertex.properties.len()];
    // The elements before the vertices are read past.
    for e in h.elements.iter().take_while(|e| e.name != "vertex").chain([vertex]) {
        if e.count > body {
            return Err(invalid(format!("the file is too short for its {} {}", e.count, e.name)));
        }
        let vertices = std::ptr::eq(e, vertex);
        for r in 0..e.count {
            for (i, p) in e.properties.iter().enumerate() {
                match p.kind {
                    Kind::Scalar(t) => {
                        let v = rows.value(t)?;
                        if let Some(s) = row.get_mut(i).filter(|_| vertices) {
                            *s = v;
                        }
                    }
                    Kind::List(c, t) => {
                        for _ in 0..rows.count(c)? {
                            rows.value(t)?;
                        }
                    }
                }
            }
            if !vertices || !r.is_multiple_of(stride) {
                continue;
            }
            let v = |i: usize| row.get(i).copied().unwrap_or(0.0);
            out.positions.push([v(x), v(y), v(z)]);
            if let (Some(c), Some(ks)) = (out.colors.as_mut(), rgb) {
                let t = |i: usize| match vertex.properties.get(i).map(|p| p.kind) {
                    Some(Kind::Scalar(s)) => s.channel(v(i)),
                    _ => 0,
                };
                c.push(ks.map(t));
            }
            for (slot, values, _) in &mut columns {
                values.push(v(*slot) as f32);
            }
        }
    }
    let columns: Vec<Column> = columns.into_iter().map(|(_, values, c)| Column { values: Floats(values.into()), ..c }).collect();
    // Names only for whole-numbered properties the file has, the first comment for each.
    let mut names: Vec<Names> = Vec::new();
    for n in h.comments.iter().filter_map(|c| Names::parse(c)) {
        let label = columns.iter().any(|c| c.name == n.property && c.integer);
        if label && !names.iter().any(|m| m.property == n.property) && names.len() < MAX_COLUMNS {
            names.push(n);
        }
    }
    out.complex = Complex { columns, names };
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vertex_property_is_kept() {
        let text = "ply\nformat ascii 1.0\ncomment role: 7 wall, -1 none\ncomment t: seconds since the start\ncomment width: 0 thin\n\
            element vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty double t\nproperty int role\nproperty float width\n\
            element face 1\nproperty list uchar int vertex_indices\nend_header\n\
            0 0 0 0.5 7 1\n1 0 0 1.5 7 1\n1 1 0 2.5 -1 2\n0 1 0 3.5 -1 2\n4 0 1 2 3\n";
        let r = read_ply_cloud(text.as_bytes(), 10).unwrap();
        assert_eq!(r.positions.len(), 4);
        let col = |n: &str| r.complex.column(n).unwrap();
        assert_eq!(&*col("t").values.0, &[0.5, 1.5, 2.5, 3.5]);
        assert!(col("role").integer && !col("width").integer);
        assert_eq!(&*col("role").values.0, &[7.0, 7.0, -1.0, -1.0]);
        r.complex.check(4).unwrap();
        // Names for the whole-numbered `role` only: `t` is no list and `width` is not whole.
        assert_eq!(r.complex.names.len(), 1);
        assert_eq!(r.complex.names("role").and_then(|n| n.name(-1)), Some("none"));
        // Every second vertex, with its numbers.
        let half = read_ply_cloud(text.as_bytes(), 2).unwrap();
        assert_eq!((half.positions.len(), &*half.complex.column("t").unwrap().values.0), (2, &[0.5f32, 2.5][..]));
        half.complex.check(2).unwrap();
    }
}
