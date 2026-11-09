//! Explicit rough and spectral dielectric events on the existing finite path.
//! Spectral paths use one RGB hero wavelength; inactive models retain the old stream.
use super::*;
use crate::glass_model::WAVELENGTHS_NM;

#[derive(Clone, Copy)]
pub(super) struct SheetWalk {
    index: usize,
    normal: Vec3,
    geometric: Vec3,
    direction: Vec3,
}
fn model_error(error: impl std::fmt::Display) -> JobError {
    JobError::new(ErrorCode::InvalidScene, error.to_string())
}
fn active(input: &job::DielectricOptions) -> bool {
    input.model.is_some_and(|model| model.roughness > 0.0 || model.spectral())
}
pub(super) fn required(task: &TransportContext<'_>, state: &State, index: usize) -> job::Result<bool> {
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU advanced glass material disappeared"))?;
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU advanced glass stack missing"))?;
    let incident_spectral =
        current(optical)?.and_then(|index| task.options.dielectrics.get(index)).and_then(|input| input.model).is_some_and(|model| model.spectral());
    Ok(active(input) || incident_spectral || optical.spectral_channel.is_some())
}
fn wavelength(state: &State) -> job::Result<f64> {
    let index = state.optical.as_ref().and_then(|optical| optical.spectral_channel).unwrap_or(1);
    WAVELENGTHS_NM.get(usize::from(index)).copied().ok_or_else(|| invalid("CPU glass wavelength channel is invalid"))
}
fn select_spectrum(task: &TransportContext<'_>, state: &mut State, index: usize) -> job::Result<()> {
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU spectral glass stack missing"))?;
    if optical.spectral_channel.is_some() {
        return Ok(());
    }
    let spectral = |index: usize| task.options.dielectrics.get(index).and_then(|input| input.model).is_some_and(|model| model.spectral());
    if !spectral(index) && !current(optical)?.is_some_and(spectral) {
        return Ok(());
    }
    let [draw, _] = variates(task, state, 0x4100)?;
    let channel = (draw * 3.0) as u8;
    let mask = match channel {
        0 => Vec3::new(3.0, 0.0, 0.0),
        1 => Vec3::new(0.0, 3.0, 0.0),
        2 => Vec3::new(0.0, 0.0, 3.0),
        _ => return Err(invalid("CPU glass wavelength draw is invalid")),
    };
    state.throughput = multiply(state.throughput, mask);
    state.optical.as_mut().ok_or_else(|| invalid("CPU spectral glass stack missing"))?.spectral_channel = Some(channel);
    Ok(())
}
fn ior(task: &TransportContext<'_>, state: &State, index: Option<usize>) -> job::Result<f64> {
    let Some(index) = index else { return Ok(1.0) };
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU spectral medium index is invalid"))?;
    input.model.unwrap_or_default().ior(input.ior, wavelength(state)?).map_err(model_error)
}
fn uniforms(task: &TransportContext<'_>, state: &State) -> job::Result<[f64; 3]> {
    let [branch, x] = variates(task, state, 0x4000)?;
    let [y, _] = variates(task, state, 0x4001)?;
    Ok([x, y, branch])
}
fn apply_weight(state: &mut State, weight: f64) -> job::Result<()> {
    if !weight.is_finite() || weight < 0.0 {
        return Err(invalid("CPU advanced glass has a nonfinite or negative weight"));
    }
    state.throughput = state.throughput * weight;
    super::super::super::lighting::validate_radiance(state.throughput).map_err(error)
}
fn finish(
    task: &TransportContext<'_>,
    state: &mut State,
    index: usize,
    direction: Vec3,
    pdf: f64,
    entering: Option<bool>,
    preserve_pdf: bool,
) -> job::Result<()> {
    let hit = unmapped(task, state)?;
    let optical = state.optical.as_mut().ok_or_else(|| invalid("CPU advanced glass stack missing"))?;
    if let Some(enter) = entering {
        if enter {
            *optical.media.get_mut(optical.len).ok_or_else(|| invalid("CPU advanced glass stack exceeds 16"))? = index;
            optical.len += 1;
        } else {
            optical.len = optical.len.checked_sub(1).ok_or_else(|| invalid("CPU advanced glass stack underflow"))?;
        }
    }
    // Finite emitters and environment endpoints remain BSDF-only here. The
    // hard-source connection has a discrete measure and does not compete with
    // those sampled endpoints, even when the rough proposal has finite density.
    if !preserve_pdf {
        optical.delta = true;
        state.previous_pdf = pdf;
        state.previous_position = hit.position;
    }
    optical.segment_start = hit.position;
    optical.source_plane = Some(hit.vertices);
    optical.sheet = None;
    if state.throughput == Vec3::ZERO {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let (ray, cursor) = interface_cursor(task, index, hit, direction, None, true)?;
    state.ray = Some(ray);
    state.cursor = Some(cursor);
    state.hit = None;
    state.depth += 1;
    state.stage = Stage::Trace;
    Ok(())
}

pub(super) fn surface(task: &TransportContext<'_>, state: &mut State, index: usize) -> job::Result<()> {
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU advanced glass material disappeared"))?;
    let thin = input.boundary == job::DielectricBoundary::ThinSheet;
    if thin && input.model.unwrap_or_default().roughness == 0.0 {
        if !continuing(task, state)? {
            state.stage = Stage::Commit;
            return Ok(());
        }
        let hit = unmapped(task, state)?;
        let direction = state.ray.ok_or_else(|| invalid("CPU modeled sheet incident ray missing"))?.direction();
        let normal = if advanced_normals(task) {
            hit.oriented_geometric_normal()
        } else {
            dielectric::shaded_interface(direction, hit.oriented_geometric_normal(), hit.shading_normal, 1.0, input.ior).map_err(error)?.1
        };
        return smooth_sheet(task, state, index, normal);
    }
    select_spectrum(task, state, index)?;
    let hit = unmapped(task, state)?;
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU advanced glass material disappeared"))?;
    let model = input.model.unwrap_or_default();
    let direction = state.ray.ok_or_else(|| invalid("CPU advanced glass incident ray missing"))?.direction();
    let geometric = hit.oriented_geometric_normal();
    let optical = state.optical.as_ref().ok_or_else(|| invalid("CPU advanced glass stack missing"))?;
    let medium = current(optical)?;
    let region = task.glass.first().ok_or_else(|| invalid("CPU advanced glass certificate missing"))?.region(index)?;
    let thin = input.boundary == job::DielectricBoundary::ThinSheet;
    let next = if thin {
        if medium.is_some() {
            return Err(invalid("CPU thin advanced glass cannot be inside a closed medium"));
        }
        Some(index)
    } else if hit.front_face {
        if medium != region.parent {
            return Err(invalid("CPU advanced glass entered outside its certified parent"));
        }
        Some(index)
    } else {
        if medium != Some(index) {
            return Err(invalid("CPU advanced glass exit does not match the current medium"));
        }
        region.parent
    };
    let ni = ior(task, state, medium)?;
    let nt = ior(task, state, next)?;
    let event = glass_lighting::Interface {
        index,
        position: hit.position,
        canonical: hit.geometric_normal,
        normal: geometric,
        geometric,
        incident: direction,
        ni,
        nt,
        next_medium: next,
        wavelength_nm: wavelength(state)?,
        kind: if thin { glass_lighting::InterfaceKind::SheetExternal } else { glass_lighting::InterfaceKind::Closed },
    };
    if model.roughness > 0.0 && glass_lighting::begin(task, state, glass_lighting::Event::Interface(event)) {
        return Ok(());
    }
    resume_surface(task, state, event)
}
pub(super) fn resume_surface(task: &TransportContext<'_>, state: &mut State, event: glass_lighting::Interface) -> job::Result<()> {
    if !continuing(task, state)? {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let input = task.options.dielectrics.get(event.index).ok_or_else(|| invalid("CPU advanced glass material disappeared"))?;
    let thin = event.kind == glass_lighting::InterfaceKind::SheetExternal;
    let Some(sample) = input
        .model
        .unwrap_or_default()
        .sample(event.incident, event.normal, event.ni, event.nt, event.wavelength_nm, uniforms(task, state)?)
        .map_err(model_error)?
    else {
        state.stage = Stage::Commit;
        return Ok(());
    };
    if (sample.crosses && sample.direction.dot(event.geometric) >= 0.0) || (!sample.crosses && sample.direction.dot(event.geometric) <= 0.0) {
        state.stage = Stage::Commit;
        return Ok(());
    }
    apply_weight(state, sample.weight)?;
    state.sampled_continuous |= !sample.delta;
    if thin && sample.crosses {
        state.optical.as_mut().ok_or_else(|| invalid("CPU rough sheet stack missing"))?.sheet =
            Some(SheetWalk { index: event.index, normal: event.normal, geometric: event.geometric, direction: sample.direction });
        state.depth += 1;
        state.stage = Stage::GlassSheet;
        return Ok(());
    }
    let entering = if !thin && sample.crosses { Some(unmapped(task, state)?.front_face) } else { None };
    finish(task, state, event.index, sample.direction, sample.pdf, entering, false)
}

fn smooth_sheet(task: &TransportContext<'_>, state: &mut State, index: usize, normal: Vec3) -> job::Result<()> {
    let input = task.options.dielectrics.get(index).ok_or_else(|| invalid("CPU modeled sheet material disappeared"))?;
    let direction = state.ray.ok_or_else(|| invalid("CPU modeled sheet incident ray missing"))?.direction();
    let (interface, reflected, transmitted) = dielectric::modeled_sheet(direction, normal, input).map_err(error)?;
    let probability = thin_probability(reflected, transmitted)?;
    let [draw, _] = variates(task, state, 0x4000)?;
    let (outgoing, preserve) = if draw < probability {
        state.throughput = multiply(state.throughput, reflected / probability);
        (interface.reflected, false)
    } else {
        let mass = 1.0 - probability;
        if mass <= 0.0 {
            return Err(invalid("CPU modeled sheet transmission has zero sampling mass"));
        }
        state.throughput = multiply(state.throughput, transmitted / mass);
        if input.shadow_transmission {
            state.previous_pdf *= mass;
        }
        (direction, input.shadow_transmission)
    };
    super::super::super::lighting::validate_radiance(state.throughput).map_err(error)?;
    finish(task, state, index, outgoing, 0.0, None, preserve)
}

pub(super) fn sheet_step(task: &TransportContext<'_>, state: &mut State) -> job::Result<()> {
    let walk = state.optical.as_ref().and_then(|optical| optical.sheet).ok_or_else(|| invalid("CPU rough sheet continuation missing"))?;
    let input = task.options.dielectrics.get(walk.index).ok_or_else(|| invalid("CPU rough sheet material disappeared"))?;
    let cosine = walk.direction.dot(walk.normal).abs();
    let distance = if input.thickness_mm == 0.0 {
        0.0
    } else if cosine > 0.0 {
        input.thickness_mm / cosine
    } else {
        return Err(invalid("CPU rough sheet has no finite grazing optical distance"));
    };
    state.throughput = multiply(state.throughput, dielectric::attenuation(input.absorption_per_mm, distance).map_err(error)?);
    let normal = if walk.direction.dot(walk.normal) < 0.0 { walk.normal } else { -walk.normal };
    let geometric = if walk.direction.dot(walk.normal) < 0.0 { walk.geometric } else { -walk.geometric };
    let hit = unmapped(task, state)?;
    let event = glass_lighting::Interface {
        index: walk.index,
        position: hit.position,
        canonical: hit.geometric_normal,
        normal,
        geometric,
        incident: walk.direction,
        ni: ior(task, state, Some(walk.index))?,
        nt: 1.0,
        next_medium: None,
        wavelength_nm: wavelength(state)?,
        kind: glass_lighting::InterfaceKind::SheetInternal,
    };
    if state.throughput == Vec3::ZERO {
        state.stage = Stage::Commit;
        return Ok(());
    }
    if glass_lighting::begin(task, state, glass_lighting::Event::Interface(event)) {
        return Ok(());
    }
    resume_sheet(task, state, event)
}
pub(super) fn resume_sheet(task: &TransportContext<'_>, state: &mut State, event: glass_lighting::Interface) -> job::Result<()> {
    if !continuing(task, state)? {
        state.stage = Stage::Commit;
        return Ok(());
    }
    let walk = state.optical.as_ref().and_then(|optical| optical.sheet).ok_or_else(|| invalid("CPU rough sheet continuation missing"))?;
    let model =
        task.options.dielectrics.get(event.index).and_then(|input| input.model).ok_or_else(|| invalid("CPU rough sheet model disappeared"))?;
    let Some(sample) =
        model.sample(event.incident, event.normal, event.ni, event.nt, event.wavelength_nm, uniforms(task, state)?).map_err(model_error)?
    else {
        state.stage = Stage::Commit;
        return Ok(());
    };
    if (sample.crosses && sample.direction.dot(event.geometric) >= 0.0) || (!sample.crosses && sample.direction.dot(event.geometric) <= 0.0) {
        state.stage = Stage::Commit;
        return Ok(());
    }
    apply_weight(state, sample.weight)?;
    state.sampled_continuous |= !sample.delta;
    if sample.crosses {
        if sample.direction.dot(walk.normal) * sample.direction.dot(walk.geometric) <= 0.0 {
            state.stage = Stage::Commit;
            return Ok(());
        }
        return finish(task, state, walk.index, sample.direction, sample.pdf, None, false);
    }
    state.optical.as_mut().ok_or_else(|| invalid("CPU rough sheet continuation missing"))?.sheet =
        Some(SheetWalk { direction: sample.direction, ..walk });
    state.depth += 1;
    state.stage = Stage::GlassSheet;
    Ok(())
}
