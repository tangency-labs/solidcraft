//! CAM commands: tools, setups, 2D contour/pocket operations, and G-code posting through
//! `solidcraft-cam`. Toolpaths are computed at post time and never stored on the design.

use serde_json::{Value, json};
use solidcraft_cam::{Cut, Feeds, Heights, Operation, Post, Program, Side, Tool, ToolKind, contour, emit, pocket, readback};
use solidcraft_doc::cam::{
    CamOperation, CamSetup, CamSettings, MAX_OPS, MAX_SETUPS, MAX_TOOLS, PolygonRegion, StockBox, default_contour_params, default_cut,
    default_pocket_params,
};

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, vec2_list};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.tool", "CAM Tool", tool_cmd).at("MANUFACTURE", "SETUP").icon("tool").params(
        "number (1…9999); kind?: flat_endmill|ball_endmill|drill (default flat_endmill); diameter (mm); flute_length (mm); \
         description?; flutes?; spindle_rpm?; feed?; plunge_feed?; stepdown?; stepover?; delete?: bool",
    ),
    CommandSpec::new("cam.setup", "CAM Setup", setup_cmd).at("MANUFACTURE", "SETUP").icon("setup").params(
        "name; work_offset?: 1…6 (G54…G59, default 1); stock?: {min:[x,y,z], max:[x,y,z]}; delete?: bool",
    ),
    CommandSpec::new("cam.contour", "2D Contour", contour_cmd).at("MANUFACTURE", "MILLING").icon("contour").params(
        "setup; name; tool (number); outer: [[x,y],…] (or regions: [{outer, holes?}]); holes?: [[[x,y],…]]; \
         side?: outside|inside|on (default outside); climb?: bool (default true); stock_to_leave?; tolerance?; \
         ramp_angle?: deg (default 0); clearance?; retract?; top?; bottom (required unless cut given); stepdown?; \
         feed?; plunge_feed?; spindle_rpm?; coolant?; cut?: {heights, feeds, stepdown}",
    ),
    CommandSpec::new("cam.pocket", "2D Pocket", pocket_cmd).at("MANUFACTURE", "MILLING").icon("pocket").params(
        "setup; name; tool (number); outer: [[x,y],…] (or regions); holes?; stepover? (default half the tool diameter); \
         climb?; stock_to_leave?; tolerance?; ramp_angle?: deg; clearance?; retract?; top?; bottom; stepdown?; \
         feed?; plunge_feed?; spindle_rpm?; coolant?; cut?",
    ),
    CommandSpec::new("cam.delete_op", "Delete CAM Operation", delete_op)
        .at("MANUFACTURE", "MILLING")
        .icon("delete")
        .params("setup; name"),
    CommandSpec::new("cam.post", "Post Process", post_cmd).icon("post").noundo().params(
        "setup; post?: rs274 | name of a saved post (default rs274); program?: name (default the setup's); \
         verify?: bool (default true: read the G-code back and check it against the toolpaths) → \
         {gcode, extension, post, operations, readback?}",
    ),
    CommandSpec::new("cam.list", "List CAM", list_cmd).icon("tool").noundo().params("→ tools, setups (with operations) and saved posts"),
];

fn tool_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool";
    let number = num(p, "number").filter(|n| *n >= 1.0 && *n <= 9999.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`number` must be an integer 1…9999"))?;
    if bool_(p, "delete").unwrap_or(false) {
        let doc = s.doc_mut();
        let before = doc.cam.tools.len();
        doc.cam.tools.retain(|t| t.number != number);
        if doc.cam.tools.len() == before {
            return Err(bad(cmd, format!("no tool {number}")));
        }
        return Ok(json!({"deleted": number}));
    }
    let kind = match str_(p, "kind").unwrap_or("flat_endmill") {
        "flat_endmill" => ToolKind::FlatEndmill,
        "ball_endmill" => ToolKind::BallEndmill,
        "drill" => ToolKind::Drill,
        other => return Err(bad(cmd, format!("unknown tool kind `{other}` (flat_endmill, ball_endmill, drill)"))),
    };
    let diameter = num(p, "diameter").ok_or_else(|| bad(cmd, "`diameter` is required (mm)"))?;
    let flute_length = num(p, "flute_length").ok_or_else(|| bad(cmd, "`flute_length` is required (mm)"))?;
    let mut tool = Tool::new(number, kind, diameter, flute_length);
    if let Some(d) = str_(p, "description") {
        tool.description = d.chars().take(256).collect();
    }
    if let Some(f) = num(p, "flutes").filter(|n| *n >= 1.0 && *n <= 100.0 && n.fract() == 0.0) {
        tool.flutes = f as u32;
    }
    tool.spindle_rpm = num(p, "spindle_rpm").or(tool.spindle_rpm);
    tool.feed = num(p, "feed").or(tool.feed);
    tool.plunge_feed = num(p, "plunge_feed").or(tool.plunge_feed);
    tool.stepdown = num(p, "stepdown").or(tool.stepdown);
    tool.stepover = num(p, "stepover").or(tool.stepover);
    tool.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let doc = s.doc_mut();
    if doc.cam.tools.len() >= MAX_TOOLS && !doc.cam.tools.iter().any(|t| t.number == number) {
        return Err(bad(cmd, format!("a design has at most {MAX_TOOLS} tools")));
    }
    match doc.cam.tools.iter_mut().find(|t| t.number == number) {
        Some(slot) => *slot = tool.clone(),
        None => doc.cam.tools.push(tool.clone()),
    }
    doc.cam.tools.sort_by_key(|t| t.number);
    Ok(json!({"tool": tool}))
}

fn setup_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.setup";
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    if bool_(p, "delete").unwrap_or(false) {
        let doc = s.doc_mut();
        let before = doc.cam.setups.len();
        doc.cam.setups.retain(|x| x.name != name);
        if doc.cam.setups.len() == before {
            return Err(bad(cmd, format!("no setup `{name}`")));
        }
        return Ok(json!({"deleted": name}));
    }
    let work_offset = num(p, "work_offset").map_or(1u8, |n| n as u8);
    let stock = match p.get("stock") {
        None => None,
        Some(v) => Some(stock_box(cmd, v)?),
    };
    let mut setup = CamSetup::new(&name);
    setup.work_offset = work_offset;
    setup.stock = stock;
    if let Some(existing) = s.doc.cam.setup(&name) {
        setup.operations = existing.operations.clone();
    }
    setup.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let doc = s.doc_mut();
    if doc.cam.setups.len() >= MAX_SETUPS && !doc.cam.setups.iter().any(|x| x.name == name) {
        return Err(bad(cmd, format!("a design has at most {MAX_SETUPS} setups")));
    }
    match doc.cam.setups.iter_mut().find(|x| x.name == name) {
        Some(slot) => {
            slot.work_offset = setup.work_offset;
            slot.stock = setup.stock;
        }
        None => doc.cam.setups.push(setup.clone()),
    }
    Ok(json!({"setup": setup_json(s.doc.cam.setup(&name).unwrap_or(&setup))}))
}

fn stock_box(cmd: &str, v: &Value) -> Result<StockBox> {
    let min = v.get("min").and_then(Value::as_array).filter(|a| a.len() == 3);
    let max = v.get("max").and_then(Value::as_array).filter(|a| a.len() == 3);
    let (Some(min), Some(max)) = (min, max) else {
        return Err(bad(cmd, "`stock` must be {min:[x,y,z], max:[x,y,z]}"));
    };
    let parse = |a: &[Value]| -> Result<[f64; 3]> {
        Ok([
            a[0].as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "stock coordinates must be finite"))?,
            a[1].as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "stock coordinates must be finite"))?,
            a[2].as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "stock coordinates must be finite"))?,
        ])
    };
    let box_ = StockBox { min: parse(min)?, max: parse(max)? };
    box_.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(box_)
}

fn contour_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    op_cmd(s, p, "cam.contour", true)
}

fn pocket_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    op_cmd(s, p, "cam.pocket", false)
}

fn op_cmd(s: &mut Session, p: &Value, cmd: &str, is_contour: bool) -> Result<Value> {
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let tool_n = num(p, "tool").filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`tool` must be a tool number"))?;
    let tool = s.doc.cam.tool(tool_n).cloned().ok_or_else(|| bad(cmd, format!("no tool {tool_n} (cam.tool first)")))?;
    let regions = regions_param(cmd, p)?;
    let cut = cut_param(cmd, p, &tool)?;
    let spindle_rpm = num(p, "spindle_rpm").or(tool.spindle_rpm);
    let coolant = bool_(p, "coolant").unwrap_or(false);
    let op = if is_contour {
        let mut params = default_contour_params();
        if let Some(side) = str_(p, "side") {
            params.side = parse_side(cmd, side)?;
        }
        if let Some(c) = bool_(p, "climb") {
            params.climb = c;
        }
        if let Some(v) = num(p, "stock_to_leave") {
            params.stock_to_leave = v;
        }
        if let Some(v) = num(p, "tolerance") {
            params.tolerance = v;
        }
        if let Some(deg) = num(p, "ramp_angle") {
            params.ramp_angle = deg.to_radians();
        }
        CamOperation::Contour { name: name.clone(), tool: tool_n, regions, params, cut, spindle_rpm, coolant }
    } else {
        let stepover = num(p, "stepover").or(tool.stepover).unwrap_or(tool.diameter * 0.5);
        let mut params = default_pocket_params(stepover);
        if let Some(c) = bool_(p, "climb") {
            params.climb = c;
        }
        if let Some(v) = num(p, "stock_to_leave") {
            params.stock_to_leave = v;
        }
        if let Some(v) = num(p, "tolerance") {
            params.tolerance = v;
        }
        if let Some(deg) = num(p, "ramp_angle") {
            params.ramp_angle = deg.to_radians();
        }
        CamOperation::Pocket { name: name.clone(), tool: tool_n, regions, params, cut, spindle_rpm, coolant }
    };
    op.validate().map_err(|e| bad(cmd, e.to_string()))?;
    // Prove the toolpath builds before saving the operation.
    build_path(&tool, &op).map_err(|e| bad(cmd, e))?;
    let doc = s.doc_mut();
    let setup = doc.cam.setup_mut(&setup_name).ok_or_else(|| bad(cmd, format!("no setup `{setup_name}` (cam.setup first)")))?;
    if let Some(slot) = setup.operations.iter_mut().find(|o| o.name() == name) {
        *slot = op;
    } else {
        if setup.operations.len() >= MAX_OPS {
            return Err(bad(cmd, format!("a setup has at most {MAX_OPS} operations")));
        }
        setup.operations.push(op);
    }
    Ok(json!({"setup": setup_name, "operation": name}))
}

fn parse_side(cmd: &str, side: &str) -> Result<Side> {
    match side {
        "outside" => Ok(Side::Outside),
        "inside" => Ok(Side::Inside),
        "on" => Ok(Side::On),
        other => Err(bad(cmd, format!("`side` must be outside, inside or on (not `{other}`)"))),
    }
}

fn regions_param(cmd: &str, p: &Value) -> Result<Vec<PolygonRegion>> {
    if let Some(arr) = p.get("regions").and_then(Value::as_array) {
        return arr.iter().map(|v| polygon_region(cmd, v)).collect();
    }
    let outer = vec2_list(cmd, p, "outer", solidcraft_doc::cam::MAX_LOOP_POINTS)?;
    if outer.len() < 3 {
        return Err(bad(cmd, "`outer` needs at least 3 points (or pass `regions`)"));
    }
    let holes = match p.get("holes") {
        None => Vec::new(),
        Some(Value::Array(hs)) => hs
            .iter()
            .map(|h| {
                let pts = h.as_array().ok_or_else(|| bad(cmd, "`holes` must be a list of point lists"))?;
                if pts.len() < 3 || pts.len() > solidcraft_doc::cam::MAX_LOOP_POINTS {
                    return Err(bad(cmd, "each hole needs 3 to 10000 points"));
                }
                pts.iter()
                    .map(|v| crate::params::vec2(v).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "hole points must be [x, y]")))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(bad(cmd, "`holes` must be a list of point lists")),
    };
    let region = PolygonRegion { outer: outer.into_iter().map(|p| [p.x, p.y]).collect(), holes };
    region.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(vec![region])
}

fn polygon_region(cmd: &str, v: &Value) -> Result<PolygonRegion> {
    let outer = v
        .get("outer")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(cmd, "each region needs `outer`: [[x,y],…]"))?
        .iter()
        .map(|p| crate::params::vec2(p).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "region points must be [x, y]")))
        .collect::<Result<Vec<_>>>()?;
    let holes = match v.get("holes") {
        None => Vec::new(),
        Some(Value::Array(hs)) => hs
            .iter()
            .map(|h| {
                h.as_array()
                    .ok_or_else(|| bad(cmd, "holes must be point lists"))?
                    .iter()
                    .map(|p| crate::params::vec2(p).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "hole points must be [x, y]")))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(bad(cmd, "`holes` must be a list of point lists")),
    };
    let region = PolygonRegion { outer, holes };
    region.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(region)
}

fn cut_param(cmd: &str, p: &Value, tool: &Tool) -> Result<Cut> {
    if let Some(raw) = p.get("cut") {
        return serde_json::from_value(raw.clone()).map_err(|e| bad(cmd, format!("`cut` is invalid: {e}")));
    }
    let bottom = num(p, "bottom").ok_or_else(|| bad(cmd, "`bottom` is required (final Z, mm)"))?;
    let top = num(p, "top").unwrap_or(0.0);
    let retract = num(p, "retract").unwrap_or(2.0);
    let clearance = num(p, "clearance").unwrap_or(10.0);
    let depth = (top - bottom).abs().max(1e-9);
    let stepdown = num(p, "stepdown").or(tool.stepdown).unwrap_or(depth);
    let feed = num(p, "feed").or(tool.feed).unwrap_or(600.0);
    let plunge = num(p, "plunge_feed").or(tool.plunge_feed).unwrap_or(200.0);
    let mut cut = default_cut(bottom, stepdown);
    cut.heights = Heights { clearance, retract, top, bottom };
    cut.feeds = Feeds { cut: feed, plunge };
    cut.heights.validate().map_err(|e| bad(cmd, e.to_string()))?;
    cut.feeds.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(cut)
}

fn delete_op(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.delete_op";
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let doc = s.doc_mut();
    let setup = doc.cam.setup_mut(&setup_name).ok_or_else(|| bad(cmd, format!("no setup `{setup_name}`")))?;
    let before = setup.operations.len();
    setup.operations.retain(|o| o.name() != name);
    if setup.operations.len() == before {
        return Err(bad(cmd, format!("no operation `{name}` in setup `{setup_name}`")));
    }
    Ok(json!({"deleted": name, "setup": setup_name}))
}

fn post_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.post";
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let setup = s.doc.cam.setup(&setup_name).cloned().ok_or_else(|| bad(cmd, format!("no setup `{setup_name}`")))?;
    if setup.operations.is_empty() {
        return Err(bad(cmd, format!("setup `{setup_name}` has no operations")));
    }
    let post_name = str_(p, "post").unwrap_or("rs274");
    let post = resolve_post(&s.doc.cam, post_name).map_err(|e| bad(cmd, e))?;
    post.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let program_name = str_(p, "program").unwrap_or(setup.name.as_str()).to_string();
    let mut program = Program { name: program_name, operations: Vec::new(), work_offset: Some(setup.work_offset) };
    for op in &setup.operations {
        let tool = s.doc.cam.tool(op.tool()).cloned().ok_or_else(|| bad(cmd, format!("operation `{}` needs tool {}", op.name(), op.tool())))?;
        let path = build_path(&tool, op).map_err(|e| bad(cmd, e))?;
        let (spindle_rpm, coolant) = match op {
            CamOperation::Contour { spindle_rpm, coolant, .. } | CamOperation::Pocket { spindle_rpm, coolant, .. } => {
                (spindle_rpm.or(tool.spindle_rpm).unwrap_or(6000.0), *coolant)
            }
        };
        program.operations.push(Operation { name: op.name().into(), tool, spindle_rpm, coolant, path });
    }
    let gcode = emit(&program, &post).map_err(|e| bad(cmd, e.to_string()))?;
    let mut out = json!({
        "gcode": gcode,
        "extension": post.extension,
        "post": post.name,
        "operations": program.operations.len(),
        "work_offset": setup.work_offset,
    });
    if bool_(p, "verify").unwrap_or(true) {
        let rb = readback(&gcode, &program, &post).map_err(|e| bad(cmd, e.to_string()))?;
        if !rb.within() {
            return Err(bad(cmd, format!("G-code read-back deviation {:.4} mm exceeds allowed {:.4} mm", rb.max_deviation, rb.allowed)));
        }
        out["readback"] = json!({
            "max_deviation": rb.max_deviation,
            "allowed": rb.allowed,
            "cut_length": rb.cut_length,
            "cut_minutes": rb.cut_minutes,
            "within": rb.within(),
        });
    }
    Ok(out)
}

fn resolve_post(cam: &CamSettings, name: &str) -> std::result::Result<Post, String> {
    if let Some(p) = cam.post(name) {
        return Ok(p.clone());
    }
    Post::builtin(name).ok_or_else(|| format!("unknown post `{name}` (built-in: {}; or save one on the design)", Post::BUILTIN.join(", ")))
}

fn build_path(tool: &Tool, op: &CamOperation) -> std::result::Result<solidcraft_cam::Toolpath, String> {
    let regions: Vec<_> = match op {
        CamOperation::Contour { regions, .. } | CamOperation::Pocket { regions, .. } => {
            regions.iter().map(|r| r.to_region().map_err(|e| e.to_string())).collect::<std::result::Result<Vec<_>, _>>()?
        }
    };
    match op {
        CamOperation::Contour { params, cut, .. } => contour(&regions, tool, params, cut).map_err(|e| e.to_string()),
        CamOperation::Pocket { params, cut, .. } => pocket(&regions, tool, params, cut).map_err(|e| e.to_string()),
    }
}

fn list_cmd(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({
        "tools": s.doc.cam.tools,
        "setups": s.doc.cam.setups.iter().map(setup_json).collect::<Vec<_>>(),
        "posts": s.doc.cam.posts.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        "builtin_posts": Post::BUILTIN,
    }))
}

fn setup_json(setup: &CamSetup) -> Value {
    json!({
        "name": setup.name,
        "work_offset": setup.work_offset,
        "stock": setup.stock,
        "operations": setup.operations.iter().map(|op| match op {
            CamOperation::Contour { name, tool, regions, params, cut, spindle_rpm, coolant } => json!({
                "type": "contour", "name": name, "tool": tool, "regions": regions.len(),
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
            CamOperation::Pocket { name, tool, regions, params, cut, spindle_rpm, coolant } => json!({
                "type": "pocket", "name": name, "tool": tool, "regions": regions.len(),
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
        }).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    #[test]
    fn contour_and_pocket_post_to_verified_gcode() {
        let mut s = Session::default();
        run(&mut s, "cam.tool", json!({"number": 1, "diameter": 6, "flute_length": 20, "spindle_rpm": 8000, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut s, "cam.setup", json!({"name": "Top", "work_offset": 1, "stock": {"min": [0, 0, -10], "max": [40, 30, 0]}}));
        run(
            &mut s,
            "cam.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "outer": [[0,0],[40,0],[40,30],[0,30]],
                "side": "outside", "bottom": -6, "stepdown": 3,
            }),
        );
        run(
            &mut s,
            "cam.pocket",
            json!({
                "setup": "Top", "name": "Pocket", "tool": 1,
                "outer": [[5,5],[35,5],[35,25],[5,25]],
                "stepover": 2.5, "bottom": -3, "stepdown": 3,
            }),
        );
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let gcode = posted["gcode"].as_str().unwrap();
        assert!(gcode.contains("G21"));
        assert!(gcode.contains("G54"));
        assert!(gcode.contains("M30"));
        assert!(posted["readback"]["within"].as_bool().unwrap());
        assert_eq!(posted["operations"], 2);

        // Round-trip through the design file.
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(cold.doc.cam.tools.len(), 1);
        assert_eq!(cold.doc.cam.setups[0].operations.len(), 2);
        assert!(cold.doc.format_pending.iter().any(|n| n == "cam-milling"));
        let again = run(&mut Session::new(cold.doc.as_ref().clone()), "cam.post", json!({"setup": "Top"}));
        assert!(again["readback"]["within"].as_bool().unwrap());
    }

    #[test]
    fn refuses_unknown_tool_and_empty_post() {
        let mut s = Session::default();
        assert!(s.execute("cam.contour", &json!({"setup": "A", "name": "C", "tool": 1, "outer": [[0,0],[10,0],[10,10]], "bottom": -1})).is_err());
        run(&mut s, "cam.tool", json!({"number": 1, "diameter": 6, "flute_length": 20}));
        run(&mut s, "cam.setup", json!({"name": "A"}));
        assert!(s.execute("cam.post", &json!({"setup": "A"})).is_err());
        assert!(s.execute("cam.delete_op", &json!({"setup": "A", "name": "missing"})).is_err());
    }

    #[test]
    fn list_and_delete() {
        let mut s = Session::default();
        run(&mut s, "cam.tool", json!({"number": 2, "diameter": 3, "flute_length": 12, "kind": "ball_endmill"}));
        run(&mut s, "cam.setup", json!({"name": "S"}));
        run(&mut s, "cam.contour", json!({"setup": "S", "name": "C", "tool": 2, "outer": [[0,0],[20,0],[20,10],[0,10]], "bottom": -2}));
        let list = run(&mut s, "cam.list", json!({}));
        assert_eq!(list["tools"].as_array().unwrap().len(), 1);
        assert_eq!(list["setups"][0]["operations"].as_array().unwrap().len(), 1);
        run(&mut s, "cam.delete_op", json!({"setup": "S", "name": "C"}));
        run(&mut s, "cam.setup", json!({"name": "S", "delete": true}));
        run(&mut s, "cam.tool", json!({"number": 2, "delete": true}));
        assert!(s.doc.cam.is_empty());
    }
}
