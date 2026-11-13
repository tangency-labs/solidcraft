//! Borrowed transport inputs and worker-local continuations. Prepared scene data
//! is immutable while a camera batch or photon batch executes.
use super::*;

pub(super) struct TransportContext<'a> {
    pub maps: &'a mut Vec<job::textures::cpu_cache::CpuTextureCache>,
    pub aging: &'a mut Vec<super::super::aging::Task>,
    pub caustics: &'a [caustics::Task],
    pub punctual_diagnostics: &'a mut job::PunctualDiagnostics,
    pub progressive: Option<&'a Progressive>,
    pub glass: &'a [super::super::glass_mesh::Task],
    pub environment: Option<&'a crate::lighting::environment_input::EnvironmentState>,
    pub scene: &'a ProductScene,
    pub request: &'a ImageRequest,
    pub options: &'a RaytraceOptions,
    pub cancel: &'a CancelToken,
    pub lights: &'a Lighting,
    pub emissions: &'a [(usize, super::super::emission::Emission)],
    pub effects: &'a [(usize, crate::surface_patterns::PreparedEffect)],
    pub work: &'a mut WorkBudget,
    pub clip: (f64, f64),
    pub pupil_sample: Option<[f64; 2]>,
    pub looks: &'a [Look],
    pub bvh: Option<&'a Bvh>,
    pub pixel: usize,
    pub spatial: u8,
    pub primary_coverage: u32,
    pub completed: Option<Vec3>,
}

impl<'a> TransportContext<'a> {
    pub fn from_image(task: &'a mut ImageTask) -> Self {
        Self {
            maps: &mut task.maps,
            aging: &mut task.aging,
            caustics: &task.caustics,
            punctual_diagnostics: &mut task.punctual_diagnostics,
            progressive: task.progressive.as_ref(),
            glass: &task.glass,
            environment: task.environment.as_ref(),
            scene: &task.scene,
            request: &task.request,
            options: &task.options,
            cancel: &task.cancel,
            lights: &task.lights,
            emissions: &task.emissions,
            effects: &task.effects,
            work: &mut task.work,
            clip: task.clip,
            pupil_sample: task.pupil_sample,
            looks: &task.looks,
            bvh: task.bvh.as_ref(),
            pixel: task.pixel,
            spatial: task.spatial,
            primary_coverage: task.primary_coverage,
            completed: None,
        }
    }

    pub fn finish_sample(&mut self, radiance: Vec3) -> job::Result<()> {
        if self.completed.is_some()
            || self.primary_coverage > 1
            || !bounded_radiance(radiance)
            || (self.primary_coverage == 0 && radiance != Vec3::ZERO)
        {
            return Err(invalid("CPU transport produced invalid camera radiance or coverage"));
        }
        self.completed = Some(radiance);
        Ok(())
    }
}
