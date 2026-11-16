//! Bounded legacy sheet/box preparation. The strict synchronous admission API
//! may drain this cursor; an image task instead yields before tracing any pixel.
use super::{
    Bounds, Result, TraceError, WorkBudget,
    box_certificate::BoxTask,
    dielectric::{self, Certificate, MAX_OVERRIDES},
};
use crate::{
    ProductScene,
    job::{CancelToken, DielectricBoundary, ImageRequest, RaytraceOptions},
};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use std::mem::size_of;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Shape,
    Box,
    FinishBox,
    Pairs,
    Sheets,
    Lens,
    Done,
}

pub(crate) struct Task {
    values: [Option<Certificate>; MAX_OVERRIDES],
    boxes: Vec<BoxTask>,
    count: usize,
    stage: Stage,
    i: usize,
    j: usize,
    pupil: Option<(Vec3, f64)>,
}

fn selected_box_capacity(scene: &ProductScene, options: &RaytraceOptions) -> Result<usize> {
    if options.dielectrics.len() > MAX_OVERRIDES {
        return Err(TraceError::InvalidInput("too many dielectric overrides"));
    }
    let mut maximum = 0;
    for input in &options.dielectrics {
        let body = scene.meshes.get(input.body_index).ok_or(TraceError::InvalidInput("dielectric body ordinal"))?;
        if body.name != input.body_name {
            return Err(TraceError::InvalidInput("dielectric captured body identity mismatch"));
        }
        if input.boundary == DielectricBoundary::ClosedMesh || input.shadow_transmission {
            return Err(TraceError::InvalidInput("general glass requires explicit CPU Path transport"));
        }
        if input.boundary == DielectricBoundary::ClosedBox {
            BoxTask::required_bytes(body.mesh.triangles.len())?;
            maximum = maximum.max(body.mesh.triangles.len());
        } else if !(1..=2).contains(&body.mesh.triangles.len()) || body.mesh.positions.len() > 36 {
            return Err(TraceError::InvalidInput("unsupported dielectric boundary tessellation"));
        }
    }
    Ok(maximum)
}

impl Task {
    pub(crate) fn required_bytes(scene: &ProductScene, options: &RaytraceOptions) -> Result<u64> {
        if options.dielectrics.is_empty() || options.path.is_some() {
            return Ok(0);
        }
        let maximum = selected_box_capacity(scene, options)?;
        let buffers = if maximum == 0 { 0 } else { BoxTask::required_bytes(maximum)? };
        buffers.checked_add(size_of::<Self>() as u64).ok_or(TraceError::MemoryLimit)
    }

    pub(super) fn new(scene: &ProductScene, request: &ImageRequest, options: &RaytraceOptions) -> Result<Self> {
        Self::required_bytes(scene, options)?;
        let maximum = selected_box_capacity(scene, options)?;
        let mut boxes = Vec::new();
        if maximum != 0 {
            boxes.try_reserve_exact(1).map_err(|_| TraceError::AllocationFailed)?;
            if boxes.capacity() != 1 {
                return Err(TraceError::MemoryLimit);
            }
            boxes.push(BoxTask::new(maximum)?);
        }
        Ok(Self {
            values: [None; MAX_OVERRIDES],
            boxes,
            count: options.dielectrics.len(),
            stage: Stage::Shape,
            i: 0,
            j: 1,
            pupil: options.lens.filter(|lens| lens.aperture_radius_mm > 0.0).map(|lens| (request.camera.eye(), lens.aperture_radius_mm)),
        })
    }

    pub(super) fn ready(&self) -> bool {
        self.stage == Stage::Done
    }
    pub(super) fn into_proof(self, work_used: u64) -> Result<super::image::CertifiedOptics> {
        if !self.ready() {
            return Err(TraceError::BuildIncomplete);
        }
        Ok(super::image::CertifiedOptics::legacy(self.values, work_used))
    }

    pub(super) fn step(&mut self, scene: &ProductScene, options: &RaytraceOptions, work: &mut WorkBudget, cancel: &CancelToken) -> Result<()> {
        if self.ready() {
            return super::check_cancel(cancel);
        }
        if self.stage == Stage::Box {
            let input = options.dielectrics.get(self.i).ok_or(TraceError::Internal("legacy box target"))?;
            let body = scene
                .meshes
                .get(input.body_index)
                .filter(|body| body.name == input.body_name)
                .ok_or(TraceError::InvalidInput("dielectric captured body identity mismatch"))?;
            if self.boxes.first_mut().ok_or(TraceError::Internal("legacy box cursor"))?.step(&body.mesh, work, cancel)? {
                self.stage = Stage::FinishBox;
            }
            return Ok(());
        }
        work.tick(cancel)?;
        match self.stage {
            Stage::Shape => {
                if let Some(input) = options.dielectrics.get(self.i) {
                    let body = scene
                        .meshes
                        .get(input.body_index)
                        .filter(|body| body.name == input.body_name)
                        .ok_or(TraceError::InvalidInput("dielectric captured body identity mismatch"))?;
                    if input.boundary == DielectricBoundary::ClosedBox {
                        self.boxes.first_mut().ok_or(TraceError::Internal("legacy box cursor"))?.reset(body.mesh.triangles.len())?;
                        self.stage = Stage::Box;
                    } else {
                        let value = dielectric::certify_sheet(&body.mesh)?;
                        *self.values.get_mut(self.i).ok_or(TraceError::Internal("legacy sheet result"))? = Some(value);
                        self.i += 1;
                    }
                } else {
                    self.i = 0;
                    self.j = 1;
                    self.stage = Stage::Pairs;
                }
            }
            Stage::FinishBox => {
                let bounds = self.boxes.first().ok_or(TraceError::Internal("legacy box result"))?.result()?;
                *self.values.get_mut(self.i).ok_or(TraceError::Internal("legacy box result slot"))? = Some(Certificate::ClosedBox(bounds));
                self.i += 1;
                self.stage = Stage::Shape;
            }
            Stage::Pairs => {
                if self.i >= self.count {
                    self.i = 0;
                    self.j = 0;
                    self.stage = Stage::Sheets;
                } else if self.j >= self.count {
                    self.i += 1;
                    self.j = self.i + 1;
                } else {
                    if let (Some(Some(Certificate::ClosedBox(a))), Some(Some(Certificate::ClosedBox(b)))) =
                        (self.values.get(self.i), self.values.get(self.j))
                        && dielectric::overlaps(*a, *b)
                    {
                        return Err(TraceError::InvalidInput("nested, touching or overlapping dielectric volumes are unsupported"));
                    }
                    self.j += 1;
                }
            }
            Stage::Sheets => {
                if self.i >= self.count {
                    self.i = 0;
                    self.stage = Stage::Lens;
                } else if self.j >= self.count {
                    self.i += 1;
                    self.j = 0;
                } else {
                    if self.i != self.j
                        && let Some(Some(Certificate::ClosedBox(a))) = self.values.get(self.i)
                        && matches!(self.values.get(self.j), Some(Some(Certificate::ThinSheet)))
                    {
                        let input = options.dielectrics.get(self.j).ok_or(TraceError::Internal("sheet override index"))?;
                        let body = scene.meshes.get(input.body_index).ok_or(TraceError::Internal("sheet body index"))?;
                        let bounds = body.mesh.bounds();
                        if dielectric::overlaps(*a, Bounds::new(bounds.min, bounds.max)?) {
                            return Err(TraceError::InvalidInput("thin sheets touching or inside dielectric volumes are unsupported"));
                        }
                    }
                    self.j += 1;
                }
            }
            Stage::Lens => {
                if let Some((eye, radius)) = self.pupil
                    && let Some(value) = self.values.get(self.i).filter(|_| self.i < self.count)
                {
                    if let Some(Certificate::ClosedBox(bounds)) = value {
                        let nearest = eye.max(bounds.min()).min(bounds.max());
                        if eye.dist(nearest) <= radius {
                            return Err(TraceError::InvalidInput("thin-lens aperture sphere touches or enters a certified dielectric volume"));
                        }
                    }
                    self.i += 1;
                } else {
                    self.stage = Stage::Done;
                }
            }
            Stage::Box => return Err(TraceError::Internal("legacy box skipped bounded cursor")),
            Stage::Done => {}
        }
        Ok(())
    }
}
