//! 3MF (3D Manufacturing Format, Core specification 1.3) packages: a zip with
//! `[Content_Types].xml`, `_rels/.rels` and the model part `3D/3dmodel.model`.
//!
//! Writing: one mesh object per body (shared vertices, outward winding), named after the body,
//! one build item each, colours as base materials. Objects with face tags also carry, per
//! triangle, the B-rep face it lies on (`ft:face`, SolidCraft's face-tag namespace; spec:
//! docs/formats/3mf-face-tags.md) and are written at full `f64`
//! precision so the receiver gets the tessellation exactly. Reading: every build item becomes one mesh
//! (components flattened with their transforms), converted to millimetres. Components and build
//! items may name an object in another model part of the package (`p:path`, the 3MF Production
//! extension, which slicers use for multi-part projects); those parts are read too. Only the root
//! model's build counts (readers must ignore the others), and a part that can't be read fails
//! only the placements that need it. Bodies are named after their objects; a slicer project whose
//! objects have no name in the model names them in its settings (Bambu Studio's
//! `Metadata/model_settings.config`), and those names are used. Packages are untrusted: sizes,
//! entry counts, model parts, mesh sizes and component nesting are capped.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use solidcraft_geom::Vec3;

use crate::{IoError, Result};

/// Largest 3MF file we read.
pub const MAX_3MF_BYTES: usize = 512 << 20;
/// Largest uncompressed part we read (zip bombs stop here).
const MAX_PART: u64 = 1 << 30;
const MAX_ENTRIES: usize = 10_000;
/// Most vertices or triangles in one file.
const MAX_ELEMENTS: usize = 20_000_000;
const MAX_COMPONENT_DEPTH: usize = 16;
const MAX_XML_DEPTH: usize = 256;

const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const MODEL_REL: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
/// SolidCraft's face-tag namespace, version 1: `ft:face` on triangles, `ft:faces` on objects
/// (docs/formats/3mf-face-tags.md).
pub const FACE_TAGS_NS: &str = "https://github.com/bherbruck/solidcraft/formats/3mf-face-tags/2026/10";

/// A named triangle mesh (millimetres, counter-clockwise seen from outside).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshObject {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub color: Option<[f32; 3]>,
    /// 0 = opaque, 1 = invisible (written as the colour's alpha).
    pub transparency: f32,
    /// Colours of their own for some triangles (faces with appearances): colour 0..1 and
    /// transparency; `tri_looks` gives each triangle's entry + 1 (0: the object's own), and is
    /// empty when no triangle has one.
    pub looks: Vec<([f32; 3], f32)>,
    pub tri_looks: Vec<u32>,
    /// The B-rep face each triangle lies on, or empty for no face tags. A tagged object is
    /// written with exact (`f64`) coordinates.
    pub tri_faces: Vec<u32>,
    /// Number of B-rep faces (written as `ft:faces` when the object is tagged).
    pub faces: u32,
}

impl MeshObject {
    fn tagged(&self) -> bool {
        !self.tri_faces.is_empty()
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

/// A triangle as written: its vertices, its colour entry (0: the object's own) and its face.
struct Tri {
    v: [u32; 3],
    look: u32,
    face: u32,
}

/// Shared vertices, merged at the precision they are written in (single, or double for tagged
/// objects), and non-degenerate triangles.
fn indexed(m: &MeshObject) -> (Vec<[f64; 3]>, Vec<Tri>) {
    let exact = m.tagged();
    let mut map: HashMap<[u64; 3], u32> = HashMap::new();
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut remap = Vec::with_capacity(m.positions.len());
    for p in &m.positions {
        // `+ 0.0` turns -0 into 0, so both merge.
        let v = if exact { [p.x + 0.0, p.y + 0.0, p.z + 0.0] } else { [p.x as f32 as f64 + 0.0, p.y as f32 as f64 + 0.0, p.z as f32 as f64 + 0.0] };
        let key = v.map(f64::to_bits);
        let next = u32::try_from(verts.len()).unwrap_or(u32::MAX);
        let i = *map.entry(key).or_insert_with(|| {
            verts.push(v);
            next
        });
        remap.push(i);
    }
    let tris = m
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(k, t)| {
            let (a, b, c) = (*remap.get(t[0] as usize)?, *remap.get(t[1] as usize)?, *remap.get(t[2] as usize)?);
            (a != b && b != c && c != a).then(|| Tri {
                v: [a, b, c],
                look: m.tri_looks.get(k).copied().unwrap_or(0),
                face: m.tri_faces.get(k).copied().unwrap_or(0),
            })
        })
        .collect();
    (verts, tris)
}

/// Make a tessellation watertight: merge vertices closer than `eps` and split triangles at
/// vertices lying on their open sides (T-junctions between separately meshed faces).
pub fn weld(positions: &[Vec3], triangles: &[[u32; 3]], eps: f64) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let (v, t, _) = weld_tagged(positions, triangles, &[], eps);
    (v, t)
}

/// [`weld`] keeping a tag per triangle (pieces of a split triangle keep its tag; missing tags
/// are 0).
pub fn weld_tagged(positions: &[Vec3], triangles: &[[u32; 3]], tags: &[u32], eps: f64) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<u32>) {
    let (verts, tris, tags, _) = weld_tagged_lines(positions, triangles, tags, eps, &[]);
    (verts, tris, tags)
}

/// [`weld_tagged`], and each of `lines` (polylines on the mesh, such as its B-rep edges) as the
/// welded vertices along it, in order, the vertices T-junction splits put on its sides included.
/// A line with a point no vertex is welded to is empty.
pub fn weld_tagged_lines(
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    tags: &[u32],
    eps: f64,
    lines: &[Vec<Vec3>],
) -> (Vec<Vec3>, Vec<[u32; 3]>, Vec<u32>, Vec<Vec<u32>>) {
    // Merge on a grid of cell `eps` (a vertex joins one already in its own or a neighbour cell).
    let cell = |p: Vec3| [(p.x / eps).round() as i64, (p.y / eps).round() as i64, (p.z / eps).round() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut verts: Vec<Vec3> = Vec::new();
    let mut remap = Vec::with_capacity(positions.len());
    for p in positions {
        let c = cell(*p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &i in grid.get(&[c[0].saturating_add(dx), c[1].saturating_add(dy), c[2].saturating_add(dz)]).into_iter().flatten() {
                        if verts.get(i as usize).is_some_and(|q| q.dist(*p) <= eps) {
                            found = Some(i);
                            break 'search;
                        }
                    }
                }
            }
        }
        let i = match found {
            Some(i) => i,
            None => {
                let i = u32::try_from(verts.len()).unwrap_or(u32::MAX);
                verts.push(*p);
                grid.entry(c).or_default().push(i);
                i
            }
        };
        remap.push(i);
    }
    // The lines' points: the vertex each was welded to.
    let find = |p: &Vec3| -> Option<u32> {
        let c = cell(*p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &i in grid.get(&[c[0].saturating_add(dx), c[1].saturating_add(dy), c[2].saturating_add(dz)]).into_iter().flatten() {
                        if verts.get(i as usize).is_some_and(|q| q.dist(*p) <= eps) {
                            return Some(i);
                        }
                    }
                }
            }
        }
        None
    };
    let mut paths: Vec<Vec<u32>> = lines.iter().map(|l| l.iter().map(find).collect::<Option<Vec<u32>>>().unwrap_or_default()).collect();
    for path in &mut paths {
        path.dedup();
    }
    // Every split side, with the vertices put on it (from its start).
    let mut split_sides: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    let (mut tris, mut tags): (Vec<[u32; 3]>, Vec<u32>) = triangles
        .iter()
        .enumerate()
        .filter_map(|(k, t)| {
            let (a, b, c) = (*remap.get(t[0] as usize)?, *remap.get(t[1] as usize)?, *remap.get(t[2] as usize)?);
            (a != b && b != c && c != a).then_some(([a, b, c], tags.get(k).copied().unwrap_or(0)))
        })
        .unzip();
    // T-junctions: split open sides at open-side vertices lying on them.
    for _ in 0..8 {
        let mut half: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &tris {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *half.entry((a, b)).or_default() += 1;
            }
        }
        let open: Vec<(u32, u32)> = half.keys().filter(|(a, b)| !half.contains_key(&(*b, *a))).copied().collect();
        if open.is_empty() || open.len() > 200_000 {
            break;
        }
        let mut cand: Vec<u32> = open.iter().flat_map(|(a, b)| [*a, *b]).collect();
        cand.sort_unstable();
        cand.dedup();
        // Vertices on each open side, ordered from its start.
        let mut splits: HashMap<(u32, u32), Vec<(f64, u32)>> = HashMap::new();
        for &(a, b) in &open {
            let (Some(pa), Some(pb)) = (verts.get(a as usize), verts.get(b as usize)) else { continue };
            let d = *pb - *pa;
            let l2 = d.dot(d);
            if !(l2 > 0.0) {
                continue;
            }
            let mut on: Vec<(f64, u32)> = cand
                .iter()
                .filter(|v| **v != a && **v != b)
                .filter_map(|v| {
                    let p = *verts.get(*v as usize)?;
                    let t = (p - *pa).dot(d) / l2;
                    (t > 1e-9 && t < 1.0 - 1e-9 && (*pa + d * t).dist(p) <= eps * 4.0).then_some((t, *v))
                })
                .collect();
            if !on.is_empty() {
                on.sort_by(|x, y| x.0.total_cmp(&y.0));
                splits.insert((a, b), on);
            }
        }
        if splits.is_empty() {
            break;
        }
        for (side, on) in &splits {
            split_sides.insert(*side, on.iter().map(|(_, v)| *v).collect());
        }
        let mut next = Vec::with_capacity(tris.len() + splits.len() * 2);
        let mut next_tags = Vec::with_capacity(next.capacity());
        for (t, tag) in tris.iter().zip(&tags) {
            // Rotate so the split side (if any) is (t0, t1); split one side per pass.
            let rot = [[t[0], t[1], t[2]], [t[1], t[2], t[0]], [t[2], t[0], t[1]]];
            match rot.iter().find_map(|r| splits.get(&(r[0], r[1])).map(|s| (r, s))) {
                Some((r, s)) => {
                    let mut prev = r[0];
                    for (_, v) in s {
                        next.push([prev, *v, r[2]]);
                        next_tags.push(*tag);
                        prev = *v;
                    }
                    next.push([prev, r[1], r[2]]);
                    next_tags.push(*tag);
                }
                None => {
                    next.push(*t);
                    next_tags.push(*tag);
                }
            }
        }
        tris = next;
        tags = next_tags;
    }
    if !split_sides.is_empty() {
        for path in &mut paths {
            *path = expand_splits(path, &split_sides);
        }
    }
    (verts, tris, tags, paths)
}

/// `path` with the vertices splits put on its sides (either way round) inserted, splits of
/// splits too (each pass of [`weld_tagged_lines`] splits a side at most once more).
fn expand_splits(path: &[u32], splits: &HashMap<(u32, u32), Vec<u32>>) -> Vec<u32> {
    let mut out: Vec<u32> = path.to_vec();
    for _ in 0..8 {
        let mut next = Vec::with_capacity(out.len());
        let mut changed = false;
        for (k, v) in out.iter().enumerate() {
            if let Some(prev) = k.checked_sub(1).and_then(|k| out.get(k)) {
                if let Some(on) = splits.get(&(*prev, *v)) {
                    next.extend(on.iter().copied());
                    changed = true;
                } else if let Some(on) = splits.get(&(*v, *prev)) {
                    next.extend(on.iter().rev().copied());
                    changed = true;
                }
            }
            next.push(*v);
        }
        out = next;
        if !changed {
            break;
        }
    }
    out
}

/// The model part's XML.
pub fn model_xml(objects: &[MeshObject]) -> String {
    model_xml_with(objects, &[])
}

/// The model part's XML with model metadata after `Application`: (3MF metadata name, such as
/// `Title` or `Designer`, and its text).
pub fn model_xml_with(objects: &[MeshObject], metadata: &[(&str, &str)]) -> String {
    model_xml_parts(objects, &[], metadata)
}

/// [`model_xml_with`], with `parts`: each a name and the indices of two or more `objects` written
/// as one object whose `<components>` are their meshes, built once, so a slicer loads them as
/// one object with parts (#8139). An index out of range or already in an earlier part is left
/// out of it, and a part left with fewer than two objects is not made: those objects are built
/// on their own, as every object not in a part is.
pub fn model_xml_parts(objects: &[MeshObject], parts: &[(String, Vec<usize>)], metadata: &[(&str, &str)]) -> String {
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let ft = if objects.iter().any(MeshObject::tagged) { format!(" xmlns:ft=\"{FACE_TAGS_NS}\"") } else { String::new() };
    let _ = writeln!(x, "<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"{CORE_NS}\"{ft}>");
    x.push_str(" <metadata name=\"Application\">SolidCraft</metadata>\n");
    for (name, text) in metadata {
        let _ = writeln!(x, " <metadata name=\"{}\">{}</metadata>", esc(name), esc(text));
    }
    x.push_str(" <resources>\n");
    let colored = objects.iter().any(|o| o.color.is_some() || o.transparency > 0.0 || !o.looks.is_empty());
    // Base material group id 1 (each object's own colour, then its faces' colours); objects
    // from 2. A see-through colour carries its alpha (#RRGGBBAA).
    let display = |c: [f32; 3], transparency: f32| {
        let c = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let alpha = if transparency > 0.0 { format!("{:02X}", ((1.0 - transparency.clamp(0.0, 1.0)) * 255.0).round() as u8) } else { String::new() };
        format!("#{:02X}{:02X}{:02X}{alpha}", c[0], c[1], c[2])
    };
    let mut first = Vec::with_capacity(objects.len());
    if colored {
        x.push_str("  <basematerials id=\"1\">\n");
        let mut n = 0usize;
        for o in objects {
            first.push(n);
            let _ =
                writeln!(x, "   <base name=\"{}\" displaycolor=\"{}\"/>", esc(&o.name), display(o.color.unwrap_or([0.7, 0.7, 0.7]), o.transparency));
            n += 1;
            for (i, (c, t)) in o.looks.iter().enumerate() {
                let _ = writeln!(x, "   <base name=\"{} face {}\" displaycolor=\"{}\"/>", esc(&o.name), i + 1, display(*c, *t));
                n += 1;
            }
        }
        x.push_str("  </basematerials>\n");
    }
    for (k, o) in objects.iter().enumerate() {
        let id = k + 2;
        let own = first.get(k).copied().unwrap_or(0);
        let mat = if colored { format!(" pid=\"1\" pindex=\"{own}\"") } else { String::new() };
        let faces = if o.tagged() { format!(" ft:faces=\"{}\"", o.faces.max(1)) } else { String::new() };
        let _ = writeln!(x, "  <object id=\"{id}\" type=\"model\" name=\"{}\"{mat}{faces}>", esc(&o.name));
        x.push_str("   <mesh>\n    <vertices>\n");
        let (verts, tris) = indexed(o);
        for v in &verts {
            // Shortest round-trip form: f64 comes back exactly, f32 as written before.
            if o.tagged() {
                let _ = writeln!(x, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", v[0], v[1], v[2]);
            } else {
                let _ = writeln!(x, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", v[0] as f32, v[1] as f32, v[2] as f32);
            }
        }
        x.push_str("    </vertices>\n    <triangles>\n");
        for t in &tris {
            let [a, b, c] = t.v;
            let _ = write!(x, "     <triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"");
            if colored && t.look > 0 && (t.look as usize) <= o.looks.len() {
                let _ = write!(x, " pid=\"1\" p1=\"{}\"", own + t.look as usize);
            }
            if o.tagged() {
                let _ = write!(x, " ft:face=\"{}\"", t.face);
            }
            x.push_str("/>\n");
        }
        x.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    // Each part an object after the meshes it names (a component refers back), built in their
    // place; the meshes in it are not built on their own.
    let mut in_part = vec![false; objects.len()];
    let mut built: Vec<usize> = Vec::new();
    for (name, members) in parts {
        let mut kept: Vec<usize> = Vec::new();
        for &k in members {
            if k < objects.len() && !in_part[k] && !kept.contains(&k) {
                kept.push(k);
            }
        }
        if kept.len() < 2 {
            continue;
        }
        let id = objects.len() + 2 + built.len();
        let _ = writeln!(x, "  <object id=\"{id}\" type=\"model\" name=\"{}\">", esc(name));
        x.push_str("   <components>\n");
        for &k in &kept {
            in_part[k] = true;
            let _ = writeln!(x, "    <component objectid=\"{}\"/>", k + 2);
        }
        x.push_str("   </components>\n  </object>\n");
        built.push(id);
    }
    x.push_str(" </resources>\n <build>\n");
    for k in (0..objects.len()).filter(|&k| !in_part[k]) {
        let _ = writeln!(x, "  <item objectid=\"{}\"/>", k + 2);
    }
    for id in built {
        let _ = writeln!(x, "  <item objectid=\"{id}\"/>");
    }
    x.push_str(" </build>\n</model>\n");
    x
}

/// A 3MF package of meshes.
pub fn write_3mf(objects: &[MeshObject]) -> Result<Vec<u8>> {
    write_3mf_with(objects, &[])
}

/// A 3MF package of meshes with model metadata (see [`model_xml_with`]).
pub fn write_3mf_with(objects: &[MeshObject], metadata: &[(&str, &str)]) -> Result<Vec<u8>> {
    write_3mf_parts(objects, &[], metadata)
}

/// A 3MF package of meshes, some of them written as objects with parts (see [`model_xml_parts`]).
pub fn write_3mf_parts(objects: &[MeshObject], parts: &[(String, Vec<usize>)], metadata: &[(&str, &str)]) -> Result<Vec<u8>> {
    if objects.is_empty() {
        return Err(IoError::Empty);
    }
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n</Types>\n";
    let rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n <Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"{MODEL_REL}\"/>\n</Relationships>\n"
    );
    let model = model_xml_parts(objects, parts, metadata);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [("[Content_Types].xml", content_types.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("3D/3dmodel.model", model.as_bytes())]
    {
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .large_file(data.len() as u64 >= u32::MAX as u64);
        zip.start_file(name, opts).map_err(|e| IoError::Invalid(format!("3MF: {e}")))?;
        zip.write_all(data).map_err(|e| IoError::Invalid(format!("3MF: {e}")))?;
    }
    zip.finish().map(|c| c.into_inner()).map_err(|e| IoError::Invalid(format!("3MF: {e}")))
}

// ---------------------------------------------------------------- reading

fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(IoError::Invalid(format!("3MF: {}", msg.into())))
}

/// The name a part is looked up by: `%XX` escapes decoded, no leading `/`, in lower case (part
/// names are URIs and compare case-insensitively).
fn part_key(name: &str) -> String {
    let b = name.as_bytes();
    let hex = |k: usize| b.get(k).and_then(|h| char::from(*h).to_digit(16));
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match (c, hex(i + 1), hex(i + 2)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b'\\', ..) => {
                out.push(b'/');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).trim_start_matches('/').to_ascii_lowercase()
}

fn read_part(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>> {
    let want = part_key(name);
    let Some(idx) = (0..zip.len()).find(|i| zip.name_for_index(*i).and_then(|n| n.ok()).is_some_and(|n| part_key(&n) == want)) else {
        return Ok(None);
    };
    match read_entry(zip, idx, name, MAX_PART)? {
        Some(data) => Ok(Some(data)),
        None => bad(format!("{name} expands to more than 1 GB")),
    }
}

/// Entry `idx` uncompressed, or `None` when it expands to more than `limit` bytes (zip bombs stop
/// there).
fn read_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, idx: usize, name: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    let mut f = zip.by_index(idx).map_err(|e| IoError::Invalid(format!("3MF {name}: {e}")))?;
    if f.size() > limit {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(f.size().min(16 << 20) as usize);
    (&mut f).take(limit.saturating_add(1)).read_to_end(&mut out).map_err(|e| IoError::Invalid(format!("3MF {name}: {e}")))?;
    Ok((out.len() as u64 <= limit).then_some(out))
}

fn attrs(e: &BytesStart) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for a in e.attributes().flatten() {
        let k = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
        let v = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
        m.insert(k, v);
    }
    m
}

/// Row-vector affine transform `[m00 m01 m02 m10 … m32]` (3MF order).
type Xf = [f64; 12];
const IDENTITY: Xf = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

fn parse_xf(s: Option<&String>) -> Result<Xf> {
    let Some(s) = s else { return Ok(IDENTITY) };
    let v: Vec<f64> = s
        .split_whitespace()
        .map(|t| t.parse::<f64>().ok().filter(|x| x.is_finite() && x.abs() < 1e12))
        .collect::<Option<_>>()
        .ok_or_else(|| IoError::Invalid(format!("3MF: bad transform `{s}`")))?;
    let mut m = IDENTITY;
    if v.len() != 12 {
        return bad(format!("transform with {} numbers", v.len()));
    }
    m.copy_from_slice(&v);
    Ok(m)
}

/// `xf` with its translation, given in a model's `unit`, in millimetres.
fn in_mm(mut xf: Xf, unit: f64) -> Xf {
    for t in xf.iter_mut().skip(9) {
        *t *= unit;
    }
    xf
}

fn apply(m: &Xf, p: Vec3) -> Vec3 {
    Vec3::new(p.x * m[0] + p.y * m[3] + p.z * m[6] + m[9], p.x * m[1] + p.y * m[4] + p.z * m[7] + m[10], p.x * m[2] + p.y * m[5] + p.z * m[8] + m[11])
}

/// `a` then `b`.
fn compose(a: &Xf, b: &Xf) -> Xf {
    let r = |i: usize| Vec3::new(a[i * 3], a[i * 3 + 1], a[i * 3 + 2]);
    let lin = |v: Vec3| Vec3::new(v.x * b[0] + v.y * b[3] + v.z * b[6], v.x * b[1] + v.y * b[4] + v.z * b[7], v.x * b[2] + v.y * b[5] + v.z * b[8]);
    let (x, y, z, t) = (lin(r(0)), lin(r(1)), lin(r(2)), apply(b, r(3)));
    [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, t.x, t.y, t.z]
}

fn det(m: &Xf) -> f64 {
    m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6])
}

/// A model part named by `p:path`: its key ([`part_key`]) and the name as written, for messages.
struct PartName {
    key: String,
    path: String,
}

/// A component or build item: object `id` placed by `xf`, in model part `part` or, without one,
/// in the part that names it.
struct Ref {
    part: Option<PartName>,
    id: u32,
    xf: Xf,
}

#[derive(Default)]
struct Object {
    name: String,
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    components: Vec<Ref>,
    material: Option<(u32, usize)>,
}

/// One model part. Positions and translations are in millimetres (converted with the part's
/// unit as it is read), so objects of parts in different units fit together. Objects are kept in
/// id order, so the parts they name are read, and reported, in the same order every run.
#[derive(Default)]
struct Model {
    objects: BTreeMap<u32, Object>,
    materials: HashMap<u32, Vec<Option<[f32; 3]>>>,
    build: Vec<Ref>,
}

/// The model parts of a package by key: the root model's key, the parts read, the parts that are
/// in the package but could not be read (with the error), and each part's name as first written.
struct Package {
    root: String,
    parts: HashMap<String, Model>,
    failed: HashMap<String, String>,
    names: HashMap<String, String>,
}

impl Package {
    /// Part `key` as the package names it.
    fn shown<'a>(&'a self, key: &'a str) -> &'a str {
        self.names.get(key).map_or(key, String::as_str)
    }
}

fn color(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 && h.len() != 8 {
        return None;
    }
    let c = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok()).map(|v| v as f32 / 255.0);
    Some([c(0)?, c(2)?, c(4)?])
}

fn unit_mm(u: &str) -> Option<f64> {
    Some(match u {
        "micron" => 1e-3,
        "millimeter" => 1.0,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        _ => return None,
    })
}

/// The model part's name in `p:path`, if any.
fn part_name(a: &HashMap<String, String>) -> Option<PartName> {
    a.get("path").map(|p| PartName { key: part_key(p), path: p.clone() })
}

/// Model part `xml`. Only the `root` model's build items are read: the spec has readers ignore
/// the build sections of the other parts (slicers leave them empty or point them anywhere).
fn parse_model(xml: &[u8], root: bool) -> Result<Model> {
    let mut rd = Reader::from_reader(xml);
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut m = Model::default();
    let mut unit = 1.0;
    let mut depth = 0usize;
    let mut cur: Option<(u32, Object)> = None;
    let mut cur_mat: Option<u32> = None;
    let mut elements = 0usize;
    let mut seen_model = false;
    loop {
        let ev = rd.read_event_into(&mut buf).map_err(|e| IoError::Invalid(format!("3MF model XML: {e}")))?;
        let (e, empty) = match &ev {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                match end.local_name().as_ref() {
                    b"object" => {
                        if let Some((id, o)) = cur.take() {
                            m.objects.insert(id, o);
                        }
                    }
                    b"basematerials" => cur_mat = None,
                    _ => {}
                }
                (None, false)
            }
            Event::Eof => break,
            _ => (None, false),
        };
        let Some(e) = e else {
            buf.clear();
            continue;
        };
        if !empty {
            depth += 1;
            if depth > MAX_XML_DEPTH {
                return bad("XML nested too deeply");
            }
        }
        elements += 1;
        if elements > MAX_ELEMENTS * 2 {
            return bad("model too large");
        }
        let a = attrs(&e);
        let num = |k: &str| a.get(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
        let id = |k: &str| a.get(k).and_then(|v| v.trim().parse::<u32>().ok());
        match e.local_name().as_ref() {
            b"model" => {
                seen_model = true;
                if let Some(u) = a.get("unit") {
                    unit = unit_mm(u).ok_or_else(|| IoError::Invalid(format!("3MF: unknown unit `{u}`")))?;
                }
            }
            b"basematerials" => {
                let gid = id("id").ok_or_else(|| IoError::Invalid("3MF: base materials without id".into()))?;
                m.materials.insert(gid, Vec::new());
                cur_mat = if empty { None } else { Some(gid) };
            }
            b"base" => {
                if let Some(g) = cur_mat.and_then(|g| m.materials.get_mut(&g)) {
                    g.push(a.get("displaycolor").and_then(|c| color(c)));
                }
            }
            b"object" => {
                let oid = id("id").ok_or_else(|| IoError::Invalid("3MF: object without id".into()))?;
                let o = Object {
                    name: a.get("name").cloned().unwrap_or_default(),
                    material: id("pid").map(|p| (p, a.get("pindex").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0))),
                    ..Default::default()
                };
                if empty {
                    m.objects.insert(oid, o);
                } else {
                    cur = Some((oid, o));
                }
            }
            b"vertex" => {
                let Some((_, o)) = cur.as_mut() else { return bad("vertex outside an object") };
                if o.positions.len() >= MAX_ELEMENTS {
                    return bad("too many vertices");
                }
                let (Some(x), Some(y), Some(z)) = (num("x"), num("y"), num("z")) else { return bad("vertex without x, y, z") };
                o.positions.push(Vec3::new(x, y, z) * unit);
            }
            b"triangle" => {
                let Some((_, o)) = cur.as_mut() else { return bad("triangle outside an object") };
                if o.triangles.len() >= MAX_ELEMENTS {
                    return bad("too many triangles");
                }
                let (Some(a1), Some(a2), Some(a3)) = (id("v1"), id("v2"), id("v3")) else { return bad("triangle without v1, v2, v3") };
                o.triangles.push([a1, a2, a3]);
            }
            b"component" => {
                let Some((_, o)) = cur.as_mut() else { return bad("component outside an object") };
                let target = id("objectid").ok_or_else(|| IoError::Invalid("3MF: component without objectid".into()))?;
                // The spec allows `p:path` only in the root model's components; a reader can still
                // follow it from any part (loops and depth are checked when placing).
                o.components.push(Ref { part: part_name(&a), id: target, xf: in_mm(parse_xf(a.get("transform"))?, unit) });
            }
            b"item" if root => {
                let target = id("objectid").ok_or_else(|| IoError::Invalid("3MF: build item without objectid".into()))?;
                m.build.push(Ref { part: part_name(&a), id: target, xf: in_mm(parse_xf(a.get("transform"))?, unit) });
            }
            _ => {}
        }
        buf.clear();
    }
    if !seen_model {
        return bad("no <model> element");
    }
    Ok(m)
}

/// Queue the model parts `m` names that were not seen yet, noting how each is spelled (`m.build`
/// is empty but for the root).
fn queue_parts(m: &Model, names: &mut HashMap<String, String>, pending: &mut Vec<String>) -> Result<()> {
    for p in m.objects.values().flat_map(|o| &o.components).chain(&m.build).filter_map(|r| r.part.as_ref()) {
        if !names.contains_key(&p.key) {
            // A package holds no more parts than it has entries.
            if names.len() >= MAX_ENTRIES {
                return bad(format!("objects name more than {MAX_ENTRIES} model parts"));
            }
            names.insert(p.key.clone(), p.path.clone());
            pending.push(p.key.clone());
        }
    }
    Ok(())
}

/// An error reading model part `name`, saying which part it was.
fn in_part(name: &str, e: IoError) -> String {
    match e {
        IoError::Invalid(msg) => {
            let msg = msg.strip_prefix("3MF: ").or_else(|| msg.strip_prefix("3MF ")).unwrap_or(&msg);
            format!("3MF model part `{name}`: {msg}")
        }
        e => e.to_string(),
    }
}

/// Where Bambu Studio keeps a project's per-object settings, among them the objects' names (its
/// root model's objects have none): `<config><object id="N"><metadata key="name" value="…"/>`,
/// with `N` the root model object a build item places. The `<part>` elements inside describe the
/// object's volumes.
const SLICER_SETTINGS: &str = "Metadata/model_settings.config";

/// The object names in slicer settings `xml` ([`SLICER_SETTINGS`]), by object id. Only an
/// object's own `name` counts (the first, if there are several); its parts' names and all other
/// settings are left alone.
fn slicer_names(xml: &[u8]) -> std::result::Result<HashMap<u32, String>, String> {
    let mut rd = Reader::from_reader(xml);
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut names = HashMap::new();
    let mut depth = 0usize;
    let mut elements = 0usize;
    // The object being read: its id (if it has a usable one) and the depth of its element.
    let mut object: Option<(Option<u32>, usize)> = None;
    loop {
        let ev = rd.read_event_into(&mut buf).map_err(|e| e.to_string())?;
        let start = matches!(ev, Event::Start(_));
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                let at = depth + 1;
                if start {
                    depth = at;
                    if depth > MAX_XML_DEPTH {
                        return Err("XML nested too deeply".into());
                    }
                }
                elements += 1;
                if elements > MAX_ELEMENTS {
                    return Err("too many elements".into());
                }
                match (e.local_name().as_ref(), object) {
                    (b"object", None) if at == 2 && start => {
                        object = Some((attrs(e).get("id").and_then(|v| v.trim().parse::<u32>().ok()), at));
                    }
                    (b"metadata", Some((Some(id), d))) if at == d + 1 => {
                        let a = attrs(e);
                        if a.get("key").is_some_and(|k| k == "name")
                            && let Some(v) = a.get("value").map(|v| v.trim()).filter(|v| !v.is_empty())
                        {
                            names.entry(id).or_insert_with(|| v.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Event::End(_) => {
                if object.is_some_and(|(_, d)| d == depth) {
                    object = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(names)
}

/// Append object `id` of model part `part` (and its components) placed by `xf` to `out`.
fn flatten<'a>(
    pkg: &'a Package,
    part: &'a str,
    id: u32,
    xf: &Xf,
    out: &mut MeshObject,
    stack: &mut Vec<(&'a str, u32)>,
    total: &mut usize,
) -> Result<()> {
    // Loops are keyed by (part, object): parts number their objects on their own, so object 1 of
    // the root placing object 1 of another part is no loop.
    if stack.contains(&(part, id)) {
        return bad("components place each other in a loop");
    }
    if stack.len() > MAX_COMPONENT_DEPTH {
        return bad(format!("components nested more than {MAX_COMPONENT_DEPTH} deep"));
    }
    let Some(m) = pkg.parts.get(part) else {
        return Err(IoError::Invalid(pkg.failed.get(part).cloned().unwrap_or_else(|| format!("3MF: no model part `{}`", pkg.shown(part)))));
    };
    let o = m.objects.get(&id).ok_or_else(|| {
        IoError::Invalid(if part == pkg.root {
            format!("3MF: missing object {id}")
        } else {
            format!("3MF: missing object {id} in model part `{}`", pkg.shown(part))
        })
    })?;
    // Every placement costs at least one unit of the budget, so objects that only place other
    // objects cannot fan out (hundreds of components per level, nested) into endless visits.
    *total = total.saturating_add(o.triangles.len().max(1));
    if *total > MAX_ELEMENTS {
        return bad("too many triangles or placements after placing components");
    }
    let n = o.positions.len();
    // One object can be placed by thousands of build items and components, and each placement
    // needs its own transformed copy of the vertices. Copying only the vertices the triangles
    // reach keeps that cost tied to the triangle budget above; copying the whole vertex array
    // let an object with many vertices and one triangle multiply into tens of gigabytes.
    let mut used: Vec<u32> = o.triangles.iter().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    if used.last().is_some_and(|v| *v as usize >= n) {
        return bad(format!("object {id}: triangle refers to a missing vertex"));
    }
    let base = u32::try_from(out.positions.len()).map_err(|_| IoError::Invalid("3MF: too many vertices".into()))?;
    if base as usize + used.len() > u32::MAX as usize {
        return bad("too many vertices");
    }
    for v in &used {
        let Some(p) = o.positions.get(*v as usize) else {
            return bad(format!("object {id}: triangle refers to a missing vertex"));
        };
        out.positions.push(apply(xf, *p));
    }
    // Vertices keep their relative order, so a mesh whose triangles use all of them is
    // numbered exactly as it was in the file.
    let at = |v: u32| used.binary_search(&v).ok().map(|i| base + i as u32);
    let flip = det(xf) < 0.0;
    for t in &o.triangles {
        let mut q = [0u32; 3];
        for (slot, v) in q.iter_mut().zip(t) {
            let Some(i) = at(*v) else {
                return bad(format!("object {id}: triangle refers to a missing vertex"));
            };
            *slot = i;
        }
        out.triangles.push(if flip { [q[0], q[2], q[1]] } else { q });
    }
    if out.color.is_none() {
        out.color = o.material.and_then(|(g, i)| m.materials.get(&g).and_then(|g| g.get(i).copied().flatten()));
    }
    stack.push((part, id));
    for c in &o.components {
        // Without `p:path` a component is in the part that names it, not in the root.
        flatten(pkg, c.part.as_ref().map_or(part, |p| p.key.as_str()), c.id, &compose(&c.xf, xf), out, stack, total)?;
    }
    stack.pop();
    Ok(())
}

/// Meshes of a 3MF package (one per build item, in millimetres) and warnings.
pub fn read_3mf(bytes: &[u8]) -> Result<(Vec<MeshObject>, Vec<String>)> {
    if bytes.len() > MAX_3MF_BYTES {
        return bad(format!("file too large ({} MB)", bytes.len() >> 20));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| IoError::Invalid(format!("3MF is not a zip package: {e}")))?;
    if zip.len() > MAX_ENTRIES {
        return bad(format!("{} zip entries", zip.len()));
    }
    // The model part named by the package relationships, else the usual name.
    let mut root = "3D/3dmodel.model".to_string();
    if let Some(rels) = read_part(&mut zip, "_rels/.rels")? {
        let mut rd = Reader::from_reader(rels.as_slice());
        let mut buf = Vec::new();
        loop {
            match rd.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"Relationship" => {
                    let a = attrs(&e);
                    if a.get("Type").is_some_and(|t| t.ends_with("/3dmodel"))
                        && let Some(t) = a.get("Target")
                    {
                        root = t.trim_start_matches('/').to_string();
                        break;
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let xml = read_part(&mut zip, &root)?.ok_or_else(|| IoError::Invalid(format!("3MF: no model part `{root}`")))?;
    let root_name = root;
    let root = part_key(&root_name);
    let model = parse_model(&xml, true)?;
    // All model parts together may expand to 1 GB; only one is held as XML at a time.
    let mut model_bytes = xml.len() as u64;
    drop(xml);
    // The model parts objects name (production extension), each read once. The entries are
    // indexed first, so a package naming thousands of parts costs no scan per name. A name with
    // no entry is left out and a part that can't be read is noted: either way only the
    // placements that need it fail, and say why.
    let mut entries = HashMap::new();
    for i in 0..zip.len() {
        if let Some(Ok(n)) = zip.name_for_index(i) {
            entries.entry(part_key(&n)).or_insert(i);
        }
    }
    let mut names = HashMap::from([(root.clone(), root_name)]);
    let mut pending = Vec::new();
    queue_parts(&model, &mut names, &mut pending)?;
    let mut parts = HashMap::from([(root.clone(), model)]);
    let mut failed = HashMap::new();
    while let Some(key) = pending.pop() {
        let Some(&idx) = entries.get(&key) else { continue };
        let name = names.get(&key).cloned().unwrap_or_else(|| key.clone());
        let read = match read_entry(&mut zip, idx, &name, MAX_PART.saturating_sub(model_bytes)) {
            Ok(Some(xml)) => {
                model_bytes = model_bytes.saturating_add(xml.len() as u64);
                parse_model(&xml, false)
            }
            // The package-wide caps refuse the whole package, not just this part.
            Ok(None) => return bad("model parts expand to more than 1 GB"),
            Err(e) => Err(e),
        };
        match read {
            Ok(m) => {
                queue_parts(&m, &mut names, &mut pending)?;
                parts.insert(key, m);
            }
            Err(e) => {
                failed.insert(key, in_part(&name, e));
            }
        }
    }
    let pkg = Package { root, parts, failed, names };
    let Some(model) = pkg.parts.get(&pkg.root) else { return bad("no root model") };
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut total = 0usize;
    // Bodies of root model objects without a name of their own: (index in `out`, object id).
    let mut unnamed = Vec::new();
    for (k, item) in model.build.iter().enumerate() {
        let part = item.part.as_ref().map_or(pkg.root.as_str(), |p| p.key.as_str());
        let mut mo = MeshObject::default();
        match flatten(&pkg, part, item.id, &item.xf, &mut mo, &mut Vec::new(), &mut total) {
            Ok(()) if !mo.triangles.is_empty() => {
                let name = pkg.parts.get(part).and_then(|m| m.objects.get(&item.id)).map(|o| o.name.trim().to_string()).filter(|n| !n.is_empty());
                if name.is_none() && part == pkg.root {
                    unnamed.push((out.len(), item.id));
                }
                mo.name = name.unwrap_or_else(|| format!("Mesh{}", k + 1));
                out.push(mo);
            }
            Ok(()) => warnings.push(format!("build item {} has no triangles", k + 1)),
            Err(e) => warnings.push(format!("build item {}: {e}", k + 1)),
        }
    }
    // A slicer project names its objects in its settings (#3858). Those are read only when a body
    // needs a name, within what the model parts left of the budget, and settings that can't be
    // read only leave the generic names.
    if !unnamed.is_empty()
        && let Some(&idx) = entries.get(&part_key(SLICER_SETTINGS))
    {
        let names = match read_entry(&mut zip, idx, SLICER_SETTINGS, MAX_PART.saturating_sub(model_bytes)) {
            Ok(Some(xml)) => slicer_names(&xml),
            Ok(None) => Err("it expands past what the model parts left of the 1 GB budget".into()),
            Err(e) => Err(e.to_string()),
        };
        match names {
            Ok(names) => {
                for (i, id) in unnamed {
                    if let (Some(o), Some(name)) = (out.get_mut(i), names.get(&id)) {
                        o.name.clone_from(name);
                    }
                }
            }
            Err(e) => warnings.push(format!("object names in `{SLICER_SETTINGS}` not read: {e}")),
        }
    }
    if out.is_empty() {
        return bad(warnings.first().cloned().unwrap_or_else(|| "no meshes to build".into()));
    }
    Ok((out, warnings))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn tetra(name: &str) -> MeshObject {
        MeshObject {
            name: name.into(),
            positions: vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)],
            triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]],
            color: None,
            ..Default::default()
        }
    }

    /// #8139: objects in a part are written once as meshes and built once, as one object whose
    /// components are those meshes; the others are built on their own, as before. Read back, the
    /// part is one object named after it, with every triangle of its meshes.
    #[test]
    fn a_part_is_one_object_whose_components_are_its_meshes() {
        let objects = [tetra("Cube"), tetra("Dome"), tetra("Loose")];
        let x = model_xml_parts(&objects, &[("Bracket:1".into(), vec![0, 1])], &[]);
        assert_eq!(x.matches("<mesh>").count(), 3, "{x}");
        assert!(x.contains("<object id=\"5\" type=\"model\" name=\"Bracket:1\">"), "{x}");
        assert!(x.contains("<component objectid=\"2\"/>") && x.contains("<component objectid=\"3\"/>"), "{x}");
        let build = x.split("<build>").nth(1).unwrap_or_default();
        assert_eq!(build.matches("<item ").count(), 2, "{x}");
        assert!(build.contains("objectid=\"4\"") && build.contains("objectid=\"5\""), "{x}");
        let (read, warnings) = read_3mf(&write_3mf_parts(&objects, &[("Bracket:1".into(), vec![0, 1])], &[]).unwrap()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let names: Vec<&str> = read.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Loose", "Bracket:1"]);
        assert_eq!(read[1].triangles.len(), 8);
        // Without parts the file is as before: one built object per mesh.
        assert_eq!(model_xml_parts(&objects, &[], &[]), model_xml_with(&objects, &[]));
        assert_eq!(model_xml_with(&objects, &[]).split("<build>").nth(1).unwrap_or_default().matches("<item ").count(), 3);
        // A part of one object, an index out of range, or one already in a part makes nothing new.
        for parts in [vec![("One".to_string(), vec![2])], vec![("Out".to_string(), vec![0, 7])]] {
            assert_eq!(model_xml_parts(&objects, &parts, &[]), model_xml_with(&objects, &[]), "{parts:?}");
        }
        let x = model_xml_parts(&objects, &[("A".into(), vec![0, 1]), ("B".into(), vec![1, 2])], &[]);
        assert!(x.contains("name=\"A\"") && !x.contains("name=\"B\""), "{x}");
    }

    /// A see-through object writes its alpha; triangles of faces with colours of their own
    /// point at their entries.
    #[test]
    fn face_colours_and_alpha() {
        let mut t = tetra("T");
        t.color = Some([0.125, 0.25, 0.75]);
        t.transparency = 0.5;
        t.looks = vec![([1.0, 0.0, 0.0], 0.0)];
        t.tri_looks = vec![0, 1, 0, 0];
        let model = model_xml(&[t]);
        assert!(model.contains("displaycolor=\"#2040BF80\""), "{model}");
        assert!(model.contains("displaycolor=\"#FF0000\""), "{model}");
        assert_eq!(model.matches("p1=\"1\"").count(), 1, "{model}");
    }

    /// A package of these parts (name, contents).
    pub(crate) fn package_bytes(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (name, data) in parts {
            zip.start_file(*name, o).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// A package of these text parts (name, contents).
    fn package_parts(parts: &[(&str, &str)]) -> Vec<u8> {
        package_bytes(&parts.iter().map(|(n, d)| (*n, d.as_bytes())).collect::<Vec<_>>())
    }

    fn package(model: &str, rels: Option<&str>) -> Vec<u8> {
        let mut parts: Vec<(&str, &str)> = rels.map(|r| ("_rels/.rels", r)).into_iter().collect();
        parts.push(("3D/3dmodel.model", model));
        package_parts(&parts)
    }

    const PRODUCTION: &str = r#"xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p""#;
    const OBJECTS_PART: &str = "3D/Objects/object_1.model";

    /// Object `id`: a tetrahedron with legs of `s` along the axes (volume s³/6).
    fn tet_object(id: u32, name: &str, s: f64) -> String {
        format!(
            r#"<object id="{id}" type="model" name="{name}"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="{s}" y="0" z="0"/><vertex x="0" y="{s}" z="0"/><vertex x="0" y="0" z="{s}"/></vertices><triangles><triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/><triangle v1="1" v2="2" v3="3"/><triangle v1="0" v2="3" v3="2"/></triangles></mesh></object>"#
        )
    }

    /// A model part holding `objects`, in `unit`, with no build items of its own.
    fn model_part(unit: &str, objects: &str) -> String {
        format!(r#"<?xml version="1.0"?><model unit="{unit}" {PRODUCTION}><resources>{objects}</resources><build/></model>"#)
    }

    fn volume(o: &MeshObject) -> f64 {
        let p = |i: u32| o.positions[i as usize];
        o.triangles.iter().map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2])))).sum::<f64>() / 6.0
    }

    /// The model parts (name, XML) of a multi-part project as slicers save it: the root model's
    /// object is two components in another model part, which holds the meshes.
    pub(crate) fn production_parts() -> [(&'static str, String); 2] {
        let root = format!(
            r#"<?xml version="1.0"?><model unit="millimeter" {PRODUCTION}><resources><object id="10" type="model" name="Pair"><components>
<component p:path="/3D/Objects/object_1.model" objectid="1"/>
<component p:path="/3D/Objects/object_1.model" objectid="2" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
</components></object></resources><build><item objectid="10" transform="1 0 0 0 1 0 0 0 1 0 0 2"/></build></model>"#
        );
        let objects = model_part("millimeter", &format!("{}{}", tet_object(1, "A", 1.0), tet_object(2, "B", 2.0)));
        [("3D/3dmodel.model", root), (OBJECTS_PART, objects)]
    }

    pub(crate) fn production_package() -> Vec<u8> {
        let [(r, root), (o, objects)] = production_parts();
        package_parts(&[(r, &root), (o, &objects)])
    }

    /// Objects kept in a model part of their own (3MF Production extension) come in with their
    /// components' and the build item's transforms (#2089).
    #[test]
    fn components_in_other_model_parts() {
        let (objs, warnings) = read_3mf(&production_package()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(objs.len(), 1);
        let o = &objs[0];
        assert_eq!(o.name, "Pair");
        assert_eq!(o.triangles.len(), 8);
        assert!((volume(o) - (1.0 + 8.0) / 6.0).abs() < 1e-9, "{}", volume(o));
        // B moved 5 along x by its component, both 2 up by the build item.
        let max_x = o.positions.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        assert!((max_x - 7.0).abs() < 1e-9, "{max_x}");
        assert!(o.positions.iter().all(|p| p.z >= 2.0 - 1e-9 && p.z <= 4.0 + 1e-9));
    }

    /// Build items can name another part too, part names compare without regard to case, and
    /// each part has its own unit: positions come out in millimetres, translations in the unit
    /// of the part that names them.
    #[test]
    fn build_items_in_other_model_parts_and_their_units() {
        let root = format!(
            r#"<model unit="millimeter" {PRODUCTION}><resources/><build>
<item p:path="/3D/Objects/object_1.model" objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
<item p:path="/3d/objects/OBJECT_1.model" objectid="2"/></build></model>"#
        );
        let objects = model_part("centimeter", &format!("{}{}", tet_object(1, "A", 1.0), tet_object(2, "B", 2.0)));
        let (objs, warnings) = read_3mf(&package_parts(&[("3D/3dmodel.model", &root), (OBJECTS_PART, &objects)])).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let names: Vec<&str> = objs.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
        // A: legs of 1 cm, moved 5 mm; B: legs of 2 cm.
        assert!((volume(&objs[0]) - 1000.0 / 6.0).abs() < 1e-6 && (volume(&objs[1]) - 8000.0 / 6.0).abs() < 1e-6);
        let xs = |o: &MeshObject| o.positions.iter().map(|p| p.x).fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert_eq!(xs(&objs[0]), (5.0, 15.0));
        assert_eq!(xs(&objs[1]), (0.0, 20.0));
    }

    #[test]
    fn hostile_model_parts_are_errors() {
        let root = |components: &str| {
            format!(
                r#"<model unit="millimeter" {PRODUCTION}><resources><object id="10" type="model"><components>{components}</components></object></resources><build><item objectid="10"/></build></model>"#
            )
        };
        let to_objects = r#"<component p:path="/3D/Objects/object_1.model" objectid="1"/>"#;
        let tet = model_part("millimeter", &tet_object(1, "A", 1.0));
        let looped = model_part(
            "millimeter",
            r#"<object id="1" type="model"><components><component p:path="/3D/3dmodel.model" objectid="10"/></components></object>"#,
        );
        let many: String = (0..=MAX_ENTRIES).map(|i| format!(r#"<component p:path="/p{i}.model" objectid="1"/>"#)).collect();
        // Each with the message it must give: before the production extension was read, every
        // one of these failed alike, on the first `p:path`.
        let cases = [
            // A part that isn't in the package (named as written), one outside it and an empty name.
            (
                package_parts(&[("3D/3dmodel.model", &root(r#"<component p:path="/3D/Objects/Missing.model" objectid="1"/>"#))]),
                "no model part `/3D/Objects/Missing.model`",
            ),
            (
                package_parts(&[("3D/3dmodel.model", &root(r#"<component p:path="/../../etc/passwd" objectid="1"/>"#)), (OBJECTS_PART, &tet)]),
                "no model part `/../../etc/passwd`",
            ),
            (package_parts(&[("3D/3dmodel.model", &root(r#"<component p:path="" objectid="1"/>"#)), (OBJECTS_PART, &tet)]), "no model part ``"),
            // An object the part doesn't have.
            (
                package_parts(&[
                    ("3D/3dmodel.model", &root(r#"<component p:path="/3D/Objects/object_1.model" objectid="7"/>"#)),
                    (OBJECTS_PART, &tet),
                ]),
                "missing object 7 in model part `/3D/Objects/object_1.model`",
            ),
            // A part that is no model.
            (
                package_parts(&[("3D/3dmodel.model", &root(to_objects)), (OBJECTS_PART, "not xml")]),
                "model part `/3D/Objects/object_1.model`: no <model> element",
            ),
            // Parts that place each other in a loop.
            (package_parts(&[("3D/3dmodel.model", &root(to_objects)), (OBJECTS_PART, &looped)]), "components place each other in a loop"),
            // More part names than a package can hold.
            (package_parts(&[("3D/3dmodel.model", &root(&many))]), "objects name more than 10000 model parts"),
        ];
        for (i, (c, msg)) in cases.iter().enumerate() {
            let err = read_3mf(c).map(|_| ()).unwrap_err().to_string();
            assert!(err.contains(msg), "case {i}: {err}");
        }
        // A placement whose part is missing is reported; the others still come in.
        let two = format!(
            r#"<model unit="millimeter" {PRODUCTION}><resources/><build><item p:path="/3D/Objects/object_1.model" objectid="1"/><item p:path="/gone.model" objectid="1"/></build></model>"#
        );
        let (objs, warnings) = read_3mf(&package_parts(&[("3D/3dmodel.model", &two), (OBJECTS_PART, &tet)])).unwrap();
        assert_eq!(objs.len(), 1);
        assert!(warnings.iter().any(|w| w.contains("build item 2") && w.contains("gone.model")), "{warnings:?}");
    }

    /// The root model: `objects`, and a build of `items`.
    fn root_model(objects: &str, items: &str) -> String {
        format!(r#"<?xml version="1.0"?><model unit="millimeter" {PRODUCTION}><resources>{objects}</resources><build>{items}</build></model>"#)
    }

    /// Object `id` made of components: (`p:path`, or "" for none, and object id).
    fn assembly(id: u32, components: &[(&str, u32)]) -> String {
        let c: String = components
            .iter()
            .map(|(path, oid)| match *path {
                "" => format!(r#"<component objectid="{oid}"/>"#),
                path => format!(r#"<component p:path="{path}" objectid="{oid}"/>"#),
            })
            .collect();
        format!(r#"<object id="{id}" type="model"><components>{c}</components></object>"#)
    }

    /// A component without `p:path` in another part is in that part, not in the root, which has
    /// an object of the same id too (T1 of the review of #2127).
    #[test]
    fn components_without_a_path_stay_in_their_part() {
        let root =
            root_model(&format!("{}{}", tet_object(2, "Root", 5.0), assembly(10, &[("/3D/Objects/object_1.model", 1)])), r#"<item objectid="10"/>"#);
        let objects = model_part("millimeter", &format!("{}{}", assembly(1, &[("", 2)]), tet_object(2, "Child", 2.0)));
        let (objs, warnings) = read_3mf(&package_parts(&[("3D/3dmodel.model", &root), (OBJECTS_PART, &objects)])).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(objs.len(), 1);
        assert!((volume(&objs[0]) - 8.0 / 6.0).abs() < 1e-9, "the child's object 2, not the root's: {}", volume(&objs[0]));
    }

    /// Parts number their objects on their own, so root object 1 placing object 1 of another part
    /// is no loop (T2).
    #[test]
    fn the_same_id_in_two_parts_is_no_loop() {
        let root = root_model(&assembly(1, &[("/3D/Objects/object_1.model", 1)]), r#"<item objectid="1"/>"#);
        let objects = model_part("millimeter", &tet_object(1, "A", 1.0));
        let (objs, warnings) = read_3mf(&package_parts(&[("3D/3dmodel.model", &root), (OBJECTS_PART, &objects)])).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(objs.len() == 1 && (volume(&objs[0]) - 1.0 / 6.0).abs() < 1e-9);
    }

    /// A package shaped like a slicer project (Bambu Studio, OrcaSlicer): object UUIDs, printable
    /// build items, thumbnails, slicer configs, and the meshes in a part of their own with an empty
    /// build (T3). Each object on the plate is one body with all its volumes, named as in the
    /// slicer's config (#3858). The rest of the config (which volumes are modifiers) isn't read, so
    /// the modifier comes in as solid.
    #[test]
    fn slicer_project_package() {
        let ns = r#"xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:BambuStudio="http://schemas.bambulab.com/package/2021" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p""#;
        let uuid = |n: u32| format!("{n:08x}-61cb-4c03-9d28-80fed5dfa1dc");
        let component = |oid: u32| {
            format!(
                r#"<component p:path="/3D/Objects/object_1.model" objectid="{oid}" p:UUID="{}" transform="1 0 0 0 1 0 0 0 1 0 0 0"/>"#,
                uuid(100 + oid)
            )
        };
        let root = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" {ns}>
 <metadata name="Application">BambuStudio-02.03.00.70</metadata>
 <metadata name="BambuStudio:3mfVersion">1</metadata>
 <resources>
  <object id="2" p:UUID="{}" type="model"><components>{}</components></object>
  <object id="4" p:UUID="{}" type="model"><components>{}{}</components></object>
 </resources>
 <build p:UUID="{}">
  <item objectid="2" p:UUID="{}" transform="1 0 0 0 1 0 0 0 1 128 128 0" printable="1"/>
  <item objectid="4" p:UUID="{}" transform="1 0 0 0 1 0 0 0 1 90 128 0" printable="1"/>
 </build>
</model>"#,
            uuid(2),
            component(1),
            uuid(4),
            component(3),
            component(5),
            uuid(200),
            uuid(201),
            uuid(202)
        );
        let mesh = |id: u32, s: f64| tet_object(id, "", s).replace(r#" name="""#, &format!(r#" p:UUID="{}""#, uuid(300 + id)));
        let objects = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" {ns}>
 <metadata name="BambuStudio:3mfVersion">1</metadata>
 <resources>{}{}{}</resources>
 <build/>
</model>"#,
            mesh(1, 10.0),
            mesh(3, 20.0),
            mesh(5, 4.0)
        );
        let settings = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <object id="2"><metadata key="name" value="Cube"/><part id="1" subtype="normal_part"><metadata key="name" value="Cube"/></part></object>
  <object id="4"><metadata key="name" value="Bracket"/><part id="3" subtype="normal_part"/><part id="5" subtype="modifier_part"/></object>
  <plate><metadata key="plater_id" value="1"/><metadata key="thumbnail_file" value="Metadata/plate_1.png"/></plate>
</config>"#;
        let content_types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/><Default Extension="png" ContentType="image/png"/></Types>"#;
        let rels = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel-1" Type="{MODEL_REL}"/><Relationship Target="/Metadata/plate_1.png" Id="rel-2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"/></Relationships>"#
        );
        let model_rels = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/Objects/object_1.model" Id="rel-1" Type="{MODEL_REL}"/></Relationships>"#
        );
        let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        let package = package_bytes(&[
            ("[Content_Types].xml", content_types.as_bytes()),
            ("_rels/.rels", rels.as_bytes()),
            ("3D/3dmodel.model", root.as_bytes()),
            ("3D/_rels/3dmodel.model.rels", model_rels.as_bytes()),
            (OBJECTS_PART, objects.as_bytes()),
            ("Metadata/model_settings.config", settings.as_bytes()),
            ("Metadata/project_settings.config", br#"{"layer_height": "0.2"}"#),
            ("Metadata/plate_1.png", png),
            ("Metadata/plate_1_small.png", png),
        ]);
        let (objs, warnings) = read_3mf(&package).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let names: Vec<&str> = objs.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Cube", "Bracket"]);
        assert_eq!((objs[0].triangles.len(), objs[1].triangles.len()), (4, 8));
        assert!((volume(&objs[0]) - 1000.0 / 6.0).abs() < 1e-6, "{}", volume(&objs[0]));
        assert!((volume(&objs[1]) - (8000.0 + 64.0) / 6.0).abs() < 1e-6, "{}", volume(&objs[1]));
        assert!(objs[0].positions.iter().all(|p| p.x >= 128.0 - 1e-9 && p.y >= 128.0 - 1e-9));
    }

    /// The body names and warnings of a package with slicer settings `settings` (or none): root
    /// objects 2 and 6 without a name and 4 named `Own`, each placing the tetrahedron of the
    /// objects part, one build item each, and a fourth build item placing that tetrahedron itself.
    fn named_by_settings(settings: Option<&str>) -> (Vec<String>, Vec<String>) {
        let path = "/3D/Objects/object_1.model";
        let own = assembly(4, &[(path, 1)]).replace(r#"type="model""#, r#"type="model" name="Own""#);
        let root = root_model(
            &format!("{}{own}{}", assembly(2, &[(path, 1)]), assembly(6, &[(path, 1)])),
            &format!(r#"<item objectid="2"/><item objectid="4"/><item objectid="6"/><item p:path="{path}" objectid="1"/>"#),
        );
        let part = model_part("millimeter", &tet_object(1, "", 1.0));
        let mut parts = vec![("3D/3dmodel.model", root.as_str()), (OBJECTS_PART, part.as_str())];
        parts.extend(settings.map(|s| (SLICER_SETTINGS, s)));
        let (objs, warnings) = read_3mf(&package_parts(&parts)).unwrap();
        (objs.into_iter().map(|o| o.name).collect(), warnings)
    }

    /// Slicer settings name the root objects that have no name of their own, by object id, with
    /// the object's first name, trimmed (#3858). A name in the model comes first; the names of
    /// an object's parts, settings outside an object, objects the build doesn't place and objects
    /// of other model parts change nothing.
    #[test]
    fn slicer_settings_name_unnamed_objects() {
        let settings = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <metadata key="name" value="Stray"/>
  <object id="2"><metadata key="name" value=" Left wall "/><metadata key="extruder" value="1"/><metadata key="name" value="Second"/>
    <part id="1" subtype="normal_part"><metadata key="name" value="Volume"/></part></object>
  <object id="4"><metadata key="name" value="From settings"/></object>
  <object id="6"><metadata key="name" value=""/><part id="1" subtype="normal_part"><metadata key="name" value="Volume only"/></part></object>
  <object id="1"><metadata key="name" value="Not a root object"/></object>
  <object id="9"><metadata key="name" value="Not placed"/></object>
  <object><metadata key="name" value="No id"/></object>
</config>"#;
        let (names, warnings) = named_by_settings(Some(settings));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(names, ["Left wall", "Own", "Mesh3", "Mesh4"]);
        // Without settings, the generic names as before.
        assert_eq!(named_by_settings(None), (vec!["Mesh1".into(), "Own".into(), "Mesh3".into(), "Mesh4".into()], Vec::new()));
    }

    /// Settings that can't be read never fail the import: the bodies keep the generic names, with
    /// a warning saying why. They are read only when a body needs a name, so a package whose
    /// objects all have one opens without a word about them.
    #[test]
    fn broken_slicer_settings_only_warn() {
        let generic: Vec<String> = ["Mesh1", "Own", "Mesh3", "Mesh4"].map(String::from).into();
        let deep = format!("<config>{}", "<object>".repeat(MAX_XML_DEPTH + 1));
        for (settings, why) in
            [(r#"<config><object id="2"><metadata key="name" value="A"/></config>"#, "config"), (deep.as_str(), "nested too deeply")]
        {
            let (names, warnings) = named_by_settings(Some(settings));
            assert_eq!(names, generic);
            assert!(
                warnings.len() == 1
                    && warnings[0].starts_with("object names in `Metadata/model_settings.config` not read: ")
                    && warnings[0].contains(why),
                "{warnings:?}"
            );
        }
        let [(r, root), (o, objects)] = production_parts();
        let (objs, warnings) = read_3mf(&package_parts(&[(r, &root), (o, &objects), (SLICER_SETTINGS, "<config><object>")])).unwrap();
        assert!(warnings.is_empty() && objs[0].name == "Pair", "{warnings:?}");
    }

    /// Placements in every part draw on one budget (T4): three parts fanning out 300 ways each
    /// (27 million placements of an empty object) stop at it, and only the build item that went
    /// over fails.
    ///
    /// The clock is not read: the budget is spent inside the reader, so the reader stops on its
    /// own and the test says what it wants to know, not how long it took. The one object that
    /// survives and the warning that names the build item that went over are what show the budget
    /// works (#2999 dropped the clock from the sibling tests the same way).
    #[test]
    fn parts_share_the_placement_budget() {
        let fan = |id: u32, path: &str| assembly(id, &vec![(path, 1); 300]);
        let root = root_model(&fan(10, "/3D/Objects/a.model"), r#"<item p:path="/3D/Objects/a.model" objectid="9"/><item objectid="10"/>"#);
        let a = model_part("millimeter", &format!("{}{}", fan(1, "/3D/Objects/b.model"), tet_object(9, "Tet", 1.0)));
        let b = model_part("millimeter", &fan(1, "/3D/Objects/c.model"));
        let c = model_part("millimeter", r#"<object id="1" type="model"/>"#);
        let package =
            package_parts(&[("3D/3dmodel.model", &root), ("3D/Objects/a.model", &a), ("3D/Objects/b.model", &b), ("3D/Objects/c.model", &c)]);
        let (objs, warnings) = read_3mf(&package).unwrap();
        assert_eq!(objs.len(), 1);
        assert!(warnings.len() == 1 && warnings[0].starts_with("build item 2") && warnings[0].contains("placements"), "{warnings:?}");
    }

    /// A loop through three parts with different object ids is found, and a chain of parts deeper
    /// than the cap stops there while one level less still comes in (T5).
    #[test]
    fn loops_and_deep_chains_across_parts() {
        let read = |parts: &[(String, String)]| read_3mf(&package_parts(&parts.iter().map(|(n, x)| (n.as_str(), x.as_str())).collect::<Vec<_>>()));
        let part = |name: &str, objects: String| (name.to_string(), model_part("millimeter", &objects));
        let looped = [
            ("3D/3dmodel.model".to_string(), root_model("", r#"<item p:path="/3D/Objects/a.model" objectid="1"/>"#)),
            part("3D/Objects/a.model", assembly(1, &[("/3D/Objects/b.model", 2)])),
            part("3D/Objects/b.model", assembly(2, &[("/3D/Objects/c.model", 3)])),
            part("3D/Objects/c.model", assembly(3, &[("/3D/Objects/a.model", 1)])),
        ];
        let err = read(&looped).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("components place each other in a loop"), "{err}");
        // Object k of part k places object k + 1 of part k + 1; the last one is a mesh.
        let chain = |n: u32| {
            let name = |k: u32| format!("3D/Objects/c{k}.model");
            let mut parts = vec![("3D/3dmodel.model".to_string(), root_model("", &format!(r#"<item p:path="/{}" objectid="1"/>"#, name(1))))];
            for k in 1..=n {
                let object = if k == n { tet_object(k, "End", 1.0) } else { assembly(k, &[(format!("/{}", name(k + 1)).as_str(), k + 1)]) };
                parts.push(part(&name(k), object));
            }
            read(&parts)
        };
        let deepest = MAX_COMPONENT_DEPTH as u32 + 1;
        let (objs, warnings) = chain(deepest).unwrap();
        assert!(objs.len() == 1 && warnings.is_empty(), "{warnings:?}");
        let err = chain(deepest + 1).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains(&format!("components nested more than {MAX_COMPONENT_DEPTH} deep")), "{err}");
    }

    /// A part that can't be read fails only the placements that need it, and none when only
    /// objects nobody places name it. The build sections of other parts are ignored, even ones
    /// pointing at a thumbnail or missing their object id (T6).
    #[test]
    fn broken_parts_and_the_builds_of_other_parts() {
        let root = root_model(
            &assembly(20, &[("/3D/Objects/unused.model", 1)]),
            r#"<item p:path="/3D/Objects/object_1.model" objectid="1"/><item p:path="/3D/Objects/Broken.model" objectid="1"/>"#,
        );
        let objects = format!(
            r#"<?xml version="1.0"?><model unit="millimeter" {PRODUCTION}><resources>{}</resources><build><item p:path="/Metadata/plate_1.png" objectid="1"/><item/></build></model>"#,
            tet_object(1, "A", 1.0)
        );
        let package = package_parts(&[
            ("3D/3dmodel.model", &root),
            (OBJECTS_PART, &objects),
            ("3D/Objects/broken.model", "not xml"),
            ("3D/Objects/unused.model", "<model><resources><object id="),
            ("Metadata/plate_1.png", "\u{89}PNG"),
        ]);
        let (objs, warnings) = read_3mf(&package).unwrap();
        assert_eq!(objs.len(), 1);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("build item 2") && warnings[0].contains("model part `/3D/Objects/Broken.model`: no <model> element"),
            "{warnings:?}"
        );
    }

    /// Part names are URIs: a `%XX` escape matches the character it stands for, either way round.
    #[test]
    fn escaped_part_names() {
        let root = root_model("", r#"<item p:path="/3D/Objects/my%20part.model" objectid="1"/><item p:path="/3D/Objects/café.model" objectid="1"/>"#);
        let tet = model_part("millimeter", &tet_object(1, "A", 1.0));
        let package = package_parts(&[("3D/3dmodel.model", &root), ("3D/Objects/my part.model", &tet), ("3D/Objects/caf%C3%A9.model", &tet)]);
        let (objs, warnings) = read_3mf(&package).unwrap();
        assert!(warnings.is_empty() && objs.len() == 2, "{warnings:?}");
    }

    #[test]
    fn package_is_well_formed() {
        let mut t = tetra("A <&> \"b\"");
        t.color = Some([1.0, 0.5, 0.0]);
        let bytes = write_3mf(&[t, tetra("B")]).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        for part in ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"] {
            let data = read_part(&mut zip, part).unwrap().unwrap();
            // Every part parses as XML to the end.
            let mut rd = Reader::from_reader(data.as_slice());
            let mut buf = Vec::new();
            let mut depth = 0i32;
            loop {
                match rd.read_event_into(&mut buf).unwrap() {
                    Event::Start(_) => depth += 1,
                    Event::End(_) => depth -= 1,
                    Event::Eof => break,
                    _ => {}
                }
                buf.clear();
            }
            assert_eq!(depth, 0, "{part}");
        }
        let model = String::from_utf8(read_part(&mut zip, "3D/3dmodel.model").unwrap().unwrap()).unwrap();
        assert!(model.contains("unit=\"millimeter\"") && model.contains(CORE_NS));
        assert!(model.contains("displaycolor=\"#FF8000\"") && model.contains("pid=\"1\" pindex=\"0\""));
        assert!(model.contains("name=\"A &lt;&amp;&gt; &quot;b&quot;\""));
        assert_eq!(model.matches("<item ").count(), 2);
        let (objs, w) = read_3mf(&bytes).unwrap();
        assert!(w.is_empty());
        assert_eq!(objs.len(), 2);
        assert_eq!(objs[0].name, "A <&> \"b\"");
        assert_eq!(objs[0].color, Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(objs[0].triangles, tetra("").triangles);
    }

    #[test]
    fn components_transforms_and_units() {
        let model = r#"<?xml version="1.0"?>
<model unit="centimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>
<object id="1" type="model" name="Tet"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/></vertices>
<triangles><triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/><triangle v1="1" v2="2" v3="3"/><triangle v1="0" v2="3" v3="2"/></triangles></mesh></object>
<object id="2" type="model" name="Pair"><components><component objectid="1"/><component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></components></object>
</resources><build><item objectid="2" transform="-1 0 0 0 1 0 0 0 1 0 0 2"/></build></model>"#;
        let (objs, _) = read_3mf(&package(model, None)).unwrap();
        assert_eq!(objs.len(), 1);
        let o = &objs[0];
        assert_eq!(o.name, "Pair");
        assert_eq!(o.triangles.len(), 8);
        // Mirrored by the item, in mm: x in [-60, 0], z in [20, 30]; winding flipped back.
        let xs: Vec<f64> = o.positions.iter().map(|p| p.x).collect();
        assert!(xs.iter().any(|x| (*x + 60.0).abs() < 1e-9) && xs.iter().all(|x| *x <= 1e-9));
        assert!(o.positions.iter().all(|p| p.z >= 20.0 - 1e-9 && p.z <= 30.0 + 1e-9));
        let v6: f64 = o.triangles.iter().map(|t| o.positions[t[0] as usize].dot(o.positions[t[1] as usize].cross(o.positions[t[2] as usize]))).sum();
        assert!((v6 / 6.0 - 2.0 * 1000.0 / 6.0).abs() < 1e-6, "{}", v6 / 6.0);
    }

    #[test]
    fn hostile_packages_are_errors() {
        let head = r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>"#;
        let cases = [
            format!(
                "{head}<object id=\"1\"><components><component objectid=\"1\"/></components></object></resources><build><item objectid=\"1\"/></build></model>"
            ),
            format!(
                "{head}<object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"7\" v3=\"9\"/></triangles></mesh></object></resources><build><item objectid=\"1\"/></build></model>"
            ),
            format!("{head}<object id=\"1\"><mesh><vertices><vertex x=\"NaN\" y=\"0\" z=\"0\"/></vertices></mesh></object></resources></model>"),
            format!("{head}</resources><build><item objectid=\"9\"/></build></model>"),
            format!("{head}<object id=\"1\"><components><component objectid=\"1\" transform=\"1 2\"/></components></object></resources></model>"),
            r#"<model unit="parsec"/>"#.to_string(),
            "<model><resources><object".to_string(),
            format!("{}{}", "<a>".repeat(1000), "</a>".repeat(1000)),
            "not xml at all".to_string(),
        ];
        for c in &cases {
            assert!(read_3mf(&package(c, None)).is_err(), "{c}");
        }
        assert!(read_3mf(b"").is_err());
        assert!(read_3mf(b"PK\x03\x04garbage").is_err());
        assert!(
            read_3mf(&package("<model/>", Some("<Relationships><Relationship Type=\"x/3dmodel\" Target=\"/missing.model\"/></Relationships>")))
                .is_err()
        );
    }

    /// Every placement needs its own transformed copy of the vertices, so an object with many
    /// vertices and one triangle used to cost its whole vertex array per build item: 100k
    /// vertices times 10,000 items asked for about 140 GB and the process aborted on the
    /// allocation, which no panic guard can intercept.
    #[test]
    fn many_placements_of_a_big_object_stay_bounded() {
        let verts: String = (0..100_000).map(|i| format!("<vertex x=\"{}\" y=\"{}\" z=\"0\"/>", i % 100, i / 100)).collect();
        let model = format!(
            "{}<object id=\"1\" type=\"model\"><mesh><vertices>{verts}</vertices>\
             <triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object></resources><build>{}</build></model>",
            r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>"#,
            "<item objectid=\"1\"/>".repeat(10_000)
        );
        let (objs, _) = read_3mf(&package(&model, None)).unwrap();
        assert_eq!(objs.len(), 10_000);
        // Only the three vertices the one triangle reaches are copied per placement.
        assert!(objs.iter().all(|o| o.positions.len() == 3 && o.triangles.len() == 1), "{:?}", objs.first().map(|o| o.positions.len()));
    }

    /// Objects that only place other objects count toward the budget too: six levels of 300
    /// components over an empty object (300^6 placements, and no triangles to count) stop at the
    /// budget instead of running for days.
    #[test]
    fn nested_fan_out_of_empty_objects_is_bounded() {
        let components = |child: u32| (0..300).map(|_| format!(r#"<component objectid="{child}"/>"#)).collect::<String>();
        let mut objects = r#"<object id="1" type="model"/>"#.to_string();
        for id in 2..=7 {
            objects += &format!(r#"<object id="{id}" type="model"><components>{}</components></object>"#, components(id - 1));
        }
        let model = format!(r#"<model unit="millimeter" {PRODUCTION}><resources>{objects}</resources><build><item objectid="7"/></build></model>"#);
        let err = read_3mf(&package(&model, None)).unwrap_err().to_string();
        assert!(err.contains("placements"), "{err}");
    }

    /// Triangles are budgeted across every placement, so fan-out cannot outrun the cap.
    #[test]
    fn placements_share_the_triangle_budget() {
        let verts: String = (0..30_000).map(|i| format!("<vertex x=\"{}\" y=\"0\" z=\"0\"/>", i % 100)).collect();
        let tris: String = (0..10_000).map(|i| format!("<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", 3 * i, 3 * i + 1, 3 * i + 2)).collect();
        let model = format!(
            "{}<object id=\"1\" type=\"model\"><mesh><vertices>{verts}</vertices><triangles>{tris}</triangles></mesh></object>\
             </resources><build>{}</build></model>",
            r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>"#,
            "<item objectid=\"1\"/>".repeat(4_000)
        );
        let (objs, warnings) = read_3mf(&package(&model, None)).unwrap();
        assert!(objs.len() < 4_000, "the shared budget must stop the later items: {} built", objs.len());
        assert!(warnings.iter().any(|w| w.contains("too many triangles")), "{warnings:?}");
    }

    /// A body far from the origin (a pattern spacing of "1e12 mm" gets there) welds without
    /// overflowing the grid's cell indices.
    #[test]
    fn weld_survives_extreme_coordinates() {
        let o = Vec3::new(1e12, 0.0, 0.0);
        let positions = [o, o + Vec3::new(0.1, 0.0, 0.0), o + Vec3::new(0.0, 0.1, 0.0), o + Vec3::new(0.0, 0.0, 0.1)];
        let triangles = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
        let (v, t) = weld(&positions, &triangles, 1e-7);
        assert_eq!((v.len(), t.len()), (4, 4));
    }
}
