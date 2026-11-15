//! Analog elastic collisions in the existing single finite dielectric/GI path.
//! No light connection through a refracting boundary, ray tree or image plane.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Collision {
    position: Vec3,
    distance: f64,
}

pub(super) fn enabled(task: &TransportContext<'_>) -> bool {
    task.options.dielectrics.iter().any(|input| input.subsurface.is_some_and(|s| s.active()))
}
pub(super) fn active(task: &TransportContext<'_>, state: &State) -> job::Result<bool> {
    let Some(optical) = &state.optical else { return Ok(false) };
    let Some(index) = current(optical)? else { return Ok(false) };
    Ok(task
        .options
        .dielectrics
        .get(index)
        .ok_or_else(|| invalid("CPU subsurface medium disappeared"))?
        .subsurface
        .is_some_and(|scattering| scattering.active()))
}

pub(super) fn step(task: &mut TransportContext<'_>, state: &mut State) -> job::Result<()> {
    match state.stage {
        Stage::Medium => decide(task, state),
        Stage::MediumScatter => scatter(task, state),
        Stage::MediumSurface => surface_range(task, state),
        _ => Err(invalid("CPU subsurface bypassed its charged phase")),
    }
}
fn decide(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU subsurface stack missing"))?;
    let index = current(optical)?.ok_or_else(|| invalid("CPU subsurface cannot scatter in vacuum"))?;
    let scattering = task
        .options
        .dielectrics
        .get(index)
        .and_then(|input| input.subsurface)
        .filter(|s| s.active())
        .ok_or_else(|| invalid("CPU subsurface rate disappeared"))?;
    let ray = state.ray.ok_or_else(|| invalid("CPU subsurface incident ray missing"))?;
    let geometry = if let Some(hit) = state.hit {
        let bvh = task.bvh.as_ref().ok_or_else(|| invalid("CPU subsurface BVH missing"))?;
        Some(PreparedScene { bvh, primitives: task.looks, source: task.scene }.resolve_hit(ray, hit).map_err(error)?)
    } else {
        None
    };
    // Emitters is already the nearest-endpoint selection. An absent endpoint
    // inside a certified closed medium is a failed boundary, not infinite fog.
    let endpoint = if let Some((_, _, point)) = state.emitter {
        point
    } else {
        geometry.as_ref().ok_or_else(|| invalid("CPU subsurface escaped its certified closed boundary"))?.position
    };
    let from = optical.segment_start;
    let distance = from.dist(endpoint);
    if !distance.is_finite() || distance <= 0.0 {
        return Err(invalid("CPU subsurface physical segment is not positive and finite"));
    }
    let [uniform, _] = variates(task, state, 0x5000)?;
    let travel = scattering.free_flight(uniform).map_err(|e| JobError::new(ErrorCode::InvalidScene, e.to_string()))?;
    if travel >= distance {
        // Analog survival is already sampled, so throughput is not attenuated
        // again. Its probability belongs in the competing endpoint MIS density.
        state.previous_pdf *= scattering.survival(distance).map_err(|e| JobError::new(ErrorCode::InvalidScene, e.to_string()))?;
        state.stage = Stage::Resolve;
        return Ok(());
    }
    let position = from + ray.direction() * travel;
    if position == from || position == endpoint {
        return Err(invalid("CPU subsurface collision is not representable between its physical endpoints; change the scene scale explicitly"));
    }
    for plane in [optical.source_plane, geometry.as_ref().map(|hit| hit.vertices)].into_iter().flatten() {
        if super::super::super::glass_mesh::origin_on_plane(plane, position)? {
            return Err(invalid("CPU subsurface collision rounds onto a source or endpoint plane; change the scene scale explicitly"));
        }
    }
    if let Some((index, _, _)) = state.emitter {
        let input =
            task.options.area_lights.as_ref().and_then(|lights| lights.get(index)).ok_or_else(|| invalid("CPU subsurface emitter missing"))?;
        if Rectangle::new(input).map_err(error)?.on_plane(position) {
            return Err(invalid("CPU subsurface collision rounds onto its analytic emitter plane; change the scene scale explicitly"));
        }
    }
    // Reuse the already prepaid one-element origin slot. Exact classification
    // catches boundary rounding and verifies the current nested chain before a
    // collision may launch any new direction. Each scan operation yields.
    state.origins.clear();
    if state.origins.capacity() == 0 {
        state.origins.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
    }
    if state.origins.capacity() != 1 {
        return Err(error(TraceError::MemoryLimit));
    }
    state.origins.push(super::super::super::origin::Task::new(position, position)?);
    state.optical.as_mut().ok_or_else(|| invalid("CPU subsurface pending stack missing"))?.collision = Some(Collision { position, distance: travel });
    state.stage = Stage::MediumOrigin;
    Ok(())
}
fn scatter(task: &mut TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU subsurface stack missing"))?;
    let event = optical.collision.ok_or_else(|| invalid("CPU subsurface collision missing"))?;
    let index = current(optical)?.ok_or_else(|| invalid("CPU subsurface collision lost its medium"))?;
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU subsurface material missing"))?;
    let factor = dielectric::attenuation(input.absorption_per_mm, event.distance).map_err(error)?;
    let throughput = multiply(state.throughput, factor);
    super::super::super::lighting::validate_radiance(throughput).map_err(error)?;
    state.throughput = throughput;
    state.optical.as_mut().ok_or_else(|| invalid("CPU subsurface stack missing"))?.collision = None;
    // A real collision is foreground even when the eye starts inside, before
    // any model/emitter hit. The depth limit counts this collision as a vertex.
    if state.depth == 0 {
        task.primary_coverage = 1;
    }
    if throughput == Vec3::ZERO {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let incoming = state.ray.ok_or_else(|| invalid("CPU subsurface incident ray missing"))?.direction();
    if glass_lighting::begin(task, state, glass_lighting::Event::Medium { index, position: event.position, incoming }) {
        return Ok(());
    }
    resume(task, state, index, event.position, incoming)
}
pub(super) fn resume(task: &TransportContext<'_>, state: &mut State, index: usize, position: Vec3, incoming: Vec3) -> job::Result<()> {
    if !continuing(task, state)? {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let scattering = task
        .options
        .dielectrics
        .get(index)
        .and_then(|input| input.subsurface)
        .filter(|scattering| scattering.active())
        .ok_or_else(|| invalid("CPU subsurface rate missing"))?;
    let uniform = variates(task, state, 0x5001)?;
    let direction = scattering.sample_direction(incoming, uniform).map_err(|e| JobError::new(ErrorCode::InvalidScene, e.to_string()))?;
    let ray = Ray::new(position, direction, 0.0_f64.next_up(), super::super::super::MAX_DISTANCE_MM).map_err(error)?;
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU subsurface stack missing"))?;
    optical.segment_start = position;
    optical.source_plane = None;
    optical.delta = true;
    state.previous_pdf = 0.0;
    state.previous_position = position;
    state.sampled_continuous = true;
    state.ray = Some(ray);
    state.cursor = Some(TraceCursor::new(ray, false));
    state.hit = None;
    state.emitter = None;
    state.depth += 1;
    state.stage = Stage::Trace;
    Ok(())
}

// Captured preparation keeps each body's primitives contiguous. Walk that range
// cooperatively instead of an uncharged all-body prefix scan or another array.
fn surface_range(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let hit = state.hit.ok_or_else(|| invalid("CPU physical surface hit missing"))?;
    let body = task.looks.get(hit.primitive_index).ok_or_else(|| invalid("CPU physical surface primitive missing"))?.body_index;
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU physical surface state missing"))?;
    let range = optical.surface_range.as_mut().ok_or_else(|| invalid("CPU physical surface range missing"))?;
    if !optical.surface_forward {
        if range[0] > 0 && task.looks.get(range[0] - 1).is_some_and(|primitive| primitive.body_index == body) {
            range[0] -= 1;
        } else {
            optical.surface_forward = true;
        }
    } else if task.looks.get(range[1]).is_some_and(|primitive| primitive.body_index == body) {
        range[1] = range[1].checked_add(1).ok_or_else(|| invalid("CPU physical surface range overflow"))?;
    } else {
        state.stage = Stage::Effects;
    }
    Ok(())
}
pub(super) fn surface_cursor(state: &State, hit: SharedHit<'_>, direction: Vec3, far: f64) -> job::Result<(Ray, TraceCursor)> {
    let range = state.optical.as_ref().and_then(|o| o.surface_range).ok_or_else(|| invalid("CPU physical surface cursor range missing"))?;
    let ray = Ray::new(hit.position, direction, 0.0_f64.next_up(), far).map_err(error)?;
    Ok((ray, TraceCursor::from_plane(ray, range[0]..range[1], hit.vertices)))
}
pub(super) fn needs_shadow_setup(state: &State) -> bool {
    state.optical.as_ref().is_some_and(|o| o.surface_range.is_some() && !o.shadow_exact)
}
pub(super) fn shadow_setup(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let hit = unmapped(task, state)?;
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU physical shadow state missing"))?;
    let old = optical.shadow_current.ok_or_else(|| invalid("CPU physical shadow ray missing"))?;
    let (direction, far) = if let Some(endpoint) = optical.shadow_endpoint {
        let direction = endpoint - hit.position;
        let distance = direction.len();
        let far = if optical.shadow_inclusive { distance } else { distance.next_down() };
        if direction.dot(old.direction()) <= 0.0 || !far.is_finite() || far <= 0.0_f64.next_up() {
            return Err(invalid("CPU physical shadow endpoint collapsed"));
        }
        (direction, far)
    } else {
        (old.direction(), old.interval()[1])
    };
    let (ray, cursor) = surface_cursor(state, hit, direction, far)?;
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU physical shadow state missing"))?;
    optical.shadow_current = Some(ray);
    optical.shadow_exact = true;
    state.shadow_ray = Some(ray);
    state.cursor = Some(cursor);
    Ok(())
}
