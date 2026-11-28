//! MCP workflow prompts (`prompts/list` and `prompts/get`) grounded in SolidCraft's bundled skills
//! and parametric CAD conventions.

use serde_json::{Value, json};

/// Prompts advertised by `prompts/list`.
pub(crate) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "model-part",
            "title": "Model a parametric part",
            "description": "Plan, build in staged batches, and quantitatively verify a new parametric 3D part in SolidCraft",
            "arguments": [
                {"name": "part", "description": "Short description of the part to model (dimensions, features, constraints)", "required": false},
                {"name": "material", "description": "Optional material or manufacturing process (e.g. 6061 Aluminum, PLA, sheet metal)", "required": false},
            ],
        }),
        json!({
            "name": "edit-design",
            "title": "Edit an existing design",
            "description": "Map an open SolidCraft design and apply parametric in-place edits without rebuilding from scratch",
            "arguments": [
                {"name": "goal", "description": "Requested change (resize, move a feature, add a fillet/cut, or make a fixed dimension parametric)", "required": false},
                {"name": "target", "description": "Optional body, sketch, or feature name to focus on", "required": false},
            ],
        }),
        json!({
            "name": "check-parameters",
            "title": "Sweep and validate design parameters",
            "description": "Test driving user parameters across their allowed ranges without mutating the saved design",
            "arguments": [
                {"name": "parameters", "description": "Optional comma-separated parameter names to test (default: all driving user parameters)", "required": false},
                {"name": "ranges", "description": "Optional min/max ranges or test sets to sweep", "required": false},
            ],
        }),
        json!({
            "name": "diagnose-feature",
            "title": "Diagnose a failing or warning feature",
            "description": "Inspect timeline errors or warnings, trace dependencies, and apply a minimal in-place fix",
            "arguments": [
                {"name": "feature", "description": "Optional feature name or numeric id to inspect (default: first failing or warning feature)", "required": false},
            ],
        }),
        json!({
            "name": "assembly-clearance",
            "title": "Check assembly clearances and motion interference",
            "description": "Verify component grounding, static clearances, and interference across joint sweeps or motion studies",
            "arguments": [
                {"name": "occurrences", "description": "Optional comma-separated occurrence or body names to check (default: all moving parts)", "required": false},
                {"name": "joint", "description": "Optional driving joint name to sweep through its range of motion", "required": false},
            ],
        }),
    ]
}

/// Fill prompt `name` with validated string `args` (MCP `GetPromptResult`).
pub(crate) fn get(name: &str, args: &Value) -> Result<Value, String> {
    let (description, text) = match name {
        "model-part" => ("Plan, build, and verify a parametric 3D part in SolidCraft", model_part_prompt(arg(args, "part"), arg(args, "material"))),
        "edit-design" => ("Map and edit an existing SolidCraft design in place", edit_design_prompt(arg(args, "goal"), arg(args, "target"))),
        "check-parameters" => (
            "Validate driving parameters across ranges without keeping changes",
            check_parameters_prompt(arg(args, "parameters"), arg(args, "ranges")),
        ),
        "diagnose-feature" => ("Diagnose and repair a failing or warning SolidCraft timeline feature", diagnose_feature_prompt(arg(args, "feature"))),
        "assembly-clearance" => {
            ("Check assembly clearances, grounding, and motion interference", assembly_clearance_prompt(arg(args, "occurrences"), arg(args, "joint")))
        }
        _ => return Err(format!("unknown prompt `{name}`")),
    };
    Ok(json!({
        "description": description,
        "messages": [
            {
                "role": "user",
                "content": {"type": "text", "text": text},
            }
        ],
    }))
}

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn model_part_prompt(part: Option<&str>, material: Option<&str>) -> String {
    let mut out = String::from("Model a parametric 3D part in SolidCraft following the `solidcraft-modeling` workflow.\n\n");
    if let Some(p) = part {
        out.push_str(&format!("Part specification: {p}\n"));
    }
    if let Some(m) = material {
        out.push_str(&format!("Material / process: {m}\n"));
    }
    if part.is_some() || material.is_some() {
        out.push('\n');
    }
    let sheet_hint = if material.is_some_and(|m| m.to_ascii_lowercase().contains("sheet")) {
        " Also read `solidcraft://skill/solidcraft-sheetmetal/reference.md` and configure an explicit sheet rule via `sheet.manage_rules`."
    } else {
        ""
    };
    out.push_str(&format!(
        "Workflow:\n\
         1. Load `solidcraft://skill/solidcraft-modeling/reference.md` (or `skill_reference {{\"skill\":\"solidcraft-modeling\"}}`){sheet_hint} and run `inspect_design {{\"measure\":false,\"sketches\":false}}`. If connected to the desktop app and the open design already has features, ask before calling `new_design`.\n\
         2. Plan in numbers first: user parameters (mm/deg, Z up), origin-centered placement, feature order (use `solid.box`/`solid.cylinder`/`solid.hole` when no custom profile is needed; fillet outside edges before `solid.shell`; place interior features after the shell and wall cuts last), exact midpoint picks, and expected volume and bounding box per stage.\n\
         3. Build in 2-4 `batch` calls with `\"verbose\": false`, naming every feature and using expression strings (not bare numbers) wherever geometry must follow parameters. End every sketch at `dof: 0`.\n\
         4. Verify each stage with `measure {{}}` against your computed volume and bbox, check round holes/bosses with `body_topology {{\"faces\":\"cylinders\"}}`, test parametric rebuilds with `check_parameters`, and finish with `screenshot {{\"source\":\"model\",\"view\":\"iso\",\"width\":480,\"height\":320}}`."
    ));
    out
}

fn edit_design_prompt(goal: Option<&str>, target: Option<&str>) -> String {
    let mut out = String::from("Edit the open SolidCraft design in place following the `solidcraft-editing` workflow.\n\n");
    if let Some(g) = goal {
        out.push_str(&format!("Requested change: {g}\n"));
    }
    if let Some(t) = target {
        out.push_str(&format!("Focus target: {t}\n"));
    }
    if goal.is_some() || target.is_some() {
        out.push('\n');
    }
    out.push_str(
        "Workflow:\n\
         1. Map the design without rebuilding: call `inspect_design {\"measure\":false,\"sketches\":false}`, `execute parameters.list {\"filter\":\"feature\"}`, and `measure {}` for a baseline. Use `solidcraft://body/{name}`, `solidcraft://sketch/{name}`, or `solidcraft://feature/{id}` to inspect only the specific targets involved.\n\
         2. Prefer editing existing parameters (`set_parameter` / `parameters.change`) or feature definitions (`timeline.edit`) rather than deleting and remodeling so downstream picks and history stay intact. Check `timeline.dependents` before deleting any feature, and take `edit.checkpoint` before risky or multi-step edits.\n\
         3. Send independent edits in one `batch`, predict the expected volume or centre-of-mass delta, then verify with `measure {}` and confirm `inspect_design {\"measure\":false,\"sketches\":false}` reports no feature errors or warnings.\n\
         4. If the edit must remain parametric, verify with `check_parameters`, and take a top-view model render if interior features changed.",
    );
    out
}

fn check_parameters_prompt(parameters: Option<&str>, ranges: Option<&str>) -> String {
    let mut out = String::from("Validate the open SolidCraft design's parametric stability following the `solidcraft-checking` workflow.\n\n");
    if let Some(p) = parameters {
        out.push_str(&format!("Parameters to test: {p}\n"));
    }
    if let Some(r) = ranges {
        out.push_str(&format!("Target ranges / test sets: {r}\n"));
    }
    if parameters.is_some() || ranges.is_some() {
        out.push('\n');
    }
    out.push_str(
        "Workflow:\n\
         1. Run `inspect_design {\"measure\":false,\"sketches\":false}` to list all user parameters, sketch DOFs, and baseline timeline health. Flag any sketch with `dof > 0`.\n\
         2. Record baseline measurements with `measure {\"summary\":true}` and round-feature axes with `body_topology {\"faces\":\"cylinders\"}` where relevant.\n\
         3. Sweep each driving parameter across its low and high bounds using `check_parameters {\"set\": {...}}` (or `execute inspect.parameter_variants` for multi-parameter combinations) so the open design is left unchanged. Check for `errors: []`, boolean-retry warnings, valid closed volumes, expected bbox shifts, and zero-distance sign flips.\n\
         4. Summarize every tested state (values, volume, bbox, errors/warnings) and call out any fragile expressions, hard-coded numbers that failed to follow parameters, or under-constrained sketches.",
    );
    out
}

fn diagnose_feature_prompt(feature: Option<&str>) -> String {
    let mut out = String::from("Diagnose and fix a failing or warning timeline feature in SolidCraft.\n\n");
    if let Some(f) = feature {
        out.push_str(&format!("Target feature: {f}\n\n"));
    }
    out.push_str(
        "Workflow:\n\
         1. Run `inspect_design {\"measure\":false,\"sketches\":false}` to identify every feature with a non-null `error` or `warning` in timeline order.\n\
         2. Inspect the first root-cause feature via `solidcraft://feature/{id}` (`timeline.inspect`), check its downstream impact with `execute timeline.dependents`, and inspect any referenced sketch via `solidcraft://sketch/{name}` (`sketch.inspect`).\n\
         3. Identify the root cause (e.g. unresolved edge/face pick, degenerate zero dimension, under-constrained sketch, shell/fillet ordering, boolean coincidence, or bad parameter expression). Use `solidcraft://command/{id}` if you need the exact parameter schema for an edit command.\n\
         4. Take `edit.checkpoint`, apply the smallest in-place fix (`parameters.change` or `timeline.edit`), confirm every timeline feature now has `error: null` and `warning: null`, and verify the resulting geometry with `measure {}`.",
    );
    out
}

fn assembly_clearance_prompt(occurrences: Option<&str>, joint: Option<&str>) -> String {
    let mut out = String::from("Check assembly clearances, grounding, and motion interference following the `solidcraft-mechanisms` workflow.\n\n");
    if let Some(o) = occurrences {
        out.push_str(&format!("Target occurrences / bodies: {o}\n"));
    }
    if let Some(j) = joint {
        out.push_str(&format!("Driving joint to sweep: {j}\n"));
    }
    if occurrences.is_some() || joint.is_some() {
        out.push('\n');
    }
    out.push_str(
        "Workflow:\n\
         1. Read `solidcraft://skill/solidcraft-mechanisms/reference.md` if needed, then map the assembly with `inspect_design {\"measure\":false,\"sketches\":false,\"joints\":\"summary\"}` and `execute joint.list {\"moving_only\":true,\"dof_free_only\":true,\"summary\":true}`.\n\
         2. Verify grounding (`fixed` vs `floating`), unexpected `conflicts`, and per-occurrence `dof`. Inspect individual parts with `solidcraft://body/{name}` or `execute occurrence.properties` rather than dumping full `component.list`.\n\
         3. Check static overlaps and gaps (`execute contact.check`, `execute inspect.interference`, `execute inspect.clearance`).\n\
         4. For moving joints or motion studies, run `execute joint.sweep {\"report_gaps\":true, ...}`, `execute joint.find_limits`, or `execute motion.interference` without leaving the assembly out of its nominal pose. Report colliding pairs, minimum clearance in mm, stop angles/positions, and actionable clearance fixes.",
    );
    out
}
