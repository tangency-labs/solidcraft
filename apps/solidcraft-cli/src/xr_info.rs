//! `xr-info`: detect the OpenXR runtime and the VR hardware.
//!
//! Phase 0 of the OpenXR integration (#2172): a diagnostic probe that enumerates
//! the OpenXR runtime, the HMD and its view configuration, and checks Vulkan
//! instance/device compatibility. Reports graceful errors when nothing is found.

use serde_json::{json, Value};

/// Run `xr-info` and print the diagnostic report as JSON.
pub fn cmd(args: &[String]) -> Result<(), String> {
    let _ = args;
    // Phase 0: skeleton. The probe will call through `openxr` to:
    //   1. Load the runtime (Entry::linked or Entry::load).
    //   2. Enumerate API layers and extensions.
    //   3. Enumerate instances and systems (HMDs).
    //   4. Query view configuration and swapchain formats.
    //   5. Check Vulkan instance/device compatibility (get_vulkan_instance/device_extensions
    //      + get_vulkan_graphics_requirements).
    //   6. Report each step or a clear, actionable error.
    println!(
        "{}",
        pretty(&json!({
            "openxr": {
                "runtime": null,
                "error": "not implemented yet — Phase 0 of #2172",
            },
        }))
    );
    Ok(())
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
