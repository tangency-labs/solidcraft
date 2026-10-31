//! Independent OSD v2 mesh exchange. The empirically supported subset and its
//! provenance are documented in `docs/formats/osd.md`; no vendor runtime is used.

use std::collections::BTreeMap;

use solidcraft_geom::Vec3;

use crate::{IoError, MeshObject, Result};

const SIGNATURE: &[u8] = b" The CAM: OpenGL stream file (version 2.00)";
const MAX_BYTES: usize = 256 << 20;
const MAX_VERTICES: usize = 3_000_000;
// Expanded output has three vertices per triangle. Bound its memory and keep
// the writer's vertex count within the reader's limit.
const MAX_TRIANGLES: usize = MAX_VERTICES / 3;
const MAX_COLOURS: usize = 256;
const IDENTITY: [f64; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

/// How OSD triangles enter a design. Mesh is the original, unconverted path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OsdImportMode {
    #[default]
    Auto,
    Mesh,
    Solid,
}

// Sewing creates persistent CAD faces and STEP text, substantially larger than a
// display mesh. Refuse before allocating those descriptions, including in Auto.
const MAX_SOLID_TRIANGLES: usize = 20_000;

pub(crate) fn solid_feature(objects: &[MeshObject], file: &str) -> Result<crate::MeshFeature> {
    use solidcraft_kernel::{ExportBody, StepHeader};
    let count = objects.iter().try_fold(0usize, |n, o| n.checked_add(o.triangles.len())).ok_or_else(|| invalid("triangle count overflow"))?;
    if count > MAX_SOLID_TRIANGLES {
        return Err(invalid(format!("solid reconstruction is limited to {MAX_SOLID_TRIANGLES} triangles; this file has {count}")));
    }
    let mut bodies = Vec::new();
    let mut warnings = Vec::new();
    // A cavity can have a different colour from its outside. Never convert
    // nested/overlapping groups separately: mesh_body normalises their winding
    // and would turn an inward cavity shell into a second positive solid.
    let bounds: Vec<_> = objects.iter().map(|o| o.positions.iter().copied().collect::<solidcraft_geom::Aabb3>()).collect();
    let disjoint = bounds.iter().enumerate().all(|(i, a)| {
        bounds
            .iter()
            .skip(i + 1)
            .all(|b| a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z)
    });
    let separate = if disjoint { objects.iter().map(solid_object).collect::<Result<Vec<_>>>() } else { Err(invalid("overlapping colour groups")) };
    match separate {
        Ok(parts) => {
            for (object, body) in objects.iter().zip(parts) {
                bodies.push((object.name.clone(), body.with_color(object.color)));
            }
        }
        Err(e) if objects.len() <= 1 => return Err(e),
        Err(_) => {
            let mut merged = MeshObject::default();
            for o in objects {
                let offset = u32::try_from(merged.positions.len()).map_err(|_| invalid("too many vertices"))?;
                merged.positions.extend_from_slice(&o.positions);
                for t in &o.triangles {
                    let mut triangle = [0; 3];
                    for (out, index) in triangle.iter_mut().zip(t) {
                        if *index as usize >= o.positions.len() {
                            return Err(invalid("triangle index out of range"));
                        }
                        *out = offset.checked_add(*index).ok_or_else(|| invalid("vertex index overflow"))?;
                    }
                    merged.triangles.push(triangle);
                }
            }
            let name = std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "OSD".into());
            bodies.push((name, solid_object(&merged)?));
            warnings.push("OSD colour groups were joined to close the solid; individual face colours were not retained. Import with osd_mode=mesh to retain them.".into());
        }
    }
    let exports: Vec<_> = bodies.iter().map(|(name, body)| ExportBody { name: name.clone(), body, color: body.color() }).collect();
    let stem = std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "OSD".into());
    let text = solidcraft_kernel::step_export_bodies(&exports, &StepHeader { file_name: format!("{stem}.step"), ..Default::default() })?;
    let mut feature = crate::step_import_feature_named(text.as_bytes(), file, crate::StepNames::Solid)?;
    // This path places the entire import in one owner component, like MeshImport.
    // No invented assembly hierarchy or new document schema is needed.
    if let solidcraft_doc::FeatureKind::Import { components, .. } = &mut feature.kind {
        components.clear();
    }
    warnings.append(&mut feature.warnings);
    warnings.push(
        "OSD reconstructed as editable solid geometry; curved surfaces may remain faceted. Original sketches and feature history are not available."
            .into(),
    );
    Ok(crate::MeshFeature { name: feature.name, kind: feature.kind, body_names: feature.body_names, warnings })
}

fn solid_object(object: &MeshObject) -> Result<solidcraft_kernel::Body> {
    use solidcraft_kernel::{EdgeSpec, FaceSpec, SurfSpec};
    let mesh = solidcraft_kernel::mesh_body(&object.positions, &object.triangles)?;
    if !mesh.is_closed_mesh() {
        return Err(invalid("the triangles do not form a closed manifold shell"));
    }
    // Prefer the existing certified prism reconstruction: it removes internal
    // triangulation edges and can recover analytic cylindrical walls.
    if let Ok(body) = solidcraft_kernel::convert_prismatic_mesh(&mesh, 0.001) {
        return Ok(body);
    }
    let (positions, triangles) = mesh.triangle_mesh().ok_or_else(|| invalid("missing triangle geometry"))?;
    let mut faces = Vec::with_capacity(triangles.len());
    for t in triangles {
        let [a, b, c] = t.map(|i| positions.get(i as usize).copied());
        let (Some(a), Some(b), Some(c)) = (a, b, c) else { return Err(invalid("triangle index out of range")) };
        faces.push(FaceSpec {
            loops: vec![vec![EdgeSpec::Line { a, b }, EdgeSpec::Line { a: b, b: c }, EdgeSpec::Line { a: c, b: a }]],
            surface: SurfSpec::Plane,
            probe: None,
        });
    }
    let body = solidcraft_kernel::sew(&faces)?;
    let issues = body.validity();
    if !issues.is_empty() {
        return Err(invalid(format!("invalid reconstructed solid: {}", issues.join("; "))));
    }
    let before = solidcraft_kernel::measure(&mesh)?.volume;
    let after = solidcraft_kernel::measure(&body)?.volume;
    if !before.is_finite() || before <= 0. || (after - before).abs() > (before.abs() * 1e-7).max(1e-6) {
        return Err(invalid("sewing changed the enclosed volume"));
    }
    Ok(body)
}

fn invalid(message: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("OSD: {message}"))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self.offset.checked_add(N).ok_or_else(|| invalid("offset overflow"))?;
        let bytes = self.bytes.get(self.offset..end).ok_or_else(|| invalid(format!("truncated record at byte {}", self.offset)))?;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        self.offset = end;
        Ok(out)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }

    fn float(&mut self) -> Result<f64> {
        let v = f32::from_le_bytes(self.take()?) as f64;
        if !v.is_finite() {
            return Err(invalid("non-finite float"));
        }
        Ok(v)
    }

    fn double(&mut self) -> Result<f64> {
        let v = f64::from_le_bytes(self.take()?);
        if !v.is_finite() {
            return Err(invalid("non-finite double"));
        }
        Ok(v)
    }

    fn point(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.float()?, self.float()?, self.float()?))
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    p: Vec3,
    colour: [f32; 3],
}

/// Read OSD version 2 as triangle meshes, grouped by flat colour. Coordinates are
/// assumed to be millimetres. Display lines and reference/snap records are skipped
/// with a warning; unsupported records or primitives fail the entire import.
pub fn read_osd(bytes: &[u8], name: &str) -> Result<(Vec<MeshObject>, Vec<String>)> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("file exceeds 256 MiB"));
    }
    let mut r = Reader { bytes, offset: 0 };
    let header = r.take::<101>()?;
    if header[0] as usize != SIGNATURE.len() || header.get(1..1 + SIGNATURE.len()) != Some(SIGNATURE) {
        return Err(invalid("not an OSD version 2 stream"));
    }
    if u32::from_le_bytes(r.take()?) != 2 {
        return Err(invalid("unsupported version (only version 2 is supported)"));
    }
    let mut matrix = IDENTITY;
    let mut reflected = false;
    let mut solid = true;
    let mut colour = [0.6; 3];
    let mut primitive = None;
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut count = 0usize;
    let mut triangles = 0usize;
    let mut objects: BTreeMap<[u32; 3], MeshObject> = BTreeMap::new();
    let mut skipped_reference = false;
    let mut skipped_lines = false;
    let mut skipped_degenerate = false;
    let mut varying_colour = false;
    while r.offset < bytes.len() {
        let offset = r.offset;
        match r.byte()? {
            0 => {
                if primitive.is_some() {
                    return Err(invalid("nested primitive"));
                }
                let mode = u32::from_le_bytes(r.take()?);
                if mode > 6 {
                    return Err(invalid(format!("unsupported primitive {mode} at byte {offset}")));
                }
                primitive = Some(mode);
                vertices.clear();
                skipped_lines |= mode < 4 || !solid;
            }
            1 => {
                let mode = primitive.take().ok_or_else(|| invalid("end without begin"))?;
                if !solid || mode < 4 {
                    continue;
                }
                if vertices.len() < 3 || (mode == 4 && !vertices.len().is_multiple_of(3)) {
                    return Err(invalid("incomplete triangle primitive"));
                }
                let n = if mode == 4 { vertices.len() / 3 } else { vertices.len() - 2 };
                for i in 0..n {
                    let indices = match mode {
                        4 => [3 * i, 3 * i + 1, 3 * i + 2],
                        5 if i % 2 == 1 => [i + 1, i, i + 2],
                        5 => [i, i + 1, i + 2],
                        _ => [0, i + 1, i + 2],
                    };
                    let [a, b, c] = indices.map(|j| vertices.get(j));
                    let (Some(a), Some(b), Some(c)) = (a, b, c) else { return Err(invalid("invalid primitive indices")) };
                    let normal = (b.p - a.p).cross(c.p - a.p);
                    let area = normal.dot(normal);
                    if !area.is_finite() {
                        return Err(invalid("triangle exceeds numeric range"));
                    }
                    if area == 0.0 {
                        skipped_degenerate = true;
                        continue;
                    }
                    triangles += 1;
                    if triangles > MAX_TRIANGLES {
                        return Err(invalid("too many triangles"));
                    }
                    varying_colour |= a.colour != c.colour || b.colour != c.colour;
                    // OpenGL's flat-shaded triangle colour comes from its last vertex.
                    let key = c.colour.map(f32::to_bits);
                    if !objects.contains_key(&key) && objects.len() >= MAX_COLOURS {
                        return Err(invalid("more than 256 flat colours"));
                    }
                    let o = objects.entry(key).or_insert_with(|| MeshObject { name: name.into(), color: Some(c.colour), ..Default::default() });
                    let start = u32::try_from(o.positions.len()).map_err(|_| invalid("too many output vertices"))?;
                    o.positions.extend(if reflected { [a.p, c.p, b.p] } else { [a.p, b.p, c.p] });
                    o.triangles.push([start, start + 1, start + 2]);
                }
            }
            2 => {
                r.point()?;
            }
            3 => {
                let mode = primitive.ok_or_else(|| invalid("vertex outside a primitive"))?;
                let p = r.point()?;
                count += 1;
                if count > MAX_VERTICES {
                    return Err(invalid("too many input vertices"));
                }
                let p = Vec3::new(
                    p.x * matrix[0] + p.y * matrix[4] + p.z * matrix[8] + matrix[12],
                    p.x * matrix[1] + p.y * matrix[5] + p.z * matrix[9] + matrix[13],
                    p.x * matrix[2] + p.y * matrix[6] + p.z * matrix[10] + matrix[14],
                );
                if ![p.x, p.y, p.z].iter().all(|x| x.is_finite()) {
                    return Err(invalid("transformed vertex exceeds numeric range"));
                }
                if solid && mode >= 4 {
                    vertices.push(Vertex { p, colour });
                }
            }
            4 => {
                let p = r.point()?;
                colour = [p.x as f32, p.y as f32, p.z as f32];
                if !colour.iter().all(|v| (0.0..=1.0).contains(v)) {
                    return Err(invalid("colour is outside 0..1"));
                }
                colour = colour.map(|v| if v == 0.0 { 0.0 } else { v });
            }
            5 => {
                if primitive.is_some() {
                    return Err(invalid("pass changed inside a primitive"));
                }
                solid = match r.byte()? {
                    0 => false,
                    1 => true,
                    _ => return Err(invalid("invalid display pass")),
                };
            }
            6 => {
                for _ in 0..6 {
                    r.double()?;
                }
                r.take::<16>()?; // Opaque metadata, not geometry.
            }
            7 => {
                if primitive.is_some() {
                    return Err(invalid("matrix changed inside a primitive"));
                }
                for value in &mut matrix {
                    *value = r.double()?;
                }
                if matrix[3] != 0.0 || matrix[7] != 0.0 || matrix[11] != 0.0 || matrix[15] != 1.0 {
                    return Err(invalid("projective matrices are not supported"));
                }
                let x = Vec3::new(matrix[0], matrix[1], matrix[2]);
                let y = Vec3::new(matrix[4], matrix[5], matrix[6]);
                let z = Vec3::new(matrix[8], matrix[9], matrix[10]);
                let determinant = x.dot(y.cross(z));
                if !determinant.is_finite() || determinant == 0.0 {
                    return Err(invalid("singular or excessive matrix"));
                }
                reflected = determinant < 0.0;
            }
            op @ 8..=10 => {
                let doubles = match op {
                    8 => 6,
                    9 => 9,
                    _ => 13,
                };
                for _ in 0..doubles {
                    r.double()?;
                }
                skipped_reference = true;
            }
            op => return Err(invalid(format!("unknown opcode {op} at byte {offset}"))),
        }
    }
    if primitive.is_some() {
        return Err(invalid("unterminated primitive"));
    }
    if objects.is_empty() {
        return Err(invalid("no solid triangles"));
    }
    let mut objects: Vec<_> = objects.into_values().collect();
    if objects.len() > 1 {
        for (i, o) in objects.iter_mut().enumerate() {
            o.name = format!("{name} ({})", i + 1);
        }
    }
    let mut warnings = Vec::new();
    if skipped_lines {
        warnings.push("OSD display lines and points were omitted; only solid triangles were imported.".into());
    }
    if skipped_reference {
        warnings.push("OSD reference/snap records were omitted; they are not CAD topology.".into());
    }
    if skipped_degenerate {
        warnings.push("OSD zero-area triangles were omitted.".into());
    }
    if varying_colour {
        warnings.push("OSD vertex colours were reduced to each triangle's last vertex colour.".into());
    }
    Ok((objects, warnings))
}

fn point32(p: Vec3) -> Result<[f32; 3]> {
    let p = [p.x as f32, p.y as f32, p.z as f32];
    if !p.iter().all(|v| v.is_finite()) {
        return Err(invalid("coordinate cannot be represented as a finite float32"));
    }
    Ok(p)
}

fn floats(out: &mut Vec<u8>, op: u8, values: [f32; 3]) {
    out.push(op);
    for v in values {
        out.extend(v.to_le_bytes());
    }
}

/// Write a canonical, lossy OSD v2 triangle stream. Body names, topology, alpha,
/// wire display and reference records are not represented. The opaque header
/// metadata is zeroed; vendor acceptance of this experimental writer is unverified.
pub fn write_osd(objects: &[MeshObject]) -> Result<Vec<u8>> {
    let count = objects.iter().try_fold(0usize, |n, o| n.checked_add(o.triangles.len())).ok_or_else(|| invalid("too many triangles"))?;
    if count == 0 {
        return Err(IoError::Empty);
    }
    // 65 bytes per triangle includes a colour record, normal and three vertices.
    let estimate = count.checked_mul(65).and_then(|n| n.checked_add(307)).ok_or_else(|| invalid("output too large"))?;
    if count > MAX_TRIANGLES || estimate > MAX_BYTES {
        return Err(invalid("output exceeds the OSD size limit"));
    }
    let mut out = Vec::with_capacity(estimate);
    out.push(SIGNATURE.len() as u8);
    out.extend(SIGNATURE);
    out.resize(101, 0);
    out.extend(2u32.to_le_bytes());
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    // Bounds include only referenced vertices, after float32 quantisation.
    for o in objects {
        for tri in &o.triangles {
            for index in tri {
                let p = point32(*o.positions.get(*index as usize).ok_or_else(|| invalid("triangle index out of range"))?)?;
                for i in 0..3 {
                    min[i] = min[i].min(p[i] as f64);
                    max[i] = max[i].max(p[i] as f64);
                }
            }
        }
    }
    out.push(6);
    for v in min.into_iter().chain(max) {
        out.extend(v.to_le_bytes());
    }
    out.extend([0; 16]);
    out.push(7);
    for v in IDENTITY {
        out.extend(v.to_le_bytes());
    }
    out.extend([5, 1, 0]);
    out.extend(4u32.to_le_bytes());
    let mut colours = std::collections::BTreeSet::new();
    for o in objects {
        for (i, tri) in o.triangles.iter().enumerate() {
            let colour = o
                .tri_looks
                .get(i)
                .and_then(|look| look.checked_sub(1))
                .and_then(|look| o.looks.get(look as usize))
                .map(|look| look.0)
                .or(o.color)
                .unwrap_or([0.6; 3]);
            if !colour.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
                return Err(invalid("invalid export colour"));
            }
            let colour = colour.map(|v| if v == 0.0 { 0.0 } else { v });
            colours.insert(colour.map(f32::to_bits));
            if colours.len() > MAX_COLOURS {
                return Err(invalid("more than 256 flat colours"));
            }
            let mut points = [[0.0; 3]; 3];
            for (p, index) in points.iter_mut().zip(tri) {
                *p = point32(*o.positions.get(*index as usize).ok_or_else(|| invalid("triangle index out of range"))?)?;
            }
            let [a, b, c] = points.map(|p| Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            let normal = (b - a).cross(c - a);
            let length = normal.dot(normal).sqrt();
            if !length.is_finite() || length == 0.0 {
                return Err(invalid("triangle collapses at float32 precision"));
            }
            floats(&mut out, 4, colour);
            floats(&mut out, 2, point32(normal * (1.0 / length))?);
            for p in points {
                floats(&mut out, 3, p);
            }
        }
    }
    out.push(1);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Authored analytic fixtures, not files exported by another CAD application.
    fn header() -> Vec<u8> {
        let mut b = vec![43];
        b.extend(b" The CAM: OpenGL stream file (version 2.00)");
        b.resize(101, 0);
        b.extend(2u32.to_le_bytes());
        b
    }

    fn primitive(b: &mut Vec<u8>, mode: u32, points: &[[f32; 3]]) {
        b.push(0);
        b.extend(mode.to_le_bytes());
        for p in points {
            floats(b, 3, *p);
        }
        b.push(1);
    }

    fn square(mode: u32) -> Vec<u8> {
        let mut b = header();
        let p = if mode == 5 {
            [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.], [2., 2., 0.]]
        } else {
            [[0., 0., 0.], [2., 0., 0.], [2., 2., 0.], [0., 2., 0.]]
        };
        primitive(&mut b, mode, &p);
        b
    }

    fn area(objects: &[MeshObject]) -> f64 {
        objects
            .iter()
            .flat_map(|o| {
                o.triangles.iter().map(|t| {
                    let [a, b, c] = t.map(|i| o.positions[i as usize]);
                    // Signed XY area detects strip winding mistakes too.
                    (b - a).cross(c - a).z * 0.5
                })
            })
            .sum()
    }

    #[test]
    fn strip_and_fan_have_the_same_area_and_winding() {
        for mode in [5, 6] {
            let (objects, warnings) = read_osd(&square(mode), "Square").unwrap();
            assert!(warnings.is_empty());
            assert_eq!(objects[0].triangles.len(), 2);
            assert_eq!(area(&objects), 4.0);
            assert_eq!(objects[0].name, "Square");
        }
    }

    #[test]
    fn affine_translation_and_reflection_are_applied_once() {
        let mut b = header();
        let mut m = IDENTITY;
        m[0] = -2.;
        m[5] = 3.;
        m[12] = 10.;
        m[13] = 20.;
        m[14] = 30.;
        b.push(7);
        for v in m {
            b.extend(v.to_le_bytes());
        }
        b.extend_from_slice(&square(6)[105..]);
        let (objects, _) = read_osd(&b, "Mirror").unwrap();
        assert_eq!(area(&objects), 24.);
        assert!(objects[0].positions.iter().all(|p| (6.0..=10.0).contains(&p.x) && (20.0..=26.0).contains(&p.y) && p.z == 30.));
        let bytes = write_osd(&objects).unwrap();
        let (again, _) = read_osd(&bytes, "Again").unwrap();
        assert_eq!(again[0].positions, objects[0].positions);
        assert_eq!(area(&again), 24.);
    }

    #[test]
    fn wire_and_reference_records_are_not_duplicated_as_faces() {
        let mut b = square(6);
        b.extend([5, 0]);
        primitive(&mut b, 3, &[[0.; 3], [1.; 3]]);
        for (op, size) in [(6, 64), (8, 48), (9, 72), (10, 104)] {
            b.push(op);
            b.extend(vec![0; size]);
        }
        let (objects, warnings) = read_osd(&b, "Square").unwrap();
        assert_eq!(objects[0].triangles.len(), 2);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn colours_survive_roundtrip() {
        let mut b = header();
        for colour in [[1., 0., 0.], [0., 0.5, 1.]] {
            floats(&mut b, 4, colour);
            b.extend_from_slice(&square(6)[105..]);
        }
        let (objects, _) = read_osd(&b, "Colours").unwrap();
        assert_eq!(objects.len(), 2);
        let (again, _) = read_osd(&write_osd(&objects).unwrap(), "Colours").unwrap();
        assert_eq!(again, objects);
    }

    #[test]
    fn malformed_streams_fail_without_returning_partial_geometry() {
        let good = square(6);
        for n in 0..good.len() {
            assert!(read_osd(&good[..n], "cut").is_err(), "cut at {n}");
        }
        for tail in [vec![255], vec![1], vec![3; 13], vec![0, 7, 0, 0, 0], vec![5, 2]] {
            let mut b = good.clone();
            b.extend(tail);
            assert!(read_osd(&b, "bad").is_err());
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut b = header();
            primitive(&mut b, 4, &[[0.; 3], [1., 0., 0.], [0., value, 0.]]);
            assert!(read_osd(&b, "bad").is_err());
        }
        let mut b = good.clone();
        b[101] = 3;
        assert!(read_osd(&b, "version").is_err());
        for slot in [0, 3, 15] {
            let mut b = header();
            let mut m = IDENTITY;
            m[slot] = if slot == 3 { 1. } else { 0. };
            b.push(7);
            for v in m {
                b.extend(v.to_le_bytes());
            }
            b.extend_from_slice(&good[105..]);
            assert!(read_osd(&b, "matrix").is_err());
        }
    }

    #[test]
    fn mutations_never_panic() {
        crate::mutate::survives("OSD", &crate::mutate::mutations(&square(6), false, 0x05d2), MAX_TRIANGLES, |b| {
            read_osd(b, "mutation").ok().map(|(objects, _)| objects.iter().map(|o| o.triangles.len()).sum())
        });
    }

    #[test]
    fn writer_refuses_more_colours_than_reader_supports() {
        let (mut objects, _) = read_osd(&square(6), "Colours").unwrap();
        let o = &mut objects[0];
        o.triangles = vec![o.triangles[0]; MAX_COLOURS + 1];
        o.looks = (0..=MAX_COLOURS).map(|i| ([i as f32 / MAX_COLOURS as f32, 0., 0.], 0.)).collect();
        o.tri_looks = (1..=MAX_COLOURS as u32 + 1).collect();
        assert!(write_osd(&objects).is_err());
        objects[0].triangles.pop();
        let bytes = write_osd(&objects).unwrap();
        assert_eq!(read_osd(&bytes, "Colours").unwrap().0.len(), MAX_COLOURS);
    }

    #[test]
    fn closed_cube_keeps_positive_volume_and_same_coloured_bodies_merge() {
        let positions = (0..8).map(|i| Vec3::new(f64::from(i & 1) * 10., f64::from((i >> 1) & 1) * 10., f64::from((i >> 2) & 1) * 10.)).collect();
        let triangles =
            vec![[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
        let cube = MeshObject { name: "Cube".into(), positions, triangles, ..Default::default() };
        let bytes = write_osd(std::slice::from_ref(&cube)).unwrap();
        let (objects, _) = read_osd(&bytes, "Cube").unwrap();
        let o = &objects[0];
        let volume: f64 = o
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| o.positions[i as usize]);
                a.dot(b.cross(c)) / 6.
            })
            .sum();
        assert!((volume - 1000.).abs() < 1e-8, "{volume}");
        let body = solidcraft_kernel::mesh_body(&o.positions, &o.triangles).unwrap();
        assert!(body.is_closed_mesh());
        assert!((solidcraft_kernel::measure(&body).unwrap().volume - 1000.).abs() < 1e-8);
        let mut shifted = cube.clone();
        for p in &mut shifted.positions {
            p.x += 20.;
        }
        let (merged, _) = read_osd(&write_osd(&[cube, shifted]).unwrap(), "Two").unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].triangles.len(), 24);
    }

    #[test]
    fn export_refuses_bad_indices_and_float32_collapse() {
        let (mut objects, _) = read_osd(&square(6), "Square").unwrap();
        objects[0].triangles[0][0] = u32::MAX;
        assert!(write_osd(&objects).is_err());
        let (mut objects, _) = read_osd(&square(6), "Square").unwrap();
        for p in &mut objects[0].positions {
            p.x += 1e20;
            p.y += 1e20;
        }
        assert!(write_osd(&objects).is_err());
        objects[0].positions[0].x = f64::INFINITY;
        assert!(write_osd(&objects).is_err());
        assert!(write_osd(&[]).is_err());
    }

    #[test]
    fn mesh_feature_roundtrip_uses_the_native_pipeline() {
        use solidcraft_doc::{Document, Model};
        assert!(crate::is_mesh_path("Square.OSD"));
        let f = crate::mesh_import_feature(&square(6), "Square.OSD").unwrap();
        let mut d = Document::new("OSD import");
        let id = d.add_feature(f.kind, Some(&f.name)).unwrap();
        let mut model = Model::new();
        model.evaluate(&d);
        assert!(model.result(id).unwrap().error.is_none());
        let state = model.state();
        assert_eq!(state.bodies.len(), 1);
        assert!(state.bodies[0].body.is_mesh());
        for format in [crate::Format::Osd, crate::Format::StlBinary, crate::Format::ThreeMf] {
            assert!(!crate::export(&state, &[], format, "Square").unwrap().is_empty());
        }
        let bytes = crate::export(&state, &[], crate::Format::Osd, "Square").unwrap();
        assert_eq!(area(&read_osd(&bytes, "Again").unwrap().0), 4.);
    }

    fn pyramid() -> MeshObject {
        MeshObject {
            name: "Pyramid".into(),
            positions: vec![Vec3::ZERO, Vec3::new(10., 0., 0.), Vec3::new(10., 10., 0.), Vec3::new(0., 10., 0.), Vec3::new(5., 5., 10.)],
            triangles: vec![[0, 2, 1], [0, 3, 2], [0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]],
            color: Some([0.2, 0.5, 0.8]),
            ..Default::default()
        }
    }

    #[test]
    fn non_prismatic_osd_opens_as_editable_solid_and_survives_cold_reopen() {
        use solidcraft_kernel::{BoolOp, boolean, box_solid, measure};
        let source = pyramid();
        let mesh = solidcraft_kernel::mesh_body(&source.positions, &source.triangles).unwrap();
        assert!(solidcraft_kernel::convert_prismatic_mesh(&mesh, 0.001).is_err(), "exercise the general faceted path");
        let bytes = write_osd(&[source]).unwrap();
        let f = crate::mesh_import_feature(&bytes, "Pyramid.OSD").unwrap();
        assert_eq!(f.name, "Pyramid");
        assert_eq!(f.body_names, ["Pyramid"]);
        assert!(matches!(f.kind, solidcraft_doc::FeatureKind::Import { .. }));
        let mut d = solidcraft_doc::Document::new("Pyramid");
        d.add_feature(f.kind, Some(&f.name)).unwrap();
        let cold = solidcraft_doc::Document::from_json(&d.to_json()).unwrap();
        let mut model = solidcraft_doc::Model::new();
        model.evaluate(&cold);
        let state = model.state();
        let body = &state.bodies[0].body;
        assert!(!body.is_mesh());
        assert!(body.validity().is_empty());
        assert!((measure(body).unwrap().volume - 1000. / 3.).abs() < 1e-6);
        // Remove the top half-height pyramid (base 5×5, height 5).
        let tool = box_solid(Vec3::new(-1., -1., 5.), Vec3::new(11., 11., 11.)).unwrap();
        let edited = boolean(body, &tool, BoolOp::Cut).unwrap().unwrap();
        assert!((measure(&edited).unwrap().volume - 875. / 3.).abs() < 1e-5);
        let step = crate::export(&state, &[], crate::Format::Step, "Pyramid").unwrap();
        let reopened = crate::step_import_feature(&step, "Pyramid.step").unwrap();
        assert_eq!(reopened.import.bodies.len(), 1);
        assert!((measure(&reopened.import.bodies[0].body).unwrap().volume - 1000. / 3.).abs() < 1e-6);
    }

    #[test]
    fn osd_mesh_mode_and_open_shell_fallback_are_explicit() {
        let bytes = write_osd(&[pyramid()]).unwrap();
        let f = crate::mesh_import_feature_with_osd_mode(&bytes, "Pyramid.osd", OsdImportMode::Mesh).unwrap();
        assert!(matches!(f.kind, solidcraft_doc::FeatureKind::MeshImport { .. }));
        let open = square(6);
        let f = crate::mesh_import_feature(&open, "Open.osd").unwrap();
        assert!(matches!(f.kind, solidcraft_doc::FeatureKind::MeshImport { .. }));
        assert!(f.warnings.iter().any(|w| w.contains("not an editable solid")));
        assert!(crate::mesh_import_feature_with_osd_mode(&open, "Open.osd", OsdImportMode::Solid).is_err());
        let mut broken = pyramid();
        broken.triangles.push(broken.triangles[0]);
        assert!(solid_feature(&[broken], "Nonmanifold.osd").is_err());
        let mut large = pyramid();
        large.triangles = vec![large.triangles[0]; MAX_SOLID_TRIANGLES + 1];
        assert!(solid_feature(&[large], "Large.osd").err().unwrap().to_string().contains("limited"));
    }

    #[test]
    fn colour_seams_do_not_leave_a_closed_osd_as_open_meshes() {
        let mut first = pyramid();
        let mut second = first.clone();
        first.triangles.truncate(2);
        second.triangles.drain(..2);
        second.color = Some([1., 0., 0.]);
        let bytes = write_osd(&[first, second]).unwrap();
        let f = crate::mesh_import_feature_with_osd_mode(&bytes, "Colours.osd", OsdImportMode::Solid).unwrap();
        assert!(matches!(f.kind, solidcraft_doc::FeatureKind::Import { .. }));
        assert!(f.warnings.iter().any(|w| w.contains("face colours were not retained")));
    }

    #[test]
    fn differently_coloured_cavity_keeps_its_empty_volume() {
        let box_mesh = |low, high, color| {
            let body = solidcraft_kernel::box_solid(low, high).unwrap();
            let mesh = body.tessellate(0.001).unwrap();
            MeshObject { name: "Box".into(), positions: mesh.positions, triangles: mesh.triangles, color: Some(color), ..Default::default() }
        };
        let outer = box_mesh(Vec3::ZERO, Vec3::new(10., 10., 10.), [0., 1., 0.]);
        for colour in [[0., 1., 0.], [1., 0., 0.]] {
            let mut inner = box_mesh(Vec3::new(2., 2., 2.), Vec3::new(8., 8., 8.), colour);
            for t in &mut inner.triangles {
                t.swap(1, 2);
            }
            let bytes = write_osd(&[outer.clone(), inner]).unwrap();
            let feature = crate::mesh_import_feature_with_osd_mode(&bytes, "Hollow.osd", OsdImportMode::Solid).unwrap();
            let mut doc = solidcraft_doc::Document::new("Hollow");
            doc.add_feature(feature.kind, Some(&feature.name)).unwrap();
            let mut model = solidcraft_doc::Model::new();
            model.evaluate(&doc);
            let state = model.state();
            let volume: f64 = state.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum();
            assert!(state.bodies.iter().all(|b| !b.body.is_mesh()));
            assert!((volume - 784.).abs() < 1e-6, "outer minus cavity, not outer plus cavity: {volume}");
        }
        let other = box_mesh(Vec3::new(20., 0., 0.), Vec3::new(30., 10., 10.), [1., 0., 0.]);
        let bytes = write_osd(&[outer, other]).unwrap();
        let feature = crate::mesh_import_feature_with_osd_mode(&bytes, "Two.osd", OsdImportMode::Solid).unwrap();
        assert_eq!(feature.body_names.len(), 2, "separate colour groups remain separate bodies");
    }
}
