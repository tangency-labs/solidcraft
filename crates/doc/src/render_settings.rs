//! Saved render intent, independent of runtime backend availability and GPU resources.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;
use solidcraft_render::{BackendId, Camera, RenderFormat, RenderLimits, RenderMode, UpAxis, ViewFrame};

pub use crate::render_job::{CpuJob, GpuJob, RenderJob, SkyEnvironment};
use crate::{DocError, Result};

pub const MAX_RENDER_SETUPS: usize = 32;
pub const MAX_SETUP_NAME_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderSetup {
    pub backend: BackendId,
    pub mode: RenderMode,
    #[serde(deserialize_with = "SavedCamera::deserialize")]
    pub camera: Camera,
    pub width: u32,
    pub height: u32,
    pub format: RenderFormat,
    pub exposure_ev: f64,
    /// Legacy common environment orientation, in radians; typed jobs require 0 here.
    pub environment_rotation: f64,
    pub environment_intensity: f64,
    /// sRGB display backplate, independent of the method's environment lighting.
    pub background: [u8; 3],
    /// Selected method's numerical intent and HDR reference; absent means the legacy request protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<RenderJob>,
    #[serde(skip_serializing_if = "crate::render_surface::SurfaceBindings::is_empty")]
    pub surfaces: crate::render_surface::SurfaceBindings,
    #[serde(skip_serializing_if = "crate::render_optics::OpticalBindings::is_empty")]
    pub optics: crate::render_optics::OpticalBindings,
    #[serde(skip_serializing_if = "solidcraft_render::stage::Stage::is_empty")]
    pub stage: solidcraft_render::stage::Stage,
}

// Reuse Camera's representation and defaults, but reject misspelled saved fields. A remote
// derive constructs Camera itself, so adding a Camera field requires updating this adapter.
// Navigation/named-view deserialization keeps its existing compatibility policy.
#[derive(Deserialize)]
#[serde(remote = "Camera", deny_unknown_fields)]
struct SavedCamera {
    target: Vec3,
    yaw: f64,
    pitch: f64,
    #[serde(default)]
    roll: f64,
    distance: f64,
    fov: f64,
    #[serde(default)]
    up: UpAxis,
    #[serde(default)]
    frame: Option<ViewFrame>,
}

impl Default for RenderSetup {
    fn default() -> Self {
        // The built-in descriptor owns the canonical default backend identifier.
        Self {
            backend: solidcraft_render::builtin_backend().id,
            mode: RenderMode::Preview,
            camera: Camera::default(),
            width: 1024,
            height: 768,
            format: RenderFormat::Png,
            exposure_ev: 0.0,
            environment_rotation: 0.0,
            environment_intensity: 1.0,
            background: [236, 240, 245],
            job: None,
            optics: Default::default(),
            surfaces: Default::default(),
            stage: Default::default(),
        }
    }
}

impl RenderSetup {
    pub fn validate(&self) -> Result<()> {
        let limits = match &self.job {
            Some(RenderJob::CpuRaytrace { options }) => options.budget.image_limits(options.progressive.is_some()),
            Some(RenderJob::GpuPath { .. }) => solidcraft_render::job::GPU_PATH_LIMITS,
            _ => RenderLimits::default(),
        };
        self.validate_with_limits(limits)
    }

    /// Runtime CPU lowering has already moved typed options out of this setup.
    /// Saving still uses validate(), so a legacy setup cannot smuggle a large profile.
    pub fn validate_for_cpu(&self, budget: solidcraft_render::job::CpuBudget) -> Result<()> {
        budget.validate_image(self.width, self.height, None).map_err(|e| DocError::Invalid(e.to_string()))?;
        self.validate_with_limits(budget.image_limits(false))
    }

    /// Typed GPU options have already been lowered; retain their dimension envelope.
    pub fn validate_for_gpu(&self) -> Result<()> {
        self.validate_with_limits(solidcraft_render::job::GPU_PATH_LIMITS)
    }

    fn validate_with_limits(&self, limits: RenderLimits) -> Result<()> {
        self.surfaces.validate()?;
        self.optics.validate()?;
        if !self.optics.is_empty() && !matches!(self.job, Some(RenderJob::GpuPath { .. })) {
            return Err(DocError::Invalid("saved optics require an explicit GPU job".into()));
        }
        if self.optics.iter().any(|o| self.surfaces.iter().any(|s| s.target == o.target)) {
            return Err(DocError::Invalid("saved opaque surface maps and optical boundaries cannot share a target".into()));
        }
        self.stage.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
        if !self.stage.is_empty() && matches!(self.job, Some(RenderJob::Raster { .. })) {
            return Err(DocError::Invalid("passive stage geometry requires raytracing; raster cannot silently discard it".into()));
        }
        if !self.surfaces.is_empty() && !matches!(self.job, Some(RenderJob::GpuPath { .. })) {
            return Err(DocError::Invalid("saved surfaces require an explicit GPU job; unsupported methods cannot discard bindings".into()));
        }
        let (width, height) = (u64::from(self.width), u64::from(self.height));
        if width == 0 || height == 0 {
            return Err(DocError::Invalid("render output must be at least 1 × 1 pixels".into()));
        }
        if let Some((w, h)) = limits.fit(width, height, None)
            && (w, h) != (width, height)
        {
            return Err(DocError::Invalid(format!(
                "render output {width} × {height} exceeds the image limits ({} pixels per side, {} pixels in all); \
                 the largest image at this aspect is {w} × {h}",
                limits.max_dimension, limits.max_pixels
            )));
        }
        let camera = &self.camera;
        if !camera.is_valid()
            || [camera.target.x, camera.target.y, camera.target.z, camera.distance].into_iter().any(|x| x.abs() > 1e9)
            || [camera.yaw, camera.pitch, camera.roll].into_iter().any(|x| x.abs() > 1e6)
            || !camera.fov.is_finite()
            || !(0.0..std::f64::consts::PI).contains(&camera.fov)
        {
            return Err(DocError::Invalid("invalid render camera (finite mm/radians, positive distance, 0 <= fov < pi required)".into()));
        }
        if !self.exposure_ev.is_finite() || !(-32.0..=32.0).contains(&self.exposure_ev) {
            return Err(DocError::Invalid("render exposure must be finite and between -32 and 32 EV".into()));
        }
        if !self.environment_rotation.is_finite() || self.environment_rotation.abs() > 1e6 {
            return Err(DocError::Invalid("render environment rotation must be finite, bounded radians".into()));
        }
        if !self.environment_intensity.is_finite() || !(0.0..=100.0).contains(&self.environment_intensity) {
            return Err(DocError::Invalid("render environment intensity must be finite and between 0 and 100".into()));
        }
        if let Some(job) = &self.job {
            if self.environment_rotation != 0.0 || self.environment_intensity != 1.0 {
                return Err(DocError::Invalid(
                    "saved job environment controls belong to job.options.environment or hdr; legacy environment_rotation/intensity must be 0/1"
                        .into(),
                ));
            }
            job.validate(&self.camera, self.exposure_ev)?;
            if let RenderJob::CpuRaytrace { options } = job
                && let Some(progressive) = options.progressive
            {
                progressive.validate_image_with_budget(self.width, self.height, options.budget).map_err(|e| DocError::Invalid(e.to_string()))?;
            }
        }
        // Desired mode/format/backend may be unavailable on this host. Saving intent is not preflight.
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "UncheckedSettings")]
pub struct RenderSettings {
    pub setups: BTreeMap<String, RenderSetup>,
    #[serde(skip_serializing_if = "crate::render_resources::HdrResources::is_empty")]
    pub resources: crate::render_resources::HdrResources,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UncheckedSettings {
    setups: BTreeMap<String, RenderSetup>,
    resources: crate::render_resources::HdrResources,
}

impl TryFrom<UncheckedSettings> for RenderSettings {
    type Error = DocError;
    fn try_from(value: UncheckedSettings) -> Result<Self> {
        let settings = Self { setups: value.setups, resources: value.resources };
        settings.validate()?;
        Ok(settings)
    }
}

impl RenderSettings {
    pub fn is_empty(&self) -> bool {
        self.setups.is_empty() && self.resources.is_empty()
    }

    pub fn validate_name(name: &str) -> Result<()> {
        if name.trim().is_empty() || name != name.trim() || name.len() > MAX_SETUP_NAME_BYTES || name.chars().any(char::is_control) {
            return Err(DocError::Invalid("render setup name must be trimmed, nonempty, at most 128 bytes and contain no control characters".into()));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.setups.len() > MAX_RENDER_SETUPS {
            return Err(DocError::Invalid("too many render setups (maximum 32)".into()));
        }
        for (name, setup) in &self.setups {
            Self::validate_name(name)?;
            setup.validate()?;
            self.validate_resources(setup)?;
        }
        Ok(())
    }

    pub fn validate_resources(&self, setup: &RenderSetup) -> Result<()> {
        match &setup.job {
            Some(RenderJob::CpuRaytrace { options }) => {
                if let Some(hdr) = options.hdr {
                    hdr.validate_resource(&self.resources)?;
                }
            }
            Some(RenderJob::GpuPath { options, .. }) => {
                if let Some(hdr) = options.hdr {
                    hdr.validate_resource(&self.resources)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use std::sync::Arc;

    #[test]
    fn saved_light_coordinates_are_admitted_only_when_the_document_can_reopen() {
        use serde_json::json;
        for coordinate in [-1e9, 1e9] {
            let variants = [
                json!({"method":"cpu_raytrace","options":{"punctual_lights":[{
                    "position_mm":[coordinate,0.0,0.0],"color":[1.0,1.0,1.0],"intensity":1.0,
                    "reference_distance_mm":100.0,"minimum_distance_mm":0.001}]}}),
                json!({"method":"gpu_path","options":{"lighting":{"area_lights":[{
                    "center":[coordinate,0.0,0.0],"u":[1.0,0.0,0.0],"v":[0.0,1.0,0.0],"radiance":[1.0,1.0,1.0]}]}}}),
                json!({"method":"cpu_raytrace","options":{"lights":[{
                    "direction":[coordinate,0.0,0.0],"color":[1.0,1.0,1.0],"intensity":1.0}]}}),
            ];
            for job in variants {
                let settings: RenderSettings = serde_json::from_value(json!({"setups":{"Bounds":{"job":job}}})).unwrap();
                settings.validate().unwrap();
                let document = Document { render: Arc::new(settings), ..Default::default() };
                assert_eq!(Document::from_json(&document.to_json()).unwrap(), document);
            }
        }
        for coordinate in [-5e9, 5e9, 1e9_f64.next_up()] {
            let mut point: solidcraft_render::job::PunctualLightOptions = serde_json::from_value(json!({
                "position_mm":[coordinate,0.0,0.0],"color":[1.0,1.0,1.0],"intensity":1.0,
                "reference_distance_mm":100.0,"minimum_distance_mm":0.001}))
            .unwrap();
            assert!(point.validate().is_err());
            point.position_mm = [0.0; 3];
            point.cone = Some(solidcraft_render::job::SpotConeOptions {
                emitted_axis: [coordinate, 0.0, 0.0],
                inner_half_angle_radians: 0.1,
                outer_half_angle_radians: 0.5,
            });
            assert!(point.validate().is_err());
            let area =
                solidcraft_render::job::AreaLight { center: [coordinate, 0.0, 0.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], radiance: [1.0; 3] };
            assert!(area.validate().is_err());
            let frame = solidcraft_render::job::textures::PlanarFrame { origin: [coordinate, 0.0, 0.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
            assert!(frame.validate().is_err());
        }
    }

    fn hdr_fixture(pixel: u8) -> Vec<u8> {
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            bytes.extend_from_slice(&[pixel, 64, 32, 129]);
        }
        bytes
    }

    #[test]
    fn embedded_hdr_resources_roundtrip_share_bytes_and_keep_numerical_intent() {
        use crate::render_resources::{CpuHdrEnvironment, GpuHdrEnvironment, HdrResource};
        let resource = HdrResource::new(hdr_fixture(128)).unwrap();
        let id = resource.id();
        let mut settings = RenderSettings::default();
        settings.resources.insert(resource).unwrap();
        settings.setups.insert(
            "CPU".into(),
            RenderSetup {
                exposure_ev: 1.25,
                job: Some(RenderJob::CpuRaytrace {
                    options: CpuJob {
                        lights: Some(Vec::new()),
                        hdr: Some(CpuHdrEnvironment { resource: id, rotation_rad: 0.75, intensity: 2.0, samples: 64 }),
                        ..Default::default()
                    },
                }),
                ..Default::default()
            },
        );
        settings.setups.insert(
            "GPU".into(),
            RenderSetup {
                exposure_ev: -2.0,
                job: Some(RenderJob::GpuPath {
                    transparent_background: true,
                    options: Box::new(GpuJob {
                        samples: 37,
                        hdr: Some(GpuHdrEnvironment { resource: id, rotation_rad: -0.5, intensity: 3.0, assume_linear_srgb: false }),
                        ..Default::default()
                    }),
                }),
                ..Default::default()
            },
        );
        settings.validate().unwrap();
        let cloned = settings.clone();
        assert!(std::ptr::eq(settings.resources.required(&id).unwrap().bytes().as_ptr(), cloned.resources.required(&id).unwrap().bytes().as_ptr()));
        let document = Document { render: Arc::new(settings), ..Default::default() };
        let reopened = Document::from_json(&document.to_json()).unwrap();
        assert_eq!(document, reopened);
        assert_eq!(reopened.render.resources.len(), 1);
        let legacy: RenderSettings = serde_json::from_str(r#"{"setups":{"Old":{}}}"#).unwrap();
        assert!(legacy.resources.is_empty());
        assert!(legacy.setups["Old"].job.is_none());
        let mut missing = serde_json::to_value(&document).unwrap();
        missing["render"]["resources"] = serde_json::json!({});
        assert!(Document::from_json(&missing.to_string()).unwrap_err().to_string().contains("missing embedded HDR"));
    }

    #[test]
    fn embedded_hdr_parse_rejects_hash_type_paths_noncanonical_payload_and_counts() {
        use crate::render_resources::{HdrResource, HdrResources, MAX_HDR_BYTES};
        use serde_json::json;
        let source = HdrResource::new(hdr_fixture(128)).unwrap();
        let id = source.id().to_string();
        let valid = serde_json::to_value(&source).unwrap();
        for (key, value) in [
            ("kind", json!("png")),
            ("data", json!("")),
            ("data", json!(format!("{}\n", source.base64().unwrap()))),
            ("data", json!("AA==")),
            ("path", json!("original.hdr")),
            ("hdr_blob", json!("blobs/not-in-plain-json.hdr")),
        ] {
            let mut resource = valid.clone();
            resource[key] = value;
            assert!(serde_json::from_value::<HdrResources>(json!({(id.clone()):resource})).is_err(), "{key}");
        }
        assert!(serde_json::from_value::<HdrResources>(json!({("0".repeat(64)):valid})).unwrap_err().to_string().contains("hash mismatch"));
        let duplicate = format!("{{\"{id}\":{valid},\"{id}\":{valid}}}");
        assert!(serde_json::from_str::<HdrResources>(&duplicate).unwrap_err().to_string().contains("duplicate"));
        let duplicate_document = format!("{{\"render\":{{\"resources\":{duplicate}}}}}");
        assert!(Document::from_json(&duplicate_document).unwrap_err().to_string().contains("duplicate"));
        let mut many = serde_json::Map::new();
        for pixel in 0..64 {
            let resource = HdrResource::new(hdr_fixture(pixel)).unwrap();
            many.insert(resource.id().to_string(), json!(resource));
        }
        assert!(serde_json::from_value::<HdrResources>(serde_json::Value::Object(many.clone())).is_ok());
        many.insert("f".repeat(64), json!({"kind":"radiance_hdr","data":"invalid"}));
        assert!(serde_json::from_value::<HdrResources>(serde_json::Value::Object(many)).unwrap_err().to_string().contains("too many"));
        let oversized = json!({(id):{"kind":"radiance_hdr","data":"A".repeat(MAX_HDR_BYTES.div_ceil(3)*4+4)}});
        assert!(Document::from_json(&json!({"render":{"resources":oversized}}).to_string()).unwrap_err().to_string().contains("budget"));
        assert!(serde_json::from_value::<HdrResources>(oversized).unwrap_err().to_string().contains("budget"));
    }

    #[test]
    fn embedded_hdr_parser_debits_aggregate_before_decoding_the_next_source() {
        use crate::render_resources::{HdrId, HdrResources, MAX_HDR_BYTES, encode_hdr_base64};
        use serde::de::value::{Error, MapDeserializer};
        // One borrowed base64 buffer supplies distinct valid prefixes: this exercises the
        // real aggregate visitor without constructing a >100 MiB JSON value/string tree.
        let mut bytes = hdr_fixture(128);
        bytes.resize(MAX_HDR_BYTES - 1, 0);
        let encoded = encode_hdr_base64(&bytes).unwrap();
        assert!(!encoded.ends_with('='));
        let sources: Vec<_> = (0..5)
            .map(|i| {
                let length = bytes.len() - 3 * i;
                (HdrId::of(&bytes[..length]).to_string(), &encoded[..encoded.len() - 4 * i])
            })
            .collect();
        let entries =
            sources.iter().map(|(id, data)| (id.as_str(), MapDeserializer::<_, Error>::new([("kind", "radiance_hdr"), ("data", *data)].into_iter())));
        let error = HdrResources::deserialize(MapDeserializer::<_, Error>::new(entries)).unwrap_err().to_string();
        assert!(error.contains("budget"), "{error}");
    }

    #[test]
    fn embedded_hdr_json_probe_bounds_escaped_base64_before_value_or_string_materialization() {
        use crate::render_resources::{HdrResource, MAX_HDR_BYTES};
        let source = HdrResource::new(hdr_fixture(128)).unwrap();
        let id = source.id().to_string();
        let encoded = source.base64().unwrap();
        let escaped = format!("\\u{:04x}{}", encoded.as_bytes()[0], &encoded[1..]);
        let mut document = serde_json::to_value(Document::default()).unwrap();
        document["render"] = serde_json::json!({"setups":{},"resources":{(id.clone()):source}});
        let wire = document.to_string().replacen(&encoded, &escaped, 1);
        assert_eq!(Document::from_json(&wire).unwrap().render.resources.len(), 1, "legal bounded JSON escapes retain the same payload");
        // One escape makes deserialize_str require owned scratch. The borrowed RawValue
        // admission scans this oversized logical base64 without that unescaped allocation.
        let tail = "A".repeat(MAX_HDR_BYTES.div_ceil(3) * 4);
        let oversized = format!("{{\"render\":{{\"resources\":{{\"{id}\":{{\"kind\":\"radiance_hdr\",\"data\":\"\\u0041{tail}\"}}}}}}}}");
        assert!(Document::from_json(&oversized).unwrap_err().to_string().contains("budget"));
        let legacy: serde_json::Value = serde_json::from_str("{\"setups\":{}}").unwrap();
        assert!(serde_json::from_value::<RenderSettings>(legacy).is_ok());
        assert!(serde_json::from_value::<RenderSettings>(serde_json::Value::Null).is_err());
        assert!(Document::from_json("{\"render\":null}").is_err());
    }

    #[test]
    fn embedded_hdr_references_enforce_method_color_and_environment_controls() {
        use crate::render_resources::{CpuHdrEnvironment, GpuHdrEnvironment, HdrResource, HdrResources};
        let declared = hdr_fixture(128);
        let mut undeclared = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 2 +X 4\n".to_vec();
        undeclared.extend_from_slice(&declared[declared.len() - 32..]);
        let mut resources = HdrResources::default();
        let id = resources.insert(HdrResource::new(undeclared).unwrap()).unwrap();
        let mut cpu = CpuHdrEnvironment { resource: id, rotation_rad: 0.0, intensity: 1.0, samples: 16 };
        assert!(cpu.validate_resource(&resources).unwrap_err().to_string().contains("primaries"));
        let mut gpu = GpuHdrEnvironment { resource: id, rotation_rad: 0.0, intensity: 1.0, assume_linear_srgb: false };
        assert!(gpu.validate_resource(&resources).is_err());
        gpu.assume_linear_srgb = true;
        gpu.validate_resource(&resources).unwrap();
        cpu.resource = resources.insert(HdrResource::new(declared).unwrap()).unwrap();
        cpu.validate_resource(&resources).unwrap();
        for samples in [0, 3, 65] {
            cpu.samples = samples;
            assert!(cpu.validate().is_err());
        }
        cpu.samples = 16;
        cpu.intensity = 65.0;
        assert!(cpu.validate().is_err());
        gpu.intensity = 100.0;
        gpu.validate().unwrap();
        gpu.rotation_rad = f64::NAN;
        assert!(gpu.validate().is_err());
        let cpu_job = CpuJob {
            hdr: Some(CpuHdrEnvironment { intensity: 1.0, ..cpu }),
            environment: Some(solidcraft_render::job::RaytraceEnvironmentOptions { color: [0.0; 3], intensity: 0.0 }),
            ..Default::default()
        };
        assert!(cpu_job.to_runtime(0.0).is_err());
        let gpu_job = GpuJob {
            hdr: Some(GpuHdrEnvironment { rotation_rad: 0.0, ..gpu }),
            environment: Some(SkyEnvironment::Sky { gradient: Default::default(), rotation_rad: 0.0, intensity: 1.0 }),
            ..Default::default()
        };
        assert!(gpu_job.to_runtime(0.0).is_err());
    }

    #[test]
    fn camera_fields_are_strict_without_changing_existing_camera_defaults() {
        let mut camera = serde_json::to_value(Camera::default()).unwrap();
        camera["fov_degrees"] = serde_json::json!(45);
        let saved = serde_json::json!({"setups":{"Studio":{"camera":camera}}});
        let mut doc = serde_json::to_value(Document::default()).unwrap();
        doc["render"] = saved.clone();
        assert!(Document::from_json(&doc.to_string()).is_err());
        assert!(serde_json::from_value::<RenderSettings>(saved).is_err());
        camera.as_object_mut().unwrap().remove("fov_degrees");
        camera.as_object_mut().unwrap().remove("roll");
        camera.as_object_mut().unwrap().remove("up");
        let setup: RenderSetup = serde_json::from_value(serde_json::json!({"camera":camera})).unwrap();
        assert_eq!(setup.camera, Camera::default());
    }

    #[test]
    fn legacy_and_named_setups_round_trip_without_changing_other_data() {
        let old = Document::new("Legacy");
        let mut json = serde_json::to_value(&old).unwrap();
        json["format"] = serde_json::json!("solidcraft/16");
        json.as_object_mut().unwrap().remove("render");
        let mut loaded = Document::from_json(&json.to_string()).unwrap();
        assert!(loaded.render.is_empty());
        assert_eq!(loaded.format, crate::format::format_string(crate::format::FORMAT));
        // Upstream owns 19 for editable offsets; saved render intent needs 20.
        for version in [17, 18, 19] {
            json["format"] = serde_json::json!(crate::format::format_string(version));
            assert_eq!(Document::from_json(&json.to_string()).unwrap(), loaded);
        }
        let before = loaded.clone();
        let setup = RenderSetup {
            backend: serde_json::from_value(serde_json::json!("future-backend")).unwrap(),
            exposure_ev: 2.5,
            mode: RenderMode::Final,
            format: RenderFormat::Exr,
            camera: Camera { up: solidcraft_render::UpAxis::Y, ..Default::default() },
            ..Default::default()
        };
        Arc::make_mut(&mut loaded.render).setups.insert("Studio".into(), setup);
        assert!(loaded.differs_only_in_render(&before));
        assert_eq!(Document::from_json(&loaded.to_json()).unwrap(), loaded);
        let mut previous_draft = serde_json::to_value(&loaded).unwrap();
        previous_draft["format"] = serde_json::json!("solidcraft/17");
        assert_eq!(Document::from_json(&previous_draft.to_string()).unwrap(), loaded);
        loaded.units = "in".into();
        assert!(!loaded.differs_only_in_render(&before));
    }

    #[test]
    fn mixed_document_changes_never_qualify_as_render_only() {
        let before = Document::default();
        let mut render_only = before.clone();
        Arc::make_mut(&mut render_only.render).setups.insert("Studio".into(), RenderSetup::default());
        assert!(render_only.differs_only_in_render(&before));
        for (key, value) in [
            ("marker", serde_json::json!(0)),
            ("materials", serde_json::json!({"Body1":"Steel"})),
            ("visibility", serde_json::json!({"hidden_bodies":["Body1"]})),
            ("body_components", serde_json::json!({"Body1":1})),
            ("next_id", serde_json::json!(99)),
        ] {
            let mut json = serde_json::to_value(&render_only).unwrap();
            json[key] = value;
            let mixed: Document = serde_json::from_value(json).unwrap();
            assert!(!mixed.differs_only_in_render(&before), "{key}");
        }
    }

    #[test]
    fn oversized_output_names_the_limits_and_the_largest_size() {
        let setup = RenderSetup { width: 4096, height: 4096, ..Default::default() };
        let message = setup.validate().unwrap_err().to_string();
        assert!(message.contains("4096 × 4096 exceeds the image limits (4096 pixels per side, 4194304 pixels in all)"), "{message}");
        assert!(message.contains("the largest image at this aspect is 2048 × 2048"), "{message}");
        let setup = RenderSetup { width: 7680, height: 4320, ..Default::default() };
        let message = setup.validate().unwrap_err().to_string();
        assert!(message.contains("the largest image at this aspect is 27"), "{message}");
        assert!(RenderSetup { width: 2048, height: 2048, ..Default::default() }.validate().is_ok());
    }

    #[test]
    fn portable_jobs_preserve_complete_numerical_intent_and_legacy_omission() {
        use serde_json::json;
        let camera = Camera { fov: 0.75, ..Default::default() };
        let wire = json!({"setups":{
            "Legacy":{"exposure_ev":2,"environment_rotation":0.75,"environment_intensity":3},
            "CPU":{"exposure_ev":1.25,"job":{"method":"cpu_raytrace","options":{
                "primary_samples":4,"lights":[],"shadows":false,
                "punctual_lights":[{"position_mm":[1,2,30],"color":[1,0.2,0.5],"intensity":2,"reference_distance_mm":50,"minimum_distance_mm":0.1,
                    "cone":{"emitted_axis":[0,0,-1],"inner_half_angle_radians":0.1,"outer_half_angle_radians":0.5}}],
                "area_lights":[{"center_mm":[3,4,50],"edge_u_mm":[20,0,0],"edge_v_mm":[0,-10,0],"color":[0.2,0.4,1],"intensity":3}],
                "area_samples":64,"seed":u32::MAX,"environment":{"color":[0.1,0.2,0.3],"intensity":0.5}
            }}},
            "GPU":{"camera":camera,"exposure_ev":-3.5,"job":{"method":"gpu_path","transparent_background":true,"options":{
                "samples":4096,"max_depth":16,"seed":81,"tone_map":"aces_fitted","denoise":true,
                "lens":{"aperture_radius_mm":0.4,"focus_distance_mm":90},
                "lighting":{"environment":[0.1,0.25,0.6],"sun":{"direction":[1,-1,-2],"angular_radius_rad":0.1,"radiance":[5,4,3]},
                    "area_lights":[{"center":[10,20,30],"u":[4,0,0],"v":[0,-2,0],"radiance":[3,2,1]}]},
                "environment":{"kind":"sky","rotation_rad":0.3,"intensity":2,"gradient":{"zenith":[0.1,0.2,0.3],"horizon":[0.4,0.5,0.6],"ground":[0.7,0.8,0.9]}}
            }}}
        }});
        let settings: RenderSettings = serde_json::from_value(wire).unwrap();
        let document = Document { render: Arc::new(settings.clone()), ..Default::default() };
        let reopened = Document::from_json(&document.to_json()).unwrap();
        assert_eq!(reopened, document);
        let saved = serde_json::to_value(&reopened.render).unwrap();
        assert!(saved["setups"]["Legacy"].get("job").is_none());
        assert_eq!(saved["setups"]["Legacy"]["environment_rotation"], 0.75);
        let RenderJob::CpuRaytrace { options } = settings.setups["CPU"].job.as_ref().unwrap() else { panic!("CPU intent changed") };
        let runtime = options.to_runtime(settings.setups["CPU"].exposure_ev).unwrap();
        assert_eq!(runtime.exposure_ev, 1.25);
        assert_eq!(runtime.area_samples, Some(64));
        assert_eq!(runtime.seed, Some(u32::MAX));
        assert!(runtime.lights.as_ref().unwrap().is_empty());
        assert!(runtime.punctual_lights.as_ref().unwrap()[0].cone.is_some());
        let RenderJob::GpuPath { options, transparent_background } = settings.setups["GPU"].job.as_ref().unwrap() else {
            panic!("GPU intent changed")
        };
        assert!(*transparent_background);
        let runtime = options.to_runtime(settings.setups["GPU"].exposure_ev).unwrap();
        assert_eq!(runtime.exposure_ev, -3.5);
        assert_eq!(runtime.lighting.unwrap().environment, [0.1, 0.25, 0.6]);
        assert!(runtime.denoise);
        assert!(saved["setups"]["GPU"]["job"]["options"].get("exposure_ev").is_none());
    }

    #[test]
    fn portable_job_options_keep_null_distinct_from_explicit_empty_or_black() {
        use serde_json::json;
        for options in [json!({}), json!({"lights":null,"punctual_lights":null,"area_lights":null,"environment":null})] {
            let setup: RenderSetup = serde_json::from_value(json!({"job":{"method":"cpu_raytrace","options":options}})).unwrap();
            setup.validate().unwrap();
            assert_eq!(setup.job, Some(RenderJob::CpuRaytrace { options: CpuJob::default() }));
        }
        let empty: RenderSetup = serde_json::from_value(json!({"job":{"method":"cpu_raytrace","options":{"lights":[]}}})).unwrap();
        empty.validate().unwrap();
        assert_ne!(empty.job, Some(RenderJob::CpuRaytrace { options: CpuJob::default() }));
        for (lighting, explicit) in [(serde_json::Value::Null, false), (json!({"environment":[0,0,0],"area_lights":[]}), true)] {
            let setup: RenderSetup = serde_json::from_value(json!({"job":{"method":"gpu_path","options":{"lighting":lighting}}})).unwrap();
            setup.validate().unwrap();
            let RenderJob::GpuPath { options, .. } = setup.job.unwrap() else { panic!("GPU intent changed") };
            assert_eq!(options.lighting.is_some(), explicit);
        }
        let legacy: RenderSetup = serde_json::from_value(json!({"job":null,"exposure_ev":22})).unwrap();
        legacy.validate().unwrap();
        assert!(legacy.job.is_none());
    }

    #[test]
    fn progressive_cpu_intent_roundtrips_offline_and_validates_the_image_work_bound() {
        use serde_json::json;
        let saved = json!({"setups":{"Film":{"width":512,"height":512,"exposure_ev":1.5,
            "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":64,"seed":u32::MAX},"lights":[]}}
        }}});
        let settings: RenderSettings = serde_json::from_value(saved.clone()).unwrap();
        let document = Document { render: Arc::new(settings), ..Default::default() };
        let reopened = Document::from_json(&document.to_json()).unwrap();
        assert_eq!(document, reopened);
        let RenderJob::CpuRaytrace { options } = reopened.render.setups["Film"].job.as_ref().unwrap() else { panic!("method changed") };
        let runtime = options.to_runtime(1.5).unwrap();
        assert_eq!(runtime.progressive.unwrap().seed, u32::MAX);
        assert_eq!(runtime.exposure_ev, 1.5);
        assert_eq!(runtime.primary_samples, 1);
        for (width, height, samples) in [(1025, 1024, 1), (1024, 1024, 17), (512, 512, 65), (16, 16, 0)] {
            let mut invalid = saved.clone();
            invalid["setups"]["Film"]["width"] = json!(width);
            invalid["setups"]["Film"]["height"] = json!(height);
            invalid["setups"]["Film"]["job"]["options"]["progressive"]["samples_per_pixel"] = json!(samples);
            assert!(serde_json::from_value::<RenderSettings>(invalid).is_err());
        }
        let mut incompatible = saved;
        incompatible["setups"]["Film"]["job"]["options"]["primary_samples"] = json!(4);
        assert!(serde_json::from_value::<RenderSettings>(incompatible).is_err());
    }

    #[test]
    fn portable_jobs_reject_unbounded_invalid_and_nonportable_inputs() {
        use serde_json::json;
        for setup in [
            json!({"job":{}}),
            json!({"job":{"method":"cpu_raytrace","options":{"primary_samples":2}}}),
            json!({"exposure_ev":17,"job":{"method":"cpu_raytrace"}}),
            json!({"exposure_ev":1,"job":{"method":"raster"}}),
            json!({"environment_rotation":0.5,"job":{"method":"gpu_path"}}),
            json!({"environment_intensity":0,"job":{"method":"cpu_raytrace"}}),
            json!({"job":{"method":"cpu_raytrace","options":{"seed":-1}}}),
            json!({"job":{"method":"cpu_raytrace","options":{"preset":"neutral","lights":[]}}}),
            json!({"job":{"method":"cpu_raytrace","options":{"environment":{"path":"not-portable.hdr"}}}}),
            json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"hdr","path":"not-portable.hdr"}}}}),
            json!({"job":{"method":"gpu_path","options":{"exposure_ev":0}}}),
            json!({"job":{"method":"gpu_path","options":{"samples":0}}}),
            json!({"job":{"method":"gpu_path","options":{"max_depth":17}}}),
            json!({"job":{"method":"gpu_path","options":{"environment":{"kind":"sky","intensity":101}}}}),
            json!({"job":{"method":"gpu_path","options":{"lighting":{"environment":[-1,0,0]}}}}),
            json!({"camera":Camera { fov: 0.0, ..Default::default() },"job":{"method":"gpu_path","options":{"lens":{"aperture_radius_mm":1,"focus_distance_mm":10}}}}),
        ] {
            let saved = json!({"setups":{"Bad":setup}});
            assert!(serde_json::from_value::<RenderSettings>(saved.clone()).is_err(), "{saved}");
        }
        for (method, fields) in [
            ("cpu_raytrace", &["reflections", "anisotropic_faces", "surface_effects", "emissions", "dielectrics"][..]),
            ("gpu_path", &["textures", "clear_coats", "dielectrics"][..]),
        ] {
            for field in fields {
                let mut options = json!({});
                options[*field] = json!([]);
                let saved = json!({"setups":{"Bad":{"job":{"method":method,"options":options}}}});
                assert!(serde_json::from_value::<RenderSettings>(saved).is_err(), "{method}.{field}");
            }
        }
        for (field, light, maximum) in [
            ("lights", json!({"direction":[0,0,-1],"color":[1,1,1],"intensity":1}), 8),
            (
                "punctual_lights",
                json!({"position_mm":[0,0,10],"color":[1,1,1],"intensity":1,"reference_distance_mm":10,"minimum_distance_mm":0.1}),
                8,
            ),
            ("area_lights", json!({"center_mm":[0,0,10],"edge_u_mm":[1,0,0],"edge_v_mm":[0,-1,0],"color":[1,1,1],"intensity":1}), 4),
        ] {
            let mut options = json!({});
            options[field] = json!(vec![light; maximum + 1]);
            let error = serde_json::from_value::<CpuJob>(options).unwrap_err().to_string();
            assert!(error.contains(&format!("at most {maximum} saved lights")), "{error}");
        }
        let area = json!({"center":[0,0,10],"u":[1,0,0],"v":[0,-1,0],"radiance":[1,1,1]});
        assert!(serde_json::from_value::<GpuJob>(json!({"lighting":{"area_lights":vec![area;5]}})).is_err());
        let mut cpu = CpuJob { lights: Some(Vec::with_capacity(9)), ..Default::default() };
        assert!(cpu.to_runtime(0.0).is_err());
        cpu.lights = None;
        assert!(cpu.to_runtime(f64::NAN).is_err());
        let gpu = GpuJob {
            environment: Some(SkyEnvironment::Sky { gradient: Default::default(), rotation_rad: f64::INFINITY, intensity: 1.0 }),
            ..Default::default()
        };
        assert!(gpu.to_runtime(0.0).is_err());
    }

    #[test]
    fn invalid_saved_intent_is_rejected_at_deserialization() {
        for patch in [
            serde_json::json!({"width":0}),
            serde_json::json!({"width":4097}),
            serde_json::json!({"width":4096,"height":4096}),
            serde_json::json!({"exposure_ev":33}),
            serde_json::json!({"environment_rotation":1000001}),
            serde_json::json!({"environment_intensity":-1}),
            serde_json::json!({"background":[256,0,0]}),
            serde_json::json!({"unknown":1}),
        ] {
            let json = serde_json::json!({"setups":{"Studio":patch}});
            assert!(serde_json::from_value::<RenderSettings>(json).is_err());
        }
        for name in ["", " padded", "line\nfeed"] {
            assert!(RenderSettings::validate_name(name).is_err());
        }
        let mut settings = RenderSettings::default();
        for i in 0..=MAX_RENDER_SETUPS {
            settings.setups.insert(i.to_string(), RenderSetup::default());
        }
        assert!(serde_json::from_value::<RenderSettings>(serde_json::to_value(settings).unwrap()).is_err());
        let mut setup = RenderSetup::default();
        setup.camera.fov = f64::NAN;
        assert!(setup.validate().is_err());
        for field in ["yaw", "pitch", "roll", "distance", "fov"] {
            let mut value = serde_json::to_value(RenderSetup::default()).unwrap();
            value["camera"][field] = serde_json::json!(1e12);
            let setup: RenderSetup = serde_json::from_value(value).unwrap();
            assert!(setup.validate().is_err(), "{field}");
        }
        setup.camera = Camera::default();
        setup.environment_intensity = f64::INFINITY;
        assert!(setup.validate().is_err());
    }
}

#[cfg(test)]
mod cpu_budget_tests {
    use super::*;
    use solidcraft_render::job::{CpuBudget, CpuProgressiveOptions};

    #[test]
    fn inactive_area_tuning_roundtrips_without_leaking_into_runtime_options() {
        let saved: CpuJob = serde_json::from_str(r#"{"seed":1,"area_samples":16,"area_lights":[]}"#).unwrap();
        let runtime = saved.to_runtime(0.0).unwrap();
        assert_eq!((saved.seed, saved.area_samples), (Some(1), Some(16)));
        assert_eq!((runtime.seed, runtime.area_samples), (None, None));
        let roundtrip: CpuJob = serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
        assert_eq!(roundtrip, saved);
        let raw = solidcraft_render::job::RaytraceOptions { seed: Some(1), ..Default::default() };
        assert!(raw.validate().is_err(), "runtime requests still reject tuning without active emitters");
    }

    #[test]
    fn resource_profiles_survive_native_intent_roundtrips_without_a_host() {
        let mut setup = RenderSetup {
            width: 3840,
            height: 2160,
            job: Some(RenderJob::CpuRaytrace {
                options: CpuJob {
                    budget: CpuBudget::Studio,
                    progressive: Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 82 }),
                    ..Default::default()
                },
            }),
            ..Default::default()
        };
        setup.validate().unwrap();
        let text = serde_json::to_string(&setup).unwrap();
        let reopened: RenderSetup = serde_json::from_str(&text).unwrap();
        assert_eq!(setup, reopened);
        let Some(RenderJob::CpuRaytrace { options }) = &mut setup.job else {
            panic!("CPU fixture");
        };
        options.budget = CpuBudget::Compact;
        assert!(setup.validate().is_err());
        let legacy: CpuJob = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.budget, CpuBudget::Compact);
        assert!(serde_json::to_value(legacy).unwrap().get("budget").is_none());
        assert!(serde_json::from_str::<CpuJob>(r#"{"budget":"unlimited"}"#).is_err());
        let legacy_large = RenderSetup { width: 3840, height: 2160, ..Default::default() };
        assert!(legacy_large.validate().is_err(), "large intent must name an explicit CPU job profile");
    }
}
