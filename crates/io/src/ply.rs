//! PLY (polygon file format) import, from the format's public description: a text header naming
//! elements and their properties, then the elements' rows in ASCII or binary (little- or
//! big-endian). The vertices (`x`, `y`, `z` of any numeric type, and `red`, `green`, `blue` when
//! present) become a point cloud, and the faces (`vertex_indices`) a mesh. Every other element
//! and property (normals, intensity, edges, …) is passed over. The file is untrusted: counts are
//! checked against its size and capped.

use solidcraft_geom::Vec3;

use crate::threemf::MeshObject;
use crate::{IoError, Result};

/// Largest header read (bytes).
const MAX_HEADER: usize = 1 << 20;
const MAX_ELEMENTS: usize = 64;
const MAX_PROPERTIES: usize = 256;
/// Most indices in one face.
const MAX_FACE: usize = 1024;
/// Most vertices and triangles of a mesh read from one file (as OBJ).
const MAX_MESH_VERTICES: usize = 20_000_000;
const MAX_TRIANGLES: usize = 20_000_000;

fn invalid(m: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("PLY: {m}"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(s: &str) -> Option<Scalar> {
        Some(match s {
            "char" | "int8" => Scalar::I8,
            "uchar" | "uint8" => Scalar::U8,
            "short" | "int16" => Scalar::I16,
            "ushort" | "uint16" => Scalar::U16,
            "int" | "int32" => Scalar::I32,
            "uint" | "uint32" => Scalar::U32,
            "float" | "float32" => Scalar::F32,
            "double" | "float64" => Scalar::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::F64 => 8,
        }
    }
    /// The value at the start of `b`.
    fn read(self, b: &[u8], big: bool) -> Option<f64> {
        fn take<const N: usize>(b: &[u8], big: bool) -> Option<[u8; N]> {
            let mut a: [u8; N] = b.get(..N)?.try_into().ok()?;
            if big {
                a.reverse();
            }
            Some(a)
        }
        Some(match self {
            Scalar::I8 => f64::from(i8::from_le_bytes(take(b, big)?)),
            Scalar::U8 => f64::from(*b.first()?),
            Scalar::I16 => f64::from(i16::from_le_bytes(take(b, big)?)),
            Scalar::U16 => f64::from(u16::from_le_bytes(take(b, big)?)),
            Scalar::I32 => f64::from(i32::from_le_bytes(take(b, big)?)),
            Scalar::U32 => f64::from(u32::from_le_bytes(take(b, big)?)),
            Scalar::F32 => f64::from(f32::from_le_bytes(take(b, big)?)),
            Scalar::F64 => f64::from_le_bytes(take(b, big)?),
        })
    }
    /// A colour channel of this type as a byte: integers by their range, floats from 0..1.
    fn channel(self, v: f64) -> u8 {
        let v = match self {
            Scalar::F32 | Scalar::F64 => v * 255.0,
            Scalar::U16 | Scalar::I16 => v / 257.0,
            Scalar::U32 | Scalar::I32 => v / 16_843_009.0,
            Scalar::U8 | Scalar::I8 => v,
        };
        if v.is_finite() { v.round().clamp(0.0, 255.0) as u8 } else { 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Scalar(Scalar),
    /// The count's type and the items' type.
    List(Scalar, Scalar),
}

#[derive(Clone, Debug)]
struct Property {
    name: String,
    kind: Kind,
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    count: u64,
    properties: Vec<Property>,
}

impl Element {
    /// Bytes per row in a binary file, when every property is a scalar.
    fn row_size(&self) -> Option<usize> {
        self.properties.iter().try_fold(0usize, |n, p| match p.kind {
            Kind::Scalar(s) => n.checked_add(s.size()),
            Kind::List(..) => None,
        })
    }
    fn find(&self, names: &[&str]) -> Option<usize> {
        names.iter().find_map(|n| self.properties.iter().position(|p| p.name.eq_ignore_ascii_case(n) && matches!(p.kind, Kind::Scalar(_))))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

/// What the header says, and where the rows start.
#[derive(Debug)]
struct Header {
    encoding: Encoding,
    elements: Vec<Element>,
    body: usize,
}

/// Does `bytes` start like a PLY file?
pub fn is_ply(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ply") && bytes.get(3).is_some_and(|b| matches!(b, b'\n' | b'\r'))
}

fn header(bytes: &[u8]) -> Result<Header> {
    if !is_ply(bytes) {
        return Err(invalid("not a PLY file (it must start with `ply`)"));
    }
    let head = bytes.get(..bytes.len().min(MAX_HEADER)).unwrap_or(bytes);
    let end = head.windows(10).position(|w| w == b"end_header").ok_or_else(|| invalid("no `end_header` in the first megabyte"))?;
    // The rows start after the end of that line (`\n`, `\r\n` or a lone `\r`).
    let mut body = end + 10;
    while head.get(body).is_some_and(|b| *b == b' ' || *b == b'\t') {
        body += 1;
    }
    match (head.get(body), head.get(body + 1)) {
        (Some(b'\r'), Some(b'\n')) => body += 2,
        (Some(b'\n' | b'\r'), _) => body += 1,
        (None, _) => {}
        _ => return Err(invalid("`end_header` is not on a line of its own")),
    }
    let text = String::from_utf8_lossy(head.get(..end).unwrap_or_default());
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in text.split(['\n', '\r']).skip(1) {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("format") => {
                encoding = Some(match it.next() {
                    Some("ascii") => Encoding::Ascii,
                    Some("binary_little_endian") => Encoding::Little,
                    Some("binary_big_endian") => Encoding::Big,
                    other => return Err(invalid(format!("unknown format `{}`", other.unwrap_or("")))),
                });
            }
            Some("element") => {
                let name = it.next().ok_or_else(|| invalid("an element without a name"))?.to_string();
                let count = it.next().and_then(|c| c.parse::<u64>().ok()).ok_or_else(|| invalid(format!("element `{name}` has no count")))?;
                if elements.len() >= MAX_ELEMENTS {
                    return Err(invalid("too many elements"));
                }
                elements.push(Element { name, count, properties: Vec::new() });
            }
            Some("property") => {
                let e = elements.last_mut().ok_or_else(|| invalid("a property before any element"))?;
                if e.properties.len() >= MAX_PROPERTIES {
                    return Err(invalid("too many properties"));
                }
                let first = it.next().unwrap_or("");
                let kind = if first == "list" {
                    let count = it.next().and_then(Scalar::parse).ok_or_else(|| invalid("a list property without a count type"))?;
                    let item = it.next().and_then(Scalar::parse).ok_or_else(|| invalid("a list property without an item type"))?;
                    if matches!(count, Scalar::F32 | Scalar::F64) {
                        return Err(invalid("a list's count must be an integer"));
                    }
                    Kind::List(count, item)
                } else {
                    Kind::Scalar(Scalar::parse(first).ok_or_else(|| invalid(format!("unknown property type `{first}`")))?)
                };
                let name = it.next().ok_or_else(|| invalid("a property without a name"))?.to_string();
                e.properties.push(Property { name, kind });
            }
            // `comment`, `obj_info` and blank lines.
            _ => {}
        }
    }
    let encoding = encoding.ok_or_else(|| invalid("no `format` line"))?;
    Ok(Header { encoding, elements, body })
}

/// A row's values, read one after another.
enum Rows<'a> {
    Binary { b: &'a [u8], pos: usize, big: bool },
    Ascii { tokens: std::slice::Split<'a, u8, fn(&u8) -> bool> },
}

fn space(b: &u8) -> bool {
    b.is_ascii_whitespace()
}

impl<'a> Rows<'a> {
    fn new(h: &Header, bytes: &'a [u8]) -> Rows<'a> {
        let b = bytes.get(h.body..).unwrap_or_default();
        match h.encoding {
            Encoding::Ascii => Rows::Ascii { tokens: b.split(space as fn(&u8) -> bool) },
            Encoding::Little => Rows::Binary { b, pos: 0, big: false },
            Encoding::Big => Rows::Binary { b, pos: 0, big: true },
        }
    }
    fn value(&mut self, t: Scalar) -> Result<f64> {
        match self {
            Rows::Binary { b, pos, big } => {
                let v = b.get(*pos..).and_then(|r| t.read(r, *big)).ok_or_else(|| invalid("the file ends early"))?;
                *pos += t.size();
                Ok(v)
            }
            Rows::Ascii { tokens } => loop {
                let tok = tokens.next().ok_or_else(|| invalid("the file ends early"))?;
                if tok.is_empty() {
                    continue;
                }
                let s = std::str::from_utf8(tok).map_err(|_| invalid("a value is not a number"))?;
                // Integers written as decimals (`255.0`) are taken too.
                return s.parse::<f64>().map_err(|_| invalid(format!("`{}` is not a number", s.chars().take(32).collect::<String>())));
            },
        }
    }
    /// Pass over `n` values of type `t`.
    fn skip(&mut self, t: Scalar, n: usize) -> Result<()> {
        match self {
            Rows::Binary { b, pos, .. } => {
                *pos = n
                    .checked_mul(t.size())
                    .and_then(|k| pos.checked_add(k))
                    .filter(|p| *p <= b.len())
                    .ok_or_else(|| invalid("the file ends early"))?;
                Ok(())
            }
            Rows::Ascii { .. } => {
                for _ in 0..n {
                    self.value(t)?;
                }
                Ok(())
            }
        }
    }
    fn count(&mut self, t: Scalar) -> Result<usize> {
        let n = self.value(t)?;
        if !(n >= 0.0 && n <= MAX_FACE as f64 && n.fract() == 0.0) {
            return Err(invalid("a list is longer than SolidCraft reads"));
        }
        Ok(n as usize)
    }
}

/// What to read from an element's rows.
struct Wanted {
    /// Property indices of x, y, z.
    xyz: Option<[usize; 3]>,
    /// Property indices of red, green, blue and their types.
    rgb: Option<[(usize, Scalar); 3]>,
    /// The face list's property index.
    faces: Option<usize>,
}

impl Wanted {
    fn of(e: &Element) -> Wanted {
        let scalar = |i: usize| match e.properties.get(i).map(|p| p.kind) {
            Some(Kind::Scalar(s)) => Some(s),
            _ => None,
        };
        let is_vertex = e.name == "vertex";
        let xyz = is_vertex.then(|| Some([e.find(&["x"])?, e.find(&["y"])?, e.find(&["z"])?])).flatten();
        let rgb = is_vertex
            .then(|| {
                let r = e.find(&["red", "r", "diffuse_red"])?;
                let g = e.find(&["green", "g", "diffuse_green"])?;
                let b = e.find(&["blue", "b", "diffuse_blue"])?;
                Some([(r, scalar(r)?), (g, scalar(g)?), (b, scalar(b)?)])
            })
            .flatten();
        let faces = (e.name == "face")
            .then(|| {
                e.properties.iter().position(|p| matches!(p.name.as_str(), "vertex_indices" | "vertex_index") && matches!(p.kind, Kind::List(..)))
            })
            .flatten();
        Wanted { xyz, rgb, faces }
    }
}

/// A vertex as read: its position (file units) and colour.
pub(crate) type Vertex = ([f64; 3], Option<[u8; 3]>);

/// A PLY file's vertices, rows `keep` says to keep, and their count in the file. Faces are read
/// into `faces` (a fan of triangles per face) when given.
struct Read {
    vertices: u64,
    colored: bool,
    bad_faces: usize,
}

fn walk(bytes: &[u8], keep: &mut dyn FnMut(u64) -> bool, on_vertex: &mut dyn FnMut(Vertex), mut faces: Option<&mut Vec<[u32; 3]>>) -> Result<Read> {
    let h = header(bytes)?;
    let vertex = h.elements.iter().find(|e| e.name == "vertex").ok_or_else(|| invalid("no `vertex` element"))?;
    let w = Wanted::of(vertex);
    if w.xyz.is_none() {
        return Err(invalid("the vertices have no `x`, `y` and `z`"));
    }
    let mut out = Read { vertices: vertex.count, colored: w.rgb.is_some(), bad_faces: 0 };
    let body = bytes.len().saturating_sub(h.body) as u64;
    let mut rows = Rows::new(&h, bytes);
    let mut slots = vec![0.0f64; MAX_PROPERTIES];
    let mut face: Vec<f64> = Vec::new();
    for e in &h.elements {
        let w = Wanted::of(e);
        let wanted_faces = w.faces.is_some() && faces.is_some();
        // A binary element of scalars that isn't needed (or has rows to pass over) is a block
        // of bytes: check its size against the file before reading any of it.
        if let (Rows::Binary { b, pos, .. }, Some(size)) = (&mut rows, e.row_size()) {
            let total = e
                .count
                .checked_mul(size as u64)
                .filter(|n| *n <= (b.len() - *pos) as u64)
                .ok_or_else(|| invalid(format!("the file is too short for its {} {}", e.count, e.name)))?;
            if w.xyz.is_none() {
                *pos += total as usize;
                continue;
            }
            // The vertices: only the rows kept are read.
            let start = *pos;
            let big = matches!(h.encoding, Encoding::Big);
            let offsets: Vec<usize> = e
                .properties
                .iter()
                .scan(0usize, |o, p| {
                    let at = *o;
                    *o += match p.kind {
                        Kind::Scalar(s) => s.size(),
                        Kind::List(..) => 0,
                    };
                    Some(at)
                })
                .collect();
            let get = |row: &[u8], i: usize| -> Option<f64> {
                match e.properties.get(i)?.kind {
                    Kind::Scalar(s) => s.read(row.get(*offsets.get(i)?..)?, big),
                    Kind::List(..) => None,
                }
            };
            for r in 0..e.count {
                if !keep(r) {
                    continue;
                }
                let at = start + (r as usize) * size;
                let row = b.get(at..at + size).ok_or_else(|| invalid("the file ends early"))?;
                let Some([x, y, z]) = w.xyz else { break };
                let p = [get(row, x), get(row, y), get(row, z)];
                let [Some(x), Some(y), Some(z)] = p else { return Err(invalid("the file ends early")) };
                let rgb = match w.rgb.map(|c| c.map(|(i, t)| get(row, i).map(|v| t.channel(v)))) {
                    Some([Some(r), Some(g), Some(b)]) => Some([r, g, b]),
                    _ => None,
                };
                on_vertex(([x, y, z], rgb));
            }
            *pos += total as usize;
            // A point cloud needs nothing after its vertices.
            if faces.is_none() {
                break;
            }
            continue;
        }
        // Row by row: ASCII, or rows with lists. A row takes at least a byte, so a count larger
        // than the file is damage, not a long loop.
        if e.count > body {
            return Err(invalid(format!("the file is too short for its {} {}", e.count, e.name)));
        }
        for r in 0..e.count {
            for (i, p) in e.properties.iter().enumerate() {
                match p.kind {
                    Kind::Scalar(t) => {
                        let v = rows.value(t)?;
                        if let Some(s) = slots.get_mut(i) {
                            *s = v;
                        }
                    }
                    Kind::List(c, t) => {
                        let n = rows.count(c)?;
                        if wanted_faces && w.faces == Some(i) {
                            face.clear();
                            for _ in 0..n {
                                face.push(rows.value(t)?);
                            }
                        } else {
                            rows.skip(t, n)?;
                        }
                    }
                }
            }
            if let Some([x, y, z]) = w.xyz {
                if keep(r) {
                    let v = |i: usize| slots.get(i).copied().unwrap_or(0.0);
                    let rgb = w.rgb.map(|c| c.map(|(i, t)| t.channel(v(i))));
                    on_vertex(([v(x), v(y), v(z)], rgb));
                }
            } else if wanted_faces && let Some(tris) = faces.as_deref_mut() {
                let n = out.vertices;
                let idx: Option<Vec<u32>> =
                    face.iter().map(|v| (*v >= 0.0 && *v < n as f64 && v.fract() == 0.0).then(|| u32::try_from(*v as u64).ok()).flatten()).collect();
                match idx {
                    Some(p) if p.len() >= 3 => {
                        for k in 1..p.len() - 1 {
                            tris.push([p[0], p[k], p[k + 1]]);
                        }
                        if tris.len() > MAX_TRIANGLES {
                            return Err(invalid(format!("more than {MAX_TRIANGLES} triangles")));
                        }
                    }
                    _ => out.bad_faces += 1,
                }
            }
        }
        if w.xyz.is_some() && faces.is_none() {
            break;
        }
    }
    Ok(out)
}

/// The vertices of a PLY file, every `stride`th one (`1`: all), and how many the file has.
/// The colour of each is `Some` when the file has colours.
pub(crate) fn read_vertices(bytes: &[u8], stride: u64, on_vertex: &mut dyn FnMut(Vertex)) -> Result<(u64, bool)> {
    let stride = stride.max(1);
    let r = walk(bytes, &mut |i| i.is_multiple_of(stride), on_vertex, None)?;
    Ok((r.vertices, r.colored))
}

/// How many vertices and faces a PLY file says it has (from its header alone).
pub fn ply_counts(bytes: &[u8]) -> Result<(u64, u64)> {
    let h = header(bytes)?;
    let count = |name: &str| h.elements.iter().find(|e| e.name == name).map_or(0, |e| e.count);
    Ok((count("vertex"), count("face")))
}

/// A PLY file's faces as a mesh named `name` (a fan of triangles per face; colours, normals and
/// texture coordinates are not read), with what could not be read.
pub fn read_ply_mesh(bytes: &[u8], name: &str) -> Result<(Vec<MeshObject>, Vec<String>)> {
    let (vertices, faces) = ply_counts(bytes)?;
    if faces == 0 {
        return Err(invalid("the file has no faces: it is a point cloud (insert it with pointcloud.insert)"));
    }
    if vertices > MAX_MESH_VERTICES as u64 {
        return Err(invalid(format!("more than {MAX_MESH_VERTICES} vertices")));
    }
    let mut positions: Vec<Vec3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut finite = true;
    let r = walk(
        bytes,
        &mut |_| true,
        &mut |([x, y, z], _)| {
            finite &= x.is_finite() && y.is_finite() && z.is_finite();
            positions.push(Vec3::new(x, y, z));
        },
        Some(&mut triangles),
    )?;
    if !finite {
        return Err(invalid("a vertex is not a finite number"));
    }
    if triangles.is_empty() {
        return Err(invalid("no face could be read"));
    }
    let mut warnings = Vec::new();
    if r.bad_faces > 0 {
        warnings.push(format!("{} face(s) could not be read and were left out", r.bad_faces));
    }
    Ok((vec![MeshObject { name: name.to_string(), positions, triangles, ..Default::default() }], warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertices(bytes: &[u8], stride: u64) -> Result<(Vec<Vertex>, u64, bool)> {
        let mut out = Vec::new();
        let (n, c) = read_vertices(bytes, stride, &mut |v| out.push(v))?;
        Ok((out, n, c))
    }

    fn binary(big: bool) -> Vec<u8> {
        let order = if big { "big" } else { "little" };
        let mut b = format!(
            "ply\r\nformat binary_{order}_endian 1.0\r\ncomment made by hand\r\nelement camera 1\r\nproperty double k\r\nelement vertex 3\r\nproperty float32 x\r\nproperty float32 y\r\nproperty float32 z\r\nproperty uchar red\r\nproperty uchar green\r\nproperty uchar blue\r\nproperty float intensity\r\nelement face 1\r\nproperty list uchar int vertex_indices\r\nend_header\r\n"
        )
        .into_bytes();
        let f = |v: f32| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        b.extend(if big { 9.5f64.to_be_bytes() } else { 9.5f64.to_le_bytes() });
        for (i, p) in [[0.0f32, 0.0, 0.0], [1.5, 0.0, 0.0], [0.0, 2.5, -3.0]].iter().enumerate() {
            for v in p {
                b.extend(f(*v));
            }
            b.extend([10 * i as u8, 20, 255]);
            b.extend(f(0.5));
        }
        b.push(3);
        for i in [0i32, 1, 2] {
            b.extend(if big { i.to_be_bytes() } else { i.to_le_bytes() });
        }
        b
    }

    #[test]
    fn binary_little_and_big_endian_with_colours_and_other_elements() {
        for big in [false, true] {
            let (v, n, colored) = vertices(&binary(big), 1).unwrap();
            assert_eq!((n, colored), (3, true));
            assert_eq!(v[1], ([1.5, 0.0, 0.0], Some([10, 20, 255])));
            assert_eq!(v[2].0, [0.0, 2.5, -3.0]);
            // Every second vertex.
            let (v, _, _) = vertices(&binary(big), 2).unwrap();
            assert_eq!(v.iter().map(|v| v.0[1]).collect::<Vec<_>>(), [0.0, 2.5]);
            let (mesh, warnings) = read_ply_mesh(&binary(big), "Tri").unwrap();
            assert!(warnings.is_empty());
            assert_eq!(mesh[0].triangles, [[0, 1, 2]]);
            assert_eq!(ply_counts(&binary(big)).unwrap(), (3, 1));
        }
    }

    #[test]
    fn ascii_with_float_colours_lists_and_a_quad() {
        let src = "ply\nformat ascii 1.0\nelement vertex 4\nproperty double x\nproperty double y\nproperty double z\nproperty float red\nproperty float green\nproperty float blue\n\
                   element face 1\nproperty list uchar uint vertex_index\nend_header\n\
                   0 0 0 1 0 0\n1 0 0 0 1 0\n1 1 0 0 0 1\n0 1 0 0.5 0.5 0.5\n4 0 1 2 3\n";
        let (v, n, colored) = vertices(src.as_bytes(), 1).unwrap();
        assert_eq!((n, colored, v.len()), (4, true, 4));
        assert_eq!(v[0].1, Some([255, 0, 0]));
        assert_eq!(v[3], ([0.0, 1.0, 0.0], Some([128, 128, 128])));
        let (mesh, _) = read_ply_mesh(src.as_bytes(), "Quad").unwrap();
        assert_eq!(mesh[0].triangles, [[0, 1, 2], [0, 2, 3]]);
        // Only points (as the glim maps are written): no colours, no faces.
        let points =
            "ply\nformat ascii 1.0\nelement vertex 2\nproperty float32 x\nproperty float32 y\nproperty float32 z\nend_header\n1 2 3\n4 5 6\n";
        let (v, n, colored) = vertices(points.as_bytes(), 1).unwrap();
        assert_eq!((v.len(), n, colored), (2, 2, false));
        assert_eq!(v[1], ([4.0, 5.0, 6.0], None));
        assert!(read_ply_mesh(points.as_bytes(), "x").is_err(), "a point cloud is not a mesh");
    }

    #[test]
    fn hostile_ply_never_panics() {
        let good = binary(false);
        let mut cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"ply".to_vec(),
            b"ply\nformat ascii 1.0\n".to_vec(),
            b"ply\nformat ascii 1.0\nelement vertex 18446744073709551615\nproperty float x\nproperty float y\nproperty float z\nend_header\n1 2 3".to_vec(),
            b"ply\nformat binary_little_endian 1.0\nelement vertex 18446744073709551615\nproperty float x\nproperty float y\nproperty float z\nend_header\n1234".to_vec(),
            b"ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty list uchar float x\nproperty float y\nproperty float z\nend_header\n\xff".to_vec(),
            b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nend_header\n1".to_vec(),
            b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 1 0 0 0 1 0 3 0 1 99\n".to_vec(),
            b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nend_header\nnan inf -inf\n".to_vec(),
            b"ply\nformat ascii 1.0\nproperty float x\nend_header\n".to_vec(),
            b"ply\nformat binary_big_endian 1.0\nelement face 2\nproperty list float int vertex_indices\nend_header\n".to_vec(),
        ];
        // Every truncation of a good file.
        for n in 0..good.len() {
            cases.push(good[..n].to_vec());
        }
        for c in &cases {
            let r = std::panic::catch_unwind(|| {
                let _ = vertices(c, 1);
                let _ = vertices(c, 3);
                let _ = read_ply_mesh(c, "x");
                let _ = ply_counts(c);
            });
            assert!(r.is_ok(), "{:?}", String::from_utf8_lossy(c));
        }
        assert!(vertices(&good[..good.len() - 30], 1).is_err(), "a cut-off file is refused");
        // A face naming a vertex that isn't there is left out, with a warning.
        let tri = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 2\nproperty list uchar int vertex_indices\nend_header\n0 0 0 1 0 0 0 1 0\n3 0 1 2\n3 0 1 99\n";
        let (mesh, warnings) = read_ply_mesh(tri.as_bytes(), "x").unwrap();
        assert_eq!((mesh[0].triangles.len(), warnings.len()), (1, 1));
    }
}
