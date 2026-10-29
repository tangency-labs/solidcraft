use super::dna::{File, View, text};
use super::{Import, Object, Result, bad};
use solidcraft_geom::Vec3;
use solidcraft_geom::subdivision::{Cage, EdgeCrease, MAX_FACES, MAX_POINTS, VertexCrease};
use std::collections::{BTreeMap, BTreeSet};
mod mirror;

#[derive(Clone, Copy)]
struct Attribute<'a> {
    bytes: &'a [u8],
    single: bool,
}
impl<'a> Attribute<'a> {
    fn element(self, index: usize, width: usize) -> Result<&'a [u8]> {
        let start = if self.single { 0 } else { index.checked_mul(width).ok_or_else(|| bad("attribute offset overflow"))? };
        self.bytes.get(start..start + width).ok_or_else(|| bad("incomplete mesh attribute"))
    }
}

fn count(view: View<'_, '_>, field: &str, max: usize) -> Result<usize> {
    let n = usize::try_from(view.uint(field, 4)?).map_err(|_| bad("invalid mesh count"))?;
    if n == 0 || n > max {
        return Err(bad(format!("{field} is empty or exceeds the editable cage budget")));
    }
    Ok(n)
}

fn attributes<'a>(mesh: View<'_, 'a>) -> Result<BTreeMap<String, Attribute<'a>>> {
    let f = mesh.file;
    let mut out = BTreeMap::new();
    if mesh.has("attribute_storage") {
        let storage = mesh.nested("attribute_storage")?;
        let n = storage.uint("dna_attributes_num", 4)? as usize;
        if n > 4096 {
            return Err(bad("too many mesh attributes"));
        }
        if n != 0 {
            let bytes = f.data(storage.ptr("dna_attributes")?)?;
            let stride = f.structure("Attribute")?.size;
            if stride == 0 {
                return Err(bad("empty attribute layout"));
            }
            for i in 0..n {
                let a = f.view(bytes.get(i * stride..).ok_or_else(|| bad("truncated attributes"))?, "Attribute")?;
                let name = text(f.data(a.ptr("name")?)?)?;
                if !matches!(name.as_str(), "position" | ".corner_vert" | ".edge_verts" | "crease_edge" | "crease_vert") {
                    continue;
                }
                let expected = match name.as_str() {
                    "position" => (7, 0),
                    ".corner_vert" => (3, 3),
                    ".edge_verts" => (4, 1),
                    "crease_edge" => (5, 1),
                    _ => (5, 0),
                };
                if a.uint("data_type", 2)? != expected.0 || a.uint("domain", 1)? != expected.1 {
                    return Err(bad("unexpected mesh attribute type or domain"));
                }
                let storage_type = a.uint("storage_type", 1)?;
                let wrapper = match storage_type {
                    0 => "AttributeArray",
                    1 => "AttributeSingle",
                    _ => return Err(bad("unsupported attribute storage")),
                };
                let data = f.view(f.data(a.ptr("data")?)?, wrapper)?;
                let single = storage_type == 1 || data.has("is_single") && data.uint("is_single", 1)? != 0;
                if !single && data.uint("size", 8)? > 1_000_000 {
                    return Err(bad("attribute count exceeds budget"));
                }
                if out.insert(name, Attribute { bytes: f.data(data.ptr("data")?)?, single }).is_some() {
                    return Err(bad("duplicate mesh attribute"));
                }
            }
        }
    }
    for domain in ["vdata", "edata", "ldata", "pdata"] {
        if !mesh.has(domain) {
            continue;
        }
        let storage = mesh.nested(domain)?;
        let n = storage.uint("totlayer", 4)? as usize;
        if n > 4096 {
            return Err(bad("too many custom-data layers"));
        }
        if n == 0 {
            continue;
        }
        let bytes = f.data(storage.ptr("layers")?)?;
        let stride = f.structure("CustomDataLayer")?.size;
        if stride == 0 {
            return Err(bad("empty custom-data layout"));
        }
        for i in 0..n {
            let a = f.view(bytes.get(i * stride..).ok_or_else(|| bad("truncated custom data"))?, "CustomDataLayer")?;
            let name = a.string("name")?;
            if matches!(name.as_str(), "position" | ".corner_vert" | ".edge_verts" | "crease_edge" | "crease_vert") && !out.contains_key(&name) {
                out.insert(name, Attribute { bytes: f.data(a.ptr("data")?)?, single: false });
            }
        }
    }
    Ok(out)
}

fn legacy_array<'f, 'a>(mesh: View<'f, 'a>, field: &str, layout: &str, n: usize) -> Result<Vec<View<'f, 'a>>> {
    let bytes = mesh.file.data(mesh.ptr(field)?)?;
    let stride = mesh.file.structure(layout)?.size;
    if stride == 0 {
        return Err(bad("empty mesh element layout"));
    }
    (0..n)
        .map(|i| {
            let start = i.checked_mul(stride).ok_or_else(|| bad("mesh array offset overflow"))?;
            mesh.file.view(bytes.get(start..).ok_or_else(|| bad("truncated mesh array"))?, layout)
        })
        .collect()
}

fn cage(mesh: View<'_, '_>) -> Result<Cage> {
    let f = mesh.file;
    let vertices = count(mesh, "totvert", MAX_POINTS)?;
    let faces = count(mesh, "totpoly", MAX_FACES)?;
    let corners = count(mesh, "totloop", MAX_FACES * 4)?;
    let edges = count(mesh, "totedge", MAX_FACES * 4)?;
    if corners != faces * 4 {
        return Err(bad("editable Forms require an all-quad control cage"));
    }
    let attrs = attributes(mesh)?;
    let mut out = Cage::default();
    if let Some(&attr) = attrs.get("position") {
        for i in 0..vertices {
            let b = attr.element(i, 12)?;
            out.points.push(Vec3::new(f.numbers.float(b, 0)?, f.numbers.float(b, 4)?, f.numbers.float(b, 8)?));
        }
    } else {
        for v in legacy_array(mesh, "mvert", "MVert", vertices)? {
            let p = v.floats("co", 3)?;
            out.points.push(Vec3::new(p[0], p[1], p[2]));
        }
    }
    let corner_vertices: Vec<usize> = if let Some(&attr) = attrs.get(".corner_vert") {
        (0..corners).map(|i| Ok(f.numbers.uint(attr.element(i, 4)?, 0, 4)? as usize)).collect::<Result<_>>()?
    } else {
        legacy_array(mesh, "mloop", "MLoop", corners)?.into_iter().map(|v| Ok(v.uint("v", 4)? as usize)).collect::<Result<_>>()?
    };
    if mesh.has("poly_offset_indices") && mesh.ptr("poly_offset_indices")? != 0 {
        let offsets = f.data(mesh.ptr("poly_offset_indices")?)?;
        for i in 0..=faces {
            if f.numbers.uint(offsets, i * 4, 4)? as usize != i * 4 {
                return Err(bad("editable Forms require contiguous quad faces"));
            }
        }
        out.faces = corner_vertices.as_chunks::<4>().0.iter().map(|v| v.to_vec()).collect();
    } else {
        for p in legacy_array(mesh, "mpoly", "MPoly", faces)? {
            let start = p.uint("loopstart", 4)? as usize;
            if p.uint("totloop", 4)? != 4 {
                return Err(bad("editable Forms require quad faces"));
            }
            let end = start.checked_add(4).ok_or_else(|| bad("face offset overflow"))?;
            out.faces.push(corner_vertices.get(start..end).ok_or_else(|| bad("invalid face corners"))?.to_vec());
        }
    }
    let sharpness = |b: &[u8]| -> Result<f64> {
        let value = f.numbers.float(b, 0)?;
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(bad("crease must be finite and in 0..1"));
        }
        Ok(value * value * 10.0)
    };
    if let Some(&attr) = attrs.get("crease_vert") {
        for i in 0..vertices {
            let sharpness = sharpness(attr.element(i, 4)?)?;
            if sharpness > 0.0 {
                out.vertex_creases.push(VertexCrease { vertex: i, sharpness });
            }
        }
    }
    if let Some(&attr) = attrs.get("crease_edge") {
        let pairs: Vec<[usize; 2]> = if let Some(&a) = attrs.get(".edge_verts") {
            (0..edges)
                .map(|i| {
                    let b = a.element(i, 8)?;
                    Ok([f.numbers.uint(b, 0, 4)? as usize, f.numbers.uint(b, 4, 4)? as usize])
                })
                .collect::<Result<_>>()?
        } else {
            legacy_array(mesh, "medge", "MEdge", edges)?
                .into_iter()
                .map(|v| Ok([v.uint("v1", 4)? as usize, v.uint("v2", 4)? as usize]))
                .collect::<Result<_>>()?
        };
        for (i, edge) in pairs.into_iter().enumerate() {
            let sharpness = sharpness(attr.element(i, 4)?)?;
            if sharpness > 0.0 {
                out.edge_creases.push(EdgeCrease { edge, sharpness });
            }
        }
    }
    out.validate().map_err(|e| bad(e.to_string()))?;
    Ok(out)
}

type Matrix = [[f64; 4]; 4];
fn identity() -> Matrix {
    [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]
}
fn mul(a: Matrix, b: Matrix) -> Matrix {
    let mut out = [[0.; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}
fn rotation(axis: usize, angle: f64) -> Matrix {
    let mut m = identity();
    let (s, c) = angle.sin_cos();
    let a = (axis + 1) % 3;
    let b = (axis + 2) % 3;
    m[a][a] = c;
    m[a][b] = -s;
    m[b][a] = s;
    m[b][b] = c;
    m
}
fn local(object: View<'_, '_>) -> Result<Matrix> {
    let loc = object.floats("loc", 3)?;
    let delta = object.floats("dloc", 3)?;
    let scale = object.floats("size", 3)?;
    let mode = object.uint("rotmode", 2)? as i16;
    let mut r = identity();
    if mode == 0 {
        let q = object.floats("quat", 4)?;
        let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !norm.is_finite() || norm < 1e-12 {
            return Err(bad("invalid object quaternion"));
        }
        let (w, x, y, z) = (q[0] / norm, q[1] / norm, q[2] / norm, q[3] / norm);
        r[0][0] = 1. - 2. * (y * y + z * z);
        r[0][1] = 2. * (x * y - z * w);
        r[0][2] = 2. * (x * z + y * w);
        r[1][0] = 2. * (x * y + z * w);
        r[1][1] = 1. - 2. * (x * x + z * z);
        r[1][2] = 2. * (y * z - x * w);
        r[2][0] = 2. * (x * z - y * w);
        r[2][1] = 2. * (y * z + x * w);
        r[2][2] = 1. - 2. * (x * x + y * y);
    } else if (1..=6).contains(&mode) {
        let angles = object.floats("rot", 3)?;
        let order = match mode {
            1 => [0, 1, 2],
            2 => [0, 2, 1],
            3 => [1, 0, 2],
            4 => [1, 2, 0],
            5 => [2, 0, 1],
            _ => [2, 1, 0],
        };
        for axis in order {
            r = mul(rotation(axis, angles[axis]), r);
        }
    } else {
        return Err(bad("axis-angle object transforms are not supported yet"));
    }
    // Delta channels are independently stored; reject unhandled rotations rather
    // than returning a plausible but misplaced cage.
    if object.floats("drot", 3)?.iter().any(|v| v.abs() > 1e-12)
        || object.floats("dquat", 4)?.iter().enumerate().any(|(i, v)| (*v - if i == 0 { 1. } else { 0. }).abs() > 1e-12)
    {
        return Err(bad("delta object rotation is unsupported"));
    }
    let ds = object.floats("dscale", 3)?;
    for i in 0..3 {
        for row in r.iter_mut().take(3) {
            row[i] *= scale[i] * ds[i];
        }
        r[i][3] = loc[i] + delta[i];
    }
    if r.iter().flatten().any(|v| !v.is_finite()) {
        return Err(bad("non-finite object transform"));
    }
    Ok(r)
}

fn world(f: &File<'_>, object: View<'_, '_>, seen: &mut BTreeSet<u64>, depth: usize) -> Result<Matrix> {
    if depth > 64 {
        return Err(bad("object parent depth exceeds 64"));
    }
    if object.nested("constraints")?.ptr("first")? != 0 {
        return Err(bad("object constraints require evaluated geometry"));
    }
    let local = local(object)?;
    let parent = object.ptr("parent")?;
    if parent == 0 {
        return Ok(local);
    }
    if object.uint("partype", 2)? & 15 != 0 {
        return Err(bad("bone and vertex parenting require evaluated geometry"));
    }
    if !seen.insert(parent) {
        return Err(bad("cyclic object parents"));
    }
    let parent_object = f.view(f.data(parent)?, "Object")?;
    let p = world(f, parent_object, seen, depth + 1)?;
    let raw = object.floats("parentinv", 16)?;
    let mut inverse = identity();
    for (i, row) in inverse.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = raw[j * 4 + i];
        }
    }
    seen.remove(&parent);
    Ok(mul(mul(p, inverse), local))
}

fn modifiers(object: View<'_, '_>, mut cage: Cage) -> Result<(Cage, usize)> {
    let f = object.file;
    let mut next = object.nested("modifiers")?.ptr("first")?;
    let mut seen = BTreeSet::new();
    let mut level = None;
    while next != 0 {
        if seen.len() >= 128 || !seen.insert(next) {
            return Err(bad("invalid modifier chain"));
        }
        let block = f.block(next)?;
        let ty = f.structs.get(block.sdna).ok_or_else(|| bad("invalid modifier layout"))?;
        let base = f.view(block.bytes, "ModifierData")?;
        if base.uint("mode", 4)? & 1 != 0 {
            if ty.name == "MirrorModifierData" && level.is_none() {
                cage = mirror::apply(object, f.view(block.bytes, "MirrorModifierData")?, cage)?;
                next = base.ptr("next")?;
                continue;
            }
            if ty.name != "SubsurfModifierData" || level.is_some() {
                return Err(bad(format!("unsupported enabled modifier {}", base.string("name")?)));
            }
            let s = f.view(block.bytes, "SubsurfModifierData")?;
            if s.uint("subdivType", 2)? != 0 {
                return Err(bad("subdivision must be Catmull-Clark"));
            }
            level = Some((s.uint("levels", 2)? as usize).min(4));
        }
        next = base.ptr("next")?;
    }
    Ok((cage, level.unwrap_or(2)))
}

pub(super) fn read(f: &File<'_>) -> Result<Import> {
    let mut out = Import::default();
    let mut total_points = 0usize;
    let mut total_faces = 0usize;
    let mut unit = 1000.0;
    if let Some(scene) = f.blocks.iter().find(|b| b.code == "SC") {
        let s = f.view(scene.bytes, "Scene")?.nested("unit")?;
        let value = f.numbers.float(s.field("scale_length")?, 0)?;
        if !value.is_finite() || value <= 0.0 || value > 1e9 {
            return Err(bad("invalid scene units"));
        }
        unit = value * 1000.;
    }
    for block in f.blocks.iter().filter(|b| b.code == "OB") {
        if block.count != 1 {
            return Err(bad("object block must contain one object"));
        }
        if out.objects.len() + out.rejected.len() >= 1024 {
            return Err(bad("too many objects"));
        }
        let object = f.view(block.bytes, "Object")?;
        if object.uint("type", 2)? != 1 {
            continue;
        }
        let name = object.nested("id")?.string("name")?.strip_prefix("OB").unwrap_or("Form").to_owned();
        let result = (|| {
            if object.has("adt") && object.ptr("adt")? != 0 {
                return Err(bad("animated objects require evaluated geometry"));
            }
            let source_cage = cage(f.view(f.data(object.ptr("data")?)?, "Mesh")?)?;
            let (mut cage, levels) = modifiers(object, source_cage)?;
            let m = world(f, object, &mut BTreeSet::from([block.address]), 0)?;
            let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
            if !det.is_finite() || det.abs() < 1e-18 {
                return Err(bad("singular object transform"));
            }
            for p in &mut cage.points {
                let v = [p.x, p.y, p.z, 1.];
                let xyz: Vec<_> = m.iter().take(3).map(|row| row.iter().zip(v).map(|(a, b)| a * b).sum::<f64>() * unit).collect();
                *p = Vec3::new(xyz[0], xyz[1], xyz[2]);
            }
            if det < 0. {
                for face in &mut cage.faces {
                    face.reverse();
                }
            }
            cage.validate().map_err(|e| bad(e.to_string()))?;
            Ok(Object { name: name.clone(), cage, levels })
        })();
        match result {
            Ok(object) => {
                total_points += object.cage.points.len();
                total_faces += object.cage.faces.len();
                if total_points > 200_000 || total_faces > 150_000 {
                    return Err(bad("total imported control-cage budget exceeded"));
                }
                out.objects.push(object)
            }
            Err(e) => out.rejected.push(format!("{name}: {e}")),
        }
    }
    Ok(out)
}
