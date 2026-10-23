//! CAM commands: tools, setups, 2D contour/pocket operations, and G-code posting through
//! `solidcraft-cam`. Toolpaths are computed at post time and never stored on the design.
//! Geometry is a sketch profile or planar face (or explicit polylines); depths and feeds are
//! expressions evaluated when posting.

use serde_json::{Value, json};
use solidcraft_cam::{
    ContourParams, Cut, Feeds, Heights, Operation, PocketParams, Post, Program, Side, Tool, ToolKind, contour, emit, pocket, readback,
};
use solidcraft_doc::cam::{
    CamContourParams, CamCut, CamFaceRef, CamGeometry, CamOperation, CamPocketParams, CamSettings, CamSetup, CamSide, MAX_OPS, MAX_SETUPS, MAX_TOOLS,
    PolygonRegion, StockBox, default_contour_params, default_cut, default_pocket_params,
};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::project::resolve_face_indices;
use solidcraft_doc::{Expr, FeatureKind, select_profiles};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_sketch::{LinkGeom, LinkKind};

use super::CommandSpec;
use crate::params::{bad, bool_, expr, num, req_expr, str_, vec2, vec2_list, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cam.tool.define", "Define Tool", tool_cmd).icon("hole").params(
        "number (1…9999); kind?: flat_endmill|ball_endmill|drill (default flat_endmill); diameter (mm); flute_length (mm); \
         description?; flutes?; spindle_rpm?; feed?; plunge_feed?; stepdown?; stepover?",
    ),
    CommandSpec::new("cam.tool.remove", "Remove Tool", tool_remove).icon("delete").params("number"),
    CommandSpec::new("cam.setup", "CAM Setup", setup_cmd).icon("settings").params(
        "name; work_offset?: 1…6 (G54…G59, default 1); tool_axis?: [x,y,z] (programmed +Z, default [0,0,1]); \
         x_axis?: [x,y,z] (programmed +X, square to the tool axis; omitted, derived); \
         work_origin?: [x,y,z] mm part zero in the model (default [0,0,0]); \
         stock?: {min:[x,y,z], max:[x,y,z]}; delete?: bool",
    ),
    CommandSpec::new("cam.op.contour", "2D Contour", contour_cmd).icon("cut").params(
        "setup; name; tool (number); sketch?: id or name, with profiles?: as solid.extrude; faces?: [{body, name, at?}]; \
         or outer: [[x,y],…] (or regions: [{outer, holes?}]); holes?: [[[x,y],…]]; \
         side?: outside|inside|on (default outside); climb?: bool (default true); stock_to_leave?; tolerance?; \
         ramp_angle?: deg (default 0); clearance?; retract?; top?; bottom (required unless cut given); stepdown?; \
         feed?; plunge_feed?; spindle_rpm?; coolant?",
    ),
    CommandSpec::new("cam.op.pocket", "2D Pocket", pocket_cmd).icon("hole").params(
        "setup; name; tool (number); sketch?: id or name, with profiles?; faces?; or outer/regions; holes?; \
         stepover? (default half the tool diameter); climb?; stock_to_leave?; tolerance?; ramp_angle?: deg; \
         clearance?; retract?; top?; bottom; stepdown?; feed?; plunge_feed?; spindle_rpm?; coolant?",
    ),
    CommandSpec::new("cam.op.remove", "Remove CAM Operation", delete_op).icon("delete").params("setup; name"),
    CommandSpec::new("cam.post", "Post Process", post_cmd).icon("export").noundo().params(
        "setup; post?: rs274 | name of a saved post (default rs274); program?: name (default the setup's) → \
         {gcode, extension, post, operations, readback}. G-code is always read back and checked against the toolpaths.",
    ),
    CommandSpec::new("cam.list", "List CAM", list_cmd).icon("list").noundo().params("→ tools, setups (with operations) and saved posts"),
];

fn tool_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool.define";
    let number = num(p, "number")
        .filter(|n| *n >= 1.0 && *n <= 9999.0 && n.fract() == 0.0)
        .map(|n| n as u32)
        .ok_or_else(|| bad(cmd, "`number` must be an integer 1…9999"))?;
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

fn tool_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.tool.remove";
    let number = num(p, "number")
        .filter(|n| *n >= 1.0 && *n <= 9999.0 && n.fract() == 0.0)
        .map(|n| n as u32)
        .ok_or_else(|| bad(cmd, "`number` must be an integer 1…9999"))?;
    let doc = s.doc_mut();
    if doc.cam.setups.iter().any(|setup| setup.operations.iter().any(|op| op.tool() == number)) {
        return Err(bad(cmd, format!("tool {number} is used by an operation")));
    }
    let before = doc.cam.tools.len();
    doc.cam.tools.retain(|t| t.number != number);
    if doc.cam.tools.len() == before {
        return Err(bad(cmd, format!("no tool {number}")));
    }
    Ok(json!({"deleted": number}))
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
    let existing = s.doc.cam.setup(&name).cloned();
    let work_offset = num(p, "work_offset").map_or(existing.as_ref().map(|e| e.work_offset).unwrap_or(1), |n| n as u8);
    let tool_axis = match p.get("tool_axis") {
        Some(v) => {
            let a = vec3(v).ok_or_else(|| bad(cmd, "`tool_axis` must be [x, y, z]"))?;
            [a.x, a.y, a.z]
        }
        None => existing.as_ref().map(|e| e.tool_axis).unwrap_or([0.0, 0.0, 1.0]),
    };
    let work_origin = match p.get("work_origin") {
        Some(v) => {
            let a = vec3(v).ok_or_else(|| bad(cmd, "`work_origin` must be [x, y, z]"))?;
            [a.x, a.y, a.z]
        }
        None => existing.as_ref().map(|e| e.work_origin).unwrap_or([0.0, 0.0, 0.0]),
    };
    let x_axis = match p.get("x_axis") {
        Some(v) => {
            let a = vec3(v).ok_or_else(|| bad(cmd, "`x_axis` must be [x, y, z]"))?;
            Some([a.x, a.y, a.z])
        }
        None => existing.as_ref().and_then(|e| e.x_axis),
    };
    let stock = match p.get("stock") {
        None => existing.as_ref().and_then(|e| e.stock),
        Some(v) => Some(stock_box(cmd, v)?),
    };
    let mut setup = CamSetup::new(&name);
    setup.work_offset = work_offset;
    setup.tool_axis = tool_axis;
    setup.x_axis = x_axis;
    setup.work_origin = work_origin;
    setup.stock = stock;
    if let Some(existing) = &existing {
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
            slot.tool_axis = setup.tool_axis;
            slot.x_axis = setup.x_axis;
            slot.work_origin = setup.work_origin;
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
    op_cmd(s, p, "cam.op.contour", true)
}

fn pocket_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    op_cmd(s, p, "cam.op.pocket", false)
}

fn op_cmd(s: &mut Session, p: &Value, cmd: &str, is_contour: bool) -> Result<Value> {
    let setup_name = str_(p, "setup").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`setup` is required"))?.to_string();
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let tool_n =
        num(p, "tool").filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u32).ok_or_else(|| bad(cmd, "`tool` must be a tool number"))?;
    let tool = s.doc.cam.tool(tool_n).cloned().ok_or_else(|| bad(cmd, format!("no tool {tool_n} (cam.tool.define first)")))?;
    let geometry = geometry_param(s, cmd, p)?;
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
        if let Some(v) = expr(p, "stock_to_leave") {
            params.stock_to_leave = v;
        }
        if let Some(v) = expr(p, "tolerance") {
            params.tolerance = v;
        }
        if let Some(v) = expr(p, "ramp_angle") {
            params.ramp_angle = v;
        }
        CamOperation::Contour { name: name.clone(), tool: tool_n, geometry, params, cut, spindle_rpm, coolant }
    } else {
        let stepover = match expr(p, "stepover") {
            Some(e) => e,
            None => match tool.stepover {
                Some(v) => Expr::new(format!("{v} mm")),
                None => Expr::new(format!("{} mm", tool.diameter * 0.5)),
            },
        };
        let mut params = default_pocket_params(stepover);
        if let Some(c) = bool_(p, "climb") {
            params.climb = c;
        }
        if let Some(v) = expr(p, "stock_to_leave") {
            params.stock_to_leave = v;
        }
        if let Some(v) = expr(p, "tolerance") {
            params.tolerance = v;
        }
        if let Some(v) = expr(p, "ramp_angle") {
            params.ramp_angle = v;
        }
        CamOperation::Pocket { name: name.clone(), tool: tool_n, geometry, params, cut, spindle_rpm, coolant }
    };
    op.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let setup = s.doc.cam.setup(&setup_name).cloned().ok_or_else(|| bad(cmd, format!("no setup `{setup_name}` (cam.setup first)")))?;
    build_path(s, &setup, &op).map_err(|e| bad(cmd, e))?;
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

fn parse_side(cmd: &str, side: &str) -> Result<CamSide> {
    match side {
        "outside" => Ok(CamSide::Outside),
        "inside" => Ok(CamSide::Inside),
        "on" => Ok(CamSide::On),
        other => Err(bad(cmd, format!("`side` must be outside, inside or on (not `{other}`)"))),
    }
}

fn geometry_param(s: &Session, cmd: &str, p: &Value) -> Result<CamGeometry> {
    if p.get("sketch").is_some() {
        let key = super::sketch_key(p.get("sketch")).ok_or_else(|| bad(cmd, "`sketch` must be a sketch id or name"))?;
        let sketch = match s.doc.find_feature(&key) {
            Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => f.id,
            _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
        };
        let profiles = super::feature_profiles(p, cmd)?;
        return Ok(CamGeometry::Sketch { sketch, profiles });
    }
    if let Some(arr) = p.get("faces").and_then(Value::as_array) {
        if arr.is_empty() {
            return Err(bad(cmd, "`faces` is empty"));
        }
        let mut faces = Vec::new();
        for v in arr {
            let body = v.get("body").and_then(Value::as_str).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "each face needs `body`"))?;
            let name = v.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "each face needs `name`"))?;
            let at = match v.get("at") {
                None => [0.0, 0.0, 0.0],
                Some(a) => {
                    let p = vec3(a).ok_or_else(|| bad(cmd, "`at` must be [x, y, z]"))?;
                    [p.x, p.y, p.z]
                }
            };
            faces.push(CamFaceRef { body: body.to_string(), name: name.to_string(), at });
        }
        return Ok(CamGeometry::Faces { faces });
    }
    Ok(CamGeometry::Polygons { regions: regions_param(cmd, p)? })
}

fn regions_param(cmd: &str, p: &Value) -> Result<Vec<PolygonRegion>> {
    if let Some(arr) = p.get("regions").and_then(Value::as_array) {
        return arr.iter().map(|v| polygon_region(cmd, v)).collect();
    }
    let outer = vec2_list(cmd, p, "outer", solidcraft_doc::cam::MAX_LOOP_POINTS)?;
    if outer.len() < 3 {
        return Err(bad(cmd, "`outer` needs at least 3 points (or pass `sketch`, `faces`, or `regions`)"));
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
                pts.iter().map(|v| vec2(v).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "hole points must be [x, y]"))).collect::<Result<Vec<_>>>()
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
        .map(|p| vec2(p).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "region points must be [x, y]")))
        .collect::<Result<Vec<_>>>()?;
    let holes = match v.get("holes") {
        None => Vec::new(),
        Some(Value::Array(hs)) => hs
            .iter()
            .map(|h| {
                h.as_array()
                    .ok_or_else(|| bad(cmd, "holes must be point lists"))?
                    .iter()
                    .map(|p| vec2(p).map(|p| [p.x, p.y]).ok_or_else(|| bad(cmd, "hole points must be [x, y]")))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(bad(cmd, "`holes` must be a list of point lists")),
    };
    let region = PolygonRegion { outer, holes };
    region.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(region)
}

fn cut_param(cmd: &str, p: &Value, tool: &Tool) -> Result<CamCut> {
    let src = p.get("cut").unwrap_or(p);
    let bottom = req_expr(cmd, src, "bottom")?;
    let top = expr(src, "top").unwrap_or_else(|| Expr::new("0 mm"));
    let retract = expr(src, "retract").unwrap_or_else(|| Expr::new("2 mm"));
    let clearance = expr(src, "clearance").unwrap_or_else(|| Expr::new("10 mm"));
    let stepdown = match expr(src, "stepdown") {
        Some(e) => e,
        None => match tool.stepdown {
            Some(v) => Expr::new(format!("{v} mm")),
            None => Expr::new(format!("{} mm", tool.diameter * 0.5)),
        },
    };
    let feed = match expr(src, "feed") {
        Some(e) => e,
        None => match tool.feed {
            Some(v) => Expr::new(format!("{v} mm")),
            None => Expr::new("600 mm"),
        },
    };
    let plunge_feed = match expr(src, "plunge_feed") {
        Some(e) => e,
        None => match tool.plunge_feed {
            Some(v) => Expr::new(format!("{v} mm")),
            None => Expr::new("200 mm"),
        },
    };
    let mut cut = default_cut(bottom, stepdown);
    cut.top = top;
    cut.retract = retract;
    cut.clearance = clearance;
    cut.feed = feed;
    cut.plunge_feed = plunge_feed;
    Ok(cut)
}

fn delete_op(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.op.remove";
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
        let path = build_path(s, &setup, op).map_err(|e| bad(cmd, e))?;
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
    Ok(out)
}

fn resolve_post(cam: &CamSettings, name: &str) -> std::result::Result<Post, String> {
    if let Some(p) = cam.post(name) {
        return Ok(p.clone());
    }
    Post::builtin(name).ok_or_else(|| format!("unknown post `{name}` (built-in: {}; or save one on the design)", Post::BUILTIN.join(", ")))
}

struct WorkFrame {
    o: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl WorkFrame {
    fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.o;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
}

/// Work X and Y for a tool axis. Along a world axis they are world axes too.
fn axes(axis: Vec3) -> Option<(Vec3, Vec3, Vec3)> {
    let z = axis.normalized().filter(|z| z.is_finite())?;
    let x = if z.z.abs() > 1.0 - 1e-9 || z.y.abs() > 1.0 - 1e-9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else if z.x.abs() > 1.0 - 1e-9 {
        Vec3::new(0.0, z.x.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0).cross(z).normalized()?
    };
    Some((x, z.cross(x), z))
}

fn work_frame(setup: &CamSetup) -> std::result::Result<WorkFrame, String> {
    let z = setup.tool_axis_vec().normalized().filter(|z| z.is_finite()).ok_or("the setup's tool axis is not a direction")?;
    let x = match setup.x_axis {
        Some(a) => {
            let x = Vec3::new(a[0], a[1], a[2]).normalized().filter(|x| x.is_finite()).ok_or("the setup's X axis is not a direction")?;
            if x.dot(z).abs() > 1e-6 {
                return Err("the setup's X axis must be square to the tool axis".into());
            }
            x
        }
        None => axes(z).ok_or("the setup's tool axis is not a direction")?.0,
    };
    let y = z.cross(x);
    let y = y.normalized().filter(|y| y.is_finite()).ok_or("the setup's axes do not form a frame")?;
    Ok(WorkFrame { o: setup.work_origin_vec(), x, y, z })
}

/// Sketch-plane regions in a setup's XY. The sketch plane must face along the tool axis.
fn to_setup(plane: &Plane, frame: &WorkFrame, regions: &[Region2]) -> std::result::Result<Vec<Region2>, String> {
    let facing = plane.normal().dot(frame.z);
    if !(facing.abs() > 1.0 - 1e-9) {
        return Err("not supported yet: operations on a sketch or face that is not square to the setup's tool axis".into());
    }
    let mirrored = facing < 0.0;
    let x_axis = plane.dir_to_world(Vec2::new(1.0, 0.0));
    let turn = x_axis.dot(frame.y).atan2(x_axis.dot(frame.x));
    let point = |p: Vec2| {
        let w = frame.local(plane.to_world(p));
        Vec2::new(w.x, w.y)
    };
    let seg = |s: &Seg2| match *s {
        Seg2::Line { a, b } => Seg2::Line { a: point(a), b: point(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc {
            center: point(center),
            radius,
            start: if mirrored { turn - start } else { turn + start },
            sweep: if mirrored { -sweep } else { sweep },
        },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: point(p0), p1: point(p1), p2: point(p2), p3: point(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: point(a), apex: point(apex), b: point(b), w },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(point) },
    };
    let lp = |l: &Loop2| Loop2 { segs: l.segs.iter().map(seg).collect() };
    Ok(regions.iter().map(|r| Region2 { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect() }).collect())
}

fn outline(geom: &[LinkGeom]) -> std::result::Result<Region2, String> {
    let mut loops: Vec<Loop2> = Vec::new();
    let mut open: Vec<Seg2> = Vec::new();
    for g in geom {
        match *g {
            LinkGeom::Point(_) => {}
            LinkGeom::Line(a, b) => open.push(Seg2::Line { a, b }),
            LinkGeom::Circle(c, r) => loops.push(Loop2::circle(c, r)),
            LinkGeom::Arc { c, a, b } => {
                let (start, end) = ((a - c).angle(), (b - c).angle());
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                open.push(Seg2::Arc { center: c, radius: a.dist(c), start, sweep: if sweep > 1e-12 { sweep } else { std::f64::consts::TAU } });
            }
            LinkGeom::Conic { a, apex, b, rho } if rho > 1e-6 && rho < 1.0 - 1e-6 => open.push(Seg2::Conic { a, apex, b, w: rho / (1.0 - rho) }),
            _ => return Err("not supported yet: faces with elliptical or spline edges".into()),
        }
    }
    if open.len() > 20_000 {
        return Err("the face has too many edges".into());
    }
    const JOIN: f64 = 1e-3;
    let mut used = vec![false; open.len()];
    for first in 0..open.len() {
        if used.get(first).copied().unwrap_or(true) {
            continue;
        }
        let Some(seed) = open.get(first).copied() else { continue };
        if let Some(flag) = used.get_mut(first) {
            *flag = true;
        }
        let mut chain = vec![seed];
        let origin = seed.start();
        let mut end = seed.end();
        while end.dist(origin) > JOIN {
            let next = open.iter().zip(&used).enumerate().filter(|(_, (_, taken))| !**taken).find_map(|(i, (seg, _))| {
                if seg.start().dist(end) <= JOIN {
                    Some((i, *seg))
                } else if seg.end().dist(end) <= JOIN {
                    Some((i, seg.reversed()))
                } else {
                    None
                }
            });
            let Some((i, seg)) = next else { return Err("the face's edges do not form closed loops".into()) };
            if let Some(flag) = used.get_mut(i) {
                *flag = true;
            }
            end = seg.end();
            chain.push(seg);
        }
        loops.push(Loop2 { segs: chain });
    }
    let outer = loops
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.signed_area().abs().total_cmp(&b.1.signed_area().abs()))
        .map(|(i, _)| i)
        .ok_or("the face has no outline")?;
    let outer = loops.remove(outer);
    Ok(Region2 { outer, holes: loops })
}

fn resolve_geometry(s: &Session, setup: &CamSetup, source: &CamGeometry) -> std::result::Result<Vec<Region2>, String> {
    let frame = work_frame(setup)?;
    let state = s.model.state();
    match source {
        CamGeometry::Polygons { regions } => regions.iter().map(|r| r.to_region().map_err(|e| e.to_string())).collect(),
        CamGeometry::Sketch { sketch, profiles } => {
            let sketch = state.sketch(*sketch).ok_or("its sketch is not built (deleted, rolled back, suppressed or failed)")?;
            let regions = select_profiles(sketch, profiles).map_err(|e| e.to_string())?;
            to_setup(&sketch.plane, &frame, &regions)
        }
        CamGeometry::Faces { faces } => {
            let mut regions = Vec::new();
            for face in faces {
                let body = state.body(&face.body).ok_or_else(|| format!("body `{}` is gone", face.body))?;
                let (index, ..) = solidcraft_doc::naming::face_on_body(body, &face.name, face.at_vec())
                    .ok_or_else(|| format!("its face on `{}` is gone", face.body))?;
                let plane = body.body.planar_face(index).ok_or("a picked face is not planar")?;
                let resolved = resolve_face_indices(&state, &plane, LinkKind::Project, &face.body, &[index]).map_err(|e| e.to_string())?;
                let region = outline(&resolved.geom)?;
                regions.extend(to_setup(&plane, &frame, &[region])?);
            }
            Ok(regions)
        }
    }
}

fn eval_len(s: &Session, e: &Expr, key: &str) -> std::result::Result<f64, String> {
    s.doc.eval(e.as_str(), Kind::Length).map_err(|err| format!("{key}: {err}"))
}

fn eval_ang(s: &Session, e: &Expr, key: &str) -> std::result::Result<f64, String> {
    s.doc.eval(e.as_str(), Kind::Angle).map_err(|err| format!("{key}: {err}"))
}

fn engine_cut(s: &Session, cut: &CamCut) -> std::result::Result<Cut, String> {
    let heights = Heights {
        clearance: eval_len(s, &cut.clearance, "clearance")?,
        retract: eval_len(s, &cut.retract, "retract")?,
        top: eval_len(s, &cut.top, "top")?,
        bottom: eval_len(s, &cut.bottom, "bottom")?,
    };
    let feeds = Feeds { cut: eval_len(s, &cut.feed, "feed")?, plunge: eval_len(s, &cut.plunge_feed, "plunge_feed")? };
    let stepdown = eval_len(s, &cut.stepdown, "stepdown")?;
    heights.validate().map_err(|e| e.to_string())?;
    feeds.validate().map_err(|e| e.to_string())?;
    if !(stepdown > 0.0) || !stepdown.is_finite() {
        return Err("stepdown must be above 0".into());
    }
    Ok(Cut { heights, feeds, stepdown })
}

fn engine_contour(s: &Session, p: &CamContourParams) -> std::result::Result<ContourParams, String> {
    Ok(ContourParams {
        side: match p.side {
            CamSide::Outside => Side::Outside,
            CamSide::Inside => Side::Inside,
            CamSide::On => Side::On,
        },
        climb: p.climb,
        stock_to_leave: eval_len(s, &p.stock_to_leave, "stock_to_leave")?,
        tolerance: eval_len(s, &p.tolerance, "tolerance")?,
        ramp_angle: eval_ang(s, &p.ramp_angle, "ramp_angle")?,
    })
}

fn engine_pocket(s: &Session, p: &CamPocketParams) -> std::result::Result<PocketParams, String> {
    Ok(PocketParams {
        stepover: eval_len(s, &p.stepover, "stepover")?,
        climb: p.climb,
        stock_to_leave: eval_len(s, &p.stock_to_leave, "stock_to_leave")?,
        tolerance: eval_len(s, &p.tolerance, "tolerance")?,
        ramp_angle: eval_ang(s, &p.ramp_angle, "ramp_angle")?,
    })
}

fn build_path(s: &Session, setup: &CamSetup, op: &CamOperation) -> std::result::Result<solidcraft_cam::Toolpath, String> {
    let regions = resolve_geometry(s, setup, op.geometry())?;
    let cut = engine_cut(s, op.cut())?;
    let tool = s.doc.cam.tool(op.tool()).ok_or_else(|| format!("there is no tool {}", op.tool()))?;
    match op {
        CamOperation::Contour { params, .. } => {
            let params = engine_contour(s, params)?;
            contour(&regions, tool, &params, &cut).map_err(|e| e.to_string())
        }
        CamOperation::Pocket { params, .. } => {
            let params = engine_pocket(s, params)?;
            pocket(&regions, tool, &params, &cut).map_err(|e| e.to_string())
        }
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
        "tool_axis": setup.tool_axis,
        "x_axis": setup.x_axis,
        "work_origin": setup.work_origin,
        "stock": setup.stock,
        "operations": setup.operations.iter().map(|op| match op {
            CamOperation::Contour { name, tool, geometry, params, cut, spindle_rpm, coolant } => json!({
                "type": "contour", "name": name, "tool": tool, "geometry": geometry,
                "params": params, "cut": cut, "spindle_rpm": spindle_rpm, "coolant": coolant,
            }),
            CamOperation::Pocket { name, tool, geometry, params, cut, spindle_rpm, coolant } => json!({
                "type": "pocket", "name": name, "tool": tool, "geometry": geometry,
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
        run(
            &mut s,
            "cam.tool.define",
            json!({"number": 1, "diameter": 6, "flute_length": 20, "spindle_rpm": 8000, "feed": 600, "plunge_feed": 200, "stepdown": 3}),
        );
        run(&mut s, "cam.setup", json!({"name": "Top", "work_offset": 1, "stock": {"min": [0, 0, -10], "max": [40, 30, 0]}}));
        run(
            &mut s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "outer": [[0,0],[40,0],[40,30],[0,30]],
                "side": "outside", "bottom": -6, "stepdown": 3,
            }),
        );
        run(
            &mut s,
            "cam.op.pocket",
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

        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(cold.doc.cam.tools.len(), 1);
        assert_eq!(cold.doc.cam.setups[0].operations.len(), 2);
        assert!(cold.doc.format_pending.iter().any(|n| n == "cam-milling"));
        let again = run(&mut Session::new(cold.doc.as_ref().clone()), "cam.post", json!({"setup": "Top"}));
        assert!(again["readback"]["within"].as_bool().unwrap());

        let forced = run(&mut Session::new(s.doc.as_ref().clone()), "cam.post", json!({"setup": "Top", "verify": false}));
        assert!(forced["readback"]["within"].as_bool().unwrap());
    }

    #[test]
    fn setup_keeps_room_for_tool_axis_and_work_origin() {
        let mut s = Session::default();
        run(&mut s, "cam.setup", json!({"name": "Side", "tool_axis": [1, 0, 0], "x_axis": [0, 1, 0], "work_origin": [10, 20, 30]}));
        assert_eq!(s.doc.cam.setups[0].tool_axis, [1.0, 0.0, 0.0]);
        assert_eq!(s.doc.cam.setups[0].x_axis, Some([0.0, 1.0, 0.0]));
        assert_eq!(s.doc.cam.setups[0].work_origin, [10.0, 20.0, 30.0]);
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(cold.doc.cam.setups[0].tool_axis, [1.0, 0.0, 0.0]);
        assert_eq!(cold.doc.cam.setups[0].x_axis, Some([0.0, 1.0, 0.0]));
        assert_eq!(cold.doc.cam.setups[0].work_origin, [10.0, 20.0, 30.0]);
        assert!(s.execute("cam.setup", &json!({"name": "Bad", "tool_axis": [0, 0, 0]})).is_err());
        assert!(s.execute("cam.setup", &json!({"name": "Skew", "x_axis": [0, 0, 1]})).is_err());
    }

    fn gcode_x_words(gcode: &str) -> Vec<i64> {
        gcode
            .split_whitespace()
            .filter_map(|w| w.strip_prefix('X'))
            .filter_map(|n| n.parse::<f64>().ok())
            .map(|x| (x * 100.0).round() as i64)
            .collect()
    }

    fn sketch_outside_gcode(s: &mut Session, setup: Value) -> String {
        run(s, "sketch.create", json!({"plane": "XY"}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        let sketch = s.active_sketch.expect("sketch");
        run(s, "sketch.finish", json!({}));
        run(s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(s, "cam.setup", setup);
        run(
            s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "sketch": sketch, "side": "outside", "bottom": -3, "stepdown": 3,
            }),
        );
        run(s, "cam.post", json!({"setup": "Top"}))["gcode"].as_str().unwrap().to_string()
    }

    #[test]
    fn work_origin_shifts_sketch_xy_and_offset_writes_g55() {
        let g0 = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top"}));
        let g10 = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top", "work_origin": [10, 0, 0]}));
        let g55 = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top", "work_offset": 2}));
        assert!(g0.contains("G54"), "{g0}");
        assert!(g55.contains("G55"), "{g55}");
        assert!(!g55.contains("G54"), "{g55}");
        let x0 = gcode_x_words(&g0);
        let x10 = gcode_x_words(&g10);
        assert!(!x0.is_empty(), "{g0}");
        assert_ne!(x0, x10, "origin must move posted X\n{g0}\n{g10}");
        let shifted: Vec<i64> = x0.iter().map(|x| x - 1000).collect();
        assert_eq!(shifted, x10, "X(origin 10) should be X(origin 0) minus 10 mm\n{g0}\n{g10}");
    }

    #[test]
    fn x_axis_rotates_work_xy() {
        let g = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top"}));
        let r = sketch_outside_gcode(&mut Session::default(), json!({"name": "Top", "x_axis": [0, 1, 0]}));
        assert_ne!(gcode_x_words(&g), gcode_x_words(&r), "a work X along model Y must change posted X\n{g}\n{r}");
        assert!(r.contains("G54"), "{r}");
    }

    #[test]
    fn refuses_unknown_tool_and_empty_post() {
        let mut s = Session::default();
        assert!(s.execute("cam.op.contour", &json!({"setup": "A", "name": "C", "tool": 1, "outer": [[0,0],[10,0],[10,10]], "bottom": -1})).is_err());
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20}));
        run(&mut s, "cam.setup", json!({"name": "A"}));
        assert!(s.execute("cam.post", &json!({"setup": "A"})).is_err());
        assert!(s.execute("cam.op.remove", &json!({"setup": "A", "name": "missing"})).is_err());
    }

    #[test]
    fn list_and_delete() {
        let mut s = Session::default();
        run(&mut s, "cam.tool.define", json!({"number": 2, "diameter": 3, "flute_length": 12, "kind": "ball_endmill"}));
        run(&mut s, "cam.setup", json!({"name": "S"}));
        run(&mut s, "cam.op.contour", json!({"setup": "S", "name": "C", "tool": 2, "outer": [[0,0],[20,0],[20,10],[0,10]], "bottom": -2}));
        let list = run(&mut s, "cam.list", json!({}));
        assert_eq!(list["tools"].as_array().unwrap().len(), 1);
        assert_eq!(list["setups"][0]["operations"].as_array().unwrap().len(), 1);
        run(&mut s, "cam.op.remove", json!({"setup": "S", "name": "C"}));
        run(&mut s, "cam.setup", json!({"name": "S", "delete": true}));
        run(&mut s, "cam.tool.remove", json!({"number": 2}));
        assert!(s.doc.cam.is_empty());
    }

    #[test]
    fn sketch_profile_follows_the_design_and_exprs_eval() {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "depth", "expression": "6 mm"}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        let sketch = s.active_sketch.expect("sketch");
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 6, "flute_length": 20, "feed": 600, "plunge_feed": 200, "stepdown": 3}));
        run(&mut s, "cam.setup", json!({"name": "Top"}));
        run(
            &mut s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Outside", "tool": 1,
                "sketch": sketch, "side": "outside", "bottom": "-depth", "stepdown": 3,
            }),
        );
        match &s.doc.cam.setups[0].operations[0] {
            CamOperation::Contour { geometry: CamGeometry::Sketch { .. }, cut, .. } => {
                assert_eq!(cut.bottom.as_str(), "-depth");
            }
            other => panic!("{other:?}"),
        }
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let gcode = posted["gcode"].as_str().unwrap();
        assert!(gcode.contains("Z-6"), "{gcode}");
        assert!(posted["readback"]["within"].as_bool().unwrap());

        run(&mut s, "parameters.change", json!({"name": "depth", "expression": "8 mm"}));
        let again = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let g2 = again["gcode"].as_str().unwrap();
        assert!(g2.contains("Z-8"), "{g2}");
        assert!(!g2.contains("Z-6"), "{g2}");
    }

    #[test]
    fn circle_sketch_posts_g2_or_g3() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
        let sketch = s.active_sketch.expect("sketch");
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "cam.tool.define", json!({"number": 1, "diameter": 4, "flute_length": 16, "feed": 400, "plunge_feed": 100, "stepdown": 2}));
        run(&mut s, "cam.setup", json!({"name": "Top"}));
        run(
            &mut s,
            "cam.op.contour",
            json!({
                "setup": "Top", "name": "Boss", "tool": 1,
                "sketch": sketch, "side": "outside", "bottom": -2, "stepdown": 2,
            }),
        );
        let posted = run(&mut s, "cam.post", json!({"setup": "Top"}));
        let gcode = posted["gcode"].as_str().unwrap();
        assert!(gcode.contains("G2 ") || gcode.contains("G3 "), "{gcode}");
        assert!(posted["readback"]["within"].as_bool().unwrap());
    }
}
