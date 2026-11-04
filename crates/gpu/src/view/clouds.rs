//! Point clouds: each point a round dot a few pixels across, facing the screen, depth-tested and
//! written like the model's faces. Every point is uploaded once, with the values it is coloured
//! and filtered by; how they are coloured and which are shown is a [`CloudStyle`] of a few
//! numbers the shaders read, so a new colour range or filter range draws again at once without
//! uploading anything.

use std::sync::Arc;

use super::{Batch, Tally, upload};

/// A point cloud as the GPU draws it.
#[derive(Clone)]
pub struct GpuCloud {
    pub id: u64,
    /// Changes when the records change (they are uploaded again).
    pub version: u64,
    /// [`DOT_SIZE`] bytes per point, written by [`cloud_dot`].
    pub dots: Arc<Vec<u8>>,
    pub style: CloudStyle,
}

/// How a cloud is coloured and which of its points are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CloudStyle {
    pub mode: ColorMode,
    /// The ramp's range of values.
    pub range: [f32; 2],
    /// A point is shown where every filter keeps its filtered value.
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

/// Where a point's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Its own colour ([`Dot::rgb`]).
    #[default]
    Own,
    /// The ramp over [`CloudStyle::range`] at its value.
    Ramp,
    /// The palette at its whole value.
    Palette,
}

/// One point: where it is, the coloured value and the four filtered values, its own colour, and
/// which of the four filters and the value it has (bits 0 to 3, and 4, of `has`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dot {
    pub at: [f32; 3],
    pub value: f32,
    pub filtered: [f32; FILTERS],
    pub rgb: [u8; 3],
    pub has: u8,
}

/// Bytes per [`Dot`].
pub const DOT_SIZE: usize = 36;

/// Append a dot's record.
pub fn cloud_dot(out: &mut Vec<u8>, d: &Dot) {
    for v in d.at.iter().chain([&d.value]).chain(&d.filtered) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&d.rgb);
    out.push(d.has);
}

fn dot_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x4, 3 => Unorm8x4];
    wgpu::VertexBufferLayout { array_stride: DOT_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
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

/// The pipeline that draws clouds' dots, and the layout of their styles.
pub(super) struct CloudPipes {
    layout: wgpu::BindGroupLayout,
    dots: wgpu::RenderPipeline,
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
        let dots = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vs_cloud_dot"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_cloud_dot"),
                buffers: &[Some(dot_layout())],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: depth,
            multisample: wgpu::MultisampleState { count: target.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_cloud"),
                targets: &[Some(wgpu::ColorTargetState { format: target.format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        CloudPipes { layout, dots }
    }
}

/// A cloud on the GPU: its dots and its style's uniform.
struct Uploaded {
    version: u64,
    dots: Option<Batch>,
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
                let dots = upload(device, "sc_cloud", &c.dots, DOT_SIZE);
                self.0.insert(c.id, Uploaded { version: c.version, dots, style, bind });
            }
            if let Some(u) = self.0.get(&c.id) {
                queue.write_buffer(&u.style, 0, &style_bytes(&c.style));
            }
        }
    }

    pub(super) fn paint(&self, pass: &mut wgpu::RenderPass<'_>, pipes: &CloudPipes, tally: &Tally) {
        for u in self.0.values() {
            let Some(batch) = &u.dots else { continue };
            pass.set_bind_group(1, &u.bind, &[]);
            pass.set_pipeline(&pipes.dots);
            for (buffer, count) in &batch.parts {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..*count);
                tally.triangles(6, *count);
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
