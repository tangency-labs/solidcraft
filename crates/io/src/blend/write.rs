//! Original, minimal SDNA writer. Only scene, collection, object and control-mesh
//! records are emitted; there are no startup templates, scripts or UI assets.
use super::{MAX_BLEND_BYTES, Object, Result, SurfaceObject, bad};
use solidcraft_geom::subdivision::Cage;
use std::collections::{BTreeMap, BTreeSet};

type Fields = Vec<(&'static str, &'static str)>;
struct Layout {
    name: &'static str,
    size: usize,
    fields: Fields,
    offsets: BTreeMap<&'static str, usize>,
}
struct Schema {
    layouts: Vec<Layout>,
    types: Vec<(&'static str, usize)>,
    names: Vec<&'static str>,
}
impl Schema {
    fn new() -> Result<Self> {
        let definitions: Vec<(&str, Fields)> = vec![
            ("Link", vec![("void", "*next"), ("void", "*prev")]),
            ("ListBase", vec![("void", "*first"), ("void", "*last")]),
            (
                "ID",
                vec![
                    ("void", "*next"),
                    ("void", "*prev"),
                    ("ID", "*newid"),
                    ("void", "*lib"),
                    ("char", "name[258]"),
                    ("short", "flag"),
                    ("int", "tag"),
                    ("int", "us"),
                    ("char", "_pad[4]"),
                ],
            ),
            ("Library", vec![("ID", "id"), ("char", "name[1024]")]),
            ("bScreen", vec![("ID", "id"), ("Scene", "*scene"), ("ListBase", "areabase")]),
            ("wmWindowManager", vec![("ID", "id"), ("ListBase", "windows")]),
            (
                "UnitSettings",
                vec![
                    ("float", "scale_length"),
                    ("char", "system"),
                    ("char", "system_rotation"),
                    ("short", "flag"),
                    ("char", "length_unit"),
                    ("char", "mass_unit"),
                    ("char", "time_unit"),
                    ("char", "temperature_unit"),
                    ("char", "_pad[4]"),
                ],
            ),
            ("AttributeArray", vec![("void", "*data"), ("void", "*sharing_info"), ("int64_t", "size"), ("char", "is_single"), ("char", "_pad[7]")]),
            (
                "Attribute",
                vec![("char", "*name"), ("short", "data_type"), ("char", "domain"), ("char", "storage_type"), ("char", "_pad[4]"), ("void", "*data")],
            ),
            ("AttributeStorage", vec![("Attribute", "*dna_attributes"), ("int", "dna_attributes_num"), ("char", "_pad[4]"), ("void", "*runtime")]),
            (
                "Mesh",
                vec![
                    ("ID", "id"),
                    ("int", "totvert"),
                    ("int", "totedge"),
                    ("int", "totpoly"),
                    ("int", "totloop"),
                    ("int", "*poly_offset_indices"),
                    ("AttributeStorage", "attribute_storage"),
                ],
            ),
            ("ModifierData", vec![("ModifierData", "*next"), ("ModifierData", "*prev"), ("int", "type"), ("int", "mode"), ("char", "name[64]")]),
            (
                "SubsurfModifierData",
                vec![
                    ("ModifierData", "modifier"),
                    ("short", "subdivType"),
                    ("short", "levels"),
                    ("short", "renderLevels"),
                    ("short", "flags"),
                    ("short", "quality"),
                    ("char", "_pad[6]"),
                ],
            ),
            (
                "Object",
                vec![
                    ("ID", "id"),
                    ("Object", "*parent"),
                    ("ID", "*data"),
                    ("short", "type"),
                    ("short", "partype"),
                    ("short", "rotmode"),
                    ("char", "_pad[2]"),
                    ("float", "loc[3]"),
                    ("float", "dloc[3]"),
                    ("float", "size[3]"),
                    ("float", "dscale[3]"),
                    ("float", "rot[3]"),
                    ("float", "drot[3]"),
                    ("float", "quat[4]"),
                    ("float", "dquat[4]"),
                    ("float", "parentinv[4][4]"),
                    ("ListBase", "modifiers"),
                    ("ListBase", "constraints"),
                ],
            ),
            ("CollectionObject", vec![("CollectionObject", "*next"), ("CollectionObject", "*prev"), ("Object", "*ob")]),
            (
                "Collection",
                vec![("ID", "id"), ("ID", "*owner_id"), ("ListBase", "gobject"), ("ListBase", "children"), ("char", "flag"), ("char", "_pad[7]")],
            ),
            (
                "ViewLayer",
                vec![
                    ("ViewLayer", "*next"),
                    ("ViewLayer", "*prev"),
                    ("char", "name[64]"),
                    ("short", "flag"),
                    ("char", "_pad[6]"),
                    ("ListBase", "object_bases"),
                    ("ListBase", "layer_collections"),
                ],
            ),
            (
                "ToolSettings",
                vec![("float", "vgroup_weight"), ("float", "doublimit"), ("char", "selectmode"), ("char", "_pad[3]"), ("int", "uvcalc_iterations")],
            ),
            (
                "Scene",
                vec![
                    ("ID", "id"),
                    ("UnitSettings", "unit"),
                    ("ListBase", "view_layers"),
                    ("Collection", "*master_collection"),
                    ("ToolSettings", "*toolsettings"),
                ],
            ),
            (
                "FileGlobal",
                vec![
                    ("char", "subvstr[4]"),
                    ("short", "subversion"),
                    ("short", "minversion"),
                    ("short", "minsubversion"),
                    ("char", "_pad[6]"),
                    ("void", "*curscreen"),
                    ("Scene", "*curscene"),
                    ("ViewLayer", "*cur_view_layer"),
                ],
            ),
        ];
        let mut types: Vec<(&'static str, usize)> = vec![("char", 1), ("short", 2), ("int", 4), ("float", 4), ("void", 0), ("int64_t", 8)];
        let mut layouts = Vec::new();
        let mut names = Vec::new();
        for (name, fields) in definitions {
            let mut size = 0usize;
            let mut offsets = BTreeMap::new();
            for &(ty, raw) in &fields {
                let mut len = if raw.contains('*') { 8 } else { types.iter().find(|t| t.0 == ty).ok_or_else(|| bad("writer type order"))?.1 };
                for dim in raw.split('[').skip(1) {
                    let n = dim.split(']').next().ok_or_else(|| bad("writer array"))?.parse::<usize>().map_err(|_| bad("writer array size"))?;
                    len = len.checked_mul(n).ok_or_else(|| bad("writer array overflow"))?;
                }
                offsets.insert(raw.trim_start_matches('*').split('[').next().ok_or_else(|| bad("writer field"))?, size);
                size += len;
                if !names.contains(&raw) {
                    names.push(raw);
                }
            }
            types.push((name, size));
            layouts.push(Layout { name, size, fields, offsets });
        }
        // Pointee types are present even when their actual layout is omitted.
        for layout in &layouts {
            for &(ty, _) in &layout.fields {
                if !types.iter().any(|t| t.0 == ty) {
                    types.push((ty, 0));
                }
            }
        }
        Ok(Self { layouts, types, names })
    }
    fn layout(&self, name: &str) -> Result<&Layout> {
        self.layouts.iter().find(|l| l.name == name).ok_or_else(|| bad("unknown writer layout"))
    }
    fn dna(&self) -> Result<Vec<u8>> {
        let mut b = b"SDNANAME".to_vec();
        b.extend((self.names.len() as u32).to_le_bytes());
        for n in &self.names {
            b.extend(n.as_bytes());
            b.push(0);
        }
        pad(&mut b);
        b.extend(b"TYPE");
        b.extend((self.types.len() as u32).to_le_bytes());
        for (n, _) in &self.types {
            b.extend(n.as_bytes());
            b.push(0);
        }
        pad(&mut b);
        b.extend(b"TLEN");
        for (_, n) in &self.types {
            b.extend((*n as u16).to_le_bytes());
        }
        pad(&mut b);
        b.extend(b"STRC");
        b.extend((self.layouts.len() as u32).to_le_bytes());
        for l in &self.layouts {
            let ti = self.types.iter().position(|t| t.0 == l.name).ok_or_else(|| bad("writer type missing"))?;
            b.extend((ti as u16).to_le_bytes());
            b.extend((l.fields.len() as u16).to_le_bytes());
            for &(ty, name) in &l.fields {
                let t = self.types.iter().position(|v| v.0 == ty).ok_or_else(|| bad("writer field type missing"))?;
                let n = self.names.iter().position(|&v| v == name).ok_or_else(|| bad("writer field name missing"))?;
                b.extend((t as u16).to_le_bytes());
                b.extend((n as u16).to_le_bytes());
            }
        }
        Ok(b)
    }
}
fn pad(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

struct Record<'a> {
    layout: &'a Layout,
    bytes: Vec<u8>,
}
impl<'a> Record<'a> {
    fn new(schema: &'a Schema, name: &str) -> Result<Self> {
        let layout = schema.layout(name)?;
        Ok(Self { layout, bytes: vec![0; layout.size] })
    }
    fn set(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let &offset = self.layout.offsets.get(name).ok_or_else(|| bad(format!("unknown writer field {}.{name}", self.layout.name)))?;
        self.bytes.get_mut(offset..offset + bytes.len()).ok_or_else(|| bad("writer field overflow"))?.copy_from_slice(bytes);
        Ok(())
    }
    fn ptr(&mut self, name: &str, n: u64) -> Result<()> {
        self.set(name, &n.to_le_bytes())
    }
    fn int(&mut self, name: &str, n: usize) -> Result<()> {
        self.set(name, &(n as u32).to_le_bytes())
    }
    fn short(&mut self, name: &str, n: u16) -> Result<()> {
        self.set(name, &n.to_le_bytes())
    }
    fn float(&mut self, name: &str, n: f32) -> Result<()> {
        self.set(name, &n.to_le_bytes())
    }
    fn text(&mut self, name: &str, value: &str) -> Result<()> {
        let mut b = value.as_bytes().to_vec();
        b.push(0);
        self.set(name, &b)
    }
    fn id(&mut self, code: &str, name: &str) -> Result<()> {
        // ID is always the first embedded field; its name starts after four pointers.
        let mut value = format!("{code}{name}");
        while value.len() > 257 {
            value.pop();
        }
        let b = value.as_bytes();
        self.bytes.get_mut(32..32 + b.len()).ok_or_else(|| bad("writer ID overflow"))?.copy_from_slice(b);
        // The ID user count is a positive hint; Blender rebuilds actual users.
        self.bytes.get_mut(296..300).ok_or_else(|| bad("writer ID count"))?.copy_from_slice(&1u32.to_le_bytes());
        Ok(())
    }
}

struct Writer {
    schema: Schema,
    bytes: Vec<u8>,
    next: u64,
}
struct Geometry<'a> {
    name: &'a str,
    cage: &'a Cage,
    levels: Option<usize>,
}
impl Writer {
    fn new() -> Result<Self> {
        Ok(Self { schema: Schema::new()?, bytes: b"BLENDER17-01v0502".to_vec(), next: 0x1000 })
    }
    fn reserve(&mut self) -> u64 {
        let n = self.next;
        self.next += 16;
        n
    }
    fn block(&mut self, code: &[u8; 4], layout: Option<&str>, address: u64, count: usize, bytes: &[u8]) -> Result<()> {
        if self.bytes.len().saturating_add(bytes.len()).saturating_add(32) > MAX_BLEND_BYTES {
            return Err(bad("export exceeds 256 MiB"));
        }
        let sdna =
            if let Some(l) = layout { self.schema.layouts.iter().position(|s| s.name == l).ok_or_else(|| bad("missing export layout"))? } else { 0 };
        self.bytes.extend(code);
        self.bytes.extend((sdna as u32).to_le_bytes());
        self.bytes.extend(address.to_le_bytes());
        self.bytes.extend((bytes.len() as u64).to_le_bytes());
        self.bytes.extend((count as u64).to_le_bytes());
        self.bytes.extend(bytes);
        Ok(())
    }
    fn raw(&mut self, bytes: &[u8]) -> Result<u64> {
        let a = self.reserve();
        self.block(b"DATA", None, a, 1, bytes)?;
        Ok(a)
    }
    fn record(&mut self, code: &[u8; 4], layout: &str, address: u64, count: usize, bytes: Vec<u8>) -> Result<()> {
        self.block(code, Some(layout), address, count, &bytes)
    }
    fn attribute(&mut self, name: &str, ty: u16, domain: u8, count: usize, bytes: &[u8]) -> Result<Vec<u8>> {
        let raw = self.raw(bytes)?;
        let mut label = name.as_bytes().to_vec();
        label.push(0);
        let label = self.raw(&label)?;
        let wrapper = self.reserve();
        let mut data = Record::new(&self.schema, "AttributeArray")?;
        data.ptr("data", raw)?;
        data.ptr("size", count as u64)?;
        let data = data.bytes;
        self.record(b"DATA", "AttributeArray", wrapper, 1, data)?;
        let mut attr = Record::new(&self.schema, "Attribute")?;
        attr.ptr("name", label)?;
        attr.short("data_type", ty)?;
        attr.set("domain", &[domain])?;
        attr.ptr("data", wrapper)?;
        Ok(attr.bytes)
    }
    fn object(&mut self, object: &Geometry<'_>) -> Result<(u64, u64)> {
        let start = self.bytes.len();
        if object.levels.is_some() {
            object.cage.validate().map_err(|e| bad(e.to_string()))?;
        } else {
            validate_surface(object.cage)?;
        }
        if object.levels.is_some() && object.cage.faces.iter().any(|f| f.len() != 4) {
            return Err(bad("Blender Forms export requires quad faces"));
        }
        let corners_count =
            object.cage.faces.iter().try_fold(0usize, |n, f| n.checked_add(f.len()).ok_or_else(|| bad("mesh corner budget overflow")))?;
        let mut edge_set = BTreeSet::new();
        for face in &object.cage.faces {
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                edge_set.insert((a.min(b), a.max(b)));
            }
        }
        let edges: Vec<_> = edge_set.into_iter().collect();
        let mut position = Vec::new();
        for p in &object.cage.points {
            for v in [p.x, p.y, p.z] {
                position.extend((v as f32).to_le_bytes());
            }
        }
        let corners: Vec<u8> = object.cage.faces.iter().flatten().flat_map(|&v| (v as u32).to_le_bytes()).collect();
        let pairs: Vec<u8> = edges.iter().flat_map(|&(a, b)| [a, b]).flat_map(|v| (v as u32).to_le_bytes()).collect();
        let mut attrs = self.attribute("position", 7, 0, object.cage.points.len(), &position)?;
        attrs.extend(self.attribute(".corner_vert", 3, 3, corners_count, &corners)?);
        attrs.extend(self.attribute(".edge_verts", 4, 1, edges.len(), &pairs)?);
        let corner_edges: Vec<u8> = object
            .cage
            .faces
            .iter()
            .flat_map(|face| face.iter().zip(face.iter().cycle().skip(1)).take(face.len()))
            .map(|(&a, &b)| edges.binary_search(&(a.min(b), a.max(b))).map_err(|_| bad("missing cage edge")))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flat_map(|v| (v as u32).to_le_bytes())
            .collect();
        attrs.extend(self.attribute(".corner_edge", 3, 3, corners_count, &corner_edges)?);
        let crease = |s: f64| ((s / 10.).clamp(0., 1.).sqrt() as f32).to_le_bytes();
        let mut ec = vec![0f32; edges.len()];
        for c in &object.cage.edge_creases {
            let e = (c.edge[0].min(c.edge[1]), c.edge[0].max(c.edge[1]));
            let i = edges.binary_search(&e).map_err(|_| bad("invalid crease edge"))?;
            ec[i] = c.sharpness as f32;
        }
        let ec: Vec<u8> = ec.into_iter().flat_map(|v| crease(f64::from(v))).collect();
        attrs.extend(self.attribute("crease_edge", 5, 1, edges.len(), &ec)?);
        let mut vc = vec![0.; object.cage.points.len()];
        for c in &object.cage.vertex_creases {
            *vc.get_mut(c.vertex).ok_or_else(|| bad("invalid crease vertex"))? = c.sharpness;
        }
        let vc: Vec<u8> = vc.into_iter().flat_map(crease).collect();
        attrs.extend(self.attribute("crease_vert", 5, 0, object.cage.points.len(), &vc)?);
        let smooth = vec![0u8; object.cage.faces.len()];
        attrs.extend(self.attribute("sharp_face", 0, 2, object.cage.faces.len(), &smooth)?);
        let attributes = self.reserve();
        self.record(b"DATA", "Attribute", attributes, 7, attrs)?;
        let mut offsets = Vec::with_capacity((object.cage.faces.len() + 1) * 4);
        let mut offset = 0u32;
        offsets.extend(offset.to_le_bytes());
        for face in &object.cage.faces {
            offset = offset.checked_add(face.len() as u32).ok_or_else(|| bad("face corner offset overflow"))?;
            offsets.extend(offset.to_le_bytes());
        }
        let offsets = self.raw(&offsets)?;
        let mesh_address = self.reserve();
        let object_address = self.reserve();
        let link_address = self.reserve();
        let mesh_start = self.bytes.len();
        let mut mesh = Record::new(&self.schema, "Mesh")?;
        mesh.id("ME", &format!("{} Cage", object.name))?;
        mesh.int("totvert", object.cage.points.len())?;
        mesh.int("totedge", edges.len())?;
        mesh.int("totpoly", object.cage.faces.len())?;
        mesh.int("totloop", corners_count)?;
        mesh.ptr("poly_offset_indices", offsets)?;
        let mut storage = Record::new(&self.schema, "AttributeStorage")?;
        storage.ptr("dna_attributes", attributes)?;
        storage.int("dna_attributes_num", 7)?;
        mesh.set("attribute_storage", &storage.bytes)?;
        let mesh = mesh.bytes;
        self.record(b"ME\0\0", "Mesh", mesh_address, 1, mesh)?;
        let mesh_end = self.bytes.len();
        let modifier_address = if let Some(levels) = object.levels {
            let address = self.reserve();
            let mut modifier = Record::new(&self.schema, "SubsurfModifierData")?;
            let mut base = Record::new(&self.schema, "ModifierData")?;
            base.int("type", 1)?;
            base.int("mode", 3)?;
            base.text("name", "Form subdivision")?;
            modifier.set("modifier", &base.bytes)?;
            modifier.short("levels", levels.min(4) as u16)?;
            modifier.short("renderLevels", levels.min(4) as u16)?;
            modifier.short("quality", 3)?;
            self.record(b"DATA", "SubsurfModifierData", address, 1, modifier.bytes)?;
            address
        } else {
            0
        };
        let object_start = self.bytes.len();
        let mut ob = Record::new(&self.schema, "Object")?;
        ob.id("OB", object.name)?;
        ob.ptr("data", mesh_address)?;
        ob.short("type", 1)?;
        ob.short("rotmode", 1)?;
        for field in ["size", "dscale"] {
            ob.set(field, &[1f32, 1., 1.].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>())?;
        }
        for field in ["quat", "dquat"] {
            ob.set(field, &[1f32, 0., 0., 0.].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>())?;
        }
        let list = [modifier_address.to_le_bytes(), modifier_address.to_le_bytes()].concat();
        ob.set("modifiers", &list)?;
        let ob = ob.bytes;
        self.record(b"OB\0\0", "Object", object_address, 1, ob)?;
        // DATA blocks are scoped to the immediately preceding datablock in
        // Blender's loader, even though their serialized pointer IDs are global.
        let ordered =
            [&self.bytes[mesh_start..mesh_end], &self.bytes[start..mesh_start], &self.bytes[object_start..], &self.bytes[mesh_end..object_start]]
                .concat();
        self.bytes.truncate(start);
        self.bytes.extend(ordered);
        Ok((object_address, link_address))
    }
}

pub(super) fn write(objects: &[Object]) -> Result<Vec<u8>> {
    write_objects(&objects.iter().map(|o| Geometry { name: &o.name, cage: &o.cage, levels: Some(o.levels) }).collect::<Vec<_>>())
}

pub(super) fn write_surfaces(objects: &[SurfaceObject]) -> Result<Vec<u8>> {
    if objects.is_empty() || objects.len() > 1024 {
        return Err(bad("export needs 1..1024 surfaces"));
    }
    let mut points = 0usize;
    let mut faces = 0usize;
    for object in objects {
        points = points.saturating_add(object.mesh.positions.len());
        faces = faces.saturating_add(object.mesh.triangles.len());
        if points > 600_000 || faces > 500_000 {
            return Err(bad("surface export exceeds 600000 vertices or 500000 triangles"));
        }
    }
    let cages: Vec<_> = objects
        .iter()
        .map(|o| Cage {
            points: o.mesh.positions.clone(),
            faces: o.mesh.triangles.iter().map(|t| t.iter().map(|&v| v as usize).collect()).collect(),
            ..Default::default()
        })
        .collect();
    write_objects(&objects.iter().zip(&cages).map(|(o, cage)| Geometry { name: &o.name, cage, levels: None }).collect::<Vec<_>>())
}

fn validate_surface(cage: &Cage) -> Result<()> {
    if cage.points.is_empty() || cage.points.len() > 600_000 || cage.faces.is_empty() || cage.faces.len() > 500_000 {
        return Err(bad("empty surface or export mesh budget exceeded"));
    }
    if cage.points.iter().any(|p| !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 1e9) {
        return Err(bad("surface coordinates must be finite and within 1000000000 mm"));
    }
    for face in &cage.faces {
        if face.len() != 3 || face.iter().any(|&v| v >= cage.points.len()) || face[0] == face[1] || face[1] == face[2] || face[2] == face[0] {
            return Err(bad("surface export requires valid triangle indices"));
        }
    }
    Ok(())
}

fn write_objects(objects: &[Geometry<'_>]) -> Result<Vec<u8>> {
    if objects.is_empty() || objects.len() > 1024 {
        return Err(bad("export needs 1..1024 Forms"));
    }
    let mut w = Writer::new()?;
    let scene_address = w.reserve();
    let collection_address = w.reserve();
    let layer_address = w.reserve();
    let mut links = Vec::new();
    for object in objects {
        links.push(w.object(object)?);
    }
    let scene_data_start = w.bytes.len();
    for (i, &(object, address)) in links.iter().enumerate() {
        let mut l = Record::new(&w.schema, "CollectionObject")?;
        l.ptr("ob", object)?;
        l.ptr("next", links.get(i + 1).map_or(0, |v| v.1))?;
        l.ptr("prev", i.checked_sub(1).and_then(|j| links.get(j)).map_or(0, |v| v.1))?;
        let l = l.bytes;
        w.record(b"DATA", "CollectionObject", address, 1, l)?;
    }
    let mut collection = Record::new(&w.schema, "Collection")?;
    collection.id("GR", "Forms")?;
    collection.ptr("owner_id", scene_address)?;
    collection.bytes.get_mut(290..292).ok_or_else(|| bad("embedded collection ID"))?.copy_from_slice(&1024u16.to_le_bytes());
    collection.set("gobject", &[links.first().map_or(0, |v| v.1).to_le_bytes(), links.last().map_or(0, |v| v.1).to_le_bytes()].concat())?;
    let collection = collection.bytes;
    w.record(b"DATA", "Collection", collection_address, 1, collection)?;
    let mut layer = Record::new(&w.schema, "ViewLayer")?;
    layer.text("name", "Forms")?;
    layer.short("flag", 1)?;
    let layer = layer.bytes;
    w.record(b"DATA", "ViewLayer", layer_address, 1, layer)?;
    let tools = w.reserve();
    let mut tools_record = Record::new(&w.schema, "ToolSettings")?;
    tools_record.float("vgroup_weight", 1.)?;
    tools_record.float("doublimit", 0.001)?;
    tools_record.set("selectmode", &[1])?;
    tools_record.int("uvcalc_iterations", 10)?;
    w.record(b"DATA", "ToolSettings", tools, 1, tools_record.bytes)?;
    let scene_start = w.bytes.len();
    let mut scene = Record::new(&w.schema, "Scene")?;
    scene.id("SC", "Forms")?;
    scene.ptr("toolsettings", tools)?;
    scene.ptr("master_collection", collection_address)?;
    scene.set("view_layers", &[layer_address.to_le_bytes(), layer_address.to_le_bytes()].concat())?;
    let mut unit = Record::new(&w.schema, "UnitSettings")?;
    unit.float("scale_length", 0.001)?;
    unit.set("system", &[1])?;
    scene.set("unit", &unit.bytes)?;
    let scene = scene.bytes;
    w.record(b"SC\0\0", "Scene", scene_address, 1, scene)?;
    let ordered = [&w.bytes[scene_start..], &w.bytes[scene_data_start..scene_start]].concat();
    w.bytes.truncate(scene_data_start);
    w.bytes.extend(ordered);
    let mut global = Record::new(&w.schema, "FileGlobal")?;
    global.short("minversion", 502)?;
    global.ptr("curscene", scene_address)?;
    global.ptr("cur_view_layer", layer_address)?;
    global.set("subvstr", b"   0")?;
    let global = global.bytes;
    w.record(b"GLOB", "FileGlobal", 0, 1, global)?;
    let wm_address = w.reserve();
    let mut wm = Record::new(&w.schema, "wmWindowManager")?;
    wm.id("WM", "Forms")?;
    let wm = wm.bytes;
    w.record(b"WM\0\0", "wmWindowManager", wm_address, 1, wm)?;
    let dna = w.schema.dna()?;
    w.block(b"DNA1", None, 0, 1, &dna)?;
    w.block(b"ENDB", None, 0, 0, &[])?;
    Ok(w.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::dna::File;
    use solidcraft_geom::{Mesh, Vec3};
    #[test]
    fn surface_meshes_keep_triangle_offsets_and_have_no_subdivision_modifier() {
        let mesh = Mesh {
            positions: vec![Vec3::new(1., 2., 3.), Vec3::new(3., 2., 3.), Vec3::new(3., 4., 3.), Vec3::new(1., 4., 3.)],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        };
        let bytes = write_surfaces(&[SurfaceObject { name: "CAD surface".into(), mesh: std::sync::Arc::new(mesh.clone()) }]).unwrap();
        let file = File::read(&bytes).unwrap();
        let block = file.blocks.iter().find(|b| b.code == "ME").unwrap();
        let data = file.view(block.bytes, "Mesh").unwrap();
        assert_eq!(data.uint("totvert", 4).unwrap(), 4);
        assert_eq!(data.uint("totpoly", 4).unwrap(), 2);
        assert_eq!(data.uint("totloop", 4).unwrap(), 6);
        let offsets = file.data(data.ptr("poly_offset_indices").unwrap()).unwrap();
        assert_eq!((0..3).map(|i| file.numbers.uint(offsets, i * 4, 4).unwrap()).collect::<Vec<_>>(), vec![0, 3, 6]);
        let object = file.blocks.iter().find(|b| b.code == "OB").unwrap();
        assert_eq!(file.view(object.bytes, "Object").unwrap().nested("modifiers").unwrap().ptr("first").unwrap(), 0);
        assert!(!file.blocks.iter().any(|b| file.structs.get(b.sdna).is_some_and(|s| s.name == "SubsurfModifierData")));
        let imported = crate::blend::read(&bytes).unwrap();
        assert!(imported.objects.is_empty());
        assert!(imported.rejected[0].contains("quad"));
        let mut invalid = mesh;
        invalid.triangles[0] = [0, 0, 2];
        assert!(write_surfaces(&[SurfaceObject { name: "Invalid".into(), mesh: std::sync::Arc::new(invalid) }]).is_err());
    }
}
