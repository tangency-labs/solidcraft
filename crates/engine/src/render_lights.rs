//! Shared light inspection and numerical adapters; no document or view-state mutation.
use crate::schema::render_lights::{Geometry, Light};
use solidcraft_doc::render_settings::{RenderJob, RenderSetup};
use solidcraft_geom::Vec3;
use solidcraft_render::job::LightingPreset;
use solidcraft_render::job::{AreaLight, AreaLights, DirectionalLightOptions, PunctualLightOptions, RectangleLightOptions, SpotConeOptions, Sun};

pub fn parse_id(id: &str) -> Result<(&str, usize), String> {
    if id.len() > 16 {
        return Err("invalid light id".into());
    }
    let (kind, slot) = id.split_once(':').ok_or("expected kind:slot light id")?;
    let limit = match kind {
        "directional" | "point" | "spot" => 8,
        "area" => 4,
        _ => return Err("unknown light kind".into()),
    };
    let index = slot.parse::<usize>().map_err(|_| "invalid light slot")?;
    if index >= limit || index.to_string() != slot {
        return Err("light slot is out of bounds or noncanonical".into());
    }
    Ok((kind, index))
}

fn vector([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Original three-band Planck approximation, white-balanced at 6500 K. This is
/// a tint control, not a calibrated spectral renderer. Only RGB is persisted.
pub fn temperature_colour(kelvin: f64) -> [f64; 3] {
    let temperature = if kelvin.is_finite() { kelvin.clamp(1000.0, 40000.0) } else { 6500.0 };
    let rgb = [615e-9_f64, 535e-9, 445e-9]
        .map(|wavelength| ((0.01438776877 / (wavelength * 6500.0)).exp() - 1.0) / ((0.01438776877 / (wavelength * temperature)).exp() - 1.0));
    let maximum = rgb.into_iter().fold(0.0_f64, f64::max);
    rgb.map(|v| v / maximum)
}

pub fn colour_temperature(colour: [f64; 3]) -> f64 {
    let [red, _, blue] = colour;
    let ratio = if red > 0.0 { blue / red } else { f64::MAX };
    let (mut low, mut high) = (1000.0, 40000.0);
    for _ in 0..24 {
        let middle = (low + high) * 0.5;
        let [r, _, b] = temperature_colour(middle);
        if b / r < ratio {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) * 0.5
}

pub fn position(light: &Light, target: [f64; 3], default_distance: f64) -> (f64, f64, f64) {
    let delta = match light.geometry {
        Geometry::Directional { direction, .. } => {
            solidcraft_render::surface::unit_direction(vector(direction)).unwrap_or(Vec3::Z) * default_distance
        }
        Geometry::Point { position_mm, .. } | Geometry::Spot { position_mm, .. } | Geometry::Area { position_mm, .. } => {
            vector(position_mm) - vector(target)
        }
    };
    let distance = delta.len().max(0.001);
    (delta.y.atan2(delta.x).to_degrees(), (delta.z / distance).clamp(-1.0, 1.0).asin().to_degrees(), distance)
}

pub fn patch(light: &mut Light, edit: &crate::schema::render_lights::LightPatch, target: [f64; 3], default_distance: f64) -> Result<(), String> {
    for (value, low, high) in [
        (edit.intensity_ev, -20.0, 27.0),
        (edit.temperature_k, 1000.0, 40000.0),
        (edit.softness, 0.0, 1.0),
        (edit.azimuth_deg, -360.0, 360.0),
        (edit.elevation_deg, -90.0, 90.0),
        (edit.distance_mm, 0.001, 1e6),
        (edit.size_mm, 0.0002, 2e6),
        (edit.width_mm, 0.000002, 2e6),
        (edit.height_mm, 0.000002, 2e6),
        (edit.reference_distance_mm, 0.001, 1e6),
        (edit.minimum_distance_mm, 0.001, 1e6),
    ] {
        if value.is_some_and(|v| !v.is_finite() || !(low..=high).contains(&v)) {
            return Err("light edit exceeds finite numerical bounds".into());
        }
    }
    for (value, low, high) in [
        (edit.position_mm, -1e12, 1e12),
        (edit.direction, -1e6, 1e6),
        (edit.edge_u_mm, -2e6, 2e6),
        (edit.edge_v_mm, -2e6, 2e6),
        (edit.radiance, 0.0, 1e8),
        (edit.colour, 0.0, 1.0),
    ] {
        if value.is_some_and(|v| v.iter().any(|v| !v.is_finite() || !(low..=high).contains(v))) {
            return Err("light vector exceeds finite numerical bounds".into());
        }
    }
    if edit.direction.is_some_and(|v| solidcraft_render::surface::unit_direction(vector(v)).is_err()) {
        return Err("light direction must be nonzero".into());
    }
    let aim = edit.aim_at.unwrap_or(target);
    if aim.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return Err("invalid light aim point".into());
    }
    let (azimuth, elevation, distance) = position(light, aim, default_distance);
    let azimuth = edit.azimuth_deg.unwrap_or(azimuth).to_radians();
    let elevation = edit.elevation_deg.unwrap_or(elevation).to_radians();
    let distance = edit.distance_mm.unwrap_or(distance);
    let direction = Vec3::new(elevation.cos() * azimuth.cos(), elevation.cos() * azimuth.sin(), elevation.sin());
    let point = vector(aim) + direction * distance;
    let old_size = match light.geometry {
        Geometry::Area { edge_u_mm, .. } => vector(edge_u_mm).len(),
        _ => 10.0,
    };
    let type_changed = edit.kind.as_ref().is_some_and(|kind| kind != light.geometry.kind());
    if let Some(kind) = edit.kind.as_ref().filter(|_| type_changed) {
        light.geometry = match kind.as_str() {
            "directional" => Geometry::Directional { direction: array(direction), angular_radius_rad: None },
            "point" => Geometry::Point { position_mm: array(point), reference_distance_mm: distance, minimum_distance_mm: 0.001 },
            "spot" => Geometry::Spot {
                position_mm: array(point),
                reference_distance_mm: distance,
                minimum_distance_mm: 0.001,
                emitted_axis: array(-direction),
                inner_half_angle_radians: 0.2,
                outer_half_angle_radians: 0.5,
            },
            "area" => Geometry::Area { position_mm: array(point), edge_u_mm: [old_size, 0.0, 0.0], edge_v_mm: [0.0, old_size, 0.0] },
            _ => return Err("unknown light type".into()),
        };
    }
    if let Some(ev) = edit.intensity_ev {
        light.intensity = ev.exp2();
    }
    if let Some(k) = edit.temperature_k {
        light.colour = temperature_colour(k);
    }
    if let Some(colour) = edit.colour {
        light.colour = colour;
    }
    if let Some(rgb) = edit.radiance {
        if !matches!(light.geometry, Geometry::Area { .. } | Geometry::Directional { angular_radius_rad: Some(_), .. }) {
            return Err("radiance requires an area light or GPU sun".into());
        }
        (light.intensity, light.colour) = radiance(rgb);
    }
    if !matches!(light.geometry, Geometry::Area { .. })
        && (edit.width_mm.is_some() || edit.height_mm.is_some() || edit.edge_u_mm.is_some() || edit.edge_v_mm.is_some())
    {
        return Err("shape requires an area light".into());
    }
    if !matches!(light.geometry, Geometry::Point { .. } | Geometry::Spot { .. })
        && (edit.reference_distance_mm.is_some() || edit.minimum_distance_mm.is_some())
    {
        return Err("falloff requires a point light".into());
    }
    if !matches!(light.geometry, Geometry::Directional { .. }) && edit.direction.is_some() {
        return Err("direction requires a directional light".into());
    }
    let moved = type_changed || edit.azimuth_deg.is_some() || edit.elevation_deg.is_some() || edit.distance_mm.is_some() || edit.aim_at.is_some();
    match &mut light.geometry {
        Geometry::Directional { direction: axis, angular_radius_rad } => {
            if edit.distance_mm.is_some() || edit.size_mm.is_some() || edit.aim_at.is_some() || edit.position_mm.is_some() {
                return Err("directional lights have no position, size or aim point".into());
            }
            if moved {
                *axis = array(direction);
            }
            if let Some(direction) = edit.direction {
                *axis = direction;
            }
            if let Some(softness) = edit.softness {
                *angular_radius_rad = Some(0.001 + softness * (std::f64::consts::FRAC_PI_2 - 0.001));
            }
        }
        Geometry::Point { position_mm, reference_distance_mm, minimum_distance_mm }
        | Geometry::Spot { position_mm, reference_distance_mm, minimum_distance_mm, .. } => {
            if edit.softness.is_some() || edit.size_mm.is_some() {
                return Err("point/spot lights have no finite emitter size or soft shadows".into());
            }
            if moved {
                *position_mm = array(point);
            }
            if let Some(position) = edit.position_mm {
                *position_mm = position;
            }
            if let Some(reference) = edit.reference_distance_mm {
                *reference_distance_mm = reference;
            }
            if let Some(minimum) = edit.minimum_distance_mm {
                *minimum_distance_mm = minimum;
            }
            if *minimum_distance_mm > *reference_distance_mm {
                return Err("minimum distance exceeds reference distance".into());
            }
        }
        Geometry::Area { position_mm, edge_u_mm, edge_v_mm } => {
            if moved {
                *position_mm = array(point);
            }
            let size = edit.size_mm.or_else(|| edit.softness.map(|s| 0.0002 + s * 1000.0)).unwrap_or(old_size);
            let ratio = vector(*edge_v_mm).len() / old_size.max(1e-6);
            if moved {
                let normal = solidcraft_render::surface::unit_direction(vector(aim) - vector(*position_mm)).unwrap_or(-Vec3::Z);
                let up = if normal.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
                let u = solidcraft_render::surface::unit_direction(up.cross(normal)).unwrap_or(Vec3::X);
                *edge_u_mm = array(u * size);
                *edge_v_mm = array(normal.cross(u) * size * ratio);
            } else if edit.size_mm.is_some() || edit.softness.is_some() {
                *edge_u_mm = array(solidcraft_render::surface::unit_direction(vector(*edge_u_mm)).unwrap_or(Vec3::X) * size);
                *edge_v_mm = array(solidcraft_render::surface::unit_direction(vector(*edge_v_mm)).unwrap_or(Vec3::Y) * size * ratio);
            }
            if let Some(position) = edit.position_mm {
                *position_mm = position;
            }
            if let Some(u) = edit.edge_u_mm {
                *edge_u_mm = u;
            }
            if let Some(v) = edit.edge_v_mm {
                *edge_v_mm = v;
            }
            if let Some(width) = edit.width_mm {
                *edge_u_mm = array(solidcraft_render::surface::unit_direction(vector(*edge_u_mm)).unwrap_or(Vec3::X) * width);
            }
            if let Some(height) = edit.height_mm {
                *edge_v_mm = array(solidcraft_render::surface::unit_direction(vector(*edge_v_mm)).unwrap_or(Vec3::Y) * height);
            }
        }
    }
    if let Geometry::Spot { position_mm, emitted_axis, .. } = &mut light.geometry
        && moved
    {
        *emitted_axis = array(solidcraft_render::surface::unit_direction(vector(aim) - vector(*position_mm)).unwrap_or(-Vec3::Z));
    }
    Ok(())
}

/// Materializes a canonical rig in a cloned setup. Caller validates and publishes
/// atomically. Unsupported backend families are refused, never silently dropped.
pub fn write(setup: &mut RenderSetup, lights: &[Light]) -> Result<(), String> {
    if lights.len() > 20 {
        return Err("at most twenty analytic lights".into());
    }
    let directional: Vec<_> = lights.iter().filter(|l| l.geometry.kind() == "directional").collect();
    let punctual: Vec<_> = lights.iter().filter(|l| matches!(l.geometry, Geometry::Point { .. } | Geometry::Spot { .. })).collect();
    let area: Vec<_> = lights.iter().filter(|l| l.geometry.kind() == "area").collect();
    if directional.len() > 8 || punctual.len() > 8 || area.len() > 4 {
        return Err("light family limit exceeded (8 directional, 8 punctual, 4 area)".into());
    }
    match setup.job.as_mut() {
        Some(RenderJob::CpuRaytrace { options }) => {
            if directional.iter().any(|light| matches!(light.geometry, Geometry::Directional { angular_radius_rad: Some(_), .. })) {
                return Err("CPU directional lights have hard shadows; softness is not supported".into());
            }
            options.preset = None;
            options.lights = Some(
                directional
                    .into_iter()
                    .map(|l| {
                        let direction = match l.geometry {
                            Geometry::Directional { direction, .. } => direction,
                            _ => [0.0, 0.0, 1.0],
                        };
                        DirectionalLightOptions { direction, color: l.colour, intensity: l.intensity }
                    })
                    .collect(),
            );
            options.punctual_lights = Some(
                punctual
                    .into_iter()
                    .filter_map(|l| {
                        let (position_mm, reference_distance_mm, minimum_distance_mm, cone) = match l.geometry {
                            Geometry::Point { position_mm, reference_distance_mm, minimum_distance_mm } => {
                                (position_mm, reference_distance_mm, minimum_distance_mm, None)
                            }
                            Geometry::Spot {
                                position_mm,
                                reference_distance_mm,
                                minimum_distance_mm,
                                emitted_axis,
                                inner_half_angle_radians,
                                outer_half_angle_radians,
                            } => (
                                position_mm,
                                reference_distance_mm,
                                minimum_distance_mm,
                                Some(SpotConeOptions { emitted_axis, inner_half_angle_radians, outer_half_angle_radians }),
                            ),
                            _ => return None,
                        };
                        Some(PunctualLightOptions {
                            position_mm,
                            reference_distance_mm,
                            minimum_distance_mm,
                            cone,
                            color: l.colour,
                            intensity: l.intensity,
                        })
                    })
                    .collect(),
            );
            options.area_lights = Some(
                area.into_iter()
                    .filter_map(|l| match l.geometry {
                        Geometry::Area { position_mm, edge_u_mm, edge_v_mm } => {
                            Some(RectangleLightOptions { center_mm: position_mm, edge_u_mm, edge_v_mm, color: l.colour, intensity: l.intensity })
                        }
                        _ => None,
                    })
                    .collect(),
            );
            if options.area_lights.as_ref().is_none_or(Vec::is_empty) {
                options.area_samples = None;
                options.seed = None;
            }
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            if !punctual.is_empty() || directional.len() > 1 {
                return Err("GPU lighting supports one Sun and four Areas; choose an explicit studio before editing legacy default lights".into());
            }
            let mut lighting = options.lighting.unwrap_or_default();
            lighting.sun = directional.first().and_then(|l| match l.geometry {
                Geometry::Directional { direction, angular_radius_rad } => {
                    Some(Sun { direction, angular_radius_rad: angular_radius_rad.unwrap_or(0.02), radiance: l.colour.map(|c| c * l.intensity) })
                }
                _ => None,
            });
            let areas: Vec<_> = area
                .into_iter()
                .filter_map(|l| match l.geometry {
                    Geometry::Area { position_mm, edge_u_mm, edge_v_mm } => Some(AreaLight {
                        center: position_mm,
                        u: edge_u_mm.map(|v| v * 0.5),
                        v: edge_v_mm.map(|v| v * 0.5),
                        radiance: l.colour.map(|c| c * l.intensity),
                    }),
                    _ => None,
                })
                .collect();
            lighting.area_lights = AreaLights::new(&areas).map_err(|e| e.to_string())?;
            options.lighting = Some(lighting);
        }
        _ => return Err("choose CPU or GPU ray tracing before editing lights".into()),
    }
    setup.validate().map_err(|e| e.to_string())
}

/// Filters only a rendering copy; returns fixed-studio weights when the backend
/// owns implicit lights that cannot be materialized in the saved light union.
pub fn filter(setup: &mut RenderSetup, visibility: &crate::schema::render_lights::Visibility) -> Result<Option<[f64; 3]>, String> {
    if !visibility.active() {
        return Ok(None);
    }
    let mut lights = inspect(setup)?;
    if lights.is_empty() {
        return Ok(None);
    }
    if matches!(setup.job, None | Some(RenderJob::Raster { .. }))
        || matches!(&setup.job,Some(RenderJob::GpuPath{options,..}) if options.lighting.is_none())
    {
        let mut weights = [0.0; 3];
        for (slot, light) in weights.iter_mut().zip(&lights) {
            *slot = if visibility.visible(&light.id) { 1.0 } else { 0.0 };
        }
        return Ok(Some(weights));
    }
    for light in &mut lights {
        if !visibility.visible(&light.id) {
            light.intensity = 0.0;
        }
    }
    write(setup, &lights)?;
    Ok(None)
}

pub fn inspect(setup: &RenderSetup) -> Result<Vec<Light>, String> {
    setup.validate().map_err(|e| e.to_string())?;
    let mut lights = Vec::new();
    match &setup.job {
        Some(RenderJob::CpuRaytrace { options }) => {
            let implicit = options.lights.is_none();
            let defaults;
            let directional = match &options.lights {
                Some(values) => values,
                None => {
                    defaults = options.preset.unwrap_or(LightingPreset::Neutral).directional_lights();
                    &defaults
                }
            };
            for (index, light) in directional.iter().enumerate() {
                push(
                    &mut lights,
                    index,
                    implicit,
                    light.intensity,
                    light.color,
                    Geometry::Directional { direction: light.direction, angular_radius_rad: None },
                );
            }
            for (index, light) in options.punctual_lights.iter().flatten().enumerate() {
                let geometry = match light.cone {
                    Some(cone) => Geometry::Spot {
                        position_mm: light.position_mm,
                        reference_distance_mm: light.reference_distance_mm,
                        minimum_distance_mm: light.minimum_distance_mm,
                        emitted_axis: cone.emitted_axis,
                        inner_half_angle_radians: cone.inner_half_angle_radians,
                        outer_half_angle_radians: cone.outer_half_angle_radians,
                    },
                    None => Geometry::Point {
                        position_mm: light.position_mm,
                        reference_distance_mm: light.reference_distance_mm,
                        minimum_distance_mm: light.minimum_distance_mm,
                    },
                };
                push(&mut lights, index, false, light.intensity, light.color, geometry);
            }
            for (index, light) in options.area_lights.iter().flatten().enumerate() {
                push(
                    &mut lights,
                    index,
                    false,
                    light.intensity,
                    light.color,
                    Geometry::Area { position_mm: light.center_mm, edge_u_mm: light.edge_u_mm, edge_v_mm: light.edge_v_mm },
                );
            }
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            if let Some(lighting) = options.lighting {
                if let Some(sun) = lighting.sun {
                    let (intensity, colour) = radiance(sun.radiance);
                    push(
                        &mut lights,
                        0,
                        false,
                        intensity,
                        colour,
                        Geometry::Directional { direction: sun.direction, angular_radius_rad: Some(sun.angular_radius_rad) },
                    );
                }
                for (index, light) in lighting.area_lights.iter().enumerate() {
                    let (intensity, colour) = radiance(light.radiance);
                    push(
                        &mut lights,
                        index,
                        false,
                        intensity,
                        colour,
                        Geometry::Area { position_mm: light.center, edge_u_mm: light.u.map(|v| v * 2.0), edge_v_mm: light.v.map(|v| v * 2.0) },
                    );
                }
            } else if options.environment.is_none() && options.hdr.is_none() {
                defaults(&mut lights);
            }
        }
        // Raster and the legacy setup use the fixed direct-light product preview.
        Some(RenderJob::Raster { .. }) | None => defaults(&mut lights),
    }
    Ok(lights)
}

fn defaults(lights: &mut Vec<Light>) {
    for (index, light) in LightingPreset::Neutral.directional_lights().into_iter().enumerate() {
        push(lights, index, true, light.intensity, light.color, Geometry::Directional { direction: light.direction, angular_radius_rad: None });
    }
}

fn radiance(rgb: [f64; 3]) -> (f64, [f64; 3]) {
    let intensity = rgb.into_iter().fold(0.0_f64, f64::max);
    (intensity, if intensity > 0.0 { rgb.map(|v| v / intensity) } else { [1.0; 3] })
}

fn push(lights: &mut Vec<Light>, index: usize, implicit: bool, intensity: f64, colour: [f64; 3], geometry: Geometry) {
    let number = lights.len() + 1;
    let (name, name_key) = match number {
        1 => ("Key".into(), "render.light_default_key"),
        2 => ("Fill".into(), "render.light_default_fill"),
        3 => ("Rim".into(), "render.light_default_rim"),
        _ => (format!("Light {number}"), "render.light_default_number"),
    };
    lights.push(Light { id: format!("{}:{index}", geometry.kind()), name, name_key: name_key.into(), number, implicit, intensity, colour, geometry });
}
