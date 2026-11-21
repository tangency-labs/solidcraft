//! The controllers' own models in the room (`solidcraft_vr::model`, as the VR software hands them
//! out): their meshes and pictures uploaded once per model, each mesh placed every frame where the
//! runtime says the controller and its moving parts (stick, trigger, buttons) are.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui_wgpu::{self, wgpu};
use solidcraft_vr::model::{self, Mat4, Model};

/// The depth buffer's format, for every pipeline drawn after the backdrop.
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A mesh's uniform: where it is (`mat4x4<f32>`) and its colour (`vec4<f32>`).
const MESH_UNIFORM: u64 = 64 + 16;

/// A vertex: place, normal, picture coordinates.
const VERTEX: u64 = 32;

/// Which model is drawn where: its asset's id and the hand holding it (two hands may hold the
/// same kind of controller, each drawn in its own place).
pub type Key = ([u8; 16], Option<usize>);

/// One controller to draw this frame.
pub struct Draw {
    pub key: Key,
    pub model: Arc<Model>,
    /// Where the model's origin is in the room.
    pub pose: Mat4,
    /// Each node's place relative to the origin; `None` for one not shown.
    pub places: Vec<Option<Mat4>>,
}

/// A mesh on the GPU.
struct GpuMesh {
    node: usize,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    colour: [f32; 4],
}

/// The most bytes the controllers' pictures take on the GPU together: a model whose pictures
/// would go past it is drawn without them (in its colours alone).
pub const MAX_PICTURE_BYTES: u64 = 256 * 1024 * 1024;

/// A model on the GPU, for one hand: its meshes (each bound to its asset's pictures).
struct Uploaded {
    meshes: Vec<GpuMesh>,
}

/// An asset's pictures on the GPU, shared by every hand holding a controller of it, and the bytes
/// they take there (none: drawn without them).
struct Pictures {
    views: Vec<wgpu::TextureView>,
    bytes: u64,
}

/// The smallest side [`fit_pictures`] shrinks pictures to (pixels).
const MIN_PICTURE_SIDE: u32 = 64;

/// The bytes a model's pictures (`images`) take on the GPU (four a pixel), however big they say
/// they are.
pub fn picture_bytes(images: &[model::Image]) -> u64 {
    images.iter().map(|i| u64::from(i.size[0]).saturating_mul(u64::from(i.size[1])).saturating_mul(4)).fold(0, u64::saturating_add)
}

/// Whether pictures of `bytes` more fit beside the `used` ones within [`MAX_PICTURE_BYTES`].
fn pictures_fit(used: u64, bytes: u64) -> bool {
    used.saturating_add(bytes) <= MAX_PICTURE_BYTES
}

/// Keep the uploads whose asset is still `live` (the models the VR software handed out), drawn
/// this frame or not: a controller that loses tracking for a frame is not uploaded again.
fn keep_live<T>(uploaded: &mut HashMap<Key, T>, live: &[[u8; 16]]) {
    uploaded.retain(|key, _| live.contains(&key.0));
}

/// Shrink the pictures of `models` until they fit in [`MAX_PICTURE_BYTES`] together, halving the
/// side they are kept at each time, down to [`MIN_PICTURE_SIDE`]: every controller keeps its
/// pictures, smaller, rather than one going without (a controller can have a dozen large ones).
/// The side they were kept at.
pub fn fit_pictures(models: &mut [Model]) -> u32 {
    let total = |models: &[Model]| models.iter().map(|m| picture_bytes(&m.images)).fold(0, u64::saturating_add);
    let mut side = model::KEPT_IMAGE_SIDE;
    while !pictures_fit(0, total(models)) && side > MIN_PICTURE_SIDE {
        side /= 2;
        for image in models.iter_mut().flat_map(|m| m.images.iter_mut()) {
            if image.size[0].max(image.size[1]) > side {
                *image = image.shrunk(side);
            }
        }
    }
    side
}

/// The controllers' models by their asset's id.
pub type Models = HashMap<[u8; 16], Arc<Model>>;

/// Each of `assets` read into a model (its meshes, and its pictures decoded); one that cannot be
/// read is left out, and the log says why.
pub fn read(assets: Vec<solidcraft_xr::ModelAsset>) -> Models {
    let listed = assets.len();
    let mut ids = Vec::new();
    let mut read = Vec::new();
    for a in assets {
        // An asset listed twice (both hands hold the same kind) is read once.
        if ids.contains(&a.cache) {
            continue;
        }
        match model::parse(&a.glb, &a.nodes) {
            Ok(model) => {
                ids.push(a.cache);
                read.push(model);
            }
            Err(e) => log::info!("VR: a controller model was not read: {e}"),
        }
    }
    let side = fit_pictures(&mut read);
    let pictures: Vec<usize> = read.iter().map(|m| m.images.len()).collect();
    let bytes = read.iter().map(|m| picture_bytes(&m.images)).fold(0, u64::saturating_add);
    log::info!("VR: {} of {listed} controller models read; pictures {pictures:?}, at most {side} pixels a side, {bytes} bytes", read.len());
    ids.into_iter().zip(read).map(|(id, model)| (id, Arc::new(model))).collect()
}

/// Reads the controllers' models beside the frame: parsing a model and decoding its pictures
/// takes longer than a frame may, so the frame goes on with the models it has until the new ones
/// are read.
#[derive(Default)]
pub struct Reader {
    job: Option<std::sync::mpsc::Receiver<Models>>,
}

impl Reader {
    /// Start reading `assets` ([`read`]) on a thread of its own, in place of a reading still
    /// going (its models are not wanted any more). Where no thread can be started, nothing is
    /// read: the controllers are drawn as boxes.
    pub fn start(&mut self, assets: Vec<solidcraft_xr::ModelAsset>) {
        let (send, receive) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("sc_vr_models".into()).spawn(move || {
            // The frame stopped waiting for these (another reading took over): nobody to tell.
            let _ = send.send(read(assets));
        });
        self.job = match spawned {
            Ok(_) => Some(receive),
            Err(e) => {
                log::info!("VR: the controller models were not read: {e}");
                None
            }
        };
    }

    /// The models read, once: `None` while they are being read, when nothing is, or when the
    /// reading failed (the models the frame has are kept).
    pub fn poll(&mut self) -> Option<Models> {
        let result = self.job.as_ref()?.try_recv();
        match result {
            Ok(models) => {
                self.job = None;
                Some(models)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                None
            }
        }
    }

    /// Wait for the models being read (tests: no clock decides when they are there).
    #[cfg(test)]
    fn wait(&mut self) -> Option<Models> {
        self.job.take()?.recv().ok()
    }
}

/// Draws the controllers' models.
pub struct Controllers {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// A white picture for meshes without one.
    white: wgpu::TextureView,
    uploaded: HashMap<Key, Uploaded>,
    /// Each asset's pictures, shared by the hands.
    pictures: HashMap<[u8; 16], Pictures>,
    /// The meshes to draw this frame, after [`Controllers::prepare`]: which model, which mesh.
    shown: Vec<(Key, usize)>,
}

impl Controllers {
    /// The pipeline for pictures in `format`, from `module`'s `vs_ctrl` and `fs_ctrl`.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        module: &wgpu::ShaderModule,
        format: wgpu::TextureFormat,
        view_uniform_size: u64,
    ) -> Controllers {
        let uniform = |binding, visibility, size| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_vr_ctrl"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX, view_uniform_size),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                uniform(4, wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT, MESH_UNIFORM),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_vr_ctrl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_vr_ctrl"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_ctrl"),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: VERTEX, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRIBUTES })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_ctrl"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            // Assets are not always wound the same way: both sides drawn, the depth sorts them.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(true, wgpu::CompareFunction::Less)),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let white = picture(device, queue, [1, 1], &[255; 4]);
        Controllers { pipeline, layout, white, uploaded: HashMap::new(), pictures: HashMap::new(), shown: Vec::new() }
    }

    /// Get `draws` ready: each model uploaded the first time it is seen (with `view_uniform` and
    /// `sampler` bound), each shown mesh's place written. Models whose asset is no longer `live`
    /// are let go; the others stay uploaded while they are not drawn (tracking lost a moment).
    pub fn prepare(&mut self, rs: &egui_wgpu::RenderState, draws: &[Draw], live: &[[u8; 16]], view_uniform: &wgpu::Buffer, sampler: &wgpu::Sampler) {
        keep_live(&mut self.uploaded, live);
        self.pictures.retain(|id, _| live.contains(id));
        self.shown.clear();
        for draw in draws {
            if !self.pictures.contains_key(&draw.key.0) {
                let used = self.pictures.values().map(|p| p.bytes).fold(0, u64::saturating_add);
                let bytes = picture_bytes(&draw.model.images);
                let pictures = if pictures_fit(used, bytes) {
                    let views = draw.model.images.iter().map(|image| picture(&rs.device, &rs.queue, image.size, &image.rgba)).collect();
                    Pictures { views, bytes }
                } else {
                    log::info!("VR: a controller model is drawn without its pictures: they would take more than {MAX_PICTURE_BYTES} bytes");
                    Pictures { views: Vec::new(), bytes: 0 }
                };
                self.pictures.insert(draw.key.0, pictures);
            }
            if !self.uploaded.contains_key(&draw.key) {
                let views = self.pictures.get(&draw.key.0).map_or(&[][..], |p| &p.views[..]);
                let meshes = self.upload(rs, &draw.model, views, view_uniform, sampler);
                self.uploaded.insert(draw.key, Uploaded { meshes });
            }
            let Some(uploaded) = self.uploaded.get(&draw.key) else { continue };
            for (i, mesh) in uploaded.meshes.iter().enumerate() {
                let Some(Some(place)) = draw.places.get(mesh.node) else { continue };
                rs.queue.write_buffer(&mesh.uniform, 0, &mesh_uniform(&model::mul(&draw.pose, place), mesh.colour));
                self.shown.push((draw.key, i));
            }
        }
    }

    /// The meshes of `model` on the GPU, bound to its asset's `pictures` on the GPU (none: each
    /// mesh in its colour alone).
    fn upload(
        &self,
        rs: &egui_wgpu::RenderState,
        model: &Model,
        pictures: &[wgpu::TextureView],
        view_uniform: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
    ) -> Vec<GpuMesh> {
        use wgpu::util::DeviceExt as _;
        let device = &rs.device;
        model
            .meshes
            .iter()
            .filter(|m| !m.indices.is_empty())
            .map(|mesh| {
                let vertex_bytes: Vec<u8> = mesh.vertices.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
                let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|v| v.to_le_bytes()).collect();
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("sc_vr_ctrl"),
                    contents: &vertex_bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("sc_vr_ctrl"),
                    contents: &index_bytes,
                    usage: wgpu::BufferUsages::INDEX,
                });
                let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sc_vr_ctrl"),
                    size: MESH_UNIFORM,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let picture = mesh.image.and_then(|i| pictures.get(i)).unwrap_or(&self.white);
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sc_vr_ctrl"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: view_uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(picture) },
                        wgpu::BindGroupEntry { binding: 4, resource: uniform.as_entire_binding() },
                    ],
                });
                GpuMesh { node: mesh.node, vertices, indices, count: mesh.indices.len() as u32, uniform, bind, colour: mesh.colour }
            })
            .collect()
    }

    /// Draw this frame's meshes ([`Controllers::prepare`]) into `pass`, whose depth buffer is
    /// [`DEPTH`].
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.shown.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        for (key, i) in &self.shown {
            let Some(mesh) = self.uploaded.get(key).and_then(|u| u.meshes.get(*i)) else { continue };
            pass.set_bind_group(0, &mesh.bind, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
    }
}

/// A mesh's uniform as the shader takes it ([`MESH_UNIFORM`]): where it is in the room (`world`,
/// its columns one after another), then its colour.
fn mesh_uniform(world: &Mat4, colour: [f32; 4]) -> Vec<u8> {
    let mut bytes: Vec<u8> = world.iter().flatten().flat_map(|v| (*v as f32).to_le_bytes()).collect();
    bytes.extend(colour.iter().flat_map(|v| v.to_le_bytes()));
    bytes
}

/// Depth testing for a pipeline drawn after the backdrop: writing the depth or not, and how it
/// compares.
pub fn depth_state(write: bool, compare: wgpu::CompareFunction) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// A picture `size` pixels in sRGB RGBA, `rgba` its pixels.
fn picture(device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2], rgba: &[u8]) -> wgpu::TextureView {
    use wgpu::util::DeviceExt as _;
    let descriptor = wgpu::TextureDescriptor {
        label: Some("sc_vr_ctrl"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    device
        .create_texture_with_data(queue, &descriptor, wgpu::util::TextureDataOrder::LayerMajor, rgba)
        .create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh's uniform is the shader's `C` (a matrix, then a colour): as big, the place's
    /// move in the matrix's last column, the colour after it; a place that is not a number still
    /// fills it exactly. A vertex is the asset's eight numbers.
    #[test]
    fn a_meshs_uniform_and_vertex_are_what_the_shader_reads() {
        assert_eq!(super::super::tests::wgsl_struct_size(super::super::SHADER, "C"), MESH_UNIFORM);
        let mut world = model::IDENTITY;
        world[3] = [0.1, 0.2, 0.3, 1.0];
        let bytes = mesh_uniform(&world, [0.5, 0.6, 0.7, 1.0]);
        assert_eq!(bytes.len() as u64, MESH_UNIFORM);
        let float = |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        assert_eq!([float(48), float(52), float(56), float(60)], [0.1, 0.2, 0.3, 1.0], "the move in the last column");
        assert_eq!([float(64), float(76)], [0.5, 1.0], "the colour after the matrix");
        assert_eq!(mesh_uniform(&[[f64::NAN; 4]; 4], [f32::INFINITY; 4]).len() as u64, MESH_UNIFORM);
        assert_eq!(std::mem::size_of::<[f32; 8]>() as u64, VERTEX);
    }

    /// A model asset that is not glTF is left out (read on its own thread, never in the frame),
    /// and the frame learns of it by polling: nothing while it reads, the models once, then
    /// nothing again. A new reading takes the place of one still going.
    #[test]
    fn models_are_read_beside_the_frame_and_bad_ones_left_out() {
        let asset =
            |cache: u8, glb: &[u8]| solidcraft_xr::ModelAsset { cache: [cache; 16], hand: None, glb: glb.to_vec(), nodes: vec!["trigger".into()] };
        assert!(read(vec![asset(1, b"not glTF"), asset(2, b"glTF\x02\x00\x00\x00\x04\x00\x00\x00")]).is_empty());
        let mut reader = Reader::default();
        assert!(reader.poll().is_none(), "nothing asked");
        reader.start(vec![asset(1, b"junk")]);
        reader.start(vec![asset(3, b"more junk"), asset(4, &[0; 64])]);
        assert_eq!(reader.wait().map(|m| m.len()), Some(0), "the last reading's models: none could be read");
        assert!(reader.poll().is_none(), "told once");
    }

    /// A model stays uploaded while its asset is the VR software's, drawn this frame or not (a
    /// controller losing tracking for a frame is not uploaded again); one whose asset is gone is
    /// let go, for either hand.
    #[test]
    fn uploads_are_kept_while_their_asset_is_live() {
        let mut uploaded: HashMap<Key, ()> = [(([1; 16], Some(0)), ()), (([1; 16], Some(1)), ()), (([2; 16], None), ())].into_iter().collect();
        keep_live(&mut uploaded, &[[1; 16], [9; 16]]);
        let mut kept: Vec<Key> = uploaded.keys().copied().collect();
        kept.sort();
        assert_eq!(kept, [([1; 16], Some(0)), ([1; 16], Some(1))]);
        keep_live(&mut uploaded, &[]);
        assert!(uploaded.is_empty(), "no models handed out: none kept");
    }

    /// The controllers' pictures together stay within [`MAX_PICTURE_BYTES`]: a model whose
    /// pictures would go past it is drawn without them. Sizes that would overflow count as too
    /// big, not as small.
    #[test]
    fn the_controllers_pictures_stay_within_their_memory() {
        let with = |sides: &[[u32; 2]]| -> Vec<model::Image> { sides.iter().map(|s| model::Image { size: *s, rgba: Vec::new() }).collect() };
        assert_eq!(picture_bytes(&with(&[])), 0);
        assert_eq!(picture_bytes(&with(&[[2, 3], [1, 1]])), 28, "four bytes a pixel");
        let big = picture_bytes(&with(&[[4096, 4096]; 4]));
        assert_eq!(big, MAX_PICTURE_BYTES, "four of the largest pictures a model may have");
        assert!(pictures_fit(0, big));
        assert!(!pictures_fit(1, big), "one byte past");
        assert!(pictures_fit(MAX_PICTURE_BYTES, 0), "no pictures always fit");
        assert_eq!(picture_bytes(&with(&[[u32::MAX, u32::MAX]; 16])), u64::MAX, "saturated, not wrapped");
        assert!(!pictures_fit(0, u64::MAX) && !pictures_fit(u64::MAX, 1));
    }

    /// Pictures too large together are shrunk until they fit, every model keeping its own, in
    /// their shape; ones that fit are kept as they are. Two Steam Frame controllers have 9 and 12
    /// pictures of 2048 a side: kept at 1024.
    #[test]
    fn the_controllers_pictures_are_shrunk_to_fit_together() {
        let blank = |side: u32| model::Image { size: [side, side], rgba: vec![0; (side * side * 4) as usize] };
        let with = |count: usize, side: u32| {
            let mut model = Model::default();
            model.images = (0..count).map(|_| blank(side)).collect();
            model
        };
        let mut frame = [with(9, 2048), with(12, 2048)];
        assert_eq!(fit_pictures(&mut frame), 1024);
        assert!(frame.iter().all(|m| m.images.iter().all(|i| i.size == [1024, 1024] && i.rgba.len() == 1024 * 1024 * 4)));
        assert!(pictures_fit(0, frame.iter().map(|m| picture_bytes(&m.images)).sum()));
        let mut small = [with(2, 256)];
        assert_eq!(fit_pictures(&mut small), model::KEPT_IMAGE_SIDE, "fits: nothing shrunk");
        assert_eq!(small[0].images[0].size, [256, 256]);
        let mut none: [Model; 0] = [];
        assert_eq!(fit_pictures(&mut none), model::KEPT_IMAGE_SIDE);
    }

    /// The controllers draw over the backdrop with the depth buffer every pipeline after it
    /// shares.
    #[test]
    fn the_controllers_depth_is_the_one_the_room_shares() {
        let state = depth_state(true, wgpu::CompareFunction::Less);
        assert_eq!((state.format, state.depth_write_enabled, state.depth_compare), (DEPTH, Some(true), Some(wgpu::CompareFunction::Less)));
        assert_eq!(depth_state(false, wgpu::CompareFunction::Always).depth_write_enabled, Some(false));
    }
}
