//! `xr-plan`: the swapchain configuration a Phase 1 session would ask for.
//!
//! Phase 0 answers "can this machine run OpenXR at all". This answers the next
//! question a renderer asks: what size, how many samples and which format would we
//! request. It renders nothing and creates no session - it reads the same report
//! `xr-info` produces and turns it into the numbers a swapchain would be created
//! with, so the choice can be checked on a tester's machine without a renderer.
//!
//! The decisions are pure functions over the report ([`plan`], [`eye_plan`]) and are
//! tested against the three captured reports, because the interesting cases - a
//! runtime that recommends more than it allows, or one with no MSAA - are exactly
//! the ones no single headset shows.

use serde_json::Value;

/// Swapchain formats to ask for, most preferred first. Only the order matters here:
/// the runtime decides which of these it can actually use, and the list is short on
/// purpose so a driver does not pick something exotic.
const FORMAT_PREFERENCE: [&str; 5] = ["R8G8B8A8_SRGB", "B8G8R8A8_SRGB", "R8G8B8A8_UNORM", "B8G8R8A8_UNORM", "R16G16B16A16_SFLOAT"];

/// Run `xr-plan`: probe the runtime, then print the configuration a session would use.
pub fn cmd(_args: &[String]) -> Result<(), String> {
    let report = crate::xr_info::probe();
    print!("{}", render(&plan(&report)));
    Ok(())
}

/// What a Phase 1 session would ask the runtime for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub runtime: Option<String>,
    pub headset: Option<String>,
    pub eye: Option<EyePlan>,
    /// Whether `XR_KHR_vulkan_swapchain_format_list` is available. With it, the app
    /// passes its preferred formats when the swapchain is created; without it, the
    /// app must enumerate and pick one format explicitly.
    pub format_list_extension: bool,
    pub notes: Vec<String>,
}

/// The per-eye half of the plan.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EyePlan {
    pub recommended: (u64, u64),
    pub maximum: (u64, u64),
    /// What we would actually request: the recommendation, clamped into the maximum.
    pub chosen: (u64, u64),
    pub recommended_samples: u64,
    pub max_samples: u64,
    /// What we would actually request: the recommendation, at least 1, within the maximum.
    pub chosen_samples: u64,
    /// True when the runtime recommended an image larger than it allows, which is a
    /// runtime bug but one we must survive rather than pass on.
    pub clamped: bool,
}

/// Turn a report into a plan. Never fails: a report with no runtime or no headset
/// produces a plan with nothing in it and the reason in `notes`.
pub(crate) fn plan(report: &Value) -> Plan {
    let runtime = match (report["openxr"]["runtime"]["runtime_name"].as_str(), report["openxr"]["runtime"]["runtime_version"].as_str()) {
        (Some(name), Some(version)) => Some(format!("{name} {version}")),
        (Some(name), None) => Some(name.to_string()),
        _ => None,
    };
    let headset = report["openxr"]["systems"].as_array().and_then(|s| s.first()).and_then(|s| s["system_name"].as_str()).map(str::to_string);

    let format_list_extension = report["openxr"]["instance_extensions"]
        .as_array()
        .map(|exts| exts.iter().any(|e| e["name"] == "XR_KHR_vulkan_swapchain_format_list"))
        .unwrap_or(false);

    let mut notes = Vec::new();
    let eye = eye_plan(report);
    match &eye {
        None => notes.push("No stereo view configuration was reported, so there is nothing to size.".to_string()),
        Some(e) => {
            if e.clamped {
                notes.push(format!(
                    "The runtime recommends {} x {} per eye but allows only {} x {}: the request is clamped to the maximum.",
                    e.recommended.0, e.recommended.1, e.maximum.0, e.maximum.1
                ));
            }
            if e.max_samples <= 1 {
                notes.push("This runtime offers no MSAA above 1x, so the session runs without it.".to_string());
            }
            if e.recommended_samples == 0 {
                notes.push("The runtime recommends 0 samples; 1 is used instead.".to_string());
            }
        }
    }
    if !format_list_extension {
        notes.push("XR_KHR_vulkan_swapchain_format_list is absent: enumerate the formats and pick one explicitly.".to_string());
    }

    Plan { runtime, headset, eye, format_list_extension, notes }
}

/// The primary stereo view configuration, sized and sampled for a session.
fn eye_plan(report: &Value) -> Option<EyePlan> {
    let stereo =
        report["openxr"]["view_configurations"].as_array()?.iter().find(|c| c.get("type").and_then(Value::as_str) == Some("primary_stereo"))?;
    let view = stereo.get("views")?.as_array()?.first()?;

    let recommended = (view.get("recommended_width")?.as_u64()?, view.get("recommended_height")?.as_u64()?);
    let maximum = (view.get("max_width")?.as_u64()?, view.get("max_height")?.as_u64()?);

    let (w, clamped_w) = clamp_axis(recommended.0, maximum.0);
    let (h, clamped_h) = clamp_axis(recommended.1, maximum.1);

    let recommended_samples = view.get("recommended_sample_count").and_then(Value::as_u64).unwrap_or(1);
    let max_samples = view.get("max_sample_count").and_then(Value::as_u64).unwrap_or(1);
    let wanted = recommended_samples.max(1);
    let chosen_samples = if max_samples == 0 { wanted } else { wanted.min(max_samples) }.max(1);

    Some(EyePlan { recommended, maximum, chosen: (w, h), recommended_samples, max_samples, chosen_samples, clamped: clamped_w || clamped_h })
}

/// Clamp one axis of the recommendation into what the runtime allows, and say whether
/// it had to. A maximum of 0 means the runtime did not report one, so the recommendation
/// stands rather than collapsing to nothing.
fn clamp_axis(recommended: u64, maximum: u64) -> (u64, bool) {
    if maximum == 0 {
        return (recommended.max(1), false);
    }
    if recommended > maximum { (maximum, true) } else { (recommended.max(1), false) }
}

/// The plan as the plain text `xr-plan` prints.
fn render(plan: &Plan) -> String {
    let mut out = String::from("SolidCraft VR plan\n==================\n\n");

    let runtime = plan.runtime.clone().unwrap_or_else(|| "not found".to_string());
    out.push_str(&format!("OpenXR runtime ..... {runtime}\n"));

    if plan.runtime.is_none() {
        out.push_str("\nNo plan: no OpenXR runtime was found.\n");
        out.push_str("Run `solidcraft-cli xr-info` for the detail and what to do about it.\n");
        return out;
    }

    out.push_str(&format!("Headset ............ {}\n", plan.headset.as_deref().unwrap_or("not found")));

    match &plan.eye {
        Some(eye) => {
            out.push_str(&format!(
                "Eye resolution ..... {} x {} recommended, {} x {} maximum\n",
                eye.recommended.0, eye.recommended.1, eye.maximum.0, eye.maximum.1
            ));
            out.push_str(&format!("Swapchain size ..... {} x {} per eye\n", eye.chosen.0, eye.chosen.1));
            out.push_str(&format!(
                "Sample count ....... {} (runtime recommends {}, maximum {})\n",
                eye.chosen_samples, eye.recommended_samples, eye.max_samples
            ));
        }
        None => out.push_str("Eye resolution ..... not reported\n"),
    }

    let formats = if plan.format_list_extension {
        "present - the preferred list is passed when the swapchain is created"
    } else {
        "absent - enumerate the formats and pick one explicitly"
    };
    out.push_str(&format!("Format list ext .... {formats}\n"));
    out.push_str(&format!("Format preference .. {}\n", FORMAT_PREFERENCE.join(", ")));

    if !plan.notes.is_empty() {
        out.push('\n');
        for note in &plan.notes {
            out.push_str(&format!("- {note}\n"));
        }
    }

    out.push_str("\nNothing is rendered: these are the numbers a Phase 1 session would request.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> Value {
        let text = match name {
            "oculus" => include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quest3s-oculus-1.208.0.json")),
            "steamvr" => include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quest3-steamvr-2.18.4.json")),
            "monado" => include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/monado-25.1.0-qwerty.json")),
            other => panic!("unknown fixture: {other}"),
        };
        serde_json::from_str(text).expect("fixture is valid JSON")
    }

    /// Quest 3S through Oculus: the recommendation fits inside the maximum, MSAA is
    /// available up to 4x, and the format list extension is absent.
    #[test]
    fn the_oculus_report_plans_the_recommended_size() {
        let plan = plan(&fixture("oculus"));
        let eye = plan.eye.expect("a stereo configuration");
        assert_eq!(eye.recommended, (1824, 1872));
        assert_eq!(eye.maximum, (3648, 3744), "the per-eye maximum, not the system-wide 4096 swapchain limit");
        assert_eq!(eye.chosen, (1824, 1872));
        assert!(!eye.clamped);
        assert_eq!(eye.chosen_samples, 1);
        assert_eq!(eye.max_samples, 4);
        assert!(!plan.format_list_extension);
        assert!(plan.notes.iter().any(|n| n.contains("enumerate the formats")), "{:?}", plan.notes);
    }

    /// SteamVR: a different size and a maximum of 1x, so the plan must not offer MSAA.
    #[test]
    fn the_steamvr_report_plans_no_msaa() {
        let plan = plan(&fixture("steamvr"));
        let eye = plan.eye.expect("a stereo configuration");
        assert_eq!(eye.chosen, (2528, 2780));
        assert_eq!(eye.max_samples, 1);
        assert_eq!(eye.chosen_samples, 1);
        assert!(plan.notes.iter().any(|n| n.contains("no MSAA above 1x")), "{:?}", plan.notes);
    }

    /// Monado on Linux: the one runtime on record that offers the format list
    /// extension, so the plan takes the other branch.
    #[test]
    fn the_monado_report_uses_the_format_list_extension() {
        let plan = plan(&fixture("monado"));
        assert!(plan.format_list_extension);
        let eye = plan.eye.expect("a stereo configuration");
        assert_eq!(eye.chosen, (896, 1007));
        assert!(!plan.notes.iter().any(|n| n.contains("enumerate the formats")), "{:?}", plan.notes);
    }

    /// A runtime that recommends more than it allows must be clamped, not trusted:
    /// passing the recommendation through would fail swapchain creation.
    #[test]
    fn a_recommendation_above_the_maximum_is_clamped() {
        let report = json!({
            "openxr": {
                "runtime": { "runtime_name": "Test", "runtime_version": "1" },
                "systems": [{ "system_name": "Test HMD" }],
                "view_configurations": [{
                    "type": "primary_stereo",
                    "views": [{
                        "recommended_width": 3000, "recommended_height": 2000,
                        "max_width": 2048, "max_height": 2048,
                        "recommended_sample_count": 8, "max_sample_count": 4,
                    }],
                }],
                "instance_extensions": [],
            }
        });
        let plan = plan(&report);
        let eye = plan.eye.expect("a stereo configuration");
        assert_eq!(eye.chosen, (2048, 2000));
        assert!(eye.clamped);
        assert_eq!(eye.chosen_samples, 4, "the sample request must be clamped too");
        assert!(plan.notes.iter().any(|n| n.contains("clamped to the maximum")), "{:?}", plan.notes);
    }

    /// A report with no runtime still renders a plan that says so, rather than panicking
    /// or printing a configuration nobody asked for.
    #[test]
    fn no_runtime_renders_no_plan() {
        let plan = plan(&json!({ "openxr": { "runtime": Value::Null } }));
        assert!(plan.eye.is_none());
        let text = render(&plan);
        assert!(text.contains("No plan: no OpenXR runtime"), "{text}");
    }

    /// The rendered plan reads like the summary: the numbers, then the reasons.
    #[test]
    fn the_rendered_plan_names_the_numbers_it_would_request() {
        let text = render(&plan(&fixture("oculus")));
        assert!(text.contains("Oculus 1.208.0"), "{text}");
        assert!(text.contains("Swapchain size ..... 1824 x 1872 per eye"), "{text}");
        assert!(text.contains("Sample count ....... 1"), "{text}");
        assert!(text.contains("Nothing is rendered"), "{text}");
    }
}
