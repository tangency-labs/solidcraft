//! Point clouds and the cells between their points: dots, segments drawn as beads (or lines when
//! they have no size), and triangles. Every element is uploaded once, as one record of a single
//! layout whatever its kind; how they are coloured and which are shown is a [`CloudStyle`] of a
//! few numbers the shaders read, so a new colour range or filter range draws again at once
//! without uploading anything.

use std::sync::Arc;

use super::{Batch, Tally, upload};

/// A point cloud as the GPU draws it.
#[derive(Clone)]
pub struct GpuCloud {
    pub id: u64,
    /// Changes when the records change (they are uploaded again).
    pub version: u64,
    /// [`CELL_SIZE`] bytes per element, written by [`cloud_cell`]: the points drawn as dots,
    /// the segments and the triangles.
    pub dots: Arc<Vec<u8>>,
    pub segments: Arc<Vec<u8>>,
    pub triangles: Arc<Vec<u8>>,
    pub style: CloudStyle,
}

/// How a cloud is coloured and which of its elements are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CloudStyle {
    pub mode: ColorMode,
    /// The ramp's range of values.
    pub range: [f32; 2],
    /// An element is shown where every filter keeps its filtered value.
    pub filters: [CloudFilter; FILTERS],
    /// A dot's size on screen, in pixels.
    pub point_size: f32,
}

/// Filters the GPU applies at once.
pub const FILTERS: usize = 4;

/// The values a filter keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CloudFilter {
    /// All of them.
    #[default]
    Off,
    /// Those within a range, ends included.
    Range([f32; 2]),
    /// Whole values `offset + i` for each bit `i` set in `mask`.
    Values { offset: i32, mask: u32 },
}

/// Where an element's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Its own colour ([`Cell::rgb`]).
    #[default]
    Own,
    /// The ramp over [`CloudStyle::range`] at its value.
    Ramp,
    /// The palette at its whole value.
    Palette,
}

/// One element: its corners (a point repeats its own, a segment its second), the coloured value
/// and the four filtered values at each corner, a bead's width and height (zero: a line), its own
/// colour, and which of the four filters and the value it has (bits 0 to 3, and 4, of `has`). A
/// segment also has the points before and after it on its bead (its own ends where it has none),
/// so that two beads meet in the plane halfway between their directions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    pub corners: [[f32; 3]; 3],
    pub neighbours: [[f32; 3]; 2],
    pub value: [f32; 3],
    pub filtered: [[f32; 3]; FILTERS],
    pub size: [f32; 2],
    pub rgb: [u8; 3],
    pub has: u8,
}

/// Bytes per [`Cell`].
pub const CELL_SIZE: usize = 132;

/// Append a cell's record.
pub fn cloud_cell(out: &mut Vec<u8>, c: &Cell) {
    let floats = c.corners.iter().flatten().chain(c.neighbours.iter().flatten()).chain(&c.value).chain(c.filtered.iter().flatten()).chain(&c.size);
    for v in floats {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&c.rgb);
    out.push(c.has);
}

fn cell_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 12] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 8 => Float32x3, 9 => Float32x3,
        3 => Float32x3, 4 => Float32x3, 5 => Float32x3, 10 => Float32x3, 11 => Float32x3,
        6 => Float32x2, 7 => Unorm8x4
    ];
    wgpu::VertexBufferLayout { array_stride: CELL_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
}

/// Corners of a bead's cross-section: a stadium, each half-disc in four chords.
const SIDES: u32 = 10;
/// What each pipeline draws: the records of which kind, and vertices per record. A bead's are
/// indexed into its [`SIDES`] corners at each end, so each is worked out once: its sides, then
/// its two ends closed by fans of triangles.
const DRAWS: [(usize, u32); 3] = [(0, 6), (1, SIDES * 6 + 2 * (SIDES - 2) * 3), (2, 3)];

/// The bead as indices into its corners: corner `j` at its start is `j`, at its end `SIDES + j`.
/// Side `q` is the quad between corners `q` and `q + 1` at both ends. The corners turn
/// counter-clockwise seen from ahead, so every triangle turns counter-clockwise seen from
/// outside: the inside faces away and is not drawn.
fn bead_indices() -> Vec<u16> {
    let n = SIDES as u16;
    let sides = (0..n).flat_map(|q| {
        let r = (q + 1) % n;
        [q, r, n + r, q, n + r, n + q]
    });
    let start = (1..n - 1).flat_map(|j| [0, j + 1, j]);
    let end = (1..n - 1).flat_map(move |j| [n, n + j, n + j + 1]);
    sides.chain(start).chain(end).collect()
}

/// The uniform a cloud's style is written to, bytes: the colouring, then a `vec4` per filter.
const STYLE_SIZE: u64 = 16 + 16 * FILTERS as u64;

fn style_bytes(s: &CloudStyle) -> [u8; STYLE_SIZE as usize] {
    let mode = match s.mode {
        ColorMode::Own => 0.0f32,
        ColorMode::Ramp => 1.0,
        ColorMode::Palette => 2.0,
    };
    let mut words = vec![mode.to_bits(), s.point_size.to_bits(), s.range[0].to_bits(), s.range[1].to_bits()];
    for f in &s.filters {
        // Kind, then a range's ends, or the first value and the mask of those kept.
        words.extend(match *f {
            CloudFilter::Off => [0.0f32.to_bits(), 0, 0, 0],
            CloudFilter::Range([lo, hi]) => [1.0f32.to_bits(), lo.to_bits(), hi.to_bits(), 0],
            CloudFilter::Values { offset, mask } => [2.0f32.to_bits(), (offset as f32).to_bits(), 0, mask],
        });
    }
    let mut out = [0u8; STYLE_SIZE as usize];
    for (o, w) in out.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        *o = w.to_le_bytes();
    }
    out
}

/// The pipelines that draw clouds: dots, segments and triangles.
pub(super) struct CloudPipes {
    layout: wgpu::BindGroupLayout,
    pipes: [wgpu::RenderPipeline; 3],
    bead: wgpu::Buffer,
}

impl CloudPipes {
    pub(super) fn new(
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        viewport: &wgpu::BindGroupLayout,
        target: super::GpuTarget,
        depth: Option<wgpu::DepthStencilState>,
    ) -> CloudPipes {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_cloud"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_cloud"),
            bind_group_layouts: &[Some(viewport), Some(&layout)],
            immediate_size: 0,
        });
        let pipe = |vs: &str, cull: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module, entry_point: Some(vs), buffers: &[Some(cell_layout())], compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: cull, ..Default::default() },
                depth_stencil: depth.clone(),
                multisample: wgpu::MultisampleState { count: target.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_cloud"),
                    targets: &[Some(wgpu::ColorTargetState { format: target.format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let indices: Vec<u8> = bead_indices().iter().flat_map(|i| i.to_le_bytes()).collect();
        let bead = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor { label: Some("sc_cloud_bead"), contents: &indices, usage: wgpu::BufferUsages::INDEX },
        );
        CloudPipes {
            pipes: [pipe("vs_cloud_dot", None), pipe("vs_cloud_segment", Some(wgpu::Face::Back)), pipe("vs_cloud_triangle", None)],
            layout,
            bead,
        }
    }
}

/// A cloud on the GPU: its records by kind and its style's uniform.
struct Uploaded {
    version: u64,
    kinds: [Option<Batch>; 3],
    style: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

/// The clouds on the GPU, by id.
#[derive(Default)]
pub(super) struct CloudDraws(std::collections::HashMap<u64, Uploaded>);

impl CloudDraws {
    /// Upload new or changed clouds, drop removed ones, and write every style.
    pub(super) fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pipes: &CloudPipes, clouds: &[GpuCloud]) {
        self.0.retain(|id, _| clouds.iter().any(|c| c.id == *id));
        for c in clouds {
            if self.0.get(&c.id).is_none_or(|u| u.version != c.version) {
                let style = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sc_cloud_style"),
                    size: STYLE_SIZE,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sc_cloud_style"),
                    layout: &pipes.layout,
                    entries: &[wgpu::BindGroupEntry { binding: 3, resource: style.as_entire_binding() }],
                });
                let kinds = [&c.dots, &c.segments, &c.triangles].map(|bytes| upload(device, "sc_cloud", bytes, CELL_SIZE));
                self.0.insert(c.id, Uploaded { version: c.version, kinds, style, bind });
            }
            if let Some(u) = self.0.get(&c.id) {
                queue.write_buffer(&u.style, 0, &style_bytes(&c.style));
            }
        }
    }

    pub(super) fn paint(&self, pass: &mut wgpu::RenderPass<'_>, pipes: &CloudPipes, tally: &Tally) {
        for u in self.0.values() {
            pass.set_bind_group(1, &u.bind, &[]);
            for (pipe, (kind, vertices)) in pipes.pipes.iter().zip(DRAWS) {
                let Some(Some(batch)) = u.kinds.get(kind) else { continue };
                pass.set_pipeline(pipe);
                for (buffer, count) in &batch.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    if kind == 1 {
                        pass.set_index_buffer(pipes.bead.slice(..), wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..vertices, 0, 0..*count);
                    } else {
                        pass.draw(0..vertices, 0..*count);
                    }
                    tally.triangles(vertices, *count);
                }
            }
        }
    }
}

/// What a frame's clouds add to its picture's key.
pub(super) fn hash_clouds(clouds: &[GpuCloud], h: &mut impl std::hash::Hasher) {
    use std::hash::Hash as _;
    for c in clouds {
        c.id.hash(h);
        c.version.hash(h);
        style_bytes(&c.style).hash(h);
    }
}
