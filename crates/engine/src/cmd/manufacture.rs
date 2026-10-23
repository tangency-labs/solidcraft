//! CNC manufacture commands (settings get/set for the document field).
//!
//! Job/runtime, library, probe, and op helpers land in follow-up PRs.

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_manufacture::{ManufactureSettings, MfgSetup};

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("manufacture.settings.get", "Manufacture Settings", get_settings)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("name?: setup name. Without name returns {setups, machines, posts, defaults}; with name returns {name, setup}. Does not evaluate the model."),
    CommandSpec::new("manufacture.settings.set", "Save Manufacture Setup", set_settings)
        .icon("settings")
        .doc_only()
        .params("name: setup name, set: object of fields to replace (stock, wcs, tools, operations, machine, post). Creates from defaults when absent, validates atomically, one undo step; identical edits are no-ops."),
    CommandSpec::new("manufacture.setup.new", "New Setup", new_setup)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .params("Opens the New Setup dialog in the Manufacture workspace. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.tools.library", "Tool Library", tool_library)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .params("Opens the document tool library for the active Manufacture setup."),
    CommandSpec::new("manufacture.hole_recognition", "Hole Recognition", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "DRILLING")
        .params("Opens Hole Recognition on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.drill", "Drill", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "DRILLING")
        .params("Opens a Drill operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.contour", "Contour", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Contour operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.face", "Face", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Face operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.adaptive", "Adaptive", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Adaptive operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.pocket", "Pocket", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Pocket operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.slot", "Slot", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Slot operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.trace", "Trace", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Trace operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.thread", "Thread", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Thread operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.bore", "Bore", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Bore operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.circular", "Circular", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Circular operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.engrave", "Engrave", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Engrave operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.2d.chamfer", "Chamfer", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "2D")
        .params("Opens a 2D Chamfer operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.3d.adaptive", "Adaptive", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "3D")
        .params("Opens 3D Adaptive Roughing on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.3d.parallel", "Parallel", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "3D")
        .params("Opens a 3D Parallel operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.3d.scallop", "Scallop", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "3D")
        .params("Opens a 3D Scallop operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.3d.steep", "Steep", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "3D")
        .params("Opens Steep and Shallow on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.3d.contour", "3D Contour", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "3D")
        .params("Opens a 3D Contour operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.3d.flat", "Flat", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "3D")
        .params("Opens a 3D Flat operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.multiaxis.swarf", "Swarf", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .params("Opens a Swarf operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.multiaxis.rough", "MA Rough", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .params("Opens Multi-Axis Roughing on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.multiaxis.rotary", "Rotary", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .params("Opens Rotary Parallel on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.multiaxis.finish", "MA Finish", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .params("Opens Multi-Axis Finishing on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.multiaxis.contour", "MA Contour", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .params("Opens Multi-Axis Contour on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.multiaxis.adv_swarf", "Adv. Swarf", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .params("Opens Advanced Swarf on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.turning.face", "Face", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .params("Opens a Turning Face operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.turning.profile", "Profile", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .params("Opens Turning Profile Roughing on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.turning.groove", "Groove", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .params("Opens a Turning Groove operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.turning.thread", "Thread", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .params("Opens a Turning Thread operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.turning.chamfer", "Chamfer", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .params("Opens a Turning Chamfer operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.turning.part", "Part", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .params("Opens a Turning Part operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.part_handling.grab", "Grab", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PART")
        .params("Opens Subspindle Grab on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.part_handling.bar_pull", "Bar Pull", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PART")
        .params("Opens a Bar Pull operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.part_handling.return", "Return", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PART")
        .params("Opens Subspindle Return on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.part_handling.tool_call", "Tool Call", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PART")
        .params("Opens a Tool Call operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.inspection.wcs", "Probe WCS", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PROBE")
        .params("Opens Probe WCS on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.inspection.geometry", "Probe Geom", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PROBE")
        .params("Opens Probe Geometry on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.inspection.surface", "Surface", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PROBE")
        .params("Opens Inspect Surface on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.inspection.align", "Align", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PROBE")
        .params("Opens Part Alignment on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.inspection.manual", "Manual", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PROBE")
        .params("Opens Manual Inspection on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.inspection.record", "Record", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "PROBE")
        .params("Opens Record Manual Inspection on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.fabrication.nest", "Nest", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "CUTTING")
        .params("Opens Nest planning on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.fabrication.profile", "2D Profile", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "CUTTING")
        .params("Opens a 2D Profile cutting operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.additive.toolpath", "Additive", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ADDITIVE")
        .params("Opens an Additive Toolpath operation on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.additive.from_gcode", "From G-code", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ADDITIVE")
        .params("Opens From G-code import on the active Manufacture setup. Document writes go through manufacture.settings.set."),
    CommandSpec::new("manufacture.additive.simulate", "Simulate", twod_open)
        .icon("settings")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ADDITIVE")
        .params("Simulates the Additive toolpath on the active Manufacture setup without a companion runtime."),
];

// ── Settings get ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetSettings {
    name: Option<String>,
}

fn get_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.settings.get";
    let req: GetSettings = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    match req.name {
        None => Ok(json!({
            "setups": s.doc.manufacture.setups,
            "machines": s.doc.manufacture.machines,
            "posts": s.doc.manufacture.posts,
            "defaults": MfgSetup::default(),
        })),
        Some(name) => {
            ManufactureSettings::validate_name(&name).map_err(|e| bad(cmd, e.to_string()))?;
            let setup = s.doc.manufacture.setups.get(&name).ok_or_else(|| bad(cmd, format!("no manufacture setup named `{name}`")))?;
            Ok(json!({"name": name, "setup": setup}))
        }
    }
}

// ── Settings set ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSettings {
    name: String,
    set: Value,
}

fn set_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.settings.set";
    let req: SetSettings = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    ManufactureSettings::validate_name(&req.name).map_err(|e| bad(cmd, e.to_string()))?;

    let patch = req.set.as_object().filter(|p| p.len() <= 16).ok_or_else(|| bad(cmd, "`set` must be an object with at most 16 fields"))?;

    // Merge patch fields onto the existing (or default) setup.
    let current = s.doc.manufacture.setups.get(&req.name).cloned().unwrap_or_default();
    let mut value = serde_json::to_value(&current).map_err(|e| bad(cmd, e.to_string()))?;
    let obj = value.as_object_mut().ok_or_else(|| bad(cmd, "internal: setup is not a JSON object"))?;
    for (key, val) in patch {
        obj.insert(key.clone(), val.clone());
    }
    let updated: MfgSetup = serde_json::from_value(value).map_err(|e| bad(cmd, format!("invalid field: {e}")))?;

    let reply = json!({"name": req.name, "setup": updated});

    // No-op: return early without touching the document (no undo step).
    if s.doc.manufacture.setups.get(&req.name) == Some(&updated) {
        return Ok(reply);
    }

    // Atomic: validate the entire settings block with the new setup before committing.
    let mut new_manufacture = s.doc.manufacture.clone();
    new_manufacture.setups.insert(req.name.clone(), updated);
    new_manufacture.validate().map_err(|e| bad(cmd, e.to_string()))?;

    // doc_mut() / Arc::make_mut creates a new Arc → execute() detects the change and pushes
    // exactly one undo step (CommandSpec::undoable defaults to true).
    s.doc_mut().manufacture = new_manufacture;

    Ok(reply)
}

fn new_setup(_s: &mut Session, _params: &Value) -> Result<Value> {
    Ok(json!({"opened": true}))
}

fn tool_library(_s: &mut Session, _params: &Value) -> Result<Value> {
    Ok(json!({"opened": true}))
}

fn twod_open(_s: &mut Session, _params: &Value) -> Result<Value> {
    Ok(json!({"opened": true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_get_returns_empty_by_default() {
        let mut s = Session::default();
        let r = s.execute("manufacture.settings.get", &json!({})).unwrap();
        assert!(r["setups"].as_object().is_some_and(|m| m.is_empty()));
        assert!(r["machines"].as_object().is_some_and(|m| m.is_empty()));
    }

    #[test]
    fn settings_set_creates_and_returns_setup() {
        let mut s = Session::default();
        let patch = json!({
            "stock": {"kind": "box", "width": 100.0, "depth": 80.0, "height": 20.0},
            "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0, "corner_radius": 0.0,
                       "length": 50.0, "flute_length": 16.0, "flutes": 4}],
            "operations": [{"type": "face", "tool_number": 1, "depth": 0.5, "stepover": 0.75}]
        });
        let r = s.execute("manufacture.settings.set", &json!({"name": "Setup1", "set": patch})).unwrap();
        assert_eq!(r["name"], "Setup1");
        assert!(s.doc.manufacture.setups.contains_key("Setup1"));
    }

    #[test]
    fn settings_set_noop_if_unchanged() {
        let mut s = Session::default();
        let patch = json!({
            "stock": {"kind": "box", "width": 100.0, "depth": 80.0, "height": 20.0}
        });
        s.execute("manufacture.settings.set", &json!({"name": "S", "set": patch.clone()})).unwrap();
        let undo_before = s.undo.len();
        // Identical patch → no undo step.
        s.execute("manufacture.settings.set", &json!({"name": "S", "set": patch})).unwrap();
        assert_eq!(s.undo.len(), undo_before, "no-op must not create an undo step");
    }

    #[test]
    fn settings_set_rejects_invalid_name() {
        let mut s = Session::default();
        let r = s.execute("manufacture.settings.set", &json!({"name": " bad", "set": {}}));
        assert!(r.is_err());
    }

    #[test]
    fn setup_new_is_display_only() {
        let mut s = Session::default();
        let r = s.execute("manufacture.setup.new", &json!({})).unwrap();
        assert_eq!(r["opened"], true);
        assert!(s.doc.manufacture.setups.is_empty());
    }

    #[test]
    fn tool_library_is_display_only() {
        let mut s = Session::default();
        let r = s.execute("manufacture.tools.library", &json!({})).unwrap();
        assert_eq!(r["opened"], true);
        assert!(s.doc.manufacture.setups.is_empty());
    }

    #[test]
    fn drill_is_display_only() {
        let mut s = Session::default();
        let r = s.execute("manufacture.drill", &json!({})).unwrap();
        assert_eq!(r["opened"], true);
        assert!(s.doc.manufacture.setups.is_empty());
    }

    #[test]
    fn hole_recognition_is_display_only() {
        let mut s = Session::default();
        let r = s.execute("manufacture.hole_recognition", &json!({})).unwrap();
        assert_eq!(r["opened"], true);
        assert!(s.doc.manufacture.setups.is_empty());
    }

    #[test]
    fn twod_commands_are_display_only() {
        const IDS: &[&str] = &[
            "manufacture.2d.contour",
            "manufacture.2d.face",
            "manufacture.2d.adaptive",
            "manufacture.2d.pocket",
            "manufacture.2d.slot",
            "manufacture.2d.trace",
            "manufacture.2d.thread",
            "manufacture.2d.bore",
            "manufacture.2d.circular",
            "manufacture.2d.engrave",
            "manufacture.2d.chamfer",
        ];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn threed_commands_are_display_only() {
        const IDS: &[&str] = &[
            "manufacture.3d.adaptive",
            "manufacture.3d.parallel",
            "manufacture.3d.scallop",
            "manufacture.3d.steep",
            "manufacture.3d.contour",
            "manufacture.3d.flat",
        ];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn multiaxis_commands_are_display_only() {
        const IDS: &[&str] = &[
            "manufacture.multiaxis.swarf",
            "manufacture.multiaxis.rough",
            "manufacture.multiaxis.rotary",
            "manufacture.multiaxis.finish",
            "manufacture.multiaxis.contour",
            "manufacture.multiaxis.adv_swarf",
        ];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn turning_commands_are_display_only() {
        const IDS: &[&str] = &[
            "manufacture.turning.face",
            "manufacture.turning.profile",
            "manufacture.turning.groove",
            "manufacture.turning.thread",
            "manufacture.turning.chamfer",
            "manufacture.turning.part",
        ];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn part_handling_commands_are_display_only() {
        const IDS: &[&str] = &[
            "manufacture.part_handling.grab",
            "manufacture.part_handling.bar_pull",
            "manufacture.part_handling.return",
            "manufacture.part_handling.tool_call",
        ];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn inspection_commands_are_display_only() {
        const IDS: &[&str] = &[
            "manufacture.inspection.wcs",
            "manufacture.inspection.geometry",
            "manufacture.inspection.surface",
            "manufacture.inspection.align",
            "manufacture.inspection.manual",
            "manufacture.inspection.record",
        ];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn fabrication_commands_are_display_only() {
        const IDS: &[&str] = &["manufacture.fabrication.nest", "manufacture.fabrication.profile"];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }

    #[test]
    fn additive_commands_are_display_only() {
        const IDS: &[&str] = &["manufacture.additive.toolpath", "manufacture.additive.from_gcode", "manufacture.additive.simulate"];
        for id in IDS {
            let mut s = Session::default();
            let r = s.execute(id, &json!({})).unwrap();
            assert_eq!(r["opened"], true, "{id}");
            assert!(s.doc.manufacture.setups.is_empty(), "{id}");
        }
    }
}
