//! Manufacture workspace chrome. These commands open UI; document writes go through `cam.setup`.
//!
//! Recut of the #1782 ribbon onto the cam document. Op dialogs land in the stacked UI PRs.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::{Result, Session};

fn opened(_s: &mut Session, _params: &Value) -> Result<Value> {
    Ok(json!({"opened": true}))
}

const fn chrome(id: &'static str, label: &'static str, panel: &'static str, icon: &'static str, params: &'static str) -> CommandSpec {
    CommandSpec::new(id, label, opened).icon(icon).noundo().doc_only().at("MANUFACTURE", panel).params(params)
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("manufacture.setup.new", "New Setup", opened)
        .icon("origin")
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .params("Opens the New Setup dialog. Document writes go through cam.setup."),
    chrome("manufacture.tools.library", "Tool Library", "MANAGE", "library", "Opens the document tool library for the active Manufacture setup."),
    chrome("manufacture.manage.machine_library", "Machines", "MANAGE", "list", "Opens the Machine Library browser."),
    chrome("manufacture.manage.post_library", "Posts", "MANAGE", "export", "Opens the Post Library browser."),
    chrome("manufacture.manage.template_library", "Templates", "MANAGE", "copy", "Opens the Template Library browser."),
    chrome("manufacture.2d.contour", "Contour", "2D", "cut", "Opens a 2D Contour operation on the active Manufacture setup."),
    chrome("manufacture.2d.face", "Face", "2D", "surface", "Opens a 2D Face operation on the active Manufacture setup."),
    chrome("manufacture.2d.adaptive", "Adaptive", "2D", "sweep", "Opens a 2D Adaptive operation on the active Manufacture setup."),
    chrome("manufacture.2d.pocket", "Pocket", "2D", "hole", "Opens a 2D Pocket operation on the active Manufacture setup."),
    chrome("manufacture.2d.slot", "Slot", "2D", "slot", "Opens a 2D Slot operation on the active Manufacture setup."),
    chrome("manufacture.2d.trace", "Trace", "2D", "trace", "Opens a 2D Trace operation on the active Manufacture setup."),
    chrome("manufacture.2d.thread", "Thread", "2D", "thread", "Opens a 2D Thread operation on the active Manufacture setup."),
    chrome("manufacture.2d.bore", "Bore", "2D", "hole", "Opens a 2D Bore operation on the active Manufacture setup."),
    chrome("manufacture.2d.circular", "Circular", "2D", "hole", "Opens a 2D Circular operation on the active Manufacture setup."),
    chrome("manufacture.2d.engrave", "Engrave", "2D", "text", "Opens a 2D Engrave operation on the active Manufacture setup."),
    chrome("manufacture.2d.chamfer", "Chamfer", "2D", "chamfer", "Opens a 2D Chamfer operation on the active Manufacture setup."),
    chrome("manufacture.drill", "Drill", "DRILLING", "hole", "Opens a Drill operation on the active Manufacture setup."),
    chrome("manufacture.hole_recognition", "Hole Recognition", "DRILLING", "hole", "Opens Hole Recognition on the active Manufacture setup."),
    chrome("manufacture.3d.adaptive", "Adaptive", "3D", "sweep", "Opens 3D Adaptive Roughing on the active Manufacture setup."),
    chrome("manufacture.3d.parallel", "Parallel", "3D", "surface", "Opens a 3D Parallel operation on the active Manufacture setup."),
    chrome("manufacture.3d.scallop", "Scallop", "3D", "untrim", "Opens a 3D Scallop operation on the active Manufacture setup."),
    chrome("manufacture.3d.steep", "Steep", "3D", "draft_analysis", "Opens Steep and Shallow on the active Manufacture setup."),
    chrome("manufacture.3d.contour", "3D Contour", "3D", "cut", "Opens a 3D Contour operation on the active Manufacture setup."),
    chrome("manufacture.3d.flat", "Flat", "3D", "surface", "Opens a 3D Flat operation on the active Manufacture setup."),
    chrome("manufacture.multiaxis.swarf", "Swarf", "MULTI-AXIS", "sweep", "Opens a Swarf operation on the active Manufacture setup."),
    chrome("manufacture.multiaxis.rough", "MA Rough", "MULTI-AXIS", "extrude", "Opens Multi-Axis Roughing on the active Manufacture setup."),
    chrome("manufacture.multiaxis.rotary", "Rotary", "MULTI-AXIS", "revolve", "Opens Rotary Parallel on the active Manufacture setup."),
    chrome("manufacture.multiaxis.finish", "MA Finish", "MULTI-AXIS", "fillet", "Opens Multi-Axis Finishing on the active Manufacture setup."),
    chrome("manufacture.multiaxis.contour", "MA Contour", "MULTI-AXIS", "cut", "Opens Multi-Axis Contour on the active Manufacture setup."),
    chrome("manufacture.multiaxis.adv_swarf", "Adv. Swarf", "MULTI-AXIS", "sweep", "Opens Advanced Swarf on the active Manufacture setup."),
    chrome("manufacture.turning.face", "Face", "TURNING", "surface", "Opens a Turning Face operation on the active Manufacture setup."),
    chrome("manufacture.turning.profile", "Profile", "TURNING", "silhouette", "Opens Turning Profile Roughing on the active Manufacture setup."),
    chrome("manufacture.turning.groove", "Groove", "TURNING", "slot", "Opens a Turning Groove operation on the active Manufacture setup."),
    chrome("manufacture.turning.thread", "Thread", "TURNING", "thread", "Opens a Turning Thread operation on the active Manufacture setup."),
    chrome("manufacture.turning.chamfer", "Chamfer", "TURNING", "chamfer", "Opens a Turning Chamfer operation on the active Manufacture setup."),
    chrome("manufacture.turning.part", "Part", "TURNING", "split_body", "Opens a Turning Part operation on the active Manufacture setup."),
    chrome("manufacture.part_handling.grab", "Grab", "PART", "joint", "Opens Subspindle Grab on the active Manufacture setup."),
    chrome("manufacture.part_handling.bar_pull", "Bar Pull", "PART", "move", "Opens a Bar Pull operation on the active Manufacture setup."),
    chrome("manufacture.part_handling.return", "Return", "PART", "update", "Opens Subspindle Return on the active Manufacture setup."),
    chrome("manufacture.part_handling.tool_call", "Tool Call", "PART", "list", "Opens a Tool Call operation on the active Manufacture setup."),
    chrome("manufacture.inspection.wcs", "Probe WCS", "PROBE", "origin", "Opens Probe WCS on the active Manufacture setup."),
    chrome("manufacture.inspection.geometry", "Probe Geom", "PROBE", "measure", "Opens Probe Geometry on the active Manufacture setup."),
    chrome("manufacture.inspection.surface", "Surface", "PROBE", "zebra", "Opens Inspect Surface on the active Manufacture setup."),
    chrome("manufacture.inspection.align", "Align", "PROBE", "align", "Opens Part Alignment on the active Manufacture setup."),
    chrome("manufacture.inspection.manual", "Manual", "PROBE", "select", "Opens Manual Inspection on the active Manufacture setup."),
    chrome("manufacture.inspection.record", "Record", "PROBE", "params", "Opens Record Manual Inspection on the active Manufacture setup."),
    chrome("manufacture.fabrication.nest", "Nest", "CUTTING", "pattern", "Opens Nest planning on the active Manufacture setup."),
    chrome(
        "manufacture.fabrication.profile",
        "2D Profile",
        "CUTTING",
        "cut",
        "Opens a 2D Profile cutting operation on the active Manufacture setup.",
    ),
    chrome(
        "manufacture.additive.toolpath",
        "Additive",
        "ADDITIVE",
        "extrude",
        "Opens an Additive Toolpath operation on the active Manufacture setup.",
    ),
    chrome("manufacture.additive.from_gcode", "From G-code", "ADDITIVE", "export", "Opens From G-code import on the active Manufacture setup."),
    chrome("manufacture.additive.simulate", "Simulate", "ADDITIVE", "camera", "Simulates the Additive toolpath on the active Manufacture setup."),
    chrome("manufacture.modify.trim", "Trim", "MODIFY", "trim", "Opens Trim on the active Manufacture setup."),
    chrome("manufacture.modify.delete_passes", "Del Passes", "MODIFY", "delete", "Opens Delete Passes on the active Manufacture setup."),
    chrome("manufacture.modify.leads_links", "Leads/Links", "MODIFY", "spline", "Opens Leads and Links on the active Manufacture setup."),
    chrome("manufacture.modify.replace_tool", "Replace Tool", "MODIFY", "update", "Opens Replace Tool on the active Manufacture setup."),
    chrome("manufacture.modify.move_entry", "Move Entry", "MODIFY", "move", "Opens Move Entry Positions on the active Manufacture setup."),
    chrome("manufacture.post.process", "Post", "POST", "export", "Opens Post Process on the active Manufacture setup."),
    chrome("manufacture.post.time", "Cycle Time", "POST", "params", "Opens Machining Time on the active Manufacture setup."),
    chrome("manufacture.post.sheet", "Setup Sheet", "POST", "list", "Opens a Setup Sheet for the active Manufacture setup."),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn setup_new_is_display_only() {
        let mut s = Session::default();
        let r = s.execute("manufacture.setup.new", &json!({})).unwrap();
        assert_eq!(r["opened"], true);
        assert!(s.doc.cam.setups.is_empty());
    }

    #[test]
    fn contour_is_display_only() {
        let mut s = Session::default();
        let r = s.execute("manufacture.2d.contour", &json!({})).unwrap();
        assert_eq!(r["opened"], true);
        assert!(s.doc.cam.setups.is_empty());
    }

    #[test]
    fn ribbon_icons_are_not_all_settings() {
        let icons: Vec<_> = COMMANDS.iter().map(|c| c.icon).collect();
        assert!(icons.contains(&"cut"));
        assert!(icons.contains(&"hole"));
        assert!(icons.contains(&"origin"));
        assert!(!icons.iter().all(|icon| *icon == "settings"));
    }
}
