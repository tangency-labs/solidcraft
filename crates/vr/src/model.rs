//! A controller's model as the VR runtime hands it out (a GLB, core glTF), read into what the
//! frame draws: triangle meshes with their colour and picture, on a tree of nodes, some of which
//! the runtime moves each frame (the stick, the trigger, the buttons).
//!
//! The asset comes from the VR software, so it is read with care: counts and sizes are capped,
//! and anything wrong is an error, never a panic.

use solidcraft_xr::view::Pose;

/// The most nodes taken.
const MAX_NODES: usize = 1024;

/// The most vertices in one model.
const MAX_VERTICES: usize = 1_000_000;

/// The most pictures in one model.
const MAX_IMAGES: usize = 16;

/// The largest picture side taken (pixels).
const MAX_IMAGE_SIDE: u32 = 4096;

/// The most a picture's decoding may hold at once (bytes): the largest picture at 16 bits a
/// channel.
const MAX_IMAGE_BYTES: u64 = MAX_IMAGE_SIDE as u64 * MAX_IMAGE_SIDE as u64 * 8;

/// The most indices in one model: three for each vertex taken.
const MAX_INDICES: usize = 3 * MAX_VERTICES;

/// The most meshes (one for each primitive drawn) in one model.
const MAX_MESHES: usize = 4096;

/// The deepest a node hangs below the top of its tree: a controller's parts hang a few deep, and
/// placing a node walks up its parents.
const MAX_DEPTH: usize = 64;

/// A 4 × 4 matrix, its columns one after another.
pub type Mat4 = [[f64; 4]; 4];

/// The identity matrix.
pub const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// One node of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub parent: Option<usize>,
    /// Its place within its parent, as the asset gives it.
    pub local: Mat4,
    pub name: String,
}

/// One triangle mesh with one material.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    /// The node it hangs on.
    pub node: usize,
    /// Each vertex: place (3), normal (3), picture coordinates (2).
    pub vertices: Vec<[f32; 8]>,
    pub indices: Vec<u32>,
    /// The base colour (linear, with alpha).
    pub colour: [f32; 4],
    /// The picture its base colour is multiplied by, an index into [`Model::images`].
    pub image: Option<usize>,
}

/// A picture: RGBA, 8 bits a channel, colours as the screen shows them (sRGB).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

/// A controller's model.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>,
    pub images: Vec<Image>,
    /// For each moving part the runtime names (in its order), the node it moves.
    pub animated: Vec<Option<usize>>,
    /// [`Model::stick`], found once when the model is read (from the nodes' names).
    stick_node: Option<usize>,
    /// [`Model::middle_of`] for each node, worked out once when the model is read (from the
    /// nodes' parents and own places, and the meshes' vertices): a frame asks for them for both
    /// hands, and a model's nodes and vertices do not change while it is drawn. Changing
    /// [`Model::nodes`] or [`Model::meshes`] afterwards does not change them.
    middles: Vec<Option<[f64; 3]>>,
}

/// Read `glb`, `animatable` the names of the parts the runtime moves (its order kept).
pub fn parse(glb: &[u8], animatable: &[String]) -> Result<Model, String> {
    // The glTF reader takes the header's length less the header's own 12 bytes without checking
    // that it is at least that: a shorter one is refused here first.
    let length = glb.get(8..12).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    if glb.starts_with(b"glTF") && !length.is_some_and(|l| l >= 12) {
        return Err("the controller model is not glTF: its header is cut short".into());
    }
    let not_gltf = |e: gltf::Error| format!("the controller model is not glTF: {e}");
    let unchecked = gltf::Gltf::from_slice_without_validation(glb).map_err(not_gltf)?;
    let json = unchecked.document.into_json();
    // The glTF checks look up each primitive's POSITION accessor without checking that it is
    // there: a primitive naming one past the accessors is refused before they run.
    let positions = gltf::json::validation::Checked::Valid(gltf::json::mesh::Semantic::Positions);
    let accessors = json.accessors.len();
    if json.meshes.iter().flat_map(|m| &m.primitives).any(|p| p.attributes.get(&positions).is_some_and(|a| a.value() >= accessors)) {
        return Err("the controller model is not glTF: a mesh names a position accessor it does not have".into());
    }
    let document = gltf::Document::from_json(json).map_err(not_gltf)?;
    let blob = unchecked.blob.as_deref();
    let doc = &document;
    if doc.nodes().len() > MAX_NODES {
        return Err(format!("the controller model has {} nodes", doc.nodes().len()));
    }
    let mut nodes: Vec<Node> =
        doc.nodes().map(|n| Node { parent: None, local: to_f64(n.transform().matrix()), name: n.name().unwrap_or_default().to_string() }).collect();
    for n in doc.nodes() {
        for child in n.children() {
            if let Some(c) = nodes.get_mut(child.index()) {
                c.parent = Some(n.index());
            }
        }
    }
    let mut images = Vec::new();
    let mut image_of = std::collections::HashMap::new();
    let mut meshes = Vec::new();
    let mut vertices_seen = 0usize;
    let mut indices_seen = 0usize;
    for n in doc.nodes() {
        let Some(mesh) = n.mesh() else { continue };
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let Some(at) = primitive.get(&gltf::Semantic::Positions) else { continue };
            // The glTF reader trusts each accessor's numbers and panics on some (no items, a
            // stride shorter than an item, an index of a type it has no case for): checked here
            // before anything is read, and the counts capped before anything is held.
            use gltf::accessor::{DataType as T, Dimensions as D};
            let fits = |a: &gltf::Accessor, shape: fn(T, D) -> bool| shape(a.data_type(), a.dimensions()) && readable(a, blob);
            let vec3 = |t, d| matches!((t, d), (T::F32, D::Vec3));
            let uv = |t, d| matches!((t, d), (T::F32 | T::U8 | T::U16, D::Vec2));
            let index = |t, d| matches!((t, d), (T::U8 | T::U16 | T::U32, D::Scalar));
            let normals_fit = primitive.get(&gltf::Semantic::Normals).is_none_or(|a| fits(&a, vec3));
            let uvs_fit = primitive.get(&gltf::Semantic::TexCoords(0)).is_none_or(|a| fits(&a, uv));
            let indices_fit = primitive.indices().is_none_or(|a| fits(&a, index));
            if !(fits(&at, vec3) && normals_fit && uvs_fit && indices_fit) {
                return Err("the controller model has a mesh its own data does not hold".into());
            }
            vertices_seen = vertices_seen.saturating_add(at.count());
            if vertices_seen > MAX_VERTICES {
                return Err("the controller model has too many vertices".into());
            }
            indices_seen = indices_seen.saturating_add(primitive.indices().map_or(at.count(), |a| a.count()));
            if indices_seen > MAX_INDICES {
                return Err("the controller model has too many triangles".into());
            }
            if meshes.len() >= MAX_MESHES {
                return Err("the controller model has too many meshes".into());
            }
            let reader = primitive.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(_) => None,
            });
            let Some(positions) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = positions.collect();
            let normals: Vec<[f32; 3]> = reader.read_normals().map(Iterator::collect).unwrap_or_default();
            let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_default();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if indices.iter().any(|&i| i as usize >= positions.len()) || !indices.len().is_multiple_of(3) {
                return Err("the controller model has a mesh that points past its vertices".into());
            }
            let vertices = positions
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let n = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                    let t = uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                    [p[0], p[1], p[2], n[0], n[1], n[2], t[0], t[1]]
                })
                .collect();
            let pbr = primitive.material().pbr_metallic_roughness();
            let image = match pbr.base_color_texture().map(|t| t.texture().source()) {
                Some(source) => match image_of.get(&source.index()) {
                    Some(at) => *at,
                    None => {
                        let read = if images.len() < MAX_IMAGES { read_image(&source, blob) } else { None };
                        let at = read.map(|image| {
                            images.push(image);
                            images.len() - 1
                        });
                        image_of.insert(source.index(), at);
                        at
                    }
                },
                None => None,
            };
            meshes.push(Mesh { node: n.index(), vertices, indices, colour: pbr.base_color_factor(), image });
        }
    }
    if meshes.is_empty() {
        // Drawn, it would leave the hand empty: a hand with a model of its own has no box.
        return Err("the controller model has nothing to draw".into());
    }
    // A loop of parents (a broken asset) would never end the walk up, and a tree too deep would
    // take the stack with it: refused.
    for node in &nodes {
        let mut at = node.parent;
        let mut steps = 0;
        while let Some(p) = at {
            steps += 1;
            if steps > nodes.len() || steps > MAX_DEPTH {
                return Err("the controller model's nodes form a loop or nest too deep".into());
            }
            at = nodes.get(p).and_then(|n| n.parent);
        }
    }
    let animated = animatable.iter().map(|name| nodes.iter().position(|n| !name.is_empty() && n.name == *name)).collect();
    // Unnamed nodes keep an empty name; a picture that could not be read leaves its meshes plain.
    nodes.shrink_to_fit();
    let stick_node = find_stick(&nodes);
    let middles = middles(&nodes, &meshes);
    Ok(Model { nodes, meshes, images, animated, stick_node, middles })
}

/// The node of the stick (or the touchpad standing in for one), by the names runtimes give it.
fn find_stick(nodes: &[Node]) -> Option<usize> {
    let named = |words: &[&str]| nodes.iter().position(|n| words.iter().any(|w| n.name.to_ascii_lowercase().contains(w)));
    named(&["thumbstick", "joystick"]).or_else(|| named(&["trackpad", "touchpad"]))
}

/// A box: its least and its greatest corner.
type Bounds = ([f64; 3], [f64; 3]);

/// `bounds` grown to hold `p`; a point that is not one is passed over.
fn grow(bounds: &mut Option<Bounds>, p: [f64; 3]) {
    if !p.iter().all(|v| v.is_finite()) {
        return;
    }
    let (lo, hi) = bounds.get_or_insert((p, p));
    for k in 0..3 {
        lo[k] = lo[k].min(p[k]);
        hi[k] = hi[k].max(p[k]);
    }
}

/// For each node, the middle of the box around the meshes hanging on it or on any node below
/// it, in the node's own place (the asset's own places of the nodes between); `None` where no
/// mesh does. A child's box is carried up to its parent as its eight corners placed there, so
/// a child's middle lands exactly where it is in the parent. The tree has no loops and is
/// shallow (refused in [`parse`] before this runs).
fn middles(nodes: &[Node], meshes: &[Mesh]) -> Vec<Option<[f64; 3]>> {
    let mut boxes: Vec<Option<Bounds>> = vec![None; nodes.len()];
    for mesh in meshes {
        let Some(slot) = boxes.get_mut(mesh.node) else { continue };
        for v in &mesh.vertices {
            grow(slot, [v[0], v[1], v[2]].map(f64::from));
        }
    }
    // The deepest first: each node's box holds everything below it before it is carried up.
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = nodes.get(i).and_then(|n| n.parent)
            && d <= MAX_DEPTH
        {
            d += 1;
            i = p;
        }
        d
    };
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(depth(i)));
    for i in order {
        let (Some(Some((lo, hi))), Some(node)) = (boxes.get(i).copied(), nodes.get(i)) else { continue };
        let Some(parent) = node.parent else { continue };
        let m = &node.local;
        for c in 0..8 {
            let corner = [0, 1, 2].map(|k| if c & (1 << k) == 0 { lo[k] } else { hi[k] });
            let placed = [0, 1, 2].map(|k| m[0][k] * corner[0] + m[1][k] * corner[1] + m[2][k] * corner[2] + m[3][k]);
            if let Some(slot) = boxes.get_mut(parent) {
                grow(slot, placed);
            }
        }
    }
    boxes.into_iter().map(|b| b.map(|(lo, hi)| [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5))).collect()
}

/// The picture `source` in the asset's own buffer, when it is one this can read (PNG).
fn read_image(source: &gltf::Image, blob: Option<&[u8]>) -> Option<Image> {
    let gltf::image::Source::View { view, .. } = source.source() else { return None };
    let start = view.offset();
    let bytes = blob?.get(start..start.checked_add(view.length())?)?;
    // Limited before it is decoded: a small file can claim a huge picture.
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_IMAGE_BYTES);
    reader.limits(limits);
    let picture = reader.decode().ok()?;
    if picture.width() > MAX_IMAGE_SIDE || picture.height() > MAX_IMAGE_SIDE || picture.width() == 0 || picture.height() == 0 {
        return None;
    }
    let rgba = picture.to_rgba8();
    Some(Image { size: [rgba.width(), rgba.height()], rgba: rgba.into_raw() })
}

/// Whether the glTF reader can read all of `accessor` from the asset's own buffer `blob`: at
/// least one item, not sparse (controller models have none), a stride that holds a whole item,
/// every item within its view and the view within the buffer.
fn readable(accessor: &gltf::Accessor, blob: Option<&[u8]>) -> bool {
    let (Some(view), Some(blob), None) = (accessor.view(), blob, accessor.sparse()) else { return false };
    if !matches!(view.buffer().source(), gltf::buffer::Source::Bin) {
        return false;
    }
    let item = accessor.size();
    let stride = view.stride().unwrap_or(item);
    let end = accessor
        .count()
        .checked_sub(1)
        .and_then(|n| n.checked_mul(stride))
        .and_then(|n| n.checked_add(item))
        .and_then(|n| n.checked_add(accessor.offset()));
    let view_end = view.offset().checked_add(view.length());
    item > 0 && stride >= item && end.is_some_and(|end| end <= view.length()) && view_end.is_some_and(|end| end <= blob.len())
}

/// A glTF matrix in double precision.
fn to_f64(m: [[f32; 4]; 4]) -> Mat4 {
    m.map(|column| column.map(f64::from))
}

/// `a` · `b`.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, value) in column.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// A pose as a matrix (a turn, then a move).
pub fn pose_matrix(pose: &Pose) -> Option<Mat4> {
    let [x, y, z] = pose.axes()?;
    let p = pose.position;
    Some([[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [p[0], p[1], p[2], 1.0]])
}

impl Model {
    /// Where each node is relative to the model's origin, with the runtime's `states` (each
    /// moving part's pose within its parent, and whether it shows) in place of the asset's own;
    /// `None` for a node not shown (it or a node above it hidden). Without states the asset's own
    /// places stand.
    pub fn places(&self, states: &[(Pose, bool)]) -> Vec<Option<Mat4>> {
        let mut local: Vec<Option<Mat4>> = self.nodes.iter().map(|n| Some(n.local)).collect();
        for (state, node) in states.iter().zip(&self.animated) {
            let (Some(node), (pose, visible)) = (node, state) else { continue };
            if let Some(slot) = local.get_mut(*node) {
                *slot = if *visible { pose_matrix(pose) } else { None };
            }
        }
        let mut out: Vec<Option<Option<Mat4>>> = vec![None; self.nodes.len()];
        for i in 0..self.nodes.len() {
            self.place(i, &local, &mut out, 0);
        }
        out.into_iter().map(Option::flatten).collect()
    }

    /// Node `i`'s place, worked out once (`out`), its parents first.
    fn place(&self, i: usize, local: &[Option<Mat4>], out: &mut [Option<Option<Mat4>>], depth: usize) -> Option<Mat4> {
        if let Some(done) = out.get(i).copied().flatten() {
            return done;
        }
        let own = local.get(i).copied().flatten();
        let parent = self.nodes.get(i).and_then(|n| n.parent);
        let placed = match (own, parent) {
            (None, _) => None,
            (Some(m), None) => Some(m),
            // The tree has no loops and is shallow (refused in `parse`); the cap guards all the same.
            (Some(_), Some(_)) if depth > MAX_DEPTH => None,
            (Some(m), Some(p)) => self.place(p, local, out, depth + 1).map(|pm| mul(&pm, &m)),
        };
        if let Some(slot) = out.get_mut(i) {
            *slot = Some(placed);
        }
        placed
    }

    /// The middle of the meshes hanging on `node` or on any node below it (the middle of their
    /// box), in the node's own place: where a part is, as a runtime often leaves its nodes at the
    /// model's origin and puts the part's place in its vertices, or hangs the part's meshes on a
    /// child of the node it moves. `None` when no mesh hangs there. Worked out when the model was
    /// read.
    pub fn middle_of(&self, node: usize) -> Option<[f64; 3]> {
        self.middles.get(node).copied().flatten()
    }

    /// The node of the stick (or the touchpad standing in for one), by the names runtimes give
    /// it. Found when the model was read.
    pub fn stick(&self) -> Option<usize> {
        self.stick_node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `json` and `bin` as a GLB: the header, then the JSON chunk padded with spaces and the
    /// binary chunk padded with zeros, each to four bytes.
    fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let pad = |mut v: Vec<u8>, with: u8| {
            while !v.len().is_multiple_of(4) {
                v.push(with);
            }
            v
        };
        let (json, bin) = (pad(json.as_bytes().to_vec(), b' '), pad(bin.to_vec(), 0));
        let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(json);
        if !bin.is_empty() {
            out.extend((bin.len() as u32).to_le_bytes());
            out.extend(b"BIN\0");
            out.extend(bin);
        }
        out
    }

    /// `values` as little-endian bytes.
    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// A PNG `width` × `height`, all one colour.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(width, height, image::Rgba([255, 128, 0, 255])))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    /// The buffer of [`controller`]: three positions, normals and picture coordinates, three
    /// `u16` indices (and two bytes to keep four apart), then `picture`. 104 bytes before it.
    fn buffer(indices: [u16; 3], picture: &[u8]) -> Vec<u8> {
        let mut bin = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
        bin.extend(floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        bin.extend(floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        bin.extend(indices.iter().flat_map(|i| i.to_le_bytes()));
        bin.extend([0, 0]);
        bin.extend(picture);
        bin
    }

    /// The accessors of [`controller`], `positions` and `indices` given (the normals and picture
    /// coordinates as they are).
    fn accessors(positions: &str, indices: &str) -> String {
        format!(
            r#"[{positions},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":2,"componentType":5126,"count":3,"type":"VEC2"}},{indices}]"#
        )
    }

    /// The position accessor of [`controller`] as it should be.
    const POSITIONS: &str = r#"{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,2,0]}"#;

    /// The index accessor of [`controller`] as it should be.
    const INDICES: &str = r#"{"bufferView":3,"componentType":5123,"count":3,"type":"SCALAR"}"#;

    /// The buffer views of [`controller`] as they should be, the picture `picture` bytes long.
    fn views(picture: usize) -> String {
        format!(
            r#"[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":36}},{{"buffer":0,"byteOffset":72,"byteLength":24}},{{"buffer":0,"byteOffset":96,"byteLength":6}},{{"buffer":0,"byteOffset":104,"byteLength":{picture}}}]"#
        )
    }

    /// The nodes of [`controller`] as they should be: the body with its triangle, and a stick
    /// hanging on it a centimetre up.
    const NODES: &str = r#"[{"name":"body","mesh":0,"children":[1]},{"name":"Thumbstick","translation":[0,0.01,0]}]"#;

    /// A controller's model: one triangle (positions, normals, picture coordinates, indices)
    /// with a picture, on `nodes`, its views and accessors given.
    fn asset(views: &str, accessors: &str, nodes: &str, bin: &[u8]) -> Vec<u8> {
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":{}}}],"bufferViews":{views},"accessors":{accessors},"images":[{{"bufferView":4,"mimeType":"image/png"}}],"textures":[{{"source":0}}],"materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[0.5,0.25,1,1],"baseColorTexture":{{"index":0}}}}}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2}},"indices":3,"material":0}}]}}],"nodes":{nodes}}}"#,
            bin.len()
        );
        glb(&json, bin)
    }

    /// The model as it should be, its picture 2 × 1.
    fn controller() -> Vec<u8> {
        let picture = png(2, 1);
        asset(&views(picture.len()), &accessors(POSITIONS, INDICES), NODES, &buffer([0, 1, 2], &picture))
    }

    /// The moving part names a runtime would give for [`controller`]: the stick, one the model
    /// does not have, and an empty one.
    fn names() -> Vec<String> {
        ["Thumbstick", "trigger", ""].map(String::from).to_vec()
    }

    /// Whether two matrices are the same but for the rounding of `f32`.
    fn close(a: &Mat4, b: &Mat4) -> bool {
        a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    /// The model is read whole: its tree, its triangle with each vertex's place, normal and
    /// picture coordinates, its colour and picture, and the runtime's moving parts found by name.
    #[test]
    fn a_controller_model_is_read_with_its_tree_mesh_and_picture() {
        let model = parse(&controller(), &names()).unwrap();
        assert_eq!(model.nodes.len(), 2);
        assert_eq!((model.nodes[0].parent, model.nodes[1].parent), (None, Some(0)));
        assert_eq!(model.nodes[1].name, "Thumbstick");
        assert_eq!(model.meshes.len(), 1);
        let mesh = &model.meshes[0];
        assert_eq!(mesh.node, 0);
        assert_eq!(mesh.indices, [0, 1, 2]);
        assert_eq!(mesh.vertices[1], [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
        assert_eq!(mesh.vertices[2], [0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0]);
        assert_eq!(mesh.colour, [0.5, 0.25, 1.0, 1.0]);
        assert_eq!(mesh.image, Some(0));
        assert_eq!(model.images.len(), 1);
        assert_eq!(model.images[0].size, [2, 1]);
        assert_eq!(model.images[0].rgba, [255, 128, 0, 255, 255, 128, 0, 255]);
        assert_eq!(model.animated, [Some(1), None, None], "found by name; an empty name finds nothing");
        assert_eq!(model.stick(), Some(1), "by its name, whatever its case");
        assert_eq!(model.middle_of(0), Some([0.5, 1.0, 0.0]), "the middle of the triangle's box");
        assert_eq!(model.middle_of(1), None, "no mesh hangs on the stick");
        assert_eq!(model.middle_of(2), None, "no such node");
    }

    /// The model with `nodes` in place of [`NODES`] (the triangle is mesh 0).
    fn with_nodes(nodes: &str) -> Model {
        let picture = png(2, 1);
        parse(&asset(&views(picture.len()), &accessors(POSITIONS, INDICES), nodes, &buffer([0, 1, 2], &picture)), &[]).unwrap()
    }

    /// A stick whose mesh hangs on a child of the stick's node has its middle where the child
    /// puts it, and so has every node above it; a turned child's middle turns with it.
    #[test]
    fn a_parts_middle_counts_the_meshes_of_the_nodes_below_it() {
        let nodes = r#"[{"name":"body","children":[1]},{"name":"Thumbstick","children":[2],"translation":[0,0.01,0]},{"name":"cap","mesh":0,"translation":[0,0,0.5]}]"#;
        let model = with_nodes(nodes);
        assert_eq!(model.stick(), Some(1));
        assert_eq!(model.middle_of(2), Some([0.5, 1.0, 0.0]), "the cap's own");
        assert_eq!(model.middle_of(1), Some([0.5, 1.0, 0.5]), "the stick's, from its cap");
        let body = model.middle_of(0).unwrap();
        assert!((0..3).all(|k| (body[k] - [0.5, 1.01, 0.5][k]).abs() < 1e-6), "{body:?}");
        // The cap turned a quarter about Z: its box's middle (0.5, 1) goes to (-1, 0.5).
        let (s, c) = std::f64::consts::FRAC_PI_4.sin_cos();
        let turned = format!(r#"[{{"name":"Thumbstick","children":[1]}},{{"name":"cap","mesh":0,"rotation":[0,0,{s},{c}]}}]"#);
        let middle = with_nodes(&turned).middle_of(0).unwrap();
        assert!((0..3).all(|k| (middle[k] - [-1.0, 0.5, 0.0][k]).abs() < 1e-6), "{middle:?}");
    }

    /// The stick is found by its name, whatever its case, a touchpad standing in where there is
    /// no stick; a model with neither has none.
    #[test]
    fn the_stick_is_found_by_name_with_a_touchpad_standing_in() {
        let pad = with_nodes(r#"[{"name":"body","mesh":0,"children":[1,2]},{"name":"TrackPad"},{"name":"Joystick_Top"}]"#);
        assert_eq!(pad.stick(), Some(2), "a stick before a pad");
        let only_pad = with_nodes(r#"[{"name":"body","mesh":0,"children":[1]},{"name":"touchpad"}]"#);
        assert_eq!(only_pad.stick(), Some(1));
        assert_eq!(with_nodes(r#"[{"name":"body","mesh":0}]"#).stick(), None);
    }

    /// Without the runtime's states each node stands where the asset puts it, under its parent;
    /// a state moves its part, a hidden part is not drawn and neither is what hangs on it, and a
    /// part whose pose is no pose is not drawn either.
    #[test]
    fn the_runtime_moves_and_hides_the_parts() {
        let model = parse(&controller(), &names()).unwrap();
        let mut up = IDENTITY;
        up[3][1] = 0.01;
        let places = model.places(&[]);
        assert_eq!(places[0], Some(IDENTITY));
        assert!(close(&places[1].unwrap(), &up), "a centimetre up");

        let pushed = Pose { position: [0.0, 0.0, 0.5], orientation: [0.0, 0.0, 0.0, 1.0] };
        let mut back = IDENTITY;
        back[3][2] = 0.5;
        let places = model.places(&[(pushed, true)]);
        assert!(close(&places[1].unwrap(), &back), "the state in place of the asset's own");
        assert_eq!(model.places(&[(pushed, false)]), [Some(IDENTITY), None]);
        let lost = Pose { position: [0.0; 3], orientation: [0.0; 4] };
        assert_eq!(model.places(&[(lost, true)]), [Some(IDENTITY), None]);
        // More states than moving parts: the extra ones change nothing.
        assert_eq!(model.places(&[(pushed, false), (pushed, false), (pushed, false), (pushed, false)]), [Some(IDENTITY), None]);

        // The body hidden: the stick on it goes too.
        let by_body = parse(&controller(), &["body".to_string()]).unwrap();
        assert_eq!(by_body.places(&[(pushed, false)]), [None, None]);
    }

    /// A pose as a matrix turns, then moves; matrices multiply as columns.
    #[test]
    fn poses_and_matrices_compose() {
        let (s, c) = (std::f64::consts::FRAC_PI_4.sin(), std::f64::consts::FRAC_PI_4.cos());
        // A quarter turn about the vertical: X to −Z, Z to X.
        let quarter = Pose { position: [1.0, 2.0, 3.0], orientation: [0.0, s, 0.0, c] };
        let m = pose_matrix(&quarter).unwrap();
        let want = [[0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 2.0, 3.0, 1.0]];
        assert!(m.iter().flatten().zip(want.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12), "{m:?}");
        assert_eq!(mul(&IDENTITY, &m), m);
        assert_eq!(mul(&m, &IDENTITY), m);
        // Moved one along its own X after the turn: one toward −Z in the room.
        let mut along = IDENTITY;
        along[3][0] = 1.0;
        let moved = mul(&m, &along);
        assert!((0..3).all(|k| (moved[3][k] - [1.0, 2.0, 2.0][k]).abs() < 1e-12), "{moved:?}");
        assert_eq!(pose_matrix(&Pose { position: [0.0; 3], orientation: [0.0; 4] }), None);
        assert_eq!(pose_matrix(&Pose { position: [f64::NAN, 0.0, 0.0], orientation: [0.0, 0.0, 0.0, 1.0] }), None);
    }

    /// Bytes that are no model, or a GLB cut anywhere, are refused with an error, never a panic.
    #[test]
    fn what_is_not_a_model_is_refused() {
        assert!(parse(&[], &[]).is_err());
        assert!(parse(b"not a model at all", &[]).is_err());
        assert!(parse(&[0xff; 64], &[]).is_err());
        assert!(parse(b"glTF", &[]).is_err());
        let whole = controller();
        for end in 0..whole.len() {
            assert!(parse(&whole[..end], &names()).is_err(), "cut at {end}");
        }
        // A header whose length is shorter than the header: the glTF reader would take 12 from it.
        for length in [0u32, 4, 11] {
            let mut short = whole.clone();
            short[8..12].copy_from_slice(&length.to_le_bytes());
            assert!(parse(&short, &[]).is_err(), "length {length}");
        }
        let mut long = whole.clone();
        long[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&long, &[]).is_err(), "a header longer than the bytes");
        let mut version = whole;
        version[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert!(parse(&version, &[]).is_err(), "a version this does not read");
    }

    /// A model with nothing to draw is refused: a hand given it would show nothing at all.
    #[test]
    fn a_model_with_no_mesh_is_refused() {
        let picture = png(2, 1);
        let bin = buffer([0, 1, 2], &picture);
        let nodes = r#"[{"name":"body"},{"name":"Thumbstick"}]"#;
        let error = parse(&asset(&views(picture.len()), &accessors(POSITIONS, INDICES), nodes, &bin), &names()).unwrap_err();
        assert!(error.contains("nothing to draw"), "{error}");
        let points = r#"{"asset":{"version":"2.0"},"meshes":[{"primitives":[{"attributes":{},"mode":0}]}],"nodes":[{"mesh":0}]}"#;
        assert!(parse(points.as_bytes(), &[]).is_err(), "points only, no triangles");
    }

    /// Accessors whose numbers the glTF reader would trust and panic on (no items, a stride
    /// shorter than an item, indices of a type it has no case for, sparse data, items past the
    /// end of their view or the view past the buffer) are refused before anything is read.
    #[test]
    fn accessors_the_reader_would_trip_on_are_refused() {
        let picture = png(2, 1);
        let bin = buffer([0, 1, 2], &picture);
        let read = |views: &str, positions: &str, indices: &str| parse(&asset(views, &accessors(positions, indices), NODES, &bin), &[]);
        let good_views = views(picture.len());
        assert!(read(&good_views, POSITIONS, INDICES).is_ok());
        let no_items = POSITIONS.replace(r#""count":3"#, r#""count":0"#);
        assert!(read(&good_views, &no_items, INDICES).is_err(), "no items");
        let narrow = good_views.replacen(r#""byteLength":36}"#, r#""byteLength":36,"byteStride":4}"#, 1);
        assert!(read(&narrow, POSITIONS, INDICES).is_err(), "a stride of 4 bytes for 12-byte items");
        let float_indices = INDICES.replace("5123", "5126");
        assert!(read(&good_views, POSITIONS, &float_indices).is_err(), "indices as floats");
        let signed_indices = INDICES.replace("5123", "5122");
        assert!(read(&good_views, POSITIONS, &signed_indices).is_err(), "indices as signed numbers");
        let flat = POSITIONS.replace("VEC3", "VEC2");
        assert!(read(&good_views, &flat, INDICES).is_err(), "positions with two numbers");
        let sparse = POSITIONS
            .replace(r#""count":3,"#, r#""count":3,"sparse":{"count":1,"indices":{"bufferView":3,"componentType":5123},"values":{"bufferView":0}},"#);
        assert!(read(&good_views, &sparse, INDICES).is_err(), "sparse");
        let past_view = POSITIONS.replace(r#""count":3"#, r#""count":4"#);
        assert!(read(&good_views, &past_view, INDICES).is_err(), "more items than the view holds");
        let offset = POSITIONS.replace(r#""bufferView":0,"#, r#""bufferView":0,"byteOffset":4,"#);
        assert!(read(&good_views, &offset, INDICES).is_err(), "items moved past the view's end");
        let huge = POSITIONS.replace(r#""count":3"#, r#""count":18446744073709551615"#);
        assert!(read(&good_views, &huge, INDICES).is_err(), "a count that overflows any size");
        let past_buffer = good_views.replacen(r#""byteOffset":0,"byteLength":36"#, r#""byteOffset":100000,"byteLength":36"#, 1);
        assert!(read(&past_buffer, POSITIONS, INDICES).is_err(), "a view past the buffer");
        let wrapping = good_views.replacen(r#""byteOffset":0,"byteLength":36"#, r#""byteOffset":18446744073709551615,"byteLength":36"#, 1);
        assert!(read(&wrapping, POSITIONS, INDICES).is_err(), "a view whose end overflows");
        let no_view = POSITIONS.replace(r#""bufferView":0,"#, "");
        assert!(read(&good_views, &no_view, INDICES).is_err(), "positions with no data");
        // A primitive naming a position accessor past the accessors: the glTF checks would index
        // past them.
        let named = |at: &str| {
            let json = format!(
                r#"{{"asset":{{"version":"2.0"}},"accessors":[],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":{at}}}}}]}}],"nodes":[{{"mesh":0}}]}}"#
            );
            parse(json.as_bytes(), &[])
        };
        assert!(named("0").is_err());
        assert!(named("4294967295").is_err());
        let mut past = asset(&good_views, &accessors(POSITIONS, INDICES), NODES, &bin);
        let at = past.windows(12).position(|w| w == br#""POSITION":0"#).unwrap();
        past[at + 11] = b'4';
        assert!(parse(&past, &[]).is_err(), "POSITION 4 of four accessors");
    }

    /// Indices past the vertices, or not three to a triangle, are refused.
    #[test]
    fn triangles_that_point_past_their_vertices_are_refused() {
        let picture = png(2, 1);
        let at = |indices: [u16; 3], accessor: &str| {
            parse(&asset(&views(picture.len()), &accessors(POSITIONS, accessor), NODES, &buffer(indices, &picture)), &[])
        };
        assert!(at([0, 1, 2], INDICES).is_ok());
        assert!(at([0, 1, 3], INDICES).is_err(), "index 3 of three vertices");
        assert!(at([0, 1, u16::MAX], INDICES).is_err());
        assert!(at([0, 1, 2], &INDICES.replace(r#""count":3"#, r#""count":2"#)).is_err(), "two indices: no triangle");
    }

    /// A tree whose parents form a loop, or nest deeper than [`MAX_DEPTH`], is refused; one just
    /// shallow enough is read and placed all the way down.
    #[test]
    fn trees_that_loop_or_nest_too_deep_are_refused() {
        let picture = png(2, 1);
        let bin = buffer([0, 1, 2], &picture);
        let read = |nodes: &str| parse(&asset(&views(picture.len()), &accessors(POSITIONS, INDICES), nodes, &bin), &[]);
        assert!(read(r#"[{"mesh":0,"children":[0]}]"#).is_err(), "its own child");
        assert!(read(r#"[{"mesh":0,"children":[1]},{"children":[0]}]"#).is_err(), "each the other's child");
        // A chain of `count` nodes, each a centimetre along Z from its parent.
        let chain = |count: usize| {
            let nodes: Vec<String> = (0..count)
                .map(|i| {
                    let mesh = if i == 0 { r#""mesh":0,"# } else { "" };
                    let children = if i + 1 < count { format!(r#","children":[{}]"#, i + 1) } else { String::new() };
                    format!(r#"{{{mesh}"translation":[0,0,0.01]{children}}}"#)
                })
                .collect();
            format!("[{}]", nodes.join(","))
        };
        // The last of MAX_DEPTH + 1 nodes hangs MAX_DEPTH below the top: taken.
        let deepest = read(&chain(MAX_DEPTH + 1)).unwrap();
        let last = deepest.places(&[])[MAX_DEPTH].unwrap();
        assert!((last[3][2] - 0.01 * (MAX_DEPTH + 1) as f64).abs() < 1e-5, "{}", last[3][2]);
        assert!(read(&chain(MAX_DEPTH + 2)).is_err(), "one deeper");
        assert!(read(&chain(MAX_NODES)).is_err());
    }

    /// More nodes, vertices, triangles or meshes than a controller has are refused before they
    /// are held, however few bytes claim them.
    #[test]
    fn models_past_the_caps_are_refused() {
        let empty_nodes = |count: usize| format!(r#"[{{"mesh":0}}{}]"#, ",{}".repeat(count - 1));
        let picture = png(2, 1);
        let bin = buffer([0, 1, 2], &picture);
        let read = |nodes: &str| parse(&asset(&views(picture.len()), &accessors(POSITIONS, INDICES), nodes, &bin), &[]);
        assert!(read(&empty_nodes(MAX_NODES)).is_ok());
        let error = read(&empty_nodes(MAX_NODES + 1)).unwrap_err();
        assert!(error.contains("1025 nodes"), "{error}");

        // `count` nodes sharing one mesh of `vertices` vertices (zeros) and no indices.
        let shared = |vertices: usize, count: usize| {
            let json = format!(
                r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":{bytes}}}],"bufferViews":[{{"buffer":0,"byteLength":{bytes}}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":{vertices},"type":"VEC3","min":[0,0,0],"max":[0,0,0]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"nodes":[{nodes}]}}"#,
                bytes = vertices * 12,
                nodes = vec![r#"{"mesh":0}"#; count].join(",")
            );
            parse(&glb(&json, &vec![0; vertices * 12]), &[])
        };
        // 333 × 3003 = 999 999 vertices: taken; 334 × 3003 is past a million.
        assert_eq!(shared(3003, 333).unwrap().meshes.len(), 333);
        let error = shared(3003, 334).unwrap_err();
        assert!(error.contains("vertices"), "{error}");

        // Three vertices, `count` nodes sharing one mesh of `indices` indices (all 0).
        let indexed = |indices: usize, count: usize| {
            let json = format!(
                r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":{bytes}}}],"bufferViews":[{{"buffer":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":{index_bytes}}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[0,0,0]}},{{"bufferView":1,"componentType":5123,"count":{indices},"type":"SCALAR"}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],"nodes":[{nodes}]}}"#,
                bytes = 36 + indices * 2,
                index_bytes = indices * 2,
                nodes = vec![r#"{"mesh":0}"#; count].join(",")
            );
            parse(&glb(&json, &vec![0; 36 + indices * 2]), &[])
        };
        // 100 × 30 000 = three million indices: taken; one more node is past the cap.
        assert!(indexed(30_000, 100).is_ok());
        let error = indexed(30_000, 101).unwrap_err();
        assert!(error.contains("triangles"), "{error}");

        // One node, one mesh of `count` triangles, each its own primitive.
        let primitives = |count: usize| {
            let json = format!(
                r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":36}}],"bufferViews":[{{"buffer":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[0,0,0]}}],"meshes":[{{"primitives":[{p}]}}],"nodes":[{{"mesh":0}}]}}"#,
                p = vec![r#"{"attributes":{"POSITION":0}}"#; count].join(",")
            );
            parse(&glb(&json, &[0; 36]), &[])
        };
        assert_eq!(primitives(MAX_MESHES).unwrap().meshes.len(), MAX_MESHES);
        let error = primitives(MAX_MESHES + 1).unwrap_err();
        assert!(error.contains("meshes"), "{error}");
    }

    /// A picture wider than [`MAX_IMAGE_SIDE`], one that only claims to be huge, or one that is
    /// no picture is left out and its mesh drawn plain; the model itself is still read.
    #[test]
    fn pictures_too_big_or_broken_leave_the_mesh_plain() {
        let model_with =
            |picture: &[u8]| parse(&asset(&views(picture.len()), &accessors(POSITIONS, INDICES), NODES, &buffer([0, 1, 2], picture)), &[]).unwrap();
        let wide = model_with(&png(MAX_IMAGE_SIDE + 1, 1));
        assert_eq!((wide.meshes[0].image, wide.images.len()), (None, 0));
        let widest = model_with(&png(MAX_IMAGE_SIDE, 1));
        assert_eq!(widest.images[0].size, [MAX_IMAGE_SIDE, 1]);
        let broken = model_with(b"\x89PNG\r\n\x1a\n not really");
        assert_eq!((broken.meshes[0].image, broken.images.len()), (None, 0));
        // A real PNG whose header claims 100 000 × 100 000 pixels.
        let mut claims = png(1, 1);
        claims[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        claims[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert_eq!(model_with(&claims).meshes[0].image, None);
    }

    /// Every digit of the asset's JSON changed to every other digit (counts, offsets, lengths,
    /// strides, indices, component types and sizes all go wrong in turn) and every byte of its
    /// binary chunk flipped: each is read or refused, never a panic.
    #[test]
    fn a_model_with_any_number_changed_never_panics() {
        let whole = controller();
        let json_end = 20 + u32::from_le_bytes([whole[12], whole[13], whole[14], whole[15]]) as usize;
        let mut tried = 0;
        for at in 20..json_end {
            if !whole[at].is_ascii_digit() {
                continue;
            }
            for digit in b'0'..=b'9' {
                let mut changed = whole.clone();
                changed[at] = digit;
                let _ = parse(&changed, &names());
                tried += 1;
            }
        }
        assert!(tried > 500, "{tried}");
        for at in json_end..whole.len() {
            let mut changed = whole.clone();
            changed[at] ^= 0xa5;
            let _ = parse(&changed, &names());
        }
    }
}
