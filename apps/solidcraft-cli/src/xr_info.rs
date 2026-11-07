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

/// Run `xr-info`: print the JSON report, or `--summary` for a plain-language
/// verdict. Every failure is inside the report — the exit code is always 0.
pub fn cmd(args: &[String]) -> Result<(), String> {
    let report = probe();
    if args.iter().any(|a| a == "--summary") {
        println!("{}", summary(&report));
    } else {
        println!("{}", pretty(&report));
    }
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
            // The loader is a per-application DLL on Windows, not a system component:
            // this failure means the probe was moved away from its own files.
            report["openxr"]["error_stage"] = json!("loader");
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
            // The loader found no registered runtime: the VR software is not installed
            // or not running, which the person testing can act on.
            report["openxr"]["error_stage"] = json!("runtime");
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

/// A plain-language version of the report, ending in a pass/fail verdict.
///
/// For someone running the probe on a headset they own: JSON tells the project
/// what a Phase 1 renderer must handle, but it does not tell the person testing
/// whether it worked. Pure over the report, so every verdict is testable without
/// VR hardware (see `tests`).
fn summary(report: &Value) -> String {
    let mut out = String::from("SolidCraft VR check\n===================\n\n");

    let runtime_name = str_at(report, &["openxr", "runtime", "runtime_name"]);
    let runtime_version = str_at(report, &["openxr", "runtime", "runtime_version"]);
    let runtime = match (runtime_name, runtime_version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n.to_string(),
        _ => "not found".to_string(),
    };
    out.push_str(&format!("OpenXR runtime ..... {runtime}\n"));

    if runtime_name.is_none() {
        out.push_str("\nRESULT: FAIL\n");
        // A missing loader is our packaging problem, not the tester's setup: say so,
        // so nobody goes hunting for VR software that is already running.
        if str_at(report, &["openxr", "error_stage"]) == Some("loader") {
            out.push_str("The OpenXR loader file is missing. That is a packaging problem on our\n");
            out.push_str("side, not something wrong with your PC.\n");
            out.push_str("Unzip the whole folder and run VR-check.cmd from inside it.\n");
        } else {
            out.push_str("No VR software is running on this PC.\n");
            out.push_str("Start SteamVR (or Quest Link, or Windows Mixed Reality), put the\n");
            out.push_str("headset on, then run the check again.\n");
        }
        if let Some(e) = str_at(report, &["openxr", "error"]) {
            out.push_str(&format!("\nDetail: {e}\n"));
        }
        return out;
    }

    let headset = headset_name(report);
    out.push_str(&format!("Headset ............ {}\n", headset.unwrap_or("not found")));

    if let Some((w, h)) = stereo_eye_size(report) {
        out.push_str(&format!("Eye resolution ..... {w} x {h} per eye\n"));
    }
    let vulkan = report["openxr"]["graphics"]["vulkan_supported"].as_bool();
    if let Some(v) = vulkan {
        let word = if v { "yes" } else { "no" };
        out.push_str(&format!("Vulkan support ..... {word}\n"));
    }

    out.push('\n');
    if headset.is_none() {
        out.push_str("RESULT: FAIL\n");
        out.push_str("The VR software is running, but no headset was found.\n");
        out.push_str("Check that the headset is plugged in and switched on, then run the\n");
        out.push_str("check again.\n");
    } else if vulkan == Some(false) {
        out.push_str("RESULT: PASS (with a note)\n");
        out.push_str("Your setup was read successfully, but Vulkan is not available here.\n");
        out.push_str("That is still useful to know - please share the report with the project.\n");
    } else {
        out.push_str("RESULT: PASS\n");
        out.push_str("Everything we need was reported. Please share the report with the project.\n");
    }
    out
}

/// The head-mounted display the runtime reports, if any.
fn headset_name(report: &Value) -> Option<&str> {
    report["openxr"]["systems"].as_array()?.first()?.get("system_name")?.as_str()
}

/// The recommended per-eye image size of the primary stereo view configuration.
fn stereo_eye_size(report: &Value) -> Option<(u64, u64)> {
    let stereo = report["openxr"]["view_configurations"]
        .as_array()?
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some("primary_stereo"))?;
    let view = stereo.get("views")?.as_array()?.first()?;
    Some((view.get("recommended_width")?.as_u64()?, view.get("recommended_height")?.as_u64()?))
}

/// Walk a path of object keys, returning the string at the end of it.
fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = value;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str()
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A report from a machine that has everything Phase 1 needs.
    fn working_setup() -> Value {
        json!({
            "openxr": {
                "runtime": { "runtime_name": "SteamVR", "runtime_version": "1.24.7" },
                "systems": [{ "system_name": "Valve Index" }],
                "view_configurations": [{
                    "type": "primary_stereo",
                    "views": [{ "recommended_width": 1440, "recommended_height": 1600 }],
                }],
                "graphics": { "vulkan_supported": true },
            }
        })
    }

    #[test]
    fn a_working_setup_passes() {
        let s = summary(&working_setup());
        assert!(s.contains("RESULT: PASS"), "{s}");
        assert!(s.contains("SteamVR 1.24.7"), "{s}");
        assert!(s.contains("Valve Index"), "{s}");
        assert!(s.contains("1440 x 1600 per eye"), "{s}");
        assert!(s.contains("Vulkan support ..... yes"), "{s}");
    }

    #[test]
    fn no_runtime_fails_and_says_what_to_do() {
        let s = summary(&json!({
            "openxr": {
                "runtime": Value::Null,
                "error": "failed to create instance: the loader was unable to find or load a runtime",
                "error_stage": "runtime",
            }
        }));
        assert!(s.contains("RESULT: FAIL"), "{s}");
        assert!(s.contains("No VR software is running"), "{s}");
        assert!(s.contains("Start SteamVR"), "{s}");
    }

    #[test]
    fn a_missing_loader_blames_the_package_not_the_tester() {
        let s = summary(&json!({
            "openxr": {
                "runtime": Value::Null,
                "error": "no OpenXR loader: failed loading",
                "error_stage": "loader",
            }
        }));
        assert!(s.contains("RESULT: FAIL"), "{s}");
        assert!(s.contains("packaging problem"), "{s}");
        assert!(!s.contains("No VR software is running"), "{s}");
    }

    #[test]
    fn runtime_without_a_headset_fails() {
        let s = summary(&json!({
            "openxr": {
                "runtime": { "runtime_name": "SteamVR", "runtime_version": "1.24.7" },
                "systems": [{ "error": "no head mounted display" }],
            }
        }));
        assert!(s.contains("RESULT: FAIL"), "{s}");
        assert!(s.contains("no headset was found"), "{s}");
    }

    #[test]
    fn no_vulkan_passes_with_a_note() {
        let mut report = working_setup();
        report["openxr"]["graphics"]["vulkan_supported"] = json!(false);
        let s = summary(&report);
        assert!(s.contains("RESULT: PASS (with a note)"), "{s}");
        assert!(s.contains("Vulkan support ..... no"), "{s}");
    }

    #[test]
    fn an_empty_report_still_prints_a_verdict() {
        let s = summary(&json!({}));
        assert!(s.contains("RESULT: FAIL"), "{s}");
    }
}
