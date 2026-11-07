//! Spatial camera sampling and CPU output presentation regressions.
// Independent reader for the public uncompressed scanline/FLOAT wire layout used
// by these retained masters. It deliberately uses neither our encoder's header
// constructor nor the production native-master decoder, and preserves raw bits.
fn native_exr_pixels(bytes: &[u8]) -> ::image::Rgba32FImage {
    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    fn string<'a>(bytes: &'a [u8], cursor: &mut usize) -> &'a [u8] {
        let begin = *cursor;
        let length = bytes[begin..].iter().position(|b| *b == 0).unwrap();
        *cursor += length + 1;
        &bytes[begin..begin + length]
    }
    assert_eq!(word(bytes, 0), 20_000_630, "OpenEXR magic");
    assert_eq!(word(bytes, 4), 2, "single-part scanline version, no feature flags");
    let mut cursor = 8;
    let mut dimensions = None;
    let mut channels = false;
    let mut uncompressed = false;
    let mut increasing = false;
    while bytes[cursor] != 0 {
        let name = string(bytes, &mut cursor);
        let kind = string(bytes, &mut cursor);
        let length = word(bytes, cursor) as usize;
        cursor += 4;
        let value = &bytes[cursor..cursor + length];
        cursor += length;
        match name {
            b"channels" => {
                assert_eq!(kind, b"chlist");
                let mut at = 0;
                for channel in [b"A", b"B", b"G", b"R"] {
                    assert_eq!(string(value, &mut at), channel);
                    assert_eq!(word(value, at), 2, "FLOAT channel type");
                    assert_eq!(&value[at + 4..at + 8], &[0; 4]);
                    assert_eq!(word(value, at + 8), 1, "full horizontal sampling");
                    assert_eq!(word(value, at + 12), 1, "full vertical sampling");
                    at += 16;
                }
                assert_eq!(&value[at..], &[0], "exactly A/B/G/R");
                assert!(!channels);
                channels = true;
            }
            b"dataWindow" => {
                assert_eq!(kind, b"box2i");
                assert_eq!(value.len(), 16);
                assert_eq!((word(value, 0), word(value, 4)), (0, 0));
                let (width, height) = (word(value, 8) + 1, word(value, 12) + 1);
                assert!(width > 0 && height > 0);
                assert!(dimensions.replace((width, height)).is_none());
            }
            b"compression" => {
                assert_eq!(kind, b"compression");
                assert_eq!(value, &[0]);
                uncompressed = true;
            }
            b"lineOrder" => {
                assert_eq!(kind, b"lineOrder");
                assert_eq!(value, &[0]);
                increasing = true;
            }
            _ => {}
        }
    }
    cursor += 1;
    assert!(channels && uncompressed && increasing);
    let (width, height) = dimensions.unwrap();
    let table = cursor;
    cursor += height as usize * 8;
    let mut pixels = ::image::Rgba32FImage::new(width, height);
    for y in 0..height {
        let at = table + y as usize * 8;
        assert_eq!(u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()), cursor as u64);
        assert_eq!(word(bytes, cursor), y);
        assert_eq!(word(bytes, cursor + 4), width * 16);
        cursor += 8;
        for (plane, channel) in [3, 2, 1, 0].into_iter().enumerate() {
            for x in 0..width {
                let at = cursor + (plane * width as usize + x as usize) * 4;
                pixels.get_pixel_mut(x, y).0[channel] = f32::from_bits(word(bytes, at));
            }
        }
        cursor += width as usize * 16;
    }
    assert_eq!(cursor, bytes.len(), "complete scanlines with no trailing payload");
    pixels
}

use super::*;
use crate::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
use crate::{ProductMesh, StandardView};
use solidcraft_geom::{Aabb3, Mesh};
use std::sync::Arc;

fn progressive_fixture(samples: u32, seed: u32, left: Option<bool>, exposure: f64) -> ImageTask {
    // Reuse the analytic half-plane camera/geometry, not fixed-AA output.
    let fixed = fixture(1, 1, left);
    let mut options = fixed.options.clone();
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed });
    options.exposure_ev = exposure;
    let mut request = fixed.request.clone();
    request.raytrace = Some(options.clone());
    ImageTask::new(fixed.scene.clone(), request, CancelToken::default(), options).unwrap()
}

type ProgressiveOutput = (Vec<(u32, Vec<u8>)>, Vec<u8>, u64);
fn progressive_complete(mut task: ImageTask, quantum: usize) -> ProgressiveOutput {
    let mut previews = Vec::new();
    for _ in 0..2_200_000 {
        let result = task.step_with_previews(quantum).unwrap();
        let progress = task.progress();
        assert!(progress.done <= progress.total);
        match result {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { png, samples_per_pixel } => previews.push((samples_per_pixel, png)),
            ImagePoll::Complete(png) => return (previews, png, task.work.used()),
        }
    }
    panic!("bounded progressive fixture did not finish");
}

#[test]
fn progressive_real_png_has_stable_prefixes_quanta_and_fresh_primary_samples() {
    let (previews, final_png, work) = progressive_complete(progressive_fixture(8, 123, Some(true), 0.0), 1);
    assert_eq!(previews.iter().map(|(samples, _)| *samples).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6, 7]);
    let (other, other_final, other_work) = progressive_complete(progressive_fixture(8, 123, Some(true), 0.0), 16_384);
    assert_eq!(other, previews);
    assert_eq!(other_final, final_png);
    assert_eq!(other_work, work, "quanta cannot reset or change the cumulative budget");
    for (samples, checkpoint) in &previews {
        let (_, prefix, _) = progressive_complete(progressive_fixture(*samples, 123, Some(true), 0.0), 3);
        assert_eq!(&prefix, checkpoint, "target SPP must not alter an earlier prefix");
    }
    let mut task = progressive_fixture(8, 123, Some(true), 0.0);
    let mut rays = Vec::new();
    for _ in 0..200_000 {
        let absent = task.pending.is_none();
        let result = task.step_with_previews(1).unwrap();
        if absent && let Some(pending) = task.pending.as_ref() {
            assert!(!pending.repeatable_primary, "jittered primary samples cannot use fixed-centre reuse");
            rays.push([pending.ray.origin().x.to_bits(), pending.ray.origin().y.to_bits()]);
        }
        if matches!(result, ImagePoll::Complete(_)) {
            break;
        }
    }
    assert_eq!(rays.len(), 8);
    rays.sort();
    rays.dedup();
    assert_eq!(rays.len(), 8, "each absolute ordinal traces a new camera sample");
}

#[test]
fn progressive_coverage_keeps_backplate_outside_exposure_and_tone_mapping() {
    for exposure in [-16.0, 0.0, 16.0] {
        let (_, background, _) = progressive_complete(progressive_fixture(5, 9, None, exposure), 4);
        assert_eq!(::image::load_from_memory(&background).unwrap().to_rgba8().get_pixel(0, 0).0, [255; 4]);
        let (_, half, _) = progressive_complete(progressive_fixture(64, 9, Some(true), exposure), 4);
        let covered = (0..64).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
        assert!(covered > 0 && covered < 64);
        let expected = encode8(1.0 - covered as f64 / 64.0);
        assert_eq!(::image::load_from_memory(&half).unwrap().to_rgba8().get_pixel(0, 0).0, [expected, expected, expected, 255]);
    }
    let (previews, _, _) = progressive_complete(progressive_fixture(5, 9, Some(true), 0.0), 1);
    assert_eq!(previews.iter().map(|(samples, _)| *samples).collect::<Vec<_>>(), [1, 2, 3, 4], "every complete nonfinal pass is published");
}

#[test]
fn progressive_resolves_the_mean_radiance_once_instead_of_averaging_display_pixels() {
    let left = fixture(1, 1, Some(true));
    let right = fixture(1, 1, Some(false));
    let mut source = left.scene.clone();
    let mut other = right.scene.meshes[0].clone();
    other.name = "Bright half".into();
    source.bounds = source.bounds.union(&other.mesh.bounds());
    source.meshes.push(other);
    let mut options = left.options.clone();
    options.reflections = None;
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: 64, seed: 9 });
    options.exposure_ev = 1.0;
    options.emissions = Some(vec![
        job::SurfaceEmission { body_index: 0, body: "Black half".into(), color: [1.0, 0.5, 0.25], strength: 1.0, sides: job::EmissionSides::Both },
        job::SurfaceEmission { body_index: 1, body: "Bright half".into(), color: [1.0, 0.5, 0.25], strength: 9.0, sides: job::EmissionSides::Both },
    ]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..left.request.clone() };
    let task = ImageTask::new(source, request, CancelToken::default(), options).unwrap();
    let (_, png, _) = progressive_complete(task, 4);
    let left_count = (0..64).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
    let mean = (left_count as f64 + (64 - left_count) as f64 * 9.0) / 64.0;
    let expected = [1.0, 0.5, 0.25].map(|color| {
        let exposed = mean * color * 2.0;
        encode8(exposed / (1.0 + exposed))
    });
    let actual = ::image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(0, 0).0;
    assert_eq!(actual, [expected[0], expected[1], expected[2], 255]);
    let fraction = left_count as f64 / 64.0;
    let prematurely_mapped = encode8(fraction * (2.0 / 3.0) + (1.0 - fraction) * (18.0 / 19.0));
    assert_ne!(actual[0], prematurely_mapped, "each sample must remain linear until the whole pass resolves");
}

#[test]
fn progressive_film_and_resolve_share_cancellation_and_one_cumulative_work_budget() {
    let mut task = progressive_fixture(4, 0, Some(true), 0.0);
    task.work = WorkBudget::new(1).unwrap();
    assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
    assert_eq!(task.step_with_previews(1).err().unwrap().code, ErrorCode::LimitExceeded);
    for during_resolve in [false, true] {
        let mut task = progressive_fixture(4, 0, Some(true), 0.0);
        if during_resolve {
            for _ in 0..200_000 {
                if task.progressive.as_ref().is_some_and(|p| p.resolving) {
                    break;
                }
                assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            }
            assert!(task.progressive.as_ref().unwrap().resolving);
        }
        task.cancel.cancel();
        assert_eq!(task.step_with_previews(4).err().unwrap().code, ErrorCode::Cancelled);
        assert!(!task.done);
    }
}

fn progressive_area_fixture(samples: u32, led: bool) -> ImageTask {
    let mut camera = crate::Camera { distance: 10.0, fov: 0.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    let receiver = ProductMesh {
        transmission: None,
        name: "diffuse area receiver".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 0.0), Vec3::new(-20.0, 20.0, 0.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::new(0.8, 0.6, 0.4), 0.0, 0.7, 1.5).unwrap()),
        faces: Vec::new(),
    };
    let mut options = RaytraceOptions {
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 9 }),
        lights: Some(Vec::new()),
        shadows: true,
        ..Default::default()
    };
    let strip = job::led_strip::LedStripOptions {
        center_mm: [0.0, 0.0, 10.0],
        length_mm: 4.0,
        width_mm: 2.0,
        length_axis: [1.0, 0.0, 0.0],
        width_axis: [0.0, -1.0, 0.0],
        color: [1.0, 0.5, 0.25],
        intensity: 8.0,
    };
    if led {
        strip.append_to(&mut options, 4).unwrap();
    } else {
        options.area_lights = Some(vec![job::RectangleLightOptions {
            center_mm: [0.0, 0.0, 10.0],
            edge_u_mm: [4.0, 0.0, 0.0],
            edge_v_mm: [0.0, -2.0, 0.0],
            color: [1.0, 0.5, 0.25],
            intensity: 8.0,
        }]);
        options.area_samples = Some(4);
    }
    options.seed = Some(73);
    let bounds = receiver.mesh.bounds();
    let request = ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: job::Integrator::Raytrace,
        raytrace: Some(options.clone()),
        camera,
        width: 1,
        height: 1,
        background: [13, 27, 41],
        transparent_background: false,
    };
    ImageTask::new(ProductScene { meshes: vec![receiver], bounds }, request, CancelToken::default(), options).unwrap()
}

fn progressive_area_expected(task: &ImageTask, samples: u32) -> [u8; 4] {
    let light = super::super::area::RectangleLight::new(
        Vec3::new(0.0, 0.0, 10.0),
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(0.0, -2.0, 0.0),
        Vec3::new(1.0, 0.5, 0.25),
        8.0,
    )
    .unwrap();
    let surface = task.scene.meshes[0].surface.unwrap();
    let mut sum = Vec3::ZERO;
    for ordinal in 0..samples {
        let [dx, dy] = progressive::variates(9, 0, ordinal, 0);
        let (origin, direction) = task.request.camera.ray(dx, dy, 1.0, 1.0);
        // This infinite receiver is z=0. Its closed-form hit does not use the BVH.
        let point = origin + direction * (-origin.z / direction.z);
        let key = area_sample_key(0, 0, 1, ordinal).unwrap();
        for index in 0..4 {
            let emitter = light.sample(point, key, 0, index, 4, 73).unwrap().unwrap().position();
            let delta = emitter - point;
            let distance_squared = delta.len2();
            let incoming = delta / distance_squared.sqrt();
            // A*cos(emitter)/(r²*N), independently of the image light cursor.
            let incident = Vec3::new(1.0, 0.5, 0.25) * (8.0 * 8.0 * incoming.z / distance_squared / 4.0);
            let brdf = surface.brdf(Vec3::Z, -direction, incoming).unwrap();
            sum += multiply(brdf, incident) * incoming.z;
        }
    }
    let mean = sum / f64::from(samples);
    [encode8(mean.x / (1.0 + mean.x)), encode8(mean.y / (1.0 + mean.y)), encode8(mean.z / (1.0 + mean.z)), 255]
}

#[test]
fn progressive_area_and_led_pngs_match_numeric_means_prefixes_and_quanta() {
    let mut rectangle = None;
    for led in [false, true] {
        let expected: Vec<_> = (1..=5).map(|samples| progressive_area_expected(&progressive_area_fixture(5, led), samples)).collect();
        let (previews, final_png, work) = progressive_complete(progressive_area_fixture(5, led), 1);
        assert_eq!(previews.iter().map(|(samples, _)| *samples).collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!((previews.clone(), final_png.clone(), work), progressive_complete(progressive_area_fixture(5, led), 16_384));
        for (samples, checkpoint) in &previews {
            let (_, prefix, _) = progressive_complete(progressive_area_fixture(*samples, led), 3);
            assert_eq!(&prefix, checkpoint);
            assert_eq!(::image::load_from_memory(checkpoint).unwrap().to_rgba8().get_pixel(0, 0).0, expected[(*samples - 1) as usize]);
        }
        assert_eq!(::image::load_from_memory(&final_png).unwrap().to_rgba8().get_pixel(0, 0).0, expected[4]);
        if let Some(previous) = &rectangle {
            assert_eq!(&(previews, final_png, work), previous, "a continuous LED uses exactly the same rectangular transport");
        } else {
            rectangle = Some((previews, final_png, work));
        }
    }
}

#[test]
fn progressive_area_and_led_shadow_continuations_observe_cancel_and_cumulative_work() {
    for led in [false, true] {
        for cancel in [false, true] {
            let mut task = progressive_area_fixture(3, led);
            for _ in 0..100_000 {
                if task.pending.as_ref().is_some_and(|pending| pending.area_shadow) {
                    break;
                }
                assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            }
            assert!(task.pending.as_ref().unwrap().area_shadow);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = task.work.used();
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert!(!task.done);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0, "an interrupted area integral cannot publish a pass");
        }
    }
}

fn fixture(samples: u8, r: u8, left: Option<bool>) -> ImageTask {
    let mut camera = crate::Camera { distance: 10.0, fov: 0.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    let meshes = if let Some(left) = left {
        let (x0, x1) = if left { (0.0, 0.5) } else { (0.5, 1.0) };
        let point = |x, y| {
            let (o, _) = camera.ray(x, y, 1.0, 1.0);
            Vec3::new(o.x, o.y, 0.0)
        };
        vec![ProductMesh {
            transmission: None,
            name: "Black half".into(),
            mesh: Arc::new(Mesh {
                positions: vec![point(x0, 0.0), point(x1, 0.0), point(x1, 1.0), point(x0, 1.0)],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            }),
            color: [0; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 1.0, 1.0, 1.5).unwrap()),
            faces: Vec::new(),
        }]
    } else {
        Vec::new()
    };
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let options = RaytraceOptions {
        primary_samples: samples,
        lights: Some(Vec::new()),
        shadows: false,
        reflections: left.map(|_| ReflectionOptions {
            bodies: vec![ReflectionBody { body: "Black half".into(), mode: ReflectionMode::RoughSpecular }],
            samples: r,
            max_bounces: 2,
        }),
        ..Default::default()
    };
    let request = ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: job::Integrator::Raytrace,
        raytrace: Some(options.clone()),
        camera,
        width: 1,
        height: 1,
        background: [255; 3],
        transparent_background: false,
    };
    ImageTask::new(ProductScene { meshes, bounds }, request, CancelToken::default(), options).unwrap()
}
fn until_emit(task: &mut ImageTask) -> usize {
    let mut primaries = 0;
    let mut previous_primary = None;
    for _ in 0..200_000 {
        let absent = task.pending.is_none();
        let before = task.work.used();
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        assert!(task.work.used() - before <= 4);
        if absent && task.pending.as_ref().is_some_and(|p| p.bounce == 0) {
            primaries += 1;
            let pending = task.pending.as_ref().unwrap();
            assert!(pending.repeatable_primary, "fixed generator must certify only intra-spatial R invariance");
            let o = pending.ray.origin();
            let d = pending.ray.direction();
            let ray = [o.x.to_bits(), o.y.to_bits(), o.z.to_bits(), d.x.to_bits(), d.y.to_bits(), d.z.to_bits()];
            if let Some((pixel, spatial, previous_ray)) = previous_primary {
                if (pixel, spatial) == (task.pixel, task.spatial) {
                    assert_eq!(ray, previous_ray, "fixed primary ray changed between R attempts");
                } else {
                    assert_ne!(ray, previous_ray, "distinct spatial samples must not reuse their primary ray");
                }
            }
            previous_primary = Some((task.pixel, task.spatial, ray));
        }
        if let Some(p) = task.pending.as_ref() {
            if p.bounce != 0 {
                assert!(!p.repeatable_primary, "secondary paths must revoke primary reuse");
            } else if let Some(hit) = p.hit {
                let shared =
                    PreparedScene { bvh: task.bvh.as_ref().unwrap(), primitives: &task.looks, source: &task.scene }.resolve_hit(p.ray, hit).unwrap();
                if task.options.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|b| b.body == shared.body_name)) {
                    assert!(!p.repeatable_primary, "selected primary hit must revoke reuse even when its reflection is null");
                }
            }
        }
        assert!(task.pixels.is_empty(), "early primary hit/miss emitted pixel");
        if task.progress().phase == Phase::Running {
            assert_eq!(task.progress().done, 0, "subsamples counted as completed pixels");
        }
        if task.emit_spatial_pixel {
            return primaries;
        }
    }
    panic!("bounded AA fixture did not reach emission");
}
fn complete(mut task: ImageTask, quantum: usize) -> (Vec<u8>, u64) {
    for _ in 0..200_000 {
        if let TaskPoll::Complete(png) = task.step(quantum).unwrap() {
            return (png, task.work.used());
        }
    }
    panic!("bounded AA fixture did not finish");
}

#[test]
fn transient_options_are_strict_default_one_and_do_not_serialize_a_new_default_field() {
    let omitted: RaytraceOptions = serde_json::from_str("{}").unwrap();
    let explicit: RaytraceOptions = serde_json::from_str(r#"{"primary_samples":1}"#).unwrap();
    assert_eq!(omitted, explicit);
    assert_eq!(omitted.primary_samples, 1);
    assert!(!serde_json::to_value(&omitted).unwrap().as_object().unwrap().contains_key("primary_samples"));
    for n in [0, 2, 8, 17, 255] {
        assert!(RaytraceOptions { primary_samples: n, ..Default::default() }.validate().is_err());
    }
    for raw in [
        r#"{"primary_samples":-1}"#,
        r#"{"primary_samples":1.5}"#,
        r#"{"primary_samples":null}"#,
        r#"{"primary_samples":"4"}"#,
        r#"{"primary_samples":4,"unknown":0}"#,
    ] {
        assert!(serde_json::from_str::<RaytraceOptions>(raw).is_err());
    }
}

#[test]
fn fixed_grids_have_independent_analytic_halfplane_and_triangle_coverage() {
    assert_eq!(primary_offset(1, 0).unwrap(), [0.5, 0.5]);
    assert_eq!(primary_offset(4, 0).unwrap(), [0.25, 0.25]);
    assert_eq!(primary_offset(4, 3).unwrap(), [0.75, 0.75]);
    for (s, half, triangle) in [(4, 2, 1), (16, 8, 3)] {
        let points: Vec<_> = (0..s).map(|i| primary_offset(s, i).unwrap()).collect();
        assert!(points.iter().flatten().all(|v| *v > 0.0 && *v < 1.0));
        assert_eq!(points.iter().filter(|p| p[0] < 0.5).count(), half);
        assert_eq!(points.iter().filter(|p| p[0] + p[1] < 5.0 / 8.0).count(), triangle);
        assert!(primary_offset(s, s).is_err());
    }
    assert!(primary_offset(8, 0).is_err());
    assert!(spatial_key(u64::MAX, 0, 16).is_err());
    assert_ne!(spatial_key(7, 0, 4).unwrap(), spatial_key(7, 1, 4).unwrap());
}

#[test]
fn compositor_keeps_conditional_scene_mean_then_display_linear_coverage() {
    // White L=1, Reinhard=1/2; half coverage over black gives1/4 display-linear.
    assert_eq!(composite_spatial(Vec3::new(0.5, 0.5, 0.5), 2, 4, [0; 3], 0.0).unwrap(), [encode8(0.25), encode8(0.25), encode8(0.25), 255]);
    assert_ne!(encode8(0.25), encode8(1.0 / 3.0));
    // Full coverage of L=0 and L=2 usesT(mean)=1/2, not mean(T)=1/3.
    assert_eq!(composite_spatial(Vec3::new(1.0, 1.0, 1.0), 4, 4, [255; 3], 0.0).unwrap(), [encode8(0.5), encode8(0.5), encode8(0.5), 255]);
    for ev in [-16.0, 0.0, 16.0] {
        assert_eq!(composite_spatial(Vec3::ZERO, 0, 16, [13, 27, 41], ev).unwrap(), [13, 27, 41, 255]);
    }
    let gray = srgb8_to_linear(128);
    assert_eq!(composite_spatial(Vec3::ZERO, 2, 4, [128; 3], 0.0).unwrap(), [encode8(gray * 0.5), encode8(gray * 0.5), encode8(gray * 0.5), 255]);
    assert!(composite_spatial(Vec3::ZERO, 5, 4, [0; 3], 0.0).is_err());
    assert!(composite_spatial(Vec3::X, 0, 4, [0; 3], 0.0).is_err());
}

#[test]
fn real_hit_first_and_miss_first_silhouettes_keep_all_attempts_and_null_coverage() {
    for s in [4, 16] {
        for r in [1, 4, 16, 64] {
            for left in [false, true] {
                let mut task = fixture(s, r, Some(left));
                let primaries = until_emit(&mut task);
                assert_eq!(primaries, usize::from(s) * usize::from(r));
                assert_eq!(task.primary_coverage, u32::from(s) * u32::from(r) / 2);
                let before = task.work.used();
                task.step(1).unwrap();
                assert_eq!(task.work.used(), before + 1);
                assert_eq!(task.pixels, [encode8(0.5), encode8(0.5), encode8(0.5), 255]);
                assert_eq!(task.pixel, 1);
            }
        }
    }
}

#[test]
fn miss_only_aa_retains_exact_background_and_quantum_independent_png() {
    for s in [4, 16] {
        let mut task = fixture(s, 1, None);
        assert_eq!(until_emit(&mut task), usize::from(s));
        assert_eq!(task.primary_coverage, 0);
        let small = complete(task, 1);
        let large = complete(fixture(s, 1, None), 16384);
        assert_eq!(small, large);
        let image = ::image::load_from_memory(&small.0).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [255; 4]);
    }
}

#[test]
fn spatial_finalization_and_emission_observe_shared_work_and_cancellation() {
    for emit in [false, true] {
        for cancel in [false, true] {
            let mut task = fixture(16, 4, Some(false));
            for _ in 0..200_000 {
                if (emit && task.emit_spatial_pixel) || (!emit && task.finish_spatial) {
                    break;
                }
                task.step(1).unwrap();
            }
            assert!(if emit { task.emit_spatial_pixel } else { task.finish_spatial });
            let used = task.work.used();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = used;
            }
            let code = task.step(1).err().unwrap().code;
            assert_eq!(code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), used);
            assert!(!task.done && task.pixels.is_empty());
        }
    }
}

#[test]
fn legacy_single_primary_ignores_aa_state_and_preserves_default_bytes_work_and_order() {
    for left in [None, Some(true), Some(false)] {
        for r in [1, 4, 16, 64] {
            let control = fixture(1, r, left);
            let mut isolated = fixture(1, r, left);
            isolated.spatial = 16;
            isolated.primary_coverage = 1024;
            isolated.spatial_radiance = Vec3::new(f64::NAN, 0.0, 0.0);
            isolated.finish_spatial = true;
            isolated.emit_spatial_pixel = true;
            assert_eq!(complete(control, 1), complete(isolated, 16384));
        }
    }
    assert!(std::mem::size_of::<ImageTask>() <= 4096);
    assert_eq!(crate::raytrace::MAX_WORK, 1_000_000_000);
    assert_eq!(job::image::MAX_JOB_BYTES, 384 * 1024 * 1024);
    let legacy = fixture(1, 4, Some(false));
    let aa = fixture(16, 4, Some(false));
    assert_eq!(legacy.admission.estimated_total_bytes, aa.admission.estimated_total_bytes);
}

#[test]
fn full_coverage_black_plane_never_leaks_display_background() {
    for samples in [4, 16] {
        let mut initial = fixture(samples, 16, Some(true));
        let top = initial.request.camera.ray(1.0, 0.0, 1.0, 1.0).0;
        let bottom = initial.request.camera.ray(1.0, 1.0, 1.0, 1.0).0;
        let mesh = Arc::get_mut(&mut initial.scene.meshes[0].mesh).unwrap();
        mesh.positions[1] = Vec3::new(top.x, top.y, 0.0);
        mesh.positions[2] = Vec3::new(bottom.x, bottom.y, 0.0);
        initial.scene.bounds = mesh.bounds();
        let mut task = ImageTask::new(initial.scene, initial.request, initial.cancel, initial.options).unwrap();
        assert_eq!(until_emit(&mut task), usize::from(samples) * 16);
        assert_eq!(task.primary_coverage, u32::from(samples) * 16);
        task.step(1).unwrap();
        assert_eq!(task.pixels, [0, 0, 0, 255]);
    }
}

#[test]
fn later_spatial_attempts_do_not_reset_work_and_never_publish_when_interrupted() {
    for cancel in [false, true] {
        let mut task = fixture(16, 64, Some(true));
        for _ in 0..200_000 {
            if task.spatial >= 1 && task.sample >= 1 {
                break;
            }
            task.step(1).unwrap();
        }
        assert!(task.spatial >= 1 && task.sample >= 1);
        let used = task.work.used();
        assert!(used > 64, "no cumulative work across spatial samples");
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = used;
        }
        assert_eq!(task.step(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), used);
        assert!(!task.done && task.pixels.is_empty());
    }
}

#[test]
fn actual_colored_lit_partial_coverage_wires_positive_radiance_exposure_and_background() {
    for samples in [4, 16] {
        for left in [false, true] {
            let mut initial = fixture(samples, 1, Some(left));
            initial.scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap());
            initial.options.reflections = None;
            initial.options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [0.25, 0.5, 1.0], intensity: 2.0 }]);
            initial.options.exposure_ev = 1.0;
            initial.request.background = [13, 73, 139];
            initial.request.raytrace = Some(initial.options.clone());
            let mut task = ImageTask::new(initial.scene, initial.request, initial.cancel, initial.options).unwrap();
            assert_eq!(until_emit(&mut task), usize::from(samples));
            assert_eq!(task.primary_coverage, u32::from(samples) / 2);
            // At normal incidence, unit-alpha white conductor: D=1/pi,
            // G1=F=1, f=1/(4pi), incoming radiance=2*colored light.
            let linear = [0.5, 1.0, 2.0].map(|v| v / (4.0 * std::f64::consts::PI));
            let expected_p = Vec3::new(linear[0], linear[1], linear[2]) * 0.5;
            assert!((task.spatial_radiance - expected_p).len() < 1e-12);
            let expected: [u8; 3] = std::array::from_fn(|i| {
                let exposed = linear[i] * 2.0;
                let display = 0.5 * exposed / (1.0 + exposed) + 0.5 * srgb8_to_linear([13, 73, 139][i]);
                encode8(display)
            });
            task.step(1).unwrap();
            assert_eq!(task.pixels, [expected[0], expected[1], expected[2], 255]);
        }
    }
}

#[test]
fn workstation_progressive_resolves_every_checkpoint_through_its_admitted_cap() {
    let fixed = fixture(1, 1, Some(true));
    let cap = job::CpuBudget::Workstation.limits().max_samples_per_pixel;
    assert_eq!(cap, job::MAX_SAMPLES_PER_PIXEL);
    for samples in [2048, cap] {
        let mut options = fixed.options.clone();
        options.budget = job::CpuBudget::Workstation;
        options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 123 });
        let request = ImageRequest { raytrace: Some(options.clone()), ..fixed.request.clone() };
        let task = ImageTask::new(fixed.scene.clone(), request, CancelToken::default(), options).unwrap();
        let (previews, final_png, _) = progressive_complete(task, 16_384);
        let expected: Vec<_> = (1..samples).collect();
        assert_eq!(previews.iter().map(|(n, _)| *n).collect::<Vec<_>>(), expected);
        for (_, png) in &previews {
            assert_eq!(::image::load_from_memory(png).unwrap().to_rgba8().dimensions(), (1, 1));
        }
        assert_eq!(::image::load_from_memory(&final_png).unwrap().to_rgba8().dimensions(), (1, 1));
    }
    assert!(composite_spatial(Vec3::ZERO, 0, job::MAX_SAMPLES_PER_PIXEL + 1, [0; 3], 0.0).is_err());
}

#[test]
fn area_light_points_include_spatial_and_film_sample_identity() {
    let light = super::super::area::RectangleLight::new(Vec3::Z * 10.0, Vec3::X * 4.0, -Vec3::Y * 4.0, Vec3::new(1.0, 1.0, 1.0), 1.0).unwrap();
    let mut keys = std::collections::BTreeSet::new();
    let mut points = std::collections::BTreeSet::new();
    for film in [0, 1, 1023, 2047, 4095, 4096, 16383, 65535] {
        for spatial in 0..16 {
            let key = area_sample_key(41, spatial, 16, film).unwrap();
            assert!(keys.insert(key));
            let point = light.sample(Vec3::ZERO, key, 2, 0, 4, 73).unwrap().unwrap().position();
            assert!(points.insert((point.x.to_bits(), point.y.to_bits())));
            assert_eq!(light.sample(Vec3::ZERO, key, 2, 0, 4, 73).unwrap().unwrap().position(), point);
        }
    }
    assert_eq!(area_sample_key(41, 0, 1, 0).unwrap(), 41);
    assert!(area_sample_key(41, 0, 1, job::MAX_SAMPLES_PER_PIXEL).is_err());
    assert!(area_sample_key(41, 4, 4, 0).is_err());
    assert!(area_sample_key(u64::MAX, 0, 4, 0).is_err());
}

fn presentation_fixture(samples: Option<u32>, primary: u8, output: Option<job::OutputOptions>, exposure: f64) -> ImageTask {
    let initial = fixture(primary, 1, Some(true));
    let mut options = initial.options.clone();
    options.reflections = None;
    options.progressive = samples.map(|samples_per_pixel| job::CpuProgressiveOptions { samples_per_pixel, seed: 9 });
    options.output = output.map(Box::new);
    options.exposure_ev = exposure;
    options.emissions = Some(vec![job::SurfaceEmission {
        body_index: 0,
        body: "Black half".into(),
        color: [1.0, 0.5, 0.25],
        strength: 9.0,
        sides: job::EmissionSides::Both,
    }]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..initial.request.clone() };
    ImageTask::new(initial.scene.clone(), request, CancelToken::default(), options).unwrap()
}
type PresentedOutput = (Vec<(u32, Vec<u8>)>, Vec<u8>, Option<Vec<u8>>, u64);
fn complete_presentation(mut task: ImageTask, units: usize) -> PresentedOutput {
    let mut previews = Vec::new();
    for _ in 0..200_000 {
        match task.step_with_previews(units).unwrap() {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { png, samples_per_pixel } => {
                assert!(task.take_linear_exr().is_none(), "provisional images never expose a raw master");
                previews.push((samples_per_pixel, png));
            }
            ImagePoll::Complete(png) => {
                assert_eq!(task.caustics.capacity(), 0, "completed presentation releases its prepaid photon map");
                let master = task.take_linear_exr();
                assert!(task.take_linear_exr().is_none(), "raw ownership transfers exactly once");
                return (previews, png, master, task.work.used());
            }
        }
    }
    panic!("bounded CPU presentation did not finish");
}
fn creative_output() -> job::OutputOptions {
    job::OutputOptions {
        tone_map: job::ToneMap::AcesFitted,
        linear_exr: true,
        finishing: job::finishing::Finishing {
            rgb_gain: [1.3, 0.8, 1.1],
            contrast: 1.2,
            saturation: 0.7,
            bloom: Some(job::bloom::Bloom { strength: 0.2, threshold: 0.5, radius: 1 }),
            photographic: job::photographic::Photographic { grain: 0.03, seed: 42, ..Default::default() },
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn cpu_identity_output_and_raw_retention_preserve_original_f64_pngs() {
    for (samples, primary) in [(None, 1), (None, 4), (Some(7), 1)] {
        let original = complete_presentation(presentation_fixture(samples, primary, None, 1.5), 1);
        for linear_exr in [false, true] {
            let identity = job::OutputOptions { linear_exr, ..Default::default() };
            let actual = complete_presentation(presentation_fixture(samples, primary, Some(identity), 1.5), 1);
            assert_eq!(actual.0, original.0, "even raw retention must preserve every original provisional PNG byte");
            assert_eq!(actual.1, original.1, "identity output must preserve the original f64 PNG exactly");
            assert_eq!(actual.2.is_some(), linear_exr);
            if let Some(master) = actual.2 {
                assert!(master.windows(b"SolidCraft native CPU raytrace".len()).any(|w| w == b"SolidCraft native CPU raytrace"));
            }
        }
    }
}

#[test]
fn cpu_shared_presentation_keeps_every_real_prefix_and_worker_quantum() {
    let output = creative_output();
    let first = complete_presentation(presentation_fixture(Some(7), 1, Some(output), -1.0), 1);
    assert_eq!(first.0.iter().map(|(samples, _)| *samples).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    assert_eq!(first, complete_presentation(presentation_fixture(Some(7), 1, Some(output), -1.0), 16_384));
    for (samples, png) in &first.0 {
        let prefix = complete_presentation(presentation_fixture(Some(*samples), 1, Some(output), -1.0), 3);
        assert_eq!(&prefix.1, png, "EXR work at a final target cannot change its shared presentation");
    }
    let legacy = complete_presentation(presentation_fixture(Some(7), 1, None, -1.0), 1);
    assert_ne!(first.1, legacy.1, "the selected presentation must affect the actual PNG");
}

#[test]
fn cpu_raw_master_keeps_hdr_and_coverage_before_every_presentation_operator() {
    let identity = job::OutputOptions { linear_exr: true, ..Default::default() };
    let original = complete_presentation(presentation_fixture(Some(7), 1, Some(identity), -8.0), 1);
    let mut creative = creative_output();
    creative.backdrop = Some(job::finishing::Backdrop::default());
    let changed = complete_presentation(presentation_fixture(Some(7), 1, Some(creative), 8.0), 1);
    assert_eq!(original.2, changed.2, "raw radiance/coverage cannot contain exposure, bloom, backdrop, tone map or photographic effects");
    let master = original.2.unwrap();
    // The one-pixel uncompressed scanline ends in its canonical A,B,G,R float channels.
    let tail = &master[master.len() - 16..];
    let channel = |at| f32::from_le_bytes(tail[at..at + 4].try_into().unwrap());
    let covered = (0..7).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count() as f64;
    let alpha = (covered / 7.0) as f32;
    assert_eq!(channel(0), alpha);
    for (actual, scale) in [(channel(12), 1.0), (channel(8), 0.5), (channel(4), 0.25)] {
        assert_eq!(actual, (covered / 7.0 * 9.0 * scale) as f32);
    }
    assert!(channel(12) > 1.0, "raw HDR radiance must survive display clipping");
    assert_ne!(original.1, changed.1);
}

#[test]
fn cpu_output_stages_share_cancel_and_one_cumulative_work_ceiling() {
    for stage in ["exr", "bloom", "display", "photographic", "png"] {
        for cancel in [false, true] {
            let mut output = creative_output();
            // The final target also exercises EXR; other stages remain reachable afterward.
            output.linear_exr = true;
            let mut task = presentation_fixture(Some(3), 1, Some(output), 0.0);
            for _ in 0..200_000 {
                if task.output.first().is_some_and(|output| output.stage_name() == stage) {
                    break;
                }
                assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
            }
            assert_eq!(task.output.first().unwrap().stage_name(), stage);
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = task.work.used();
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert!(!task.done);
            assert!(task.take_linear_exr().is_none(), "interrupted output cannot expose a final master");
        }
    }
}

fn cpu_lens_fixture(samples: u32, lens: Option<job::ThinLens>) -> ImageTask {
    let fixed = fixture(1, 1, Some(true));
    let mut options = fixed.options.clone();
    options.reflections = None;
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 9 });
    options.lens = lens;
    let mut request = ImageRequest { raytrace: Some(options.clone()), ..fixed.request.clone() };
    // At z=0 this perspective projection has the same eight-mm image span as
    // the analytic half-plane fixture. Positive pupil radius changes transport only.
    request.camera.fov = 2.0 * 0.4f64.atan();
    ImageTask::new(fixed.scene.clone(), request, CancelToken::default(), options).unwrap()
}

#[test]
fn cpu_thin_lens_rays_match_rolled_axial_focus_and_uniform_area_pupil() {
    let mut request = fixture(1, 1, None).request;
    request.width = 8;
    request.height = 6;
    for (yaw, pitch, roll) in [(0.0, 0.0, 0.0), (-0.45, 0.5, 0.7)] {
        request.camera = crate::Camera { target: Vec3::new(2.0, 3.0, 4.0), distance: 12.0, fov: 0.7, yaw, pitch, roll, ..Default::default() };
        let camera = request.camera;
        let (right, up, back) = camera.basis();
        let lens = job::ThinLens { aperture_radius_mm: 0.4, focus_distance_mm: 9.0 };
        for (x, y) in [(0.25, 1.5), (4.0, 3.0), (7.8, 5.7)] {
            let nx = x / f64::from(request.width) * 2.0 - 1.0;
            let ny = 1.0 - y / f64::from(request.height) * 2.0;
            let focal = camera.eye() - back * lens.focus_distance_mm
                + (right * (nx * f64::from(request.width) / f64::from(request.height)) + up * ny)
                    * ((camera.fov * 0.5).tan() * lens.focus_distance_mm);
            for pupil in [[0.0, 0.0], [0.25, 0.25], [0.81, 0.5]] {
                let disk = (right * (std::f64::consts::TAU * pupil[1]).cos() + up * (std::f64::consts::TAU * pupil[1]).sin())
                    * (lens.aperture_radius_mm * pupil[0].sqrt());
                let ray = lens_primary_ray(&request, x, y, (0.1, 100.0), lens, pupil).unwrap();
                for depth in [0.0, lens.focus_distance_mm * 0.5, lens.focus_distance_mm, lens.focus_distance_mm * 2.0] {
                    let start_depth = (ray.origin() - camera.eye()).dot(-back);
                    let hit = ray.origin() + ray.direction() * ((depth - start_depth) / ray.direction().dot(-back));
                    let pinhole = camera.eye() + (focal - camera.eye()) * (depth / lens.focus_distance_mm);
                    let expected = pinhole + disk * (1.0 - depth / lens.focus_distance_mm);
                    assert!(hit.dist(expected) < 1e-10, "axial depth={depth}, pupil={pupil:?}: {hit:?} != {expected:?}");
                }
            }
        }
    }
}

#[test]
fn cpu_disabled_lens_preserves_fixed_and_every_progressive_png_byte_and_work_unit() {
    for output in [None, Some(job::OutputOptions { linear_exr: true, ..Default::default() })] {
        for (samples, primary) in [(None, 1), (None, 4), (None, 16), (Some(7), 1)] {
            let original = presentation_fixture(samples, primary, output, 1.5);
            let mut options = original.options.clone();
            options.lens = Some(job::ThinLens { aperture_radius_mm: 0.0, focus_distance_mm: 8.0 });
            let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
            let zero = ImageTask::new(original.scene.clone(), request, CancelToken::default(), options).unwrap();
            assert_eq!(zero.admission.estimated_total_bytes, original.admission.estimated_total_bytes, "a zero lens needs no extra buffer");
            assert_eq!(complete_presentation(original, 1), complete_presentation(zero, 1));
        }
    }
}

#[test]
fn cpu_thin_lens_image_has_numeric_defocus_stable_prefixes_and_no_exposure_factor() {
    let radius = 4.0;
    let focused = Some(job::ThinLens { aperture_radius_mm: radius, focus_distance_mm: 10.0 });
    let defocused = Some(job::ThinLens { aperture_radius_mm: radius, focus_distance_mm: 20.0 });
    let pinhole = progressive_complete(cpu_lens_fixture(64, None), 1);
    let sharp = progressive_complete(cpu_lens_fixture(64, focused), 1);
    assert_eq!((sharp.0, sharp.1), (pinhole.0, pinhole.1), "every pupil must meet the same focused surface without an aperture-area exposure factor");
    let actual = progressive_complete(cpu_lens_fixture(64, defocused), 1);
    assert_eq!(actual, progressive_complete(cpu_lens_fixture(64, defocused), 16_384));
    for (samples, png) in &actual.0 {
        assert_eq!(*png, progressive_complete(cpu_lens_fixture(*samples, defocused), 3).1);
    }
    let task = cpu_lens_fixture(64, defocused);
    let camera = task.request.camera;
    let (right, up, back) = camera.basis();
    let bounds = task.scene.meshes[0].mesh.bounds();
    let covered = (0..64)
        .filter(|ordinal| {
            let [x, y] = progressive::variates(9, 0, *ordinal, 0);
            let [u, v] = progressive::variates(9, 0, *ordinal, 0x4c454e53);
            let disk = (right * (std::f64::consts::TAU * v).cos() + up * (std::f64::consts::TAU * v).sin()) * (radius * u.sqrt());
            let point = camera.eye() - back * camera.distance
                + (right * (2.0 * x - 1.0) + up * (1.0 - 2.0 * y)) * camera.half_height()
                + disk * (1.0 - camera.distance / 20.0);
            point.x >= bounds.min.x && point.x <= bounds.max.x && point.y >= bounds.min.y && point.y <= bounds.max.y
        })
        .count();
    let expected = encode8(1.0 - covered as f64 / 64.0);
    let rgba = ::image::load_from_memory(&actual.1).unwrap().to_rgba8();
    assert_eq!(rgba.get_pixel(0, 0).0, [expected, expected, expected, 255]);
    assert_ne!(actual.1, progressive_complete(cpu_lens_fixture(64, None), 1).1, "physical defocus must change the actual image");
}

#[test]
fn cpu_lens_pupil_continuations_observe_cancel_and_one_cumulative_work_budget() {
    for pupil_only in [true, false] {
        for cancel in [false, true] {
            let mut task = cpu_lens_fixture(7, Some(job::ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 20.0 }));
            for _ in 0..100_000 {
                let before = task.work.used();
                assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
                assert!(task.work.used() - before <= 1, "pupil generation is a separate bounded continuation");
                if if pupil_only { task.pupil_sample.is_some() } else { task.pending.is_some() } {
                    break;
                }
            }
            if pupil_only {
                assert!(task.pupil_sample.is_some());
                assert!(task.pending.is_none(), "the pupil continuation yields before constructing the primary ray");
            } else {
                assert!(task.pending.is_some(), "the real pupil ray must have been generated");
                assert!(!task.pending.as_ref().unwrap().repeatable_primary);
                assert!(task.pupil_sample.is_none());
            }
            let before = task.work.used();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = before;
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), before);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            assert!(task.pixels.is_empty());
            assert!(task.take_linear_exr().is_none());
        }
    }
}

#[test]
fn cpu_lens_admission_keeps_the_whole_pupil_outside_certified_glass() {
    let scene = dielectric::tests::scene(dielectric::tests::box_body(8.0));
    let options = RaytraceOptions {
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: 4, seed: 9 }),
        lens: Some(job::ThinLens { aperture_radius_mm: 1.99, focus_distance_mm: 10.0 }),
        dielectrics: vec![dielectric::tests::input(job::DielectricBoundary::ClosedBox)],
        ..Default::default()
    };
    let mut request = dielectric::tests::request(options.clone());
    request.camera.distance = 10.0;
    request.camera.fov = 0.7;
    admit(&scene, &request, &options).unwrap();
    for radius in [2.0, 2.01, 10.0] {
        let options = RaytraceOptions { lens: Some(job::ThinLens { aperture_radius_mm: radius, focus_distance_mm: 10.0 }), ..options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..request.clone() };
        assert!(admit(&scene, &request, &options).unwrap_err().message.contains("aperture sphere"));
    }
}

#[test]
fn cpu_lens_extreme_valid_physical_settings_refuse_before_ray_transport() {
    let mut task = cpu_lens_fixture(4, None);
    task.options.lens = Some(job::ThinLens { aperture_radius_mm: 1e7, focus_distance_mm: 1e-6 });
    task.request.raytrace = Some(task.options.clone());
    let error = admit(&task.scene, &task.request, &task.options).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("bounded ray"));
    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
}

fn alpha_fixture(samples: Option<u32>, primary: u8, transparent: bool, output: Option<job::OutputOptions>, exposure: f64) -> ImageTask {
    let original = presentation_fixture(samples, primary, output, exposure);
    let request = ImageRequest { transparent_background: transparent, ..original.request.clone() };
    ImageTask::new(original.scene, request, original.cancel, original.options).unwrap()
}
fn alpha_pixel(png: &[u8]) -> [u8; 4] {
    ::image::load_from_memory(png).unwrap().to_rgba8().get_pixel(0, 0).0
}
fn alpha_encoded_work(output: &PresentedOutput) -> u64 {
    output.0.iter().map(|(_, png)| png.len() as u64).sum::<u64>() + output.1.len() as u64
}

#[test]
fn cpu_transparent_png_uses_analytic_straight_rgb_and_fixed_or_film_coverage() {
    for primary in [4, 16] {
        for exposure in [-2.0, 0.0, 1.5] {
            let clear = complete_presentation(alpha_fixture(None, primary, true, None, exposure), 1);
            let opaque = complete_presentation(alpha_fixture(None, primary, false, None, exposure), 1);
            let exposed = 2.0f64.powf(exposure);
            let rgb = [9.0, 4.5, 2.25].map(|linear| encode8(linear * exposed / (1.0 + linear * exposed)));
            assert_eq!(
                alpha_pixel(&clear.1),
                [rgb[0], rgb[1], rgb[2], 128],
                "straight RGB is the covered scene mean, never multiplied by edge alpha"
            );
            assert_eq!(clear.3, opaque.3, "a background flag adds no transport or work debit");
            assert_ne!(clear.1, opaque.1);
        }
    }
    let complete = complete_presentation(alpha_fixture(Some(16), 1, true, None, 1.5), 1);
    let rgb = [9.0, 4.5, 2.25].map(|linear| {
        let value = linear * 2.0f64.powf(1.5);
        encode8(value / (1.0 + value))
    });
    for (samples, png) in complete.0.iter().chain(std::iter::once(&(16, complete.1.clone()))) {
        let covered = (0..*samples).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
        let alpha = (covered as f64 / f64::from(*samples) * 255.0).round() as u8;
        assert_eq!(alpha_pixel(png), if alpha == 0 { [0; 4] } else { [rgb[0], rgb[1], rgb[2], alpha] });
    }
    for primary in [1, 4, 16] {
        let original = fixture(primary, 1, None);
        let request = ImageRequest { transparent_background: true, ..original.request.clone() };
        let task = ImageTask::new(original.scene, request, original.cancel, original.options).unwrap();
        assert_eq!(alpha_pixel(&progressive_complete(task, 1).1), [0; 4]);
    }
    assert!(composite_spatial_alpha(Vec3::X, 0, 4, [1, 2, 3], 0.0, true).is_err());
    assert!(composite_spatial_alpha(Vec3::ZERO, 5, 4, [1, 2, 3], 0.0, true).is_err());
    let empty = fixture(1, 1, None);
    let options = RaytraceOptions {
        output: Some(Box::new(job::OutputOptions {
            finishing: job::finishing::Finishing { contrast: 0.5, ..Default::default() },
            ..Default::default()
        })),
        ..empty.options
    };
    let request = ImageRequest { transparent_background: true, raytrace: Some(options.clone()), ..empty.request };
    let clear = ImageTask::new(empty.scene, request, empty.cancel, options).unwrap();
    assert_eq!(
        alpha_pixel(&complete_presentation(clear, 1).1),
        [0; 4],
        "allowed colour finishing cannot leave lifted-black RGB in a zero-alpha pixel"
    );
}

#[test]
fn cpu_alpha_keeps_opaque_f64_bytes_work_and_all_raw_exr_bits() {
    let identity = job::OutputOptions { linear_exr: true, ..Default::default() };
    for (samples, primary) in [(None, 1), (None, 4), (None, 16), (Some(7), 1)] {
        let original = complete_presentation(presentation_fixture(samples, primary, Some(identity), 1.5), 1);
        let false_flag = complete_presentation(alpha_fixture(samples, primary, false, Some(identity), 1.5), 1);
        assert_eq!(false_flag, original, "false stays exactly on the legacy f64 PNG/master/work path");
        let source = alpha_fixture(samples, primary, false, Some(identity), 1.5);
        let transparent = alpha_fixture(samples, primary, true, Some(identity), 1.5);
        assert_eq!(
            source.admission.estimated_total_bytes, transparent.admission.estimated_total_bytes,
            "alpha uses existing coverage/RGBA storage only"
        );
        let alpha = complete_presentation(transparent, 1);
        assert_eq!(alpha.2, original.2, "the scene-linear premultiplied raw master is independent of display alpha choice");
        assert_eq!(
            alpha.3 - alpha_encoded_work(&alpha),
            original.3 - alpha_encoded_work(&original),
            "only actual encoded-byte digest work can differ with PNG content"
        );
        let no_master = complete_presentation(alpha_fixture(samples, primary, true, None, 1.5), 1);
        assert_eq!((alpha.0, alpha.1), (no_master.0, no_master.1), "raw retention cannot change any straight-alpha PNG prefix");
    }
}

#[test]
fn cpu_alpha_shared_finishing_preserves_prefixes_quanta_and_raw_values() {
    let output = job::OutputOptions {
        tone_map: job::ToneMap::AcesFitted,
        finishing: job::finishing::Finishing { rgb_gain: [1.2, 0.8, 1.1], ..Default::default() },
        linear_exr: true,
        ..Default::default()
    };
    let actual = complete_presentation(alpha_fixture(Some(7), 1, true, Some(output), -1.0), 1);
    assert_eq!(actual, complete_presentation(alpha_fixture(Some(7), 1, true, Some(output), -1.0), 16384));
    for (samples, png) in &actual.0 {
        assert_eq!(*png, complete_presentation(alpha_fixture(Some(*samples), 1, true, Some(output), -1.0), 3).1);
    }
    let opaque = complete_presentation(alpha_fixture(Some(7), 1, false, Some(output), -1.0), 1);
    assert_eq!(actual.2, opaque.2);
    assert_eq!(
        actual.3 - alpha_encoded_work(&actual),
        opaque.3 - alpha_encoded_work(&opaque),
        "alpha adds no film, conversion or presentation work beyond actual PNG byte hashing"
    );
    let coverage = (0..7).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
    let rgb = std::array::from_fn::<_, 3, _>(|i| {
        let x: f64 = [9.0, 4.5, 2.25][i] * 0.5 * [1.2, 0.8, 1.1][i];
        encode8(((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0))
    });
    assert_eq!(alpha_pixel(&actual.1), [rgb[0], rgb[1], rgb[2], (coverage as f64 / 7.0 * 255.0).round() as u8]);
}

#[test]
fn cpu_alpha_presentation_rounds_six_sample_half_ties_from_exact_hit_counts() {
    let covered = (0..6).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
    assert_eq!(covered, 5, "the fixture must exercise the exact 5/6 alpha half tie");
    let baseline =
        complete_presentation(alpha_fixture(Some(6), 1, true, Some(job::OutputOptions { linear_exr: true, ..Default::default() }), 1.5), 1);
    for tone_map in [job::ToneMap::Reinhard, job::ToneMap::AcesFitted] {
        for denoise in [false, true] {
            for creative in [false, true] {
                let mut finishing = job::finishing::Finishing::default();
                if creative {
                    finishing.rgb_gain = [1.2, 0.8, 1.1];
                    finishing.contrast = 1.1;
                    finishing.saturation = 0.7;
                    finishing.vignette = 0.2;
                    finishing.looks.push(job::looks::LookLayer { kind: job::looks::LookKind::Warm, strength: 0.4, enabled: true }).unwrap();
                }
                let make = |samples, retain| {
                    let output = job::OutputOptions { tone_map, finishing, linear_exr: retain, ..Default::default() };
                    let original = alpha_fixture(Some(samples), 1, true, Some(output), 1.5);
                    let options = RaytraceOptions { denoise, ..original.options.clone() };
                    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
                    ImageTask::new(original.scene, request, original.cancel, options).unwrap()
                };
                let actual = complete_presentation(make(6, true), 1);
                assert_eq!(actual, complete_presentation(make(6, true), 16384));
                assert_eq!(alpha_pixel(&actual.1)[3], 213, "5/6 must round upwards at exactly 212.5");
                for (samples, png) in actual.0.iter().chain(std::iter::once(&(6, actual.1.clone()))) {
                    let hits = (0..*samples).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
                    let expected = (hits as f64 / f64::from(*samples) * 255.0).round() as u8;
                    assert_eq!(alpha_pixel(png)[3], expected, "sample{samples}, {tone_map:?}, denoise={denoise}, creative={creative}");
                    assert_eq!(*png, complete_presentation(make(*samples, true), 3).1);
                }
                assert_eq!(actual.2, baseline.2, "presentation and denoising must preserve every raw EXR bit");
                let no_master = complete_presentation(make(6, false), 1);
                assert_eq!((actual.0, actual.1), (no_master.0, no_master.1), "raw retention must not change PNG coverage");
            }
        }
    }
}

#[test]
fn cpu_alpha_retains_opaque_only_effect_guards_and_cancellable_completion() {
    let original = alpha_fixture(Some(3), 1, true, None, 0.0);
    let creative = creative_output();
    for output in [
        job::OutputOptions { backdrop: Some(Default::default()), ..Default::default() },
        job::OutputOptions { finishing: job::finishing::Finishing { bloom: creative.finishing.bloom, ..Default::default() }, ..Default::default() },
        job::OutputOptions {
            finishing: job::finishing::Finishing { photographic: creative.finishing.photographic, ..Default::default() },
            ..Default::default()
        },
    ] {
        let options = RaytraceOptions { output: Some(Box::new(output)), ..original.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        assert!(admit(&original.scene, &request, &options).is_err(), "alpha never silently strips opaque photographic/bloom/backdrop intent");
    }
    for stage in ["exr", "display", "png"] {
        for cancel in [false, true] {
            let output = job::OutputOptions { tone_map: job::ToneMap::AcesFitted, linear_exr: true, ..Default::default() };
            let mut task = alpha_fixture(Some(3), 1, true, Some(output), 0.0);
            for _ in 0..200000 {
                if task.output.first().is_some_and(|output| output.stage_name() == stage) {
                    break;
                }
                assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
            }
            assert_eq!(task.output.first().unwrap().stage_name(), stage);
            let before = task.work.used();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = before;
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), before);
            assert!(!task.done);
            assert!(task.take_linear_exr().is_none());
        }
    }
}

#[test]
fn cpu_disabled_denoise_preserves_every_png_exr_work_unit_and_buffer_admission() {
    for output in [None, Some(job::OutputOptions { linear_exr: true, ..Default::default() })] {
        for samples in [None, Some(5)] {
            let original = presentation_fixture(samples, 1, output, 1.5);
            let options: RaytraceOptions = serde_json::from_value(serde_json::json!({"denoise":false})).unwrap();
            assert!(!options.denoise);
            assert!(serde_json::to_value(options).unwrap().get("denoise").is_none());
            let mut options = original.options.clone();
            options.denoise = false;
            let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
            let explicit = ImageTask::new(original.scene.clone(), request, CancelToken::default(), options).unwrap();
            assert_eq!(original.denoise.capacity(), 0);
            assert_eq!(explicit.denoise.capacity(), 0);
            assert_eq!(original.admission.estimated_total_bytes, explicit.admission.estimated_total_bytes);
            assert_eq!(complete_presentation(original, 1), complete_presentation(explicit, 16384));
        }
    }
}

#[test]
fn cpu_alpha_and_denoise_compose_without_changing_matte_prefixes_or_raw_masters() {
    let output = job::OutputOptions { linear_exr: true, ..Default::default() };
    let composed = |samples, transparent, denoise| {
        let original = alpha_fixture(Some(samples), 1, transparent, Some(output), 1.5);
        let options = RaytraceOptions { denoise, ..original.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        ImageTask::new(original.scene, request, original.cancel, options).unwrap()
    };
    let actual = complete_presentation(composed(7, true, true), 1);
    assert_eq!(actual, complete_presentation(composed(7, true, true), 16384));
    assert_eq!(actual.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    let rgb = [9.0, 4.5, 2.25].map(|radiance| {
        let exposed = radiance * 2.0f64.powf(1.5);
        encode8(exposed / (1.0 + exposed))
    });
    for (samples, png) in actual.0.iter().chain(std::iter::once(&(7, actual.1.clone()))) {
        let covered = (0..*samples).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
        let alpha = (covered as f64 / f64::from(*samples) * 255.0).round() as u8;
        assert_eq!(alpha_pixel(png), if alpha == 0 { [0; 4] } else { [rgb[0], rgb[1], rgb[2], alpha] });
        assert_eq!(*png, complete_presentation(composed(*samples, true, true), 3).1);
    }
    let unfiltered = complete_presentation(composed(7, true, false), 1);
    let opaque = complete_presentation(composed(7, false, true), 1);
    assert_eq!(actual.2, unfiltered.2, "denoising cannot enter the premultiplied raw companion");
    assert_eq!(actual.2, opaque.2, "PNG alpha choice cannot enter the premultiplied raw companion");
    assert_eq!((actual.0, actual.1), (unfiltered.0, unfiltered.1), "partial-coverage/emitter bypass retains the analytical straight matte");
    let source = composed(7, true, true);
    let creative = creative_output();
    for incompatible in [
        job::OutputOptions { backdrop: Some(Default::default()), ..output },
        job::OutputOptions { finishing: job::finishing::Finishing { bloom: creative.finishing.bloom, ..Default::default() }, ..output },
        job::OutputOptions { finishing: job::finishing::Finishing { photographic: creative.finishing.photographic, ..Default::default() }, ..output },
    ] {
        let options = RaytraceOptions { output: Some(Box::new(incompatible)), ..source.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..source.request.clone() };
        assert!(admit(&source.scene, &request, &options).is_err(), "the denoiser cannot erase an alpha-incompatible authored effect");
    }
}

fn path_fixture(samples: u32, depth: u8, ceiling: bool) -> ImageTask {
    let original = fixture(1, 1, Some(true));
    let quad = |z: f64, down: bool| Mesh {
        positions: vec![Vec3::new(-1e6, -1e6, z), Vec3::new(1e6, -1e6, z), Vec3::new(1e6, 1e6, z), Vec3::new(-1e6, 1e6, z)],
        normals: vec![if down { -Vec3::Z } else { Vec3::Z }; 4],
        triangles: if down { vec![[0, 2, 1], [0, 3, 2]] } else { vec![[0, 1, 2], [0, 2, 3]] },
        tri_face: vec![0; 2],
        ..Default::default()
    };
    let mut body = original.scene.meshes[0].clone();
    body.mesh = Arc::new(quad(0.0, false));
    body.surface = Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 1.0, 1.0).unwrap());
    let mut meshes = vec![body];
    if ceiling {
        let mut emitter = meshes[0].clone();
        emitter.name = "Red ceiling".into();
        // A real camera aperture keeps the emitter out of the primary path even
        // when the producer starts orthographic rays before the complete bounds.
        // Four downward-facing quads surround the 1x1 mm opening at z20.
        let mut ceiling = Mesh::default();
        for [x0, x1, y0, y1] in [[-1e6, -0.5, -1e6, 1e6], [0.5, 1e6, -1e6, 1e6], [-0.5, 0.5, -1e6, -0.5], [-0.5, 0.5, 0.5, 1e6]] {
            let base = ceiling.positions.len() as u32;
            ceiling.positions.extend([Vec3::new(x0, y0, 20.0), Vec3::new(x1, y0, 20.0), Vec3::new(x1, y1, 20.0), Vec3::new(x0, y1, 20.0)]);
            ceiling.normals.extend([-Vec3::Z; 4]);
            ceiling.triangles.extend([[base, base + 2, base + 1], [base, base + 3, base + 2]]);
            ceiling.tri_face.extend([0; 2]);
        }
        emitter.mesh = Arc::new(ceiling);
        emitter.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap());
        meshes.push(emitter);
    }
    let scene = ProductScene { bounds: meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds())), meshes };
    let options = RaytraceOptions {
        path: Some(job::CpuPathOptions { max_depth: depth }),
        budget: job::CpuBudget::Studio,
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 9 }),
        shadows: true,
        lights: Some(vec![]),
        environment: (!ceiling).then_some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        emissions: ceiling.then_some(vec![job::SurfaceEmission {
            body_index: 1,
            body: "Red ceiling".into(),
            color: [1.0, 0.0, 0.0],
            strength: 2.0,
            sides: job::EmissionSides::Both,
        }]),
        ..Default::default()
    };
    // This fixture's independent GGX-mixture oracles assume exactly normal view.
    // StandardView::Top deliberately stops 1e-6 rad short of the pole, which
    // changes reflected proposals and their MIS density at depth >= 2.
    // Its 0.8x0.8 mm orthographic footprint fits inside the physical ceiling
    // aperture. No private clipping override or modified producer is needed.
    let camera = crate::Camera { distance: 1.0, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, roll: 0.0, ..original.request.camera };
    assert!((camera.ray(0.5, 0.5, 1.0, 1.0).1 + Vec3::Z).len() < 1e-15, "analytical normal-view fixture");
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..original.request };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
fn path_mean(mut task: ImageTask) -> ([f64; 3], u64) {
    // For a uniform one-pixel film, each completed pass is exactly one
    // successful sample commit. Read that cursor, never an unpublished pixel.
    assert_eq!((task.request.width, task.request.height), (1, 1));
    assert!(task.options.adaptive.is_none());
    let target = task.options.progressive.unwrap().samples_per_pixel;
    for _ in 0..2_000_000 {
        if task.progressive.as_ref().is_some_and(|p| p.film.samples() == target) {
            let (mean, covered, samples) = task.progressive.as_ref().unwrap().film.pixel(0).unwrap();
            assert_eq!(samples, target);
            assert_eq!(covered, samples);
            return (mean, task.work.used());
        }
        let path_step = task.bvh.is_some()
            && task.progressive.as_ref().is_some_and(|p| p.film.ready() && !p.resolving)
            && task.pixel == 0
            && task.output.is_empty()
            && task.encoder.is_none();
        let before = task.work.used();
        let before_count = path_step.then(|| task.progressive.as_ref().unwrap().film.samples());
        assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
        if path_step {
            let used = task.work.used() - before;
            if task.options.path.is_some() {
                assert!(used <= 1, "each real path continuation consumes at most one unit");
            } else {
                // Legacy pixel_one can finish its charged shade and then call
                // finish_sample's charged film.add in this same continuation.
                // The one-operation GI state machine does not use that route.
                assert!(used <= 2, "legacy transport has at most shade plus one sample commit");
                if used == 2 {
                    assert_eq!(task.pixel, 1, "the extra legacy unit commits exactly pixel0");
                    assert!(task.pending.is_none(), "a committed legacy sample drains its pending path");
                    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), before_count.unwrap() + 1);
                }
            }
        }
    }
    panic!("bounded path film did not complete");
}
fn independent_lambert_scatter(ordinal: u32) -> (f64, f64) {
    let [component, u] = progressive::variates(9, 0, ordinal, 0x5041_5448_3000);
    let cosine = if component < 0.5 { (1.0 - u).sqrt() } else { 1.0 - 2.0 * u };
    if cosine <= 0.0 {
        return (0.0, 0.0);
    }
    let pdf = 0.5 * cosine / std::f64::consts::PI + 0.5 / (4.0 * std::f64::consts::PI);
    (cosine, pdf)
}
#[test]
fn cpu_rough_metal_furnace_uses_the_opt_in_for_captured_materials() {
    let legacy = path_fixture(1024, 1, false);
    let mut scene = legacy.scene;
    scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap());
    for enabled in [false, true] {
        let options = RaytraceOptions { metal_energy_compensation: enabled, ..legacy.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..legacy.request.clone() };
        let task = ImageTask::new(scene.clone(), request, CancelToken::default(), options).unwrap();
        let (mean, _) = path_mean(task);
        // Independent analytic result for a white roughness-one GGX substrate
        // at normal incidence. With compensation, a white furnace returns one.
        let expected = if enabled { 1.0 } else { 1.0 - 2.0_f64.ln() };
        eprintln!("metal compensation={enabled}: furnace={mean:?}");
        for channel in mean {
            assert!((channel - expected).abs() < 0.035, "enabled={enabled}: {mean:?} vs {expected}");
        }
    }
}

#[test]
fn cpu_path_real_furnace_and_colored_bounce_match_independent_per_ordinal_estimators() {
    use std::f64::consts::PI;
    let samples = 1024;
    for depth in [1, 2] {
        let mut expected = 0.0;
        for ordinal in 0..samples {
            let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000);
            let z = 1.0 - 2.0 * u;
            if z > 0.0 {
                let light_pdf = 1.0 / (4.0 * PI);
                let bsdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
                let weight = if depth == 1 { 1.0 } else { light_pdf * light_pdf / (light_pdf * light_pdf + bsdf * bsdf) };
                expected += 2.0 * z * weight;
            }
            if depth == 2 {
                let (cosine, pdf) = independent_lambert_scatter(ordinal);
                if pdf > 0.0 {
                    let light_pdf = 1.0 / (4.0 * PI);
                    expected += 0.5 * cosine / (PI * pdf) * (pdf * pdf / (pdf * pdf + light_pdf * light_pdf));
                }
            }
        }
        expected /= f64::from(samples);
        let (actual, _) = path_mean(path_fixture(samples, depth, false));
        for value in actual {
            assert!((value - expected).abs() < 1e-11, "depth{depth}: {value} vs independent estimator{expected}");
        }
        assert!((expected - 0.5).abs() < 0.1, "Lambert furnace expected0.5; fixed-seed bounded estimate{expected}");
    }
    let receiver = |depth| {
        let mut task = path_fixture(samples, depth, true);
        for _ in 0..100_000 {
            if task.bvh.is_some() {
                break;
            }
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        }
        let bvh = task.bvh.as_ref().expect("actual colored-bounce BVH must be ready before lighting");
        let prepared = PreparedScene { bvh, primitives: &task.looks, source: &task.scene };
        // Read-only diagnostics use a separate budget: neither film ordinals nor
        // the producer's cumulative work are consumed by these hit assertions.
        let mut work = WorkBudget::new(100_000).unwrap();
        let mut secondary_hits = 0;
        for ordinal in 0..samples {
            let [dx, dy] = progressive::variates(9, 0, ordinal, 0);
            let ray = primary_ray(&task.request, dx, dy, task.clip).unwrap();
            let aperture = ray.origin() + ray.direction() * ((20.0 - ray.origin().z) / ray.direction().z);
            assert!(aperture.x.abs() < 0.5 && aperture.y.abs() < 0.5, "primary must pass through the real ceiling opening");
            let hit = bvh.closest_hit(&ray, &mut work, &task.cancel).unwrap().expect("camera must see the receiver");
            let hit = prepared.resolve_hit(ray, hit).unwrap();
            assert_eq!(hit.body_index, 0, "primary must see the receiver, never its red ceiling");
            assert_eq!(hit.body_name, "Black half");
            assert!(hit.front_face);
            assert_eq!(hit.geometric_normal, Vec3::Z);
            assert_eq!(hit.shading_normal, Vec3::Z);
            assert_eq!(hit.position.z, 0.0);
            let [component, x] = progressive::variates(9, 0, ordinal, 0x5041_5448_3000);
            let [y, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_3001);
            let secondary =
                super::super::indirect::sample(hit.surface, hit.shading_normal, hit.oriented_geometric_normal(), -ray.direction(), [component, x, y])
                    .unwrap();
            assert_eq!(secondary.is_some(), independent_lambert_scatter(ordinal).1 > 0.0, "the fixed oracle retains every null proposal");
            if let Some(secondary) = secondary {
                let ray = hit.spawn(secondary.direction).unwrap();
                let second = bvh
                    .closest_hit(&ray, &mut work, &task.cancel)
                    .unwrap()
                    .expect("every accepted secondary must see the red ceiling outside its opening");
                let second = prepared.resolve_hit(ray, second).unwrap();
                assert_eq!(second.body_index, 1);
                assert_eq!(second.body_name, "Red ceiling");
                assert!(second.front_face);
                assert_eq!(second.geometric_normal, -Vec3::Z);
                assert_eq!(second.shading_normal, -Vec3::Z);
                assert!(second.vertices.iter().all(|vertex| vertex.z == 20.0), "the secondary hit belongs to the exact ceiling plane");
                assert!(second.position.x.abs() > 0.5 || second.position.y.abs() > 0.5);
                secondary_hits += 1;
            }
        }
        assert_eq!(secondary_hits, 771, "the fixed seed exercises the complete accepted continuation set without retries");
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0, "no lighting sample was consumed by preparation or diagnostics");
        assert!(task.options.environment.is_none());
        assert!(task.options.lights.as_ref().unwrap().is_empty());
        assert!(task.options.emissions.as_ref().unwrap().iter().all(|emitter| emitter.body_index != 0));
        task
    };
    let direct = path_mean(receiver(1)).0;
    assert_eq!(direct, [0.0; 3], "no direct source illuminates the receiver");
    let indirect = path_mean(receiver(2)).0;
    let expected = (0..samples)
        .map(|ordinal| {
            let (cosine, pdf) = independent_lambert_scatter(ordinal);
            if pdf == 0.0 { 0.0 } else { cosine / (PI * pdf) }
        })
        .sum::<f64>()
        / f64::from(samples);
    assert!((indirect[0] - expected).abs() < 1e-11, "actual neighbor emission must travel through diffuse continuation");
    assert_eq!(indirect[1..], [0.0, 0.0]);
    assert!((expected - 1.0).abs() < 0.12);
}
#[test]
fn cpu_path_real_prefixes_quanta_admission_and_legacy_omission_are_stable() {
    let output = job::OutputOptions { linear_exr: true, ..Default::default() };
    let make = |samples| {
        let original = path_fixture(samples, 3, false);
        let options = RaytraceOptions { output: Some(Box::new(output)), ..original.options };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, original.cancel, options).unwrap()
    };
    let actual = complete_presentation(make(7), 1);
    assert_eq!(actual, complete_presentation(make(7), 16384));
    for (samples, png) in &actual.0 {
        assert_eq!(*png, complete_presentation(make(*samples), 3).1);
    }
    assert!(actual.2.is_some());
    let task = make(3);
    assert_eq!(task.path.capacity(), 1);
    let options = RaytraceOptions { path: None, ..task.options.clone() };
    let request = ImageRequest { raytrace: Some(options.clone()), ..task.request.clone() };
    let old = ImageTask::new(task.scene.clone(), request, CancelToken::default(), options).unwrap();
    assert_eq!(old.path.capacity(), 0);
    assert_eq!(task.admission.estimated_total_bytes - old.admission.estimated_total_bytes, std::mem::size_of::<indirect_image::State>() as u64);
    let omitted: RaytraceOptions = serde_json::from_str("{}").unwrap();
    assert!(omitted.path.is_none());
    assert!(serde_json::to_value(&omitted).unwrap().get("path").is_none());
    assert_eq!(complete_presentation(old, 1), {
        let mut wire = serde_json::to_value(&task.options).unwrap();
        wire.as_object_mut().unwrap().remove("path");
        let options: RaytraceOptions = serde_json::from_value(wire).unwrap();
        let request = ImageRequest { raytrace: Some(options.clone()), ..task.request };
        complete_presentation(ImageTask::new(task.scene, request, CancelToken::default(), options).unwrap(), 16384)
    });
}
#[test]
fn cpu_path_cancellation_and_exhaustion_never_commit_an_incomplete_camera_path() {
    for stage in [
        "new",
        "trace",
        "emitters",
        "resolve",
        "effects",
        "emission",
        "directional",
        "punctual",
        "area",
        "environment",
        "shadow",
        "scatter",
        "commit",
    ] {
        for cancel in [false, true] {
            let mut task = path_fixture(7, 3, false);
            for _ in 0..200_000 {
                if task.bvh.is_some() && task.path.first().is_some_and(|p| p.stage_name() == stage) {
                    break;
                }
                assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
            }
            assert_eq!(task.path.first().unwrap().stage_name(), stage);
            let before = task.work.used();
            let samples = task.progressive.as_ref().unwrap().film.samples();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = before;
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), before);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), samples);
            assert!(task.take_linear_exr().is_none());
            assert!(!task.done);
        }
    }
}

#[test]
fn cpu_path_real_rectangular_emitter_and_blocker_have_the_analytic_lambert_mean() {
    let expected = 0.5 * 4.0 / 5.0_f64.sqrt() * (1.0 / 5.0_f64.sqrt()).atan() / std::f64::consts::PI;
    for direct_samples in [1, 4, 16] {
        // z=2 is the exact emitter endpoint: opaque geometry wins BSDF
        // distance ties, so direct visibility must include that endpoint too.
        for blocker_height in [None, Some(1.0), Some(2.0)] {
            let original = path_fixture(1024, 2, false);
            let mut scene = original.scene;
            if let Some(height) = blocker_height {
                let mut occluder = scene.meshes[0].clone();
                occluder.name = "Black blocker".into();
                let mut mesh = (*occluder.mesh).clone();
                for p in &mut mesh.positions {
                    p.z = height;
                }
                occluder.mesh = Arc::new(mesh);
                occluder.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap());
                scene.meshes.push(occluder);
                scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
            }
            let options = RaytraceOptions {
                environment: None,
                area_lights: Some(vec![job::RectangleLightOptions {
                    center_mm: [0.0, 0.0, 2.0],
                    edge_u_mm: [2.0, 0.0, 0.0],
                    edge_v_mm: [0.0, -2.0, 0.0],
                    color: [1.0; 3],
                    intensity: 1.0,
                }]),
                area_samples: Some(direct_samples),
                seed: Some(81),
                ..original.options
            };
            let camera = crate::Camera { distance: 0.1, fov: 0.001, ..original.request.camera };
            let request = ImageRequest { camera, raytrace: Some(options.clone()), ..original.request };
            let (actual, _) = path_mean(ImageTask::new(scene, request, CancelToken::default(), options).unwrap());
            if blocker_height.is_some() {
                assert_eq!(actual, [0.0; 3], "black geometry at {blocker_height:?} blocks both strategies, including endpoint ties");
            } else {
                for channel in actual {
                    assert!((channel - expected).abs() < 0.02, "N={direct_samples}, real rectangle MIS {channel}, analytic {expected}");
                }
            }
        }
    }
}
#[test]
fn cpu_path_refuses_mixed_transport_instead_of_erasing_selected_optical_data() {
    let original = path_fixture(3, 2, false);
    let reflection =
        ReflectionOptions { bodies: vec![ReflectionBody { body: "Black half".into(), mode: ReflectionMode::Mirror }], samples: 1, max_bounces: 2 };
    let options = RaytraceOptions { reflections: Some(reflection.clone()), ..original.options.clone() };
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    assert!(admit(&original.scene, &request, &options).unwrap_err().message.contains("selected reflection"));
    assert_eq!(options.reflections, Some(reflection));
    for depth in [0, job::MAX_PATH_DEPTH + 1] {
        assert!(RaytraceOptions { path: Some(job::CpuPathOptions { max_depth: depth }), ..original.options.clone() }.validate().is_err());
    }
    assert!(RaytraceOptions { progressive: None, ..original.options.clone() }.validate().unwrap_err().message.contains("progressive"));
    assert!(RaytraceOptions { shadows: false, ..original.options.clone() }.validate().unwrap_err().message.contains("shadows"));
}

#[test]
fn cpu_path_overlapping_coplanar_emitters_refuse_without_mutating_authored_rows() {
    let original = path_fixture(3, 2, false);
    let first = job::RectangleLightOptions {
        center_mm: [0.0, 0.0, 2.0],
        edge_u_mm: [2.0, 0.0, 0.0],
        edge_v_mm: [0.0, -2.0, 0.0],
        color: [1.0; 3],
        intensity: 1.0,
    };
    let rows: Vec<_> = (0..5).map(|index| job::RectangleLightOptions { center_mm: [f64::from(index) * 3.0, 0.0, 2.0], ..first.clone() }).collect();
    let options = RaytraceOptions { area_lights: Some(rows.clone()), ..original.options.clone() };
    let error = options.validate().unwrap_err();
    assert_eq!(error.code, job::ErrorCode::InvalidRequest);
    assert!(error.message.contains("at most four rectangle lights"), "{}", error.message);
    assert_eq!(options.area_lights, Some(rows), "count refusal retains every authored source");
    for center in [[0.0, 0.0, 2.0], [1.0, 0.0, 2.0]] {
        let second = job::RectangleLightOptions { center_mm: center, ..first.clone() };
        let rows = vec![first.clone(), second];
        let options = RaytraceOptions { area_lights: Some(rows.clone()), ..original.options.clone() };
        let error = options.validate().unwrap_err();
        assert!(error.message.contains("overlapping"));
        assert_eq!(options.area_lights, Some(rows), "refusal retains both authored sources");
    }
    for center in [[2.0, 0.0, 2.0], [3.0, 0.0, 2.0], [0.0, 0.0, 3.0]] {
        let options = RaytraceOptions {
            area_lights: Some(vec![first.clone(), job::RectangleLightOptions { center_mm: center, ..first.clone() }]),
            ..original.options.clone()
        };
        assert!(options.validate().is_ok(), "disjoint or edge-adjacent supports remain valid");
    }
    let opposite = job::RectangleLightOptions { edge_v_mm: [0.0, 2.0, 0.0], ..first.clone() };
    assert!(
        RaytraceOptions { area_lights: Some(vec![first, opposite]), ..original.options }.validate().is_ok(),
        "opposite one-sided supports illuminate disjoint hemispheres"
    );
}

#[test]
fn cpu_path_real_hdr_source_counts_match_independent_per_ordinal_mis_and_texel_lookup() {
    use crate::lighting::{HdrSource, environment_input::EnvironmentInput};
    use std::f64::consts::{PI, TAU};
    // Deliberately nonconstant RGBE cells: the reference selects a texel from the
    // physical direction independently of Environment::sample/radiance/PDF.
    let texels: [[u8; 4]; 8] = [
        [128, 32, 16, 129],
        [16, 128, 32, 129],
        [32, 16, 128, 129],
        [64, 96, 128, 129],
        [96, 64, 32, 129],
        [32, 96, 64, 129],
        [64, 32, 96, 129],
        [128, 64, 96, 129],
    ];
    let radiance = |direction: Vec3| {
        let phi = direction.y.atan2(direction.x).rem_euclid(TAU);
        let x = ((phi / TAU * 4.0) as usize).min(3);
        let y = ((direction.z.clamp(-1.0, 1.0).acos() / PI * 2.0) as usize).min(1);
        let pixel = texels[y * 4 + x];
        Vec3::new(f64::from(pixel[0]), f64::from(pixel[1]), f64::from(pixel[2])) / 128.0
    };
    let samples = 128;
    for source_count in [1, 4, 16, 64] {
        let side = match source_count {
            1 => 1,
            4 => 2,
            16 => 4,
            _ => 8,
        };
        for depth in [1, 2] {
            let mut expected = Vec3::ZERO;
            for ordinal in 0..samples {
                for source_sample in 0..source_count {
                    let [dx, dy] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000 + u64::from(source_sample));
                    let u = ((f64::from(source_sample / side) + dx) / f64::from(side)).max(0.0_f64.next_up()).min(1.0_f64.next_down());
                    let v = ((f64::from(source_sample % side) + dy) / f64::from(side)).max(0.0_f64.next_up()).min(1.0_f64.next_down());
                    let z = 1.0 - 2.0 * u;
                    if z > 0.0 {
                        let phi = TAU * v;
                        let radius = ((1.0 - z) * (1.0 + z)).sqrt();
                        let direction = Vec3::new(radius * phi.cos(), radius * phi.sin(), z);
                        let source_pdf = f64::from(source_count) / (4.0 * PI);
                        let bsdf_pdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
                        let weight = if depth == 1 { 1.0 } else { source_pdf * source_pdf / (source_pdf * source_pdf + bsdf_pdf * bsdf_pdf) };
                        expected += radiance(direction) * (2.0 * z * weight / f64::from(source_count));
                    }
                }
                if depth == 2 {
                    let (z, pdf) = independent_lambert_scatter(ordinal);
                    if pdf > 0.0 {
                        let [angle, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_3001);
                        let phi = TAU * angle;
                        let radius = (1.0 - z * z).sqrt();
                        let direction = Vec3::new(radius * phi.cos(), radius * phi.sin(), z);
                        let source_pdf = f64::from(source_count) / (4.0 * PI);
                        let weight = pdf * pdf / (pdf * pdf + source_pdf * source_pdf);
                        expected += radiance(direction) * (0.5 * z / (PI * pdf) * weight);
                    }
                }
            }
            expected = expected / f64::from(samples);
            let original = path_fixture(samples, depth, false);
            let options = RaytraceOptions { environment: None, ..original.options };
            let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
            let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
            for pixel in texels {
                bytes.extend_from_slice(&pixel);
            }
            let input = EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.0, 1.0, source_count).unwrap();
            let (actual, _) =
                path_mean(ImageTask::new_with_environment(original.scene, request, CancelToken::default(), options, Some(input)).unwrap());
            for (actual, expected) in actual.into_iter().zip([expected.x, expected.y, expected.z]) {
                assert!((actual - expected).abs() < 1e-11, "HDR N={source_count}, depth={depth}: {actual} vs independent {expected}");
            }
        }
    }
}

#[test]
fn composed_cpu_path_alpha_and_denoise_keep_analytic_matte_all_prefixes_and_raw_master() {
    let output = job::OutputOptions { linear_exr: true, ..Default::default() };
    let make = |samples, transparent, denoise, path: bool| {
        let original = alpha_fixture(Some(samples), 1, transparent, Some(output), 1.5);
        let options =
            RaytraceOptions { path: path.then_some(job::CpuPathOptions { max_depth: 1 }), denoise, shadows: true, ..original.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        ImageTask::new(original.scene, request, original.cancel, options).unwrap()
    };
    let actual = complete_presentation(make(7, true, true, true), 1);
    assert_eq!(actual, complete_presentation(make(7, true, true, true), 16384));
    assert_eq!(actual.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    let rgb = [9.0, 4.5, 2.25].map(|radiance| {
        let exposed = radiance * 2.0f64.powf(1.5);
        encode8(exposed / (1.0 + exposed))
    });
    for (samples, png) in actual.0.iter().chain(std::iter::once(&(7, actual.1.clone()))) {
        let covered = (0..*samples).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
        let alpha = (covered as f64 / f64::from(*samples) * 255.0).round() as u8;
        assert_eq!(alpha_pixel(png), if alpha == 0 { [0; 4] } else { [rgb[0], rgb[1], rgb[2], alpha] });
        assert_eq!(*png, complete_presentation(make(*samples, true, true, true), 3).1);
    }
    let unfiltered = complete_presentation(make(7, true, false, true), 1);
    let opaque = complete_presentation(make(7, false, true, true), 1);
    let direct = complete_presentation(make(7, true, true, false), 1);
    assert_eq!(actual.2, unfiltered.2, "filtering cannot enter the GI raw companion");
    assert_eq!(actual.2, opaque.2, "background removal cannot enter the GI raw companion");
    assert_eq!(actual.2, direct.2, "a camera-visible emitter has the same raw radiance and coverage in either transport");
    assert_eq!((actual.0, actual.1), (unfiltered.0, unfiltered.1), "the partial-coverage emitter bypass keeps the analytic straight matte");
}

fn composed_panel_fixture(samples: u32, denoise: bool, path: bool, panel_z: f64, front: bool) -> ImageTask {
    let original = path_fixture(samples, 1, false);
    let point = |x, y| {
        let (origin, _) = original.request.camera.ray(x, y, 13.0, 13.0);
        Vec3::new(origin.x, origin.y, panel_z)
    };
    let corner = point(5.0, 2.0);
    let u = point(8.0, 2.0) - corner;
    let mut v = point(5.0, 11.0) - corner;
    let center = corner + (u + v) * 0.5;
    if (u.cross(v).z > 0.0) != front {
        v = -v;
    }
    let array = |p: Vec3| [p.x, p.y, p.z];
    let lights = (0..4)
        .map(|index| job::RectangleLightOptions {
            center_mm: array(center + Vec3::X * (100.0 * f64::from(index))),
            edge_u_mm: array(u),
            edge_v_mm: array(v),
            color: [0.5, 0.4, 0.3],
            intensity: 1.0,
        })
        .collect();
    let options = RaytraceOptions {
        denoise,
        path: path.then_some(job::CpuPathOptions { max_depth: 1 }),
        area_lights: Some(lights),
        area_samples: Some(1),
        seed: Some(31),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..original.options
    };
    let request = ImageRequest { width: 13, height: 13, raytrace: Some(options.clone()), ..original.request };
    ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
}
fn composed_panel_guides(mut task: ImageTask) -> (Vec<job::denoise::Guide>, u64) {
    for _ in 0..300_000 {
        if task.denoise.first().is_some_and(|guides| guides.ready()) {
            assert_eq!(task.pixel, 0);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            return (task.denoise.first().unwrap().pixels().unwrap().to_vec(), task.work.used());
        }
        let guiding = task.bvh.is_some();
        let before = task.work.used();
        assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        if guiding {
            assert_eq!(task.work.used() - before, 1, "each guide ray/traversal/source/effect/store continuation owns one work unit");
        }
    }
    panic!("bounded composed guide prepass did not finish");
}
#[test]
fn composed_path_denoise_guides_bypass_visible_analytic_emitters_before_backing_geometry() {
    let (guides, work) = composed_panel_guides(composed_panel_fixture(4, true, true, 2.0, true));
    for y in 0..13 {
        for x in 0..13 {
            assert_eq!(guides[y * 13 + x].body == 0, (5..8).contains(&x) && (2..11).contains(&y), "guide ({x},{y})");
        }
    }
    let (legacy, old_work) = composed_panel_guides(composed_panel_fixture(4, true, false, 2.0, true));
    assert!(legacy.iter().all(|guide| guide.body == 1), "absent path keeps analytic lights invisible to the first-hit prepass");
    assert_eq!(
        work - old_work,
        13 * 13 * 4 - 3 * 9,
        "four source scans per pixel; each of the27 bypassed panel pixels skips the underlying body-guide resolution"
    );
    let original = composed_panel_fixture(4, true, false, 2.0, true);
    let options = RaytraceOptions { area_lights: None, area_samples: None, seed: None, ..original.options };
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
    let no_lights = composed_panel_guides(ImageTask::new(original.scene, request, original.cancel, options).unwrap());
    assert_eq!(no_lights.1, old_work, "legacy guide work does not scan direct-only analytic lights");
    for (z, front) in [(0.0, true), (-2.0, true), (2.0, false)] {
        let (guides, _) = composed_panel_guides(composed_panel_fixture(4, true, true, z, front));
        assert!(
            guides.iter().all(|guide| guide.body == 1),
            "geometry wins exact ties, nearer body hits and back-facing emitter supports; z={z}, front={front}"
        );
    }
}
#[test]
fn composed_path_denoise_keeps_all_foreground_emitter_pixels_prefixes_and_raw_master_exact() {
    let actual = complete_presentation(composed_panel_fixture(4, true, true, 2.0, true), 1);
    assert_eq!(actual, complete_presentation(composed_panel_fixture(4, true, true, 2.0, true), 16384));
    assert_eq!(actual.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3]);
    let raw = complete_presentation(composed_panel_fixture(4, false, true, 2.0, true), 1);
    assert_eq!(actual.2, raw.2, "emitter bypass and filtering cannot alter raw GI EXR radiance");
    assert_ne!(actual.1, raw.1, "the noisy diffuse backing must actually exercise the filter");
    let rgb = [0.5_f64, 0.4, 0.3].map(|radiance| encode8(radiance / (1.0 + radiance)));
    for ((samples, png), (raw_samples, raw_png)) in
        actual.0.iter().chain(std::iter::once(&(4, actual.1.clone()))).zip(raw.0.iter().chain(std::iter::once(&(4, raw.1.clone()))))
    {
        assert_eq!(samples, raw_samples);
        assert_eq!(*png, complete_presentation(composed_panel_fixture(*samples, true, true, 2.0, true), 3).1);
        let filtered = ::image::load_from_memory(png).unwrap().to_rgba8();
        let unfiltered = ::image::load_from_memory(raw_png).unwrap().to_rgba8();
        for y in 2..11 {
            for x in 5..8 {
                assert_eq!(filtered.get_pixel(x, y), unfiltered.get_pixel(x, y), "sample{samples}, panel({x},{y})");
                assert_eq!(filtered.get_pixel(x, y).0, [rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
}
#[test]
fn composed_path_guide_emitter_scans_cancel_and_exhaust_before_consuming_the_next_source() {
    for cancel in [false, true] {
        let mut task = composed_panel_fixture(4, true, true, 2.0, true);
        for _ in 0..300_000 {
            if task.denoise.first().is_some_and(|guide| guide.emitter_scans() == 2) {
                break;
            }
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        }
        let guide = task.denoise.first().unwrap();
        assert_eq!(guide.emitter_scans(), 2);
        assert!(!guide.ready_to_store(&task.options), "remaining emitter scans precede the actual guide store");
        assert_eq!(guide.completed_pixels(), 0);
        let used = task.work.used();
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = used;
        }
        assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), used);
        let guide = task.denoise.first().unwrap();
        assert_eq!(guide.emitter_scans(), 2);
        assert!(!guide.ready_to_store(&task.options), "remaining emitter scans precede the actual guide store");
        assert_eq!(guide.completed_pixels(), 0);
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
    }
}

// CPU aging uses the existing saved optical-layer schema, but evaluates a world
// field once before sharing one closure with transport and the first-hit guides.
fn aging_stack(layers: &[job::aging::AgingLayer]) -> Box<job::aging::AgingStack> {
    let mut stack = job::aging::AgingStack::default();
    for layer in layers {
        stack.push(*layer).unwrap();
    }
    Box::new(stack)
}
fn solid_age() -> job::aging::AgingLayer {
    job::aging::AgingLayer {
        kind: job::aging::AgingKind::Dust,
        strength: 1.0,
        coverage: 1.0,
        upward_bias: 0.0,
        color_srgb: [255, 0, 0],
        metallic: 0.0,
        roughness: 1.0,
        scale_mm: 100_000.0,
        ..Default::default()
    }
}
fn age_surface(
    layers: &[job::aging::AgingLayer],
    input: crate::surface_patterns::ShadingSurface,
    point: Vec3,
    outward: Vec3,
    eligible: bool,
) -> crate::surface_patterns::ShadingSurface {
    let stack = aging_stack(layers);
    let mut task = super::super::aging::Task::default();
    task.begin(input, point, outward, 0, eligible).unwrap();
    while !task.ready() {
        task.advance(&stack).unwrap();
    }
    task.surface().unwrap()
}
#[test]
fn cpu_aging_all_kinds_match_independent_full_coverage_optical_values_and_keep_normals() {
    let input = crate::surface_patterns::ShadingSurface {
        surface: OpaqueSurface::new(Vec3::new(0.2, 0.4, 0.6), 0.7, 0.3, 1.6).unwrap(),
        shading_normal: Vec3::new(0.0, 0.6, 0.8),
    };
    // Independent integer hash oracle at q=(0,0,0): the two value-noise
    // octaves are exact lattice values, not values from the CPU evaluator.
    let hash = |cell: [u32; 3], seed: u32| {
        let mut h = (cell[0] & 4095).wrapping_mul(0x8da6_b343)
            ^ (cell[1] & 4095).wrapping_mul(0xd816_3841)
            ^ (cell[2] & 4095).wrapping_mul(0xcb1a_b31f)
            ^ seed.wrapping_mul(0x9e37_79b9);
        h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
        h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
        f64::from((h ^ (h >> 16)) >> 8) / 16_777_216.0
    };
    let field = 0.75 * hash([0, 0, 0], 19) + 0.25 * hash([19, 7, 3], 20);
    for kind in job::aging::AgingKind::ALL {
        let layer = job::aging::AgingLayer { kind, ..solid_age() };
        let changed = age_surface(&[layer], input, Vec3::ZERO, Vec3::Z, true);
        assert_eq!(changed.shading_normal, input.shading_normal);
        assert_eq!(changed.surface.dielectric_ior(), 1.6);
        if kind == job::aging::AgingKind::Scratches {
            // Independent band0 line at u=0 lies outside the narrow scratch;
            // this first point is an exact no-op, rather than invented wear.
            let line = ((hash([0, 0, 0], 19) * 0.7).fract() - 0.5).abs();
            assert!(line >= 0.09);
            assert_eq!(changed, input);
        } else {
            let expected = match kind {
                job::aging::AgingKind::Rust | job::aging::AgingKind::Patina | job::aging::AgingKind::Grime => {
                    Vec3::new((0.45 + 0.7 * field).min(1.0), 0.0, 0.0)
                }
                job::aging::AgingKind::Fading => Vec3::new(0.6, 0.2, 0.3),
                _ => Vec3::X,
            };
            assert!((changed.surface.base_color() - expected).len() < 1e-15, "{kind:?}");
            assert_eq!(changed.surface.metallic(), 0.0);
            assert_eq!(changed.surface.roughness(), 1.0);
        }
    }
}
#[test]
fn cpu_aging_uses_original_material_class_layer_order_targets_and_coat_attenuation() {
    let coat = crate::clear_coat::ClearCoat::new(0.8, 0.2, 1.7).unwrap();
    let input = crate::surface_patterns::ShadingSurface {
        surface: OpaqueSurface::new(Vec3::new(0.2, 0.3, 0.4), 1.0, 0.4, 1.6).unwrap().with_clear_coat(coat),
        shading_normal: Vec3::Z,
    };
    let first = job::aging::AgingLayer { strength: 0.25, surface_class: job::aging::SurfaceClass::Metal, ..solid_age() };
    let changed = age_surface(&[first], input, Vec3::ZERO, Vec3::Z, true);
    assert!((changed.surface.base_color() - Vec3::new(0.4, 0.225, 0.3)).len() < 1e-15);
    assert_eq!(changed.surface.metallic(), 0.75);
    let remaining = changed.surface.clear_coat().unwrap();
    assert!((remaining.weight() - 0.6).abs() < 1e-15);
    assert_eq!((remaining.roughness(), remaining.ior()), (0.2, 1.7));
    let original_class = [
        job::aging::AgingLayer { surface_class: job::aging::SurfaceClass::Metal, ..solid_age() },
        job::aging::AgingLayer { surface_class: job::aging::SurfaceClass::Dielectric, color_srgb: [0, 0, 255], ..solid_age() },
        job::aging::AgingLayer { surface_class: job::aging::SurfaceClass::Metal, color_srgb: [0, 255, 0], ..solid_age() },
    ];
    assert_eq!(
        age_surface(&original_class, input, Vec3::ZERO, Vec3::Z, true).surface.base_color(),
        Vec3::Y,
        "class selection uses original metallic, after earlier layers lower the current value"
    );
    let other = job::aging::AgingLayer {
        target: Some(job::aging::BodyTarget { body_index: 1, body_name: job::aging::BodyName::new("other").unwrap() }),
        ..solid_age()
    };
    assert_eq!(age_surface(&[other], input, Vec3::ZERO, Vec3::Z, true), input);
    assert_eq!(age_surface(&[solid_age()], input, Vec3::ZERO, Vec3::Z, false), input, "glass/stage bypass is exact");
    let down = job::aging::AgingLayer { upward_bias: 1.0, ..solid_age() };
    assert_eq!(age_surface(&[down], input, Vec3::ZERO, -Vec3::Z, true), input, "use original winding, not face-forward shading normal");
}
#[test]
fn cpu_aging_world_coordinates_are_camera_independent_and_keep_scratch_band_identity() {
    let input = crate::surface_patterns::ShadingSurface { surface: OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap(), shading_normal: Vec3::Z };
    let layer = job::aging::AgingLayer { kind: job::aging::AgingKind::Scratches, scale_mm: 2.0, ..solid_age() };
    let point = Vec3::new(0.009380418558915457, 5.75, 0.0);
    let expected = age_surface(&[layer], input, point, Vec3::Z, true);
    assert!(expected.surface.base_color().x > 0.1, "the anchor oracle must exercise a visible scratch");
    // Move world origin and hit together; the exact f64 millimetre frame is the
    // same, even across the GPU noise-rebase boundary (which Scratch bands do not share).
    let offset = Vec3::new(8192.0, 8192.0, 8192.0);
    let shifted = job::aging::AgingLayer { origin_mm: [offset.x, offset.y, offset.z], ..layer };
    let moved = age_surface(&[shifted], input, point + offset, Vec3::Z, true);
    assert!((moved.surface.base_color() - expected.surface.base_color()).len() < 1e-10);
    let mut another = super::super::aging::Task::default();
    another.begin(input, point, Vec3::Z, 0, true).unwrap();
    another.advance(&aging_stack(&[layer])).unwrap();
    assert_eq!(another.surface().unwrap(), expected, "there is no camera argument or frame-local field origin");
}
fn aging_image(samples: u32, path: bool, denoise: bool, layers: &[job::aging::AgingLayer]) -> ImageTask {
    let original = path_fixture(samples, 2, false);
    let options = RaytraceOptions {
        aging: (!layers.is_empty()).then(|| aging_stack(layers)),
        denoise,
        path: path.then_some(job::CpuPathOptions { max_depth: 2 }),
        lights: (!path)
            .then(|| vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: std::f64::consts::PI }])
            .or_else(|| Some(vec![])),
        environment: path.then_some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..original.options
    };
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
    ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_aging_actual_direct_png_and_raw_master_have_analytic_lambert_response() {
    let result = complete_presentation(aging_image(5, false, false, &[solid_age()]), 1);
    assert_eq!(result.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3, 4]);
    for png in result.0.iter().map(|p| &p.1).chain(std::iter::once(&result.1)) {
        assert_eq!(::image::load_from_memory(png).unwrap().to_rgba8().get_pixel(0, 0).0, [encode8(0.5), 0, 0, 255]);
    }
    let raw = native_exr_pixels(result.2.as_deref().unwrap());
    assert_eq!(raw.get_pixel(0, 0).0, [1.0, 0.0, 0.0, 1.0]);
}
#[test]
fn cpu_aging_actual_path_uses_aged_response_pdf_and_scatter_for_every_prefix_and_quantum() {
    use std::f64::consts::PI;
    let samples = 64;
    let actual = path_mean(aging_image(samples, true, false, &[solid_age()])).0;
    let mut expected = 0.0;
    for ordinal in 0..samples {
        let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000);
        let z = 1.0 - 2.0 * u;
        let light_pdf = 1.0 / (4.0 * PI);
        if z > 0.0 {
            let bsdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
            expected += 4.0 * z * light_pdf * light_pdf / (light_pdf * light_pdf + bsdf * bsdf);
        }
        let (cosine, pdf) = independent_lambert_scatter(ordinal);
        if pdf > 0.0 {
            expected += cosine / (PI * pdf) * pdf * pdf / (pdf * pdf + light_pdf * light_pdf);
        }
    }
    expected /= f64::from(samples);
    assert!((actual[0] - expected).abs() < 1e-11);
    assert_eq!(actual[1..], [0.0, 0.0]);
    let layer = job::aging::AgingLayer { coverage: 0.5, ..solid_age() };
    let small = complete_presentation(aging_image(5, true, false, &[layer]), 1);
    assert_eq!(small, complete_presentation(aging_image(5, true, false, &[layer]), 16384));
    for (samples, png) in &small.0 {
        assert_eq!(*png, complete_presentation(aging_image(*samples, true, false, &[layer]), 3).1);
    }
}
#[test]
fn cpu_aging_guides_see_the_aged_material_and_filter_never_changes_raw_intent() {
    // The public constructor's minimum is0.045; it is still strictly inside
    // the smooth-metal guide bypass range (<0.25), before aging changes it.
    let smooth = OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 1.0, 0.045, 1.0).unwrap();
    assert_eq!(smooth.roughness(), crate::surface::MIN_ROUGHNESS);
    let mut pristine = aging_image(5, true, true, &[]);
    pristine.scene.meshes[0].surface = Some(smooth);
    let (unaged_guides, _) = composed_panel_guides(pristine);
    assert_eq!(unaged_guides.len(), 1);
    assert_eq!(unaged_guides[0].body, 0, "the valid unaged smooth-metal fixture actually bypasses denoising");
    let mut task = aging_image(5, true, true, &[solid_age()]);
    task.scene.meshes[0].surface = Some(smooth);
    let (guides, _) = composed_panel_guides(task);
    assert_eq!(guides.len(), 1);
    assert_eq!(guides[0].body, 1, "aging removes smooth-metal bypass before guides are captured");
    assert_eq!(guides[0].albedo, [1.0, 0.0, 0.0]);
    let filtered = complete_presentation(aging_image(5, true, true, &[solid_age()]), 1);
    let raw = complete_presentation(aging_image(5, true, false, &[solid_age()]), 16384);
    assert_eq!(filtered.2, raw.2, "the denoiser only sees the copied pass, never raw EXR/film");
    assert_eq!(filtered, complete_presentation(aging_image(5, true, true, &[solid_age()]), 16384));
}
#[test]
fn cpu_aging_absent_and_inactive_layers_keep_old_png_work_and_exact_admission_deltas() {
    let layer = job::aging::AgingLayer { enabled: false, ..solid_age() };
    let inactive = aging_image(5, false, false, &[layer]);
    let absent = aging_image(5, false, false, &[]);
    assert_eq!(inactive.aging.capacity(), 0);
    assert_eq!(
        inactive.admission.estimated_total_bytes - absent.admission.estimated_total_bytes,
        2 * std::mem::size_of::<job::aging::AgingStack>() as u64
    );
    assert_eq!(complete_presentation(inactive, 1), complete_presentation(absent, 16384));
    let active = aging_image(5, false, false, &[solid_age()]);
    let inactive = aging_image(5, false, false, &[layer]);
    assert_eq!(active.aging.capacity(), 1);
    assert_eq!(active.admission.estimated_total_bytes - inactive.admission.estimated_total_bytes, super::super::aging::Task::required_bytes());
    assert_eq!(
        active.options.retained_aging_bytes().unwrap(),
        std::mem::size_of::<job::aging::AgingStack>() as u64,
        "Record separately charges its retained options box"
    );
    assert!(serde_json::to_value(RaytraceOptions::default()).unwrap().get("aging").is_none());
}
#[test]
fn cpu_aging_cancel_and_work_exhaustion_do_not_advance_a_layer_or_commit_a_pass() {
    for path in [false, true] {
        for guides in [false, true] {
            for cancel in [false, true] {
                let mut task = aging_image(5, path, guides, &[solid_age(), solid_age(), solid_age()]);
                for _ in 0..100_000 {
                    if task.aging.first().is_some_and(|aging| aging.started() && aging.cursor() == 1 && !aging.ready()) {
                        break;
                    }
                    assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
                }
                assert_eq!(task.aging.first().unwrap().cursor(), 1);
                let used = task.work.used();
                if cancel {
                    task.cancel.cancel();
                } else {
                    task.work = WorkBudget::new(used).unwrap();
                    task.work.charge(used, &task.cancel).unwrap();
                }
                assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
                assert_eq!(task.aging.first().unwrap().cursor(), 1);
                assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
                assert!(!task.done);
            }
        }
    }
}

#[test]
fn cpu_aging_limits_and_incompatible_authored_materials_refuse_without_dropping_intent() {
    let original = aging_image(3, false, false, &[solid_age()]);
    let validate = |options: RaytraceOptions| {
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        admit(&original.scene, &request, &options).unwrap_err().to_string()
    };
    let mut options = original.options.clone();
    options.aging.as_mut().unwrap().get_mut(0).unwrap().target =
        Some(job::aging::BodyTarget { body_index: 0, body_name: job::aging::BodyName::new("replaced").unwrap() });
    assert!(validate(options).contains("identity changed"));
    let mut options = original.options.clone();
    options.aging.as_mut().unwrap().get_mut(0).unwrap().scale_mm = 0.01;
    assert!(validate(options).contains("4096 repeats"));
    let mut options = original.options.clone();
    options.aging.as_mut().unwrap().get_mut(0).unwrap().strength = f64::NAN;
    assert!(options.validate().is_err());
    let mut options = original.options.clone();
    options.anisotropic_faces[0] = Some(job::AnisotropicFaceOptions {
        body_index: 0,
        body_name: original.scene.meshes[0].name.clone(),
        face: 0,
        tangent: [1.0, 0.0, 0.0],
        orientation: job::AnisotropicOrientation::Zero,
        roughness_x: 0.2,
        roughness_y: 0.6,
    });
    let before = options.clone();
    assert!(validate(options.clone()).contains("anisotropic roughness axes"));
    assert_eq!(options, before);
    let mut options = original.options.clone();
    options.dielectrics.push(job::DielectricOptions {
        model: None,
        shadow_transmission: false,
        subsurface: None,
        body_index: 0,
        body_name: original.scene.meshes[0].name.clone(),
        boundary: job::DielectricBoundary::ThinSheet,
        ior: 1.5,
        absorption_per_mm: [0.0; 3],
        thickness_mm: 1.0,
    });
    options.aging.as_mut().unwrap().get_mut(0).unwrap().target =
        Some(job::aging::BodyTarget { body_index: 0, body_name: job::aging::BodyName::new(&original.scene.meshes[0].name).unwrap() });
    assert!(validate(options.clone()).contains("explicit certified dielectric"));
    assert!(!super::super::aging::eligible(&options, 0, &original.scene.meshes[0].name));
    assert!(!super::super::aging::eligible(&RaytraceOptions::default(), 1, "__render_stage__/floor"));
    let mut metallic_scene = original.scene.clone();
    metallic_scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::X, 1.0, 0.5, 1.0).unwrap());
    let mut options = original.options.clone();
    options.reflections = Some(ReflectionOptions {
        bodies: vec![ReflectionBody { body: metallic_scene.meshes[0].name.clone(), mode: ReflectionMode::RoughSpecular }],
        samples: 1,
        max_bounces: 2,
    });
    assert!(options.validate_environment_reflections(&metallic_scene).unwrap_err().to_string().contains("diffuse lobe introduced"));
    options.aging.as_mut().unwrap().get_mut(0).unwrap().metallic = 1.0;
    options.validate_environment_reflections(&metallic_scene).unwrap();
    options.aging.as_mut().unwrap().get_mut(0).unwrap().metallic = 0.0;
    options.aging.as_mut().unwrap().get_mut(0).unwrap().surface_class = job::aging::SurfaceClass::Dielectric;
    options.validate_environment_reflections(&metallic_scene).unwrap();
}

#[test]
fn cpu_aging_point_zero_two_is_preserved_before_real_path_mis_and_sharp_metal_guide_bypass() {
    use std::f64::consts::PI;
    let layer = job::aging::AgingLayer { roughness: 0.02, metallic: 1.0, color_srgb: [255; 3], ..solid_age() };
    let input = crate::surface_patterns::ShadingSurface { surface: OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap(), shading_normal: Vec3::Z };
    assert_eq!(age_surface(&[layer], input, Vec3::ZERO, Vec3::Z, true).surface.roughness(), 0.02);
    let a2 = 0.02_f64.powi(4);
    let response_pdf = |cosine: f64| {
        let nh2 = (1.0 + cosine) / 2.0;
        let d = (1.0 - cosine) / 2.0 + nh2 * a2;
        let distribution = a2 / (PI * d * d);
        let g1 = 2.0 * cosine / (cosine + (a2 + (1.0 - a2) * cosine * cosine).sqrt());
        (distribution * g1 / 4.0, 0.5 * cosine / PI + 0.5 * distribution / 4.0)
    };
    let samples = 64;
    let light_pdf = 1.0 / (4.0 * PI);
    let mut expected = 0.0;
    for ordinal in 0..samples {
        let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000);
        let cosine = 1.0 - 2.0 * u;
        if cosine > 0.0 {
            let (response, pdf) = response_pdf(cosine);
            expected += response / light_pdf * light_pdf * light_pdf / (light_pdf * light_pdf + pdf * pdf);
        }
        let [component, u] = progressive::variates(9, 0, ordinal, 0x5041_5448_3000);
        let cosine = if component < 0.5 { (1.0 - u).sqrt() } else { ((1.0 - u) - a2 * u) / ((1.0 - u) + a2 * u) };
        if cosine > 0.0 {
            let (response, pdf) = response_pdf(cosine);
            expected += response / pdf * pdf * pdf / (pdf * pdf + light_pdf * light_pdf);
        }
    }
    expected /= f64::from(samples);
    let actual = path_mean(aging_image(samples, true, false, &[layer])).0;
    for channel in actual {
        assert!((channel - expected).abs() < 1e-11, "actual unclamped path MIS {channel}, independent oracle {expected}");
    }
    let (guides, _) = composed_panel_guides(aging_image(5, true, true, &[layer]));
    assert_eq!(guides[0].body, 0, "the guide sees the sharp aged metal, not the original diffuse body");
    let filtered = complete_presentation(aging_image(5, true, true, &[layer]), 1);
    let unfiltered = complete_presentation(aging_image(5, true, false, &[layer]), 3);
    assert_eq!(filtered.2, unfiltered.2, "unfiltered film remains the raw master");
    assert_eq!((filtered.0, filtered.1), (unfiltered.0, unfiltered.1), "sharp-metal guide bypass is exact");
}
fn aging_reflection(samples: u32) -> ImageTask {
    let original = aging_image(samples, false, false, &[]);
    let options = RaytraceOptions {
        aging: Some(aging_stack(&[job::aging::AgingLayer { roughness: 0.02, metallic: 1.0, color_srgb: [255; 3], ..solid_age() }])),
        reflections: Some(ReflectionOptions {
            bodies: vec![ReflectionBody { body: original.scene.meshes[0].name.clone(), mode: ReflectionMode::RoughSpecular }],
            samples: 1,
            max_bounces: 1,
        }),
        lights: Some(Vec::new()),
        environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        ..original.options
    };
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
    ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn no_aging_legacy_direct_shade_and_film_commit_have_the_existing_two_unit_boundary() {
    let samples = 3;
    let mut task = aging_image(samples, false, false, &[]);
    assert_eq!((task.request.width, task.request.height), (1, 1));
    assert!(task.options.adaptive.is_none());
    assert!(task.options.path.is_none() && task.options.aging.is_none() && task.options.reflections.is_none());
    let mut two_unit_commits = 0;
    for _ in 0..2_000_000 {
        if task.progressive.as_ref().is_some_and(|p| p.film.samples() == samples) {
            let (mean, covered, count) = task.progressive.as_ref().unwrap().film.pixel(0).unwrap();
            assert_eq!((covered, count), (samples, samples));
            // Independent normal-incidence Lambertian reference: rho0.5,
            // F0=0 at IOR1, light radiance PI, cosine1 gives0.5.
            for value in mean {
                assert!((value - 0.5).abs() < 1e-15);
            }
            assert_eq!(two_unit_commits, samples, "one pre-existing shade+film boundary per unaged primary sample");
            return;
        }
        let transport = task.bvh.is_some()
            && task.progressive.as_ref().is_some_and(|p| p.film.ready() && !p.resolving)
            && task.pixel == 0
            && task.output.is_empty()
            && task.encoder.is_none();
        let count_before = transport.then(|| task.progressive.as_ref().unwrap().film.samples());
        let before = task.work.used();
        assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
        if transport {
            let used = task.work.used() - before;
            assert!(used <= 2);
            if used == 2 {
                two_unit_commits += 1;
                assert_eq!(task.pixel, 1);
                assert!(task.pending.is_none());
                assert_eq!(task.progressive.as_ref().unwrap().film.samples(), count_before.unwrap() + 1);
            }
        }
    }
    panic!("bounded unaged legacy fixture did not complete");
}
#[test]
fn cpu_aging_point_zero_two_legacy_reflection_keeps_independent_furnace_all_prefixes_and_quanta() {
    let samples = 17;
    let (actual, _) = path_mean(aging_reflection(samples));
    let a2 = 0.02_f64.powi(4);
    let expected = (0..samples)
        .map(|ordinal| {
            let [u, _] = progressive::variates(9, 0, ordinal, 0x100);
            let cosine = ((1.0 - u) - a2 * u) / ((1.0 - u) + a2 * u);
            if cosine <= 0.0 { 0.0 } else { 2.0 * cosine / (cosine + (a2 + (1.0 - a2) * cosine * cosine).sqrt()) }
        })
        .sum::<f64>()
        / f64::from(samples);
    assert!((expected - 1.0).abs() < 1e-8);
    for value in actual {
        assert!((value - expected).abs() < 1e-12);
    }
    let result = complete_presentation(aging_reflection(samples), 1);
    assert_eq!(result, complete_presentation(aging_reflection(samples), 16384));
    assert_eq!(result.0.len(), 16);
    for (samples, png) in &result.0 {
        assert_eq!(*png, complete_presentation(aging_reflection(*samples), 3).1);
    }
    let raw = native_exr_pixels(result.2.as_deref().unwrap());
    assert_eq!(raw.get_pixel(0, 0).0, [expected as f32, expected as f32, expected as f32, 1.0]);
}

fn cpu_map_png(width: u32, height: u32, values: &[u8]) -> Vec<u8> {
    use ::image::ImageEncoder;
    let mut bytes = Vec::new();
    ::image::codecs::png::PngEncoder::new(&mut bytes).write_image(values, width, height, ::image::ExtendedColorType::Rgba8).unwrap();
    bytes
}
fn cpu_map_binding(body: usize, name: &str, color: Option<Vec<u8>>) -> job::textures::TextureBinding {
    use crate::texture_input::MapRole;
    job::textures::TextureBinding {
        body_index: body,
        body_name: name.into(),
        frame: job::textures::PlanarFrame { origin: [0.0; 3], u: [1e6, 0.0, 0.0], v: [0.0, 1e6, 0.0] },
        normal_strength: 1.0,
        base_color: color.map(|bytes| job::textures::TextureMap::new(bytes, MapRole::BaseColor).unwrap()),
        normal: None,
        roughness: None,
        metallic: None,
        base_color_mode: job::textures::BaseColorMode::Replace,
        flip_v: false,
    }
}
fn cpu_mapped_fixture(samples: u32, path: bool, guides: bool, all_roles: bool) -> ImageTask {
    use crate::texture_input::MapRole;
    let original = path_fixture(samples, if path { 2 } else { 1 }, false);
    let mut scene = original.scene.clone();
    scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 1.0, if all_roles { 1.5 } else { 1.0 }).unwrap());
    let mut binding = cpu_map_binding(0, &scene.meshes[0].name, Some(cpu_map_png(1, 1, &[128, 64, 255, 255])));
    if all_roles {
        binding.roughness = Some(job::textures::TextureMap::new(cpu_map_png(1, 1, &[128, 128, 128, 255]), MapRole::Roughness).unwrap());
        binding.metallic = Some(job::textures::TextureMap::new(cpu_map_png(1, 1, &[64, 64, 64, 255]), MapRole::Metallic).unwrap());
        binding.normal = Some(job::textures::TextureMap::new(cpu_map_png(1, 1, &[128, 255, 255, 255]), MapRole::NormalOpenGl).unwrap());
    }
    let options = RaytraceOptions {
        path: path.then_some(job::CpuPathOptions { max_depth: 2 }),
        denoise: guides,
        lights: Some(if path { vec![] } else { vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 3.0 }] }),
        environment: path.then_some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..original.options
    };
    let request = ImageRequest {
        textures: Some(Arc::new(job::textures::RuntimeTextures::new(vec![binding]).unwrap())),
        raytrace: Some(options.clone()),
        ..original.request
    };
    ImageTask::new(scene, request, original.cancel, options).unwrap()
}
fn cpu_map_linear(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    (if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } as f32) as f64
}
#[test]
fn cpu_filtered_checker_reaches_direct_path_guides_and_raw_master_without_aliasing() {
    use crate::texture_input::MapRole;
    for path in [false, true] {
        let make = |mapped: bool| {
            let original = cpu_mapped_fixture(4, path, !path, false);
            let mut scene = original.scene;
            for p in &mut Arc::make_mut(&mut scene.meshes[0].mesh).positions {
                *p = *p * 1e-6;
            }
            scene.bounds = scene.meshes[0].mesh.bounds();
            let mut request = original.request;
            let mut binding = request.textures.as_ref().unwrap().bindings()[0].clone();
            binding.frame.u = [1.0, 0.0, 0.0];
            binding.frame.v = [0.0, 1.0, 0.0];
            let pixels: Vec<_> = (0..64)
                .flat_map(|index| {
                    let value = if index % 2 == 0 { 0 } else { 255 };
                    [value, value, value, 255]
                })
                .collect();
            binding.base_color = Some(job::textures::TextureMap::new(cpu_map_png(8, 8, &pixels), MapRole::BaseColor).unwrap());
            request.textures = mapped.then(|| Arc::new(job::textures::RuntimeTextures::new(vec![binding]).unwrap().with_filtering(true)));
            let options = RaytraceOptions { texture_filtering: mapped, ..original.options };
            request.raytrace = Some(options.clone());
            ImageTask::new(scene, request, original.cancel, options).unwrap()
        };
        let filtered = complete_presentation(make(true), 1);
        let coarse = complete_presentation(make(true), 16384);
        let constant = complete_presentation(make(false), 1);
        assert_eq!(filtered, coarse, "filter preparation and image output remain quantum independent");
        assert_eq!(filtered.0, constant.0, "every filtered preview equals the independent constant half-reflectance plane");
        assert_eq!(filtered.1, constant.1);
        assert_eq!(filtered.2, constant.2, "the raw master consumes filtered reflectance in direct and GI transport");
    }
}

#[test]
fn cpu_saved_map_all_roles_reach_real_direct_brdf_film_png_and_raw_master() {
    use std::f64::consts::PI;
    let task = cpu_mapped_fixture(4, false, false, true);
    let mut raw = cpu_mapped_fixture(4, false, false, true);
    let target = raw.options.progressive.unwrap().samples_per_pixel;
    while raw.progressive.as_ref().unwrap().film.samples() != target {
        raw.step_with_previews(1).unwrap();
    }
    let (actual, coverage, samples) = raw.progressive.as_ref().unwrap().film.pixel(0).unwrap();
    assert_eq!((coverage, samples), (4, 4));
    // Independent normal-view Cook–Torrance formula, including the symmetric
    // Fresnel substrate attenuation used by the shared optical contract.
    let n = Vec3::new(f64::from(128_f32 / 255.0) * 2.0 - 1.0, 1.0, 1.0).normalized().unwrap();
    let cosine = n.z;
    let rough = f64::from(128_f32 / 255.0);
    let metal = f64::from(64_f32 / 255.0);
    let a2 = rough.powi(4);
    let d = a2 / (PI * ((1.0 - cosine * cosine) + cosine * cosine * a2).powi(2));
    let visibility = 2.0 / (cosine + (a2 + (1.0 - a2) * cosine * cosine).sqrt());
    let expected: [f64; 3] = [128, 64, 255].map(|byte| {
        let base = cpu_map_linear(byte);
        let f0 = 0.04 * (1.0 - metal) + base * metal;
        let f = f0 + (1.0 - f0) * (1.0 - cosine).powi(5);
        let brdf = f0 * d * 0.25 * visibility * visibility + (1.0 - f) * (1.0 - f) * base * (1.0 - metal) / PI;
        brdf * cosine * 3.0
    });
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 1e-12, "mapped closure {a} != independent response {b}");
    }
    let rendered = complete_presentation(task, 1);
    assert_eq!(rendered, complete_presentation(cpu_mapped_fixture(4, false, false, true), 16384));
    assert!(rendered.2.is_some());
    let expected = expected.map(|v| encode8(v / (1.0 + v)));
    assert_eq!(alpha_pixel(&rendered.1), [expected[0], expected[1], expected[2], 255]);
    for (samples, png) in &rendered.0 {
        assert_eq!(*png, complete_presentation(cpu_mapped_fixture(*samples, false, false, true), 3).1);
    }
}
#[test]
fn cpu_saved_map_indirect_brdf_and_pdf_match_independent_per_ordinal_furnace() {
    use std::f64::consts::PI;
    let samples = 1024;
    let mut value = 0.0;
    for ordinal in 0..samples {
        let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000);
        let z = 1.0 - 2.0 * u;
        let lp = 1.0 / (4.0 * PI);
        if z > 0.0 {
            let bp = 0.5 * z / PI + 0.5 / (4.0 * PI);
            value += 4.0 * z * lp * lp / (lp * lp + bp * bp);
        }
        let (z, pdf) = independent_lambert_scatter(ordinal);
        if pdf > 0.0 {
            value += z / (PI * pdf) * pdf * pdf / (pdf * pdf + lp * lp);
        }
    }
    value /= f64::from(samples);
    let (actual, _) = path_mean(cpu_mapped_fixture(samples, true, false, false));
    for (a, byte) in actual.into_iter().zip([128, 64, 255]) {
        let b = cpu_map_linear(byte) * value;
        assert!((a - b).abs() < 1e-11, "mapped MIS {a} != independent {b}");
    }
}
#[test]
fn cpu_saved_maps_are_applied_to_secondary_vertices_before_continuation_sampling() {
    let original = path_fixture(512, 4, true);
    let mapped_color = [cpu_map_linear(32), cpu_map_linear(128), cpu_map_linear(64)];
    let binding = cpu_map_binding(1, &original.scene.meshes[1].name, Some(cpu_map_png(1, 1, &[32, 128, 64, 255])));
    let resources = Arc::new(job::textures::RuntimeTextures::new(vec![binding]).unwrap());
    let mapped = ImageTask::new(
        original.scene.clone(),
        ImageRequest { textures: Some(resources), ..original.request.clone() },
        CancelToken::default(),
        original.options.clone(),
    )
    .unwrap();
    let mut explicit = original.scene.clone();
    explicit.meshes[1].surface = Some(OpaqueSurface::new(Vec3::new(mapped_color[0], mapped_color[1], mapped_color[2]), 0.0, 1.0, 1.0).unwrap());
    let reference = ImageTask::new(explicit, original.request.clone(), CancelToken::default(), original.options.clone()).unwrap();
    let actual = path_mean(mapped).0;
    let expected = path_mean(reference).0;
    // A constant map still evaluates the reference bilinear weights in f64;
    // compare the independent explicit closure at the numerical transport scale.
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-12, "secondary mapped BRDF/PDF/proposal {actual} != explicit material {expected}");
    }
    assert_ne!(actual, path_mean(original).0, "a map on a secondary opaque surface must change the actual multi-bounce result");
}
#[test]
fn cpu_saved_map_guides_use_the_same_prepared_material_and_never_change_raw_film() {
    let mut task = cpu_mapped_fixture(4, true, true, true);
    for _ in 0..200_000 {
        if task.denoise.first().unwrap().ready() {
            break;
        }
        task.step_with_previews(1).unwrap();
    }
    let guide = task.denoise.first().unwrap().pixels().unwrap()[0];
    assert_eq!(guide.body, 1);
    assert_eq!(guide.albedo, [cpu_map_linear(128) as f32, cpu_map_linear(64) as f32, 1.0]);
    let expected = Vec3::new(f64::from(128_f32 / 255.0) * 2.0 - 1.0, 1.0, 1.0).normalized().unwrap();
    assert!((Vec3::new(f64::from(guide.normal[0]), f64::from(guide.normal[1]), f64::from(guide.normal[2])) - expected).len() < 1e-7);
    let filtered = complete_presentation(cpu_mapped_fixture(4, true, true, true), 1);
    let raw = complete_presentation(cpu_mapped_fixture(4, true, false, true), 16384);
    assert_eq!(filtered.2, raw.2, "guide capture/filtering cannot change raw mapped radiance");
    assert_eq!(filtered, complete_presentation(cpu_mapped_fixture(4, true, true, true), 16384));
}
#[test]
fn cpu_saved_map_cache_and_hit_continuations_cancel_or_exhaust_before_next_mutation() {
    for during_hit in [false, true] {
        for cancel in [false, true] {
            let mut task = cpu_mapped_fixture(4, true, false, true);
            for _ in 0..200_000 {
                let cache = task.maps.first().unwrap();
                if (!during_hit && cache.preparation_phase() == Some("linear")) || (during_hit && cache.hit_role() == Some(1)) {
                    break;
                }
                assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
            }
            let before = task.work.used();
            let before_maps = task.maps.first().unwrap().prepared_maps();
            let role = task.maps.first().unwrap().hit_role();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = before;
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), before);
            assert_eq!(task.maps.first().unwrap().prepared_maps(), before_maps);
            assert_eq!(task.maps.first().unwrap().hit_role(), role);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            assert!(task.take_linear_exr().is_none());
            assert!(!task.done);
        }
    }
}

#[test]
fn cpu_map_cache_bills_actual_source_capacity_pixels_and_one_peak_decoder_and_deduplicates_roles() {
    use crate::job::textures::cpu_cache::CpuTextureCache;
    let original = cpu_mapped_fixture(4, true, false, true);
    let resources = original.request.textures.as_ref().unwrap();
    let bytes = CpuTextureCache::required_bytes(resources, &original.scene, &original.options).unwrap();
    let mismatch = RaytraceOptions { texture_filtering: true, ..original.options.clone() };
    assert!(CpuTextureCache::required_bytes(resources, &original.scene, &mismatch).unwrap_err().message.contains("texture filtering"));
    let filtered = (**resources).clone().with_filtering(true);
    assert!(CpuTextureCache::required_bytes(&filtered, &original.scene, &original.options).unwrap_err().message.contains("texture filtering"));
    let mut plain_request = original.request.clone();
    plain_request.textures = None;
    let plain = ImageTask::new(original.scene.clone(), plain_request, CancelToken::default(), original.options.clone()).unwrap();
    assert_eq!(plain.maps.capacity(), 0, "omitted maps create no cache/decoder/hit heap slot");
    assert_eq!(original.admission.estimated_total_bytes - plain.admission.estimated_total_bytes, bytes);
    assert_eq!(original.admission.preparation_bytes - plain.admission.preparation_bytes, bytes);
    let mut clone = original.scene.clone();
    let mut body = clone.meshes[0].clone();
    body.name = "same maps second body".into();
    clone.meshes.push(body);
    let first = resources.bindings()[0].clone();
    let mut second = first.clone();
    second.body_index = 1;
    second.body_name = clone.meshes[1].name.clone();
    let shared = Arc::new(job::textures::RuntimeTextures::new(vec![first, second]).unwrap());
    assert_eq!(shared.summary().linear_pixel_bytes, resources.summary().linear_pixel_bytes);
    assert_eq!(shared.summary().encoded_bytes, resources.summary().encoded_bytes);
    let shared_bytes = CpuTextureCache::required_bytes(&shared, &clone, &original.options).unwrap();
    assert!(shared_bytes > bytes, "actual binding/name/map metadata capacities are still admitted");
    assert!(shared_bytes - bytes < 64 * 1024, "shared pixels and codec workspace cannot be charged twice");
    // Independently count unique source+role rows from the authored resources;
    // two bodies share the same four maps and must not double the progress target.
    let mut sources = std::collections::BTreeSet::new();
    let mut unique_rows = 0_u64;
    for binding in shared.bindings() {
        for map in [&binding.base_color, &binding.roughness, &binding.metallic, &binding.normal].into_iter().flatten() {
            if sources.insert(map.identity()) {
                unique_rows += u64::from(map.dimensions()[1]);
            }
        }
    }
    let mut cache = CpuTextureCache::new(shared, &CancelToken::default()).unwrap();
    let mut work = 0;
    while !cache.ready() {
        cache
            .prepare_one(&CancelToken::default(), &mut |units| {
                assert_eq!(units, 1);
                work += units;
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(cache.prepared_maps(), 4, "source+role identity shares actual owned normalized pixels");
    assert_eq!(cache.preparation_rows(), (unique_rows, unique_rows), "real shared-map preparation reaches its unique decoded-row target");
    assert!(work > 0);
    let no_maps = complete_presentation(plain, 1);
    let mut reference = cpu_mapped_fixture(4, true, false, true);
    reference.request.textures = None;
    let reference = ImageTask::new(reference.scene, reference.request, reference.cancel, reference.options).unwrap();
    assert_eq!(no_maps, complete_presentation(reference, 16384), "no-map PNG/raw/work stays on the ordinary producer for every quantum");
}
#[test]
fn cpu_map_active_producer_refuses_legacy_selected_optics_without_erasing_any_intent() {
    let original = cpu_mapped_fixture(4, true, false, false);
    let mut options = original.options.clone();
    options.path = None;
    options.reflections = Some(ReflectionOptions {
        bodies: vec![ReflectionBody { body: original.scene.meshes[0].name.clone(), mode: ReflectionMode::Mirror }],
        samples: 1,
        max_bounces: 2,
    });
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    let before = serde_json::to_value(&options).unwrap();
    assert!(admit(&original.scene, &request, &options).unwrap_err().message.contains("CPU surface maps"));
    assert_eq!(serde_json::to_value(&options).unwrap(), before);
    assert!(request.textures.is_some());
    let mut legacy = options.clone();
    legacy.reflections = None;
    let plain = ImageRequest { raytrace: Some(legacy.clone()), textures: None, ..request };
    assert!(admit(&original.scene, &plain, &legacy).is_ok());
}
#[test]
fn cpu_map_spatial_pixels_use_actual_world_coordinates_for_every_jittered_prefix() {
    let make = |samples| {
        let original = path_fixture(samples, 1, false);
        let mut scene = original.scene;
        let mut mesh = (*scene.meshes[0].mesh).clone();
        for point in &mut mesh.positions {
            point.x = point.x.signum() * 10.0;
            point.y = point.y.signum() * 10.0;
        }
        scene.bounds = mesh.bounds();
        scene.meshes[0].mesh = Arc::new(mesh);
        scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap());
        let mut binding = cpu_map_binding(0, &scene.meshes[0].name, Some(cpu_map_png(2, 1, &[0, 0, 0, 255, 255, 0, 0, 255])));
        binding.frame.u = [1.0, 0.0, 0.0];
        binding.frame.v = [0.0, 1.0, 0.0];
        let options = RaytraceOptions {
            path: None,
            environment: None,
            lights: Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: std::f64::consts::PI }]),
            output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
            ..original.options
        };
        // Keep this coordinate/jitter oracle magnified under the native mip
        // policy; minification has its own independent physical-wood regressions.
        let camera = crate::Camera { distance: 0.5, ..original.request.camera };
        let request = ImageRequest {
            camera,
            textures: Some(Arc::new(job::textures::RuntimeTextures::new(vec![binding]).unwrap())),
            raytrace: Some(options.clone()),
            ..original.request
        };
        ImageTask::new(scene, request, original.cancel, options).unwrap()
    };
    let camera = make(1).request.camera;
    // Independent ray/plane witness: unit planar U/V and a 2x1 texture turn
    // these axis-aligned world-pixel spans into a diagonal texel differential.
    // Its largest scale is 0.8 texel, strictly below the magnification boundary.
    assert_eq!(camera.fov, 0.0);
    let plane_hit = |x, y| {
        let (origin, direction) = camera.ray(x, y, 1.0, 1.0);
        origin + direction * (-origin.z / direction.z)
    };
    let centre = plane_hit(0.5, 0.5);
    let du = plane_hit(1.5, 0.5) - centre;
    let dv = plane_hit(0.5, 1.5) - centre;
    assert!((du - Vec3::X * 0.4).len() < 1e-12);
    assert!((dv + Vec3::Y * 0.4).len() < 1e-12);
    let largest_texel_span = (du.x.abs() * 2.0).max(dv.y.abs());
    assert!(largest_texel_span < 1.0, "the independent LOD0 oracle must remain physically magnified");
    let expected = |count: u32| {
        let mut mean = 0.0;
        for ordinal in 0..count {
            let [dx, dy] = progressive::variates(9, 0, ordinal, 0);
            let (origin, direction) = camera.ray(dx, dy, 1.0, 1.0);
            let point = origin + direction * (-origin.z / direction.z);
            let x = point.x.rem_euclid(1.0) * 2.0 - 0.5;
            let low = x.floor();
            let t = x - low;
            let first = if (low as i64).rem_euclid(2) == 0 { 0.0 } else { 1.0 };
            mean += first * (1.0 - t) + (1.0 - first) * t;
        }
        mean /= f64::from(count);
        [encode8(mean / (1.0 + mean)), 0, 0, 255]
    };
    let prefixes: Vec<_> = (1..=8).map(expected).collect();
    assert!(prefixes.windows(2).any(|pair| pair[0] != pair[1]), "actual jittered world coordinates must change the prefix mean");
    let fixed_centre = [encode8(0.5 / 1.5), 0, 0, 255];
    assert!(prefixes.iter().any(|pixel| *pixel != fixed_centre), "a constant-centre lookup must not satisfy the coordinate oracle");
    let result = complete_presentation(make(8), 1);
    assert_eq!(result, complete_presentation(make(8), 16384));
    for (samples, png) in result.0.iter().chain(std::iter::once(&(8, result.1.clone()))) {
        assert_eq!(alpha_pixel(png), expected(*samples), "actual mapped world-point mean at prefix{samples}");
        assert_eq!(*png, complete_presentation(make(*samples), 3).1);
    }
}

#[test]
fn cpu_saved_maps_keep_explicit_fixed_spatial_sampling_and_all_primary_counts_usable() {
    let expected = complete_presentation(cpu_mapped_fixture(1, false, false, true), 1);
    for primary_samples in [1, 4, 16] {
        let make = || {
            let original = cpu_mapped_fixture(1, false, false, true);
            let options = RaytraceOptions { progressive: None, primary_samples, ..original.options };
            let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
            ImageTask::new(original.scene, request, original.cancel, options).unwrap()
        };
        let intent = make();
        assert!(intent.options.progressive.is_none());
        assert_eq!(intent.options.primary_samples, primary_samples);
        let actual = complete_presentation(intent, 1);
        assert!(actual.0.is_empty());
        assert_eq!(
            (actual.1.clone(), actual.2.clone()),
            (expected.1.clone(), expected.2.clone()),
            "constant mapped radiance must not depend on an authored spatial count"
        );
        assert_eq!(actual, complete_presentation(make(), 16384));
    }
}

fn composed_map_aging_fixture(samples: u32, path: bool, guides: bool, explicit: bool, half_plane: bool) -> ImageTask {
    use crate::texture_input::MapRole;
    let original = path_fixture(samples, 2, false);
    let mut scene = original.scene.clone();
    let mapped_base = Vec3::new(cpu_map_linear(128), cpu_map_linear(64), cpu_map_linear(192));
    let final_base = (mapped_base + Vec3::new(1.0, 0.0, 0.0)) * 0.5;
    let mapped_normal = Vec3::new(f64::from(128_f32 / 255.0) * 2.0 - 1.0, f64::from(128_f32 / 255.0) * 2.0 - 1.0, 1.0).normalized().unwrap();
    let mesh = Arc::make_mut(&mut scene.meshes[0].mesh);
    if half_plane {
        for point in &mut mesh.positions {
            if point.x < 0.0 {
                point.x = 0.0;
            }
        }
    }
    if explicit {
        mesh.normals.fill(mapped_normal);
    }
    scene.bounds = mesh.bounds();
    // The original dielectric must become metal through its map BEFORE the
    // Metal-only aging layer classifies it; the final layer returns a dielectric.
    scene.meshes[0].surface = Some(if explicit {
        OpaqueSurface::new(final_base, 0.0, 0.35, 1.0).unwrap()
    } else {
        OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 1.0, 1.0).unwrap()
    });
    let layer = job::aging::AgingLayer {
        kind: job::aging::AgingKind::Fading,
        surface_class: job::aging::SurfaceClass::Metal,
        roughness: 0.35,
        ..solid_age()
    };
    let options = RaytraceOptions {
        aging: (!explicit).then(|| aging_stack(&[layer])),
        path: path.then_some(job::CpuPathOptions { max_depth: 2 }),
        denoise: guides,
        environment: path.then_some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        lights: Some(if path {
            vec![]
        } else {
            vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: std::f64::consts::PI }]
        }),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..original.options
    };
    let textures = (!explicit).then(|| {
        let mut binding = cpu_map_binding(0, &scene.meshes[0].name, Some(cpu_map_png(1, 1, &[128, 64, 192, 255])));
        binding.roughness = Some(job::textures::TextureMap::new(cpu_map_png(1, 1, &[128, 128, 128, 255]), MapRole::Roughness).unwrap());
        binding.metallic = Some(job::textures::TextureMap::new(cpu_map_png(1, 1, &[255; 4]), MapRole::Metallic).unwrap());
        binding.normal = Some(job::textures::TextureMap::new(cpu_map_png(1, 1, &[128, 128, 255, 255]), MapRole::NormalOpenGl).unwrap());
        Arc::new(job::textures::RuntimeTextures::new(vec![binding]).unwrap())
    });
    let mut camera = original.request.camera;
    if half_plane {
        camera.target.x = 0.1;
    }
    let request = ImageRequest { textures, raytrace: Some(options.clone()), camera, transparent_background: true, ..original.request };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}

#[test]
fn composed_cpu_maps_precede_aging_classification_and_share_exact_material_with_direct_path_and_guides() {
    let final_base = [(cpu_map_linear(128) + 1.0) * 0.5, cpu_map_linear(64) * 0.5, cpu_map_linear(192) * 0.5];
    let normal = Vec3::new(f64::from(128_f32 / 255.0) * 2.0 - 1.0, f64::from(128_f32 / 255.0) * 2.0 - 1.0, 1.0).normalized().unwrap();
    for path in [false, true] {
        let mut mapped = composed_map_aging_fixture(8, path, false, false, false);
        // Stop before the first light sample so these assertions inspect the
        // actual shared cached closure, not a separately recomputed material.
        for _ in 0..200_000 {
            if mapped.aging.first().unwrap().ready() {
                break;
            }
            assert!(matches!(mapped.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        }
        let before_age = mapped.maps.first().unwrap().surface().unwrap();
        assert_eq!(before_age.surface.metallic(), 1.0);
        assert_eq!(before_age.surface.roughness(), f64::from(128_f32 / 255.0));
        let aged = mapped.aging.first().unwrap().surface().unwrap();
        assert_eq!(aged.surface.metallic(), 0.0, "Metal class is captured after map resolution");
        assert_eq!(aged.surface.roughness(), 0.35, "the roughness map cannot overwrite the later authored aging layer");
        for (actual, expected) in [aged.surface.base_color().x, aged.surface.base_color().y, aged.surface.base_color().z].into_iter().zip(final_base)
        {
            assert!((actual - expected).abs() < 1e-15);
        }
        assert!((aged.shading_normal - normal).len() < 1e-15, "aging preserves the complete mapped normal");
        let actual = path_mean(mapped).0;
        let reference = path_mean(composed_map_aging_fixture(8, path, false, true, false)).0;
        for (actual, expected) in actual.into_iter().zip(reference) {
            assert!((actual - expected).abs() < 1e-12, "mapped then aged BRDF/PDF/proposal {actual} != explicit closure {expected}");
        }
        if !path {
            // IOR1, nonmetallic Fading layer: diffuse BRDF = colour/pi;
            // normal-view pi-strength source contributes colour * mapped N.z.
            for (actual, color) in actual.into_iter().zip(final_base) {
                assert!((actual - color * normal.z).abs() < 1e-12);
            }
        }
        let (guides, _) = composed_panel_guides(composed_map_aging_fixture(8, path, true, false, false));
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].body, 1, "the final dielectric must not retain the mapped smooth-metal guide classification");
        assert_eq!(guides[0].albedo, final_base.map(|v| v as f32));
        for (actual, expected) in guides[0].normal.into_iter().zip([normal.x, normal.y, normal.z]) {
            assert!((f64::from(actual) - expected).abs() < 1e-7);
        }
    }
}

#[test]
fn composed_cpu_maps_and_aging_keep_alpha_all_prefixes_raw_master_quanta_and_denoise_intent() {
    for path in [false, true] {
        for guides in [false, true] {
            let actual = complete_presentation(composed_map_aging_fixture(8, path, guides, false, true), 1);
            assert_eq!(actual, complete_presentation(composed_map_aging_fixture(8, path, guides, false, true), 16384));
            let reference = complete_presentation(composed_map_aging_fixture(8, path, guides, true, true), 3);
            assert_eq!(actual.0, reference.0, "every mapped+aged PNG prefix equals the independently authored closure");
            assert_eq!(actual.1, reference.1);
            assert_eq!(actual.2, reference.2);
            let unfiltered = complete_presentation(composed_map_aging_fixture(8, path, false, false, true), 3);
            assert_eq!(actual.2, unfiltered.2, "map/aging guide evaluation and denoise never change raw EXR");
            let camera = composed_map_aging_fixture(1, path, false, false, true).request.camera;
            let mut covered = 0_u32;
            for ordinal in 0..8 {
                let [x, y] = progressive::variates(9, 0, ordinal, 0);
                let (origin, direction) = camera.ray(x, y, 1.0, 1.0);
                let position = origin + direction * (-origin.z / direction.z);
                covered += u32::from(position.x >= 0.0);
                let png = if ordinal == 7 { &actual.1 } else { &actual.0[ordinal as usize].1 };
                assert_eq!(alpha_pixel(png)[3], ((covered * 510 + ordinal + 1) / (2 * (ordinal + 1))) as u8);
                assert_eq!(*png, complete_presentation(composed_map_aging_fixture(ordinal + 1, path, guides, false, true), 3).1);
            }
            assert!(covered > 0 && covered < 8, "the fixture exercises a real partial-coverage matte");
            let raw = native_exr_pixels(actual.2.as_deref().unwrap());
            assert_eq!(raw.get_pixel(0, 0).0[3], covered as f32 / 8.0);
        }
    }
}

#[test]
fn composed_cpu_map_and_aging_reservations_add_and_cancel_before_the_first_aging_layer() {
    let task = composed_map_aging_fixture(4, false, false, false, false);
    let mut bare_options = task.options.clone();
    bare_options.aging = None;
    let bare_request = ImageRequest { textures: None, raytrace: Some(bare_options.clone()), ..task.request.clone() };
    let bare = admit(&task.scene, &bare_request, &bare_options).unwrap();
    let map_bytes =
        job::textures::cpu_cache::CpuTextureCache::required_bytes(task.request.textures.as_ref().unwrap(), &task.scene, &task.options).unwrap();
    assert_eq!(
        task.admission.estimated_total_bytes - bare.estimated_total_bytes,
        map_bytes + 2 * std::mem::size_of::<job::aging::AgingStack>() as u64 + super::super::aging::Task::required_bytes()
    );
    assert_eq!((task.maps.capacity(), task.aging.capacity()), (1, 1));
    for path in [false, true] {
        for guides in [false, true] {
            for cancel in [false, true] {
                let mut task = composed_map_aging_fixture(4, path, guides, false, false);
                for _ in 0..200_000 {
                    if task.maps.first().unwrap().hit_ready() && !task.aging.first().unwrap().started() {
                        break;
                    }
                    task.step_with_previews(1).unwrap();
                }
                assert_eq!(task.maps.first().unwrap().hit_role(), Some(4));
                assert!(!task.aging.first().unwrap().started(), "a final map role must yield before any aging mutation");
                let used = task.work.used();
                if cancel {
                    task.cancel.cancel();
                } else {
                    task.work.limit = used;
                }
                assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
                assert_eq!(task.work.used(), used);
                assert!(!task.aging.first().unwrap().started());
                assert_eq!(task.maps.first().unwrap().hit_role(), Some(4));
                assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
                assert!(task.take_linear_exr().is_none());
            }
        }
    }
}

mod adaptive_cpu_images {
    use super::*;
    use crate::job::adaptive::{AdaptiveSampling, SamplingReport};

    fn policy() -> AdaptiveSampling {
        AdaptiveSampling { pilot_samples: 16, min_samples: 16, relative_error: 0.0, absolute_error: 0.2, coverage_error: 0.1 }
    }
    fn adaptive_fixture(denoise: bool, path: bool, lens: bool) -> ImageTask {
        let mut camera = crate::Camera { distance: 10.0, fov: 0.0, ..Default::default() };
        camera.set_view(StandardView::Top);
        if lens {
            camera.fov = 35.0_f64.to_radians();
        }
        let point = |x, y| {
            let (o, d) = camera.ray(x, y, 3.0, 1.0);
            o + d * (-o.z / d.z)
        };
        let mesh = Arc::new(Mesh {
            positions: vec![point(0.0, 0.0), point(1.5, 0.0), point(1.5, 1.0), point(0.0, 1.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        });
        let bounds = mesh.bounds();
        let scene = ProductScene {
            bounds,
            meshes: vec![ProductMesh {
                transmission: None,
                name: "Adaptive analytic half-plane".into(),
                mesh,
                color: [0; 3],
                opacity: 1.0,
                surface: Some(OpaqueSurface::new(Vec3::ZERO, 1.0, 1.0, 1.5).unwrap()),
                faces: Vec::new(),
            }],
        };
        let options = RaytraceOptions {
            adaptive: Some(Box::new(policy())),
            progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: 48, seed: 9 }),
            denoise,
            path: path.then_some(job::CpuPathOptions { max_depth: 1 }),
            shadows: path,
            lens: lens.then_some(job::ThinLens { aperture_radius_mm: 0.01, focus_distance_mm: 10.0 }),
            output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
            emissions: Some(vec![job::SurfaceEmission {
                body_index: 0,
                body: "Adaptive analytic half-plane".into(),
                color: [1.0, 0.5, 0.25],
                strength: 4.0,
                sides: job::EmissionSides::Both,
            }]),
            lights: Some(Vec::new()),
            ..Default::default()
        };
        let request = ImageRequest {
            width: 3,
            height: 1,
            camera,
            transparent_background: true,
            raytrace: Some(options.clone()),
            integrator: job::Integrator::Raytrace,
            ..fixture(1, 1, None).request
        };
        ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
    }
    fn finish(mut task: ImageTask, quantum: usize) -> (PresentedOutput, SamplingReport) {
        let mut previews = Vec::new();
        for _ in 0..500_000 {
            match task.step_with_previews(quantum).unwrap() {
                ImagePoll::Pending(progress) => assert!(progress.done <= progress.total),
                ImagePoll::Preview { png, samples_per_pixel } => {
                    assert!(task.take_linear_exr().is_none());
                    assert!(task.sampling_report().is_none(), "pilots and unfinished production are not a final sampling report");
                    previews.push((samples_per_pixel, png));
                }
                ImagePoll::Complete(png) => {
                    let report = task.sampling_report().expect("adaptive final report survives release of the working film");
                    let exr = task.take_linear_exr();
                    assert!(task.take_linear_exr().is_none());
                    assert!(task.progressive.is_none(), "working film is released before final transfer");
                    return ((previews, png, exr, task.work.used()), report);
                }
            }
        }
        panic!("bounded adaptive image did not finish");
    }
    fn hits(pixel: u64, seed: u32, count: u32) -> u32 {
        match pixel {
            0 => count,
            1 => (0..count).filter(|n| progressive::variates(seed, pixel, *n, 0)[0] < 0.5).count() as u32,
            _ => 0,
        }
    }
    fn alpha(covered: u32, samples: u32) -> u8 {
        ((covered * 510 + samples) / (samples * 2)) as u8
    }
    fn allocation() -> [u32; 3] {
        // Independent Bernoulli sample variance, including premultiplied HDR RGB.
        let covered = f64::from(hits(1, 9 ^ 0xa511_e9b3, 16));
        let fraction = covered / 16.0;
        let variance = 16.0 / 15.0 * fraction * (1.0 - fraction);
        let estimate = ((variance.sqrt() * 4.0 / 0.2).min(48.0_f64.sqrt()).powi(2).ceil() as u32).clamp(16, 48);
        [16, estimate, 16]
    }
    #[test]
    fn actual_adaptive_png_prefixes_discard_pilots_and_use_heterogeneous_exact_counts_for_raw_and_alpha() {
        let (actual, report) = finish(adaptive_fixture(false, false, false), 1);
        assert_eq!((actual.clone(), report.clone()), finish(adaptive_fixture(false, false, false), 16384));
        assert_eq!(actual.0.iter().map(|p| p.0).collect::<Vec<_>>(), (1..64).collect::<Vec<_>>());
        let counts = allocation();
        assert_eq!(counts, [16, 48, 16], "independent pilot variance selects different real per-pixel production budgets");
        assert_eq!((report.minimum_production_samples, report.maximum_production_samples), (16, 48));
        assert_eq!((report.production_samples, report.primary_path_evaluations), (80, 128));
        assert_eq!((report.pilot_samples_per_pixel, report.pixels_at_cap, report.transmission_strata), (16, 1, 1));
        for (absolute, png) in actual.0.iter().chain(std::iter::once(&(64, actual.1.clone()))) {
            let pilot = *absolute <= 16;
            let ordinal = if pilot { *absolute } else { *absolute - 16 };
            let seed = if pilot { 9 ^ 0xa511_e9b3 } else { 9 };
            let image = ::image::load_from_memory(png).unwrap().to_rgba8();
            for (pixel, &count) in counts.iter().enumerate() {
                let n = if pilot { ordinal } else { ordinal.min(count) };
                let covered = hits(pixel as u64, seed, n);
                let expected_rgb = if covered == 0 { [0; 3] } else { [4.0_f64, 2.0, 1.0].map(|r| encode8(r / (1.0 + r))) };
                assert_eq!(
                    image.get_pixel(pixel as u32, 0).0,
                    [expected_rgb[0], expected_rgb[1], expected_rgb[2], alpha(covered, n)],
                    "absolute pass {absolute}, pixel {pixel}"
                );
            }
        }
        let exr = actual.2.unwrap();
        let tail = &exr[exr.len() - 3 * 16..];
        let channel =
            |channel: usize, pixel: usize| f32::from_le_bytes(tail[(channel * 3 + pixel) * 4..(channel * 3 + pixel + 1) * 4].try_into().unwrap());
        for (pixel, &count) in counts.iter().enumerate() {
            let coverage = f64::from(hits(pixel as u64, 9, count)) / f64::from(count);
            for (c, scale) in [1.0, 1.0, 2.0, 4.0].into_iter().enumerate() {
                assert_eq!(channel(c, pixel), (coverage * scale) as f32, "raw A,B,G,R are normalized by this pixel's production count");
            }
        }
        let production = hits(1, 9, 48);
        let leaked_pilots = hits(1, 9 ^ 0xa511_e9b3, 16) + production;
        assert_ne!(f64::from(production) / 48.0, f64::from(leaked_pilots) / 64.0, "the fixture distinguishes incorrect pilot leakage");
    }
    #[test]
    fn adaptive_shared_output_denoise_and_opaque_path_keep_raw_and_exact_alpha() {
        let original = finish(adaptive_fixture(false, false, false), 1);
        let denoised = finish(adaptive_fixture(true, false, false), 3);
        assert_eq!(original.0.2, denoised.0.2, "the filter cannot change per-pixel-normalized raw production radiance");
        assert_eq!(original.0.0, denoised.0.0, "emission and partial matte are genuine filter bypasses at every pilot/production pass");
        assert_eq!(original.0.1, denoised.0.1);
        let path = finish(adaptive_fixture(true, true, false), 4);
        assert_eq!(
            (original.0.0, original.0.1, original.0.2, original.1),
            (path.0.0, path.0.1, path.0.2, path.1),
            "depth-one opaque GI must consume the same adaptive primary identity and exact count plane"
        );
        let lens = finish(adaptive_fixture(true, true, true), 4);
        assert_eq!(
            lens,
            finish(adaptive_fixture(true, true, true), 1),
            "actual positive aperture, guides and path transport remain independent of quantum"
        );
        assert!(lens.0.2.is_some());
    }
    #[test]
    fn adaptive_reset_stopped_pixels_resolve_and_finishing_obey_one_work_ceiling_and_cancel() {
        for point in ["reset", "stopped", "resolve", "output"] {
            for cancel in [false, true] {
                let mut task = adaptive_fixture(true, false, false);
                for _ in 0..500_000 {
                    let p = task.progressive.as_ref().unwrap();
                    let reached = match point {
                        "reset" => p.film.resetting() && p.film.reset_progress() == 1,
                        "stopped" => {
                            task.pixel == 0
                                && !p.resolving
                                && !p.film.resetting()
                                && p.film.samples() == 33
                                && task.output.is_empty()
                                && task.encoder.is_none()
                        }
                        "resolve" => p.resolving && p.resolve_pixel == 1,
                        _ => !task.output.is_empty(),
                    };
                    if reached {
                        break;
                    }
                    assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
                }
                let p = task.progressive.as_ref().unwrap();
                let before = (p.film.samples(), p.film.reset_progress(), p.resolve_pixel, task.pixel);
                let work = task.work.used();
                assert!(work > 0);
                if cancel {
                    task.cancel.cancel();
                } else {
                    task.work.limit = work;
                }
                assert_eq!(
                    task.step_with_previews(1).err().unwrap().code,
                    if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded },
                    "{point}"
                );
                let p = task.progressive.as_ref().unwrap();
                assert_eq!(
                    (p.film.samples(), p.film.reset_progress(), p.resolve_pixel, task.pixel),
                    before,
                    "{point}: debit/cancel must precede mutation"
                );
                assert_eq!(task.work.used(), work);
                assert!(task.take_linear_exr().is_none());
                assert!(task.sampling_report().is_none());
            }
        }
    }
    #[test]
    fn adaptive_last_pilot_is_finished_and_published_before_reset_can_begin() {
        for stop in ["advance", "cancel", "work"] {
            let mut task = adaptive_fixture(false, false, false);
            let pilot = policy().pilot_samples;
            let count = task.request.width as usize * task.request.height as usize;
            let mut prefixes = Vec::new();
            for _ in 0..500_000 {
                let p = task.progressive.as_ref().unwrap();
                if p.film.samples() == pilot && task.pixel == count && !p.resolving {
                    break;
                }
                match task.step_with_previews(1).unwrap() {
                    ImagePoll::Preview { samples_per_pixel, .. } => prefixes.push(samples_per_pixel),
                    ImagePoll::Pending(_) => {}
                    ImagePoll::Complete(_) => {
                        panic!("production cannot finish before all pilots are handed off")
                    }
                }
            }
            assert_eq!(prefixes, (1..pilot).collect::<Vec<_>>());
            assert_eq!(task.pixel, count);
            assert!(task.progressive.as_ref().unwrap().film.pilot_complete());
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            let p = task.progressive.as_ref().unwrap();
            assert!(p.resolving, "the final pilot must resolve rather than reset at the pixel boundary");
            assert!(!p.film.resetting());
            assert_eq!(p.film.samples(), pilot);
            assert_eq!(p.film.counts().unwrap().unwrap().pixel(0).unwrap().samples(), pilot);
            for _ in 0..500_000 {
                match task.step_with_previews(1).unwrap() {
                    ImagePoll::Preview { samples_per_pixel, .. } => {
                        assert_eq!(samples_per_pixel, pilot, "the last pilot is published exactly once");
                        prefixes.push(samples_per_pixel);
                        break;
                    }
                    ImagePoll::Pending(_) => {}
                    ImagePoll::Complete(_) => {
                        panic!("pilot output cannot lend the final image/master")
                    }
                }
            }
            assert_eq!(prefixes, (1..=pilot).collect::<Vec<_>>());
            assert_eq!(task.pixel, 0, "only completed PNG handoff opens the reset boundary");
            assert!(task.output.is_empty() && task.encoder.is_none());
            assert!(task.take_linear_exr().is_none() && task.sampling_report().is_none());
            let p = task.progressive.as_ref().unwrap();
            assert!(p.film.pilot_complete() && !p.film.resetting());
            let frozen: Vec<_> = (0..count)
                .map(|pixel| {
                    let counts = p.film.counts().unwrap().unwrap().pixel(pixel).unwrap();
                    (counts.samples(), counts.covered())
                })
                .collect();
            let work = task.work.used();
            if stop != "advance" {
                if stop == "cancel" {
                    task.cancel.cancel();
                } else {
                    task.work.limit = work;
                }
                assert_eq!(
                    task.step_with_previews(1).err().unwrap().code,
                    if stop == "cancel" { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded }
                );
                let p = task.progressive.as_ref().unwrap();
                assert!(p.film.pilot_complete() && !p.film.resetting());
                assert_eq!(task.pixel, 0);
                assert_eq!(task.work.used(), work);
                for (pixel, before) in frozen.iter().enumerate() {
                    let counts = p.film.counts().unwrap().unwrap().pixel(pixel).unwrap();
                    assert_eq!((counts.samples(), counts.covered()), *before, "failed reset cannot discard any pilot yet");
                }
                continue;
            }
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            assert!(task.progressive.as_ref().unwrap().film.resetting());
            assert!(task.progressive.as_ref().unwrap().film.counts().is_err(), "incomplete/reset counts must remain unavailable");
            assert_eq!(task.work.used(), work + 1, "begin reset keeps its prepaid work debit");
            for _ in 0..500_000 {
                match task.step_with_previews(1).unwrap() {
                    ImagePoll::Preview { samples_per_pixel, .. } => {
                        assert_eq!(samples_per_pixel, pilot + 1);
                        break;
                    }
                    ImagePoll::Pending(_) => {}
                    ImagePoll::Complete(_) => panic!("first production pass cannot be the final"),
                }
            }
            let p = task.progressive.as_ref().unwrap();
            assert_eq!(p.film.samples(), pilot + 1);
            for pixel in 0..count {
                let counts = p.film.counts().unwrap().unwrap().pixel(pixel).unwrap();
                assert_eq!(counts.samples(), 1, "independent production must discard all pilot counts");
            }
            assert_eq!(p.film.counts().unwrap().unwrap().pixel(0).unwrap().covered(), 1);
            assert!(task.take_linear_exr().is_none() && task.sampling_report().is_none());
        }
    }
    #[test]
    fn ordinary_camera_film_keeps_its_original_storage_rng_and_debits_when_adaptive_is_absent() {
        let task = progressive_fixture(8, 9, Some(true), 0.0);
        assert!(matches!(task.progressive.as_ref().unwrap().film, super::super::super::progressive::CameraFilm::Uniform(_)));
        assert_eq!(task.sampling.capacity(), 0);
        assert_eq!(task.options.retained_adaptive_bytes().unwrap(), 0);
        let (previews, png, work) = progressive_complete(task, 1);
        assert_eq!((previews.clone(), png.clone(), work), progressive_complete(progressive_fixture(8, 9, Some(true), 0.0), 16384));
        for (n, prefix) in previews {
            assert_eq!(prefix, progressive_complete(progressive_fixture(n, 9, Some(true), 0.0), 3).1);
        }
        assert!(std::mem::size_of::<ImageTask>() <= 4096);
    }
}

mod composed_adaptive_map_aging_tests {
    use super::*;
    use crate::job::adaptive::{AdaptiveSampling, SamplingReport};
    #[derive(Debug, PartialEq)]
    struct GuideFacts {
        body: u32,
        albedo: [f32; 3],
        normal: [f32; 3],
    }
    #[derive(Debug, PartialEq)]
    struct Completed {
        previews: Vec<(u32, Vec<u8>)>,
        png: Vec<u8>,
        exr: Vec<u8>,
        report: SamplingReport,
        means: Vec<[f64; 3]>,
        counts: Vec<(u32, u32)>,
        guides: Vec<GuideFacts>,
        work: u64,
    }
    fn fixture(path: bool, guides: bool, explicit: bool, adaptive: bool) -> ImageTask {
        let original = composed_map_aging_fixture(32, path, guides, explicit, true);
        let options = RaytraceOptions {
            adaptive: adaptive.then(|| {
                Box::new(AdaptiveSampling { pilot_samples: 16, min_samples: 16, relative_error: 0.0, absolute_error: 0.2, coverage_error: 0.01 })
            }),
            ..original.options
        };
        let request = ImageRequest { width: 3, height: 1, raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    }
    fn finish(mut task: ImageTask, quantum: usize) -> Completed {
        let target = task.options.preview_target().unwrap();
        let mut previews = Vec::new();
        let mut means = Vec::new();
        let mut counts = Vec::new();
        let mut guides = Vec::new();
        for _ in 0..1_000_000 {
            if guides.is_empty() && task.denoise.first().is_some_and(|g| g.ready()) {
                assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0, "complete deterministic map/aging guides precede all pilot samples");
                guides = task
                    .denoise
                    .first()
                    .unwrap()
                    .pixels()
                    .unwrap()
                    .iter()
                    .map(|g| GuideFacts { body: g.body, albedo: g.albedo, normal: g.normal })
                    .collect();
            }
            if means.is_empty() && task.progressive.as_ref().is_some_and(|p| p.film.samples() == target) {
                for index in 0..3 {
                    let (mean, covered, samples) = task.progressive.as_ref().unwrap().film.pixel(index).unwrap();
                    means.push(mean);
                    counts.push((covered, samples));
                }
            }
            match task.step_with_previews(quantum).unwrap() {
                ImagePoll::Pending(progress) => assert!(progress.done <= progress.total),
                ImagePoll::Preview { png, samples_per_pixel } => {
                    assert!(task.take_linear_exr().is_none());
                    previews.push((samples_per_pixel, png));
                }
                ImagePoll::Complete(png) => {
                    return Completed {
                        previews,
                        png,
                        exr: task.take_linear_exr().unwrap(),
                        report: task.sampling_report().unwrap(),
                        means,
                        counts,
                        guides,
                        work: task.work.used(),
                    };
                }
            }
        }
        panic!("bounded combined adaptive map/aging fixture did not finish");
    }
    fn covered(camera: crate::Camera, pixel: usize, seed: u32, samples: u32) -> u32 {
        (0..samples)
            .filter(|ordinal| {
                let [x, y] = progressive::variates(seed, pixel as u64, *ordinal, 0);
                let (origin, direction) = camera.ray(pixel as f64 + x, y, 3.0, 1.0);
                (origin + direction * (-origin.z / direction.z)).x >= 0.0
            })
            .count() as u32
    }
    #[test]
    fn combined_adaptive_maps_aging_direct_has_independent_heterogeneous_counts_alpha_raw_and_final_guide_closure() {
        let camera = fixture(false, true, false, true).request.camera;
        let actual = finish(fixture(false, true, false, true), 1);
        assert_eq!(actual, finish(fixture(false, true, false, true), 16384));
        assert_eq!(actual.previews.iter().map(|p| p.0).collect::<Vec<_>>(), (1..48).collect::<Vec<_>>());
        assert_eq!(actual.counts.iter().map(|p| p.1).collect::<Vec<_>>(), [16, 32, 16]);
        assert_eq!((actual.report.production_samples, actual.report.primary_path_evaluations), (64, 112));
        assert_eq!((actual.report.minimum_production_samples, actual.report.maximum_production_samples, actual.report.pixels_at_cap), (16, 32, 1));
        let base = [(cpu_map_linear(128) + 1.0) * 0.5, cpu_map_linear(64) * 0.5, cpu_map_linear(192) * 0.5];
        let normal = Vec3::new(f64::from(128_f32 / 255.0) * 2.0 - 1.0, f64::from(128_f32 / 255.0) * 2.0 - 1.0, 1.0).normalized().unwrap();
        assert_eq!(actual.guides.iter().map(|g| g.body).collect::<Vec<_>>(), [0, 1, 1]);
        for g in actual.guides.iter().filter(|g| g.body != 0) {
            assert_eq!(g.albedo, base.map(|v| v as f32));
            for (v, n) in g.normal.into_iter().zip([normal.x, normal.y, normal.z]) {
                assert!((f64::from(v) - n).abs() < 1e-7);
            }
        }
        for (absolute, png) in actual.previews.iter().chain(std::iter::once(&(48, actual.png.clone()))) {
            let pilot = *absolute <= 16;
            let pass = if pilot { *absolute } else { *absolute - 16 };
            let seed = if pilot { 9 ^ 0xa511_e9b3 } else { 9 };
            let image = ::image::load_from_memory(png).unwrap().to_rgba8();
            for pixel in 0..3 {
                let samples = if pilot { pass } else { pass.min(actual.counts[pixel].1) };
                let hits = covered(camera, pixel, seed, samples);
                assert_eq!(
                    image.get_pixel(pixel as u32, 0).0[3],
                    ((hits * 510 + samples) / (samples * 2)) as u8,
                    "pass{absolute}, pixel{pixel}: exact own-count alpha"
                );
            }
        }
        let raw = native_exr_pixels(&actual.exr);
        for pixel in 0..3 {
            let samples = actual.counts[pixel].1;
            let hits = covered(camera, pixel, 9, samples);
            assert_eq!(actual.counts[pixel].0, hits);
            let coverage = f64::from(hits) / f64::from(samples);
            assert_eq!(raw.get_pixel(pixel as u32, 0).0[3], coverage as f32);
            for (channel, color) in base.into_iter().enumerate() {
                assert!(
                    (actual.means[pixel][channel] - color * normal.z * coverage).abs() < 1e-12,
                    "independent normal-view Lambert oracle after maps then fading"
                );
                assert_eq!(
                    raw.get_pixel(pixel as u32, 0).0[channel],
                    (color * normal.z * coverage) as f32,
                    "raw uses each production denominator before PNG quantization"
                );
            }
        }
        let pilot = covered(camera, 1, 9 ^ 0xa511_e9b3, 16);
        let production = covered(camera, 1, 9, 32);
        assert_ne!(f64::from(production) / 32.0, f64::from(pilot + production) / 48.0, "fixture detects pilot leakage");
        let reference = finish(fixture(false, true, true, true), 3);
        assert_eq!(
            (actual.previews, actual.png, actual.exr, actual.report, actual.counts),
            (reference.previews, reference.png, reference.exr, reference.report, reference.counts),
            "complete mapped-then-aged output equals independently authored closure"
        );
    }
    #[test]
    fn combined_adaptive_maps_aging_gi_filter_and_unfiltered_raw_keep_all_prefixes_and_pilot_discard() {
        for path in [false, true] {
            let actual = finish(fixture(path, true, false, true), 1);
            assert_eq!(actual, finish(fixture(path, true, false, true), 16384));
            let reference = finish(fixture(path, true, true, true), 3);
            assert_eq!(
                (actual.previews.clone(), actual.png.clone(), actual.exr.clone(), actual.report.clone(), actual.counts.clone()),
                (reference.previews, reference.png, reference.exr, reference.report, reference.counts)
            );
            let unfiltered = finish(fixture(path, false, false, true), 3);
            assert_eq!(actual.exr, unfiltered.exr, "guides/filter must not alter normalized adaptive raw GI radiance");
            assert_eq!(actual.report, unfiltered.report, "guide preparation cannot contribute pilots or production samples");
            assert_eq!(actual.counts, unfiltered.counts);
        }
        let adaptive = finish(fixture(false, false, false, true), 1);
        let ordinary = complete_presentation(fixture(false, false, false, false), 3);
        assert_eq!(
            adaptive.exr,
            ordinary.2.unwrap(),
            "constant full-covered pixels and independently allocated partial pixels match the original production stream, never pilot+production averaging"
        );
    }
    #[test]
    fn combined_adaptive_maps_aging_bytes_add_to_existing_leases_and_cancel_before_production_aging_mutates() {
        let task = fixture(false, false, false, true);
        let mut plain = task.options.clone();
        plain.adaptive = None;
        let request = ImageRequest { raytrace: Some(plain.clone()), ..task.request.clone() };
        let baseline = admit(&task.scene, &request, &plain).unwrap();
        let progressive = task.options.progressive.unwrap();
        let film_delta = crate::film::adaptive::AdaptiveCameraFilm::required_bytes(
            3,
            1,
            progressive,
            *task.options.adaptive.as_deref().unwrap(),
            task.options.budget,
        )
        .unwrap()
            - crate::film::primary::PrimaryFilm::required_bytes(3, 1, progressive, task.options.budget).unwrap();
        let extra = film_delta + 2 * std::mem::size_of::<AdaptiveSampling>() as u64 + std::mem::size_of::<SamplingReport>() as u64;
        assert_eq!(
            task.admission.estimated_total_bytes - baseline.estimated_total_bytes,
            extra,
            "maps+aging resources remain additive; no hidden count or snapshot plane"
        );
        assert_eq!(task.admission.preparation_bytes - baseline.preparation_bytes, extra);
        assert_eq!((task.maps.capacity(), task.aging.capacity(), task.sampling.capacity()), (1, 1, 1));
        for path in [false, true] {
            for guides in [false, true] {
                for cancel in [false, true] {
                    let mut task = fixture(path, guides, false, true);
                    let mut reached = false;
                    for _ in 0..1_000_000 {
                        let film = &task.progressive.as_ref().unwrap().film;
                        if film.samples() >= 16
                            && !film.resetting()
                            && task.maps.first().unwrap().hit_ready()
                            && !task.aging.first().unwrap().started()
                        {
                            reached = true;
                            break;
                        }
                        assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
                    }
                    assert!(reached, "stop after a production map role, not an earlier pilot/guide state");
                    assert_eq!(task.maps.first().unwrap().hit_role(), Some(4));
                    let used = task.work.used();
                    let samples = task.progressive.as_ref().unwrap().film.samples();
                    let pixel = task.pixel;
                    if cancel {
                        task.cancel.cancel();
                    } else {
                        task.work.limit = used;
                    }
                    assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
                    assert_eq!(task.work.used(), used);
                    assert!(!task.aging.first().unwrap().started());
                    assert_eq!(task.maps.first().unwrap().hit_role(), Some(4));
                    assert_eq!((task.progressive.as_ref().unwrap().film.samples(), task.pixel), (samples, pixel));
                    assert!(task.take_linear_exr().is_none());
                    assert!(task.sampling_report().is_none());
                }
            }
        }
    }
}

fn cpu_general_glass_fixture(samples: u32, nested: bool, ior: f64) -> ImageTask {
    let outer = dielectric::tests::box_body(2.0);
    let mut meshes = vec![outer];
    if nested {
        let mut inner = dielectric::tests::box_body(1.0);
        inner.name = "Inner glass".into();
        for p in &mut Arc::make_mut(&mut inner.mesh).positions {
            p.x *= 0.5;
            p.y *= 0.5;
            p.z += 0.5;
        }
        meshes.push(inner);
    }
    let scene = ProductScene { bounds: meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds())), meshes };
    let dielectrics = scene
        .meshes
        .iter()
        .enumerate()
        .map(|(body_index, body)| job::DielectricOptions {
            model: None,
            body_index,
            body_name: body.name.clone(),
            boundary: job::DielectricBoundary::ClosedMesh,
            ior,
            absorption_per_mm: if body_index == 0 { [0.1, 0.2, 0.0] } else { [0.3, 0.0, 0.4] },
            thickness_mm: 0.0,
            shadow_transmission: false,
            subsurface: None,
        })
        .collect();
    let options = RaytraceOptions {
        path: Some(job::CpuPathOptions { max_depth: 16 }),
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 9 }),
        budget: job::CpuBudget::Studio,
        dielectrics,
        lights: Some(vec![]),
        shadows: true,
        environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..Default::default()
    };
    let mut request = dielectric::tests::request(options.clone());
    request.camera = crate::Camera { distance: 1.0, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, roll: 0.0, ..request.camera };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
fn opus_serial_optics_fixture(count: usize, depth: u8, inside_last: bool) -> ImageTask {
    let mut meshes = Vec::new();
    for index in 0..count {
        let mut body = dielectric::tests::box_body(0.25);
        body.name = format!("OPUS optical target {index}");
        let offset = if inside_last { Vec3::new(30.0 * (index as f64 - (count - 1) as f64), 0.0, 0.0) } else { Vec3::new(0.0, 0.0, index as f64) };
        for point in &mut Arc::make_mut(&mut body.mesh).positions {
            *point += offset;
        }
        meshes.push(body);
    }
    let scene = ProductScene { bounds: meshes.iter().fold(Aabb3::EMPTY, |bounds, mesh| bounds.union(&mesh.mesh.bounds())), meshes };
    let dielectrics = scene
        .meshes
        .iter()
        .enumerate()
        .map(|(body_index, body)| job::DielectricOptions {
            model: None,
            body_index,
            body_name: body.name.clone(),
            boundary: job::DielectricBoundary::ClosedMesh,
            ior: 1.0,
            absorption_per_mm: [0.1, 0.2, 0.0],
            thickness_mm: 0.0,
            shadow_transmission: false,
            subsurface: None,
        })
        .collect();
    let options = RaytraceOptions {
        path: Some(job::CpuPathOptions { max_depth: depth }),
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: 1, seed: 9 }),
        budget: job::CpuBudget::Studio,
        dielectrics,
        lights: Some(vec![]),
        shadows: true,
        environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        ..Default::default()
    };
    let mut request = dielectric::tests::request(options.clone());
    request.camera.fov = 0.00000001;
    if inside_last {
        request.camera.distance = 0.125;
    }
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_opus_thirty_serial_optics_need_deeper_paths_and_match_exact_beer_lengths() {
    let (actual, _) = path_mean(opus_serial_optics_fixture(30, job::MAX_PATH_DEPTH, false));
    let expected = [(-0.1_f64 * 7.5).exp(), (-0.2_f64 * 7.5).exp(), 1.0];
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-10, "30 independent solid lengths: {actual} vs {expected}");
    }
    assert_eq!(path_mean(opus_serial_optics_fixture(30, 16, false)).0, [0.0; 3], "the old depth cap truncates the same path");
}
#[test]
fn cpu_opus_128_targets_classify_inside_the_last_ordinal_without_bitset_aliases() {
    let (actual, _) = path_mean(opus_serial_optics_fixture(job::MAX_OPTICAL_TARGETS, job::MAX_PATH_DEPTH, true));
    let expected = [(-0.1_f64 * 0.125).exp(), (-0.2_f64 * 0.125).exp(), 1.0];
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-10, "body127 initial-medium Beer: {actual} vs {expected}");
    }
}

#[test]
fn cpu_general_glass_nested_index_matched_media_have_independent_physical_beer_lengths_and_raw_master() {
    let task = cpu_general_glass_fixture(8, true, 1.0);
    let (actual, _) = path_mean(task);
    // Two outer segments of0.5mm replace, not add to, the1mm inner medium.
    let expected = [(-0.4_f64).exp(), (-0.2_f64).exp(), (-0.4_f64).exp()];
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "{a} vs physical nested Beer{e}");
    }
    let output = complete_presentation(cpu_general_glass_fixture(8, true, 1.0), 1);
    assert_eq!(output, complete_presentation(cpu_general_glass_fixture(8, true, 1.0), 16384));
    for (samples, png) in &output.0 {
        assert_eq!(*png, complete_presentation(cpu_general_glass_fixture(*samples, true, 1.0), 3).1);
    }
    let raw = native_exr_pixels(output.2.as_deref().unwrap());
    assert_eq!(raw.get_pixel(0, 0).0, [expected[0] as f32, expected[1] as f32, expected[2] as f32, 1.0]);
}
#[test]
fn cpu_general_glass_fresnel_delta_prefixes_match_an_independent_normal_slab_counter_estimator() {
    let samples = 64;
    let mut expected = [0.0; 3];
    for ordinal in 0..samples {
        let mut inside = false;
        let mut throughput = [1.0; 3];
        let mut result = [0.0; 3];
        for depth in 0..15_u64 {
            if inside {
                for (t, sigma) in throughput.iter_mut().zip([0.1_f64, 0.2, 0.0]) {
                    *t *= (-sigma * 2.0).exp();
                }
            }
            let [branch, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_4000 + depth * 0x10000);
            if branch < 0.04 {
                if !inside {
                    result = throughput;
                    break;
                }
            } else if inside {
                for t in &mut throughput {
                    *t *= 9.0 / 4.0;
                }
                result = throughput;
                break;
            } else {
                for t in &mut throughput {
                    *t *= 4.0 / 9.0;
                }
                inside = true;
            }
        }
        for (sum, value) in expected.iter_mut().zip(result) {
            *sum += value / f64::from(samples);
        }
    }
    let (actual, _) = path_mean(cpu_general_glass_fixture(samples, false, 1.5));
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "actual film{a} vs independent Fresnel/Beer path{e}");
    }
    let output = complete_presentation(cpu_general_glass_fixture(samples, false, 1.5), 1);
    assert_eq!(output, complete_presentation(cpu_general_glass_fixture(samples, false, 1.5), 16384));
    for (samples, png) in &output.0 {
        assert_eq!(*png, complete_presentation(cpu_general_glass_fixture(*samples, false, 1.5), 7).1);
    }
}
#[test]
fn cpu_general_closed_octahedron_is_consumed_by_real_path_film_instead_of_the_box_preview() {
    let base = cpu_general_glass_fixture(4, false, 1.0);
    let mut scene = base.scene.clone();
    let p = vec![Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
    let mut triangles = vec![];
    let ring = [0, 2, 1, 3];
    for i in 0..4 {
        let a = ring[i];
        let b = ring[(i + 1) % 4];
        triangles.extend([[a, b, 4], [b, a, 5]]);
    }
    scene.meshes[0].mesh = Arc::new(Mesh { positions: p, triangles, ..Default::default() });
    scene.bounds = scene.meshes[0].mesh.bounds();
    let mut options = base.options.clone();
    options.dielectrics[0].absorption_per_mm = [0.0; 3];
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    assert!(dielectric::certify(&scene, &dielectric::tests::input(job::DielectricBoundary::ClosedBox)).is_err());
    assert_eq!(path_mean(ImageTask::new(scene, request, CancelToken::default(), options).unwrap()).0, [1.0; 3]);
}
#[test]
fn cpu_general_glass_preparation_is_cancellable_prepaid_and_never_exports_a_partial_boundary() {
    for cancel in [false, true] {
        let mut task = cpu_general_glass_fixture(4, true, 1.0);
        let weak = Arc::downgrade(&task.scene.meshes[0].mesh);
        let bytes = super::super::glass_mesh::Task::required_bytes(&task.scene, &task.options).unwrap();
        assert!(bytes > 0 && task.admission.preparation_bytes >= bytes);
        while task.work.used() < 100 {
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        }
        assert!(task.glass.first().is_some_and(|g| !g.ready()));
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
        if cancel {
            task.cancel.cancel();
        } else {
            task.work = WorkBudget::new(1).unwrap();
            task.step_with_previews(1).unwrap();
        }
        let error = task.step_with_previews(1).err().unwrap();
        assert_eq!(error.code, if cancel { job::ErrorCode::Cancelled } else { job::ErrorCode::LimitExceeded });
        assert!(task.take_linear_exr().is_none());
        drop(task);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn cpu_general_glass_delta_reaches_real_hdr_and_area_emitter_without_losing_their_pdf_mass() {
    let expected_beer = [(-0.2_f64).exp(), (-0.4_f64).exp(), 1.0];
    for area in [false, true] {
        let base = cpu_general_glass_fixture(4, false, 1.0);
        let mut options = base.options.clone();
        options.environment = None;
        if area {
            options.area_lights = Some(vec![job::RectangleLightOptions {
                center_mm: [0.0, 0.0, -2.0],
                edge_u_mm: [2.0, 0.0, 0.0],
                edge_v_mm: [0.0, 2.0, 0.0],
                color: [1.0, 0.5, 0.25],
                intensity: 2.0,
            }]);
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let input = if area {
            None
        } else {
            let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
            for _ in 0..8 {
                bytes.extend_from_slice(&[128, 64, 32, 129]);
            }
            Some(crate::lighting::environment_input::EnvironmentInput::new(crate::lighting::HdrSource::new(bytes).unwrap(), 0.0, 1.0, 16).unwrap())
        };
        let task = ImageTask::new_with_environment(base.scene.clone(), request, CancelToken::default(), options, input).unwrap();
        let output = complete_presentation(task, 1);
        let raw = native_exr_pixels(output.2.as_deref().unwrap());
        let scale = if area { 2.0 } else { 1.0 };
        assert_eq!(
            raw.get_pixel(0, 0).0,
            [(expected_beer[0] * scale) as f32, (expected_beer[1] * scale * 0.5) as f32, (expected_beer[2] * scale * 0.25) as f32, 1.0]
        );
    }
}
fn cpu_glass_vertical_sheet_fixture(samples: u32, depth: u8) -> ImageTask {
    let base = path_fixture(samples, depth, false);
    let mut scene = base.scene.clone();
    // Keep the opaque receiver compact so the independent primary point does not
    // inherit cancellation from a million-millimetre numerical test plane.
    for p in &mut Arc::make_mut(&mut scene.meshes[0].mesh).positions {
        p.x *= 0.0001;
        p.y *= 0.0001;
    }
    let sheet = ProductMesh {
        transmission: None,
        name: "Vertical sheet".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(2.0, -10.0, 1.0), Vec3::new(2.0, -10.0, 5.0), Vec3::new(2.0, 10.0, 5.0), Vec3::new(2.0, 10.0, 1.0)],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: None,
        faces: vec![],
    };
    scene.meshes.push(sheet);
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let mut options = base.options.clone();
    options.dielectrics = vec![job::DielectricOptions {
        model: None,
        body_index: 1,
        body_name: "Vertical sheet".into(),
        boundary: job::DielectricBoundary::ThinSheet,
        ior: 1.5,
        absorption_per_mm: [0.0; 3],
        thickness_mm: 0.0,
        shadow_transmission: true,
        subsurface: None,
    }];
    options.output = Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() }));
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_glass_thin_ordered_visibility_and_bsdf_chains_match_independent_sample_count_mis() {
    use std::f64::consts::{PI, TAU};
    let samples = 128;
    let sheet_probability = |cosine: f64| {
        let ct = (1.0 - (1.0 - cosine * cosine) / 2.25).sqrt();
        let fs = ((cosine - 1.5 * ct) / (cosine + 1.5 * ct)).powi(2);
        let fp = ((1.5 * cosine - ct) / (1.5 * cosine + ct)).powi(2);
        let r = 0.5 * (2.0 * fs / (1.0 + fs) + 2.0 * fp / (1.0 + fp));
        (r, 1.0 - r)
    };
    let hit_sheet = |p: Vec3, d: Vec3| {
        if d.x <= 0.0 {
            return false;
        }
        let t = (2.0 - p.x) / d.x;
        let q = p + d * t;
        t > 0.0 && q.y >= -10.0 && q.y <= 10.0 && q.z >= 1.0 && q.z <= 5.0
    };
    for source_count in [1_u32, 4, 16, 64] {
        for depth in [2, 3] {
            let mut task = cpu_glass_vertical_sheet_fixture(samples, depth);
            if source_count > 1 {
                let mut options = task.options.clone();
                options.environment = None;
                let request = ImageRequest { raytrace: Some(options.clone()), ..task.request.clone() };
                let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
                for _ in 0..8 {
                    bytes.extend_from_slice(&[128, 128, 128, 129]);
                }
                let environment = crate::lighting::environment_input::EnvironmentInput::new(
                    crate::lighting::HdrSource::new(bytes).unwrap(),
                    0.0,
                    1.0,
                    source_count,
                )
                .unwrap();
                task = ImageTask::new_with_environment(task.scene.clone(), request, CancelToken::default(), options, Some(environment)).unwrap();
            }
            while task.bvh.is_none() {
                assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            }
            let prepared = PreparedScene { bvh: task.bvh.as_ref().unwrap(), primitives: &task.looks, source: &task.scene };
            let mut diagnostic = WorkBudget::new(100_000).unwrap();
            let mut expected = 0.0;
            let mut transmitted = 0;
            let mut reflected = 0;
            let mut connected = 0;
            for ordinal in 0..samples {
                let [x, y] = progressive::variates(9, 0, ordinal, 0);
                let ray = primary_ray(&task.request, x, y, task.clip).unwrap();
                let hit = prepared.resolve_hit(ray, prepared.bvh.closest_hit(&ray, &mut diagnostic, &task.cancel).unwrap().unwrap()).unwrap();
                assert_eq!(hit.body_index, 0, "primary must see the opaque receiver, not the optical test panel");
                assert_eq!(hit.geometric_normal, Vec3::Z);
                let p = hit.position;
                let light_pdf = f64::from(source_count) / (4.0 * PI);
                let side = match source_count {
                    1 => 1,
                    4 => 2,
                    16 => 4,
                    _ => 8,
                };
                for source_sample in 0..source_count {
                    let [u, v] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000 + u64::from(source_sample));
                    let u = ((f64::from(source_sample / side) + u) / f64::from(side)).max(0.0_f64.next_up()).min(1.0_f64.next_down());
                    let v = ((f64::from(source_sample % side) + v) / f64::from(side)).max(0.0_f64.next_up()).min(1.0_f64.next_down());
                    assert!(u > 0.0 && u < 1.0 && v > 0.0 && v < 1.0);
                    let z = 1.0 - 2.0 * u;
                    let r = (1.0 - z * z).sqrt();
                    let light = Vec3::new(r * (TAU * v).cos(), r * (TAU * v).sin(), z);
                    if z > 0.0 {
                        let bsdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
                        let t = if hit_sheet(p, light) {
                            connected += 1;
                            sheet_probability(light.x).1
                        } else {
                            1.0
                        };
                        let competitor = if depth == 2 && t != 1.0 { 0.0 } else { bsdf * t };
                        expected +=
                            2.0 * z * t * (light_pdf * light_pdf / (light_pdf * light_pdf + competitor * competitor)) / f64::from(source_count);
                    }
                }
                let [component, u] = progressive::variates(9, 0, ordinal, 0x5041_5448_3000);
                let [v, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_3001);
                let z = if component < 0.5 { (1.0 - u).sqrt() } else { 1.0 - 2.0 * u };
                if z <= 0.0 {
                    continue;
                }
                let r = (1.0 - z * z).sqrt();
                let direction = Vec3::new(r * (TAU * v).cos(), r * (TAU * v).sin(), z);
                let pdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
                let weight = if hit_sheet(p, direction) {
                    if depth == 2 {
                        continue;
                    }
                    let (reflection, transmission) = sheet_probability(direction.x);
                    let [branch, _] = progressive::variates(9, 0, ordinal, 0x5041_5449_4000);
                    if branch < reflection {
                        reflected += 1;
                        1.0
                    } else {
                        transmitted += 1;
                        (pdf * transmission).powi(2) / ((pdf * transmission).powi(2) + light_pdf * light_pdf)
                    }
                } else {
                    pdf * pdf / (pdf * pdf + light_pdf * light_pdf)
                };
                expected += 0.5 * z / (PI * pdf) * weight;
            }
            assert!(connected > 0, "real NEE connections must cross the sheet");
            if depth == 3 {
                assert!(transmitted > 0 && reflected > 0, "both actual dielectric branches need fixed-seed coverage");
            }
            expected /= f64::from(samples);
            let (actual, _) = path_mean(task);
            for a in actual {
                assert!((a - expected).abs() < 1e-11, "depth{depth}: actual{a} vs independent sheet/BSDF/NEE{expected}");
            }
        }
    }
}

#[test]
fn cpu_general_glass_keeps_an_opaque_surface_at_its_exit_instead_of_skipping_the_contact() {
    let base = cpu_general_glass_fixture(4, false, 1.0);
    let mut scene = base.scene.clone();
    let receiver = ProductMesh {
        transmission: None,
        name: "Opaque contact".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }),
        color: [0; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap()),
        faces: vec![],
    };
    scene.meshes.push(receiver);
    let mut options = base.options.clone();
    options.environment = None;
    options.emissions = Some(vec![job::SurfaceEmission {
        body_index: 1,
        body: "Opaque contact".into(),
        color: [1.0, 0.5, 0.25],
        strength: 2.0,
        sides: job::EmissionSides::Both,
    }]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    let mean = path_mean(ImageTask::new(scene, request, CancelToken::default(), options).unwrap()).0;
    let expected = [2.0 * (-0.2_f64).exp(), (-0.4_f64).exp(), 0.5];
    for (a, e) in mean.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "opaque material at glass exit: {a} vs Beer-scaled emission{e}");
    }
}

#[test]
fn cpu_general_glass_adaptive_pilots_keep_production_fresnel_ordinals_raw_and_filtered_matte_identical() {
    let build = |adaptive: bool, pilot_seed: bool| {
        let base = cpu_general_glass_fixture(16, false, 2.5);
        let mut options = base.options.clone();
        options.denoise = true;
        if adaptive {
            options.adaptive = Some(Box::new(job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }));
        }
        if pilot_seed {
            options.progressive.as_mut().unwrap().seed ^= 0xa511_e9b3;
        }
        let request = ImageRequest { transparent_background: true, raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    let reference = complete_presentation(build(false, false), 1);
    let pilot = complete_presentation(build(false, true), 1);
    assert_ne!(reference.2, pilot.2, "the real Fresnel/Beer fixture must distinguish independent pilot radiance");
    let run = |quantum| {
        let mut task = build(true, false);
        let mut previews = 0;
        for _ in 0..2_000_000 {
            match task.step_with_previews(quantum).unwrap() {
                ImagePoll::Pending(_) => {}
                ImagePoll::Preview { .. } => {
                    previews += 1;
                    assert!(task.take_linear_exr().is_none());
                }
                ImagePoll::Complete(png) => {
                    let raw = task.take_linear_exr().unwrap();
                    let report = task.sampling_report().unwrap();
                    return (png, raw, previews, report, task.work.used());
                }
            }
        }
        panic!("bounded adaptive glass producer did not complete")
    };
    let actual = run(1);
    assert_eq!(actual, run(16384));
    assert_eq!(actual.0, reference.1);
    assert_eq!(actual.1, reference.2.unwrap());
    assert_eq!(actual.2, 31);
    assert_eq!((actual.3.production_samples, actual.3.primary_path_evaluations, actual.3.pilot_samples_per_pixel), (16, 32, 16));
    let mut cancelled = build(true, false);
    let mut saw_preview = false;
    for _ in 0..200_000 {
        if matches!(cancelled.step_with_previews(1).unwrap(), ImagePoll::Preview { .. }) {
            saw_preview = true;
            break;
        }
    }
    assert!(saw_preview, "cancel must interrupt actual glass sampling after a completed pilot image");
    assert!(cancelled.take_linear_exr().is_none());
    cancelled.cancel.cancel();
    assert_eq!(cancelled.step_with_previews(1).err().unwrap().code, job::ErrorCode::Cancelled);
    assert!(cancelled.take_linear_exr().is_none());
}

fn cpu_inside_glass_fixture(samples: u32, nested: bool, endpoint: u8, lens: Option<job::ThinLens>) -> ImageTask {
    let base = cpu_general_glass_fixture(samples, nested, 1.0);
    let mut scene = base.scene.clone();
    let mut options = base.options.clone();
    options.lens = lens;
    if endpoint != 0 {
        let z = if endpoint == 1 { 0.75 } else { 0.6 };
        let body = scene.meshes.len();
        scene.meshes.push(ProductMesh {
            transmission: None,
            name: "Inside opaque".into(),
            mesh: Arc::new(Mesh {
                positions: vec![Vec3::new(-3.0, -3.0, z), Vec3::new(3.0, -3.0, z), Vec3::new(3.0, 3.0, z), Vec3::new(-3.0, 3.0, z)],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                ..Default::default()
            }),
            color: [0; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap()),
            faces: vec![],
        });
        options.environment = None;
        if endpoint == 1 {
            options.emissions = Some(vec![job::SurfaceEmission {
                body_index: body,
                body: "Inside opaque".into(),
                color: [1.0, 0.5, 0.25],
                strength: 2.0,
                sides: job::EmissionSides::Both,
            }]);
        } else {
            options.area_lights = Some(vec![job::RectangleLightOptions {
                center_mm: [0.0, 0.0, 0.75],
                edge_u_mm: [2.0, 0.0, 0.0],
                edge_v_mm: [0.0, 2.0, 0.0],
                color: [1.0, 0.5, 0.25],
                intensity: 2.0,
            }]);
        }
    }
    let camera = crate::Camera {
        target: Vec3::new(0.0, 0.0, 0.5),
        distance: 0.5,
        fov: 0.6,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        ..base.request.camera
    };
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
fn inside_master_pixel(master: &[u8]) -> [f32; 4] {
    assert_eq!(&master[..4], &[0x76, 0x2f, 0x31, 0x01]);
    // Every caller owns a1x1 native, uncompressed, FLOAT A/B/G/R master.
    let tail = &master[master.len() - 16..];
    let f = |i| f32::from_le_bytes(tail[i..i + 4].try_into().unwrap());
    [f(12), f(8), f(4), f(0)]
}
#[test]
fn cpu_inside_glass_starts_beer_at_the_physical_eye_before_opaque_or_analytic_emission() {
    let samples = 16;
    for nested in [false, true] {
        for endpoint in [0, 1, 2] {
            let mut task = cpu_inside_glass_fixture(samples, nested, endpoint, None);
            let camera = task.request.camera;
            while task.bvh.is_none() {
                assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            }
            if endpoint != 0 {
                let prepared = PreparedScene { bvh: task.bvh.as_ref().unwrap(), primitives: &task.looks, source: &task.scene };
                let mut diagnostic = WorkBudget::new(100_000).unwrap();
                let ray = primary_ray(&task.request, 0.5, 0.5, task.clip).unwrap();
                let hit = prepared.resolve_hit(ray, prepared.bvh.closest_hit(&ray, &mut diagnostic, &task.cancel).unwrap().unwrap()).unwrap();
                assert_eq!(
                    hit.body_name, "Inside opaque",
                    "the closest geometry must precede the Glass exit, so first-hit medium guessing cannot prove this case"
                );
                assert_eq!(hit.position.z, if endpoint == 1 { 0.75 } else { 0.6 });
            }
            let mut expected = [0.0; 3];
            for ordinal in 0..samples {
                let [x, y] = progressive::variates(9, 0, ordinal, 0);
                let d = camera.ray(x, y, 1.0, 1.0).1;
                let cosine = -d.z;
                assert!(cosine > 0.9);
                for (channel, sum) in expected.iter_mut().enumerate() {
                    let outer = [0.1_f64, 0.2, 0.0][channel];
                    let inner = if nested { [0.3_f64, 0.0, 0.4][channel] } else { outer };
                    let optical = if endpoint == 0 { if nested { 0.5 * (outer + inner) } else { outer } } else { 0.25 * inner };
                    let radiance = if endpoint == 0 { 1.0 } else { [2.0, 1.0, 0.5][channel] };
                    *sum += radiance * (-optical / cosine).exp() / f64::from(samples);
                }
            }
            let actual = path_mean(task).0;
            for (a, e) in actual.into_iter().zip(expected) {
                assert!((a - e).abs() < 1e-12, "nested={nested} endpoint={endpoint}: physical-eye Beer {a} vs{e}");
            }
        }
    }
}
#[test]
fn cpu_inside_glass_hdr_png_prefixes_quanta_raw_and_independent_production_are_stable() {
    let build = |samples| {
        let base = cpu_inside_glass_fixture(samples, true, 0, None);
        let mut options = base.options.clone();
        options.environment = None;
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            bytes.extend_from_slice(&[128, 64, 32, 129]);
        }
        let input = crate::lighting::environment_input::EnvironmentInput::new(crate::lighting::HdrSource::new(bytes).unwrap(), 0.0, 1.0, 16).unwrap();
        ImageTask::new_with_environment(base.scene.clone(), request, CancelToken::default(), options, Some(input)).unwrap()
    };
    let actual = complete_presentation(build(7), 1);
    assert_eq!(actual, complete_presentation(build(7), 16384));
    for (n, png) in &actual.0 {
        assert_eq!(*png, complete_presentation(build(*n), 3).1);
    }
    let camera = build(7).request.camera;
    let mut expected = [0.0; 3];
    for ordinal in 0..7 {
        let [x, y] = progressive::variates(9, 0, ordinal, 0);
        let c = -camera.ray(x, y, 1.0, 1.0).1.z;
        for (channel, sum) in expected.iter_mut().enumerate() {
            *sum += [1.0, 0.5, 0.25][channel] * (-[0.2_f64, 0.1, 0.2][channel] / c).exp() / 7.0;
        }
    }
    assert_eq!(inside_master_pixel(actual.2.as_deref().unwrap()), [expected[0] as f32, expected[1] as f32, expected[2] as f32, 1.0]);
}
#[test]
fn cpu_inside_glass_total_internal_reflection_retains_the_initial_medium_until_the_finite_depth_limit() {
    let base = cpu_inside_glass_fixture(16, false, 0, None);
    let mut options = base.options.clone();
    options.dielectrics[0].ior = 1.5;
    options.dielectrics[0].absorption_per_mm = [0.0; 3];
    let mut camera = base.request.camera;
    camera.fov = 0.2;
    camera.yaw = std::f64::consts::FRAC_PI_4;
    camera.pitch = (1.0 / 3.0_f64.sqrt()).asin();
    camera.distance = 0.25;
    camera.target = Vec3::new(0.0, 0.0, 1.0) - camera.back() * camera.distance;
    for ordinal in 0..16 {
        let [x, y] = progressive::variates(9, 0, ordinal, 0);
        let d = camera.ray(x, y, 1.0, 1.0).1;
        assert!(
            [d.x, d.y, d.z].into_iter().all(|v| v.abs() < (1.0 - 1.0 / 2.25_f64).sqrt()),
            "all axis-face encounters are analytically above the critical angle"
        );
    }
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
    assert_eq!(path_mean(ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()).0, [0.0; 3]);
}
#[test]
fn cpu_inside_glass_refuses_actual_near_clip_crossings_for_eye_and_pupil_without_publishing_a_frame() {
    for lens in [None, Some(job::ThinLens { aperture_radius_mm: 0.1, focus_distance_mm: 2.0 })] {
        let base = cpu_inside_glass_fixture(4, false, 0, lens);
        let mut request = base.request.clone();
        let eye = Vec3::new(0.0, 0.0, 0.0005);
        request.camera.target = eye - request.camera.back() * request.camera.distance;
        let mut task = ImageTask::new(base.scene.clone(), request, CancelToken::default(), base.options.clone()).unwrap();
        let mut failure = None;
        for _ in 0..100_000 {
            match task.step_with_previews(1) {
                Err(e) => {
                    failure = Some(e);
                    break;
                }
                Ok(ImagePoll::Pending(_)) => {}
                Ok(_) => panic!("near crossing must not publish any prefix or master"),
            }
        }
        let error = failure.expect("actual near-clip crossing must terminate");
        assert!(error.message.contains("near clipping"), "{error}");
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
    }
}
#[test]
fn cpu_inside_glass_origin_state_is_prepaid_reused_and_cancelled_before_partial_medium_use() {
    let mut task = cpu_inside_glass_fixture(4, true, 1, None);
    let with_preparation = task.admission.preparation_bytes;
    let with_total = task.admission.estimated_total_bytes;
    let mut plain = task.options.clone();
    plain.dielectrics = vec![];
    let request = ImageRequest { raytrace: Some(plain.clone()), ..task.request.clone() };
    let without = admit(&task.scene, &request, &plain).unwrap();
    let expected = task.options.retained_dielectric_bytes().unwrap()
        + task.request.raytrace.as_ref().unwrap().retained_dielectric_bytes().unwrap()
        + super::super::glass_mesh::Task::required_bytes(&task.scene, &task.options).unwrap()
        + std::mem::size_of::<CertifiedOptics>() as u64
        + super::super::origin::Task::required_bytes();
    assert_eq!(with_preparation - without.preparation_bytes, expected);
    assert_eq!(with_total - without.estimated_total_bytes, expected);
    let mut origin_steps = 0;
    for _ in 0..100_000 {
        let classifying = task.path.first().is_some_and(|s| s.stage_name() == "origin");
        let before = task.work.used();
        assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        if classifying {
            assert_eq!(task.work.used() - before, 1);
            origin_steps += 1;
        }
        if origin_steps >= 5 {
            break;
        }
    }
    assert!(origin_steps >= 5);
    let info = task.path.first().unwrap().origin_slot_info();
    assert_eq!((info.0, info.1), (1, 1));
    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
    task.cancel.cancel();
    assert_eq!(task.step_with_previews(1).err().unwrap().code, job::ErrorCode::Cancelled);
    assert_eq!(task.path.first().unwrap().origin_slot_info(), info);
    assert!(task.take_linear_exr().is_none());
    let mut outside = cpu_general_glass_fixture(4, false, 1.0);
    while outside.progressive.as_ref().unwrap().film.samples() == 0 {
        outside.step_with_previews(1).unwrap();
    }
    assert_eq!(outside.path.first().unwrap().origin_slot_info(), (0, 0, false), "established outside/orthographic origins allocate no classifier");
}

fn cpu_mixed_pupil_glass_fixture(samples: u32, boundary: bool) -> ImageTask {
    let lens = job::ThinLens { aperture_radius_mm: 1.0, focus_distance_mm: 2.0 };
    let base = cpu_inside_glass_fixture(samples, false, 0, Some(lens));
    let mut scene = base.scene.clone();
    let mut camera = base.request.camera;
    camera.target.x = 0.25;
    let right = if boundary {
        let [u, v] = progressive::variates(9, 0, 0, 0x4c454e53);
        let (r, up, _) = camera.basis();
        (camera.eye() + (r * (std::f64::consts::TAU * v).cos() + up * (std::f64::consts::TAU * v).sin()) * (lens.aperture_radius_mm * u.sqrt())).x
    } else {
        0.0
    };
    for p in &mut Arc::make_mut(&mut scene.meshes[0].mesh).positions {
        p.x = if p.x > 0.0 { right } else { right - 10.0 };
    }
    scene.bounds = scene.meshes[0].mesh.bounds();
    let request = ImageRequest { camera, transparent_background: true, raytrace: Some(base.options.clone()), ..base.request.clone() };
    ImageTask::new(scene, request, CancelToken::default(), base.options.clone()).unwrap()
}
#[test]
fn cpu_glass_lens_classifies_each_real_pupil_and_keeps_analytic_coverage_beer_prefixes_and_raw() {
    let samples = 16;
    let mut task = cpu_mixed_pupil_glass_fixture(samples, false);
    let camera = task.request.camera;
    let lens = task.options.lens.unwrap();
    let (right, up, back) = camera.basis();
    let mut expected = [0.0; 3];
    let mut hits = 0;
    let mut inside = 0;
    let mut outside = 0;
    for ordinal in 0..samples {
        let [x, y] = progressive::variates(9, 0, ordinal, 0);
        let [u, v] = progressive::variates(9, 0, ordinal, 0x4c454e53);
        let displacement =
            (right * (std::f64::consts::TAU * v).cos() + up * (std::f64::consts::TAU * v).sin()) * (lens.aperture_radius_mm * u.sqrt());
        let origin = camera.eye() + displacement;
        let pinhole = camera.ray(x, y, 1.0, 1.0).1;
        let aimed = pinhole * (lens.focus_distance_mm / pinhole.dot(-back)) - displacement;
        let direction = aimed / aimed.len();
        if origin.x < 0.0 {
            inside += 1;
        } else {
            outside += 1;
        }
        let mut enter = f64::NEG_INFINITY;
        let mut exit = f64::INFINITY;
        for (p, d, lo, hi) in [(origin.x, direction.x, -10.0, 0.0), (origin.y, direction.y, -10.0, 10.0), (origin.z, direction.z, 0.0, 2.0)] {
            if d == 0.0 {
                if p < lo || p > hi {
                    exit = f64::NEG_INFINITY;
                    break;
                }
            } else {
                let a = (lo - p) / d;
                let b = (hi - p) / d;
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
            }
        }
        let start = enter.max(0.0);
        if exit > start {
            if enter > 0.0 {
                assert!(enter > 0.01, "outside pupil must not cross a boundary in its clipped near segment");
            }
            assert!(exit > 0.01);
            hits += 1;
            let length = exit - start;
            for (sum, sigma) in expected.iter_mut().zip([0.1_f64, 0.2, 0.0]) {
                *sum += (-sigma * length).exp() / f64::from(samples);
            }
        }
    }
    assert!(inside > 0 && outside > 0 && hits < samples, "the real aperture must exercise both initial media and genuine misses");
    while task.progressive.as_ref().unwrap().film.samples() != samples {
        task.step_with_previews(1).unwrap();
    }
    let (mean, covered, n) = task.progressive.as_ref().unwrap().film.pixel(0).unwrap();
    assert_eq!((covered, n), (hits, samples));
    for (a, e) in mean.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "per-pupil analytic Beer{a} vs{e}");
    }
    let actual = complete_presentation(cpu_mixed_pupil_glass_fixture(samples, false), 1);
    assert_eq!(actual, complete_presentation(cpu_mixed_pupil_glass_fixture(samples, false), 16384));
    for (n, png) in &actual.0 {
        assert_eq!(*png, complete_presentation(cpu_mixed_pupil_glass_fixture(*n, false), 3).1);
    }
    assert_eq!(
        inside_master_pixel(actual.2.as_deref().unwrap()),
        [expected[0] as f32, expected[1] as f32, expected[2] as f32, hits as f32 / samples as f32]
    );
}
#[test]
fn cpu_glass_lens_refuses_an_actual_pupil_on_a_boundary_instead_of_guessing_air() {
    let mut task = cpu_mixed_pupil_glass_fixture(4, true);
    let mut failure = None;
    for _ in 0..100_000 {
        match task.step_with_previews(1) {
            Err(e) => {
                failure = Some(e);
                break;
            }
            Ok(ImagePoll::Pending(_)) => {}
            Ok(_) => panic!("a boundary pupil must not publish an image"),
        }
    }
    assert!(failure.expect("exact pupil contact must terminate").message.contains("exactly on"));
    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
    assert!(task.take_linear_exr().is_none());
}

#[test]
fn cpu_inside_glass_adaptive_reuses_the_origin_slot_but_discards_pilot_medium_and_lens_samples() {
    let build = |adaptive: bool, pilot: bool| {
        let base = cpu_mixed_pupil_glass_fixture(16, false);
        let mut options = base.options.clone();
        if adaptive {
            options.adaptive = Some(Box::new(job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }));
        }
        if pilot {
            options.progressive.as_mut().unwrap().seed ^= 0xa511_e9b3;
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    let reference = complete_presentation(build(false, false), 1);
    let pilot = complete_presentation(build(false, true), 1);
    assert_ne!(reference.2, pilot.2, "the physical pupil fixture must expose pilot leakage");
    let actual = complete_presentation(build(true, false), 1);
    assert_eq!(actual, complete_presentation(build(true, false), 16384));
    assert_eq!(actual.0.len(), 31);
    assert_eq!(actual.1, reference.1);
    assert_eq!(actual.2, reference.2);
}
#[test]
fn cpu_inside_glass_origin_slot_keeps_exact_capacity_and_no_sample_commit_on_work_exhaustion() {
    let mut task = cpu_inside_glass_fixture(4, true, 0, None);
    for _ in 0..100_000 {
        if task.path.first().is_some_and(|s| s.stage_name() == "origin" && !s.origin_slot_info().2) {
            break;
        }
        task.step_with_previews(1).unwrap();
    }
    assert_eq!(task.path.first().unwrap().origin_slot_info(), (1, 1, false));
    let before = task.work.used();
    task.work.limit = before;
    assert_eq!(task.step_with_previews(1).err().unwrap().code, job::ErrorCode::LimitExceeded);
    assert_eq!(task.work.used(), before);
    assert_eq!(task.path.first().unwrap().origin_slot_info(), (1, 1, false));
    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
    assert!(task.take_linear_exr().is_none());
    let mut reused = cpu_inside_glass_fixture(4, true, 0, None);
    let eye = reused.request.camera.eye();
    let mut seeded = false;
    for pass in 1..=2 {
        while reused.progressive.as_ref().unwrap().film.samples() < pass {
            reused.step_with_previews(1).unwrap();
            if !seeded && reused.path.first().is_some_and(|s| s.stage_name() == "trace") {
                let (chain, len, start) = reused.path.first().unwrap().initial_medium().unwrap();
                assert_eq!((len, &chain[..len]), (2, &[0_usize, 1][..]));
                assert_eq!(start, eye);
                seeded = true;
            }
        }
        assert_eq!(reused.path.first().unwrap().origin_slot_info(), (0, 1, false), "completed samples reuse exactly one prepaid slot");
    }
    assert!(seeded, "the classified initial medium must be visible before any first-hit lighting");
}

fn cpu_subsurface_fixture(samples: u32, nested: bool, endpoint: u8, depth: u8, rates: [f64; 2], g: f64, lens: Option<job::ThinLens>) -> ImageTask {
    let base = cpu_inside_glass_fixture(samples, nested, endpoint, lens);
    let mut options = base.options.clone();
    options.path = Some(job::CpuPathOptions { max_depth: depth });
    for (index, input) in options.dielectrics.iter_mut().enumerate() {
        input.subsurface = Some(crate::transmission::subsurface::Subsurface { scattering_per_mm: rates[index], anisotropy: g });
    }
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_subsurface_only_keeps_smooth_snell_direction_and_opt_in_caustics_keep_geometric_facets() {
    let smooth = Vec3::new(0.28, 0.0, 0.96);
    let incident = -Vec3::Z;
    let eta = 2.0_f64 / 3.0;
    let transmitted_cosine = (1.0 - (eta * smooth.x).powi(2)).sqrt();
    let smooth_direction = incident * eta + smooth * (eta * smooth.z - transmitted_cosine);
    assert!(smooth_direction.x < -0.09, "the smooth and geometric predictions must differ");
    let seed = (0..256).find(|&seed| progressive::variates(seed, 0, 0, 0x5041_5448_4000)[0] > 0.2).unwrap();
    for caustics in [false, true] {
        let base = cpu_general_glass_fixture(1, false, 1.5);
        let mut scene = base.scene.clone();
        let mesh = Arc::make_mut(&mut scene.meshes[0].mesh);
        mesh.normals = vec![smooth; mesh.positions.len()];
        let mut options = base.options.clone();
        options.path = Some(job::CpuPathOptions { max_depth: 2 });
        options.progressive.as_mut().unwrap().seed = seed;
        options.dielectrics[0].subsurface = Some(crate::transmission::subsurface::Subsurface { scattering_per_mm: 0.3, anisotropy: 0.4 });
        options.caustics = caustics.then_some(job::CausticOptions { photons: 1, ..Default::default() });
        let camera = crate::Camera { fov: 0.0, ..base.request.camera };
        let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
        let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
        let mut transmitted = None;
        for _ in 0..200_000 {
            if let Some((1, ray, _)) = task.path.first().and_then(|path| path.surface_source()) {
                // Observe the first refracted segment before HG free flight can
                // replace its direction. Geometry still owns medium membership.
                transmitted = Some(ray.direction());
                break;
            }
            assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
        }
        let actual = transmitted.expect("fixture must trace the first transmitted segment");
        let expected = if caustics { incident } else { smooth_direction };
        assert!((actual - expected).len() < 1e-12, "caustics={caustics}: {actual:?} vs Snell direction {expected:?}");
    }
}

#[test]
fn cpu_subsurface_first_vertex_has_independent_analog_survival_and_physical_eye_pupil_beer() {
    let samples = 64;
    for lens in [None, Some(job::ThinLens { aperture_radius_mm: 0.1, focus_distance_mm: 2.0 })] {
        for endpoint in [1, 2] {
            let build = || cpu_subsurface_fixture(samples, false, endpoint, 1, [2.0, 0.0], 0.4, lens);
            let camera = build().request.camera;
            let (right, up, back) = camera.basis();
            let mut expected = [0.0; 3];
            let (mut collisions, mut survivors) = (0, 0);
            for ordinal in 0..samples {
                let [x, y] = progressive::variates(9, 0, ordinal, 0);
                let mut origin = camera.eye();
                let mut direction = camera.ray(x, y, 1.0, 1.0).1;
                if let Some(lens) = lens {
                    let [u, v] = progressive::variates(9, 0, ordinal, 0x4c454e53);
                    let displacement =
                        (right * (std::f64::consts::TAU * v).cos() + up * (std::f64::consts::TAU * v).sin()) * (lens.aperture_radius_mm * u.sqrt());
                    origin += displacement;
                    let aimed = direction * (lens.focus_distance_mm / direction.dot(-back)) - displacement;
                    direction = aimed / aimed.len();
                }
                let length = (0.75 - origin.z) / direction.z;
                let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_5000);
                // Direct CDF survival decision, independent of free_flight().
                if u < -(-2.0 * length).exp_m1() {
                    collisions += 1;
                    continue;
                }
                survivors += 1;
                for (channel, sum) in expected.iter_mut().enumerate() {
                    *sum += [2.0, 1.0, 0.5][channel] * (-[0.1_f64, 0.2, 0.0][channel] * length).exp() / f64::from(samples);
                }
            }
            assert!(collisions > 0 && survivors > 0);
            for (a, e) in path_mean(build()).0.into_iter().zip(expected) {
                assert!((a - e).abs() < 1e-12, "physical origin survival/Beer: {a} vs{e}");
            }
            let output = complete_presentation(build(), 1);
            assert_eq!(
                native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0,
                [expected[0] as f32, expected[1] as f32, expected[2] as f32, 1.0]
            );
        }
    }
}
#[test]
fn cpu_nested_subsurface_uses_each_current_collision_rate_and_absorption_to_the_finite_depth_endpoint() {
    let samples = 64;
    let build = || cpu_subsurface_fixture(samples, true, 0, 3, [1.3, 0.8], -0.6, None);
    let camera = build().request.camera;
    let mut expected = [0.0; 3];
    let mut surviving = 0;
    for ordinal in 0..samples {
        let [x, y] = progressive::variates(9, 0, ordinal, 0);
        let distance = 0.5 / -camera.ray(x, y, 1.0, 1.0).1.z;
        let [inner, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_5000);
        let [outer, _] = progressive::variates(9, 0, ordinal, 0x5041_5449_5000);
        if inner < -(-0.8 * distance).exp_m1() || outer < -(-1.3 * distance).exp_m1() {
            continue;
        }
        surviving += 1;
        // A collision plus both interfaces cannot escape in three vertices.
        for (channel, sum) in expected.iter_mut().enumerate() {
            *sum += (-[0.4_f64, 0.2, 0.4][channel] * distance).exp() / f64::from(samples);
        }
    }
    assert!(surviving > 0 && surviving < samples);
    for (a, e) in path_mean(build()).0.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "nested analog/Beer/depth: {a} vs{e}");
    }
}

fn independent_single_box_subsurface(task: &ImageTask, environment: &impl Fn(Vec3) -> Vec3) -> [f64; 3] {
    let samples = task.options.progressive.unwrap().samples_per_pixel;
    let input = &task.options.dielectrics[0];
    let scattering = input.subsurface.unwrap();
    let camera = task.request.camera;
    let depth = task.options.path.unwrap().max_depth;
    let mut expected = Vec3::ZERO;
    for ordinal in 0..samples {
        let [x, y] = progressive::variates(9, 0, ordinal, 0);
        let mut point = camera.eye();
        let mut direction = camera.ray(x, y, 1.0, 1.0).1;
        let mut weight = Vec3::new(1.0, 1.0, 1.0);
        for vertex in 0..depth {
            let mut distance = f64::INFINITY;
            for (p, d, lo, hi) in [(point.x, direction.x, -10.0, 10.0), (point.y, direction.y, -10.0, 10.0), (point.z, direction.z, 0.0, 2.0)] {
                if d != 0.0 {
                    distance = distance.min(((if d > 0.0 { hi } else { lo }) - p) / d);
                }
            }
            assert!(distance > 0.0 && distance.is_finite());
            let domain = 0x5041_5448_0000 + u64::from(vertex) * 0x10000;
            let [u, _] = progressive::variates(9, 0, ordinal, domain + 0x5000);
            // Independent exp inverse and exact analytic slab exit, not traversal.
            let u = if u == 0.0 { 2.0_f64.powi(-54) } else { u };
            let travel = -(1.0 - u).ln() / scattering.scattering_per_mm;
            let length = travel.min(distance);
            weight = Vec3::new(
                weight.x * (-input.absorption_per_mm[0] * length).exp(),
                weight.y * (-input.absorption_per_mm[1] * length).exp(),
                weight.z * (-input.absorption_per_mm[2] * length).exp(),
            );
            if travel >= distance {
                if vertex + 1 < depth {
                    let light = environment(direction);
                    expected += Vec3::new(weight.x * light.x, weight.y * light.y, weight.z * light.z);
                }
                break;
            }
            if vertex + 1 == depth {
                break;
            }
            point += direction * travel;
            let [u, v] = progressive::variates(9, 0, ordinal, domain + 0x5001);
            let g = scattering.anisotropy;
            let cosine = if g == 0.0 {
                2.0 * u - 1.0
            } else {
                let q = (1.0 - g * g) / (1.0 - g + 2.0 * g * u);
                (1.0 + g * g - q * q) / (2.0 * g)
            };
            let radius = (1.0 - cosine * cosine).max(0.0).sqrt();
            let axis = if direction.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
            let tangent = axis.cross(direction);
            let tangent = tangent / tangent.len();
            let other = direction.cross(tangent);
            direction =
                tangent * (radius * (std::f64::consts::TAU * v).cos()) + other * (radius * (std::f64::consts::TAU * v).sin()) + direction * cosine;
            direction = direction / direction.len();
        }
    }
    expected = expected / f64::from(samples);
    [expected.x, expected.y, expected.z]
}
#[test]
fn cpu_subsurface_hg_walk_and_beer_match_an_independent_per_ordinal_slab_oracle() {
    for g in [-0.6, 0.0, 0.6] {
        let build = || cpu_subsurface_fixture(64, false, 0, 16, [1.0, 0.0], g, None);
        let expected = independent_single_box_subsurface(&build(), &|_| Vec3::new(1.0, 1.0, 1.0));
        for (a, e) in path_mean(build()).0.into_iter().zip(expected) {
            assert!((a - e).abs() < 1e-12, "g{g}: independent free-flight/HG/Beer{a} vs{e}");
        }
    }
}
#[test]
fn cpu_subsurface_inactive_some_preserves_original_radiance_work_every_png_prefix_and_raw_bits() {
    let build = |present| {
        let base = cpu_inside_glass_fixture(8, true, 0, None);
        let mut options = base.options.clone();
        if present {
            for input in &mut options.dielectrics {
                input.subsurface = Some(crate::transmission::subsurface::Subsurface { scattering_per_mm: 0.0, anisotropy: 0.95 });
            }
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    assert_eq!(path_mean(build(false)), path_mean(build(true)), "inactive fields add no transport work or random dimension");
    assert_eq!(complete_presentation(build(false), 1), complete_presentation(build(true), 1));
    assert_eq!(build(false).admission.estimated_total_bytes, build(true).admission.estimated_total_bytes);
    let inactive = build(true);
    let mut options = inactive.options.clone();
    for input in &mut options.dielectrics {
        input.subsurface.as_mut().unwrap().scattering_per_mm = 0.3;
    }
    let request = ImageRequest { raytrace: Some(options.clone()), ..inactive.request.clone() };
    let active = ImageTask::new(inactive.scene.clone(), request, CancelToken::default(), options).unwrap();
    assert_eq!(
        serde_json::to_value(active.admission).unwrap(),
        serde_json::to_value(inactive.admission).unwrap(),
        "active events reuse the prepaid classifier and single path slot, not a new image plane"
    );
}
#[test]
fn cpu_subsurface_white_furnace_keeps_unit_radiance_without_absorption_or_unfinished_paths() {
    let base = cpu_subsurface_fixture(32, false, 0, 16, [0.1, 0.0], 0.0, None);
    let mut options = base.options.clone();
    options.dielectrics[0].absorption_per_mm = [0.0; 3];
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    let mut task = ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap();
    let mut scattered = false;
    while task.progressive.as_ref().unwrap().film.samples() < 32 {
        scattered |= task.path.first().is_some_and(|state| state.stage_name() == "medium_scatter");
        task.step_with_previews(1).unwrap();
    }
    assert!(scattered, "the white furnace must include a real collision, not only inactive/no-event paths");
    assert_eq!(task.progressive.as_ref().unwrap().film.pixel(0).unwrap().0, [1.0; 3]);
}
#[test]
fn cpu_subsurface_prefixes_quanta_alpha_finishing_and_adaptive_production_keep_the_raw_walk_exact() {
    let build = |samples, adaptive, pilot, finishing| {
        let base = cpu_subsurface_fixture(samples, false, 0, 16, [1.0, 0.0], 0.3, None);
        let mut options = base.options.clone();
        if adaptive {
            options.adaptive = Some(Box::new(job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }));
        }
        if pilot {
            options.progressive.as_mut().unwrap().seed ^= 0xa511_e9b3;
        }
        if finishing {
            options.output.as_mut().unwrap().finishing.saturation = 0.2;
        }
        let request = ImageRequest { transparent_background: true, raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    let output = complete_presentation(build(8, false, false, false), 1);
    assert_eq!(output, complete_presentation(build(8, false, false, false), 16384));
    for (n, png) in &output.0 {
        assert_eq!(*png, complete_presentation(build(*n, false, false, false), 3).1);
    }
    assert_eq!(native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0[3], 1.0);
    let finished = complete_presentation(build(8, false, false, true), 1);
    assert_ne!(finished.1, output.1);
    assert_eq!(finished.2, output.2, "finishing cannot rewrite raw scattering radiance");
    let reference = complete_presentation(build(16, false, false, false), 1);
    assert_ne!(reference.2, complete_presentation(build(16, false, true, false), 1).2, "actual fixture must expose pilot leakage");
    let adaptive = complete_presentation(build(16, true, false, false), 1);
    assert_eq!(adaptive, complete_presentation(build(16, true, false, false), 16384));
    assert_eq!(adaptive.0.len(), 31);
    assert_eq!(adaptive.1, reference.1);
    assert_eq!(adaptive.2, reference.2);
}
#[test]
fn cpu_subsurface_collision_classification_yields_one_debit_and_cancels_or_exhausts_before_any_film_commit() {
    let build = || cpu_subsurface_fixture(4, false, 0, 16, [100.0, 0.0], 0.0, None);
    for cancel in [false, true] {
        let mut task = build();
        let mut decision = false;
        for _ in 0..100_000 {
            let phase = task.path.first().map(|p| p.stage_name());
            if phase == Some("medium_origin") {
                break;
            }
            let before = task.work.used();
            task.step_with_previews(1).unwrap();
            if phase == Some("medium") {
                decision = true;
                assert_eq!(task.work.used() - before, 1);
            }
        }
        assert!(decision);
        assert_eq!(task.path[0].stage_name(), "medium_origin");
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        for _ in 0..5 {
            let before = task.work.used();
            task.step_with_previews(1).unwrap();
            assert_eq!(task.work.used() - before, 1);
        }
        assert_eq!(task.path[0].origin_slot_info().1, 1);
        let before = task.work.used();
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = before;
        }
        assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { job::ErrorCode::Cancelled } else { job::ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), before);
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
    }
}

fn cpu_subsurface_contained_light_fixture(source_samples: u32) -> ImageTask {
    let base = cpu_subsurface_fixture(8, false, 1, 4, [0.1, 0.0], 0.2, None);
    let mut scene = base.scene.clone();
    scene.meshes.last_mut().unwrap().surface = Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 1.0, 1.0).unwrap());
    let mut options = base.options.clone();
    options.emissions = None;
    options.area_samples = Some(source_samples);
    options.area_lights = Some(vec![job::RectangleLightOptions {
        center_mm: [0.0, 0.0, 1.25],
        edge_u_mm: [4.0, 0.0, 0.0],
        edge_v_mm: [0.0, -4.0, 0.0],
        color: [1.0; 3],
        intensity: 1.0,
    }]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_subsurface_contained_nee_has_analytic_scattering_survival_and_matching_sample_count_mis() {
    for samples in [1, 4, 16, 64] {
        let mut task = cpu_subsurface_contained_light_fixture(samples);
        let receiver = task.scene.meshes.len() - 1;
        let mut range_steps = 0;
        let mut was_range = false;
        let mut last_range = None;
        let mut discoveries = std::collections::BTreeSet::new();
        for _ in 0..100_000 {
            if task.path.first().is_some_and(|p| {
                p.stage_name() == "shadow_beer"
                    && p.shadow_probe().is_some_and(|p| p.endpoint.is_some())
                    && p.surface_source().is_some_and(|(_, _, hit)| task.looks[hit.primitive_index].body_index == receiver)
            }) {
                break;
            }
            let range = task.path.first().is_some_and(|p| p.stage_name() == "medium_surface");
            if range && !was_range {
                let (depth, _, hit) = task.path[0].surface_source().unwrap();
                let key = (task.progressive.as_ref().unwrap().film.samples(), depth, hit.primitive_index);
                assert!(discoveries.insert(key), "a captured source hit discovers its physical range only once");
                last_range = Some(key);
                range_steps = 0;
            }
            let before = task.work.used();
            task.step_with_previews(1).unwrap();
            if range {
                range_steps += 1;
                assert_eq!(task.work.used() - before, 1);
            }
            was_range = range;
        }
        assert!(
            range_steps > 0 && range_steps <= task.scene.meshes.last().unwrap().mesh.triangles.len() + 2,
            "physical source ranges are discovered cooperatively once per hit"
        );
        assert_eq!(task.path[0].stage_name(), "shadow_beer");
        let (depth, incident, hit) = task.path[0].surface_source().unwrap();
        assert_eq!(last_range, Some((task.progressive.as_ref().unwrap().film.samples(), depth, hit.primitive_index)));
        let prepared = PreparedScene { bvh: task.bvh.as_ref().unwrap(), primitives: &task.looks, source: &task.scene };
        let captured_source = prepared.resolve_hit(incident, hit).unwrap();
        assert_eq!(captured_source.body_index, receiver);
        assert!(captured_source.vertices.iter().all(|vertex| vertex.z == 0.75), "the chosen captured receiver is the intended plane");
        let before = task.path[0].shadow_probe().unwrap();
        assert_eq!(
            before.from, captured_source.position,
            "source equality uses the actual reconstructed hit, not a guessed decimal plane coordinate"
        );
        assert_eq!(before.ray_origin, Some(before.from), "SSS light visibility must start on the actual physical surface, not a finite offset");
        let length = before.from.dist(before.endpoint.unwrap());
        assert_eq!(before.probability, 1.0);
        let survival = (-0.1 * length).exp();
        let expected =
            Vec3::new(before.contribution.x * (-0.2 * length).exp(), before.contribution.y * (-0.3 * length).exp(), before.contribution.z * survival);
        task.step_with_previews(1).unwrap();
        assert_eq!(task.path[0].stage_name(), "shadow_emitters", "the contained connection must have no boundary or opaque blocker");
        let after = task.path[0].shadow_probe().unwrap();
        assert!((after.probability - survival).abs() < 1e-15);
        assert!((after.contribution - expected).len() < 1e-12);
        for _ in 0..16 {
            if task.path[0].stage_name() == "add" {
                break;
            }
            task.step_with_previews(1).unwrap();
        }
        assert_eq!(task.path[0].stage_name(), "add");
        let [light, bsdf] = after.mis.unwrap();
        let weight = light * light / (light * light + (bsdf * survival).powi(2));
        let expected = before.radiance + expected * weight;
        task.step_with_previews(1).unwrap();
        assert!(
            (task.path[0].shadow_probe().unwrap().radiance - expected).len() < 1e-12,
            "sourceN={samples}: contained MIS must include analog survival"
        );
    }
}
#[test]
fn cpu_subsurface_sampled_no_collision_endpoint_carries_the_same_survival_density_without_double_attenuation() {
    let mut task = cpu_subsurface_contained_light_fixture(16);
    let mut witnessed = false;
    for _ in 0..200_000 {
        if let Some(state) = task.path.first().filter(|p| p.stage_name() == "medium") {
            let probe = state.medium_probe().unwrap();
            if probe.depth > 0
                && !probe.delta
                && let Some(endpoint) = probe.emitter
            {
                let length = probe.from.dist(endpoint);
                let ordinal = task.progressive.as_ref().unwrap().film.samples();
                let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_5000 + u64::from(probe.depth) * 0x10000);
                if u >= -(-0.1 * length).exp_m1() {
                    task.step_with_previews(1).unwrap();
                    assert_eq!(task.path[0].stage_name(), "resolve");
                    let after = task.path[0].medium_probe().unwrap();
                    assert_eq!(after.ray_origin, Some(probe.from), "SSS BSDF travel must share the physical source line");
                    assert_eq!(after.throughput, probe.throughput, "analog survival cannot be multiplied into sampled throughput a second time");
                    assert!((after.pdf - probe.pdf * (-0.1 * length).exp()).abs() < 1e-15);
                    witnessed = true;
                    break;
                }
            }
        }
        task.step_with_previews(1).unwrap();
    }
    assert!(witnessed, "a real BSDF continuation must reach the contained analytic emitter without a collision");
}

#[test]
fn cpu_subsurface_refuses_an_unrepresentable_actual_collision_instead_of_moving_its_source_boundary() {
    let base = cpu_subsurface_fixture(1, false, 0, 16, [100.0, 0.0], 0.0, None);
    let offset = Vec3::new(1e12 - 100.0, 1e12 - 100.0, 1e12 - 100.0);
    let mut scene = base.scene.clone();
    for mesh in &mut scene.meshes {
        for position in &mut Arc::make_mut(&mut mesh.mesh).positions {
            *position += offset;
        }
    }
    scene.bounds = scene.meshes[0].mesh.bounds();
    let seed = (0..100_000)
        .find(|seed| {
            let [u, _] = progressive::variates(*seed, 0, 0, 0x5041_5448_5000);
            let flight = -(1.0 - u).ln() / 100.0;
            flight > 0.0 && flight < 0.00001
        })
        .expect("fixed finite seed search must exercise sub-ULP flight");
    let mut options = base.options.clone();
    options.progressive.as_mut().unwrap().seed = seed;
    let mut camera = base.request.camera;
    camera.target += offset;
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    let mut failure = None;
    for _ in 0..100_000 {
        match task.step_with_previews(1) {
            Err(error) => {
                failure = Some(error);
                break;
            }
            Ok(ImagePoll::Pending(_)) => {}
            Ok(_) => panic!("an unrepresentable collision cannot publish an image"),
        }
    }
    assert!(failure.expect("precision refusal must terminate").message.contains("not representable"));
    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
    assert!(task.take_linear_exr().is_none());
}

#[test]
fn cpu_subsurface_real_nonconstant_hdr_matches_independent_walk_escape_texels_and_all_png_prefixes() {
    use std::f64::consts::{PI, TAU};
    let texels: [[u8; 4]; 8] = [
        [128, 32, 16, 129],
        [16, 128, 32, 129],
        [32, 16, 128, 129],
        [64, 96, 128, 129],
        [96, 64, 32, 129],
        [32, 96, 64, 129],
        [64, 32, 96, 129],
        [128, 64, 96, 129],
    ];
    let environment = |direction: Vec3| {
        let column = ((direction.y.atan2(direction.x).rem_euclid(TAU) * 4.0 / TAU) as usize).min(3);
        let row = ((direction.z.clamp(-1.0, 1.0).acos() * 2.0 / PI) as usize).min(1);
        let pixel = texels[row * 4 + column];
        Vec3::new(f64::from(pixel[0]), f64::from(pixel[1]), f64::from(pixel[2])) / 128.0
    };
    let build = |samples| {
        let base = cpu_subsurface_fixture(samples, false, 0, 16, [1.0, 0.0], -0.6, None);
        let options = RaytraceOptions { environment: None, ..base.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for pixel in texels {
            bytes.extend_from_slice(&pixel);
        }
        let hdr = crate::lighting::environment_input::EnvironmentInput::new(crate::lighting::HdrSource::new(bytes).unwrap(), 0.0, 1.0, 16).unwrap();
        ImageTask::new_with_environment(base.scene.clone(), request, CancelToken::default(), options, Some(hdr)).unwrap()
    };
    let expected = independent_single_box_subsurface(&build(64), &environment);
    for (actual, expected) in path_mean(build(64)).0.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-12, "independent HG/slab/nonconstant HDR{actual} vs{expected}");
    }
    let output = complete_presentation(build(7), 1);
    assert_eq!(output, complete_presentation(build(7), 16384));
    for (samples, png) in &output.0 {
        assert_eq!(*png, complete_presentation(build(*samples), 3).1);
    }
    let expected = independent_single_box_subsurface(&build(7), &environment);
    assert_eq!(native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0, [expected[0] as f32, expected[1] as f32, expected[2] as f32, 1.0]);
}

fn cpu_volume_fixture(samples: u32, depth: u8, perspective: bool, opaque: bool, volume: Option<job::volume::HomogeneousVolume>) -> ImageTask {
    let base = fixture(1, 1, Some(true));
    let body = ProductMesh {
        name: "Volume receiver".into(),
        transmission: None,
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }),
        color: [0; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap()),
        faces: vec![],
    };
    let scene =
        if opaque { ProductScene { bounds: body.mesh.bounds(), meshes: vec![body] } } else { ProductScene { bounds: Aabb3::EMPTY, meshes: vec![] } };
    let camera = crate::Camera {
        target: Vec3::ZERO,
        distance: 3.0,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        fov: if perspective { 0.3 } else { 0.0 },
        ..base.request.camera
    };
    let options = RaytraceOptions {
        volume,
        path: Some(job::CpuPathOptions { max_depth: depth }),
        budget: job::CpuBudget::Studio,
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 9 }),
        lights: Some(vec![]),
        shadows: true,
        environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..Default::default()
    };
    let request = ImageRequest {
        camera,
        integrator: job::Integrator::Raytrace,
        width: 1,
        height: 1,
        raytrace: Some(options.clone()),
        transparent_background: true,
        ..base.request
    };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
fn volume_slab() -> job::volume::HomogeneousVolume {
    job::volume::HomogeneousVolume {
        min_mm: [-10.0, -10.0, 0.0],
        max_mm: [10.0, 10.0, 2.0],
        extinction_per_mm: 0.5,
        scattering_albedo: [0.8, 0.6, 0.4],
        anisotropy: 0.0,
    }
}
fn volume_mean(mut task: ImageTask) -> ([f64; 3], u32, u32, u64) {
    let target = task.options.progressive.unwrap().samples_per_pixel;
    for _ in 0..2_000_000 {
        if task.progressive.as_ref().is_some_and(|p| p.film.samples() == target) {
            let (rgb, covered, samples) = task.progressive.as_ref().unwrap().film.pixel(0).unwrap();
            return (rgb, covered, samples, task.work.used());
        }
        let bounded = task.bvh.is_some()
            && task.progressive.as_ref().is_some_and(|p| p.film.ready() && !p.resolving)
            && task.output.is_empty()
            && task.encoder.is_none();
        let before = task.work.used();
        task.step_with_previews(1).unwrap();
        if bounded {
            assert!(task.work.used() - before <= 1, "every real Atmosphere continuation has one prepaid debit");
        }
    }
    panic!("bounded Atmosphere fixture did not complete");
}
fn independent_volume_segment(v: job::volume::HomogeneousVolume, p: Vec3, d: Vec3, limit: f64) -> [f64; 2] {
    let mut a = 0.0_f64;
    let mut b = limit;
    for (axis, (p, d)) in [(p.x, d.x), (p.y, d.y), (p.z, d.z)].into_iter().enumerate() {
        if d == 0.0 {
            if p < v.min_mm[axis] || p > v.max_mm[axis] {
                return [0.0; 2];
            }
        } else {
            let x = (v.min_mm[axis] - p) / d;
            let y = (v.max_mm[axis] - p) / d;
            a = a.max(x.min(y));
            b = b.min(x.max(y));
        }
    }
    if a < b { [a, b] } else { [0.0; 2] }
}
// The real fixture is the finite square [-10,10]^2 at z0, not an
// infinite half-space. Grazing source/continuation rays may miss its edge.
fn independent_volume_blocker(point: Vec3, direction: Vec3) -> Option<f64> {
    if direction.z >= 0.0 || point.z <= 0.0 {
        return None;
    }
    let distance = -point.z / direction.z;
    let plane = point + direction * distance;
    (plane.x.abs() <= 10.0 && plane.y.abs() <= 10.0).then_some(distance)
}
fn independent_volume_walk(samples: u32, depth: u8, v: job::volume::HomogeneousVolume, blocker: bool) -> ([f64; 3], u32) {
    independent_volume_environment(samples, depth, v, blocker, 1, &|_| Vec3::new(1.0, 1.0, 1.0))
}
fn independent_volume_environment(
    samples: u32,
    depth: u8,
    v: job::volume::HomogeneousVolume,
    blocker: bool,
    count: u32,
    environment: &impl Fn(Vec3) -> Vec3,
) -> ([f64; 3], u32) {
    independent_volume_celestial(samples, depth, v, blocker, count, environment, false)
}
fn independent_volume_celestial(
    samples: u32,
    depth: u8,
    v: job::volume::HomogeneousVolume,
    blocker: bool,
    count: u32,
    environment: &impl Fn(Vec3) -> Vec3,
    sun: bool,
) -> ([f64; 3], u32) {
    use std::f64::consts::{PI, TAU};
    let phase = |incoming: Vec3, d: Vec3| {
        let c = incoming.dot(d).clamp(-1.0, 1.0);
        (1.0 - v.anisotropy * v.anisotropy) / (4.0 * PI * (1.0 + v.anisotropy * v.anisotropy - 2.0 * v.anisotropy * c).powf(1.5))
    };
    let power = |a: f64, b: f64| a * a / (a * a + b * b);
    let mut sum = Vec3::ZERO;
    let mut covered = 0;
    let camera = cpu_volume_fixture(1, depth, false, blocker, Some(v)).request.camera;
    for ordinal in 0..samples {
        let [dx, dy] = progressive::variates(9, 0, ordinal, 0);
        let (point, d) = camera.ray(dx, dy, 1.0, 1.0);
        // Its exact orthographic line is unchanged; this slab is before the empty
        // scene near plane, so optical transport starts at z2, not scene bounds.
        let mut point = point + d * ((2.0 - point.z) / d.z);
        let mut direction = d;
        let mut weight = Vec3::new(1.0, 1.0, 1.0);
        let mut previous_pdf = 0.0;
        let mut foreground = blocker;
        for vertex in 0..depth {
            let endpoint = if blocker { independent_volume_blocker(point, direction).unwrap_or(1e12) } else { 1e12 };
            let [entry, exit] = independent_volume_segment(v, point, direction, endpoint);
            let domain = 0x5041_5448_0000 + u64::from(vertex) * 0x10000;
            let [u, _] = progressive::variates(9, 0, ordinal, domain + 0x6000);
            let u = if u == 0.0 { 2.0_f64.powi(-54) } else { u };
            let travel = -(1.0 - u).ln() / v.extinction_per_mm;
            let survival = (-v.extinction_per_mm * (exit - entry)).exp();
            if entry >= exit || travel >= exit - entry {
                if vertex > 0 && endpoint == 1e12 {
                    let li = environment(direction);
                    sum +=
                        Vec3::new(weight.x * li.x, weight.y * li.y, weight.z * li.z) * power(previous_pdf * survival, f64::from(count) / (4.0 * PI));
                    if sun && direction.z >= 0.7_f64.cos() {
                        let omega = 4.0 * PI * (0.7_f64 * 0.5).sin().powi(2);
                        sum += Vec3::new(weight.x * 2.0, weight.y * 3.0, weight.z * 5.0) * power(previous_pdf * survival, 1.0 / omega);
                    }
                }
                break;
            }
            foreground = true;
            point += direction * (entry + travel);
            weight = Vec3::new(weight.x * v.scattering_albedo[0], weight.y * v.scattering_albedo[1], weight.z * v.scattering_albedo[2]);
            if weight == Vec3::ZERO {
                break;
            }
            let side = match count {
                1 => 1,
                4 => 2,
                16 => 4,
                _ => 8,
            };
            for source in 0..count {
                let [u, t] = progressive::variates(9, 0, ordinal, domain + 0x2000 + u64::from(source));
                let u = ((f64::from(source / side) + u) / f64::from(side)).max(0.0_f64.next_up()).min(1.0_f64.next_down());
                let t = ((f64::from(source % side) + t) / f64::from(side)).max(0.0_f64.next_up()).min(1.0_f64.next_down());
                let z = 1.0 - 2.0 * u;
                let r = ((1.0 - z) * (1.0 + z)).sqrt();
                let light = Vec3::new(r * (TAU * t).cos(), r * (TAU * t).sin(), z);
                let phase_pdf = phase(direction, light);
                let [a, b] = independent_volume_segment(v, point, light, 1e12);
                let tr = (-v.extinction_per_mm * (b - a)).exp();
                if !(blocker && independent_volume_blocker(point, light).is_some()) {
                    let mis = if vertex + 1 == depth { 1.0 } else { power(f64::from(count) / (4.0 * PI), phase_pdf * tr) };
                    let li = environment(light);
                    sum += Vec3::new(weight.x * li.x, weight.y * li.y, weight.z * li.z) * (phase_pdf * (4.0 * PI) * tr * mis / f64::from(count));
                }
            }
            if sun {
                let omega = 4.0 * PI * (0.7_f64 * 0.5).sin().powi(2);
                let [u, t] = progressive::variates(9, 0, ordinal, domain + 0x7000);
                let z = 1.0 - u * omega / (2.0 * PI);
                let radius = ((1.0 - z) * (1.0 + z)).sqrt();
                let light = Vec3::new(radius * (TAU * t).cos(), radius * (TAU * t).sin(), z);
                let density = phase(direction, light);
                let [a, b] = independent_volume_segment(v, point, light, 1e12);
                let tr = (-v.extinction_per_mm * (b - a)).exp();
                let mis = if vertex + 1 == depth { 1.0 } else { power(1.0 / omega, density * tr) };
                sum += Vec3::new(weight.x * 2.0, weight.y * 3.0, weight.z * 5.0) * (density * omega * tr * mis);
            }
            if vertex + 1 == depth {
                break;
            }
            let [u, t] = progressive::variates(9, 0, ordinal, domain + 0x6001);
            let g = v.anisotropy;
            let c = if g == 0.0 {
                2.0 * u - 1.0
            } else {
                let q = (1.0 - g * g) / (1.0 - g + 2.0 * g * u);
                (1.0 + g * g - q * q) / (2.0 * g)
            };
            let r = (1.0 - c * c).max(0.0).sqrt();
            let axis = if direction.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
            let tangent = axis.cross(direction);
            let tangent = tangent / tangent.len();
            let next = tangent * (r * (TAU * t).cos()) + direction.cross(tangent) * (r * (TAU * t).sin()) + direction * c;
            let next = next / next.len();
            previous_pdf = phase(direction, next);
            direction = next;
        }
        covered += u32::from(foreground);
    }
    sum = sum / f64::from(samples);
    ([sum.x, sum.y, sum.z], covered)
}
#[test]
fn cpu_volume_none_and_authored_zero_keep_original_pixels_rng_work_and_heap_owners() {
    for path in [None, Some(job::CpuPathOptions { max_depth: 4 })] {
        let build = |zero: bool| {
            let base =
                cpu_volume_fixture(7, 4, false, true, zero.then(|| job::volume::HomogeneousVolume { extinction_per_mm: 0.0, ..volume_slab() }));
            let options = RaytraceOptions { path, ..base.options.clone() };
            let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
            ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
        };
        if path.is_some() {
            assert_eq!(volume_mean(build(false)), volume_mean(build(true)));
        }
        assert_eq!(
            complete_presentation(build(false), 1),
            complete_presentation(build(true), 1),
            "legacy and Path prefixes, RAW and every cumulative work unit stay identical"
        );
        assert_eq!(serde_json::to_value(build(false).admission).unwrap(), serde_json::to_value(build(true).admission).unwrap());
        assert_eq!(build(false).options.retained_bytes().unwrap(), build(true).options.retained_bytes().unwrap());
        assert_eq!(serde_json::to_value(&build(true).options).unwrap()["volume"]["extinction_per_mm"], 0.0);
    }
}

#[test]
fn cpu_volume_single_and_multiple_scatter_match_independent_hg_slab_nee_escape_and_blocker_oracles() {
    let grazing = Vec3::new(0.7252901844548614, -0.6841978696212823, -0.0763375630940093);
    let source = Vec3::new(-0.7543915972220849, -1.1230912621927815, 1.4665695089641722);
    assert!(independent_volume_blocker(source, grazing).is_none(), "fixed-seed ordinal5 crosses the z0 plane beyond the finite square");
    assert_eq!(independent_volume_blocker(Vec3::new(0.0, 0.0, 1.0), -Vec3::Z), Some(1.0));
    for g in [-0.6, 0.0, 0.6] {
        for depth in [1, 2, 16] {
            for blocker in [false, true] {
                let v = job::volume::HomogeneousVolume { anisotropy: g, ..volume_slab() };
                let expected = independent_volume_walk(64, depth, v, blocker);
                let (actual, covered, samples, _) = volume_mean(cpu_volume_fixture(64, depth, false, blocker, Some(v)));
                assert_eq!((covered, samples), (expected.1, 64));
                for (a, e) in actual.into_iter().zip(expected.0) {
                    assert!((a - e).abs() < 1e-12, "g{g},depth{depth},blocker{blocker}: independent HG/slab/MIS {a} vs{e}");
                }
            }
        }
    }
}
#[test]
fn cpu_volume_orthographic_union_and_physical_eye_pupil_absorption_keep_model_and_camera_unchanged() {
    for perspective in [false, true] {
        for lens in [None, Some(job::ThinLens { aperture_radius_mm: 0.1, focus_distance_mm: 3.0 })] {
            if !perspective && lens.is_some() {
                continue;
            }
            let original = cpu_volume_fixture(128, 1, perspective, true, None);
            let initial = primary_ray(&original.request, 0.5, 0.5, original.clip).unwrap();
            let max_z = initial.origin().z.max(original.request.camera.eye().z) + 2.0;
            let v = job::volume::HomogeneousVolume {
                min_mm: [-10.0, -10.0, -2.0],
                max_mm: [10.0, 10.0, max_z],
                extinction_per_mm: 1.0 / max_z,
                scattering_albedo: [0.0; 3],
                ..volume_slab()
            };
            let mut options = original.options.clone();
            options.volume = Some(v);
            options.lens = lens;
            options.environment = None;
            options.emissions = Some(vec![job::SurfaceEmission {
                body_index: 0,
                body: "Volume receiver".into(),
                color: [1.0, 0.5, 0.25],
                strength: 2.0,
                sides: job::EmissionSides::Both,
            }]);
            let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
            let make = || ImageTask::new(original.scene.clone(), request.clone(), CancelToken::default(), options.clone()).unwrap();
            let mut survivors = 0;
            for ordinal in 0..128 {
                let [x, y] = progressive::variates(9, 0, ordinal, 0);
                let camera = request.camera;
                let (right, up, back) = camera.basis();
                let (mut source, mut d) = camera.ray(x, y, 1.0, 1.0);
                if perspective {
                    source = camera.eye();
                    if let Some(lens) = lens {
                        let [u, t] = progressive::variates(9, 0, ordinal, 0x4c454e53);
                        let offset = (right * (std::f64::consts::TAU * t).cos() + up * (std::f64::consts::TAU * t).sin())
                            * (lens.aperture_radius_mm * u.sqrt());
                        source += offset;
                        let aimed = d * (lens.focus_distance_mm / d.dot(-back)) - offset;
                        d = aimed / aimed.len();
                    }
                } else {
                    source += d * ((max_z - source.z) / d.z);
                }
                let length = -source.z / d.z;
                let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_6000);
                survivors += u32::from(u >= -(-v.extinction_per_mm * length).exp_m1());
            }
            assert!(survivors > 0 && survivors < 128);
            let actual = volume_mean(make());
            let p = f64::from(survivors) / 128.0;
            assert_eq!(actual.0, [2.0 * p, p, 0.5 * p]);
            assert_eq!((actual.1, actual.2), (128, 128));
            assert_eq!(make().request.camera, original.request.camera);
            assert_eq!(make().scene.bounds, original.scene.bounds);
            assert!(
                Arc::ptr_eq(&make().scene.meshes[0].mesh, &original.scene.meshes[0].mesh),
                "constructing Atmosphere must retain the exact captured immutable geometry"
            );
            let mut probe = make();
            while probe.path.first().is_none_or(|p| p.stage_name() != "volume") {
                probe.step_with_previews(1).unwrap();
            }
            assert!(
                Arc::ptr_eq(&probe.scene.meshes[0].mesh, &original.scene.meshes[0].mesh),
                "actual fog preparation and traversal must keep the same captured geometry"
            );
            let p = probe.path[0].volume_probe().unwrap();
            assert_eq!(p.source, p.ray.unwrap().origin());
            if !perspective {
                assert!(p.source.z > initial.origin().z, "foreground fog beyond the original model near plane must be included");
            } else if lens.is_none() {
                assert_eq!(p.source, request.camera.eye());
                assert!(p.ray.unwrap().interval()[0] > 0.0, "the model retains its near interval while optical length starts at the eye");
            }
        }
    }
}
#[test]
fn cpu_volume_empty_scene_alpha_prefixes_raw_quanta_finishing_and_adaptive_use_the_same_film() {
    let build = |samples, adaptive, pilot, finish| {
        let base = cpu_volume_fixture(samples, 4, false, false, Some(volume_slab()));
        let mut options = base.options.clone();
        if adaptive {
            options.adaptive = Some(Box::new(job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }));
        }
        if pilot {
            options.progressive.as_mut().unwrap().seed ^= 0xa511_e9b3;
        }
        if finish {
            options.output.as_mut().unwrap().finishing.saturation = 0.2;
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    let output = complete_presentation(build(7, false, false, false), 1);
    assert_eq!(output, complete_presentation(build(7, false, false, false), 16384));
    for (n, png) in &output.0 {
        assert_eq!(*png, complete_presentation(build(*n, false, false, false), 3).1);
    }
    let expected = independent_volume_walk(7, 4, volume_slab(), false);
    let alpha = f64::from(expected.1) / 7.0;
    assert_eq!(
        native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0,
        [expected.0[0] as f32, expected.0[1] as f32, expected.0[2] as f32, alpha as f32]
    );
    assert_eq!(alpha_pixel(&output.1)[3], (alpha * 255.0).round() as u8);
    let finished = complete_presentation(build(7, false, false, true), 1);
    assert_ne!(finished.1, output.1);
    assert_eq!(finished.2, output.2);
    let reference = complete_presentation(build(16, false, false, false), 1);
    let adaptive = complete_presentation(build(16, true, false, false), 1);
    assert_eq!(adaptive, complete_presentation(build(16, true, false, false), 16384));
    assert_eq!(adaptive.0.len(), 31);
    assert_eq!(adaptive.1, reference.1);
    assert_eq!(adaptive.2, reference.2);
    assert_ne!(reference.2, complete_presentation(build(16, false, true, false), 1).2, "actual pilot seed must expose leakage");
}
#[test]
fn cpu_volume_decision_and_visibility_yield_and_cancel_before_film_publication() {
    for phase in ["volume", "volume_shadow"] {
        for cancel in [false, true] {
            let mut task = cpu_volume_fixture(4, 4, false, false, Some(job::volume::HomogeneousVolume { extinction_per_mm: 100.0, ..volume_slab() }));
            for _ in 0..100_000 {
                if task.path.first().is_some_and(|s| s.stage_name() == phase) {
                    break;
                }
                task.step_with_previews(1).unwrap();
            }
            assert_eq!(task.path[0].stage_name(), phase);
            let before = task.work.used();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = before;
            }
            assert_eq!(
                task.step_with_previews(1).err().unwrap().code,
                if cancel { job::ErrorCode::Cancelled } else { job::ErrorCode::LimitExceeded }
            );
            assert_eq!(task.work.used(), before);
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            assert!(task.take_linear_exr().is_none());
        }
    }
}

#[test]
fn cpu_volume_opaque_connections_keep_exact_physical_sources_and_survival_in_the_deferred_mis_density() {
    let base = cpu_volume_fixture(128, 2, false, true, Some(job::volume::HomogeneousVolume { scattering_albedo: [0.0; 3], ..volume_slab() }));
    let mut scene = base.scene.clone();
    scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 1.0, 1.0).unwrap());
    let mut task = ImageTask::new(scene, base.request.clone(), CancelToken::default(), base.options.clone()).unwrap();
    let mut ranges = 0;
    let mut connections = 0;
    for _ in 0..2_000_000 {
        if task.progressive.as_ref().unwrap().film.samples() == 128 {
            break;
        }
        if task.path.first().is_some_and(|s| s.stage_name() == "volume_surface") {
            let before = task.work.used();
            task.step_with_previews(1).unwrap();
            assert_eq!(task.work.used() - before, 1);
            ranges += 1;
            continue;
        }
        if task.path.first().is_some_and(|s| s.stage_name() == "volume_shadow") {
            let before = task.path[0].volume_probe().unwrap();
            if before.incoming.is_none() {
                assert_eq!(before.depth, 0);
                assert_eq!(before.throughput, Vec3::new(1.0, 1.0, 1.0));
                let ray = before.shadow.unwrap();
                assert_eq!(ray.origin(), before.source, "no normal offset may shorten the optical connection");
                assert_eq!(before.source.z, 0.0);
                let [a, b] = independent_volume_segment(volume_slab(), before.source, ray.direction(), ray.interval()[1]);
                let survival = (-0.5 * (b - a)).exp();
                let used = task.work.used();
                task.step_with_previews(1).unwrap();
                assert_eq!(task.work.used() - used, 1);
                let attenuated = task.path[0].volume_probe().unwrap();
                assert_eq!(attenuated.survival, survival);
                assert_eq!(attenuated.contribution, before.contribution * survival);
                while task.path[0].stage_name() != "add" {
                    task.step_with_previews(1).unwrap();
                }
                let [light, bsdf] = task.path[0].volume_probe().unwrap().mis.unwrap();
                let weight = light * light / (light * light + (bsdf * survival).powi(2));
                task.step_with_previews(1).unwrap();
                let after = task.path[0].volume_probe().unwrap();
                assert!((after.radiance - (before.radiance + attenuated.contribution * weight)).len() < 1e-14);
                connections += 1;
                continue;
            }
        }
        task.step_with_previews(1).unwrap();
    }
    assert!(ranges > 0 && connections > 0, "actual opaque paths must exercise the range walk and nonzero physical connection");
}
#[test]
fn cpu_volume_integrates_real_perspective_near_prefix_before_the_unchanged_model_interval() {
    let base = cpu_volume_fixture(64, 1, true, true, None);
    let eye = base.request.camera.eye();
    let near = base.clip.0;
    assert!(near > 0.0 && near < 0.01);
    let v = job::volume::HomogeneousVolume {
        min_mm: [-10.0, -10.0, eye.z - near * 0.75],
        max_mm: [10.0, 10.0, eye.z],
        extinction_per_mm: 1000.0,
        scattering_albedo: [0.0; 3],
        ..volume_slab()
    };
    let mut options = base.options.clone();
    options.volume = Some(v);
    options.environment = None;
    options.emissions = Some(vec![job::SurfaceEmission {
        body_index: 0,
        body: "Volume receiver".into(),
        color: [1.0; 3],
        strength: 1.0,
        sides: job::EmissionSides::Both,
    }]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    let mut expected = 0.0;
    let mut collisions = 0;
    for ordinal in 0..64 {
        let [x, y] = progressive::variates(9, 0, ordinal, 0);
        let d = request.camera.ray(x, y, 1.0, 1.0).1;
        let length = (eye.z - v.min_mm[2]) / (-d.z);
        let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_6000);
        if u < -(-v.extinction_per_mm * length).exp_m1() {
            collisions += 1;
        } else {
            expected += 1.0 / 64.0;
        }
    }
    assert!(collisions > 0 && collisions < 64);
    let task = ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap();
    let mean = volume_mean(task);
    assert_eq!(mean.0, [expected; 3]);
    assert_eq!((mean.1, mean.2), (64, 64), "collisions before the old near plane remain visible foreground");
}

#[test]
fn cpu_volume_finite_rectangle_connections_match_independent_phase_survival_and_source_count_mis() {
    use std::f64::consts::PI;
    for samples in [1, 4, 16, 64] {
        let base = cpu_volume_fixture(
            4,
            4,
            false,
            false,
            Some(job::volume::HomogeneousVolume { extinction_per_mm: 100.0, anisotropy: 0.3, ..volume_slab() }),
        );
        let mut options = base.options.clone();
        options.environment = None;
        options.area_samples = Some(samples);
        options.area_lights = Some(vec![job::RectangleLightOptions {
            center_mm: [0.0, 0.0, 4.0],
            edge_u_mm: [1.0, 0.0, 0.0],
            edge_v_mm: [0.0, -1.0, 0.0],
            color: [1.0, 0.5, 0.25],
            intensity: 2.0,
        }]);
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let mut task = ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap();
        let mut observed = 0;
        for _ in 0..100_000 {
            if task.path.first().is_some_and(|s| s.stage_name() == "volume_shadow") {
                let before = task.path[0].volume_probe().unwrap();
                if let Some(incoming) = before.incoming {
                    let ray = before.shadow.unwrap();
                    assert_eq!(ray.origin(), before.source);
                    let length = ray.interval()[1];
                    let light = f64::from(samples) * length * length / ray.direction().z;
                    let c = incoming.dot(ray.direction());
                    let phase = (1.0 - 0.3_f64.powi(2)) / (4.0 * PI * (1.0 + 0.3_f64.powi(2) - 0.6 * c).powf(1.5));
                    let [actual_light, actual_phase] = before.mis.unwrap();
                    assert!((actual_light - light).abs() < light * 1e-12);
                    assert!((actual_phase - phase).abs() < 1e-14);
                    let v = job::volume::HomogeneousVolume { extinction_per_mm: 100.0, ..volume_slab() };
                    let [a, b] = independent_volume_segment(v, before.source, ray.direction(), length);
                    let survival = (-100.0 * (b - a)).exp();
                    task.step_with_previews(1).unwrap();
                    let shadow = task.path[0].volume_probe().unwrap();
                    assert_eq!(shadow.survival, survival);
                    assert_eq!(shadow.contribution, before.contribution * survival);
                    while task.path[0].stage_name() != "add" {
                        task.step_with_previews(1).unwrap();
                    }
                    let competing = phase * survival;
                    let weight = light * light / (light * light + competing * competing);
                    task.step_with_previews(1).unwrap();
                    let after = task.path[0].volume_probe().unwrap();
                    assert!((after.radiance - (before.radiance + shadow.contribution * weight)).len() < 1e-13);
                    observed += 1;
                    if observed == samples {
                        break;
                    }
                }
            }
            task.step_with_previews(1).unwrap();
        }
        assert_eq!(observed, samples, "every source sample must traverse the real bounded physical visibility path");
    }
}
#[test]
fn cpu_volume_white_furnace_energy_matches_the_independent_finite_walk_and_analytic_primary_coverage() {
    let volume = job::volume::HomogeneousVolume { scattering_albedo: [1.0; 3], ..volume_slab() };
    let expected = independent_volume_walk(128, 16, volume, false);
    let actual = volume_mean(cpu_volume_fixture(128, 16, false, false, Some(volume)));
    for (a, e) in actual.0.into_iter().zip(expected.0) {
        assert!((a - e).abs() < 1e-12, "unit-albedo white furnace {a} vs independent {e}");
    }
    let collisions = (0..128).filter(|n| progressive::variates(9, 0, *n, 0x5041_5448_6000)[0] < 1.0 - (-1.0_f64).exp()).count();
    assert_eq!(actual.1, collisions as u32);
    assert!(actual.0.iter().all(|a| *a > 0.0));
    assert_eq!(actual.0[0], actual.0[1]);
    assert_eq!(actual.0[1], actual.0[2]);
}

#[test]
fn cpu_volume_nonconstant_hdr_nee_and_escape_match_independent_texels_and_source_count_densities() {
    use std::f64::consts::{PI, TAU};
    let texels: [[u8; 4]; 8] = [
        [128, 32, 16, 129],
        [16, 128, 32, 129],
        [32, 16, 128, 129],
        [64, 96, 128, 129],
        [96, 64, 32, 129],
        [32, 96, 64, 129],
        [64, 32, 96, 129],
        [128, 64, 96, 129],
    ];
    let environment = |d: Vec3| {
        let x = ((d.y.atan2(d.x).rem_euclid(TAU) * 4.0 / TAU) as usize).min(3);
        let y = ((d.z.clamp(-1.0, 1.0).acos() * 2.0 / PI) as usize).min(1);
        let p = texels[y * 4 + x];
        Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])) / 128.0
    };
    let build = |samples, depth, count| {
        let base = cpu_volume_fixture(samples, depth, false, false, Some(volume_slab()));
        let options = RaytraceOptions { environment: None, ..base.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for texel in texels {
            bytes.extend_from_slice(&texel);
        }
        let input =
            crate::lighting::environment_input::EnvironmentInput::new(crate::lighting::HdrSource::new(bytes).unwrap(), 0.0, 1.0, count).unwrap();
        ImageTask::new_with_environment(base.scene.clone(), request, CancelToken::default(), options, Some(input)).unwrap()
    };
    for depth in [1, 2] {
        for count in [1, 4, 16, 64] {
            let expected = independent_volume_environment(32, depth, volume_slab(), false, count, &environment);
            let actual = volume_mean(build(32, depth, count));
            assert_eq!(actual.1, expected.1);
            for (a, e) in actual.0.into_iter().zip(expected.0) {
                assert!((a - e).abs() < 1e-12, "real HDR depth{depth},N{count}: {a} vs independent {e}");
            }
        }
    }
    let output = complete_presentation(build(7, 2, 16), 1);
    assert_eq!(output, complete_presentation(build(7, 2, 16), 16384));
    for (n, png) in &output.0 {
        assert_eq!(*png, complete_presentation(build(*n, 2, 16), 3).1);
    }
    let (expected, covered) = independent_volume_environment(7, 2, volume_slab(), false, 16, &environment);
    assert_eq!(
        native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0,
        [expected[0] as f32, expected[1] as f32, expected[2] as f32, (f64::from(covered) / 7.0) as f32]
    );
}

fn celestial_fixture(samples: u32, depth: u8, sun: bool, sky: bool) -> ImageTask {
    let original = path_fixture(samples, depth, false);
    let options = RaytraceOptions {
        environment: None,
        sun: sun.then(|| Box::new(job::Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.7, radiance: [2.0, 3.0, 5.0] })),
        sky: sky.then(|| {
            Box::new(job::CpuSky {
                gradient: crate::lighting::environment::SkyGradient {
                    zenith: [0.25, 0.5, 1.0],
                    horizon: [0.75, 0.5, 0.25],
                    ground: [0.125, 0.25, 0.5],
                },
                rotation_rad: 0.731,
                intensity: 1.75,
            })
        }),
        ..original.options
    };
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
    ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
}
fn independent_sky(direction: Vec3) -> Vec3 {
    use std::f64::consts::PI;
    let y = ((direction.z.clamp(-1.0, 1.0).acos() / PI * 256.0) as u32).min(255);
    let z = (PI * (f64::from(y) + 0.5) / 256.0).cos();
    let t = z.abs().sqrt() as f32;
    let horizon = [0.75_f32, 0.5, 0.25];
    let endpoint = if z >= 0.0 { [0.25_f32, 0.5, 1.0] } else { [0.125_f32, 0.25, 0.5] };
    let rgb: [f64; 3] = std::array::from_fn(|i| f64::from(horizon[i] * (1.0 - t) + endpoint[i] * t) * 1.75);
    Vec3::new(rgb[0], rgb[1], rgb[2])
}
fn independent_celestial_mean(samples: u32, depth: u8, sun: bool, sky: bool) -> [f64; 3] {
    use std::f64::consts::{PI, TAU};
    let mut result = Vec3::ZERO;
    let angle = 4.0 * PI * (0.7_f64 * 0.5).sin().powi(2);
    let sun_radiance = Vec3::new(2.0, 3.0, 5.0);
    let power = |a: f64, b: f64| a * a / (a * a + b * b);
    for ordinal in 0..samples {
        if sky {
            let [u, v] = progressive::variates(9, 0, ordinal, 0x5041_5448_2000);
            let z = 1.0 - 2.0 * u;
            if z > 0.0 {
                let r = (1.0 - z * z).sqrt();
                let d = Vec3::new(r * (TAU * v).cos(), r * (TAU * v).sin(), z);
                let bsdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
                result += independent_sky(d) * (2.0 * z * if depth == 1 { 1.0 } else { power(1.0 / (4.0 * PI), bsdf) });
            }
        }
        if sun {
            let [u, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_7000);
            let z = 1.0 - u * angle / (2.0 * PI);
            let bsdf = 0.5 * z / PI + 0.5 / (4.0 * PI);
            result += sun_radiance * (0.5 * z / PI * angle * if depth == 1 { 1.0 } else { power(1.0 / angle, bsdf) });
        }
        if depth == 2 {
            let (z, pdf) = independent_lambert_scatter(ordinal);
            if pdf > 0.0 {
                let [v, _] = progressive::variates(9, 0, ordinal, 0x5041_5448_3001);
                let r = (1.0 - z * z).sqrt();
                let d = Vec3::new(r * (TAU * v).cos(), r * (TAU * v).sin(), z);
                if sky {
                    result += independent_sky(d) * (0.5 * z / (PI * pdf) * power(pdf, 1.0 / (4.0 * PI)));
                }
                if sun && z >= 0.7_f64.cos() {
                    result += sun_radiance * (0.5 * z / (PI * pdf) * power(pdf, 1.0 / angle));
                }
            }
        }
    }
    result = result / f64::from(samples);
    [result.x, result.y, result.z]
}
#[test]
fn cpu_sun_sky_real_path_matches_independent_per_ordinal_nee_and_escape_mis() {
    for samples in [1, 7, 128] {
        for depth in [1, 2] {
            for (sun, sky) in [(true, false), (false, true), (true, true)] {
                let expected = independent_celestial_mean(samples, depth, sun, sky);
                let (actual, _) = path_mean(celestial_fixture(samples, depth, sun, sky));
                for (actual, expected) in actual.into_iter().zip(expected) {
                    assert!((actual - expected).abs() < 1e-11, "N={samples}, depth={depth}, Sun={sun}, Sky={sky}: {actual} != {expected}");
                }
            }
        }
    }
}
#[test]
fn cpu_sun_cap_and_sky_rows_keep_shared_domain_and_independent_energy() {
    use std::f64::consts::{PI, TAU};
    let sun = job::Sun { direction: [0.0, 0.0, 2.0], angular_radius_rad: 0.7, radiance: [2.0, 3.0, 5.0] };
    let omega = 2.0 * PI * (1.0 - 0.7_f64.cos());
    assert!((sun.solid_angle() - omega).abs() < 1e-15);
    let mut cosine = 0.0;
    for i in 0..4096 {
        let u = [(f64::from(i) + 0.5) / 4096.0, (f64::from((i * 2371) % 4096) + 0.5) / 4096.0];
        let sample = super::super::celestial::sample(sun, u).unwrap();
        assert!((sample.direction.len() - 1.0).abs() < 1e-15);
        assert!(sample.direction.z > 0.7_f64.cos());
        assert!((sample.pdf - 1.0 / omega).abs() < 1e-14);
        cosine += sample.direction.z;
    }
    assert!((cosine / 4096.0 - (1.0 + 0.7_f64.cos()) / 2.0).abs() < 1e-14);
    let task = celestial_fixture(1, 1, false, true);
    let sky = *task.options.sky.as_deref().unwrap();
    let mut energy = Vec3::ZERO;
    for row in 0..256 {
        let z = (PI * (f64::from(row) + 0.5) / 256.0).cos();
        let radius = (1.0 - z * z).sqrt();
        let d = Vec3::new(radius, 0.0, z);
        let expected = independent_sky(d);
        assert_eq!(sky.radiance(d).unwrap(), expected);
        assert_eq!(
            job::CpuSky { rotation_rad: sky.rotation_rad + 7.0, ..sky }
                .radiance(Vec3::new(radius * (TAU / 3.0).cos(), radius * (TAU / 3.0).sin(), z))
                .unwrap(),
            expected
        );
        if row < 128 {
            let top = (PI * f64::from(row) / 256.0).cos();
            let bottom = (PI * f64::from(row + 1) / 256.0).cos();
            energy += expected * (0.5 * (top * top - bottom * bottom));
        }
    }
    // Analytic continuous gradient: albedo0.5 times cosine-weighted hemisphere.
    let analytic = Vec3::new(0.75 * 0.5 + (0.25 - 0.75) * 0.4, 0.5 * 0.5, 0.25 * 0.5 + (1.0 - 0.25) * 0.4) * 1.75;
    assert!((energy - analytic).len() < 0.0001, "finite GPU row policy converges to independent gradient energy");
    assert!(super::super::celestial::sample(sun, [1.0, 0.5]).is_err());
    assert_eq!(super::super::celestial::pdf(sun, -Vec3::Z).unwrap(), 0.0);
}
#[test]
fn cpu_celestial_prefixes_raw_ownership_quanta_and_memory_are_exact() {
    let make = |samples, denoise| {
        let original = celestial_fixture(samples, 2, true, true);
        let options =
            RaytraceOptions { denoise, output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })), ..original.options };
        let request = ImageRequest { transparent_background: true, raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    let a = complete_presentation(make(7, true), 1);
    let b = complete_presentation(make(7, true), 16384);
    assert_eq!(a, b, "all immutable checkpoints/RAW/work are independent of pump quantum");
    assert_eq!(a.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    for (samples, png) in &a.0 {
        assert_eq!(*png, complete_presentation(make(*samples, true), 3).1);
    }
    let expected = independent_celestial_mean(7, 2, true, true).map(|v| v as f32);
    assert_eq!(native_exr_pixels(a.2.as_ref().unwrap()).get_pixel(0, 0).0, [expected[0], expected[1], expected[2], 1.0]);
    assert_eq!(a.2, complete_presentation(make(7, false), 1).2, "denoise never alters raw celestial radiance");
    let original = celestial_fixture(3, 1, false, false);
    let base = admit(&original.scene, &original.request, &original.options).unwrap();
    let mut options = original.options.clone();
    options.sun = Some(Box::new(job::Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.7, radiance: [2.0, 3.0, 5.0] }));
    options.sky = Some(Box::new(job::CpuSky::default()));
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    let extra = admit(&original.scene, &request, &options).unwrap();
    let boxes = (std::mem::size_of::<job::Sun>() + std::mem::size_of::<job::CpuSky>()) as u64;
    assert_eq!(extra.preparation_bytes - base.preparation_bytes, 2 * boxes, "producer and captured request own separate, prepaid boxes");
    assert_eq!(options.retained_bytes().unwrap() - original.options.retained_bytes().unwrap(), boxes);
    assert_eq!(original.options.retained_celestial_bytes().unwrap(), 0);
    let wire = serde_json::to_value(RaytraceOptions::default()).unwrap();
    assert!(wire.get("sun").is_none() && wire.get("sky").is_none());
}
#[test]
fn cpu_sun_continuation_cancel_and_work_refuse_before_mutation() {
    for cancel in [false, true] {
        let mut task = celestial_fixture(3, 2, true, true);
        for _ in 0..100_000 {
            if task.path.first().is_some_and(|p| p.stage_name() == "sun") {
                break;
            }
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
        }
        assert_eq!(task.path.first().unwrap().stage_name(), "sun");
        let before = (task.work.used(), task.pixel, task.progressive.as_ref().unwrap().film.samples());
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = task.work.used();
        }
        assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!((task.work.used(), task.pixel, task.progressive.as_ref().unwrap().film.samples()), before);
        assert!(task.take_linear_exr().is_none());
    }
}

#[test]
fn cpu_celestial_lens_adaptive_and_nested_glass_keep_complete_radiance_and_raw() {
    let make = |adaptive: bool, lens: bool| {
        let original = celestial_fixture(16, 1, true, true);
        let options = RaytraceOptions {
            adaptive: adaptive.then(|| Box::new(job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() })),
            lens: lens.then_some(job::ThinLens { aperture_radius_mm: 0.01, focus_distance_mm: 1.0 }),
            denoise: true,
            output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
            ..original.options
        };
        let camera = crate::Camera { fov: 0.001, ..original.request.camera };
        let request = ImageRequest { camera, transparent_background: true, raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    let reference = complete_presentation(make(false, false), 1);
    let pupil = complete_presentation(make(false, true), 1);
    assert_eq!(
        (reference.1.clone(), reference.2.clone()),
        (pupil.1, pupil.2),
        "a focused, fully covered Lambert receiver has no aperture exposure factor"
    );
    let mut task = make(true, true);
    let mut previews = Vec::new();
    let (png, raw, report) = loop {
        match task.step_with_previews(1).unwrap() {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { samples_per_pixel, png } => {
                previews.push((samples_per_pixel, png));
                assert!(task.take_linear_exr().is_none());
            }
            ImagePoll::Complete(png) => break (png, task.take_linear_exr().unwrap(), task.sampling_report().unwrap()),
        }
    };
    assert_eq!(previews.iter().map(|p| p.0).collect::<Vec<_>>(), (1..32).collect::<Vec<_>>());
    assert_eq!(png, reference.1);
    assert_eq!(Some(raw), reference.2, "pilot Sun/Sky samples must never leak into production RAW");
    assert_eq!((report.pilot_samples_per_pixel, report.production_samples, report.primary_path_evaluations), (16, 16, 32));
    let original = cpu_general_glass_fixture(8, true, 1.0);
    let options = RaytraceOptions {
        environment: None,
        sun: Some(Box::new(job::Sun { direction: [0.0, 0.0, -1.0], angular_radius_rad: 0.7, radiance: [0.25, 0.5, 0.75] })),
        sky: Some(Box::new(job::CpuSky {
            gradient: crate::lighting::environment::SkyGradient { zenith: [1.0, 2.0, 3.0], horizon: [1.0, 2.0, 3.0], ground: [1.0, 2.0, 3.0] },
            rotation_rad: 1.75,
            intensity: 1.0,
        })),
        ..original.options
    };
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
    let actual = path_mean(ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()).0;
    let expected = [1.25 * (-0.4_f64).exp(), 2.5 * (-0.2_f64).exp(), 3.75 * (-0.4_f64).exp()];
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "nested delta escape sees both emitters after physical Beer: {a} vs {e}");
    }
}
#[test]
fn cpu_hdr_and_finite_sun_are_additive_without_sky_or_default_source_substitution() {
    use crate::lighting::{HdrSource, environment_input::EnvironmentInput};
    let make = |sun| {
        let original = celestial_fixture(32, 2, sun, false);
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            bytes.extend_from_slice(&[128, 64, 32, 129]);
        }
        let input = EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.731, 1.0, 4).unwrap();
        ImageTask::new_with_environment(original.scene, original.request, CancelToken::default(), original.options, Some(input)).unwrap()
    };
    let base = path_mean(make(false)).0;
    let combined = path_mean(make(true)).0;
    let sun = independent_celestial_mean(32, 2, true, false);
    for i in 0..3 {
        assert!((combined[i] - base[i] - sun[i]).abs() < 1e-11, "HDR source and Sun retain independent proposals/escape MIS");
    }
}

#[test]
fn cpu_celestial_maps_then_aging_feed_the_same_brdf_pdf_and_guides() {
    let make = |explicit, half| {
        let original = composed_map_aging_fixture(16, true, true, explicit, half);
        let source = celestial_fixture(16, 2, true, true);
        let options = RaytraceOptions { environment: None, sun: source.options.sun, sky: source.options.sky, ..original.options };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    for half in [false, true] {
        let actual = complete_presentation(make(false, half), 1);
        let expected = complete_presentation(make(true, half), 1);
        assert_eq!(
            (actual.0, actual.1, actual.2),
            (expected.0, expected.1, expected.2),
            "Sun/Sky use the composed mapped/aged material, shading normal and exact matte in every pass"
        );
    }
}

#[test]
fn cpu_sun_sampler_pdf_support_agrees_at_shared_radius_limits_and_oblique_axes() {
    for direction in [[0.0, 0.0, 1.0], [0.2, -0.3, 0.9], [1.0, 1.0, 1.0]] {
        for radius in [0.001, std::f64::consts::FRAC_PI_2] {
            let sun = job::Sun { direction, angular_radius_rad: radius, radiance: [1.0; 3] };
            for u in [0.0, 0.5, 1.0_f64.next_down()] {
                let sample = super::super::celestial::sample(sun, [u, 0.731]).unwrap();
                assert_eq!(super::super::celestial::pdf(sun, sample.direction).unwrap(), sample.pdf, "axis={direction:?},radius={radius},u={u}");
                assert_eq!(super::super::celestial::radiance(sun, sample.direction).unwrap(), Vec3::new(1.0, 1.0, 1.0));
            }
        }
    }
    let mut options = celestial_fixture(3, 2, true, false).options;
    options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }; 8]);
    assert!(options.validate().unwrap_err().message.contains("eight"));
    options.lights.as_mut().unwrap().pop();
    assert!(options.validate().is_ok());
    options.sun.as_mut().unwrap().angular_radius_rad = 0.0;
    assert!(options.validate().is_err(), "zero radius stays an explicit hard source, never a widened finite cap");
}

#[test]
fn cpu_uniform_sky_preserves_the_existing_constant_environment_pixels_and_work() {
    let make = |sky: bool| {
        let original = path_fixture(7, 2, false);
        let options = RaytraceOptions {
            environment: if sky { None } else { original.options.environment },
            sky: sky.then(|| {
                Box::new(job::CpuSky {
                    gradient: crate::lighting::environment::SkyGradient { zenith: [1.0; 3], horizon: [1.0; 3], ground: [1.0; 3] },
                    rotation_rad: 0.731,
                    intensity: 1.0,
                })
            }),
            output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
            ..original.options
        };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    assert_eq!(
        complete_presentation(make(false), 1),
        complete_presentation(make(true), 1),
        "uniformSky adds no hidden pass/RNG/work and uses the same existing constant-environment target"
    );
}

#[test]
fn cpu_finite_sun_real_opaque_visibility_keeps_the_camera_aperture_separate() {
    let make = |blocked| {
        let original = path_fixture(16, 1, blocked);
        let options = RaytraceOptions {
            environment: None,
            emissions: None,
            sun: Some(Box::new(job::Sun { direction: [1.0, 0.0, 1.0], angular_radius_rad: 0.1, radiance: [2.0, 3.0, 5.0] })),
            ..original.options
        };
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    assert!(path_mean(make(false)).0.into_iter().all(|v| v > 0.0));
    assert_eq!(path_mean(make(true)).0, [0.0; 3], "all cap directions meet the real black ceiling outside the primary aperture");
}

#[test]
fn composed_cpu_volume_sun_and_sky_use_phase_nee_physical_sources_and_complementary_escape_mis() {
    let sources = celestial_fixture(1, 1, true, true).options;
    let build = |samples, depth| {
        let base = cpu_volume_fixture(samples, depth, false, false, Some(job::volume::HomogeneousVolume { anisotropy: 0.3, ..volume_slab() }));
        let options = RaytraceOptions { environment: None, sun: sources.sun.clone(), sky: sources.sky.clone(), ..base.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    for depth in [1, 2, 16] {
        let expected = independent_volume_celestial(64, depth, build(1, depth).options.volume.unwrap(), false, 1, &independent_sky, true);
        let actual = volume_mean(build(64, depth));
        assert_eq!(actual.1, expected.1);
        for (a, e) in actual.0.into_iter().zip(expected.0) {
            assert!((a - e).abs() < 1e-12, "volume+Sun/Sky depth{depth}: {a} vs independent {e}");
        }
    }
    let output = complete_presentation(build(7, 4), 1);
    assert_eq!(output, complete_presentation(build(7, 4), 16384));
    for (n, png) in &output.0 {
        assert_eq!(*png, complete_presentation(build(*n, 4), 3).1);
    }
    let expected = independent_volume_celestial(7, 4, build(1, 4).options.volume.unwrap(), false, 1, &independent_sky, true);
    assert_eq!(
        native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0,
        [expected.0[0] as f32, expected.0[1] as f32, expected.0[2] as f32, (f64::from(expected.1) / 7.0) as f32]
    );
}
#[test]
fn composed_cpu_subsurface_sun_and_sky_keep_phase_only_delta_escape_and_physical_beer() {
    let sources = celestial_fixture(1, 1, true, true).options;
    let build = |samples| {
        let base = cpu_subsurface_fixture(samples, false, 0, 16, [1.0, 0.0], -0.6, None);
        let options = RaytraceOptions { environment: None, sun: sources.sun.clone(), sky: sources.sky.clone(), ..base.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap()
    };
    let environment = |d: Vec3| independent_sky(d) + if d.z >= 0.7_f64.cos() { Vec3::new(2.0, 3.0, 5.0) } else { Vec3::ZERO };
    let expected = independent_single_box_subsurface(&build(64), &environment);
    for (a, e) in path_mean(build(64)).0.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-12, "SSS+Sun/Sky escape/Beer: {a} vs independent {e}");
    }
    let output = complete_presentation(build(7), 1);
    assert_eq!(output, complete_presentation(build(7), 16384));
    for (n, png) in &output.0 {
        assert_eq!(*png, complete_presentation(build(*n), 3).1);
    }
}

#[test]
fn cpu_volume_empty_scene_integrates_boxes_before_the_original_ortho_origin_and_beyond_its_far_plane() {
    let base = cpu_volume_fixture(64, 1, false, false, None);
    let original = primary_ray(&base.request, 0.5, 0.5, base.clip).unwrap();
    let h = (0..64).filter(|n| progressive::variates(9, 0, *n, 0x5041_5448_6000)[0] < 1.0 - (-1.0_f64).exp()).count();
    assert!(h > 0 && h < 64);
    for [lo, hi] in [[-1000.0, -998.0], [original.origin().z + 10.0, original.origin().z + 12.0]] {
        if hi < 0.0 {
            assert!(-hi + original.origin().z > original.interval()[1], "the far fixture must lie beyond the old complete model interval");
        } else {
            assert!(lo > original.origin().z, "the near fixture must lie before the old optical origin");
        }
        let volume = job::volume::HomogeneousVolume {
            min_mm: [-10.0, -10.0, lo],
            max_mm: [10.0, 10.0, hi],
            extinction_per_mm: 0.5,
            scattering_albedo: [0.0; 3],
            anisotropy: 0.0,
        };
        let options = RaytraceOptions { volume: Some(volume), environment: None, ..base.options.clone() };
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let build = || ImageTask::new(base.scene.clone(), request.clone(), CancelToken::default(), options.clone()).unwrap();
        let actual = volume_mean(build());
        assert_eq!(actual.0, [0.0; 3]);
        assert_eq!((actual.1, actual.2), (h as u32, 64));
        let output = complete_presentation(build(), 1);
        assert_eq!(output, complete_presentation(build(), 16384));
        let alpha = h as f64 / 64.0;
        assert_eq!(native_exr_pixels(output.2.as_deref().unwrap()).get_pixel(0, 0).0, [0.0, 0.0, 0.0, alpha as f32]);
        assert_eq!(alpha_pixel(&output.1), [0, 0, 0, (alpha * 255.0).round() as u8]);
        assert_eq!(build().request.camera, base.request.camera);
        assert_eq!(build().scene.bounds, base.scene.bounds);
        assert!(build().scene.meshes.is_empty());
    }
}

fn prepared_general_glass_fixture(nested: bool, ior: f64) -> ImageTask {
    let original = cpu_general_glass_fixture(8, nested, ior);
    let original_work = original.work.used();
    let scene = original.scene.clone();
    let request = original.request.clone();
    let options = original.options.clone();
    drop(original);
    let cancel = CancelToken::default();
    let proof = certify_for_request(&scene, &request, &options, &cancel).unwrap().unwrap();
    assert!(proof.legacy.is_none());
    assert_eq!((proof.general.len(), proof.general.capacity()), (1, 1));
    assert!(proof.general[0].ready());
    assert!(proof.work_used > 0);
    let certified_work = proof.work_used;
    let allocation = proof.general.as_ptr();
    let source = Arc::as_ptr(&scene.meshes[0].mesh);
    let task = ImageTask::new_prepared(scene, request, cancel, options, None, Some(proof)).unwrap();
    assert_eq!(task.glass.as_ptr(), allocation, "the certified allocation moves directly into the image task");
    assert_eq!(Arc::as_ptr(&task.scene.meshes[0].mesh), source, "certification never rebuilds or repairs the rendered boundary");
    assert!(task.glass[0].ready());
    assert_eq!(task.work.used(), original_work + certified_work, "the one certificate debit follows the proof");
    assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
    task
}

#[test]
fn cpu_general_glass_strict_preparation_moves_the_ready_boundary_without_recertifying_or_changing_any_prefix() {
    for nested in [false, true] {
        for ior in [1.0, 1.5] {
            let standalone = complete_presentation(cpu_general_glass_fixture(8, nested, ior), 1);
            assert!(standalone.2.is_some(), "compare the actual unmodified raw master as well as every PNG");
            for quantum in [1, 16384] {
                let prepared = complete_presentation(prepared_general_glass_fixture(nested, ior), quantum);
                assert_eq!(prepared, standalone, "all prefixes, final PNG, raw EXR and cumulative work must match exactly");
            }
        }
    }
}

#[test]
fn cpu_general_glass_prepared_cancel_and_work_exhaustion_keep_the_original_certificate_debit_without_a_sample() {
    for cancel in [false, true] {
        let mut task = prepared_general_glass_fixture(true, 1.5);
        let used = task.work.used();
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = used;
        }
        let error = task.step_with_previews(1).err().unwrap();
        assert_eq!(error.code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), used);
        assert!(task.glass[0].ready(), "neither failure replaces a complete proof with a fresh cursor");
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
        assert!(task.encoder.is_none() && task.output.is_empty());
    }
}

#[test]
fn cpu_path_tessellated_sheet_preserves_real_transmitted_visibility_film_and_raw_master() {
    let fixture = || {
        let original = cpu_glass_vertical_sheet_fixture(32, 8);
        let mut scene = original.scene.clone();
        let mut mesh = Mesh::default();
        for y in 0..4 {
            for z in 0..2 {
                let y = -10.0 + f64::from(y) * 5.0;
                let z = 1.0 + f64::from(z) * 2.0;
                let begin = mesh.positions.len() as u32;
                mesh.positions.extend([
                    Vec3::new(2.0, y, z),
                    Vec3::new(2.0, y, z + 2.0),
                    Vec3::new(2.0, y + 5.0, z + 2.0),
                    Vec3::new(2.0, y + 5.0, z),
                ]);
                mesh.triangles.extend([[begin, begin + 1, begin + 2], [begin, begin + 2, begin + 3]]);
            }
        }
        scene.meshes[1].mesh = Arc::new(mesh);
        ImageTask::new(scene, original.request.clone(), CancelToken::default(), original.options.clone()).unwrap()
    };
    let baseline = complete_presentation(cpu_glass_vertical_sheet_fixture(32, 8), 16384);
    let actual = complete_presentation(fixture(), 1);
    let batched = complete_presentation(fixture(), 16384);
    assert_eq!(actual, batched, "worker quanta preserve every sheet checkpoint and raw bit");
    assert_eq!(actual.0, baseline.0, "tessellation preserves the complete camera prefixes");
    assert_eq!(actual.1, baseline.1, "tessellation preserves the final PNG");
    assert_eq!(actual.2, baseline.2, "tessellation preserves the linear EXR");
    assert!(actual.3 > baseline.3, "additional sheet proof and traversal are charged rather than hidden");
}

fn cpu_model_sheet_fixture(samples: u32, depth: u8, model: crate::glass_model::GlassModel, absorption: [f64; 3]) -> ImageTask {
    let original = cpu_general_glass_fixture(samples, false, 1.5);
    let mut scene = original.scene.clone();
    scene.meshes[0].mesh = Arc::new(Mesh {
        positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        ..Default::default()
    });
    scene.bounds = scene.meshes[0].mesh.bounds();
    let mut options = original.options.clone();
    options.path = Some(job::CpuPathOptions { max_depth: depth });
    options.dielectrics[0].boundary = job::DielectricBoundary::ThinSheet;
    options.dielectrics[0].thickness_mm = 0.7;
    options.dielectrics[0].absorption_per_mm = absorption;
    options.dielectrics[0].model = Some(model);
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_inactive_glass_model_preserves_every_original_prefix_master_and_work_unit() {
    let original = cpu_general_glass_fixture(8, false, 1.5);
    let mut options = original.options.clone();
    options.dielectrics[0].model = Some(Default::default());
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    let explicit = ImageTask::new(original.scene.clone(), request, CancelToken::default(), options).unwrap();
    assert_eq!(complete_presentation(original, 1), complete_presentation(explicit, 16384));
}
#[test]
fn cpu_spectral_sheet_rgb_furnace_has_unit_mass_and_real_film_response() {
    use crate::glass_model::{Dispersion, GlassModel, ThinFilm};
    let model = GlassModel {
        roughness: 0.0,
        dispersion: Some(Dispersion { abbe_number: 50.0 }),
        thin_film: Some(ThinFilm { ior: 1.3, thickness_nm: 430.0 }),
    };
    let samples = 128;
    // Independent normal-incidence Airy power quotient and lossless parallel
    // slab series. No production Fresnel, Cauchy or sheet helper supplies the
    // expected coefficients. The mean-RGB proposal has unit expected energy,
    // but its finite R/p or T/(1-p) samples differ between colour channels.
    let b = (0.5 / 50.0) / (486.1327_f64.powi(-2) - 656.2725_f64.powi(-2));
    let reflected = [610.0_f64, 550.0, 460.0].map(|wavelength| {
        let substrate = 1.5 + b * (wavelength.powi(-2) - 587.5618_f64.powi(-2));
        let first = (1.0_f64 - 1.3) / (1.0 + 1.3);
        let second = (1.3 - substrate) / (1.3 + substrate);
        let cosine = (4.0 * std::f64::consts::PI * 1.3 * 430.0 / wavelength).cos();
        let f = (first * first + second * second + 2.0 * first * second * cosine) / (1.0 + (first * second).powi(2) + 2.0 * first * second * cosine);
        2.0 * f / (1.0 + f)
    });
    let probability = reflected.into_iter().sum::<f64>() / 3.0;
    let mut expected = [0.0; 3];
    let mut reflection_count = 0;
    for ordinal in 0..samples {
        let reflect = progressive::variates(9, 0, ordinal, 0x5041_5448_4000)[0] < probability;
        reflection_count += u32::from(reflect);
        for (sum, r) in expected.iter_mut().zip(reflected) {
            *sum += if reflect { r / probability } else { (1.0 - r) / (1.0 - probability) } / f64::from(samples);
        }
    }
    assert!(reflection_count > 0 && reflection_count < samples, "both sheet branches need fixed-counter coverage");
    for r in reflected {
        let furnace = probability * (r / probability) + (1.0 - probability) * ((1.0 - r) / (1.0 - probability));
        assert!((furnace - 1.0).abs() < 1e-14, "each wavelength has unit analytic expected furnace energy");
    }
    let (actual, _) = path_mean(cpu_model_sheet_fixture(samples, 4, model, [0.0; 3]));
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-12, "finite RGB slab estimate {actual} vs independent Airy/counter estimate {expected}");
    }
    let coated = complete_presentation(cpu_model_sheet_fixture(32, 4, model, [0.2, 0.1, 0.0]), 1);
    assert_eq!(coated, complete_presentation(cpu_model_sheet_fixture(32, 4, model, [0.2, 0.1, 0.0]), 16384));
    let plain = complete_presentation(cpu_model_sheet_fixture(32, 4, GlassModel { thin_film: None, ..model }, [0.2, 0.1, 0.0]), 16384);
    assert_ne!(coated.2, plain.2, "interference must reach actual linear transport rather than only request metadata");
}
#[test]
fn cpu_rough_sheet_walk_consumes_virtual_interfaces_and_keeps_prefixes_raw_and_quanta_stable() {
    let model = crate::glass_model::GlassModel { roughness: 0.4, ..Default::default() };
    let shallow = path_mean(cpu_model_sheet_fixture(64, 2, model, [0.1, 0.2, 0.0])).0;
    let deep = path_mean(cpu_model_sheet_fixture(64, 12, model, [0.1, 0.2, 0.0])).0;
    assert!(deep.iter().all(|value| value.is_finite() && *value >= 0.0));
    assert!(deep.into_iter().zip(shallow).all(|(deep, shallow)| deep >= shallow));
    assert!(
        deep.into_iter().zip(shallow).any(|(deep, shallow)| deep > shallow + 0.1),
        "completed virtual exits add the transmitted rough-sheet paths"
    );
    let actual = complete_presentation(cpu_model_sheet_fixture(16, 12, model, [0.1, 0.2, 0.0]), 1);
    assert_eq!(actual, complete_presentation(cpu_model_sheet_fixture(16, 12, model, [0.1, 0.2, 0.0]), 16384));
    let smooth = complete_presentation(cpu_model_sheet_fixture(16, 12, Default::default(), [0.1, 0.2, 0.0]), 16384);
    assert_ne!(actual.2, smooth.2, "frosted transport changes the actual film");
}
fn cpu_directional_caustic_fixture(photons: u32, radius_mm: f64) -> ImageTask {
    let original = cpu_general_glass_fixture(1, false, 1.5);
    let mut scene = original.scene.clone();
    scene.meshes.push(ProductMesh {
        name: "Caustic receiver".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-10.0, -10.0, 1.0), Vec3::new(10.0, -10.0, 1.0), Vec3::new(10.0, 10.0, 1.0), Vec3::new(-10.0, 10.0, 1.0)],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }),
        surface: Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 0.5, 1.0).unwrap()),
        color: [255; 3],
        opacity: 1.0,
        faces: vec![],
        transmission: None,
    });
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    let mut options = original.options.clone();
    options.path = Some(job::CpuPathOptions { max_depth: 1 });
    options.dielectrics[0].absorption_per_mm = [0.0; 3];
    options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }]);
    options.environment = Some(job::RaytraceEnvironmentOptions { color: [0.0; 3], intensity: 0.0 });
    options.caustics = Some(job::CausticOptions { photons, radius_mm, max_depth: 8, seed: 81, area_lights: false, environment: false });
    let camera = crate::Camera {
        target: Vec3::new(0.0, 0.0, 1.0),
        distance: 0.5,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        fov: 1e-6,
        ..original.request.camera
    };
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..original.request.clone() };
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
#[test]
fn cpu_delta_source_caustic_power_matches_independent_emission_counter_and_kernel_without_radiance_eta_loss() {
    let photons = 4096;
    let radius = 3.0;
    let fixture = cpu_directional_caustic_fixture(photons, radius);
    let bounds = fixture.scene.bounds;
    let source_radius = (bounds.max - bounds.min).len() * 0.5;
    let [dx, dy] = progressive::variates(9, 0, 0, 0);
    let (origin, direction) = fixture.request.camera.ray(dx, dy, 1.0, 1.0);
    let receiver = origin + direction * ((1.0 - origin.z) / direction.z);
    let mut accepted = 0;
    for ordinal in 0..photons {
        let [u, v] = progressive::variates(81, 0, ordinal, 0x4341_5553_0001);
        let r = source_radius * u.sqrt();
        let position = Vec3::new(r * (std::f64::consts::TAU * v).cos(), r * (std::f64::consts::TAU * v).sin(), 1.0);
        let branch = progressive::variates(81, 0, ordinal, 0x4341_5553_0002)[0];
        if position.dist(receiver) <= radius && branch >= 0.04 {
            accepted += 1;
        }
    }
    assert!(accepted > 0);
    // Each surviving photon carries irradiance*pi*source_radius²/N power;
    // Lambert BRDF is1/pi and the disk kernel is1/(pi*radius²). Entry into
    // n=1.5 retains power: a radiance eta² factor here would fail by2.25.
    let expected = f64::from(accepted) * source_radius.powi(2) / (f64::from(photons) * std::f64::consts::PI * radius * radius);
    let (actual, _) = path_mean(fixture);
    for value in actual {
        assert!((value - expected).abs() < 1e-10, "caustic {value} vs independent photon power {expected}");
    }
    let actual = complete_presentation(cpu_directional_caustic_fixture(512, radius), 1);
    assert!(actual.1.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(actual.2.as_ref().is_some_and(|master| master.starts_with(b"\x76\x2f\x31\x01")));
    assert_eq!(actual, complete_presentation(cpu_directional_caustic_fixture(512, radius), 16384));
    let original = cpu_directional_caustic_fixture(512, radius);
    let mut options = original.options.clone();
    options.caustics = None;
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    assert_eq!(
        path_mean(ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()).0,
        [0.0; 3],
        "ordinary delta NEE cannot sample the closed refractive connection"
    );
}
#[test]
fn cpu_area_photon_caustics_match_rectangle_radiometry_and_partition_camera_depth() {
    let photons = 4096;
    let radius = 3.0;
    let edge = 18.0;
    let intensity = 2.0;
    let mut original = cpu_directional_caustic_fixture(photons, radius);
    for position in &mut Arc::make_mut(&mut original.scene.meshes[1].mesh).positions {
        position.z = -1.0;
    }
    original.scene.bounds = original.scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    original.request.camera.target.z = -1.0;
    original.options.dielectrics[0].ior = 1.0;
    original.options.lights = Some(vec![]);
    original.options.area_lights = Some(vec![job::RectangleLightOptions {
        center_mm: [0.0, 0.0, 3.0],
        edge_u_mm: [edge, 0.0, 0.0],
        edge_v_mm: [0.0, -edge, 0.0],
        color: [1.0; 3],
        intensity,
    }]);
    let camera_seed = 12;
    original.options.progressive.as_mut().unwrap().seed = camera_seed;
    let render = |area_lights, photon_depth, camera_depth, roughness| {
        let mut options = original.options.clone();
        options.caustics = Some(job::CausticOptions { area_lights, max_depth: photon_depth, ..options.caustics.unwrap() });
        options.path = Some(job::CpuPathOptions { max_depth: camera_depth });
        options.dielectrics[0].model = Some(crate::glass_model::GlassModel { roughness, ..Default::default() });
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        path_mean(ImageTask::new(original.scene.clone(), request, CancelToken::default(), options).unwrap()).0
    };

    // Matched-index glass leaves each direction straight. An independent
    // emitter-plane to receiver-plane calculation can count kernel members.
    let [dx, dy] = progressive::variates(camera_seed, 0, 0, 0);
    let (origin, direction) = original.request.camera.ray(dx, dy, 1.0, 1.0);
    let receiver = origin + direction * ((-1.0 - origin.z) / direction.z);
    let mut accepted = 0;
    for ordinal in 0..photons {
        let [s, t] = progressive::variates(81, 0, ordinal, 0x4341_5553_0010);
        let [u, v] = progressive::variates(81, 0, ordinal, 0x4341_5553_0001);
        let radius_on_plane = 4.0 * (u / (1.0 - u)).sqrt();
        let phi = std::f64::consts::TAU * v;
        let position = Vec3::new((s - 0.5) * edge - radius_on_plane * phi.cos(), (0.5 - t) * edge + radius_on_plane * phi.sin(), -1.0);
        if position.dist(receiver) <= radius {
            accepted += 1;
        }
    }
    assert!(accepted > 0);
    let expected = f64::from(accepted) * intensity * edge.powi(2) / (f64::from(photons) * std::f64::consts::PI * radius * radius);
    let camera_only = render(false, 3, 4, 0.0);
    assert!(camera_only[0] > 0.0);
    assert_eq!(render(true, 2, 4, 0.0), camera_only, "the light-path depth cannot own this camera suffix");
    assert_eq!(render(true, 3, 3, 0.0), [0.0; 3], "the camera-path depth cannot fit the emitter endpoint");
    for value in render(true, 3, 4, 0.0) {
        assert!((value - expected).abs() < 1e-10, "rectangle caustic {value} vs independent photon power {expected}");
    }
    assert_eq!(render(true, 3, 4, 0.35), render(false, 3, 4, 0.35), "authored rough glass remains camera sampled at matched IOR");

    let mut options = original.options;
    let lights = options.area_lights.as_mut().unwrap();
    let mut blocker = lights[0].clone();
    blocker.center_mm[2] = 2.5;
    blocker.intensity = 0.0;
    lights.push(blocker);
    options.caustics.as_mut().unwrap().area_lights = true;
    options.path = Some(job::CpuPathOptions { max_depth: 4 });
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
    let blocked = path_mean(ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()).0;
    assert_eq!(blocked, [0.0; 3], "the receiver-facing rectangle intercepts both representations of the same path");
}

#[test]
fn cpu_environment_caustics_match_disk_radiometry_and_preserve_finite_sun() {
    let photons = 4096;
    let radius = 3.0;
    let mut original = cpu_directional_caustic_fixture(photons, radius);
    original.options.dielectrics[0].ior = 1.0;
    original.options.lights = Some(vec![]);
    original.options.environment = Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 });
    let camera_seed = 12;
    original.options.progressive.as_mut().unwrap().seed = camera_seed;
    let render = |environment, photon_depth, camera_depth, sun: bool, blocker: bool| {
        let mut options = original.options.clone();
        options.caustics = Some(job::CausticOptions { environment, max_depth: photon_depth, ..options.caustics.unwrap() });
        options.path = Some(job::CpuPathOptions { max_depth: camera_depth });
        options.sun = sun.then(|| Box::new(job::Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.7, radiance: [0.25, 0.5, 0.75] }));
        if blocker {
            options.area_lights = Some(vec![job::RectangleLightOptions {
                center_mm: [0.0, 0.0, 64.0],
                edge_u_mm: [1e6, 0.0, 0.0],
                edge_v_mm: [0.0, -1e6, 0.0],
                color: [1.0; 3],
                intensity: 0.0,
            }]);
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        path_mean(ImageTask::new(original.scene.clone(), request, CancelToken::default(), options).unwrap()).0
    };

    // The receiver plane crosses the launch sphere's center. Project the
    // disk sample onto it along the incident line, independently of tracing.
    let bounds = original.scene.bounds;
    let center = (bounds.min + bounds.max) * 0.5;
    let source_radius = (bounds.max - bounds.min).len() * 0.5;
    let [dx, dy] = progressive::variates(camera_seed, 0, 0, 0);
    let (origin, direction) = original.request.camera.ray(dx, dy, 1.0, 1.0);
    let receiver = origin + direction * ((1.0 - origin.z) / direction.z);
    let mut accepted = 0;
    for ordinal in 0..photons {
        let [s, t] = progressive::variates(81, 0, ordinal, 0x4341_5553_0010);
        let z = 1.0 - 2.0 * s;
        if z <= 0.0 {
            continue;
        }
        let phi = std::f64::consts::TAU * t;
        let xy = (1.0 - z * z).sqrt();
        let toward = Vec3::new(xy * phi.cos(), xy * phi.sin(), z);
        let tangent = super::super::unit((if z > 0.9 { Vec3::Y } else { Vec3::Z }).cross(toward)).unwrap();
        let [u, v] = progressive::variates(81, 0, ordinal, 0x4341_5553_0001);
        let angle = std::f64::consts::TAU * v;
        let disk = (tangent * angle.cos() + toward.cross(tangent) * angle.sin()) * (source_radius * u.sqrt());
        let position = center + disk - toward * (disk.z / z);
        if position.dist(receiver) <= radius {
            accepted += 1;
            let blocker = position + toward * (63.0 / z);
            assert!(blocker.x.abs().max(blocker.y.abs()) < 5e5);
        }
    }
    assert!(accepted > 0);
    let expected = f64::from(accepted) * 4.0 * source_radius.powi(2) / (f64::from(photons) * radius * radius);
    let camera_with_sun = render(false, 2, 3, true, false);
    let camera_without_sun = render(false, 2, 3, false, false);
    assert!(camera_without_sun[0] > 0.0 && camera_with_sun[0] > camera_without_sun[0]);
    assert_eq!(render(true, 1, 3, true, false), camera_with_sun, "short photon depth keeps the entire camera endpoint");
    assert_eq!(render(true, 2, 2, true, false), [0.0; 3], "short camera depth cannot fit the endpoint");
    assert_eq!(render(true, 2, 3, false, true), [0.0; 3], "the rectangle beyond the launch disk blocks the environment");
    for (channel, value) in render(true, 2, 3, true, false).into_iter().enumerate() {
        let expected_with_sun = expected + camera_with_sun[channel] - camera_without_sun[channel];
        assert!((value - expected_with_sun).abs() < 1e-10, "environment caustic plus sun {value} vs {expected_with_sun}");
    }
}

#[test]
fn cpu_caustic_prepass_is_prepaid_and_cancels_or_exhausts_before_any_image_commit() {
    for cancel in [false, true] {
        let mut task = cpu_directional_caustic_fixture(512, 3.0);
        let mut entered = false;
        for _ in 0..200_000 {
            if task.bvh.is_some() && task.glass.first().is_some_and(|glass| glass.ready()) && task.caustics.first().is_some_and(|map| !map.ready()) {
                entered = true;
                break;
            }
            task.step_with_previews(1).unwrap();
        }
        assert!(entered);
        assert!(task.admission.preparation_bytes >= caustics::Task::required_bytes(task.options.caustics.unwrap()).unwrap());
        let before = task.work.used();
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = before;
        }
        assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), before);
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
    }
}

#[test]
fn cpu_smooth_rgb_film_dispersion_sheet_keeps_transmitted_shadow_mis_and_inactive_identity() {
    let fixture = |active: bool| {
        let original = cpu_glass_vertical_sheet_fixture(16, 4);
        let mut options = original.options.clone();
        options.dielectrics[0].model = Some(if active {
            crate::glass_model::GlassModel {
                roughness: 0.0,
                dispersion: Some(crate::glass_model::Dispersion { abbe_number: 45.0 }),
                thin_film: Some(crate::glass_model::ThinFilm { ior: 1.3, thickness_nm: 430.0 }),
            }
        } else {
            Default::default()
        });
        if active {
            options.dielectrics[0].absorption_per_mm = [0.1, 0.2, 0.0];
            options.dielectrics[0].thickness_mm = 0.7;
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    let legacy = complete_presentation(cpu_glass_vertical_sheet_fixture(16, 4), 1);
    assert_eq!(legacy, complete_presentation(fixture(false), 16384));
    let active = complete_presentation(fixture(true), 1);
    assert_eq!(active, complete_presentation(fixture(true), 16384));
    assert_ne!(active.2, legacy.2, "the spectral slab affects actual transmitted-light connections and BSDF chains");
    let raw = native_exr_pixels(active.2.as_deref().unwrap());
    assert!(raw.pixels().all(|pixel| pixel.0.iter().all(|value| value.is_finite() && *value >= 0.0)));
}
#[test]
fn cpu_photon_prepass_respects_an_analytic_emitter_before_the_opaque_receiver() {
    let original = cpu_directional_caustic_fixture(512, 3.0);
    let mut options = original.options.clone();
    options.area_lights = Some(vec![job::RectangleLightOptions {
        center_mm: [0.0, 0.0, 1.75],
        edge_u_mm: [18.0, 0.0, 0.0],
        edge_v_mm: [0.0, 18.0, 0.0],
        color: [1.0; 3],
        intensity: 0.0,
    }]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
    assert_eq!(
        path_mean(ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()).0,
        [0.0; 3],
        "nonradiant analytic area geometry still blocks source photons before the receiver"
    );
}
#[test]
fn cpu_point_source_inside_certified_glass_keeps_initial_medium_and_adds_reflected_caustic_power() {
    let fixture = |caustics: bool| {
        let original = cpu_directional_caustic_fixture(1024, 3.0);
        let mut options = original.options.clone();
        options.lights = Some(vec![]);
        options.punctual_lights = Some(vec![job::PunctualLightOptions {
            position_mm: [0.0, 0.0, 1.5],
            color: [1.0; 3],
            intensity: 1.0,
            reference_distance_mm: 1.0,
            minimum_distance_mm: 0.001,
            cone: None,
        }]);
        if !caustics {
            options.caustics = None;
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..original.request.clone() };
        ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
    };
    let baseline = path_mean(fixture(false)).0;
    let actual = path_mean(fixture(true)).0;
    assert!(
        actual.into_iter().zip(baseline).all(|(actual, baseline)| actual > baseline),
        "inside-source initialization must preserve upward photons reflected by the closed boundary; direct paths are already covered by NEE"
    );
}

fn cpu_hard_rough_fixture(samples: u32, depth: u8, sheet: bool, roughness: f64, light: [f64; 3]) -> ImageTask {
    let model = crate::glass_model::GlassModel { roughness, ..Default::default() };
    let base = if sheet { cpu_model_sheet_fixture(samples, depth, model, [0.0; 3]) } else { cpu_general_glass_fixture(samples, false, 1.5) };
    let mut options = base.options.clone();
    options.path = Some(job::CpuPathOptions { max_depth: depth });
    options.dielectrics[0].model = Some(model);
    options.dielectrics[0].absorption_per_mm = [0.0; 3];
    options.lights = Some(vec![job::DirectionalLightOptions { direction: light, color: [1.0; 3], intensity: 1.0 }]);
    options.environment = Some(job::RaytraceEnvironmentOptions { color: [0.0; 3], intensity: 0.0 });
    let camera = crate::Camera {
        target: Vec3::new(0.0, 0.0, if sheet { 0.0 } else { 2.0 }),
        distance: 0.5,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        fov: 1e-9,
        ..base.request.camera
    };
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
    ImageTask::new(base.scene, request, CancelToken::default(), options).unwrap()
}
fn cpu_reconfigure(base: ImageTask, change: impl FnOnce(&mut RaytraceOptions)) -> ImageTask {
    let mut options = base.options.clone();
    change(&mut options);
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    ImageTask::new(base.scene, request, CancelToken::default(), options).unwrap()
}
fn assert_rgb_close(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() <= tolerance, "linear radiance {a} vs independent expected {e}");
    }
}
#[test]
fn cpu_rough_hard_source_reflection_has_independent_normal_ggx_measure_at_the_last_vertex() {
    // At normal incidence, D=1/(pi*r^4), G=1 and F=((1-1.5)/(1+1.5))^2=.04.
    // A hard source contributes f*cos(theta), without a light-selection PDF.
    for sheet in [false, true] {
        for roughness in [0.4_f64, 0.7, 1.0] {
            let expected = 0.04 / (4.0 * std::f64::consts::PI * roughness.powi(4));
            assert_rgb_close(path_mean(cpu_hard_rough_fixture(4, 1, sheet, roughness, [0.0, 0.0, 1.0])).0, [expected; 3], 1e-12);
        }
        for spot in [false, true] {
            let base = cpu_hard_rough_fixture(4, 1, sheet, 0.5, [0.0, 0.0, 1.0]);
            let z = if sheet { 2.0 } else { 4.0 };
            let point = cpu_reconfigure(base, |options| {
                options.lights = Some(vec![]);
                options.punctual_lights = Some(vec![job::PunctualLightOptions {
                    position_mm: [0.0, 0.0, z],
                    color: [1.0; 3],
                    intensity: 2.0,
                    reference_distance_mm: 3.0,
                    minimum_distance_mm: 0.001,
                    cone: spot.then_some(job::SpotConeOptions {
                        emitted_axis: [0.0, 0.0, -1.0],
                        inner_half_angle_radians: 0.2,
                        outer_half_angle_radians: 0.4,
                    }),
                }]);
            });
            let expected = 0.04 / (4.0 * std::f64::consts::PI * 0.5_f64.powi(4)) * 2.0 * 9.0 / 4.0;
            assert_rgb_close(path_mean(point).0, [expected; 3], 1e-10);
        }
    }
}
#[test]
fn cpu_inside_point_and_spot_rough_transmission_apply_physical_beer_and_hg_survival() {
    for spot in [false, true] {
        let fixture = cpu_reconfigure(cpu_hard_rough_fixture(4, 1, false, 0.5, [0.0, 0.0, -1.0]), |options| {
            options.lights = Some(vec![]);
            options.dielectrics[0].absorption_per_mm = [0.2, 0.4, 0.0];
            options.dielectrics[0].subsurface = Some(crate::transmission::subsurface::Subsurface { scattering_per_mm: 0.3, anisotropy: 0.4 });
            options.punctual_lights = Some(vec![job::PunctualLightOptions {
                position_mm: [0.0, 0.0, 1.0],
                color: [1.0; 3],
                intensity: 1.0,
                reference_distance_mm: 1.0,
                minimum_distance_mm: 0.001,
                cone: spot.then_some(job::SpotConeOptions {
                    emitted_axis: [0.0, 0.0, 1.0],
                    inner_half_angle_radians: 0.2,
                    outer_half_angle_radians: 0.4,
                }),
            }]);
        });
        // Normal Walter radiance BTDF: (1-F)*D*ni^2/(ni-nt)^2.
        // The source is exactly1mm into the transmitted-side medium. Visibility
        // excludes its endpoint by one ULP; attenuation still uses the full mm.
        let btdf = 0.96 / (std::f64::consts::PI * 0.5_f64.powi(4) * 0.5_f64.powi(2));
        assert_rgb_close(path_mean(fixture).0, [0.2_f64, 0.4, 0.0].map(|a| btdf * (-(a + 0.3)).exp()), 1e-9);
    }
}
#[test]
fn cpu_rough_direct_lighting_survives_a_null_sampled_continuation() {
    let seed = (0..4096)
        .find(|seed| {
            let [branch, u] = progressive::variates(*seed, 0, 0, 0x5041_5448_4000);
            branch < 0.02 && u > 0.75
        })
        .expect("fixed counter covers a reflected visible facet below the macro horizon");
    for sheet in [false, true] {
        let mut task = cpu_reconfigure(cpu_hard_rough_fixture(1, 2, sheet, 1.0, [0.0, 0.0, 1.0]), |o| o.progressive.as_mut().unwrap().seed = seed);
        let mut null_commit = false;
        for _ in 0..200_000 {
            if let Some(state) = task.path.first()
                && state.stage_name() == "commit"
            {
                assert_eq!(state.medium_probe().unwrap().depth, 0, "the sampled continuation is actually null at the first interface");
                null_commit = true;
                break;
            }
            task.step_with_previews(1).unwrap();
        }
        assert!(null_commit);
        assert_rgb_close(path_mean(task).0, [0.04 / (4.0 * std::f64::consts::PI); 3], 1e-12);
    }
}
fn cpu_backlit_rough_fixture(depth: u8, sheet: bool, map: bool, photons: u32) -> ImageTask {
    cpu_reconfigure(cpu_hard_rough_fixture(32, depth, sheet, 0.4, [0.0, 0.0, -1.0]), |options| {
        options.caustics =
            map.then_some(job::CausticOptions { photons, radius_mm: 3.0, max_depth: 2, seed: 81, area_lights: false, environment: false });
    })
}
#[test]
fn cpu_rough_receiver_reservoir_finalizes_at_depth_and_camera_partition_has_one_first_continuous_receiver() {
    for sheet in [false, true] {
        let map_one = path_mean(cpu_backlit_rough_fixture(1, sheet, true, 1024)).0;
        let map_two = path_mean(cpu_backlit_rough_fixture(2, sheet, true, 1024)).0;
        assert_eq!(map_one, map_two, "a sampled rough continuation must not count a second photon/NEE receiver");
        assert!(map_one.into_iter().all(|value| value > 0.0), "the reservoir retains the terminal second-interface rough receiver");
        assert_eq!(path_mean(cpu_backlit_rough_fixture(1, sheet, false, 1024)).0, [0.0; 3]);
        assert!(
            path_mean(cpu_backlit_rough_fixture(2, sheet, false, 1024)).0.into_iter().all(|value| value > 0.0),
            "without the map, the last-depth internal escape connection contributes"
        );
    }
    let fixture = || cpu_reconfigure(cpu_backlit_rough_fixture(2, true, true, 256), |o| o.progressive.as_mut().unwrap().samples_per_pixel = 4);
    assert_eq!(
        complete_presentation(fixture(), 1),
        complete_presentation(fixture(), 16384),
        "reservoir, gather and NEE keep every prefix, raw bit and charged work unit across quanta"
    );
}
#[test]
fn cpu_internal_rough_sheet_direct_survives_a_later_null_interface_sample() {
    let mut found = false;
    for seed in 0..256 {
        let mut task = cpu_reconfigure(cpu_hard_rough_fixture(1, 3, true, 1.0, [0.0, 0.0, -1.0]), |o| o.progressive.as_mut().unwrap().seed = seed);
        let mut retained = None;
        for _ in 0..50_000 {
            if let Some(state) = task.path.first() {
                if let Some((1, 3, pending, radiance)) = state.hard_light_probe()
                    && pending.x > 0.0
                {
                    retained = Some(radiance + pending);
                }
                if state.stage_name() == "commit" {
                    if state.medium_probe().unwrap().depth == 1
                        && let Some(expected) = retained
                    {
                        assert_rgb_close(path_mean(task).0, [expected.x, expected.y, expected.z], 1e-12);
                        found = true;
                    }
                    break;
                }
            }
            task.step_with_previews(1).unwrap();
        }
        if found {
            break;
        }
    }
    assert!(found, "fixed counter search must exercise positive internal lighting followed by a null microfacet continuation");
}
#[test]
fn cpu_hard_glass_lighting_is_cooperative_and_cancellable_before_any_film_commit() {
    for cancel in [false, true] {
        let mut task = cpu_hard_rough_fixture(4, 1, false, 0.5, [0.0, 0.0, 1.0]);
        let mut entered = false;
        for _ in 0..200_000 {
            if task.path.first().is_some_and(|state| state.stage_name() == "glass_lighting") {
                entered = true;
                break;
            }
            task.step_with_previews(1).unwrap();
        }
        assert!(entered);
        let before = task.work.used();
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = before;
        }
        assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), before);
        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
        assert!(task.take_linear_exr().is_none());
    }
}
#[test]
fn cpu_selected_hg_hard_point_and_spot_nee_match_independent_collision_phase_and_extinction() {
    let samples = 64;
    for spot in [false, true] {
        let base = cpu_subsurface_fixture(samples, false, 0, 1, [0.8, 0.0], 0.4, None);
        let mut options = base.options.clone();
        options.dielectrics[0].absorption_per_mm = [0.2, 0.4, 0.0];
        options.environment = Some(job::RaytraceEnvironmentOptions { color: [0.0; 3], intensity: 0.0 });
        options.punctual_lights = Some(vec![job::PunctualLightOptions {
            position_mm: [1.0, 0.0, 1.0],
            color: [1.0; 3],
            intensity: 2.0,
            reference_distance_mm: 1.0,
            minimum_distance_mm: 0.001,
            cone: spot.then_some(job::SpotConeOptions {
                emitted_axis: [-1.0, 0.0, -0.3],
                inner_half_angle_radians: 0.1,
                outer_half_angle_radians: 1.1,
            }),
        }]);
        let camera = crate::Camera { fov: 1e-9, ..base.request.camera };
        let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
        let mut expected = [0.0; 3];
        let mut collisions = 0;
        for ordinal in 0..samples {
            let [x, y] = progressive::variates(9, 0, ordinal, 0);
            let (origin, direction) = camera.ray(x, y, 1.0, 1.0);
            let u = progressive::variates(9, 0, ordinal, 0x5041_5448_5000)[0];
            let travel = -(1.0 - u).ln() / 0.8;
            if travel >= -origin.z / direction.z {
                continue;
            }
            collisions += 1;
            let point = origin + direction * travel;
            let delta = Vec3::new(1.0, 0.0, 1.0) - point;
            let distance = delta.len();
            let toward_source = delta / distance;
            let cosine = direction.dot(toward_source);
            let phase = (1.0 - 0.4_f64.powi(2)) / (4.0 * std::f64::consts::PI * (1.0 + 0.4_f64.powi(2) - 0.8 * cosine).powf(1.5));
            let cone = if spot {
                let axis = Vec3::new(-1.0, 0.0, -0.3);
                let cosine = (-toward_source).dot(axis / axis.len());
                let t = ((cosine - 1.1_f64.cos()) / (0.1_f64.cos() - 1.1_f64.cos())).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            } else {
                1.0
            };
            for (sum, absorption) in expected.iter_mut().zip([0.2_f64, 0.4, 0.0]) {
                *sum += phase * 2.0 * cone / distance.powi(2) * (-absorption * (travel + distance) - 0.8 * distance).exp() / f64::from(samples);
            }
        }
        assert!(collisions > 0 && collisions < samples);
        assert_rgb_close(path_mean(ImageTask::new(base.scene, request, CancelToken::default(), options).unwrap()).0, expected, 1e-12);
    }
}
#[test]
fn cpu_hg_photon_reservoir_reaches_volume_receivers_after_a_closed_optical_event() {
    let fixture = |map: bool| {
        let base = cpu_subsurface_fixture(64, false, 0, 1, [0.8, 0.0], 0.4, None);
        cpu_reconfigure(base, |options| {
            options.dielectrics[0].absorption_per_mm = [0.0; 3];
            options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }]);
            options.environment = Some(job::RaytraceEnvironmentOptions { color: [0.0; 3], intensity: 0.0 });
            options.caustics =
                map.then_some(job::CausticOptions { photons: 1024, radius_mm: 3.0, max_depth: 4, seed: 81, area_lights: false, environment: false });
        })
    };
    assert_eq!(path_mean(fixture(false)).0, [0.0; 3], "a straight delta connection cannot cross the closed optical boundary");
    let mut task = fixture(true);
    for _ in 0..1_000_000 {
        if task.caustics.first().is_some_and(|map| map.ready()) {
            break;
        }
        task.step_with_previews(1).unwrap();
    }
    let counts = task.caustics.first().unwrap().receiver_counts();
    assert!(counts[2] > 0, "selected HG collisions must be actual photon receivers");
    assert!(counts.into_iter().sum::<usize>() <= 1024, "one reservoir record at most per launched light path");
    assert!(path_mean(task).0.into_iter().all(|value| value > 0.0));
}
#[test]
fn cpu_matched_index_coated_rough_delta_transmission_keeps_the_first_continuous_photon_receiver() {
    let base = cpu_backlit_rough_fixture(3, false, true, 1024);
    let mut scene = base.scene.clone();
    let mut upper = dielectric::tests::box_body(0.5);
    upper.name = "Matched coated upper glass".into();
    for position in &mut Arc::make_mut(&mut upper.mesh).positions {
        position.z += 2.5;
    }
    scene.meshes.push(upper);
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    let mut options = base.options.clone();
    options.progressive.as_mut().unwrap().samples_per_pixel = 64;
    let mut upper_input = options.dielectrics[0].clone();
    upper_input.body_index = 1;
    upper_input.body_name = scene.meshes[1].name.clone();
    upper_input.ior = 1.0;
    upper_input.model = Some(crate::glass_model::GlassModel {
        roughness: 0.7,
        thin_film: Some(crate::glass_model::ThinFilm { ior: 1.3, thickness_nm: 250.0 }),
        dispersion: None,
    });
    options.dielectrics.push(upper_input);
    let camera = crate::Camera { target: Vec3::new(0.0, 0.0, 3.0), ..base.request.camera };
    let request = ImageRequest { camera, raytrace: Some(options.clone()), ..base.request.clone() };
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    let mut reached = false;
    for _ in 0..1_000_000 {
        if let Some(state) = task.path.first()
            && let Some((2, _, hit)) = state.surface_source()
            && task.looks[hit.primitive_index].body_index == 0
        {
            assert!(!state.continuous_sampled(), "both coated matched-index transmissions are delta events despite authored roughness");
            reached = true;
            break;
        }
        task.step_with_previews(1).unwrap();
    }
    assert!(reached);
    assert!(path_mean(task).0.into_iter().all(|value| value > 0.0), "a later rough receiver still gathers after a chain of actual delta samples");
}
#[test]
fn cpu_rough_hard_visibility_crosses_a_smooth_sheet_and_keeps_earlier_analytic_occlusion() {
    let fixture = |sheet: bool, blocker: bool| {
        let base = cpu_hard_rough_fixture(4, 1, false, 0.5, [0.2, 0.0, 1.0]);
        let mut scene = base.scene.clone();
        let mut options = base.options.clone();
        if sheet {
            let mut body = scene.meshes[0].clone();
            body.name = "Smooth optical visibility sheet".into();
            body.mesh = Arc::new(Mesh {
                positions: vec![Vec3::new(-10.0, -10.0, 3.5), Vec3::new(10.0, -10.0, 3.5), Vec3::new(10.0, 10.0, 3.5), Vec3::new(-10.0, 10.0, 3.5)],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                ..Default::default()
            });
            let mut input = options.dielectrics[0].clone();
            input.body_index = 1;
            input.body_name = body.name.clone();
            input.boundary = job::DielectricBoundary::ThinSheet;
            input.model = None;
            input.thickness_mm = 0.2;
            input.shadow_transmission = true;
            options.dielectrics.push(input);
            scene.meshes.push(body);
        }
        if blocker {
            options.area_lights = Some(vec![job::RectangleLightOptions {
                center_mm: [0.0, 0.0, 3.0],
                edge_u_mm: [18.0, 0.0, 0.0],
                edge_v_mm: [0.0, -18.0, 0.0],
                color: [1.0; 3],
                intensity: 0.0,
            }]);
        }
        scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
    };
    let bare = path_mean(fixture(false, false)).0;
    let cosine = 1.0 / 1.04_f64.sqrt();
    let transmitted_cosine = (1.0 - (1.0 - cosine * cosine) / 2.25).sqrt();
    let rs = ((cosine - 1.5 * transmitted_cosine) / (cosine + 1.5 * transmitted_cosine)).powi(2);
    let rp = ((1.5 * cosine - transmitted_cosine) / (1.5 * cosine + transmitted_cosine)).powi(2);
    // Internal bounces retain their polarization. Sum each independent
    // lossless slab series before averaging the two incident polarizations.
    let transmission = 0.5 * ((1.0 - rs) / (1.0 + rs) + (1.0 - rp) / (1.0 + rp));
    assert_rgb_close(path_mean(fixture(true, false)).0, bare.map(|value| value * transmission), 1e-12);
    assert_eq!(
        path_mean(fixture(true, true)).0,
        [0.0; 3],
        "the area before the transmitted sheet remains inside the original physical visibility segment"
    );
}
#[test]
fn cpu_photon_multi_receiver_reservoir_keeps_one_record_and_has_an_independent_finite_counter_power_oracle() {
    let base = cpu_directional_caustic_fixture(4096, 3.0);
    let mut scene = base.scene.clone();
    scene.meshes[0] = dielectric::tests::box_body(1.0);
    for p in &mut Arc::make_mut(&mut scene.meshes[0].mesh).positions {
        p.z += 4.0;
    }
    let mut lower = dielectric::tests::box_body(1.0);
    lower.name = "Lower matched rough glass".into();
    for p in &mut Arc::make_mut(&mut lower.mesh).positions {
        p.z += 2.0;
    }
    scene.meshes.push(lower);
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    let mut options = base.options.clone();
    options.caustics.as_mut().unwrap().max_depth = 5;
    options.dielectrics[0].body_name = scene.meshes[0].name.clone();
    options.dielectrics[0].ior = 1.0;
    options.dielectrics[0].model = Some(crate::glass_model::GlassModel { roughness: 0.4, ..Default::default() });
    let mut second = options.dielectrics[0].clone();
    second.body_index = 2;
    second.body_name = scene.meshes[2].name.clone();
    options.dielectrics.push(second);
    let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    let [x, y] = progressive::variates(9, 0, 0, 0);
    let (origin, direction) = task.request.camera.ray(x, y, 1.0, 1.0);
    let receiver = origin + direction * ((1.0 - origin.z) / direction.z);
    let source_radius = (task.scene.bounds.max - task.scene.bounds.min).len() * 0.5;
    let mut accepted = 0;
    for ordinal in 0..4096 {
        let [u, v] = progressive::variates(81, 0, ordinal, 0x4341_5553_0001);
        let r = source_radius * u.sqrt();
        let position = Vec3::new(r * (std::f64::consts::TAU * v).cos(), r * (std::f64::consts::TAU * v).sin(), 1.0);
        // All four interfaces have equal indices: straight, unit power, no
        // resampling or Fresnel loss. The first optical event is ineligible;
        // exit/entry/exit/opaque are exactly four candidate receiver visits.
        // The last independent reservoir draw alone decides opaque retention.
        let retain_opaque = progressive::variates(81, 0, ordinal, 0x4341_5553_0000 + 4 * 256 + 13)[0] < 0.25;
        if position.dist(receiver) <= 3.0 && retain_opaque {
            accepted += 1;
        }
    }
    assert!(accepted > 0);
    for _ in 0..2_000_000 {
        if task.caustics.first().is_some_and(|map| map.ready()) {
            break;
        }
        task.step_with_previews(1).unwrap();
    }
    let counts = task.caustics.first().unwrap().receiver_counts();
    assert!(counts[0] > 0 && counts[1] > 0, "the same bounded map stores both opaque and optical visits");
    assert!(counts.into_iter().sum::<usize>() <= 4096);
    let expected = 4.0 * f64::from(accepted) * source_radius.powi(2) / (4096.0 * std::f64::consts::PI * 9.0);
    assert_rgb_close(path_mean(task).0, [expected; 3], 1e-10);
}
