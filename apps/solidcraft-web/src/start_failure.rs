//! What the page says when SolidCraft cannot start in the browser.
//!
//! The usual reason on an older PC is graphics: the browser offers neither WebGPU nor WebGL2,
//! often because it has turned them off for an old graphics card or driver. The page then says
//! what the person can change in the browser instead of showing only the technical error.

use solidcraft_ui_egui::i18n;

/// The words the loading overlay shows instead of the logo.
#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    /// What happened, in one sentence.
    pub why: String,
    /// Things to try, in order.
    pub steps: Vec<String>,
    /// The error as the browser and wgpu gave it, behind a "Technical details" fold.
    pub details_label: String,
    pub details: String,
    pub retry: String,
}

/// The notice for `error`. `webgl2` and `webgpu` say whether a fresh canvas offers WebGL2 and
/// whether the browser has `navigator.gpu`. Graphics is the cause when neither is there, or when
/// the error comes from setting up the graphics device (an adapter or a limit it refuses).
pub fn notice(error: &str, webgl2: bool, webgpu: bool) -> Notice {
    let graphics = !(webgl2 || webgpu) || is_graphics_error(error);
    let (title, why, steps) = if graphics {
        (
            i18n::tr("web.no_graphics_title"),
            i18n::tr("web.no_graphics_why"),
            ["web.no_graphics_acceleration", "web.no_graphics_update", "web.no_graphics_chrome", "web.no_graphics_firefox"]
                .iter()
                .map(|id| i18n::tr(id).to_owned())
                .collect(),
        )
    } else {
        (i18n::tr("web.start_failed_title"), i18n::tr("web.start_failed_why"), Vec::new())
    };
    Notice {
        title: title.to_owned(),
        why: why.to_owned(),
        steps,
        details_label: i18n::tr("web.start_failed_details").to_owned(),
        details: error.to_owned(),
        retry: i18n::tr("web.start_failed_retry").to_owned(),
    }
}

/// An error from creating the wgpu surface, adapter or device.
fn is_graphics_error(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    ["wgpu", "webgl", "webgpu", "adapter", "surface"].iter().any(|word| e.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_webgl2_or_webgpu_says_what_to_try() {
        let error = r#"failed to create wgpu surface: Failed to create surface for any enabled backend: {Gl: InstanceError { message: "canvas.getContext() returned null; webgl2 not available or canvas already in use", source: None }}"#;
        let n = notice(error, false, true);
        assert_eq!(n.title, "SolidCraft can't draw in this browser");
        assert_eq!(n.steps.len(), 4);
        assert!(n.steps[0].contains("graphics acceleration"), "{:?}", n.steps);
        assert!(n.steps.iter().any(|s| s.contains("chrome://flags")));
        assert!(n.steps.iter().any(|s| s.contains("webgl.force-enabled")));
        assert_eq!(n.details, error, "the technical error is still there to copy into a bug report");
    }

    #[test]
    fn a_refused_graphics_device_is_graphics_too() {
        // WebGL2 exists, but the adapter refused the device.
        assert_eq!(notice("WgpuError: RequestDeviceError", true, false).title, "SolidCraft can't draw in this browser");
        assert_eq!(notice("No suitable adapter found", true, true).steps.len(), 4);
    }

    #[test]
    fn other_failures_keep_a_plain_message() {
        let n = notice("SolidCraft browser API: JsValue(TypeError)", true, true);
        assert_eq!(n.title, "SolidCraft failed to start");
        assert!(n.steps.is_empty());
        assert_eq!(n.details, "SolidCraft browser API: JsValue(TypeError)");
    }
}
