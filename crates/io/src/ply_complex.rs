//! A PLY file read with everything a point cloud can hold: the vertices with all their
//! properties, and the `edge` and `face` elements as segments and triangles (a face as a fan)
//! with theirs. Positions are `x`, `y`, `z` and colours `red`, `green`, `blue`; every other
//! scalar property becomes a column named as in the file, whole-numbered when its type is an
//! integer. A comment `property: 0 name, 1 name` names the values of a whole-numbered property.
//! A file with edges or faces is read whole, since thinning its vertices would cut them; a file
//! of points alone keeps every `stride`th vertex.

use solidcraft_doc::point_cloud::{Column, Complex, Floats, Indices, MAX_CELLS, MAX_COLUMNS, Names, On};

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
    /// Faces whose indices could not be read.
    pub bad_faces: usize,
}

fn integer(s: Scalar) -> bool {
    !matches!(s, Scalar::F32 | Scalar::F64)
}

fn named(e: &Element, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|n| e.properties.iter().position(|p| p.name.eq_ignore_ascii_case(n) && matches!(p.kind, Kind::Scalar(_))))
}

/// The columns an element's scalar properties fill, leaving out those at `taken`.
fn columns_of(e: &Element, on: On, taken: &[usize]) -> Vec<(usize, Column)> {
    e.properties
        .iter()
        .enumerate()
        .filter(|(i, _)| !taken.contains(i))
        .filter_map(|(i, p)| match p.kind {
            Kind::Scalar(s) => Some((i, Column { name: p.name.clone(), on, integer: integer(s), values: Floats::default() })),
            Kind::List(..) => None,
        })
        .collect()
}

/// An index into `n` vertices, or `None` for anything else.
fn index(v: f64, n: usize) -> Option<u32> {
    (v >= 0.0 && v < n as f64 && v.fract() == 0.0).then(|| u32::try_from(v as u64).ok()).flatten()
}

/// Read a PLY file, keeping every `stride`th vertex when it has no edges or faces, and refusing
/// one with edges or faces and more than `max_points` vertices.
pub(crate) fn read_ply_cloud(bytes: &[u8], max_points: usize) -> Result<PlyCloud> {
    let h = header(bytes)?;
    let vertex = h.elements.iter().find(|e| e.name == "vertex").ok_or_else(|| invalid("no `vertex` element"))?;
    let has_cells = h.elements.iter().any(|e| (e.name == "edge" || e.name == "face") && e.count > 0);
    let stride = if has_cells {
        if vertex.count > max_points as u64 {
            return Err(invalid(format!("{} vertices with edges or faces: more than the {max_points} points read whole", vertex.count)));
        }
        1
    } else {
        vertex.count.div_ceil(max_points.max(1) as u64).max(1)
    };
    let body = bytes.len().saturating_sub(h.body) as u64;
    let mut rows = Rows::new(&h, bytes);
    let mut out = PlyCloud { positions: Vec::new(), colors: None, complex: Complex::default(), vertices: vertex.count, bad_faces: 0 };
    let mut columns: Vec<(On, Vec<f32>, Column)> = Vec::new();
    let (mut segments, mut triangles) = (Vec::new(), Vec::new());
    let mut row = vec![0.0f64; e_max(&h.elements)];
    let mut list: Vec<f64> = Vec::new();
    for e in &h.elements {
        if e.count > body {
            return Err(invalid(format!("the file is too short for its {} {}", e.count, e.name)));
        }
        let (on, taken, xyz, rgb, ends, face) = match e.name.as_str() {
            "vertex" => {
                let xyz = [named(e, &["x"]), named(e, &["y"]), named(e, &["z"])];
                let [Some(x), Some(y), Some(z)] = xyz else { return Err(invalid("the vertices have no `x`, `y` and `z`")) };
                let rgb =
                    [named(e, &["red", "r", "diffuse_red"]), named(e, &["green", "g", "diffuse_green"]), named(e, &["blue", "b", "diffuse_blue"])];
                let rgb = match rgb {
                    [Some(r), Some(g), Some(b)] => Some([r, g, b]),
                    _ => None,
                };
                let mut taken = vec![x, y, z];
                taken.extend(rgb.into_iter().flatten());
                (Some(On::Point), taken, Some([x, y, z]), rgb, None, None)
            }
            "edge" => {
                let (Some(a), Some(b)) = (named(e, &["vertex1"]), named(e, &["vertex2"])) else {
                    return Err(invalid("the edges have no `vertex1` and `vertex2`"));
                };
                (Some(On::Segment), vec![a, b], None, None, Some([a, b]), None)
            }
            "face" => {
                let face = e
                    .properties
                    .iter()
                    .position(|p| matches!(p.name.as_str(), "vertex_indices" | "vertex_index") && matches!(p.kind, Kind::List(..)));
                (face.map(|_| On::Triangle), Vec::new(), None, None, None, face)
            }
            _ => (None, Vec::new(), None, None, None, None),
        };
        let mine: Vec<(usize, Column)> = on.map(|on| columns_of(e, on, &taken)).unwrap_or_default();
        if columns.len() + mine.len() > MAX_COLUMNS {
            return Err(invalid(format!("more than {MAX_COLUMNS} properties")));
        }
        let first = columns.len();
        let slots: Vec<usize> = mine.iter().map(|(i, _)| *i).collect();
        columns.extend(mine.into_iter().map(|(_, c)| (c.on, Vec::new(), c)));
        if rgb.is_some() {
            out.colors = Some(Vec::new());
        }
        for r in 0..e.count {
            list.clear();
            for (i, p) in e.properties.iter().enumerate() {
                match p.kind {
                    Kind::Scalar(t) => {
                        let v = rows.value(t)?;
                        if let Some(s) = row.get_mut(i) {
                            *s = v;
                        }
                    }
                    Kind::List(c, t) => {
                        let n = rows.count(c)?;
                        for _ in 0..n {
                            let v = rows.value(t)?;
                            if face == Some(i) {
                                list.push(v);
                            }
                        }
                    }
                }
            }
            let v = |i: usize| row.get(i).copied().unwrap_or(0.0);
            // How many elements this row stands for: a vertex kept, a segment, a fan's triangles.
            let copies = if let Some([x, y, z]) = xyz {
                if !r.is_multiple_of(stride) {
                    continue;
                }
                out.positions.push([v(x), v(y), v(z)]);
                if let (Some(c), Some(ks)) = (out.colors.as_mut(), rgb) {
                    let t = |i: usize| match e.properties.get(i).map(|p| p.kind) {
                        Some(Kind::Scalar(s)) => s.channel(v(i)),
                        _ => 0,
                    };
                    c.push(ks.map(t));
                }
                1
            } else if let Some([a, b]) = ends {
                let n = vertex.count as usize;
                let (Some(a), Some(b)) = (index(v(a), n), index(v(b), n)) else { return Err(invalid("an edge names a missing vertex")) };
                segments.extend([a, b]);
                1
            } else if face.is_some() {
                let n = vertex.count as usize;
                match list.iter().map(|x| index(*x, n)).collect::<Option<Vec<u32>>>() {
                    Some(p) if p.len() >= 3 => {
                        for k in 1..p.len() - 1 {
                            triangles.extend([p[0], p[k], p[k + 1]]);
                        }
                        p.len() - 2
                    }
                    _ => {
                        out.bad_faces += 1;
                        0
                    }
                }
            } else {
                0
            };
            for (k, slot) in slots.iter().enumerate() {
                if let Some((_, values, _)) = columns.get_mut(first + k) {
                    values.extend(std::iter::repeat_n(v(*slot) as f32, copies));
                }
            }
            if segments.len() / 2 > MAX_CELLS || triangles.len() / 3 > MAX_CELLS {
                return Err(invalid(format!("more than {MAX_CELLS} edges or triangles")));
            }
        }
    }
    let columns: Vec<Column> = columns.into_iter().map(|(_, values, c)| Column { values: Floats(values.into()), ..c }).collect();
    // Names only for whole-numbered properties the file has, the first comment for each.
    let mut names: Vec<Names> = Vec::new();
    for n in h.comments.iter().filter_map(|c| Names::parse(c)) {
        let label = columns.iter().any(|c| c.name == n.property) && columns.iter().filter(|c| c.name == n.property).all(|c| c.integer);
        if label && !names.iter().any(|m| m.property == n.property) && names.len() < MAX_COLUMNS {
            names.push(n);
        }
    }
    out.complex = Complex { segments: Indices(segments.into()), triangles: Indices(triangles.into()), columns, names };
    Ok(out)
}

/// The most properties of any element: the row buffer's length.
fn e_max(elements: &[Element]) -> usize {
    elements.iter().map(|e| e.properties.len()).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_faces_and_every_property_are_kept() {
        let text = "ply\nformat ascii 1.0\ncomment role: 7 wall, -1 none\ncomment t: seconds since the start\ncomment width: 0 thin\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty double t\n\
            element edge 2\nproperty int vertex1\nproperty int vertex2\nproperty int role\nproperty double width\n\
            element face 1\nproperty list uchar int vertex_indices\nproperty int layer\nend_header\n\
            0 0 0 0.5\n1 0 0 1.5\n1 1 0 2.5\n0 1 0 3.5\n0 1 7 0.42\n1 2 -1 0\n4 0 1 2 3 9\n";
        let r = read_ply_cloud(text.as_bytes(), 10).unwrap();
        assert_eq!(r.positions.len(), 4);
        assert_eq!(&*r.complex.segments.0, &[0, 1, 1, 2]);
        assert_eq!(&*r.complex.triangles.0, &[0, 1, 2, 0, 2, 3], "a quad is a fan of two");
        let col = |n: &str, on| r.complex.column(n, on).unwrap();
        assert_eq!(&*col("t", On::Point).values.0, &[0.5, 1.5, 2.5, 3.5]);
        assert!(col("role", On::Segment).integer && !col("width", On::Segment).integer);
        assert_eq!(&*col("role", On::Segment).values.0, &[7.0, -1.0]);
        assert_eq!(&*col("layer", On::Triangle).values.0, &[9.0, 9.0], "each triangle of the fan has the face's");
        r.complex.check(4).unwrap();
        // Names for the whole-numbered `role` only: `t` is no list and `width` is not whole.
        assert_eq!(r.complex.names.len(), 1);
        assert_eq!(r.complex.names("role").and_then(|n| n.name(-1)), Some("none"));
        assert!(read_ply_cloud(text.as_bytes(), 3).is_err(), "edges and faces are not thinned");
        let bad = text.replace("1 2 -1 0", "1 9 -1 0");
        assert!(read_ply_cloud(bad.as_bytes(), 10).is_err(), "an edge to a missing vertex");
    }
}
