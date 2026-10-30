//! The compact mesh codec. A shape is a display mesh cut into one chunk per B-rep face; a body is
//! a shape's hash and a placement.
//!
//! A face chunk, as hashed and sent (varints are LEB128 in their shortest form, signed ones
//! zig-zag):
//!
//! ```text
//! name length, name (UTF-8)       the face's persistent name
//! vertex count, triangle count
//! normals                         0 or 1: one normal per vertex follows the positions
//! positions                       per vertex, x y z on the 1 µm grid, each minus the previous
//!                                 vertex's (the first minus 0), signed
//! normals                         per vertex, octahedral, two i16 little-endian
//! triangles                       per corner, the vertex index minus the previous corner's, signed
//! ```
//!
//! A shape keeps its faces in name order, so a face is found by its name and the shape's hash
//! doesn't follow the B-rep's face numbering. A shape message is one of:
//!
//! ```text
//! 0 (whole)    face count, then per face, in name order: 0 and the chunk, or 1 and the
//!              32-byte hash of a chunk the receiver holds or will pull
//! 1 (delta)    the hash of a shape the receiver holds, the count and names of the faces it
//!              loses (in name order), then the faces it gains or that changed, as above
//! ```
//!
//! A delta costs nothing for a face that didn't change; a receiver without its base refuses it,
//! and the sender sends the shape whole. A face can also travel alone, as the blob a receiver
//! pulls by id ([`FaceChunk::to_blob`]): one too large to ride inline, or one held elsewhere.
//!
//! Every message (a shape message or a face blob) goes as its inflated length, a varint, then
//! the deflate stream. The receiver knows what a message costs before inflating it
//! ([`declared_size`]), and nothing inflates past what it declared.
//!
//! The encoder writes each face in one canonical form, so a face that comes out of the mesher
//! the same on the grid gets the same bytes and hash however the mesh numbers or orders it:
//! corners are (grid position, octahedral normal) pairs, each triangle starts at its least corner
//! (keeping its turning sense), the triangles are sorted, and equal corners are one vertex,
//! numbered in the order the triangles first use them. Remeshing an unchanged face reorders its
//! triangles; without this, most faces of a body would count as changed after any edit.

use std::collections::HashMap;
use std::sync::Arc;

use miniz_oxide::inflate::TINFLStatus;
use sha2::{Digest, Sha256};
use solidcraft_geom::{Mesh, Vec3};

use crate::bytes::{Reader, put_varint, unzigzag, zigzag};
use crate::{WireError, require};

/// A SHA-256 content hash.
pub type Hash = [u8; 32];

/// Positions travel on a 1 µm grid: whole multiples of a thousandth of a millimetre, so a
/// coordinate comes back within half a micrometre. Plenty for a preview, and it is what makes
/// the deltas small (plan, "Network weight").
const GRID_PER_MM: f64 = 1000.0;
/// The largest coordinate and placement entry, in millimetres: a display bound, far below the
/// B-rep's 1e12 (`geom::brep`). It is the GPU path's own (`gpu::path::scene`) and a sketch's
/// (`solidcraft_sketch::MAX_COORD`), so what reaches the GPU from someone else stays where an
/// f32 view holds it well. Its grid value fits in an `f64` exactly (under 2^53).
const MAX_COORD_MM: f64 = 1e9;
const MAX_GRID: u64 = (MAX_COORD_MM * GRID_PER_MM) as u64;
/// Most faces, vertices and triangles in one shape (held chunks included): the caps the
/// live-layer plan set, about 300 MB as a [`Mesh`] at most.
const MAX_FACES: usize = 1_000_000;
const MAX_VERTICES: usize = 4_000_000;
const MAX_TRIANGLES: usize = 8_000_000;
/// The longest face name on the wire: the plan's 4 KiB for a persistent name (presence's
/// selection carries the same names). A split face's name lists its neighbours, so names run to
/// a few hundred bytes; a body with a longer one doesn't go.
const MAX_NAME_BYTES: usize = 4 << 10;
/// Most bytes of face names in one shape: presence and the Browser keep every name of every body
/// they hold. That is 1,024 names at the 4 KiB cap, or a scan named per triangle (10,000 faces).
const MAX_SHAPE_NAME_BYTES: usize = 4 << 20;
/// Most bytes a message may declare, and so inflate to, and most a shape may take sent whole.
const MAX_DECODED_BYTES: usize = 64 << 20;

/// Deflate level (miniz, 0 to 10). On the sensor enclosure, 9 is under 1% smaller than 6 at the
/// same encode time: hashing and sorting the faces cost more than deflate at these sizes.
const DEFLATE_LEVEL: u8 = 9;
const WHOLE: u8 = 0;
const DELTA: u8 = 1;
const INLINE: u8 = 0;
const HELD: u8 = 1;
/// Octahedral coordinates in -1..=1 as i16 (snorm16).
const OCT_SCALE: f64 = 32767.0;

/// One face of a shape: its persistent name and its triangles, quantised. Made only by
/// [`Shape::from_mesh`] and [`Shape::decode`], so its bytes are always valid.
#[derive(Debug, PartialEq, Eq)]
pub struct FaceChunk {
    name: String,
    /// The chunk as hashed and sent.
    bytes: Vec<u8>,
    vertices: usize,
    triangles: usize,
    normals: bool,
    hash: Hash,
}

impl FaceChunk {
    fn new(head: Head, bytes: Vec<u8>) -> FaceChunk {
        let hash = Sha256::new().chain_update(b"solidcraft-wire/face\0").chain_update(&bytes).finalize().into();
        FaceChunk { name: head.name.to_owned(), bytes, vertices: head.vertices, triangles: head.triangles, normals: head.normals, hash }
    }

    /// The face's persistent name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The content hash: over the name and the quantised vertices, normals and triangles.
    pub fn hash(&self) -> &Hash {
        &self.hash
    }

    /// The face alone, as the blob a receiver pulls by id when a shape message names it by hash.
    /// A blob longer than one frame goes as pieces: that is the blob layer's rule, for every blob.
    pub fn to_blob(&self) -> Vec<u8> {
        seal(&self.bytes)
    }

    /// Reads [`to_blob`](FaceChunk::to_blob)'s bytes, from anyone, with the checks of a face
    /// inside a shape message.
    pub fn from_blob(bytes: &[u8]) -> Result<FaceChunk, WireError> {
        let raw = unseal(bytes)?;
        let mut r = Reader::new(&raw);
        let head = walk(&mut r, None)?;
        require(r.left() == 0, "bytes after the face")?;
        // The name is copied out, so the chunk can take `raw` without a second copy.
        let name = head.name.to_owned();
        Ok(FaceChunk::new(Head { name: &name, ..head }, raw))
    }
}

/// A display mesh on the wire, one chunk per B-rep face in name order, keyed by its
/// [`hash`](Shape::hash).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    faces: Vec<Arc<FaceChunk>>,
    hash: Hash,
    vertices: usize,
    triangles: usize,
}

impl Shape {
    /// Cuts `mesh` into face chunks: a triangle belongs to face `tri_face` (face 0 when there is
    /// none), and face `i` is named `face_names[i]`. Positions go to the 1 µm grid. Each face
    /// keeps its triangles as a set (in the canonical order, module docs); vertices no triangle
    /// uses are left out, and one that several faces use goes once in each.
    pub fn from_mesh<S: AsRef<str>>(mesh: &Mesh, face_names: &[S]) -> Result<Shape, WireError> {
        let (n_pos, n_tri) = (mesh.positions.len(), mesh.triangles.len());
        require(n_pos <= MAX_VERTICES && n_tri <= MAX_TRIANGLES, "too many vertices or triangles")?;
        let normals = !mesh.normals.is_empty();
        require(!normals || mesh.normals.len() == n_pos, "a normal per vertex, or none")?;
        require(mesh.tri_face.is_empty() || mesh.tri_face.len() == n_tri, "a face per triangle, or none")?;
        let face_of = |t: u32| mesh.tri_face.get(t as usize).map_or(0, |&f| u64::from(f));
        let faces = mesh.tri_face.iter().max().map_or(u64::from(n_tri > 0), |&f| u64::from(f) + 1);
        require(faces <= MAX_FACES as u64, "too many faces")?;
        require(face_names.len() as u64 >= faces, "a name for every face")?;
        // Triangles grouped by face (each face's own order doesn't count: it is sorted below).
        let mut order: Vec<u32> = (0..n_tri as u32).collect();
        order.sort_unstable_by_key(|&t| face_of(t));
        let mut order = order.into_iter().peekable();
        let mut tris: Vec<[Corner; 3]> = Vec::new();
        let mut chunks = Vec::with_capacity(faces as usize);
        for (face, name) in (0..faces).zip(face_names) {
            tris.clear();
            while let Some(t) = order.next_if(|&t| face_of(t) == face) {
                let mut tri = [Corner::default(); 3];
                for (c, g) in tri.iter_mut().zip(mesh.triangles.get(t as usize).copied().unwrap_or_default()) {
                    *c = corner(mesh, g as usize, normals)?;
                }
                // Two corners on one grid point: no area at 1 µm and nothing to draw.
                if tri[0].0 == tri[1].0 || tri[1].0 == tri[2].0 || tri[0].0 == tri[2].0 {
                    continue;
                }
                let least = tri.iter().enumerate().min_by_key(|(_, c)| **c).map_or(0, |(k, _)| k);
                tri.rotate_left(least);
                tris.push(tri);
            }
            tris.sort_unstable();
            chunks.push(Arc::new(chunk(name.as_ref(), &tris, normals)?));
        }
        chunks.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        require(in_name_order(&chunks), "two faces with one name")?;
        Shape::new(chunks)
    }

    fn new(faces: Vec<Arc<FaceChunk>>) -> Result<Shape, WireError> {
        let vertices = faces.iter().map(|c| c.vertices).sum();
        let triangles = faces.iter().map(|c| c.triangles).sum();
        require(vertices <= MAX_VERTICES && triangles <= MAX_TRIANGLES, "too many vertices or triangles")?;
        require(faces.iter().map(|c| c.name.len()).sum::<usize>() <= MAX_SHAPE_NAME_BYTES, "face names over 4 MiB in one shape")?;
        // Sent whole, it must inflate within the cap too, so whoever holds it can pass it on.
        require(whole_len(&faces) <= MAX_DECODED_BYTES, "the shape is over the size cap")?;
        let mut h = Sha256::new().chain_update(b"solidcraft-wire/shape\0");
        for c in &faces {
            h.update(c.hash);
        }
        Ok(Shape { faces, hash: h.finalize().into(), vertices, triangles })
    }

    /// The content hash: over the faces' hashes, in name order.
    pub fn hash(&self) -> &Hash {
        &self.hash
    }

    /// The face chunks, in name order.
    pub fn faces(&self) -> &[Arc<FaceChunk>] {
        &self.faces
    }

    /// The face named `name`.
    fn find(&self, name: &str) -> Option<&Arc<FaceChunk>> {
        self.faces.binary_search_by(|c| c.name.as_str().cmp(name)).ok().and_then(|k| self.faces.get(k))
    }

    /// The bytes to send the whole shape: every face chunk, except those the sender `knows` the
    /// receiver holds, or will pull ([`FaceChunk::to_blob`]), which go as their hash. A chunk no longer than its hash always goes whole.
    pub fn encode(&self, knows: impl Fn(&Hash) -> bool) -> Vec<u8> {
        let mut raw = Vec::with_capacity(whole_len(&self.faces));
        raw.push(WHOLE);
        put_faces(&mut raw, self.faces.iter().map(|c| &**c), &knows);
        seal(&raw)
    }

    /// The bytes to send a receiver that holds `base`: the names of the faces this shape lacks,
    /// and the faces that are new or changed (by hash when the sender `knows` the receiver holds
    /// them). An unchanged face costs nothing.
    pub fn encode_delta(&self, base: &Shape, knows: impl Fn(&Hash) -> bool) -> Vec<u8> {
        let removed: Vec<&str> = base.faces.iter().map(|c| c.name.as_str()).filter(|n| self.find(n).is_none()).collect();
        let changed: Vec<&FaceChunk> = self.faces.iter().filter(|c| base.find(&c.name).is_none_or(|b| b.hash != c.hash)).map(|c| &**c).collect();
        let mut raw = Vec::with_capacity(64 + changed.iter().map(|c| 1 + c.bytes.len()).sum::<usize>());
        raw.push(DELTA);
        raw.extend_from_slice(&base.hash);
        put_varint(&mut raw, removed.len() as u64);
        for name in removed {
            put_name(&mut raw, name);
        }
        put_faces(&mut raw, changed.into_iter(), &knows);
        seal(&raw)
    }

    /// Reads [`encode`](Shape::encode)'s or [`encode_delta`](Shape::encode_delta)'s bytes, from
    /// anyone: `held` gives the shapes and chunks this side already has, by hash. What it hands
    /// back must carry the hash asked for, or the message is refused.
    pub fn decode(bytes: &[u8], held: &impl Held) -> Result<Shape, WireError> {
        let raw = unseal(bytes)?;
        let mut r = Reader::new(&raw);
        let shape = match r.byte()? {
            WHOLE => Shape::new(read_faces(&mut r, held)?)?,
            DELTA => {
                let hash: Hash = r.take(32)?.try_into().map_err(|_| WireError("truncated"))?;
                let base = held.shape(&hash).filter(|s| s.hash == hash).ok_or(WireError("a delta against a shape this side does not hold"))?;
                // Each removed name takes at least its length byte.
                let n = r.count(base.faces.len(), 1, "too many faces")?;
                let mut removed: Vec<&str> = Vec::with_capacity(n);
                for _ in 0..n {
                    let name = read_name(&mut r)?;
                    require(removed.last().is_none_or(|last| *last < name), "faces out of name order, or a name twice")?;
                    require(base.find(name).is_some(), "a delta removes a face its base lacks")?;
                    removed.push(name);
                }
                let changed = read_faces(&mut r, held)?;
                require(changed.iter().all(|c| removed.binary_search(&c.name()).is_err()), "a delta removes and changes one face")?;
                let replaced = |c: &FaceChunk| changed.binary_search_by(|d| d.name.cmp(&c.name)).is_ok();
                let mut faces: Vec<Arc<FaceChunk>> =
                    base.faces.iter().filter(|c| removed.binary_search(&c.name()).is_err() && !replaced(c)).cloned().collect();
                faces.extend(changed);
                require(faces.len() <= MAX_FACES, "too many faces")?;
                faces.sort_unstable_by(|a, b| a.name.cmp(&b.name));
                Shape::new(faces)?
            }
            _ => return Err(WireError("a message form")),
        };
        require(r.left() == 0, "bytes after the last face")?;
        Ok(shape)
    }

    /// The mesh: each face's vertices and triangles in turn, `tri_face` the face's index in
    /// name order.
    /// Normals when every face has them; no edges (the receiver draws them from the faces).
    pub fn to_mesh(&self) -> Result<Mesh, WireError> {
        let normals = self.faces.iter().all(|c| c.normals);
        let mut mesh = Mesh::default();
        mesh.positions.reserve_exact(self.vertices);
        mesh.normals.reserve_exact(if normals { self.vertices } else { 0 });
        mesh.triangles.reserve_exact(self.triangles);
        mesh.tri_face.reserve_exact(self.triangles);
        for (face, c) in self.faces.iter().enumerate() {
            let face = u32::try_from(face).map_err(|_| WireError("too many faces"))?;
            walk(&mut Reader::new(&c.bytes), Some(Sink { mesh: &mut mesh, face, normals }))?;
        }
        Ok(mesh)
    }
}

/// A message as sent: its inflated length, then the deflate stream.
fn seal(raw: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(10 + raw.len() / 2);
    put_varint(&mut b, raw.len() as u64);
    b.extend(miniz_oxide::deflate::compress_to_vec(raw, DEFLATE_LEVEL));
    b
}

/// Reads [`seal`]'s bytes. It inflates one byte past the declared size at most and refuses
/// anything but exactly that size, so a decompression bomb stops at what it declared.
fn unseal(bytes: &[u8]) -> Result<Vec<u8>, WireError> {
    let mut r = Reader::new(bytes);
    let n = declared(&mut r)?;
    let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(r.rest(), n + 1).map_err(|e| match e.status {
        TINFLStatus::HasMoreOutput => WireError("inflates past its declared size"),
        _ => WireError("not deflate"),
    })?;
    require(raw.len() <= n, "inflates past its declared size")?;
    require(raw.len() == n, "inflates short of its declared size")?;
    Ok(raw)
}

fn declared(r: &mut Reader) -> Result<usize, WireError> {
    usize::try_from(r.varint()?).ok().filter(|&n| n <= MAX_DECODED_BYTES).ok_or(WireError("a declared size past the cap"))
}

/// What a shape message or a face blob inflates to, read from its first bytes without inflating
/// (at most 64 MiB), so the receiver can count it against its budget first.
pub fn declared_size(bytes: &[u8]) -> Result<usize, WireError> {
    declared(&mut Reader::new(bytes))
}

/// What the receiving side already holds, by content hash: shapes (the bases of deltas) and face
/// chunks. `()` holds nothing. (The sender's guess at it is the `knows` of
/// [`encode`](Shape::encode).)
pub trait Held {
    fn shape(&self, hash: &Hash) -> Option<Arc<Shape>>;
    fn face(&self, hash: &Hash) -> Option<Arc<FaceChunk>>;
}

impl Held for () {
    fn shape(&self, _: &Hash) -> Option<Arc<Shape>> {
        None
    }

    fn face(&self, _: &Hash) -> Option<Arc<FaceChunk>> {
        None
    }
}

/// Whether the faces' names are strictly ascending: in name order, none twice.
fn in_name_order(faces: &[Arc<FaceChunk>]) -> bool {
    faces.windows(2).all(|w| matches!(w, [a, b] if a.name < b.name))
}

/// Writes a count, then each face inline, or as its hash when the sender `knows` the receiver
/// holds it (and it is longer than its hash).
fn put_faces<'a>(raw: &mut Vec<u8>, faces: impl ExactSizeIterator<Item = &'a FaceChunk>, knows: &impl Fn(&Hash) -> bool) {
    put_varint(raw, faces.len() as u64);
    for c in faces {
        if c.bytes.len() > c.hash.len() && knows(&c.hash) {
            raw.push(HELD);
            raw.extend_from_slice(&c.hash);
        } else {
            raw.push(INLINE);
            raw.extend_from_slice(&c.bytes);
        }
    }
}

/// Reads [`put_faces`]' count and faces, in name order. The vertex and triangle totals are
/// checked as they come, so a long list of held chunks stops at the cap.
fn read_faces(r: &mut Reader, held: &impl Held) -> Result<Vec<Arc<FaceChunk>>, WireError> {
    // Every face takes at least its tag byte.
    let n = r.count(MAX_FACES, 1, "too many faces")?;
    let mut faces: Vec<Arc<FaceChunk>> = Vec::with_capacity(n);
    let (mut vertices, mut triangles) = (0usize, 0usize);
    for _ in 0..n {
        let chunk = match r.byte()? {
            INLINE => {
                let start = r.mark();
                let head = walk(r, None)?;
                Arc::new(FaceChunk::new(head, r.since(start).to_vec()))
            }
            HELD => {
                let hash: Hash = r.take(32)?.try_into().map_err(|_| WireError("truncated"))?;
                held.face(&hash).filter(|c| c.hash == hash).ok_or(WireError("a face chunk this side does not hold"))?
            }
            _ => return Err(WireError("a face tag")),
        };
        require(faces.last().is_none_or(|last| last.name < chunk.name), "faces out of name order, or a name twice")?;
        vertices = vertices.saturating_add(chunk.vertices);
        triangles = triangles.saturating_add(chunk.triangles);
        require(vertices <= MAX_VERTICES && triangles <= MAX_TRIANGLES, "too many vertices or triangles")?;
        faces.push(chunk);
    }
    Ok(faces)
}

fn put_name(b: &mut Vec<u8>, name: &str) {
    put_varint(b, name.len() as u64);
    b.extend_from_slice(name.as_bytes());
}

fn read_name<'a>(r: &mut Reader<'a>) -> Result<&'a str, WireError> {
    let len = r.count(MAX_NAME_BYTES, 1, "a face name over 4 KiB")?;
    std::str::from_utf8(r.take(len)?).map_err(|_| WireError("a face name not UTF-8"))
}

/// The shape message's length with every chunk inline, before deflate (at most).
fn whole_len(faces: &[Arc<FaceChunk>]) -> usize {
    faces.iter().fold(10usize, |n, c| n.saturating_add(1 + c.bytes.len()))
}

/// A chunk's counts and name, read before its data.
struct Head<'a> {
    name: &'a str,
    vertices: usize,
    triangles: usize,
    normals: bool,
}

/// Where [`walk`] puts a chunk's vertices and triangles, as face `face`.
struct Sink<'m> {
    mesh: &'m mut Mesh,
    face: u32,
    normals: bool,
}

/// A corner on the wire: its position on the grid and its octahedral normal ([0, 0] without
/// normals).
type Corner = ([i64; 3], [i16; 2]);

/// Vertex `g` of `mesh` as a corner.
fn corner(mesh: &Mesh, g: usize, normals: bool) -> Result<Corner, WireError> {
    let p = mesh.positions.get(g).ok_or(WireError("a triangle corner past the last vertex"))?;
    let mut q = [0i64; 3];
    for (q, x) in q.iter_mut().zip([p.x, p.y, p.z]) {
        require(x.is_finite() && x.abs() <= MAX_COORD_MM, "a coordinate not finite or past 1e9 mm")?;
        *q = (x * GRID_PER_MM).round() as i64;
    }
    let n = if normals { oct(mesh.normals.get(g).copied().unwrap_or_default())? } else { [0; 2] };
    Ok((q, n))
}

/// Writes face chunk `name` from its triangles in canonical order: equal corners become one
/// vertex, numbered by first use.
fn chunk(name: &str, tris: &[[Corner; 3]], normals: bool) -> Result<FaceChunk, WireError> {
    require(name.len() <= MAX_NAME_BYTES, "a face name over 4 KiB")?;
    // A scan is one face of millions of triangles sharing their corners: room for three
    // vertices a triangle would be gigabytes, so at most the cap's worth.
    let room = (tris.len() * 3).min(MAX_VERTICES);
    let mut ids: HashMap<Corner, u32> = HashMap::with_capacity(room);
    let mut verts: Vec<Corner> = Vec::with_capacity(room);
    let mut corners: Vec<u32> = Vec::with_capacity(tris.len() * 3);
    for c in tris.iter().flatten() {
        let id = match ids.get(c) {
            Some(&id) => id,
            None => {
                require(verts.len() < MAX_VERTICES, "too many vertices")?;
                verts.push(*c);
                let id = verts.len() as u32 - 1;
                ids.insert(*c, id);
                id
            }
        };
        corners.push(id);
    }
    // Typical sizes: up to 9 bytes of position deltas and 4 of normal per vertex, 2 per corner.
    let mut b = Vec::with_capacity(name.len() + 22 + verts.len() * 13 + corners.len() * 2);
    put_name(&mut b, name);
    put_varint(&mut b, verts.len() as u64);
    put_varint(&mut b, tris.len() as u64);
    b.push(u8::from(normals));
    let mut prev = [0i64; 3];
    for (q, _) in &verts {
        for (last, q) in prev.iter_mut().zip(q) {
            put_varint(&mut b, zigzag(q - *last));
            *last = *q;
        }
    }
    if normals {
        for v in verts.iter().flat_map(|(_, n)| n) {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    let mut last = 0i64;
    for &i in &corners {
        put_varint(&mut b, zigzag(i64::from(i) - last));
        last = i64::from(i);
    }
    Ok(FaceChunk::new(Head { name, vertices: verts.len(), triangles: tris.len(), normals }, b))
}

/// Reads one face chunk, checking every count, coordinate and index; with a sink, adds its
/// vertices and triangles to the mesh there.
fn walk<'a>(r: &mut Reader<'a>, mut sink: Option<Sink>) -> Result<Head<'a>, WireError> {
    let name = read_name(r)?;
    // Each vertex takes at least 3 bytes, each triangle 3.
    let vertices = r.count(MAX_VERTICES, 3, "too many vertices")?;
    let triangles = r.count(MAX_TRIANGLES, 3, "too many triangles")?;
    let normals = match r.byte()? {
        0 => false,
        1 => true,
        _ => return Err(WireError("the normals flag")),
    };
    let base = match &sink {
        Some(s) => u32::try_from(s.mesh.positions.len()).map_err(|_| WireError("too many vertices"))?,
        None => 0,
    };
    let mut q = [0i64; 3];
    for _ in 0..vertices {
        for c in &mut q {
            *c = c.checked_add(unzigzag(r.varint()?)).filter(|c| c.unsigned_abs() <= MAX_GRID).ok_or(WireError("a coordinate past 1e9 mm"))?;
        }
        if let Some(s) = sink.as_mut() {
            let [x, y, z] = q.map(|c| c as f64 / GRID_PER_MM);
            s.mesh.positions.push(Vec3::new(x, y, z));
        }
    }
    if normals {
        for _ in 0..vertices {
            let n = match r.take(4)? {
                &[a0, a1, b0, b1] => unoct([i16::from_le_bytes([a0, a1]), i16::from_le_bytes([b0, b1])]),
                _ => return Err(WireError("truncated")),
            };
            if let Some(s) = sink.as_mut().filter(|s| s.normals) {
                s.mesh.normals.push(n);
            }
        }
    }
    let mut last = 0i64;
    for _ in 0..triangles {
        let mut t = [0u32; 3];
        for c in &mut t {
            last = last.checked_add(unzigzag(r.varint()?)).ok_or(WireError("a triangle corner out of range"))?;
            let i = u32::try_from(last).ok().filter(|&i| (i as usize) < vertices).ok_or(WireError("a triangle corner out of range"))?;
            *c = base.checked_add(i).ok_or(WireError("too many vertices"))?;
        }
        require(t[0] != t[1] && t[1] != t[2] && t[0] != t[2], "a triangle with a repeated corner")?;
        if let Some(s) = sink.as_mut() {
            s.mesh.triangles.push(t);
            s.mesh.tri_face.push(s.face);
        }
    }
    Ok(Head { name, vertices, triangles, normals })
}

/// A normal as two octahedral coordinates: the direction projected onto the octahedron
/// |x| + |y| + |z| = 1, its lower half folded out over the corners of the square. A zero normal
/// goes as +Z.
fn oct(n: Vec3) -> Result<[i16; 2], WireError> {
    require(n.is_finite(), "a normal not finite")?;
    let s = n.x.abs() + n.y.abs() + n.z.abs();
    let (x, y, z) = if s > 0.0 { (n.x / s, n.y / s, n.z / s) } else { (0.0, 0.0, 1.0) };
    let (x, y) = if z < 0.0 { fold(x, y) } else { (x, y) };
    Ok([x, y].map(|v| (v.clamp(-1.0, 1.0) * OCT_SCALE).round() as i16))
}

fn unoct(v: [i16; 2]) -> Vec3 {
    let [x, y] = v.map(|c| (f64::from(c) / OCT_SCALE).clamp(-1.0, 1.0));
    let z = 1.0 - x.abs() - y.abs();
    let (x, y) = if z < 0.0 { fold(x, y) } else { (x, y) };
    // |x| + |y| + |z| = 1, so the length is at least 1/sqrt(3).
    Vec3::new(x, y, z).normalized().unwrap_or(Vec3::Z)
}

/// The octahedral fold between the upper and lower half (its own inverse).
fn fold(x: f64, y: f64) -> (f64, f64) {
    let sign = |v: f64| if v >= 0.0 { 1.0 } else { -1.0 };
    ((1.0 - y.abs()) * sign(x), (1.0 - x.abs()) * sign(y))
}

/// A body's placement: the columns of a 4 x 3 affine transform (x, y and z axes, then the
/// translation), as `doc::assembly::Mat`'s first three rows.
pub type Placement = [[f64; 3]; 4];

/// A body on the wire: the shape it shows and where. The placement goes as twelve f64s
/// (96 bytes): exactly the author's transform, so the ghost lines up with the body the
/// receiver builds later, and a rotation needs no rule of its own for quantising.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub shape: Hash,
    pub placement: Placement,
}

impl Body {
    /// The length of [`to_bytes`](Body::to_bytes): the hash, then twelve f64 little-endian.
    pub const BYTES: usize = 32 + 12 * 8;

    pub fn to_bytes(&self) -> Result<Vec<u8>, WireError> {
        let mut b = Vec::with_capacity(Body::BYTES);
        b.extend_from_slice(&self.shape);
        for &v in self.placement.as_flattened() {
            require(v.is_finite() && v.abs() <= MAX_COORD_MM, "a placement not finite or past 1e9")?;
            b.extend_from_slice(&v.to_le_bytes());
        }
        Ok(b)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Body, WireError> {
        require(bytes.len() == Body::BYTES, "a body is 128 bytes")?;
        let mut r = Reader::new(bytes);
        let shape: Hash = r.take(32)?.try_into().map_err(|_| WireError("truncated"))?;
        let mut placement = [[0.0; 3]; 4];
        for v in placement.as_flattened_mut() {
            let b: [u8; 8] = r.take(8)?.try_into().map_err(|_| WireError("truncated"))?;
            *v = f64::from_le_bytes(b);
            require(v.is_finite() && v.abs() <= MAX_COORD_MM, "a placement not finite or past 1e9")?;
        }
        Ok(Body { shape, placement })
    }
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
