//! One bounded homogeneous-medium continuation on the existing finite path slot.
//! No ray tree, image plane or change to model/camera/framing bounds.
use super::*;

#[derive(Default)]
pub(super) struct Context {
    pub source: Vec3,
    source_plane: Option<[Vec3; 3]>,
    /// Incoming propagation direction while evaluating a collision vertex.
    pub event: Option<Vec3>,
    range: Option<[usize; 2]>,
    forward: bool,
    pub shadow_mis: Option<[f64; 2]>,
    pub shadow_probability: f64,
}
pub(super) fn enabled(task: &TransportContext<'_>) -> bool {
    task.options.volume.as_ref().is_some_and(|v| v.active())
}
fn medium(task: &TransportContext<'_>) -> job::Result<job::volume::HomogeneousVolume> {
    task.options.volume.as_ref().copied().filter(|v| v.active()).ok_or_else(|| invalid("CPU Atmosphere disappeared"))
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn segment(v: job::volume::HomogeneousVolume, point: Vec3, direction: Vec3, limit: f64) -> job::Result<[f64; 2]> {
    let result = v.segment(array(point), array(direction), limit)?;
    if limit == super::super::super::MAX_DISTANCE_MM {
        let mut enter = f64::NEG_INFINITY;
        let mut leave = f64::INFINITY;
        for (axis, (p, d)) in [(point.x, direction.x), (point.y, direction.y), (point.z, direction.z)].into_iter().enumerate() {
            if d == 0.0 {
                if p < v.min_mm[axis] || p > v.max_mm[axis] {
                    return Ok(result);
                }
            } else {
                let a = (v.min_mm[axis] - p) / d;
                let b = (v.max_mm[axis] - p) / d;
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
            }
        }
        if enter < leave && leave > limit && leave > 0.0 {
            return Err(invalid("CPU Atmosphere extends beyond the admitted ray distance; move/resize it explicitly"));
        }
    }
    Ok(result)
}
fn context(state: &State) -> job::Result<&Context> {
    state.volume.as_ref().ok_or_else(|| invalid("CPU Atmosphere context missing"))
}
fn context_mut(state: &mut State) -> job::Result<&mut Context> {
    state.volume.as_mut().ok_or_else(|| invalid("CPU Atmosphere context missing"))
}

/// Keep the model's original near interval. Perspective optics start at the
/// physical eye/pupil. Orthographic optics start before the union of its original
/// near plane and the box's signed line entry, without changing that pixel line.
pub(super) fn primary(task: &TransportContext<'_>, ray: Ray, physical: Option<Vec3>) -> job::Result<Ray> {
    let v = medium(task)?;
    let direction = ray.direction();
    let (source, near) = if let Some(eye) = physical {
        let near = (ray.origin() - eye).dot(direction);
        if !near.is_finite() || near < 0.0 {
            return Err(invalid("CPU Atmosphere physical near interval is invalid"));
        }
        (eye, near)
    } else {
        let mut enter = f64::NEG_INFINITY;
        let mut leave = f64::INFINITY;
        let origin = array(ray.origin());
        let d = array(direction);
        for axis in 0..3 {
            if d[axis] == 0.0 {
                if origin[axis] < v.min_mm[axis] || origin[axis] > v.max_mm[axis] {
                    return Ray::new(ray.origin(), direction, ray.interval()[0], super::super::super::MAX_DISTANCE_MM).map_err(error);
                }
            } else {
                let a = (v.min_mm[axis] - origin[axis]) / d[axis];
                let b = (v.max_mm[axis] - origin[axis]) / d[axis];
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
            }
        }
        let shift = if enter < leave { enter.min(0.0) } else { 0.0 };
        (ray.origin() + direction * shift, ray.interval()[0] - shift)
    };
    Ray::new(source, direction, near, super::super::super::MAX_DISTANCE_MM).map_err(error)
}
pub(super) fn begin(task: &TransportContext<'_>, state: &mut State, ray: Ray) -> job::Result<()> {
    medium(task)?;
    state.volume = Some(Context { source: ray.origin(), ..Default::default() });
    Ok(())
}
pub(super) fn reset_hit(state: &mut State) {
    if let Some(v) = &mut state.volume {
        v.range = None;
    }
}
pub(super) fn surface(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let hit = unmapped(task, state)?;
    let v = context_mut(state)?;
    v.source = hit.position;
    v.source_plane = Some(hit.vertices);
    v.event = None;
    v.range = Some([hit.primitive_index, hit.primitive_index.checked_add(1).ok_or_else(|| invalid("CPU Atmosphere source range overflow"))?]);
    v.forward = false;
    state.stage = Stage::VolumeSurface;
    Ok(())
}
pub(super) fn source(task: &TransportContext<'_>, state: &State) -> job::Result<Vec3> {
    if let Some(v) = &state.volume { Ok(v.source) } else { Ok(shared(task, state)?.position) }
}
pub(super) fn ray(task: &TransportContext<'_>, state: &State, direction: Vec3, endpoint: Option<Vec3>, inclusive: bool) -> job::Result<Option<Ray>> {
    if let Some(v) = &state.volume {
        let (direction, far) = if let Some(point) = endpoint {
            let delta = point - v.source;
            let length = delta.len();
            let far = if inclusive { length } else { length.next_down() };
            if !far.is_finite() || far <= 0.0_f64.next_up() || delta.dot(direction) <= 0.0 {
                return Ok(None);
            }
            (delta, far)
        } else {
            (direction, super::super::super::MAX_DISTANCE_MM)
        };
        Ray::new(v.source, direction, 0.0_f64.next_up(), far).map(Some).map_err(error)
    } else if let Some(point) = endpoint {
        bounded_shadow(shared(task, state)?, point, direction)
    } else {
        shared(task, state)?.spawn(direction).map(Some).map_err(error)
    }
}
pub(super) fn cursor(task: &TransportContext<'_>, state: &State, ray: Ray) -> job::Result<TraceCursor> {
    let v = context(state)?;
    if let Some(range) = v.range {
        let plane = v.source_plane.ok_or_else(|| invalid("CPU Atmosphere source plane missing"))?;
        // The same producing body's exact plane is excluded, not an offset path.
        if range[1] > task.looks.len() {
            return Err(invalid("CPU Atmosphere source range changed"));
        }
        Ok(TraceCursor::from_plane(ray, range[0]..range[1], plane))
    } else {
        Ok(TraceCursor::new(ray, false))
    }
}
pub(super) fn response(task: &TransportContext<'_>, state: &State, direction: Vec3) -> job::Result<Option<f64>> {
    context(state)?.event.map(|incoming| medium(task)?.phase(incoming.dot(direction).clamp(-1.0, 1.0))).transpose()
}
pub(super) fn step(task: &mut TransportContext<'_>, state: &mut State) -> job::Result<()> {
    match state.stage {
        Stage::Volume => decide(task, state),
        Stage::VolumeSurface => range(task, state),
        Stage::VolumeShadow => shadow(task, state),
        _ => Err(invalid("CPU Atmosphere bypassed its charged phase")),
    }
}
fn decide(task: &mut TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let v = medium(task)?;
    let c = context(state)?;
    let ray = state.ray.ok_or_else(|| invalid("CPU Atmosphere incident ray missing"))?;
    let endpoint = if let Some((_, _, point)) = state.emitter {
        Some(point)
    } else if state.hit.is_some() {
        Some(unmapped(task, state)?.position)
    } else {
        None
    };
    let limit = endpoint.map_or(super::super::super::MAX_DISTANCE_MM, |p| c.source.dist(p));
    let [near, far] = segment(v, c.source, ray.direction(), limit)?;
    if near >= far {
        state.stage = Stage::Resolve;
        return Ok(());
    }
    let [u, _] = variates(task, state, 0x6000)?;
    let u = if u == 0.0 { 2.0_f64.powi(-54) } else { u };
    let travel = -(-u).ln_1p() / v.extinction_per_mm;
    if travel >= far - near {
        state.previous_pdf *= v.transmittance(far - near)?;
        state.stage = Stage::Resolve;
        return Ok(());
    }
    let point = c.source + ray.direction() * (near + travel);
    if point == c.source || endpoint == Some(point) || !array(point).into_iter().enumerate().all(|(i, x)| x > v.min_mm[i] && x < v.max_mm[i]) {
        return Err(invalid("CPU Atmosphere collision is not representable strictly inside its box; change scale or density explicitly"));
    }
    for plane in [c.source_plane, state.hit.map(|_| unmapped(task, state)).transpose()?.map(|h| h.vertices)].into_iter().flatten() {
        if super::super::super::glass_mesh::origin_on_plane(plane, point)? {
            return Err(invalid("CPU Atmosphere collision rounds onto a source/endpoint plane"));
        }
    }
    if let Some((i, _, _)) = state.emitter {
        let light = task.options.area_lights.as_ref().and_then(|l| l.get(i)).ok_or_else(|| invalid("CPU Atmosphere emitter disappeared"))?;
        if Rectangle::new(light).map_err(error)?.on_plane(point) {
            return Err(invalid("CPU Atmosphere collision rounds onto its emitter plane"));
        }
    }
    state.throughput = multiply(state.throughput, Vec3::new(v.scattering_albedo[0], v.scattering_albedo[1], v.scattering_albedo[2]));
    super::super::super::lighting::validate_radiance(state.throughput).map_err(error)?;
    if state.depth == 0 {
        task.primary_coverage = 1;
    }
    let c = context_mut(state)?;
    c.source = point;
    c.source_plane = None;
    c.range = None;
    c.event = Some(ray.direction());
    if state.throughput == Vec3::ZERO {
        state.stage = Stage::Commit;
        return Ok(());
    }
    state.light = 0;
    state.sample = 0;
    state.stage = Stage::Directional;
    Ok(())
}
fn range(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let hit = state.hit.ok_or_else(|| invalid("CPU Atmosphere source hit missing"))?;
    let body = task.looks.get(hit.primitive_index).ok_or_else(|| invalid("CPU Atmosphere source primitive missing"))?.body_index;
    let c = context_mut(state)?;
    let r = c.range.as_mut().ok_or_else(|| invalid("CPU Atmosphere source range missing"))?;
    if !c.forward {
        if r[0] > 0 && task.looks.get(r[0] - 1).is_some_and(|p| p.body_index == body) {
            r[0] -= 1;
        } else {
            c.forward = true;
        }
    } else if task.looks.get(r[1]).is_some_and(|p| p.body_index == body) {
        r[1] = r[1].checked_add(1).ok_or_else(|| invalid("CPU Atmosphere source range overflow"))?;
    } else {
        state.stage = Stage::Effects;
    }
    Ok(())
}
fn shadow(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let ray = state.shadow_ray.ok_or_else(|| invalid("CPU Atmosphere visibility ray missing"))?;
    let v = medium(task)?;
    let [a, b] = segment(v, ray.origin(), ray.direction(), ray.interval()[1])?;
    let survival = v.transmittance(b - a)?;
    state.contribution = state.contribution * survival;
    context_mut(state)?.shadow_probability = survival;
    state.stage = Stage::ShadowEmitters;
    Ok(())
}
pub(super) fn scatter(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let c = context(state)?;
    let incoming = c.event.ok_or_else(|| invalid("CPU Atmosphere collision vertex missing"))?;
    let position = c.source;
    if !continuing(task, state)? {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let v = medium(task)?;
    let u = variates(task, state, 0x6001)?;
    // Reuse the exact shared HG inverse/frame, not a new approximation or BRDF.
    let hg = crate::transmission::subsurface::Subsurface { scattering_per_mm: 1.0, anisotropy: v.anisotropy };
    let direction = hg.sample_direction(incoming, u).map_err(|e| JobError::new(ErrorCode::InvalidScene, e.to_string()))?;
    state.previous_pdf = v.phase(incoming.dot(direction).clamp(-1.0, 1.0))?;
    state.previous_position = position;
    let ray = Ray::new(position, direction, 0.0_f64.next_up(), super::super::super::MAX_DISTANCE_MM).map_err(error)?;
    state.ray = Some(ray);
    state.cursor = Some(TraceCursor::new(ray, false));
    state.hit = None;
    state.emitter = None;
    state.depth += 1;
    context_mut(state)?.event = None;
    state.stage = Stage::Trace;
    Ok(())
}
