//! Display-only outlines, prepared and classified in bounded batches on native and wasm.
//! Topology is shared by placements and survives camera movement. Large scenes fill in over
//! subsequent frames, within adjacency and output safety caps; authored edges always draw.
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use solidcore_record::Vec3;
use solidcraft_engine::doc::{Mat, apply_point, apply_vector, mat_inverse};
use solidcraft_engine::geom::Mesh;
use solidcraft_engine::render::{Camera, PreparedSilhouettes, SilhouetteBuilder, SilhouetteView};

pub(crate) const WORK_PER_FRAME: usize = 8_192;
const MAX_ADJACENCY_EDGES: usize = 250_000;
const MAX_CACHED_CANDIDATES: usize = 500_000;
const MAX_CACHED_MESHES: usize = 256;
/// Bounds copying/uploading the view-dependent buffer as well as its allocation.
const MAX_SEGMENTS: usize = 65_536;

pub(crate) struct Source {
    mesh: Arc<Mesh>,
    placement: Mat,
    inverse: Mat,
}

impl Source {
    pub fn new(mesh: Arc<Mesh>, placement: Mat) -> Option<Self> {
        Some(Self { mesh, inverse: mat_inverse(&placement)?, placement })
    }

    fn view(&self, view: SilhouetteView) -> SilhouetteView {
        match view {
            SilhouetteView::Orthographic { toward_eye } => SilhouetteView::Orthographic { toward_eye: apply_vector(&self.inverse, toward_eye) },
            SilhouetteView::Perspective { eye } => SilhouetteView::Perspective { eye: apply_point(&self.inverse, eye) },
        }
    }
}

struct Prepared {
    // A weak identity prevents address reuse without retaining obsolete mesh buffers.
    _mesh: Weak<Mesh>,
    edges: PreparedSilhouettes,
}

#[derive(Default)]
pub(crate) struct Outlines {
    model: Option<u64>,
    bodies: usize,
    source: Option<Source>,
    prepared: HashMap<usize, Prepared>,
    making: Option<(Arc<Mesh>, SilhouetteBuilder)>,
    cancel: bool,
    cached_candidates: usize,
    view: Option<SilhouetteView>,
    body: usize,
    edge: usize,
    segments: Vec<[Vec3; 2]>,
    generation: u64,
}

impl Outlines {
    pub fn model(&self) -> Option<u64> {
        self.model
    }

    pub fn set_model(&mut self, model: Option<u64>, bodies: usize) {
        self.model = model;
        self.bodies = bodies;
        // A partially built tree is retired incrementally, never dropped by an orbit or edit.
        self.cancel = self.making.is_some();
        if bodies == 0 {
            self.prepared.clear();
            self.cached_candidates = 0;
        }
        self.view = None;
        self.restart();
    }

    fn restart(&mut self) {
        self.body = 0;
        self.edge = 0;
        self.source = None;
        self.segments.clear();
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn segments(&self) -> &[[Vec3; 2]] {
        &self.segments
    }

    pub fn busy(&self) -> bool {
        self.making.is_some() || (self.body < self.bodies && self.segments.len() < MAX_SEGMENTS)
    }

    /// Sources are requested only as work reaches them, including placements and hidden bodies.
    /// Returns work consumed, so tests can assert a frame's bound without wall-clock timing.
    pub fn advance(&mut self, camera: &Camera, budget: usize, mut source: impl FnMut(usize) -> Option<Source>) -> usize {
        let view = if camera.fov > 0.0 {
            SilhouetteView::Perspective { eye: camera.eye() }
        } else {
            SilhouetteView::Orthographic { toward_eye: camera.back() }
        };
        if self.view != Some(view) {
            self.view = Some(view);
            self.restart();
        }
        let before = self.segments.len();
        let mut left = budget;
        while left > 0 && self.busy() {
            // Preparation does not depend on the camera or classification cursor. Complete
            // it even if an orbit restarted classification on an earlier placement.
            if let Some((mesh, builder)) = self.making.as_mut() {
                let done = if self.cancel { builder.discard(&mut left) } else { builder.advance(mesh, &mut left) };
                if !done {
                    break;
                }
                let Some((mesh, builder)) = self.making.take() else { break };
                if self.cancel {
                    self.cancel = false;
                    continue;
                }
                let Some(edges) = builder.finish() else { break };
                // Contiguous candidate buffers have a bounded total allocation. Evicted
                // topology is rebuilt incrementally if a later camera needs it.
                if self.cached_candidates.saturating_add(edges.candidates().len()) > MAX_CACHED_CANDIDATES || self.prepared.len() >= MAX_CACHED_MESHES
                {
                    self.prepared.clear();
                    self.cached_candidates = 0;
                }
                self.cached_candidates += edges.candidates().len();
                self.prepared.insert(Arc::as_ptr(&mesh) as usize, Prepared { _mesh: Arc::downgrade(&mesh), edges });
                continue;
            }
            if self.source.is_none() {
                left -= 1;
                self.source = source(self.body);
                if self.source.is_none() {
                    self.body += 1;
                }
                continue;
            }
            let Some(current) = self.source.as_ref() else { break };
            let key = Arc::as_ptr(&current.mesh) as usize;
            if let Some(prepared) = self.prepared.get(&key) {
                left -= 1;
                if let Some(edge) = prepared.edges.candidates().get(self.edge) {
                    self.edge += 1;
                    if let Some(segment) = edge.segment(current.view(view)) {
                        let segment = segment.map(|p| apply_point(&current.placement, p));
                        if segment.iter().all(|p| [p.x, p.y, p.z].iter().all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))) {
                            self.segments.push(segment);
                        }
                    }
                } else {
                    self.body += 1;
                    self.edge = 0;
                    self.source = None;
                }
            } else {
                self.making = Some((current.mesh.clone(), SilhouetteBuilder::new(MAX_ADJACENCY_EDGES)));
            }
        }
        if self.segments.len() != before {
            self.generation = self.generation.wrapping_add(1);
        }
        budget - left
    }
}
