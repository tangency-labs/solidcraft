//! `xr-info`: detect the OpenXR runtime and the VR hardware.
//!
//! Phase 0 of the OpenXR integration (#2172): a diagnostic probe that reads the
//! OpenXR runtime, the HMD and its view configuration, and reports what a Phase 1
//! stereo renderer would have to work with. Reports stored results when nothing
//! is found: no runtime, no headset and no Vulkan support are answers, not errors.
//!
//! The loader is opened at run time ([`load_entry`]) rather than compiled in. The
//! `static` feature would build the Khronos C++ loader through CMake on every
//! build, including the macOS release build in .github/workflows/release.yml,
//! for no benefit here: the OpenXR runtime on a VR machine already installs a
//! loader. `crates/threedx-client/src/macos.rs` makes the same trade for the same
//! reason -- the 3Dconnexion driver ships its own library -- and keeps its
//! `unsafe` to the declarations and the lookups, as this module does.

use serde_json::{Value, json};

/// Run `xr-info` and print the diagnostic report as JSON.
/// Every failure is inside the JSON — the exit code is always 0.
pub fn cmd(_args: &[String]) -> Result<(), String> {
    let report = probe();
    println!("{}", pretty(&report));
    Ok(())
}

/// Open the OpenXR loader the runtime installs, at run time.
///
/// Split out so the `unsafe` allowance is one function with one call, rather than
/// the whole module.
#[allow(unsafe_code)]
fn load_entry() -> Result<openxr::Entry, String> {
    // SAFETY: `Entry::load` loads the platform's OpenXR loader library
    // (`openxr_loader.dll` on Windows, `libopenxr_loader.so.1`/`.dylib` elsewhere)
    // from the dynamic loader's search path. The OpenXR specification requires
    // that library to implement the core entry points this program calls through,
    // and the runtime installer puts it there; a machine with no OpenXR runtime
    // simply fails the load, which the caller reports as "no runtime" instead of
    // trusting it. Nothing below this call assumes a loader is present.
    unsafe { openxr::Entry::load(&()) }.map_err(|e| format!("no OpenXR loader: {e}"))
}

/// Probe the OpenXR runtime and build a JSON diagnostic report.
/// Never fails — every error is recorded inside the JSON so the exit code is always 0.
fn probe() -> Value {
    let mut report = json!({
        "openxr": {
            "runtime": Value::Null,
            "api_layers": [],
            "instance_extensions": [],
        }
    });
    // -----------------------------------------------------------------------
    // 1. Open the OpenXR entry point.
    // -----------------------------------------------------------------------
    let entry = match load_entry() {
        Ok(e) => e,
        Err(msg) => {
            report["openxr"]["error"] = json!(msg);
            return report;
        }
    };

    // -----------------------------------------------------------------------
    // 2. Enumerate API layers and instance extensions.
    // -----------------------------------------------------------------------
    let layers = entry.enumerate_layers().unwrap_or_default();
    let mut layer_list: Vec<Value> = Vec::new();
    for l in &layers {
        layer_list.push(json!({
            "name": l.layer_name,
            "description": l.description,
            "spec_version": format!("{}", l.spec_version),
            "layer_version": l.layer_version,
        }));
    }
    report["openxr"]["api_layers"] = Value::Array(layer_list);

    let extensions = entry.enumerate_extensions().unwrap_or_default();
    let ext_names = extensions.names();
    let mut ext_list: Vec<Value> = Vec::new();
    for name in &ext_names {
        // `names()` gives nul-terminated byte strings.
        let text = String::from_utf8_lossy(name);
        ext_list.push(json!({
            "name": text.trim_end_matches('\0'),
        }));
    }
    report["openxr"]["instance_extensions"] = Value::Array(ext_list);

    // -----------------------------------------------------------------------
    // 3. Create an instance with a minimal extension set (no graphics API yet).
    // -----------------------------------------------------------------------
    let app_info = openxr::ApplicationInfo {
        application_name: "SolidCraft xr-info",
        application_version: 1,
        engine_name: "SolidCraft",
        engine_version: 1,
        api_version: openxr::Version::new(1, 0, 0),
    };
    let instance = match entry.create_instance(&app_info, &openxr::ExtensionSet::default(), &[], &()) {
        Ok(inst) => inst,
        Err(e) => {
            report["openxr"]["error"] = json!(format!("failed to create instance: {e}"));
            return report;
        }
    };

    let instance_props = match instance.properties() {
        Ok(p) => p,
        Err(e) => {
            report["openxr"]["error"] = json!(format!("failed to query instance properties: {e}"));
            return report;
        }
    };
    report["openxr"]["runtime"] = json!({
        "runtime_name": instance_props.runtime_name,
        "runtime_version": format!("{}", instance_props.runtime_version),
    });

    // -----------------------------------------------------------------------
    // 4. Enumerate systems (HMDs / VR headsets).
    // -----------------------------------------------------------------------
    let system = match instance.system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY) {
        Ok(sys) => sys,
        Err(e) => {
            report["openxr"]["systems"] = json!([
                {"form_factor": "head_mounted_display", "error": format!("{e}")}
            ]);
            return report;
        }
    };

    let system_props = match instance.system_properties(system) {
        Ok(p) => p,
        Err(e) => {
            report["openxr"]["systems"] = json!([
                {"form_factor": "head_mounted_display", "error": format!("system_properties failed: {e}")}
            ]);
            return report;
        }
    };
    report["openxr"]["systems"] = json!([
        {
            "system_id": format!("{:?}", system_props.system_id),
            "vendor_id": system_props.vendor_id,
            "system_name": system_props.system_name,
            "graphics_properties": {
                "max_swapchain_image_width": system_props.graphics_properties.max_swapchain_image_width,
                "max_swapchain_image_height": system_props.graphics_properties.max_swapchain_image_height,
                "max_layer_count": system_props.graphics_properties.max_layer_count,
            },
            "tracking_properties": {
                "orientation_tracking": system_props.tracking_properties.orientation_tracking,
                "position_tracking": system_props.tracking_properties.position_tracking,
            },
        }
    ]);

    // -----------------------------------------------------------------------
    // 5. Query view configurations.
    // -----------------------------------------------------------------------
    let view_configs = instance.enumerate_view_configurations(system).unwrap_or_default();
    let mut vc_list: Vec<Value> = Vec::new();
    for vc in &view_configs {
        let vci = match instance.enumerate_view_configuration_views(system, *vc) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let config_name = match *vc {
            openxr::ViewConfigurationType::PRIMARY_MONO => "primary_mono",
            openxr::ViewConfigurationType::PRIMARY_STEREO => "primary_stereo",
            openxr::ViewConfigurationType::PRIMARY_QUAD_VARJO => "primary_quad_varjo",
            openxr::ViewConfigurationType::SECONDARY_MONO_FIRST_PERSON_OBSERVER_MSFT => "secondary_mono_first_person_observer_msft",
            _ => "unknown",
        };
        let mut views: Vec<Value> = Vec::new();
        for view in &vci {
            views.push(json!({
                "recommended_width": view.recommended_image_rect_width,
                "recommended_height": view.recommended_image_rect_height,
                "max_width": view.max_image_rect_width,
                "max_height": view.max_image_rect_height,
                "recommended_swapchain_count": view.recommended_swapchain_sample_count,
                "max_swapchain_count": view.max_swapchain_sample_count,
            }));
        }
        vc_list.push(json!({
            "type": config_name,
            "views": views,
        }));
    }
    report["openxr"]["view_configurations"] = Value::Array(vc_list);

    // -----------------------------------------------------------------------
    // 6. Check Vulkan instance/device extension availability.
    // -----------------------------------------------------------------------
    let vulkan_ext_present = extensions.khr_vulkan_enable2 || extensions.khr_vulkan_enable;
    report["openxr"]["graphics"] = json!({
        "vulkan_supported": vulkan_ext_present,
        "note": if vulkan_ext_present {
            "XR_KHR_vulkan_enable2 available — wgpu can adopt the OpenXR instance via wgpu-hal"
        } else {
            "no XR_KHR_vulkan_enable(2) extension — wgpu cannot adopt the OpenXR instance"
        },
    });

    report
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
