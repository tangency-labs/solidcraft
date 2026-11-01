use std::f64::consts::TAU;
use std::sync::Arc;

use solidcraft_geom::{Mesh, Vec3};

use solidcraft_testkit::{mutations, survives};

use super::{MAX_DECODED_BYTES, MAX_TRIANGLES, MAX_VERTICES};
use crate::{FaceChunk, Hash, Held, Shape, WireError, declared_size};

/// A closed cylinder as a B-rep face tessellation gives it: the side, with smooth normals, and
/// two flat caps, each face with vertices of its own.
fn cylinder(r: f64, h: f64, n: u32, at: Vec3) -> Mesh {
    let mut m = Mesh::default();
    let ring = |k: u32| (TAU * f64::from(k) / f64::from(n)).sin_cos();
    for k in 0..n {
        let (s, c) = ring(k);
        for z in [0.0, h] {
            m.positions.push(at + Vec3::new(r * c, r * s, z));
            m.normals.push(Vec3::new(c, s, 0.0));
        }
    }
    for k in 0..n {
        let (a, b) = (2 * k, 2 * ((k + 1) % n));
        m.triangles.extend([[a, b, a + 1], [a + 1, b, b + 1]]);
        m.tri_face.extend([0, 0]);
    }
    for (face, z, nz) in [(1, 0.0, -1.0), (2, h, 1.0)] {
        let centre = m.positions.len() as u32;
        m.positions.push(at + Vec3::new(0.0, 0.0, z));
        m.normals.push(Vec3::new(0.0, 0.0, nz));
        for k in 0..n {
            let (s, c) = ring(k);
            m.positions.push(at + Vec3::new(r * c, r * s, z));
            m.normals.push(Vec3::new(0.0, 0.0, nz));
        }
        for k in 0..n {
            let (a, b) = (centre + 1 + k, centre + 1 + (k + 1) % n);
            m.triangles.push(if nz > 0.0 { [centre, a, b] } else { [centre, b, a] });
            m.tri_face.push(face);
        }
    }
    m
}

const NAMES: [&str; 3] = ["F1:side", "F1:bottom", "F1:top"];
/// The same, in name order: the order a shape keeps its faces in.
const SORTED: [&str; 3] = ["F1:bottom", "F1:side", "F1:top"];

fn sample() -> Mesh {
    cylinder(12.345_678_9, 30.000_4, 64, Vec3::new(100.123_456_7, -50.5, 3.3))
}

fn send(m: &Mesh, names: &[&str]) -> Vec<u8> {
    Shape::from_mesh(m, names).unwrap().encode(|_| false)
}

/// Reads a message on a side that holds nothing.
fn receive(bytes: &[u8]) -> Result<Shape, WireError> {
    Shape::decode(bytes, &())
}

/// A side that holds these shapes, and their faces.
struct Holds(Vec<Arc<Shape>>);

impl Held for Holds {
    fn shape(&self, hash: &Hash) -> Option<Arc<Shape>> {
        self.0.iter().find(|s| s.hash() == hash).cloned()
    }

    fn face(&self, hash: &Hash) -> Option<Arc<FaceChunk>> {
        self.0.iter().flat_map(|s| s.faces()).find(|c| c.hash() == hash).cloned()
    }
}

/// A side whose store answers every hash, with this shape and its first face whatever was asked.
struct Lies(Arc<Shape>);

impl Held for Lies {
    fn shape(&self, _: &Hash) -> Option<Arc<Shape>> {
        Some(self.0.clone())
    }

    fn face(&self, _: &Hash) -> Option<Arc<FaceChunk>> {
        self.0.faces().first().cloned()
    }
}

/// The face of `shape` named `name`.
fn face<'s>(shape: &'s Shape, name: &str) -> &'s Arc<FaceChunk> {
    shape.faces().iter().find(|c| c.name() == name).unwrap()
}

/// The angle between two directions, in radians.
fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).len().atan2(a.dot(b))
}

/// The worst angle a normal may come back off by: 1e-4 rad (0.006°). The octahedral grid's
/// 2 x 16 bits, rounded to nearest, measure 6.3e-5 rad at worst over the test's directions.
const NORMAL_BOUND: f64 = 1e-4;

/// Each face (by name) comes back as the same triangles, each turning the same way (in any
/// order, from any corner), with corners within half a micrometre and normals within
/// [`NORMAL_BOUND`]. `names` are the sent mesh's face names.
fn assert_same_within(sent: &Mesh, names: &[&str], got: &Shape) {
    // Per face name, its triangles as corners (position, normal), each from its least corner on
    // the 1 µm grid, sorted by those grid points.
    let faces = |m: &Mesh, name: &dyn Fn(u32) -> String| {
        let grid = |p: Vec3| [p.x, p.y, p.z].map(|x| (x * 1e3).round() as i64);
        let mut by_face: std::collections::BTreeMap<String, Vec<[(Vec3, Vec3); 3]>> = Default::default();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let mut tri = t.map(|i| (m.positions[i as usize], m.normals[i as usize]));
            let least = (0..3).min_by_key(|&k| grid(tri[k].0)).unwrap();
            tri.rotate_left(least);
            by_face.entry(name(*f)).or_default().push(tri);
        }
        for tris in by_face.values_mut() {
            tris.sort_by_key(|t| t.map(|c| grid(c.0)));
        }
        by_face
    };
    let mesh = got.to_mesh().unwrap();
    let a = faces(sent, &|f| names[f as usize].to_owned());
    let b = faces(&mesh, &|f| got.faces()[f as usize].name().to_owned());
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "faces");
    for ((f, ta), tb) in a.iter().zip(b.values()) {
        assert_eq!(ta.len(), tb.len(), "face {f}: triangles");
        for ((p, n), (q, m)) in ta.iter().flatten().zip(tb.iter().flatten()) {
            for (u, v) in [(p.x, q.x), (p.y, q.y), (p.z, q.z)] {
                assert!((u - v).abs() <= 0.5e-3 + 1e-12, "face {f}: {u} came back as {v}");
            }
            let n = n.normalized().unwrap();
            assert!(angle(n, *m) <= NORMAL_BOUND, "face {f}: normal {n:?} came back as {m:?}");
            assert!((m.len() - 1.0).abs() < 1e-12, "a unit normal");
        }
    }
}

#[test]
fn a_mesh_comes_back_face_by_face_within_half_a_micrometre() {
    let m = sample();
    let got = receive(&send(&m, &NAMES)).unwrap();
    assert_eq!(got.faces().iter().map(|f| f.name()).collect::<Vec<_>>(), SORTED, "faces in name order");
    assert_same_within(&m, &NAMES, &got);
}

#[test]
fn a_mesh_without_face_ids_or_normals_is_one_face_without_normals() {
    let mut m = sample();
    m.tri_face.clear();
    m.normals.clear();
    let shape = Shape::from_mesh(&m, &["Body1:face:0"]).unwrap();
    assert_eq!(shape.faces().len(), 1);
    let got = receive(&shape.encode(|_| false)).unwrap().to_mesh().unwrap();
    assert!(got.normals.is_empty() && got.tri_face.iter().all(|&f| f == 0));
    assert_eq!(got.triangles.len(), m.triangles.len());
    // Nothing at all is a shape without faces.
    let empty = receive(&send(&Mesh::default(), &[])).unwrap();
    assert!(empty.faces().is_empty() && empty.to_mesh().unwrap().positions.is_empty());
}

/// Inflates a message: its declared length, then raw deflate.
fn inflate(bytes: &[u8]) -> Vec<u8> {
    let mut r = crate::bytes::Reader::new(bytes);
    let n = r.varint().unwrap() as usize;
    let raw = miniz_oxide::inflate::decompress_to_vec(r.rest()).unwrap();
    assert_eq!(raw.len(), n, "the declared size");
    raw
}

fn sha256(parts: &[&[u8]]) -> Hash {
    use sha2::Digest;
    parts.iter().fold(sha2::Sha256::new(), |h, p| h.chain_update(p)).finalize().into()
}

#[test]
fn a_face_goes_as_the_format_says_and_hashes_its_own_bytes() {
    let m = Mesh {
        positions: vec![Vec3::new(0.001, -0.002, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)],
        normals: vec![Vec3::Z; 3],
        triangles: vec![[0, 1, 2]],
        tri_face: vec![0],
        ..Default::default()
    };
    let shape = Shape::from_mesh(&m, &["F1:top"]).unwrap();
    // Worked out by hand from the module's description. The triangle starts at its least corner,
    // (0, 2000, 0) µm, and goes on the same way round.
    let chunk: &[u8] = &[
        6, b'F', b'1', b':', b't', b'o', b'p', // name
        3, 1, 1, // 3 vertices, 1 triangle, normals
        0, 0xa0, 0x1f, 0, // (0, 2000, 0) µm
        2, 0xa3, 0x1f, 0, // (+1, -2002, 0)
        0xce, 0x0f, 4, 0, // (+999, +2, 0)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // +Z three times
        0, 2, 2, // corners 0, +1, +1
    ];
    // A whole shape (0), one face, inline (0).
    assert_eq!(inflate(&shape.encode(|_| false)), [&[0, 1, 0], chunk].concat());
    let face = sha256(&[b"solidcraft-wire/face\0", chunk]);
    assert_eq!(shape.faces()[0].hash(), &face);
    assert_eq!(shape.hash(), &sha256(&[b"solidcraft-wire/shape\0", &face]));
}

/// `m` with its vertices numbered the other way round (the same triangles).
fn renumbered(m: &Mesh) -> Mesh {
    let n = m.positions.len() as u32;
    Mesh {
        positions: m.positions.iter().rev().copied().collect(),
        normals: m.normals.iter().rev().copied().collect(),
        triangles: m.triangles.iter().map(|t| t.map(|i| n - 1 - i)).collect(),
        tri_face: m.tri_face.clone(),
        ..Default::default()
    }
}

/// `m` with each face's triangles in the reverse order, each starting at its next corner (the
/// same turning sense): a mesher that walks the face another way.
fn reordered(m: &Mesh) -> Mesh {
    let mut order: Vec<usize> = (0..m.triangles.len()).collect();
    order.sort_by_key(|&t| (m.tri_face[t], std::cmp::Reverse(t)));
    Mesh {
        triangles: order.iter().map(|&t| [m.triangles[t][1], m.triangles[t][2], m.triangles[t][0]]).collect(),
        tri_face: order.iter().map(|&t| m.tri_face[t]).collect(),
        ..m.clone()
    }
}

/// `m` as a triangle soup: three vertices of its own for every triangle.
fn unwelded(m: &Mesh) -> Mesh {
    let corners: Vec<u32> = m.triangles.iter().flatten().copied().collect();
    Mesh {
        positions: corners.iter().map(|&i| m.positions[i as usize]).collect(),
        normals: corners.iter().map(|&i| m.normals[i as usize]).collect(),
        triangles: (0..m.triangles.len() as u32).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect(),
        ..m.clone()
    }
}

fn hashes(m: &Mesh, names: &[&str]) -> (Hash, Vec<Hash>) {
    let s = Shape::from_mesh(m, names).unwrap();
    (*s.hash(), s.faces().iter().map(|f| *f.hash()).collect())
}

#[test]
fn the_content_hash_depends_only_on_the_quantised_faces_and_names() {
    // On the grid exactly, so a nudge below half a micrometre rounds back.
    let mut m = cylinder(10.0, 20.0, 32, Vec3::new(1.0, 2.0, 3.0));
    for p in &mut m.positions {
        *p = Vec3::new((p.x * 1e3).round() / 1e3, (p.y * 1e3).round() / 1e3, (p.z * 1e3).round() / 1e3);
    }
    let (shape, faces) = hashes(&m, &NAMES);
    assert_eq!(hashes(&m, &NAMES), (shape, faces.clone()), "stable");
    assert_eq!(hashes(&renumbered(&m), &NAMES), (shape, faces.clone()), "the mesh's vertex numbering doesn't count");
    assert_eq!(hashes(&reordered(&m), &NAMES), (shape, faces.clone()), "nor the order of a face's triangles, or where each starts");
    assert_eq!(hashes(&unwelded(&m), &NAMES), (shape, faces.clone()), "nor whether its corners share vertices");
    let mut nudged = m.clone();
    nudged.positions.iter_mut().for_each(|p| *p += Vec3::new(0.000_4, -0.000_4, 0.000_2));
    assert_eq!(hashes(&nudged, &NAMES), (shape, faces.clone()), "under half a micrometre is the same grid point");
    // One micrometre on the top cap: that face and the shape change, the others don't.
    let mut moved = m.clone();
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if *f == 2 {
            for &i in t {
                moved.positions[i as usize] = m.positions[i as usize] + Vec3::new(0.0, 0.0, 0.001);
            }
        }
    }
    let (s2, f2) = hashes(&moved, &NAMES);
    assert!(s2 != shape && f2[0] == faces[0] && f2[1] == faces[1] && f2[2] != faces[2]);
    // A face renamed: only its hash changes.
    let (s3, f3) = hashes(&m, &["F1:side", "F1:bottom", "F9:top"]);
    assert!(s3 != shape && f3[..2] == faces[..2] && f3[2] != faces[2]);
}

#[test]
fn normals_come_back_within_the_octahedral_bound() {
    // Directions all over the sphere, the axes, both signs of zero, and next to the fold.
    let mut dirs = vec![Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z, Vec3::new(-0.0, -0.0, -1.0), Vec3::new(1.0, 1.0, -1e-9)];
    for i in 0..20_000 {
        // A Fibonacci sphere, at length 3: normals need not be unit.
        let z = 1.0 - 2.0 * (f64::from(i) + 0.5) / 20_000.0;
        let a = f64::from(i) * 2.399_963_229_728_653;
        let r = (1.0 - z * z).sqrt();
        dirs.push(Vec3::new(r * a.cos(), r * a.sin(), z) * 3.0);
    }
    let n = dirs.len() as u32;
    let m = Mesh {
        positions: (0..n).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect(),
        normals: dirs.clone(),
        triangles: (0..n).map(|i| [i, (i + 1) % n, (i + 2) % n]).collect(),
        ..Default::default()
    };
    let got = receive(&send(&m, &["F1:sphere"])).unwrap().to_mesh().unwrap();
    // Vertex i sits at x = i.
    assert_eq!(got.normals.len(), dirs.len());
    let sent = |p: &Vec3| dirs[p.x.round() as usize].normalized().unwrap();
    let worst = got.positions.iter().zip(&got.normals).map(|(p, n)| angle(sent(p), *n)).fold(0.0, f64::max);
    eprintln!("octahedral 2 x 16 bit: worst normal error {worst:.2e} rad over {n} directions");
    assert!(worst <= NORMAL_BOUND, "{worst}");
    // A zero normal goes as +Z; one that isn't finite is refused.
    let mut zero = m.clone();
    zero.normals[0] = Vec3::ZERO;
    let got = receive(&send(&zero, &["F"])).unwrap().to_mesh().unwrap();
    assert_eq!(got.positions.iter().zip(&got.normals).find(|(p, _)| p.x == 0.0).map(|(_, n)| *n), Some(Vec3::Z));
    zero.normals[0] = Vec3::new(f64::NAN, 0.0, 1.0);
    assert!(Shape::from_mesh(&zero, &["F"]).is_err());
}

/// The sample with its top cap 1 mm higher: the top changes, and the side (its top ring
/// moves); the bottom doesn't.
fn raised() -> Mesh {
    let mut m = sample();
    let top: Vec<u32> = m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f == 2).flat_map(|(t, _)| *t).collect();
    for i in top {
        m.positions[i as usize].z += 1.0;
    }
    for p in m.positions.iter_mut().skip(1).step_by(2).take(64) {
        p.z += 1.0;
    }
    m
}

#[test]
fn faces_the_receiver_holds_go_as_their_hash() {
    let before = Arc::new(Shape::from_mesh(&sample(), &NAMES).unwrap());
    let after = Shape::from_mesh(&raised(), &NAMES).unwrap();
    let changed: Vec<bool> = SORTED.iter().map(|n| face(&after, n) != face(&before, n)).collect();
    assert_eq!(changed, [false, true, true], "bottom, side, top");
    let holds = Holds(vec![before.clone()]);
    let delta = after.encode(|h| holds.face(h).is_some());
    let whole = after.encode(|_| false);
    assert!(delta.len() < whole.len(), "{} of {}", delta.len(), whole.len());
    let got = Shape::decode(&delta, &holds).unwrap();
    assert_eq!(got, after);
    assert!(Arc::ptr_eq(face(&got, "F1:bottom"), face(&before, "F1:bottom")), "the held chunk itself, not a copy");
    assert_same_within(&raised(), &NAMES, &got);
    // Without the held face it can't be read.
    assert_eq!(receive(&delta), Err(WireError("a face chunk this side does not hold")));
    // A face no longer than its hash always goes whole.
    let one = Mesh { positions: vec![Vec3::ZERO; 3], triangles: vec![[0, 1, 2]], ..Default::default() };
    let tiny = Shape::from_mesh(&one, &["F"]).unwrap();
    assert_eq!(tiny.encode(|_| true), tiny.encode(|_| false));
}

#[test]
fn a_delta_names_only_the_faces_that_changed() {
    let before = Arc::new(Shape::from_mesh(&sample(), &NAMES).unwrap());
    let after = Shape::from_mesh(&raised(), &NAMES).unwrap();
    let holds = Holds(vec![before.clone()]);
    let delta = after.encode_delta(&before, |_| false);
    assert_eq!(Shape::decode(&delta, &holds), Ok(after.clone()), "the same shape as sent whole");
    assert_eq!(Shape::decode(&after.encode(|_| false), &holds), Ok(after.clone()));
    // Unchanged faces cost nothing, not even their hash: ten more of them leave the delta as it
    // was, and add 33 bytes each when held faces go by hash.
    let ten = |m: Mesh| {
        let mut m = m;
        for k in 0..10u32 {
            let at = m.positions.len() as u32;
            m.positions.extend([Vec3::new(f64::from(k), 500.0, 0.0), Vec3::new(f64::from(k), 501.0, 0.0), Vec3::new(f64::from(k), 500.0, 1.0)]);
            m.normals.extend([Vec3::X; 3]);
            m.triangles.push([at, at + 1, at + 2]);
            m.tri_face.push(3 + k);
        }
        m
    };
    let more: Vec<String> = NAMES.iter().map(|n| n.to_string()).chain((0..10).map(|k| format!("F7:extra:{k}"))).collect();
    let (more_before, more_after) = (Shape::from_mesh(&ten(sample()), &more).unwrap(), Shape::from_mesh(&ten(raised()), &more).unwrap());
    assert_eq!(inflate(&more_after.encode_delta(&more_before, |_| false)).len(), inflate(&delta).len());
    let by_hash = |a: &Shape, b: &Shape| inflate(&a.encode(|h| b.faces().iter().any(|c| c.hash() == h))).len();
    assert_eq!(by_hash(&more_after, &more_before), by_hash(&after, &before) + 10 * 33);
    // Changed faces may go by hash too, when held from elsewhere.
    let elsewhere = Holds(vec![before.clone(), Arc::new(after.clone())]);
    let tiny = after.encode_delta(&before, |h| elsewhere.face(h).is_some());
    assert!(tiny.len() < delta.len() / 4);
    assert_eq!(Shape::decode(&tiny, &elsewhere), Ok(after.clone()));
    // The same shape: an empty delta.
    let same = before.encode_delta(&before, |_| false);
    assert_eq!(inflate(&same), [&[1][..], before.hash(), &[0, 0]].concat(), "the base and two empty lists");
    assert_eq!(Shape::decode(&same, &holds).as_ref().map(Shape::hash), Ok(before.hash()));
    // A face removed and one added: the bottom goes, "F2:lid" comes.
    let m = raised();
    let names = ["F1:side", "F2:lid", "F1:top"];
    let other = Shape::from_mesh(&m, &names).unwrap();
    assert_eq!(other.faces().iter().map(|c| c.name()).collect::<Vec<_>>(), ["F1:side", "F1:top", "F2:lid"]);
    let delta = other.encode_delta(&before, |_| false);
    let got = Shape::decode(&delta, &holds).unwrap();
    assert_eq!(got, other);
    assert_same_within(&m, &names, &got);
    // A face that comes before the kept ones by name: the result is still in name order.
    let first = Shape::from_mesh(&sample(), &["F1:side", "F0:new", "F1:top"]).unwrap();
    assert_eq!(Shape::decode(&first.encode_delta(&before, |_| false), &holds), Ok(first));
    // Without the base, the sender falls back to the whole shape.
    assert_eq!(receive(&delta), Err(WireError("a delta against a shape this side does not hold")));
}

/// Deflates a hand-made message behind its true length.
fn deflated(raw: &[u8]) -> Vec<u8> {
    declaring(raw.len(), raw)
}

/// Deflates a hand-made message behind the length `n`, true or not.
fn declaring(n: usize, raw: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    crate::bytes::put_varint(&mut b, n as u64);
    b.extend(miniz_oxide::deflate::compress_to_vec(raw, 6));
    b
}

/// A shape message of one inline face: `name`, then the rest of the chunk as given.
fn one_face(name: &[u8], rest: &[u8]) -> Vec<u8> {
    deflated(&[&[0, 1, 0, name.len() as u8], name, rest].concat())
}

/// A delta against `base` that changes one face to the given inline chunk: `name`, then the rest.
fn one_face_delta(base: &Hash, name: &[u8], rest: &[u8]) -> Vec<u8> {
    deflated(&[&[1][..], base, &[0, 1, 0, name.len() as u8], name, rest].concat())
}

/// One good face: 3 vertices, 1 triangle, no normals (after its name).
const GOOD: &[u8] = &[3, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0, 0, 2, 2];

/// Faces that lie (name, the rest of the chunk), each with the reason it is refused for.
fn face_lies() -> Vec<(Vec<u8>, Vec<u8>, &'static str)> {
    // A coordinate one grid step past 1e9 mm (zig-zag 2e12 + 2).
    let mut far = vec![3, 1, 0];
    crate::bytes::put_varint(&mut far, 2_000_000_000_002);
    far.extend([3, 0, 4, 0, 0, 0, 4, 0, 0, 2, 2]);
    let f = || b"F".to_vec();
    vec![
        (f(), vec![3, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0, 0, 2], "truncated"),
        (vec![0xc3, 0x28], GOOD.to_vec(), "a face name not UTF-8"),
        (f(), vec![3, 1, 2, 2, 3, 0, 4, 0, 0, 0, 4, 0, 0, 2, 2], "the normals flag"),
        (f(), vec![3, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0, 0, 2, 4], "a triangle corner out of range"),
        (f(), vec![3, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0, 1, 2, 2], "a triangle corner out of range"),
        (f(), vec![0x83, 0, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0, 0, 2, 2], "varint not in its shortest form"),
        (f(), far, "a coordinate past 1e9 mm"),
        // Corners 0, 0, 2: nothing to draw, and nothing an honest sender writes.
        (f(), vec![3, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0, 0, 0, 4], "a triangle with a repeated corner"),
        (f(), vec![3, 1, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f, 3, 0], "varint over 64 bits"),
        // Counts past the caps, and under them but more than the bytes left could hold. Nothing
        // is reserved for them.
        (f(), vec![0xff, 0xff, 0xff, 0xff, 0x0f, 1, 0, 2, 3, 0], "too many vertices"),
        (f(), vec![3, 0xff, 0xff, 0xff, 0xff, 0x0f, 0, 2, 3, 0], "too many triangles"),
        (f(), vec![0xe8, 0x07, 1, 0, 2, 3, 0, 4, 0, 0, 0, 4, 0], "too many vertices"),
    ]
}

#[test]
fn each_lie_in_a_shape_message_is_refused() {
    assert!(receive(&one_face(b"F", GOOD)).is_ok());
    // Each refused for its own reason.
    let refused = |bytes: &[u8], why: &'static str| assert_eq!(receive(bytes), Err(WireError(why)));
    for (name, rest, why) in face_lies() {
        refused(&one_face(&name, &rest), why);
    }
    refused(b"", "truncated");
    refused(&[0xff; 64], "varint over 64 bits");
    refused(&[&[10][..], &[0xff; 64]].concat(), "not deflate");
    refused(&deflated(&[2, 0]), "a message form");
    refused(&deflated(&[&[0, 1, 0, 1, b'F'][..], GOOD, &[0]].concat()), "bytes after the last face");
    refused(&deflated(&[&[0, 2, 0, 1, b'F'][..], GOOD].concat()), "truncated");
    refused(&deflated(&[&[0, 1, 2, 1, b'F'][..], GOOD].concat()), "a face tag");
    refused(&deflated(&[0, 0xff, 0xff, 0xff, 0xff, 0x0f, 0]), "too many faces");
    refused(&deflated(&[0, 0x10, 0]), "too many faces");
    refused(&deflated(&[0, 1, 0, 0xff, 0xff, 0xff, 0x7f]), "a face name over 4 KiB");
    // 4097 bytes long.
    refused(&deflated(&[&[0, 1, 0, 0x81, 0x20][..], &[b'x'; 4097], GOOD].concat()), "a face name over 4 KiB");
    refused(&deflated(&[&[0, 1, 1][..], &[7; 32]].concat()), "a face chunk this side does not hold");
    // Two faces with one name, or out of name order: a shape keeps its faces by name.
    refused(&deflated(&[&[0, 2, 0, 1, b'F'][..], GOOD, &[0, 1, b'F'], GOOD].concat()), "faces out of name order, or a name twice");
    refused(&deflated(&[&[0, 2, 0, 1, b'G'][..], GOOD, &[0, 1, b'F'], GOOD].concat()), "faces out of name order, or a name twice");
    // A message is exactly as long as it says: one byte more or less is refused.
    let good = [&[0, 1, 0, 1, b'F'][..], GOOD].concat();
    assert!(receive(&deflated(&good)).is_ok());
    refused(&declaring(good.len() - 1, &good), "inflates past its declared size");
    refused(&declaring(good.len() + 1, &good), "inflates short of its declared size");
    // A decompression bomb: 65 MiB of zeros in about 64 KB. Said truthfully, its size is past
    // the cap and nothing is inflated; told as 20 bytes, inflating stops at the 21st.
    let zeros = vec![0; MAX_DECODED_BYTES + (1 << 20)];
    let bomb = deflated(&zeros);
    assert!(bomb.len() < 100_000);
    assert_eq!(declared_size(&bomb), Err(WireError("a declared size past the cap")));
    refused(&bomb, "a declared size past the cap");
    let lying = declaring(20, &zeros);
    let t = std::time::Instant::now();
    refused(&lying, "inflates past its declared size");
    eprintln!("bomb: {} bytes refused in {:?}", bomb.len(), t.elapsed());
    // Inflating stops at the declared size plus one byte, before what comes later is read: here
    // 100 stored bytes, then a block of the reserved type, which only a decoder that read on
    // would reach.
    let stored = [&[20, 0, 100, 0, 0x9b, 0xff][..], &[0; 100], &[0x07]].concat();
    refused(&stored, "inflates past its declared size");
    refused(&[&[100][..], &stored[1..]].concat(), "not deflate");
}

#[test]
fn each_lie_in_a_delta_is_refused() {
    let base = Arc::new(Shape::from_mesh(&sample(), &NAMES).unwrap());
    let holds = Holds(vec![base.clone()]);
    let h = base.hash();
    let refused = |raw: &[u8], why: &'static str| assert_eq!(Shape::decode(&deflated(raw), &holds), Err(WireError(why)));
    assert!(Shape::decode(&one_face_delta(h, b"F", GOOD), &holds).is_ok());
    for (name, rest, why) in face_lies() {
        assert_eq!(Shape::decode(&one_face_delta(h, &name, &rest), &holds), Err(WireError(why)), "{why}");
    }
    let removes = |names: &[&str]| {
        let mut raw = [&[1][..], h, &[names.len() as u8]].concat();
        for n in names {
            raw.push(n.len() as u8);
            raw.extend(n.as_bytes());
        }
        raw
    };
    refused(&[&[1][..], &[9; 32], &[0, 0]].concat(), "a delta against a shape this side does not hold");
    refused(&[&[1][..], &h[..20]].concat(), "truncated");
    refused(&[removes(&["F1:top"]), vec![0, 0]].concat(), "bytes after the last face");
    refused(&[removes(&["F9:nope"]), vec![0]].concat(), "a delta removes a face its base lacks");
    refused(&[removes(&["F1:top", "F1:top"]), vec![0]].concat(), "faces out of name order, or a name twice");
    refused(&[removes(&["F1:top", "F1:side"]), vec![0]].concat(), "faces out of name order, or a name twice");
    refused(&[removes(&["F1:a", "F1:b", "F1:c", "F1:d"]), vec![0]].concat(), "too many faces");
    // A face changed twice, and one both removed and changed.
    refused(&[&removes(&[])[..], &[2, 0, 1, b'F'], GOOD, &[0, 1, b'F'], GOOD].concat(), "faces out of name order, or a name twice");
    refused(&[&removes(&["F1:top"])[..], &[1, 0, 6], b"F1:top", GOOD].concat(), "a delta removes and changes one face");
    refused(&[&removes(&[])[..], &[1, 1], &[7; 32]].concat(), "a face chunk this side does not hold");
    refused(&[&removes(&[])[..], &[1, 2], GOOD].concat(), "a face tag");
    refused(&[&removes(&[])[..], &[0xff, 0xff, 0xff, 0xff, 0x0f]].concat(), "too many faces");
    // A store that hands back something under another hash than the one asked for is not
    // believed.
    let lies = Lies(base.clone());
    assert_eq!(Shape::decode(&deflated(&[&[1][..], h, &[0, 0]].concat()), &lies).as_ref().map(Shape::hash), Ok(h));
    let lied = |raw: &[u8], why: &'static str| assert_eq!(Shape::decode(&deflated(raw), &lies), Err(WireError(why)));
    lied(&[&[1][..], &[9; 32], &[0, 0]].concat(), "a delta against a shape this side does not hold");
    lied(&[&[0, 1, 1][..], &[7; 32]].concat(), "a face chunk this side does not hold");
}

#[test]
fn a_face_past_the_vertex_cap_is_refused_as_it_is_cut() {
    // One face of `n` different corners (the last triangle filled up with the first corner).
    // Cutting it reserves room for the cap's worth of vertices at most, not three a triangle.
    let tris = |n: usize| -> Vec<[super::Corner; 3]> {
        let corner = |k: usize| ([if k < n { k as i64 } else { 0 }, 0, 0], [0i16; 2]);
        (0..n.div_ceil(3)).map(|t| [corner(3 * t), corner(3 * t + 1), corner(3 * t + 2)]).collect()
    };
    assert!(super::chunk("F", &tris(MAX_VERTICES), false).is_ok());
    assert_eq!(super::chunk("F", &tris(MAX_VERTICES + 1), false), Err(WireError("too many vertices")));
}

#[test]
fn a_mesh_that_cannot_go_is_refused_before_anything_is_sent() {
    let refused = |m: &Mesh, names: &[&str], why: &'static str| assert_eq!(Shape::from_mesh(m, names), Err(WireError(why)));
    let one = || Mesh { positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y], triangles: vec![[0, 1, 2]], ..Default::default() };
    assert!(Shape::from_mesh(&one(), &["F"]).is_ok());
    refused(&one(), &[], "a name for every face");
    refused(&Mesh { triangles: vec![[0, 1, 3]], ..one() }, &["F"], "a triangle corner past the last vertex");
    refused(&Mesh { normals: vec![Vec3::Z], ..one() }, &["F"], "a normal per vertex, or none");
    refused(&Mesh { tri_face: vec![0, 0], ..one() }, &["F"], "a face per triangle, or none");
    refused(&Mesh { tri_face: vec![u32::MAX], ..one() }, &["F"], "too many faces");
    refused(&Mesh { tri_face: vec![1], ..one() }, &["F", "F"], "two faces with one name");
    for bad in [f64::NAN, f64::INFINITY, 1.000_001e9, -1.000_001e9] {
        refused(&Mesh { positions: vec![Vec3::new(bad, 0.0, 0.0), Vec3::X, Vec3::Y], ..one() }, &["F"], "a coordinate not finite or past 1e9 mm");
    }
    // A name goes up to 4 KiB, the plan's cap for a name on the wire.
    let name = "x".repeat(4096);
    assert_eq!(receive(&send(&one(), &[&name])).unwrap().faces()[0].name(), name);
    refused(&one(), &[&"x".repeat(4097)], "a face name over 4 KiB");
    // The display bound itself goes, and comes back within half a micrometre.
    let edge = Mesh { positions: vec![Vec3::new(1e9, -1e9, 0.0), Vec3::new(-1e9, 1e9, 1e9), Vec3::Y], ..one() };
    let got = receive(&send(&edge, &["F"])).unwrap().to_mesh().unwrap();
    for p in &edge.positions {
        assert!(got.positions.iter().any(|q| p.dist(*q) < 1e-9 * p.len().max(1.0)), "{p:?} didn't come back: {:?}", got.positions);
    }
}

#[test]
fn shape_messages_survive_mutation() {
    let m = sample();
    let shape = Shape::from_mesh(&m, &NAMES).unwrap();
    let read = |b: &[u8]| receive(b).ok().and_then(|s| s.to_mesh().ok()).map(|m| m.triangles.len());
    survives("shape message (deflated)", &mutations(&shape.encode(|_| false), false, 0x5ade), MAX_TRIANGLES, read);
    // Mutations of the deflated bytes mostly stop at deflate's own checks: these mutate the
    // message inside and deflate it again, so they reach the chunk parser.
    let raw = inflate(&shape.encode(|_| false));
    let inside: Vec<Vec<u8>> = mutations(&raw, false, 0x5adf).iter().map(|b| deflated(b)).collect();
    survives("shape message (inside the deflate)", &inside, MAX_TRIANGLES, read);
    // With held faces and a base: a side that holds the shape before the edit.
    let base = Arc::new(shape);
    let holds = Holds(vec![base.clone()]);
    let read = |b: &[u8]| Shape::decode(b, &holds).ok().and_then(|s| s.to_mesh().ok()).map(|m| m.triangles.len());
    let after = Shape::from_mesh(&raised(), &NAMES).unwrap();
    let by_hash = inflate(&after.encode(|h| holds.face(h).is_some()));
    let inside: Vec<Vec<u8>> = mutations(&by_hash, false, 0x5ae0).iter().map(|b| deflated(b)).collect();
    survives("shape message with a held face", &inside, MAX_TRIANGLES, read);
    // Deltas: faces changed, removed and added.
    let other = Shape::from_mesh(&raised(), &["F1:side", "F2:lid", "F1:top"]).unwrap();
    for (k, delta) in [after.encode_delta(&base, |_| false), other.encode_delta(&base, |_| false)].iter().enumerate() {
        let inside: Vec<Vec<u8>> = mutations(&inflate(delta), false, 0xde17a + k as u64).iter().map(|b| deflated(b)).collect();
        survives("delta", &inside, MAX_TRIANGLES, read);
    }
}

#[test]
fn a_body_is_a_shape_hash_and_its_exact_placement() {
    let body = crate::Body {
        shape: *Shape::from_mesh(&sample(), &NAMES).unwrap().hash(),
        placement: [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [123.456_789_012_345, -0.1, 1e-300]],
    };
    let bytes = body.to_bytes().unwrap();
    assert_eq!(bytes.len(), crate::Body::BYTES);
    assert_eq!(crate::Body::from_bytes(&bytes), Ok(body), "bit for bit");
    assert!(crate::Body::from_bytes(&bytes[..127]).is_err() && crate::Body::from_bytes(&[bytes.as_slice(), &[0]].concat()).is_err());
    for bad in [f64::NAN, f64::NEG_INFINITY, 1.000_001e9, 2e12] {
        let mut lie = body;
        lie.placement[3][0] = bad;
        assert!(lie.to_bytes().is_err());
        let mut b = bytes.clone();
        b[32 + 9 * 8..32 + 10 * 8].copy_from_slice(&bad.to_le_bytes());
        assert!(crate::Body::from_bytes(&b).is_err());
    }
    survives("body", &mutations(&bytes, false, 0xb0d1), 1, |b| crate::Body::from_bytes(b).ok().map(|_| 1));
}

#[test]
fn a_triangle_with_no_area_on_the_grid_is_left_out() {
    // The first two corners are 0.1 µm apart: one grid point.
    let m = Mesh {
        positions: vec![Vec3::ZERO, Vec3::new(0.000_1, 0.0, 0.0), Vec3::X, Vec3::Y],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        ..Default::default()
    };
    let got = receive(&send(&m, &["F"])).unwrap().to_mesh().unwrap();
    assert_eq!(got.triangles.len(), 1, "only the triangle with an area");
    // All of a face's triangles collapsed: the face goes, empty.
    let dot = Mesh { positions: vec![Vec3::ZERO; 3], triangles: vec![[0, 1, 2]], ..Default::default() };
    let got = receive(&send(&dot, &["F"])).unwrap();
    assert_eq!((got.faces().len(), got.to_mesh().unwrap().triangles.len()), (1, 0));
}

/// A whole shape message of `n` faces, each GOOD under a name `size` bytes long.
fn named_faces(n: usize, size: usize) -> Vec<u8> {
    let mut raw = vec![0];
    crate::bytes::put_varint(&mut raw, n as u64);
    for k in 0..n {
        let name = format!("{k:05}{}", "x".repeat(size - 5));
        raw.push(0);
        crate::bytes::put_varint(&mut raw, name.len() as u64);
        raw.extend(name.as_bytes());
        raw.extend(GOOD);
    }
    deflated(&raw)
}

#[test]
fn a_shape_holds_at_most_4_mib_of_face_names() {
    // 1,024 names at the 4 KiB cap is exactly 4 MiB.
    assert_eq!(receive(&named_faces(1024, 4096)).map(|s| s.faces().len()), Ok(1024));
    assert_eq!(receive(&named_faces(1025, 4096)), Err(WireError("face names over 4 MiB in one shape")));
    // The same when it is cut from a mesh: 1,025 one-triangle faces.
    let mut m = Mesh::default();
    for k in 0..1025u32 {
        m.positions.extend([Vec3::new(f64::from(k), 0.0, 0.0), Vec3::new(f64::from(k), 1.0, 0.0), Vec3::new(f64::from(k), 0.0, 1.0)]);
        m.triangles.push([3 * k, 3 * k + 1, 3 * k + 2]);
        m.tri_face.push(k);
    }
    let names: Vec<String> = (0..1025).map(|k| format!("{k:05}{}", "x".repeat(4091))).collect();
    assert_eq!(Shape::from_mesh(&m, &names), Err(WireError("face names over 4 MiB in one shape")));
    assert!(Shape::from_mesh(&m, &names.iter().map(|n| &n[..64]).collect::<Vec<_>>()).is_ok());
}

#[test]
fn a_face_travels_alone_as_a_blob() {
    let shape = Shape::from_mesh(&sample(), &NAMES).unwrap();
    for c in shape.faces() {
        let blob = c.to_blob();
        assert_eq!(declared_size(&blob), Ok(c.bytes.len()));
        assert_eq!(FaceChunk::from_blob(&blob).as_ref(), Ok(&**c), "{}: the same chunk and hash", c.name());
    }
    // A face too large for one frame (a scan: one face, 20,000 triangles) still goes as one
    // blob; the blob layer cuts it into pieces.
    let mut scan = cylinder(50.0, 80.0, 5000, Vec3::ZERO);
    scan.tri_face.clear();
    let scan = Shape::from_mesh(&scan, &["Scan:face:0"]).unwrap();
    let blob = scan.faces()[0].to_blob();
    assert!(blob.len() > 64 << 10, "{} bytes", blob.len());
    assert_eq!(FaceChunk::from_blob(&blob).map(|c| c.triangles), Ok(20_000));
    // Each lie is refused alone as inside a shape message, and so is anything after the face.
    for (name, rest, why) in face_lies() {
        let raw = [&[name.len() as u8][..], &name, &rest].concat();
        assert_eq!(FaceChunk::from_blob(&deflated(&raw)), Err(WireError(why)), "{why}");
    }
    let good = [&[1, b'F'][..], GOOD].concat();
    assert!(FaceChunk::from_blob(&deflated(&good)).is_ok());
    assert_eq!(FaceChunk::from_blob(&deflated(&[&good[..], &[0]].concat())), Err(WireError("bytes after the face")));
    assert_eq!(FaceChunk::from_blob(&declaring(good.len() + 1, &good)), Err(WireError("inflates short of its declared size")));
    let blob = shape.faces()[1].to_blob();
    let read = |b: &[u8]| FaceChunk::from_blob(b).ok().map(|c| c.triangles);
    survives("face blob (deflated)", &mutations(&blob, false, 0xb10b), MAX_TRIANGLES, read);
    let inside: Vec<Vec<u8>> = mutations(&inflate(&blob), false, 0xb10c).iter().map(|b| deflated(b)).collect();
    survives("face blob (inside the deflate)", &inside, MAX_TRIANGLES, read);
}
