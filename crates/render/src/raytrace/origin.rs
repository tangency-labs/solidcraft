//! One reusable prepaid primary-origin certificate. No camera sample, RNG, image
//! plane or scene mutation: one bounds/BVH/triangle/hierarchy operation per step.
use super::{
    Bounds, WorkBudget,
    bvh::{PrimitiveScan, ScanPoll},
    glass_mesh,
};
use crate::job::{self, CancelToken, DielectricBoundary, DielectricOptions};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
fn invalid(message: &str) -> job::JobError {
    job::JobError::new(job::ErrorCode::InvalidScene, message)
}
fn contains(b: Bounds, p: Vec3) -> bool {
    let lo = b.min();
    let hi = b.max();
    p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && p.z >= lo.z && p.z <= hi.z
}
#[derive(Clone, Copy)]
enum Stage {
    Bounds,
    Faces,
    FaceResult,
    Preclip,
    PreclipScan,
    Order,
    Done,
}
pub(super) struct Task {
    origin: Vec3,
    near: Vec3,
    point: usize,
    medium: usize,
    winding: i32,
    bits: [u128; 2],
    stage: Stage,
    ordered: [usize; job::MAX_NESTED_MEDIA],
    occupied: u16,
    len: usize,
    cursor: Option<PrimitiveScan>,
}
impl Task {
    pub(super) fn required_bytes() -> u64 {
        std::mem::size_of::<Self>() as u64
    }
    /// Outward bounds are conservative. Existing outside rays pay no new work
    /// and allocate no classifier slot; actual ambiguous pupil samples decide.
    pub(super) fn required(certificate: &glass_mesh::Task, inputs: &[DielectricOptions], origin: Vec3, near: Vec3) -> job::Result<bool> {
        for (i, _) in inputs.iter().enumerate() {
            let b = certificate.region(i)?.bounds;
            let lo = origin.min(near);
            let hi = origin.max(near);
            let min = b.min();
            let max = b.max();
            if lo.x <= max.x && hi.x >= min.x && lo.y <= max.y && hi.y >= min.y && lo.z <= max.z && hi.z >= min.z {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn new(origin: Vec3, near: Vec3) -> job::Result<Self> {
        if !super::bounded(origin) || !super::bounded(near) {
            return Err(invalid("CPU camera origin exceeds the bounded world envelope"));
        }
        Ok(Self {
            origin,
            near,
            point: 0,
            medium: 0,
            winding: 0,
            bits: [0; 2],
            stage: Stage::Bounds,
            ordered: [0; job::MAX_NESTED_MEDIA],
            occupied: 0,
            len: 0,
            cursor: None,
        })
    }
    pub(super) fn ready(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }
    pub(super) fn result(&self) -> job::Result<([usize; job::MAX_NESTED_MEDIA], usize, Vec3)> {
        if !self.ready() {
            return Err(invalid("CPU camera medium certificate is incomplete"));
        }
        Ok((self.ordered, self.len, self.origin))
    }
    pub(super) fn step(
        &mut self,
        certificate: &glass_mesh::Task,
        inputs: &[DielectricOptions],
        scene: super::prepared::PreparedScene<'_>,
        work: &mut WorkBudget,
        cancel: &CancelToken,
    ) -> job::Result<()> {
        if matches!(self.stage, Stage::Faces | Stage::PreclipScan) {
            let poll = self
                .cursor
                .as_mut()
                .ok_or_else(|| invalid("CPU camera classification cursor missing"))?
                .step(scene.bvh, work, cancel)
                .map_err(super::image::error)?;
            match poll {
                ScanPoll::Pending => {}
                ScanPoll::Done => {
                    self.cursor = None;
                    self.stage = if matches!(self.stage, Stage::Faces) { Stage::FaceResult } else { Stage::Order };
                    self.medium = if matches!(self.stage, Stage::Order) { 0 } else { self.medium };
                }
                ScanPoll::Primitive(index) => {
                    let look = scene.primitives.get(index).ok_or_else(|| invalid("CPU camera classification primitive missing"))?;
                    if matches!(self.stage, Stage::Faces) {
                        let region = certificate.region(self.medium)?;
                        if (region.primitive_begin..region.primitive_end).contains(&index) {
                            let p = if self.point == 0 { self.origin } else { self.near };
                            self.winding = self
                                .winding
                                .checked_add(i32::from(glass_mesh::origin_crossing(look.vertices, p)?))
                                .ok_or_else(|| invalid("CPU camera winding overflow"))?;
                        }
                    } else if inputs.iter().any(|input| input.body_index == look.body_index)
                        && glass_mesh::origin_segment_contact(look.vertices, self.origin, self.near)?
                    {
                        return Err(invalid("CPU camera near clipping meets a Glass boundary; move the camera or pupil away from that boundary"));
                    }
                }
            }
            return Ok(());
        }
        work.tick(cancel).map_err(super::image::error)?;
        if inputs.len() > job::MAX_OPTICAL_TARGETS {
            return Err(invalid("CPU camera classification exceeds 128 media"));
        }
        match self.stage {
            Stage::Bounds => {
                let Some(input) = inputs.get(self.medium) else {
                    if self.point == 0 && self.origin != self.near {
                        self.point = 1;
                        self.medium = 0;
                        return Ok(());
                    }
                    if self.point == 0 {
                        self.bits[1] = self.bits[0];
                    }
                    if self.bits[0] != self.bits[1] {
                        return Err(invalid("CPU camera near clipping crosses a Glass boundary; move the camera or pupil away from that boundary"));
                    }
                    self.medium = 0;
                    self.stage = if self.origin == self.near { Stage::Order } else { Stage::Preclip };
                    return Ok(());
                };
                let region = certificate.region(self.medium)?;
                let p = if self.point == 0 { self.origin } else { self.near };
                if input.boundary == DielectricBoundary::ThinSheet || !contains(region.bounds, p) {
                    self.medium += 1;
                    return Ok(());
                }
                self.winding = 0;
                self.cursor = Some(PrimitiveScan::new(p, Vec3::new(region.bounds.max().x, p.y, p.z)).map_err(super::image::error)?);
                self.stage = Stage::Faces;
            }
            Stage::FaceResult => {
                if !(0..=1).contains(&self.winding) {
                    return Err(invalid("CPU camera winding disagrees with its certified closed manifold"));
                }
                if self.winding == 1 {
                    let bits = self.bits.get_mut(self.point).ok_or_else(|| invalid("CPU camera point cursor"))?;
                    *bits |= 1_u128 << self.medium;
                }
                self.medium += 1;
                self.stage = Stage::Bounds;
            }
            Stage::Preclip => {
                self.cursor = Some(PrimitiveScan::new(self.origin.min(self.near), self.origin.max(self.near)).map_err(super::image::error)?);
                self.stage = Stage::PreclipScan;
            }
            Stage::Order => {
                let Some(_) = inputs.get(self.medium) else {
                    if self.occupied.count_ones() as usize != self.len {
                        return Err(invalid("CPU camera medium hierarchy has a missing parent"));
                    }
                    self.stage = Stage::Done;
                    return Ok(());
                };
                if self.bits[0] & (1_u128 << self.medium) != 0 {
                    let mut parent = certificate.region(self.medium)?.parent;
                    let mut depth = 0;
                    for _ in 0..job::MAX_NESTED_MEDIA {
                        let Some(p) = parent else {
                            break;
                        };
                        if p >= inputs.len() || self.bits[0] & (1_u128 << p) == 0 {
                            return Err(invalid("CPU camera origin is not inside its certified medium parent"));
                        }
                        depth += 1;
                        parent = certificate.region(p)?.parent;
                    }
                    if parent.is_some() || depth >= job::MAX_NESTED_MEDIA {
                        return Err(invalid("CPU camera medium hierarchy exceeds 16 layers"));
                    }
                    let occupied = 1_u16 << depth;
                    if self.occupied & occupied != 0 {
                        return Err(invalid("CPU camera origin is inside unrelated optical regions"));
                    }
                    *self.ordered.get_mut(depth).ok_or_else(|| invalid("CPU camera ordered medium slot"))? = self.medium;
                    self.occupied |= occupied;
                    self.len = self.len.max(depth + 1);
                }
                self.medium += 1;
            }
            Stage::Done => return Err(invalid("CPU camera medium certificate already complete")),
            Stage::Faces | Stage::PreclipScan => return Err(invalid("CPU camera scan bypassed its charged continuation")),
        }
        Ok(())
    }
}
