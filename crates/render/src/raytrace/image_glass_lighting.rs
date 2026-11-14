//! Cooperative hard-source connections at physical optical and HG vertices.
//! Finite emitters and environment endpoints retain their BSDF-only technique.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InterfaceKind {
    Closed,
    SheetExternal,
    SheetInternal,
}
#[derive(Clone, Copy)]
pub(super) struct Interface {
    pub index: usize,
    pub position: Vec3,
    pub canonical: Vec3,
    pub normal: Vec3,
    pub geometric: Vec3,
    pub incident: Vec3,
    pub ni: f64,
    pub nt: f64,
    pub next_medium: Option<usize>,
    pub wavelength_nm: f64,
    pub kind: InterfaceKind,
}
#[derive(Clone, Copy)]
pub(super) enum Event {
    Interface(Interface),
    Medium { index: usize, position: Vec3, incoming: Vec3 },
}
impl Event {
    fn position(self) -> Vec3 {
        match self {
            Self::Interface(event) => event.position,
            Self::Medium { position, .. } => position,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Gather,
    Directional,
    Punctual,
    Trace,
    Resolve,
    Select,
    Emitters,
    Add,
    Done,
}
pub(super) struct Context {
    event: Event,
    phase: Phase,
    after_shadow: Phase,
    light: usize,
    pending: Vec3,
    medium: Option<usize>,
    from: Vec3,
    endpoint: Option<Vec3>,
    visibility: Option<Ray>,
    hit: Option<super::super::super::Hit>,
    selector: usize,
    selected: Option<usize>,
    interfaces: u8,
    emitter: usize,
}
#[cfg(test)]
impl Context {
    pub(super) fn pending_probe(&self) -> Option<(u8, Vec3)> {
        (self.phase == Phase::Add).then_some({
            (
                match self.event {
                    Event::Interface(event) => match event.kind {
                        InterfaceKind::Closed => 1,
                        InterfaceKind::SheetExternal => 2,
                        InterfaceKind::SheetInternal => 3,
                    },
                    Event::Medium { .. } => 4,
                },
                self.pending,
            )
        })
    }
}
fn model_error(error: impl std::fmt::Display) -> JobError {
    JobError::new(ErrorCode::InvalidScene, error.to_string())
}
pub(super) fn begin(task: &TransportContext<'_>, state: &mut State, event: Event) -> bool {
    if !vertex_lighting(task, state)
        || (task.caustics.is_empty() && task.lights.lights().is_empty() && task.options.punctual_lights.as_ref().is_none_or(Vec::is_empty))
    {
        return false;
    }
    state.caustic_gather = Default::default();
    state.glass_lighting = Some(Context {
        event,
        phase: if task.caustics.is_empty() { Phase::Directional } else { Phase::Gather },
        after_shadow: Phase::Directional,
        light: 0,
        pending: Vec3::ZERO,
        medium: None,
        from: event.position(),
        endpoint: None,
        visibility: None,
        hit: None,
        selector: 0,
        selected: None,
        interfaces: 0,
        emitter: 0,
    });
    state.stage = Stage::GlassLighting;
    true
}
fn scattering(task: &TransportContext<'_>, index: usize) -> job::Result<crate::transmission::subsurface::Subsurface> {
    task.options
        .dielectrics
        .get(index)
        .and_then(|input| input.subsurface)
        .filter(|scattering| scattering.active())
        .ok_or_else(|| invalid("CPU hard-source HG receiver lost its medium"))
}
fn gather(task: &TransportContext<'_>, state: &mut State, event: Event) -> job::Result<Option<Vec3>> {
    match event {
        Event::Interface(event) => {
            let input = task.options.dielectrics.get(event.index).ok_or_else(|| invalid("CPU rough photon receiver disappeared"))?;
            let outward = if event.kind == InterfaceKind::SheetInternal { -event.geometric } else { event.geometric };
            let side = if event.kind == InterfaceKind::Closed {
                0
            } else if outward.dot(event.canonical) > 0.0 {
                1
            } else {
                -1
            };
            super::super::caustics::gather_rough(
                task,
                &mut state.caustic_gather,
                super::super::caustics::RoughReceiver {
                    position: event.position,
                    canonical: event.canonical,
                    geometric: event.geometric,
                    normal: event.normal,
                    incident: event.incident,
                    body: input.body_index,
                    side,
                    model: input.model.unwrap_or_default(),
                    ni: event.ni,
                    nt: event.nt,
                    wavelength_nm: event.wavelength_nm,
                },
            )
        }
        Event::Medium { index, position, incoming } => {
            let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU HG photon receiver disappeared"))?;
            super::super::caustics::gather_volume(task, &mut state.caustic_gather, position, input.body_index, incoming, scattering(task, index)?)
        }
    }
}
fn response(task: &TransportContext<'_>, state: &State, event: Event, direction: Vec3) -> job::Result<(f64, Option<usize>)> {
    match event {
        Event::Interface(event) => {
            let geometric_cosine = direction.dot(event.geometric);
            if direction.dot(event.normal) * geometric_cosine <= 0.0
                || (event.kind == InterfaceKind::SheetExternal && geometric_cosine <= 0.0)
                || (event.kind == InterfaceKind::SheetInternal && geometric_cosine >= 0.0)
            {
                return Ok((0.0, None));
            }
            let input = task.options.dielectrics.get(event.index).ok_or_else(|| invalid("CPU hard-source glass material disappeared"))?;
            let value = input
                .model
                .unwrap_or_default()
                .evaluate_projected(event.incident, direction, event.normal, event.ni, event.nt, event.wavelength_nm)
                .map_err(model_error)?
                .value;
            let medium = if event.kind != InterfaceKind::Closed {
                None
            } else if geometric_cosine > 0.0 {
                current(state.optical.as_ref().ok_or_else(|| invalid("CPU hard-source optical stack missing"))?)?
            } else {
                event.next_medium
            };
            Ok((value, medium))
        }
        Event::Medium { index, incoming, .. } => {
            Ok((scattering(task, index)?.phase(incoming.dot(direction).clamp(-1.0, 1.0)).map_err(model_error)?, Some(index)))
        }
    }
}
fn cursor(task: &TransportContext<'_>, state: &State, event: Event, ray: Ray) -> job::Result<TraceCursor> {
    match event {
        Event::Interface(event) => {
            let region = task.glass.first().ok_or_else(|| invalid("CPU hard-source optical certificate missing"))?.region(event.index)?;
            Ok(TraceCursor::from_plane(ray, region.primitive_begin..region.primitive_end, unmapped(task, state)?.vertices))
        }
        Event::Medium { .. } => Ok(TraceCursor::new(ray, false)),
    }
}
fn queue(
    task: &TransportContext<'_>,
    state: &mut State,
    context: &mut Context,
    direction: Vec3,
    radiance: Vec3,
    endpoint: Option<Vec3>,
    next: Phase,
) -> job::Result<()> {
    let (value, medium) = response(task, state, context.event, direction)?;
    context.pending = multiply(state.throughput, radiance) * value;
    super::super::super::lighting::validate_radiance(context.pending).map_err(error)?;
    context.after_shadow = next;
    if context.pending == Vec3::ZERO {
        context.phase = next;
        return Ok(());
    }
    let position = context.event.position();
    let (direction, far) = if let Some(endpoint) = endpoint {
        let delta = endpoint - position;
        let distance = delta.len();
        if !distance.is_finite() || distance.next_down() <= 0.0_f64.next_up() || delta.dot(direction) <= 0.0 {
            return Err(invalid("CPU hard-source finite physical shadow collapsed"));
        }
        (delta, distance.next_down())
    } else {
        (direction, super::super::super::MAX_DISTANCE_MM)
    };
    let ray = Ray::new(position, direction, 0.0_f64.next_up(), far).map_err(error)?;
    state.cursor = Some(cursor(task, state, context.event, ray)?);
    state.shadow_ray = Some(ray);
    context.medium = medium;
    context.from = position;
    context.endpoint = endpoint;
    context.visibility = Some(ray);
    context.hit = None;
    context.interfaces = 0;
    context.emitter = 0;
    context.phase = Phase::Trace;
    Ok(())
}
fn attenuate(task: &TransportContext<'_>, context: &mut Context, to: Vec3) -> job::Result<()> {
    if let Some(index) = context.medium {
        let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU hard-source shadow medium disappeared"))?;
        let distance = context.from.dist(to);
        context.pending = multiply(context.pending, dielectric::attenuation(input.absorption_per_mm, distance).map_err(error)?);
        if let Some(scattering) = input.subsurface.filter(|scattering| scattering.active()) {
            context.pending = context.pending * scattering.survival(distance).map_err(model_error)?;
        }
    }
    context.from = to;
    Ok(())
}
fn advance(task: &mut TransportContext<'_>, state: &mut State, context: &mut Context) -> job::Result<()> {
    if context.phase == Phase::Trace {
        let looks = &task.looks;
        let inputs = &task.options.dielectrics;
        let result = state
            .cursor
            .as_mut()
            .ok_or_else(|| invalid("CPU hard-source shadow cursor missing"))?
            .step_ordered(task.bvh.as_ref().ok_or_else(|| invalid("CPU hard-source shadow BVH missing"))?, task.work, task.cancel, |new, old| {
                let optical = |hit: super::super::super::Hit| {
                    looks.get(hit.primitive_index).is_some_and(|look| inputs.iter().any(|input| input.body_index == look.body_index))
                };
                match (optical(new), optical(old)) {
                    (false, true) => true,
                    (true, false) => false,
                    _ => new.primitive_index < old.primitive_index,
                }
            })
            .map_err(error)?;
        if let Some(hit) = result {
            state.cursor = None;
            context.hit = hit;
            context.phase = Phase::Resolve;
        }
        return Ok(());
    }
    task.work.tick(task.cancel).map_err(error)?;
    match context.phase {
        Phase::Gather => {
            if let Some(value) = gather(task, state, context.event)? {
                state.radiance += multiply(state.throughput, value);
                super::super::super::lighting::validate_radiance(state.radiance).map_err(error)?;
            } else {
                context.phase = Phase::Directional;
            }
        }
        Phase::Directional => {
            if let Some(light) = task.lights.lights().get(context.light) {
                context.light += 1;
                queue(task, state, context, light.direction(), light.color() * light.intensity(), None, Phase::Directional)?;
            } else {
                context.light = 0;
                context.phase = Phase::Punctual;
            }
        }
        Phase::Punctual => {
            if let Some(input) = task.options.punctual_lights.as_ref().and_then(|lights| lights.get(context.light)) {
                context.light += 1;
                let sample = input.light().map_err(error)?.sample(context.event.position()).map_err(error)?;
                if let Some(why) = sample.omitted {
                    match why {
                        super::super::super::punctual::OmittedSample::NearSource => {
                            task.punctual_diagnostics.near_source = task.punctual_diagnostics.near_source.saturating_add(1);
                        }
                        super::super::super::punctual::OmittedSample::RadianceLimit => {
                            task.punctual_diagnostics.radiance_limit = task.punctual_diagnostics.radiance_limit.saturating_add(1);
                        }
                    }
                } else {
                    queue(
                        task,
                        state,
                        context,
                        sample.direction,
                        sample.incident_linear_srgb,
                        Some(Vec3::new(input.position_mm[0], input.position_mm[1], input.position_mm[2])),
                        Phase::Punctual,
                    )?;
                }
            } else {
                context.phase = Phase::Done;
            }
        }
        Phase::Resolve => {
            if let Some(raw) = context.hit {
                let ray = state.shadow_ray.ok_or_else(|| invalid("CPU hard-source visibility ray missing"))?;
                let hit = PreparedScene {
                    bvh: task.bvh.as_ref().ok_or_else(|| invalid("CPU hard-source visibility BVH missing"))?,
                    primitives: task.looks,
                    source: task.scene,
                }
                .resolve_hit(ray, raw)
                .map_err(error)?;
                attenuate(task, context, hit.position)?;
                context.selector = 0;
                context.selected = None;
                context.phase = Phase::Select;
            } else {
                if let Some(endpoint) = context.endpoint {
                    attenuate(task, context, endpoint)?;
                } else if context.medium.is_some() {
                    return Err(invalid("CPU hard-source shadow escaped its certified current medium"));
                }
                context.phase = Phase::Emitters;
            }
        }
        Phase::Select => {
            let ray = state.shadow_ray.ok_or_else(|| invalid("CPU hard-source visibility ray missing"))?;
            let hit = PreparedScene {
                bvh: task.bvh.as_ref().ok_or_else(|| invalid("CPU hard-source visibility BVH missing"))?,
                primitives: task.looks,
                source: task.scene,
            }
            .resolve_hit(ray, context.hit.ok_or_else(|| invalid("CPU hard-source visibility hit missing"))?)
            .map_err(error)?;
            if let Some(input) = task.options.dielectrics.get(context.selector) {
                if input.body_index == hit.body_index && input.body_name == hit.body_name {
                    context.selected = Some(context.selector);
                    context.selector = task.options.dielectrics.len();
                } else {
                    context.selector += 1;
                }
                return Ok(());
            }
            if let Some(index) = context.selected {
                let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU hard-source sheet material disappeared"))?;
                if input.boundary == job::DielectricBoundary::ThinSheet
                    && input.shadow_transmission
                    && input.model.unwrap_or_default().roughness == 0.0
                {
                    let (_, _, transmission) = dielectric::modeled_sheet(ray.direction(), hit.oriented_geometric_normal(), input).map_err(error)?;
                    context.pending = multiply(context.pending, transmission);
                    context.interfaces = context
                        .interfaces
                        .checked_add(1)
                        .filter(|count| *count <= 16)
                        .ok_or_else(|| invalid("CPU hard-source shadow exceeds sixteen thin interfaces"))?;
                    if context.pending != Vec3::ZERO {
                        let (next_ray, next_cursor) = interface_cursor(task, index, hit, ray.direction(), context.endpoint, false)?;
                        state.shadow_ray = Some(next_ray);
                        state.cursor = Some(next_cursor);
                        context.phase = Phase::Trace;
                        return Ok(());
                    }
                }
            }
            // A delta-source connection cannot bend through another closed
            // interface. Those missing source paths belong to the photon map.
            context.pending = Vec3::ZERO;
            context.phase = Phase::Add;
        }
        Phase::Emitters => {
            if let Some(input) = task.options.area_lights.as_ref().and_then(|lights| lights.get(context.emitter)) {
                context.emitter += 1;
                let rectangle = Rectangle::new(input).map_err(error)?;
                if !rectangle.on_plane(context.event.position())
                    && rectangle
                        .intersect(context.visibility.ok_or_else(|| invalid("CPU hard-source analytic visibility ray missing"))?)
                        .map_err(error)?
                        .is_some()
                {
                    context.pending = Vec3::ZERO;
                    context.phase = Phase::Add;
                }
            } else {
                context.phase = Phase::Add;
            }
        }
        Phase::Add => {
            state.radiance += context.pending;
            super::super::super::lighting::validate_radiance(state.radiance).map_err(error)?;
            state.shadow_ray = None;
            context.phase = context.after_shadow;
        }
        Phase::Done => {}
        Phase::Trace => return Err(invalid("CPU hard-source shadow bypassed its cooperative cursor")),
    }
    Ok(())
}
pub(super) fn step(task: &mut TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let mut context = state.glass_lighting.take().ok_or_else(|| invalid("CPU hard-source vertex context missing"))?;
    let result = advance(task, state, &mut context);
    if result.is_ok() && context.phase == Phase::Done {
        return match context.event {
            Event::Interface(event) if event.kind == InterfaceKind::SheetInternal => advanced::resume_sheet(task, state, event),
            Event::Interface(event) => advanced::resume_surface(task, state, event),
            Event::Medium { index, position, incoming } => scattering::resume(task, state, index, position, incoming),
        };
    }
    state.glass_lighting = Some(context);
    result
}
