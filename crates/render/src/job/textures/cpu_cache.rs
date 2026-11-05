//! One work-owned cache for opaque CPU image maps. Every hit reuses one complete
//! mapped closure for directional, point, area and environment direct lighting.
use super::cpu_preparation::{MapPoint, MapTask, PreparedMap};
use super::{RuntimeTextures, TextureBinding, TextureMap, validate_cpu_options};
use crate::{
    ProductScene,
    job::{CancelToken, ErrorCode, JobError, RaytraceOptions, Result},
    surface_patterns::ShadingSurface,
};
use solidcraft_geom::prelude::*;
use std::sync::Arc;
fn invalid_owned(message: String) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

fn invalid(message: &'static str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn limit() -> JobError {
    JobError::new(ErrorCode::LimitExceeded, "CPU map cache exceeds admitted capacity")
}
fn reserve<T>(length: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "CPU map cache allocation failed"))?;
    if values.capacity() != length {
        return Err(limit());
    }
    Ok(values)
}
fn map(binding: &TextureBinding, role: usize) -> Option<&TextureMap> {
    match role {
        0 => binding.base_color.as_ref(),
        1 => binding.roughness.as_ref(),
        2 => binding.metallic.as_ref(),
        3 => binding.normal.as_ref(),
        _ => None,
    }
}
#[derive(Clone, Copy)]
struct Binding {
    body: usize,
    source: usize,
    maps: [Option<usize>; 4],
}
#[derive(Clone, Copy)]
struct Hit {
    binding: Option<usize>,
    point: MapPoint,
    current: ShadingSurface,
    role: usize,
    footprint: Option<[solidcore_record::Vec3; 2]>,
}
pub(crate) struct CpuTextureCache {
    resources: Arc<RuntimeTextures>,
    maps: Vec<PreparedMap>,
    bindings: Vec<Binding>,
    current: Vec<MapTask>,
    binding: usize,
    role: usize,
    scan: usize,
    ready: bool,
    hit: Option<Hit>,
    prepared_rows: u64,
    // Until a role is scanned, its source height is an upper-bound estimate.
    // Existing deduplication continuations remove reused rows without decoding
    // them or adding a constructor-wide identity scan.
    total_rows: u64,
}
impl CpuTextureCache {
    fn slots(resources: &RuntimeTextures) -> usize {
        resources.bindings().iter().map(|binding| (0..4).filter(|&role| map(binding, role).is_some()).count()).sum()
    }
    /// Actual retained source capacities plus all unique complete mip pixels; binding/map
    /// metadata is conservatively prepaid for every nonempty role, including shared
    /// roles. One sequential decoder workspace overlaps the complete cache ceiling.
    pub(crate) fn required_bytes(resources: &RuntimeTextures, scene: &ProductScene, options: &RaytraceOptions) -> Result<u64> {
        validate_cpu_options(options)?;
        resources.validate_scene(scene)?;
        if resources.bindings().iter().any(|binding| options.dielectrics.iter().any(|glass| glass.body_index == binding.body_index)) {
            return Err(invalid("CPU maps and Glass cannot share the same captured boundary"));
        }
        let mut width = 0;
        for binding in resources.bindings() {
            for role in 0..4 {
                if let Some(map) = map(binding, role) {
                    MapTask::required_bytes(map)?;
                    width = width.max(map.dimensions()[0]);
                }
            }
        }
        let slots = Self::slots(resources) as u64;
        resources
            .cpu_source_bytes()?
            .checked_add(size_of::<Self>() as u64)
            .and_then(|n| n.checked_add(resources.bindings().len() as u64 * size_of::<Binding>() as u64))
            .and_then(|n| n.checked_add(slots * (size_of::<PreparedMap>() as u64 + 11 * PreparedMap::level_metadata_bytes())))
            .and_then(|n| n.checked_add(resources.summary().linear_pixel_bytes.checked_mul(2)?))
            .and_then(|n| n.checked_add(MapTask::row_workspace_bytes(width)))
            .ok_or_else(limit)
    }
    /// The containing ImageTask admission and host lease prepay required_bytes.
    pub(crate) fn new(resources: Arc<RuntimeTextures>, cancel: &CancelToken) -> Result<Self> {
        cancel.check()?;
        let total_rows = resources.bindings().iter().try_fold(0_u64, |sum, binding| {
            (0..4).try_fold(sum, |sum, role| {
                sum.checked_add(map(binding, role).map_or(0, |source| u64::from(source.dimensions()[1]))).ok_or_else(limit)
            })
        })?;
        Ok(Self {
            maps: reserve(Self::slots(&resources))?,
            bindings: reserve(resources.bindings().len())?,
            current: reserve(1)?,
            resources,
            binding: 0,
            role: 0,
            scan: 0,
            ready: false,
            hit: None,
            prepared_rows: 0,
            total_rows,
        })
    }
    pub(crate) fn ready(&self) -> bool {
        self.ready
    }
    pub(crate) fn preparation_rows(&self) -> (u64, u64) {
        (self.prepared_rows + self.current.first().map_or(0, |current| u64::from(current.completed_rows())), self.total_rows)
    }
    /// The image worker supplies its same cumulative-work debit. Never debit a
    /// second operation after a PNG continuation or perform an image-wide scan.
    pub(crate) fn prepare_one(&mut self, cancel: &CancelToken, charge: &mut impl FnMut(u64) -> Result<()>) -> Result<bool> {
        cancel.check()?;
        if self.ready {
            return Err(invalid("CPU map cache preparation is already complete"));
        }
        if let Some(current) = self.current.first_mut() {
            if current.step(charge)? {
                self.prepared_rows =
                    self.prepared_rows.checked_add(u64::from(current.completed_rows())).filter(|rows| *rows <= self.total_rows).ok_or_else(limit)?;
                let prepared = self.current.pop().ok_or_else(|| invalid("CPU current map disappeared"))?.finish()?;
                if self.maps.len() == self.maps.capacity() {
                    return Err(limit());
                }
                let index = self.maps.len();
                self.maps.push(prepared);
                *self
                    .bindings
                    .get_mut(self.binding)
                    .and_then(|b| b.maps.get_mut(self.role))
                    .ok_or_else(|| invalid("CPU map binding cursor disappeared"))? = Some(index);
                self.role += 1;
                self.scan = 0;
            }
            return Ok(false);
        }
        charge(1)?;
        cancel.check()?;
        if self.binding == self.resources.bindings().len() {
            // At most128 fixed-size records; sort_unstable allocates no heap.
            self.bindings.sort_unstable_by_key(|b| b.body);
            self.ready = true;
            return Ok(true);
        }
        if self.bindings.len() == self.binding {
            if self.bindings.len() == self.bindings.capacity() {
                return Err(limit());
            }
            let source = self.resources.bindings().get(self.binding).ok_or_else(|| invalid("CPU source binding disappeared"))?;
            self.bindings.push(Binding { body: source.body_index, source: self.binding, maps: [None; 4] });
            return Ok(false);
        }
        if self.role == 4 {
            self.binding += 1;
            self.role = 0;
            self.scan = 0;
            return Ok(false);
        }
        let source = self.resources.bindings().get(self.binding).ok_or_else(|| invalid("CPU map source disappeared"))?;
        let Some(source) = map(source, self.role) else {
            self.role += 1;
            self.scan = 0;
            return Ok(false);
        };
        if let Some(prepared) = self.maps.get(self.scan) {
            if prepared.identity().0 == source.identity() {
                let total_rows =
                    self.total_rows.checked_sub(u64::from(source.dimensions()[1])).filter(|rows| *rows >= self.prepared_rows).ok_or_else(limit)?;
                *self
                    .bindings
                    .get_mut(self.binding)
                    .and_then(|b| b.maps.get_mut(self.role))
                    .ok_or_else(|| invalid("CPU map binding cursor disappeared"))? = Some(self.scan);
                self.total_rows = total_rows;
                self.role += 1;
                self.scan = 0;
            } else {
                self.scan += 1;
            }
        } else {
            let bytes = MapTask::required_bytes(source)?;
            self.current.push(MapTask::new(source.clone(), bytes, cancel.clone())?);
        }
        Ok(false)
    }
    pub(crate) fn reset_hit(&mut self) {
        self.hit = None;
    }
    pub(crate) fn hit_started(&self) -> bool {
        self.hit.is_some()
    }
    pub(crate) fn hit_ready(&self) -> bool {
        self.hit.is_some_and(|h| h.role == 4)
    }
    pub(crate) fn begin_hit(
        &mut self,
        input: ShadingSurface,
        body: Option<usize>,
        point: MapPoint,
        footprint: Option<[solidcore_record::Vec3; 2]>,
    ) -> Result<()> {
        if !self.ready {
            return Err(invalid("CPU surface maps are not completely prepared"));
        }
        let binding = body.and_then(|body| self.bindings.binary_search_by_key(&body, |b| b.body).ok());
        self.hit = Some(Hit { binding, point, footprint, current: input, role: if binding.is_none() { 4 } else { 0 } });
        Ok(())
    }
    /// Caller charges one cumulative work unit before this one-role operation.
    pub(crate) fn advance_hit(&mut self) -> Result<bool> {
        let hit = self.hit.as_mut().ok_or_else(|| invalid("CPU mapped hit is not initialized"))?;
        if hit.role == 4 {
            return Err(invalid("CPU mapped hit is already complete"));
        }
        let prepared = hit.binding.and_then(|i| self.bindings.get(i)).ok_or_else(|| invalid("CPU mapped body disappeared"))?;
        if let Some(index) = prepared.maps.get(hit.role).copied().flatten() {
            let binding = self.resources.bindings().get(prepared.source).ok_or_else(|| invalid("CPU mapped source disappeared"))?;
            let map = self.maps.get(index).ok_or_else(|| invalid("CPU prepared map disappeared"))?;
            let lod = match hit.footprint {
                None => map.level_count().saturating_sub(1) as f64,
                Some([dx, dy]) => {
                    let u = solidcore_record::Vec3::new(binding.frame.u[0], binding.frame.u[1], binding.frame.u[2]);
                    let v = solidcore_record::Vec3::new(binding.frame.v[0], binding.frame.v[1], binding.frame.v[2]);
                    let gradient =
                        |delta: solidcore_record::Vec3| [delta.dot(u) / u.len2(), delta.dot(v) / v.len2() * if binding.flip_v { -1.0 } else { 1.0 }];
                    let (dx, dy) = (gradient(dx), gradient(dy));
                    if dx.iter().chain(&dy).any(|value| !value.is_finite() || value.abs() > crate::texture_lod::MAX_UV_GRADIENT) {
                        map.level_count().saturating_sub(1) as f64
                    } else {
                        crate::texture_lod::select_isotropic_lod(map.dimensions(0).ok_or_else(|| invalid("CPU map base level missing"))?, dx, dy)
                            .map_err(|e| invalid_owned(e.to_string()))?
                            .lod
                    }
                }
            };
            hit.current = map.apply(binding, hit.point, hit.current, lod)?;
        }
        hit.role += 1;
        Ok(hit.role == 4)
    }
    pub(crate) fn surface(&self) -> Result<ShadingSurface> {
        let hit = self.hit.filter(|h| h.role == 4).ok_or_else(|| invalid("CPU mapped closure is incomplete"))?;
        Ok(hit.current)
    }
    #[cfg(test)]
    pub(crate) fn preparation_phase(&self) -> Option<&'static str> {
        self.current.first().map(MapTask::phase_name)
    }
    #[cfg(test)]
    pub(crate) fn prepared_maps(&self) -> usize {
        self.maps.len()
    }
    #[cfg(test)]
    pub(crate) fn hit_role(&self) -> Option<usize> {
        self.hit.map(|h| h.role)
    }
}
